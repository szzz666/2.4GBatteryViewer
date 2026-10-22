//! Logitech HID++ 1.0 / 2.0
//!
//! 传输：short report 0x10（7 字节）/ long report 0x11（20 字节）。
//! Windows 上两个 usage collection 分别是 0x01（short）和 0x02（long），
//! 短请求的回复可能出现在任一 collection，因此两个都打开一起读。
use hidapi::{HidApi, HidDevice};

use super::{drain, open, RawReading};
use crate::model::{BatteryState, DeviceInfo, HidInterface};

const REPORT_SHORT: u8 = 0x10;
const REPORT_LONG: u8 = 0x11;
const SW_ID: u8 = 0x0f;

/// 找到短/长两个 collection
fn transports(dev: &DeviceInfo) -> (Option<&HidInterface>, Option<&HidInterface>) {
    let short = dev
        .interfaces
        .iter()
        .find(|i| i.usage_page == 0xff00 && i.usage == 0x01)
        .or_else(|| dev.interfaces.iter().find(|i| i.usage == 0x02));
    let long = dev
        .interfaces
        .iter()
        .find(|i| i.usage_page == 0xff00 && i.usage == 0x02);
    (short, long)
}

fn try_receivers(dev: &DeviceInfo) -> Vec<u8> {
    // Logitech 接收器：0xFF 表示接收器自身，1..6 为配对槽位
    let mut v = vec![0xffu8];
    if dev.kind == "receiver" || dev.interfaces.iter().any(|i| i.usage_page == 0xff00) {
        v.extend(1..=6);
    }
    v
}

/// 打开可用的长报告句柄（20 字节），没有则用短报告。
fn open_long(api: &HidApi, dev: &DeviceInfo) -> Option<HidDevice> {
    let (short, long) = transports(dev);
    for iface in [long, short].into_iter().flatten() {
        if let Ok(h) = open(api, &iface.path) {
            drain(&h, 8);
            return Some(h);
        }
    }
    None
}

fn send_and_read(h: &HidDevice, msg: &[u8], timeout_ms: i32) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    if h.write(msg).is_err() {
        return out;
    }
    let mut buf = [0u8; 64];
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms.max(60) as u64);
    while std::time::Instant::now() < deadline {
        let remain = (deadline - std::time::Instant::now()).as_millis() as i32;
        match h.read_timeout(&mut buf, remain.max(1)) {
            Ok(0) => break,
            Ok(n) => out.push(buf[..n].to_vec()),
            Err(_) => break,
        }
    }
    out
}

fn long_request(device_index: u8, feature_index: u8, function: u8, params: &[u8]) -> Vec<u8> {
    let mut msg = vec![0u8; 20];
    msg[0] = REPORT_LONG;
    msg[1] = device_index;
    msg[2] = feature_index;
    msg[3] = (function << 4) | SW_ID;
    for (i, b) in params.iter().take(16).enumerate() {
        msg[4 + i] = *b;
    }
    msg
}

fn short_request(device_index: u8, sub_id: u8, address: u8, params: &[u8]) -> Vec<u8> {
    let mut msg = vec![0u8; 7];
    msg[0] = REPORT_SHORT;
    msg[1] = device_index;
    msg[2] = sub_id;
    msg[3] = address;
    for (i, b) in params.iter().take(3).enumerate() {
        msg[4 + i] = *b;
    }
    msg
}

fn get_feature_index(h: &HidDevice, device_index: u8, feature: u16, timeout_ms: i32) -> Option<u8> {
    let params = [(feature >> 8) as u8, (feature & 0xff) as u8];
    let msg = long_request(device_index, 0x00, 0x00, &params);
    for resp in send_and_read(h, &msg, timeout_ms) {
        if resp.len() >= 5
            && resp[0] == REPORT_LONG
            && resp[1] == device_index
            && resp[2] != 0x00
            && resp[2] != 0xff
            && resp[3] == (0x00 << 4 | SW_ID)
        {
            return Some(resp[4]);
        }
    }
    None
}

fn interpret_flags(flags: u8) -> BatteryState {
    // bit0 charging, bit1 discharging... Logitech 0x1004 get_status
    match flags {
        0x01 | 0x02 => BatteryState::Charging,
        0x03 => BatteryState::Charging,
        0x04 => BatteryState::Discharging,
        _ => BatteryState::Unknown,
    }
}

