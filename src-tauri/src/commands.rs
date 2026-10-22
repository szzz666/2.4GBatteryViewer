//! Tauri 命令层：前端调用的所有接口。
//!
//! 约定：所有会碰 HID 的命令都写成 async，并用 spawn_blocking 跑在阻塞线程池里。
//! Tauri 的同步命令是在 UI 线程上执行的，慢一点整个窗口就白屏卡死——调试台
//! 之前“一打开就卡住”就是踩了这个坑。
use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};

use crate::engine::{Engine, SnapshotEvent, EVENT_SETTINGS};
use crate::model::{
    AppSettings, DeviceInfo, DeviceSnapshot, FeatureResult, HidInterface, ProbeResult, ProtocolTry,
    ReportIdProbe,
};
use crate::proto::{self, generic};
use crate::scan;

pub struct EngineState(pub Arc<Engine>);

fn engine_of(app: &AppHandle) -> Arc<Engine> {
    app.state::<EngineState>().0.clone()
}

/// 把最新设置推给所有窗口（面板 / 悬浮框 / 任务栏各有一份 React 状态）
fn broadcast_settings(app: &AppHandle, settings: &AppSettings) {
    let _ = app.emit(EVENT_SETTINGS, settings);
}

#[tauri::command]
pub fn get_snapshot(app: AppHandle) -> SnapshotEvent {
    engine_of(&app).snapshot_event(false)
}

#[tauri::command]
pub async fn rescan(app: AppHandle) -> Result<SnapshotEvent, String> {
    let app2 = app.clone();
    let ev = engine_of(&app).snapshot_event(true);
    std::thread::spawn(move || {
        let e = engine_of(&app2);
        e.rescan(&app2, true);
    });
    Ok(ev)
}

