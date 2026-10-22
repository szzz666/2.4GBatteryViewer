//! Razer Synapse 协议：90 字节 Feature Report。
//! 注意校验是“异或”，不是 CRC-8；transaction id 逐型号不同，需要依次尝试。
use hidapi::HidApi;

use super::{drain, open, RawReading};
use crate::model::{BatteryState, HidInterface};

fn xor_crc(report: &[u8]) -> u8 {
    let mut crc = 0u8;
    for b in &report[2..88] {
        crc ^= *b;
    }
    crc
}

fn build(tid: u8, class: u8, id: u8, data_size: u8) -> Vec<u8> {
    let mut r = vec![0u8; 90];
    r[0] = 0x00;
    r[1] = tid;
    r[2] = 0x00;
    r[3] = 0x00;
    r[4] = 0x00;
    r[5] = data_size;
    r[6] = class;
    r[7] = id;
    let crc = xor_crc(&r);
    r[88] = crc;
    r[89] = 0x00;
    r
}

fn exchange(api: &HidApi, iface: &HidInterface, report: &[u8]) -> Result<Vec<u8>, String> {
    let h = open(api, &iface.path)?;
    drain(&h, 4);
    // hidapi 的 feature report 需要首字节是 report id
    let mut buf = vec![0u8; report.len() + 1];
    buf[0] = 0x00;
    buf[1..].copy_from_slice(report);
    h.send_feature_report(&buf).map_err(|e| format!("发送 Feature 失败: {e}"))?;
    std::thread::sleep(std::time::Duration::from_millis(120));
    let mut resp = vec![0u8; report.len() + 1];
    resp[0] = 0x00;
    let n = h.get_feature_report(&mut resp).map_err(|e| format!("读取 Feature 失败: {e}"))?;
    if n == 0 {
        return Err("Feature 读取为空".into());
    }
    Ok(resp[1..].to_vec())
}

pub fn read(api: &HidApi, iface: &HidInterface, _timeout_ms: i32) -> Result<RawReading, String> {
    // transaction id 逐型号不同，依次尝试
    let mut last_err = String::new();
    for tid in [0x1fu8, 0x3f, 0xff, 0x1e, 0x00] {
        let req = build(tid, 0x07, 0x80, 0x02);
        match exchange(api, iface, &req) {
            Ok(resp) => {
                if resp.len() < 10 {
                    last_err = "回复长度不足".into();
                    continue;
                }
                let status = resp[0];
                if status != 0x02 && status != 0x00 {
                    last_err = format!("Razer 状态码 0x{status:02X}");
                    continue;
                }
                let raw = resp[9];
                let percent = ((raw as f32 / 255.0) * 100.0).round() as u8;
                if percent > 100 {
                    last_err = format!("电量异常 {raw}");
                    continue;
                }
                // 单独查询充电状态
                let charging = match exchange(api, iface, &build(tid, 0x07, 0x84, 0x02)) {
                    Ok(r) if r.len() > 9 => r[9] == 1,
                    _ => false,
                };
                let state = if charging {
                    if percent >= 100 { BatteryState::Full } else { BatteryState::Charging }
                } else if percent >= 100 {
                    BatteryState::Full
                } else {
                    BatteryState::Discharging
                };
                return Ok(RawReading {
                    percent,
                    state,
                    voltage_mv: None,
                    source: format!("Razer Synapse · TID 0x{tid:02X}"),
                });
            }
            Err(e) => last_err = e,
        }
    }
    Err(if last_err.is_empty() { "Razer 无响应".into() } else { last_err })
}
