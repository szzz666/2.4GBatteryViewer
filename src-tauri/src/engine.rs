//! 轮询引擎：后台线程按设定间隔刷新所有候选设备的电量，并把快照广播给前端。
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Condvar;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::model::{AppSettings, BatteryState, DeviceSnapshot, Reading};
use crate::proto;
use crate::scan;

pub const EVENT_SNAPSHOT: &str = "battery://snapshot";
pub const EVENT_SCAN: &str = "battery://scan";
pub const EVENT_SETTINGS: &str = "settings://changed";

#[derive(Default)]
pub struct Shared {
    pub devices: RwLock<Vec<DeviceSnapshot>>,
    pub settings: RwLock<AppSettings>,
    pub last_scan: Mutex<Option<Instant>>,
    pub scanning: AtomicBool,
    pub refresh_seq: AtomicU64,
    /// 设备 key -> 上次成功的协议 id（省去每轮重新试探）
    pub protocol_cache: RwLock<HashMap<String, String>>,
    /// 设备 key -> 该时间点之前不再重试（没有任何协议可用时退回低频探测）
    pub give_up_until: RwLock<HashMap<String, Instant>>,
    /// 设备 key -> 最近一次失败原因（暂停探测后仍能显示）
    pub last_error: RwLock<HashMap<String, String>>,
    /// 低电量提醒冷却：设备 key -> 上次提醒时间
    pub low_notified: RwLock<HashMap<String, Instant>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotEvent {
    pub devices: Vec<DeviceSnapshot>,
    pub scanning: bool,
    pub refreshed_at: i64,
    pub refresh_ms: u64,
}

pub struct Engine {
    pub shared: Arc<Shared>,
    /// 设置变化时立即唤醒轮询线程（改刷新间隔无需等下一轮）
    pub wake: Arc<(std::sync::Mutex<bool>, Condvar)>,
    pub wake_wanted: AtomicBool,
}

/// 每次操作独立创建一个 hidapi 实例。
///
/// 以前是一个全局 Mutex<HidApi>：轮询线程读一轮要几秒，期间所有调试命令
/// 都被堵在锁上，表现就是“打开调试台就卡死”。hidapi 实例本身不持有设备
/// 句柄，随时新建的代价可以忽略，换成按需创建后两边互不阻塞。
pub fn new_api() -> hidapi::HidApi {
    hidapi::HidApi::new().expect("初始化 hidapi 失败")
}

impl Engine {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            shared: Arc::new(Shared::default()),
            wake: Arc::new((std::sync::Mutex::new(false), Condvar::new())),
            wake_wanted: AtomicBool::new(false),
        })
    }

    /// 唤醒轮询线程，让它按新设置重新计时
    pub fn wake_now(&self) {
        self.wake_wanted.store(true, Ordering::SeqCst);
        let (lock, cv) = &*self.wake;
        let mut guard = lock.lock().unwrap_or_else(|x| x.into_inner());
        *guard = true;
        cv.notify_all();
        drop(guard);
    }

    /// 全量扫描并建立设备列表。
    ///
    /// clear_cache 只在用户手动“重新扫描”时为 true：定时重扫会保留协议缓存，
    /// 否则每分钟都会把所有协议重新试探一遍（几十秒的 CPU / HID 空转）。
    pub fn rescan(&self, app: &AppHandle, clear_cache: bool) {
        if self.shared.scanning.swap(true, Ordering::SeqCst) {
            return;
        }
        let settings = self.shared.settings.read().clone();
        let devices = scan::scan_all();
        let mut snapshots = Vec::new();
        for dev in devices {
            if !scan::is_battery_candidate(&dev) && settings.overrides.get(&dev.key).is_none() {
                continue;
            }
            snapshots.push(DeviceSnapshot {
                device: dev,
                reading: None,
                hidden: false,
            });
        }
        // 保留已有读数，避免刷新时闪烁
        let old: HashMap<String, Option<Reading>> = self
            .shared
            .devices
            .read()
            .iter()
            .map(|s| (s.device.key.clone(), s.reading.clone()))
            .collect();
        for s in &mut snapshots {
            if let Some(r) = old.get(&s.device.key) {
                s.reading = r.clone();
            }
        }
        *self.shared.devices.write() = snapshots;
        if clear_cache {
            self.shared.protocol_cache.write().clear();
            self.shared.give_up_until.write().clear();
            self.shared.last_error.write().clear();
        }
        *self.shared.last_scan.lock() = Some(Instant::now());
        self.shared.scanning.store(false, Ordering::SeqCst);
        let _ = app.emit(EVENT_SCAN, self.snapshot_event(false));
        self.refresh_all(app);
    }

    /// 对所有设备刷新一次电量。
    pub fn refresh_all(&self, app: &AppHandle) {
        let devices: Vec<_> = self.shared.devices.read().iter().map(|s| s.device.clone()).collect();
        let settings = self.shared.settings.read().clone();
        let api = new_api();
        let mut results: HashMap<String, Option<Reading>> = HashMap::new();

        for dev in devices {
            // 被用户隐藏的设备不再探测（省电、省时间）
            if settings.hidden.contains(&dev.key) {
                continue;
            }
            let override_id = settings.overrides.get(&dev.key).map(|s| s.as_str());
            let started = Instant::now();
            let mut reading = None;
            let mut last_err = String::new();
            let mut fallback_err = String::new();

            // 低占用：优先复用上次成功的协议；彻底失败的设备退回低频探测
            let cached = self.shared.protocol_cache.read().get(&dev.key).cloned();
            let given_up = self
                .shared
                .give_up_until
                .read()
                .get(&dev.key)
                .map(|t| *t > Instant::now())
                .unwrap_or(false);

            let order: Vec<&'static str> = match (&override_id, &cached) {
                (Some(o), _) if !given_up => proto::candidates(&dev, Some(o)),
                (_, Some(c)) if !given_up => {
                    let mut v = vec![proto::meta(c).map(|m| m.id).unwrap_or("nordic52")];
                    v.extend(proto::candidates(&dev, None));
                    v
                }
                _ if given_up => Vec::new(),
                _ => proto::candidates(&dev, None),
            };

            for proto_id in order {
                let timeout = if cached.is_some() { 600 } else { 260 };
                match proto::read_once(&api, &dev, proto_id, None, timeout) {
                    Ok(raw) => {
                        let latency = started.elapsed().as_millis() as u32;
                        reading = Some(raw.into_reading(&dev.key, proto_id, latency, None));
                        self.shared
                            .protocol_cache
                            .write()
                            .insert(dev.key.clone(), proto_id.to_string());
                        self.shared.give_up_until.write().remove(&dev.key);
                        break;
                    }
                    Err(e) => {
                        // 优先记录“设备有响应但协议不匹配”的具体错误，
                        // 只有全是“找不到接口”时才用兜底说明
                        if e.contains("找不到适用于") || e.contains("未注册 Windows 电池类") {
                            if fallback_err.is_empty() {
                                fallback_err = e;
                            }
                        } else if last_err.is_empty() {
                            last_err = e;
                        }
                    }
                }
            }

            // 所有协议都失败：5 分钟内不再尝试，避免空转（手动刷新会清空）
            if reading.is_none() && override_id.is_none() && !given_up {
                self.shared
                    .give_up_until
                    .write()
                    .insert(dev.key.clone(), Instant::now() + Duration::from_secs(300));
            }

            if reading.is_none() {
                if last_err.is_empty() {
                    last_err = fallback_err;
                }
                // 暂停探测时沿用上次的具体失败原因，保证提示不变
                if !last_err.is_empty() {
                    self.shared
                        .last_error
                        .write()
                        .insert(dev.key.clone(), last_err.clone());
                } else if let Some(cached) = self.shared.last_error.read().get(&dev.key).cloned() {
                    last_err = cached;
                }
                let note = if last_err.is_empty() {
                    "该设备没有公开的电量协议｜可在调试台手动标定".to_string()
                } else if given_up {
                    format!("{last_err}｜已暂停探测，稍后自动重试")
                } else {
                    format!("{last_err}｜可在调试台手动标定协议")
                };

                // 之前读到过就保留上一次读数，只更新说明；否则显示“未能读取”
                let prev = self
                    .shared
                    .devices
                    .read()
                    .iter()
                    .find(|s| s.device.key == dev.key)
                    .and_then(|s| s.reading.clone())
                    .filter(|r| r.percent.is_some());
                reading = Some(match prev {
                    Some(mut r) => {
                        r.note = Some(note);
                        r
                    }
                    None => Reading {
                        key: dev.key.clone(),
                        percent: None,
                        state: BatteryState::Unknown,
                        voltage_mv: None,
                        source: "未匹配协议".into(),
                        protocol: "none".into(),
                        note: Some(note),
                        updated_at: proto::now_ms(),
                        latency_ms: started.elapsed().as_millis() as u32,
                    },
                });
            }
            results.insert(dev.key.clone(), reading);
        }

        {
            let mut guard = self.shared.devices.write();
            for snap in guard.iter_mut() {
                if let Some(r) = results.remove(&snap.device.key) {
                    snap.reading = r;
                }
            }
        }
        self.shared.refresh_seq.fetch_add(1, Ordering::SeqCst);
        let _ = app.emit(EVENT_SNAPSHOT, self.snapshot_event(false));
    }

    /// 设置变更：清掉刚取消隐藏设备的“暂停探测”，并立即刷新。
    pub fn on_settings_changed(&self, app: &AppHandle, old: &AppSettings, new: &AppSettings) {
        {
            let mut give_up = self.shared.give_up_until.write();
            for key in old.hidden.iter() {
                if !new.hidden.contains(key) {
                    give_up.remove(key);
                }
            }
        }
        self.wake_now();
        let _ = app.emit(EVENT_SCAN, self.snapshot_event(false));
    }

    pub fn snapshot_event(&self, scanning: bool) -> SnapshotEvent {
        let settings = self.shared.settings.read().clone();
        let mut devices = self.shared.devices.read().clone();
        // 隐藏状态在广播时按当前设置重新计算，前端拿到的永远是最新的
        for d in devices.iter_mut() {
            d.hidden = settings.hidden.contains(&d.device.key);
        }
        SnapshotEvent {
            devices,
            scanning,
            refreshed_at: proto::now_ms(),
            refresh_ms: settings.refresh_ms,
        }
    }
}