fn unified_battery(h: &HidDevice, device_index: u8, feature_index: u8, timeout_ms: i32) -> Result<RawReading, String> {
    // get_capabilities (0x00)
    let caps = long_request(device_index, feature_index, 0x00, &[0, 0, 0]);
    let _ = send_and_read(h, &caps, timeout_ms);
    // get_status (0x10)
    let msg = long_request(device_index, feature_index, 0x10, &[0, 0, 0]);
    for resp in send_and_read(h, &msg, timeout_ms) {
        if resp.len() >= 9 && resp[2] == feature_index && resp[3] == (0x10 << 4 | SW_ID) {
            let soc = resp[4];
            let flags = resp[6];
            if soc == 0 && flags == 0 {
                return Err("设备返回 0%（可能休眠）".into());
            }
            return Ok(RawReading {
                percent: soc.min(100),
                state: interpret_flags(flags),
                voltage_mv: None,
                source: "HID++ 2.0 · 0x1004 UNIFIED BATTERY".into(),
            });
        }
    }
    Err("0x1004 无响应".into())
}

fn battery_status_1000(h: &HidDevice, device_index: u8, feature_index: u8, timeout_ms: i32) -> Result<RawReading, String> {
    let msg = long_request(device_index, feature_index, 0x00, &[0, 0, 0]);
    for resp in send_and_read(h, &msg, timeout_ms) {
        if resp.len() >= 9 && resp[2] == feature_index && resp[3] == (0x00 << 4 | SW_ID) {
            let battery = resp[4];
            let state = match resp[5] {
                0 => BatteryState::Discharging,
                1 => BatteryState::Charging,
                2 => BatteryState::Charging,
                3 => BatteryState::Full,
                4 => BatteryState::Charging,
                5 => BatteryState::Full,
                _ => BatteryState::Unknown,
            };
            if battery == 0 {
                return Err("0x1000 返回 0%".into());
            }
            return Ok(RawReading {
                percent: battery.min(100),
                state,
                voltage_mv: None,
                source: "HID++ 2.0 · 0x1000 BATTERY LEVEL STATUS".into(),
            });
        }
    }
    Err("0x1000 无响应".into())
}

fn battery_voltage_1001(h: &HidDevice, device_index: u8, feature_index: u8, timeout_ms: i32) -> Result<RawReading, String> {
    let msg = long_request(device_index, feature_index, 0x00, &[0]);
    for resp in send_and_read(h, &msg, timeout_ms) {
        if resp.len() >= 7 && resp[2] == feature_index {
            let mv = ((resp[5] as u16) << 8) | resp[6] as u16;
            if mv < 2000 || mv > 6000 {
                return Err(format!("电压异常 {mv}mV"));
            }
            return Ok(RawReading {
                percent: voltage_to_percent(mv),
                state: BatteryState::Unknown,
                voltage_mv: Some(mv),
                source: "HID++ 2.0 · 0x1001 BATTERY VOLTAGE".into(),
            });
        }
    }
    Err("0x1001 无响应".into())
}

/// 单节锂电近似曲线
pub fn voltage_to_percent(mv: u16) -> u8 {
    const TABLE: &[(u16, u8)] = &[
        (4200, 100), (4100, 90), (4000, 80), (3900, 70), (3800, 60), (3700, 50),
        (3650, 40), (3600, 30), (3500, 20), (3400, 10), (3300, 5), (3200, 0),
    ];
    if mv >= TABLE[0].0 {
        return 100;
    }
    if mv <= TABLE[TABLE.len() - 1].0 {
        return 0;
    }
    for w in TABLE.windows(2) {
        let (hi_v, hi_p) = w[0];
        let (lo_v, lo_p) = w[1];
        if mv <= hi_v && mv >= lo_v {
            let span = (hi_v - lo_v) as f32;
            let t = (mv - lo_v) as f32 / span;
            return (lo_p as f32 + t * (hi_p as f32 - lo_p as f32)).round() as u8;
        }
    }
    let _ = TABLE;
    0
}

