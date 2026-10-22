//! 协议引擎：把各种厂商私有/公开的 HID 电量协议统一成 Reading。
pub mod aula;
pub mod cm;
pub mod generic;
pub mod hidpp;
pub mod nordic;
pub mod razer;
pub mod steelseries;

use crate::model::{BatteryState, DeviceInfo, HidInterface, Reading, ProtocolTry};
use crate::model::HidInterface as Iface;

/// 一个可打开的 HID collection。
pub struct Port {
    pub api: *const hidapi::HidApi,
    pub path: String,
    pub iface: Iface,
}

#[derive(Debug, Clone)]
pub struct RawReading {
    pub percent: u8,
    pub state: BatteryState,
    pub voltage_mv: Option<u16>,
    pub source: String,
}

impl RawReading {
    pub fn into_reading(self, key: &str, protocol: &str, latency_ms: u32, note: Option<String>) -> Reading {
        Reading {
            key: key.to_string(),
            percent: Some(self.percent.min(100)),
            state: self.state,
            voltage_mv: self.voltage_mv,
            source: self.source,
            protocol: protocol.to_string(),
            note,
            updated_at: now_ms(),
            latency_ms,
        }
    }
}

pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 协议描述，用于 UI 展示与手动标定。
pub struct ProtocolMeta {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

pub const PROTOCOLS: &[ProtocolMeta] = &[
    ProtocolMeta { id: "hidpp2", name: "Logitech HID++ 2.0", description: "0x1004 统一电量 / 0x1000 / 0x1001 / 0x1F20" },
    ProtocolMeta { id: "hidpp1", name: "Logitech HID++ 1.0", description: "寄存器 0x0D 百分比 / 0x07 档位" },
    ProtocolMeta { id: "razer", name: "Razer Synapse", description: "90 字节 Feature，命令类 0x07" },
    ProtocolMeta { id: "nordic52", name: "Nordic 52840 / Compx", description: "17 字节 Report 0x08，校验和 0x55（ATK/VXE/VGN/Zaopin/Pulsar…）" },
    ProtocolMeta { id: "nordic54", name: "Nordic 54L15", description: "64 字节 Report 0x08，命令 0x72（ATK Zero）" },
    ProtocolMeta { id: "aula", name: "AULA 鼠标 0x55/0xAA", description: "64 字节帧，CMD 0x30 读电量（狼蛛网页驱动）" },
    ProtocolMeta { id: "aulakb", name: "AULA 键盘/耳机", description: "Report 0x55 / Cmd 0x65，回包 55 65 电量 状态" },
    ProtocolMeta { id: "cm55", name: "C-Media 0x55/0x65", description: "通用 55 65 01 查询，回复 55 65 电量 充电" },
    ProtocolMeta { id: "mchose", name: "MCHOSE 0x55/0x65", description: "64 字节 Report 0x55，UsagePage 0xFF90" },
    ProtocolMeta { id: "realtek", name: "Realtek 推送式", description: "Report 0x13 子类型 0x1D，字节异或 0xFF" },
    ProtocolMeta { id: "steelseries", name: "SteelSeries Arctis", description: "06 18 / 00 B0 / 06 B0" },
    ProtocolMeta { id: "hyperx", name: "HyperX Cloud", description: "06 FF BB 02 查询" },
    ProtocolMeta { id: "corsair", name: "Corsair", description: "00 02 … 02 0F 查询" },
    ProtocolMeta { id: "glorious", name: "Glorious / Lamzu", description: "65 字节 Feature 00 00 00 02 02 00 83" },
    ProtocolMeta { id: "a4tech", name: "A4Tech Bloody", description: "00 80 00 … 7F" },
    ProtocolMeta { id: "hidusage", name: "标准 HID 电量", description: "Usage 0x06:0x20 Battery Strength / 0x85 电池系统" },
    ProtocolMeta { id: "winbattery", name: "Windows 电池类", description: "HidBatt 设备节点 IOCTL_BATTERY_QUERY_INFORMATION" },
];

pub fn meta(id: &str) -> Option<&'static ProtocolMeta> {
    PROTOCOLS.iter().find(|p| p.id == id)
}