/// 后台轮询线程
pub fn spawn_worker(app: AppHandle, engine: Arc<Engine>) {
    std::thread::Builder::new()
        .name("battery-poll".into())
        .spawn(move || {
            // 启动时扫描一次
            std::thread::sleep(Duration::from_millis(250));
            engine.rescan(&app, true);

            let mut last_scan = Instant::now();
            loop {
                let settings = engine.shared.settings.read().clone();
                let interval = Duration::from_millis(settings.refresh_ms.clamp(2000, 3_600_000));
                // 分片睡眠并在设置变更时被唤醒，改刷新间隔能立刻生效
                let deadline = Instant::now() + interval;
                loop {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    let (lock, cv) = &*engine.wake;
                    let guard = lock.lock().unwrap_or_else(|x| x.into_inner());
                    let _ = cv.wait_timeout(guard, deadline - now);
                    if engine.wake_wanted.swap(false, Ordering::SeqCst) {
                        break;
                    }
                }

                // 设备热插拔：每分钟重新枚举一次（不清协议缓存）
                if last_scan.elapsed() > Duration::from_secs(60) {
                    last_scan = Instant::now();
                    engine.rescan(&app, false);
                } else {
                    engine.refresh_all(&app);
                }

                // 低电量提醒
                notify_low_battery(&app, &engine, settings.warn_percent);
            }
        })
        .expect("启动轮询线程失败");
}