fn try_device_index(h: &HidDevice, dev_idx: u8, timeout_ms: i32) -> Result<RawReading, String> {
    // 先 ping
    let ping = short_request(dev_idx, 0x10, 0x00, &[0, 0, 5]);
    let replies = send_and_read(h, &ping, timeout_ms);
    if replies.is_empty() && dev_idx != 0xff {
        return Err("无响应".into());
    }
    if let Some(idx) = get_feature_index(h, dev_idx, 0x1004, timeout_ms) {
        if let Ok(r) = unified_battery(h, dev_idx, idx, timeout_ms) {
            return Ok(r);
        }
    }
    if let Some(idx) = get_feature_index(h, dev_idx, 0x1000, timeout_ms) {
        if let Ok(r) = battery_status_1000(h, dev_idx, idx, timeout_ms) {
            return Ok(r);
        }
    }
    if let Some(idx) = get_feature_index(h, dev_idx, 0x1001, timeout_ms) {
        if let Ok(r) = battery_voltage_1001(h, dev_idx, idx, timeout_ms) {
            return Ok(r);
        }
    }
    Err("该设备未提供 HID++ 2.0 电量特性".into())
}

pub fn read_v2(api: &HidApi, _iface: &HidInterface, dev: &DeviceInfo, timeout_ms: i32) -> Result<RawReading, String> {
    let h = open_long(api, dev).ok_or("无法打开 HID++ 接口")?;
    let mut last = String::from("无响应");
    for idx in try_receivers(dev) {
        match try_device_index(&h, idx, timeout_ms) {
            Ok(r) => return Ok(r),
            Err(e) => last = format!("设备 {idx:02X}: {e}"),
        }
    }
    Err(last)
}

fn reg_0d(h: &HidDevice, dev_idx: u8, timeout_ms: i32) -> Result<RawReading, String> {
    let msg = short_request(dev_idx, 0x81, 0x0d, &[0, 0, 0]);
    for resp in send_and_read(h, &msg, timeout_ms) {
        if resp.len() >= 7 && resp[2] == 0x0d {
            let raw = resp[4] & 0x7f;
            let state = match resp[5] {
                0 => BatteryState::Discharging,
                1 => BatteryState::Charging,
                2 => BatteryState::Charging,
                3 => BatteryState::Full,
                4 => BatteryState::Charging,
                _ => BatteryState::Unknown,
            };
            if raw == 0 {
                return Err("寄存器 0x0D 返回 0".into());
            }
            return Ok(RawReading {
                percent: raw.min(100),
                state,
                voltage_mv: None,
                source: "HID++ 1.0 · 寄存器 0x0D".into(),
            });
        }
    }
    Err("寄存器 0x0D 无响应".into())
}

fn reg_07(h: &HidDevice, dev_idx: u8, timeout_ms: i32) -> Result<RawReading, String> {
    let msg = short_request(dev_idx, 0x81, 0x07, &[0, 0, 0]);
    for resp in send_and_read(h, &msg, timeout_ms) {
        if resp.len() >= 7 && resp[2] == 0x07 {
            let level = &resp[4..7];
            let percent = match level[0] {
                0 => 0,
                1 => 5,
                2 => 20,
                3 => 35,
                4 => 50,
                5 => 65,
                6 => 80,
                7 => 95,
                _ => 0,
            };
            let state = if level[1] >= 1 && level[1] <= 3 {
                BatteryState::Charging
            } else if level[2] != 0 {
                BatteryState::Full
            } else {
                BatteryState::Discharging
            };
            return Ok(RawReading {
                percent,
                state,
                voltage_mv: None,
                source: "HID++ 1.0 · 寄存器 0x07".into(),
            });
        }
    }
    Err("寄存器 0x07 无响应".into())
}

pub fn read_v1(api: &HidApi, _iface: &HidInterface, dev: &DeviceInfo, timeout_ms: i32) -> Result<RawReading, String> {
    let h = open_long(api, dev).ok_or("无法打开 HID++ 接口")?;
    let mut last = String::from("无响应");
    for idx in try_receivers(dev) {
        if let Ok(r) = reg_0d(&h, idx, timeout_ms) {
            return Ok(r);
        }
        if let Ok(r) = reg_07(&h, idx, timeout_ms) {
            return Ok(r);
        }
        last = format!("设备 {idx:02X}: 寄存器 0x0D/0x07 均无响应");
    }
    Err(last)
}

/// 调试：直接发送任意 HID++ 报文
pub fn raw_exchange(api: &HidApi, iface: &HidInterface, payload: &[u8], timeout_ms: i32) -> Result<Vec<Vec<u8>>, String> {
    let h = open(api, &iface.path)?;
    drain(&h, 4);
    Ok(send_and_read(&h, payload, timeout_ms))
}