/// 依据 VID/PID 猜测候选协议（顺序即尝试顺序）。
pub fn candidates(dev: &DeviceInfo, override_id: Option<&str>) -> Vec<&'static str> {
    if let Some(id) = override_id {
        if let Some(m) = meta(id) {
            return vec![m.id];
        }
    }
    let mut v: Vec<&'static str> = Vec::new();
    match dev.vendor_id {
        0x046d => {
            v.push("hidpp2");
            v.push("hidpp1");
        }
        0x1532 => v.push("razer"),
        0x3554 | 0x373b | 0x25a7 => {
            v.push("nordic52");
            v.push("nordic54");
        }
        0x3837 => {
            v.push("mchose");
            v.push("cm55");
            v.push("nordic52");
        }
        0x1038 => v.push("steelseries"),
        0x03f0 => v.push("hyperx"),
        0x1b1c => v.push("corsair"),
        0x258a => v.push("glorious"),
        0x1956 => v.push("a4tech"),
        // THI / iKF / C-Media 系耳机
        0x0e6a | 0x0c76 | 0x1b3f | 0x054c | 0x1395 => {
            v.push("cm55");
            v.push("mchose");
        }
        0x38a6 | 0x384c | 0x04d9 | 0x1ea7 | 0x30fa => {
            v.push("aula");
            v.push("aulakb");
            v.push("cm55");
            v.push("nordic52");
        }
        _ => {}
    }
    // 通用兜底：几乎所有带厂商页的设备都值得试一遍
    for id in ["aula", "aulakb", "nordic52", "cm55", "nordic54", "hidpp2", "razer", "mchose", "glorious", "steelseries", "hyperx"] {
        if !v.contains(&id) {
            v.push(id);
        }
    }
    v.push("realtek");
    v.push("hidusage");
    v.push("winbattery");
    v
}

/// 选择用于某协议的最佳 collection。
pub fn pick_iface<'a>(dev: &'a DeviceInfo, proto: &str) -> Option<&'a HidInterface> {
    let ifaces = &dev.interfaces;
    match proto {
        "hidpp2" | "hidpp1" => ifaces
            .iter()
            .find(|i| i.usage_page == 0xff00 && i.usage == 0x01)
            .or_else(|| ifaces.iter().find(|i| i.usage == 0x02 && i.usage_page <= 0x01))
            .or_else(|| ifaces.iter().find(|i| i.usage_page >= 0xff00)),
        "aula" => aula::pick_mouse(ifaces).or_else(|| aula::pick_keyboard(ifaces)),
        "aulakb" => aula::pick_keyboard(ifaces),
        "razer" => ifaces
            .iter()
            .find(|i| i.feature_len >= 90)
            .or_else(|| ifaces.iter().find(|i| i.usage_page >= 0xff00)),
        "nordic52" => ifaces
            .iter()
            .find(|i| i.usage_page == 0xff02 && (i.usage == 0x02 || i.usage == 0x01))
            .or_else(|| ifaces.iter().find(|i| i.usage_page == 0xff02))
            .or_else(|| ifaces.iter().find(|i| i.output_len == 17)),
        "nordic54" => ifaces
            .iter()
            .find(|i| i.usage_page == 0xff05)
            .or_else(|| ifaces.iter().find(|i| i.output_len == 64 && i.usage_page >= 0xff00)),
        "cm55" | "mchose" => ifaces
            .iter()
            .find(|i| i.usage_page == 0xff90)
            .or_else(|| {
                ifaces
                    .iter()
                    .filter(|i| i.usage_page >= 0xff00 && i.output_len >= 17)
                    .max_by_key(|i| i.output_len)
            })
            .or_else(|| ifaces.iter().find(|i| i.output_len >= 17))
            // C-Media 耳机只有 0x0C:0x01 通道可写，且报告长度 32
            .or_else(|| {
                ifaces
                    .iter()
                    .find(|i| i.usage_page == 0x000c && i.output_len >= 8)
            }),
        "realtek" => ifaces
            .iter()
            .find(|i| i.usage_page == 0xff01)
            .or_else(|| ifaces.iter().find(|i| i.usage_page >= 0xff00 && i.input_len >= 16)),
        "steelseries" | "hyperx" | "corsair" | "glorious" | "a4tech" => ifaces
            .iter()
            .find(|i| i.usage_page >= 0xff00 && i.input_len >= 8)
            .or_else(|| ifaces.iter().find(|i| i.input_len >= 8 && i.input_len <= 64)),
        "hidusage" => ifaces
            .iter()
            .find(|i| i.usage_page == 0x06 && i.usage == 0x20)
            .or_else(|| ifaces.iter().find(|i| i.usage_page == 0x85)),
        _ => ifaces.iter().find(|i| i.usage_page >= 0xff00),
    }
}

