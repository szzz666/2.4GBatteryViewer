//! 标准 HID 电量、Glorious/Lamzu、A4Tech、Windows 电池类。
use hidapi::HidApi;

use super::{drain, open, RawReading};
use crate::model::{BatteryState, DeviceInfo, HidInterface};

/// 标准 HID 电量：Usage 0x06:0x20 Battery Strength / 0x85 Battery System
pub fn read_hid_usage(api: &HidApi, iface: &HidInterface, _timeout_ms: i32) -> Result<RawReading, String> {
    let h = open(api, &iface.path)?;
    drain(&h, 4);

    if iface.feature_len > 0 {
        // 个别设备对 Feature 请求不作回复，HidD_GetFeature 会一直卡在驱动里。
        // 放到独立线程里最多等 1.5 秒，读不到就走输入报告，绝不拖死轮询线程。
        if let Some(r) = feature_probe(&iface.path, iface.feature_len) {
            return Ok(r);
        }
    }

    let mut inbuf = vec![0u8; iface.input_len.max(1) as usize];
    for _ in 0..3 {
        let n = h.read_timeout(&mut inbuf, 150).unwrap_or(0);
        if n == 0 {
            break;
        }
        if let Some(p) = plausible_percent(&inbuf[1..n]) {
            return Ok(RawReading {
                percent: p,
                state: if p >= 100 { BatteryState::Full } else { BatteryState::Discharging },
                voltage_mv: None,
                source: "标准 HID · Input Report".into(),
            });
        }
    }
    Err("标准 HID 电量 usage 未报告数据".into())
}

/// 在带超时的独立线程里试读 Feature Report
fn feature_probe(path: &str, feature_len: u16) -> Option<RawReading> {
    let path = path.to_string();
    let (tx, rx) = std::sync::mpsc::channel::<Option<RawReading>>();
    std::thread::spawn(move || {
        let api = crate::engine::new_api();
        let Ok(h) = open(&api, &path) else {
            let _ = tx.send(None);
            return;
        };
        let mut buf = vec![0u8; feature_len as usize];
        for rid in 0u8..=16 {
            buf.iter_mut().for_each(|b| *b = 0);
            buf[0] = rid;
            if h.get_feature_report(&mut buf).is_err() {
                continue;
            }
            if let Some(p) = plausible_percent(&buf[1..]) {
                let _ = tx.send(Some(RawReading {
                    percent: p,
                    state: if p >= 100 { BatteryState::Full } else { BatteryState::Discharging },
                    voltage_mv: None,
                    source: format!("标准 HID · Feature Report 0x{rid:02X}"),
                }));
                return;
            }
        }
        let _ = tx.send(None);
    });
    rx.recv_timeout(std::time::Duration::from_millis(1500))
        .ok()
        .flatten()
}

fn plausible_percent(data: &[u8]) -> Option<u8> {
    data.iter().copied().find(|v| *v > 0 && *v <= 100)
}

/// Glorious / Lamzu：65 字节 feature
pub fn read_glorious(api: &HidApi, iface: &HidInterface, _timeout_ms: i32) -> Result<RawReading, String> {
    let h = open(api, &iface.path)?;
    drain(&h, 4);
    let mut req = vec![0u8; 65];
    req[0] = 0x00;
    req[3] = 0x02;
    req[4] = 0x02;
    req[6] = 0x83;
    h.send_feature_report(&req).map_err(|e| format!("发送失败: {e}"))?;
    std::thread::sleep(std::time::Duration::from_millis(80));
    let mut resp = vec![0u8; 65];
    let n = h.get_feature_report(&mut resp).map_err(|e| format!("读取失败: {e}"))?;
    if n < 9 {
        return Err("回复过短".into());
    }
    let percent = resp[8];
    if percent > 100 {
        return Err(format!("电量字段异常 {percent}"));
    }
    Ok(RawReading {
        percent,
        state: if percent >= 100 { BatteryState::Full } else { BatteryState::Discharging },
        voltage_mv: None,
        source: "Glorious/Lamzu · Feature 0x83".into(),
    })
}

/// A4Tech Bloody
pub fn read_a4tech(api: &HidApi, iface: &HidInterface, _timeout_ms: i32) -> Result<RawReading, String> {
    let h = open(api, &iface.path)?;
    drain(&h, 4);
    let mut req = [0u8; 8];
    req[1] = 0x80;
    req[7] = 0x7f;
    h.write(&req).map_err(|e| format!("写入失败: {e}"))?;
    let mut resp = [0u8; 8];
    let n = h.read_timeout(&mut resp, 400).unwrap_or(0);
    if n < 8 || resp[1] != 0x80 {
        return Err("A4Tech 无响应".into());
    }
    let percent = resp[2].min(100);
    Ok(RawReading {
        percent,
        state: if percent >= 100 { BatteryState::Full } else { BatteryState::Discharging },
        voltage_mv: None,
        source: "A4Tech Bloody · 00 80".into(),
    })
}

/// Windows 电池类（HidBatt）。只有暴露 0x06:0x20 / 0x85 usage 的设备才会走这里，
/// 这类设备会由标准 HID 通道读到数据，这里只作为兜底说明。
pub fn read_win_battery(dev: &DeviceInfo) -> Result<RawReading, String> {
    let has_battery_usage = dev
        .interfaces
        .iter()
        .any(|i| (i.usage_page == 0x06 && i.usage == 0x20) || i.usage_page == 0x85);
    if !has_battery_usage {
        return Err("该设备未注册 Windows 电池类".into());
    }
    Err("请用「标准 HID 电量」协议读取".into())
}

/// 调试：读 Feature Report
pub fn dump_feature(api: &HidApi, iface: &HidInterface, report_id: u8, len: usize) -> Result<Vec<u8>, String> {
    let h = open(api, &iface.path)?;
    let mut buf = vec![0u8; len.max(1)];
    buf[0] = report_id;
    let n = h
        .get_feature_report(&mut buf)
        .map_err(|e| format!("get_feature_report 失败: {e}"))?;
    buf.truncate(n);
    Ok(buf)
}

/// 调试：发送任意 output report 并读回
pub fn send_raw(api: &HidApi, iface: &HidInterface, data: &[u8], timeout_ms: i32) -> Result<Vec<Vec<u8>>, String> {
    let h = open(api, &iface.path)?;
    drain(&h, 4);
    h.write(data).map_err(|e| format!("写入失败: {e}"))?;
    let mut out = Vec::new();
    let mut buf = [0u8; 512];
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms.max(100) as u64);
    while std::time::Instant::now() < deadline {
        let remain = (deadline - std::time::Instant::now()).as_millis() as i32;
        match h.read_timeout(&mut buf, remain.max(1)) {
            Ok(0) => break,
            Ok(n) => out.push(buf[..n].to_vec()),
            Err(_) => break,
        }
    }
    Ok(out)
}

#[allow(dead_code)]
fn _unused(_c: *mut c_void) {}

use std::ffi::c_void;