#[tauri::command]
pub async fn refresh_now(app: AppHandle) -> Result<SnapshotEvent, String> {
    let e = engine_of(&app);
    // 手动刷新时清空“暂停探测”，确保会重新尝试所有协议
    e.shared.give_up_until.write().clear();
    let app2 = app.clone();
    std::thread::spawn(move || {
        let e = engine_of(&app2);
        e.refresh_all(&app2);
    });
    Ok(e.snapshot_event(false))
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> AppSettings {
    engine_of(&app).shared.settings.read().clone()
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, settings: AppSettings) -> Result<AppSettings, String> {
    let e = engine_of(&app);
    let old = e.shared.settings.read().clone();
    let mut settings = settings;
    // 任务栏宽度由后端按内容实测，前端手里那份可能已经过期，别拿它覆盖
    settings.taskbar_width = old.taskbar_width;
    {
        let mut guard = e.shared.settings.write();
        *guard = settings.clone();
    }
    // 先落盘：失败要让用户看到，而不是静默丢弃
    crate::store::save(&settings)?;
    crate::autostart::apply(&app, settings.autostart);
    e.on_settings_changed(&app, &old, &settings);
    broadcast_settings(&app, &settings);
    Ok(settings)
}

/// 恢复默认设置
#[tauri::command]
pub async fn reset_settings(app: AppHandle) -> Result<AppSettings, String> {
    crate::store::reset()?;
    let defaults = AppSettings::default();
    let e = engine_of(&app);
    let old = e.shared.settings.read().clone();
    {
        let mut guard = e.shared.settings.write();
        *guard = defaults.clone();
    }
    crate::store::save(&defaults)?;
    crate::autostart::apply(&app, false);
    // 默认配置是"只有主面板"：把任务栏药丸和悬浮框真的收起来，
    // 否则点了“恢复默认”它们还挂在屏幕上，看起来像没生效
    let _ = crate::taskbar::unembed(&app);
    if let Some(w) = app.get_webview_window("taskbar") {
        let _ = w.hide();
    }
    if let Some(w) = app.get_webview_window("float") {
        let _ = w.hide();
    }
    e.on_settings_changed(&app, &old, &defaults);
    broadcast_settings(&app, &defaults);
    let app2 = app.clone();
    std::thread::spawn(move || {
        let e = engine_of(&app2);
        e.rescan(&app2, true);
    });
    Ok(defaults)
}

/// 启用/关闭任务栏形态
#[tauri::command]
pub async fn set_taskbar_enabled(app: AppHandle, enabled: bool) -> Result<(), String> {
    let e = engine_of(&app);
    if enabled {
        crate::windows::switch_mode(&app, &e, "taskbar")
    } else {
        crate::windows::disable_taskbar(&app, &e)
    }
}

/// 按内容调整悬浮框尺寸（单行显示，宽度自适应）
#[tauri::command]
pub fn resize_float(app: AppHandle, width: f64, height: f64) -> Result<(), String> {
    let w = width.clamp(90.0, 900.0).round();
    let h = height.clamp(24.0, 120.0).round();
    if let Some(win) = app.get_webview_window("float") {
        let _ = win.set_size(tauri::LogicalSize::new(w, h));
    }
    Ok(())
}

/// 调整任务栏窗口宽度（内容变化后由前端调用）
#[tauri::command]
pub fn resize_taskbar(app: AppHandle, width: f64) -> Result<(), String> {
    let e = engine_of(&app);
    let w = width.clamp(140.0, 900.0).round() as i32;
    let (enabled, changed) = {
        let mut s = e.shared.settings.write();
        if s.taskbar_width == w {
            (s.taskbar_enabled, false)
        } else {
            s.taskbar_width = w;
            let snap = s.clone();
            let enabled = snap.taskbar_enabled;
            drop(s);
            let _ = crate::store::save(&snap);
            (enabled, true)
        }
    };
    if !changed {
        return Ok(());
    }
    // 药丸已经被关掉时不要动窗口：隐藏的窗口一旦 set_size / show 就会重新冒出来
    if !enabled {
        return Ok(());
    }
    if let Some(win) = app.get_webview_window("taskbar") {
        let _ = win.set_size(tauri::LogicalSize::new(w as f64, 32.0));
    }
    // 重新贴到任务栏右侧（顶层窗口，屏幕坐标）
    crate::taskbar::dock(&app, w);
    Ok(())
}

#[tauri::command]
pub fn list_devices(app: AppHandle) -> Vec<DeviceInfo> {
    engine_of(&app)
        .shared
        .devices
        .read()
        .iter()
        .map(|s| s.device.clone())
        .collect()
}

/// 调试：重新枚举全系统 HID（包含无关设备）
#[tauri::command]
pub async fn enumerate_all() -> Vec<DeviceInfo> {
    tauri::async_runtime::spawn_blocking(scan::scan_all_full)
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn list_interfaces() -> Vec<HidInterface> {
    tauri::async_runtime::spawn_blocking(crate::hidwin::enumerate_interfaces)
        .await
        .unwrap_or_default()
}

/// 调试：AULA 接收器握手诊断 —— 返回原始收发记录
#[tauri::command]
pub async fn aula_diagnose(path: String) -> serde_json::Value {
    tauri::async_runtime::spawn_blocking(move || {
        let api = crate::engine::new_api();
        let Ok(h) = crate::proto::open(&api, &path) else {
            return serde_json::json!({ "error": "打开设备失败" });
        };
        crate::proto::drain(&h, 4);
        let mut log = Vec::new();
        for (rid, name) in [(0x0bu8, "OUT 0x0B"), (0x14u8, "OUT 0x14")] {
            for cmd in [0x30u8, 0xed, 0x03, 0x0e] {
                let mut frame = vec![0u8; 64];
                frame[0] = 0x55;
                frame[1] = cmd;
                frame[2] = 0x01;
                frame[4] = 0;
                frame[3] = frame[4..8].iter().fold(0u8, |a, b| a.wrapping_add(*b));
                let mut buf = vec![rid];
                buf.extend_from_slice(&frame);
                let sent_ok = h.write(&buf).is_ok();
                let replies = crate::proto::read_reports(&h, 3, 200);
                log.push(serde_json::json!({
                    "channel": name,
                    "cmd": format!("0x{cmd:02X}"),
                    "sent": sent_ok,
                    "replies": replies.iter().map(|r| r.iter().take(24).map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>(),
                }));
            }
        }
        serde_json::json!({ "path": path, "log": log })
    })
    .await
    .unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() }))
}

#[tauri::command]
pub fn list_protocols() -> Vec<serde_json::Value> {
    proto::PROTOCOLS
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "name": p.name,
                "description": p.description,
            })
        })
        .collect()
}