/// 统一入口：读一次电量。
pub fn read_once(
    api: &hidapi::HidApi,
    dev: &DeviceInfo,
    proto: &str,
    override_iface: Option<&str>,
    timeout_ms: i32,
) -> Result<RawReading, String> {
    let iface = match override_iface {
        Some(p) => dev.interfaces.iter().find(|i| i.path == p),
        None => pick_iface(dev, proto),
    }
    .ok_or_else(|| format!("找不到适用于 {proto} 的 HID 接口"))?;

    let started = std::time::Instant::now();
    let result = match proto {
        "hidpp2" => hidpp::read_v2(api, iface, dev, timeout_ms),
        "hidpp1" => hidpp::read_v1(api, iface, dev, timeout_ms),
        "aula" => aula::read(api, iface, timeout_ms),
        "aulakb" => aula::read_keyboard(api, iface, timeout_ms),
        "razer" => razer::read(api, iface, timeout_ms),
        "nordic52" => nordic::read_52(api, iface, timeout_ms),
        "nordic54" => nordic::read_54(api, iface, timeout_ms),
        "cm55" => cm::read_cm55(api, iface, timeout_ms),
        "mchose" => cm::read_mchose(api, iface, timeout_ms),
        "realtek" => cm::read_realtek(api, iface, timeout_ms),
        "steelseries" => steelseries::read(api, iface, timeout_ms),
        "hyperx" => steelseries::read_hyperx(api, iface, timeout_ms),
        "corsair" => steelseries::read_corsair(api, iface, timeout_ms),
        "glorious" => generic::read_glorious(api, iface, timeout_ms),
        "a4tech" => generic::read_a4tech(api, iface, timeout_ms),
        "hidusage" => generic::read_hid_usage(api, iface, timeout_ms),
        "winbattery" => generic::read_win_battery(dev),
        other => Err(format!("未知协议 {other}")),
    };
    let _ = started;
    result
}

pub fn open(api: &hidapi::HidApi, path: &str) -> Result<hidapi::HidDevice, String> {
    let c = std::ffi::CString::new(path).map_err(|e| e.to_string())?;
    api.open_path(&c)
        .ok()
        .ok_or_else(|| format!("打开 HID 失败: {path}"))
}

/// 关闭前清空输入缓冲，避免后续读到旧数据。
pub fn drain(dev: &hidapi::HidDevice, max: usize) {
    let mut buf = [0u8; 512];
    for _ in 0..max {
        if dev.read_timeout(&mut buf, 3).unwrap_or(0) == 0 {
            break;
        }
    }
}

/// 读出 0..n 个输入报告（带整体超时）。
pub fn read_reports(dev: &hidapi::HidDevice, want: usize, timeout_ms: i32) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut buf = [0u8; 512];
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms.max(50) as u64);
    while out.len() < want && std::time::Instant::now() < deadline {
        let remain = deadline.saturating_duration_since(std::time::Instant::now()).as_millis() as i32;
        let n = dev.read_timeout(&mut buf, remain.max(1)).unwrap_or(0);
        if n == 0 {
            break;
        }
        out.push(buf[..n].to_vec());
    }
    out
}

/// 调试：对某个设备跑遍候选协议，返回全部结果。
pub fn diagnose(api: &hidapi::HidApi, dev: &DeviceInfo) -> Vec<ProtocolTry> {
    let mut out = Vec::new();
    for proto in candidates(dev, None) {
        let m = meta(proto);
        let started = std::time::Instant::now();
        let res = read_once(api, dev, proto, None, 350);
        let elapsed = started.elapsed().as_millis() as u32;
        out.push(ProtocolTry {
            protocol: proto.to_string(),
            name: m.map(|m| m.name.to_string()).unwrap_or_else(|| proto.to_string()),
            ok: res.is_ok(),
            percent: res.as_ref().ok().map(|r| r.percent),
            state: res.as_ref().map(|r| r.state).unwrap_or(BatteryState::Unknown),
            voltage_mv: res.as_ref().ok().and_then(|r| r.voltage_mv),
            detail: match &res {
                Ok(r) => r.source.clone(),
                Err(e) => e.clone(),
            },
            raw: Vec::new(),
            elapsed_ms: elapsed,
        });
    }
    out
}