fn notify_low_battery(app: &AppHandle, engine: &Arc<Engine>, threshold: u8) {
    let devices = engine.shared.devices.read();
    let mut notified = engine.shared.low_notified.write();
    for snap in devices.iter() {
        if snap.hidden {
            continue;
        }
        if let Some(r) = &snap.reading {
            if let Some(p) = r.percent {
                if p > threshold || r.state != crate::model::BatteryState::Discharging {
                    notified.remove(&snap.device.key);
                    continue;
                }
                let due = notified
                    .get(&snap.device.key)
                    .map(|t| t.elapsed() > Duration::from_secs(1800))
                    .unwrap_or(true);
                if due {
                    notified.insert(snap.device.key.clone(), Instant::now());
                    let _ = app.emit(
                        "battery://low",
                        serde_json::json!({
                            "key": snap.device.key,
                            "name": snap.device.name,
                            "percent": p,
                        }),
                    );
                }
            }
        }
    }
}

/// 任务栏看门狗：高频检查停靠位置，Explorer 重启 / 分辨率变化后 1-2 秒内恢复。
pub fn spawn_taskbar_watchdog(app: AppHandle, engine: Arc<Engine>) {
    std::thread::Builder::new()
        .name("taskbar-watchdog".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_millis(1500));
            if engine.shared.settings.read().taskbar_enabled {
                crate::taskbar::ensure_embedded(&app, &engine);
            }
        })
        .expect("启动任务栏看门狗失败");
}