/// 调试：读取 Feature Report（同样加 3 秒硬超时，防止设备不回复时按钮一直转圈）
#[tauri::command]
pub async fn read_feature(path: String, report_id: u8, length: usize, use_feature: bool) -> FeatureResult {
    tauri::async_runtime::spawn_blocking(move || {
        let (tx, rx) = std::sync::mpsc::channel::<FeatureResult>();
        std::thread::spawn(move || {
            let _ = tx.send(read_feature_blocking(path, report_id, length, use_feature));
        });
        match rx.recv_timeout(std::time::Duration::from_secs(3)) {
            Ok(r) => r,
            Err(_) => FeatureResult {
                ok: false,
                data: Vec::new(),
                error: Some("设备 3 秒内没有响应（该 collection 可能不支持 Feature Report）".into()),
                elapsed_ms: 3000,
            },
        }
    })
    .await
    .unwrap_or(FeatureResult { ok: false, data: Vec::new(), error: Some("任务被取消".into()), elapsed_ms: 0 })
}

fn read_feature_blocking(path: String, report_id: u8, length: usize, use_feature: bool) -> FeatureResult {
    let started = std::time::Instant::now();
    {
        let api = crate::engine::new_api();
        let result = if use_feature {
            let c = match std::ffi::CString::new(path.as_str()) {
                Ok(c) => c,
                Err(e) => return FeatureResult { ok: false, data: Vec::new(), error: Some(e.to_string()), elapsed_ms: 0 },
            };
            api.open_path(&c)
                .map_err(|e| format!("打开设备失败: {e}"))
                .and_then(|h| {
                    let mut buf = vec![0u8; length.max(1)];
                    buf[0] = report_id;
                    let n = h
                        .get_feature_report(&mut buf)
                        .map_err(|e| format!("get_feature_report: {e}"))?;
                    buf.truncate(n);
                    Ok(buf)
                })
        } else {
            let c = match std::ffi::CString::new(path.as_str()) {
                Ok(c) => c,
                Err(e) => return FeatureResult { ok: false, data: Vec::new(), error: Some(e.to_string()), elapsed_ms: 0 },
            };
            api.open_path(&c)
                .map_err(|e| format!("打开设备失败: {e}"))
                .and_then(|h| {
                    let mut buf = vec![0u8; length.max(1)];
                    let n = h
                        .read_timeout(&mut buf, 500)
                        .map_err(|e| format!("read_timeout: {e}"))?;
                    buf.truncate(n);
                    Ok(buf)
                })
        };
        match result {
            Ok(data) => FeatureResult { ok: true, data, error: None, elapsed_ms: started.elapsed().as_millis() as u32 },
            Err(e) => FeatureResult { ok: false, data: Vec::new(), error: Some(e), elapsed_ms: started.elapsed().as_millis() as u32 },
        }
    }
}

/// 调试：试发任意命令
#[tauri::command]
pub async fn send_probe(path: String, hex: String, timeout_ms: i64, as_feature: bool) -> ProbeResult {
    tauri::async_runtime::spawn_blocking(move || {
        let started = std::time::Instant::now();
        let data = match parse_hex(&hex) {
            Ok(d) => d,
            Err(e) => {
                return ProbeResult {
                    ok: false,
                    sent: Vec::new(),
                    received: Vec::new(),
                    error: Some(e),
                    elapsed_ms: 0,
                }
            }
        };
        let api = crate::engine::new_api();
        let c = match std::ffi::CString::new(path.as_str()) {
            Ok(c) => c,
            Err(e) => {
                return ProbeResult { ok: false, sent: data, received: Vec::new(), error: Some(e.to_string()), elapsed_ms: 0 }
            }
        };
        let Ok(h) = api.open_path(&c) else {
            return ProbeResult {
                ok: false,
                sent: data,
                received: Vec::new(),
                error: Some("打开设备失败".into()),
                elapsed_ms: started.elapsed().as_millis() as u32,
            };
        };
        proto::drain(&h, 4);
        let write_result = if as_feature {
            h.send_feature_report(&data).map(|_| ()).map_err(|e| e.to_string())
        } else {
            h.write(&data).map(|_| ()).map_err(|e| e.to_string())
        };
        let mut received = Vec::new();
        let mut buf = [0u8; 512];
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis((timeout_ms.max(100)) as u64);
        while std::time::Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(std::time::Instant::now()).as_millis() as i32;
            match h.read_timeout(&mut buf, remain.max(1)) {
                Ok(0) => break,
                Ok(n) => received.push(buf[..n].to_vec()),
                Err(_) => break,
            }
        }
        ProbeResult {
            ok: write_result.is_ok(),
            sent: data,
            received,
            error: write_result.err(),
            elapsed_ms: started.elapsed().as_millis() as u32,
        }
    })
    .await
    .unwrap_or(ProbeResult { ok: false, sent: Vec::new(), received: Vec::new(), error: Some("任务被取消".into()), elapsed_ms: 0 })
}

/// 调试：探测哪些 report id 可用。
///
/// 某些设备（尤其是系统独占的鼠标 collection）对 Feature 请求不作回复，
/// HidD_GetFeature 会一直卡在驱动里。这里把扫描放到独立线程、最多等 4 秒，
/// 拿到多少返回多少——绝不让界面按钮永远转圈。
#[tauri::command]
pub async fn probe_report_ids(path: String, feature_len: usize) -> Vec<ReportIdProbe> {
    if feature_len == 0 {
        return Vec::new();
    }
    tauri::async_runtime::spawn_blocking(move || {
        let (tx, rx) = std::sync::mpsc::channel::<ReportIdProbe>();
        std::thread::spawn(move || {
            let api = crate::engine::new_api();
            let Ok(c) = std::ffi::CString::new(path.as_str()) else { return };
            let Ok(h) = api.open_path(&c) else { return };
            for rid in 0u8..=0x40 {
                let mut buf = vec![0u8; feature_len];
                buf[0] = rid;
                if h.get_feature_report(&mut buf).is_ok() {
                    let non_zero = buf.iter().any(|b| *b != 0);
                    let item = ReportIdProbe {
                        report_id: rid,
                        feature_len,
                        non_zero,
                        data: buf,
                    };
                    if tx.send(item).is_err() {
                        return;
                    }
                }
            }
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        let mut out = Vec::new();
        loop {
            let remain = deadline.saturating_duration_since(std::time::Instant::now());
            if remain.is_zero() {
                break;
            }
            match rx.recv_timeout(remain) {
                Ok(p) => out.push(p),
                Err(_) => break,
            }
        }
        out
    })
    .await
    .unwrap_or_default()
}

/// 调试：对某设备跑遍所有协议
#[tauri::command]
pub async fn diagnose_device(app: AppHandle, key: String) -> Vec<ProtocolTry> {
    let e = engine_of(&app);
    let dev = {
        let devices = e.shared.devices.read();
        match devices.iter().find(|s| s.device.key == key) {
            Some(s) => s.device.clone(),
            None => return Vec::new(),
        }
    };
    tauri::async_runtime::spawn_blocking(move || {
        let api = crate::engine::new_api();
        proto::diagnose(&api, &dev)
    })
    .await
    .unwrap_or_default()
}

/// 调试：用指定协议读取一次
#[tauri::command]
pub async fn try_protocol(app: AppHandle, key: String, protocol: String, path: Option<String>) -> ProtocolTry {
    let e = engine_of(&app);
    let dev = {
        let devices = e.shared.devices.read();
        devices.iter().find(|s| s.device.key == key).map(|s| s.device.clone())
    };
    let Some(dev) = dev else {
        return ProtocolTry {
            protocol: protocol.clone(),
            name: protocol,
            ok: false,
            percent: None,
            state: crate::model::BatteryState::Unknown,
            voltage_mv: None,
            detail: "设备不存在".into(),
            raw: Vec::new(),
            elapsed_ms: 0,
        };
    };
    tauri::async_runtime::spawn_blocking(move || {
        let api = crate::engine::new_api();
        let started = std::time::Instant::now();
        let res = proto::read_once(&api, &dev, &protocol, path.as_deref(), 800);
        let m = proto::meta(&protocol);
        ProtocolTry {
            protocol: protocol.clone(),
            name: m.map(|m| m.name.to_string()).unwrap_or_else(|| protocol.clone()),
            ok: res.is_ok(),
            percent: res.as_ref().ok().map(|r| r.percent),
            state: res.as_ref().map(|r| r.state).unwrap_or(crate::model::BatteryState::Unknown),
            voltage_mv: res.as_ref().ok().and_then(|r| r.voltage_mv),
            detail: match &res {
                Ok(r) => r.source.clone(),
                Err(e) => e.clone(),
            },
            raw: Vec::new(),
            elapsed_ms: started.elapsed().as_millis() as u32,
        }
    })
    .await
    .unwrap_or(ProtocolTry {
        protocol: String::new(),
        name: String::new(),
        ok: false,
        percent: None,
        state: crate::model::BatteryState::Unknown,
        voltage_mv: None,
        detail: "任务被取消".into(),
        raw: Vec::new(),
        elapsed_ms: 0,
    })
}

/// 调试：读取指定 collection 的输入报告
#[tauri::command]
pub async fn read_input(path: String, timeout_ms: i64) -> Vec<Vec<u8>> {
    tauri::async_runtime::spawn_blocking(move || {
        let api = crate::engine::new_api();
        let Ok(c) = std::ffi::CString::new(path.as_str()) else { return Vec::new() };
        let Ok(h) = api.open_path(&c) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut buf = [0u8; 512];
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms.max(100) as u64);
        while std::time::Instant::now() < deadline {
            let remain = deadline.saturating_duration_since(std::time::Instant::now()).as_millis() as i32;
            match h.read_timeout(&mut buf, remain.max(1)) {
                Ok(0) => break,
                Ok(n) => out.push(buf[..n].to_vec()),
                Err(_) => break,
            }
        }
        out
    })
    .await
    .unwrap_or_default()
}

/// 调试：Feature 写入
#[tauri::command]
pub async fn write_feature(path: String, hex: String) -> FeatureResult {
    tauri::async_runtime::spawn_blocking(move || {
        let started = std::time::Instant::now();
        let data = match parse_hex(&hex) {
            Ok(d) => d,
            Err(e) => {
                return FeatureResult { ok: false, data: Vec::new(), error: Some(e), elapsed_ms: 0 }
            }
        };
        let api = crate::engine::new_api();
        let Ok(c) = std::ffi::CString::new(path.as_str()) else {
            return FeatureResult { ok: false, data, error: Some("路径非法".into()), elapsed_ms: 0 };
        };
        let Ok(h) = api.open_path(&c) else {
            return FeatureResult {
                ok: false,
                data: data.clone(),
                error: Some("打开设备失败".into()),
                elapsed_ms: started.elapsed().as_millis() as u32,
            };
        };
        match h.send_feature_report(&data) {
            Ok(_) => FeatureResult { ok: true, data, error: None, elapsed_ms: started.elapsed().as_millis() as u32 },
            Err(e) => FeatureResult {
                ok: false,
                data: data.clone(),
                error: Some(e.to_string()),
                elapsed_ms: started.elapsed().as_millis() as u32,
            },
        }
    })
    .await
    .unwrap_or(FeatureResult { ok: false, data: Vec::new(), error: Some("任务被取消".into()), elapsed_ms: 0 })
}

/// 调试：手动标定 —— 指定 deviceKey 使用哪个协议，并立即验证
#[tauri::command]
pub async fn calibrate(app: AppHandle, key: String, protocol: String, path: Option<String>) -> Result<ProtocolTry, String> {
    let result = try_protocol(app.clone(), key.clone(), protocol.clone(), path).await;
    if !result.ok {
        return Err(result.detail);
    }
    let e = engine_of(&app);
    {
        let mut settings = e.shared.settings.write();
        settings.overrides.insert(key.clone(), protocol);
        let s = settings.clone();
        drop(settings);
        crate::store::save(&s)?;
        broadcast_settings(&app, &s);
    }
    e.shared.give_up_until.write().remove(&key);
    let app2 = app.clone();
    std::thread::spawn(move || {
        let e = engine_of(&app2);
        e.refresh_all(&app2);
    });
    Ok(result)
}

/// 撤销手动标定
#[tauri::command]
pub async fn clear_override(app: AppHandle, key: String) -> Result<(), String> {
    let e = engine_of(&app);
    let snapshot = {
        let mut settings = e.shared.settings.write();
        settings.overrides.remove(&key);
        settings.clone()
    };
    crate::store::save(&snapshot)?;
    e.shared.give_up_until.write().remove(&key);
    broadcast_settings(&app, &snapshot);
    Ok(())
}

/// 窗口形态切换
#[tauri::command]
pub async fn set_mode(app: AppHandle, mode: String) -> Result<(), String> {
    let e = engine_of(&app);
    crate::windows::switch_mode(&app, &e, &mode)
}

#[tauri::command]
pub fn get_mode() -> String {
    crate::windows::current_mode()
}

#[tauri::command]
pub async fn set_float_locked(app: AppHandle, locked: bool) -> Result<(), String> {
    let e = engine_of(&app);
    crate::windows::set_locked(&app, &e, locked)
}

/// 关闭悬浮框（记住“下次启动不自动显示”）
#[tauri::command]
pub async fn close_float(app: AppHandle) -> Result<(), String> {
    let e = engine_of(&app);
    crate::windows::hide_float(&app, &e);
    Ok(())
}

#[tauri::command]
pub fn save_float_position(app: AppHandle, x: i32, y: i32) -> Result<(), String> {
    let e = engine_of(&app);
    crate::windows::remember_float_position(&e, x, y);
    Ok(())
}

#[tauri::command]
pub fn start_drag(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("float") {
        w.start_dragging().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn toggle_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    crate::autostart::apply(&app, enabled);
    Ok(enabled)
}

#[tauri::command]
pub fn is_autostart_enabled() -> bool {
    crate::autostart::is_enabled()
}

#[tauri::command]
pub fn hide_window(app: AppHandle, label: String) {
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.hide();
    }
}

#[tauri::command]
pub fn show_window(app: AppHandle, label: String) {
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub fn bump_refresh(app: AppHandle) -> u64 {
    engine_of(&app).shared.refresh_seq.load(Ordering::SeqCst)
}

#[tauri::command]
pub async fn dump_interface(path: String, report_id: u8, length: usize) -> FeatureResult {
    tauri::async_runtime::spawn_blocking(move || {
        let api = crate::engine::new_api();
        let started = std::time::Instant::now();
        let iface = HidInterface {
            path,
            vendor_id: 0,
            product_id: 0,
            manufacturer: None,
            product: None,
            serial: None,
            usage_page: 0,
            usage: 0,
            interface_number: 0,
            input_len: 64,
            output_len: 64,
            feature_len: length as u16,
            report_ids: Vec::new(),
            instance_id: None,
            bus_reported_desc: None,
        };
        match generic::dump_feature(&api, &iface, report_id, length) {
            Ok(data) => FeatureResult { ok: true, data, error: None, elapsed_ms: started.elapsed().as_millis() as u32 },
            Err(e) => FeatureResult { ok: false, data: Vec::new(), error: Some(e), elapsed_ms: started.elapsed().as_millis() as u32 },
        }
    })
    .await
    .unwrap_or(FeatureResult { ok: false, data: Vec::new(), error: Some("任务被取消".into()), elapsed_ms: 0 })
}

#[tauri::command]
pub fn snapshot_for(app: AppHandle, key: String) -> Option<DeviceSnapshot> {
    engine_of(&app)
        .snapshot_event(false)
        .devices
        .into_iter()
        .find(|s| s.device.key == key)
}

#[tauri::command]
pub fn emit_debug(app: AppHandle, payload: serde_json::Value) {
    let _ = app.emit("battery://debug", payload);
}

fn parse_hex(text: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = text
        .chars()
        .filter(|c| c.is_ascii_hexdigit() || *c == ' ' || *c == ',' || *c == '-' || *c == ':')
        .collect();
    let mut out = Vec::new();
    let mut nibble: Option<u8> = None;
    for ch in cleaned.chars() {
        if let Some(v) = ch.to_digit(16) {
            match nibble {
                Some(hi) => {
                    out.push((hi << 4) | v as u8);
                    nibble = None;
                }
                None => nibble = Some(v as u8),
            }
        } else {
            // 分隔符：如果剩下半个字节，补齐为 0
            if let Some(hi) = nibble.take() {
                out.push(hi << 4);
            }
        }
    }
    if let Some(hi) = nibble {
        out.push(hi << 4);
    }
    if out.is_empty() {
        return Err("HEX 内容为空".into());
    }
    Ok(out)
}
