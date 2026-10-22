//! Nordic / Compx 2.4G 方案（ATK、VXE、VGN、Zaopin、Scyrox、Dareu、G-Wolves、Pulsar、Kysona…）
use hidapi::{HidApi, HidDevice};

use super::{drain, open, read_reports, RawReading};
use crate::model::{BatteryState, HidInterface};

const MAGIC: u8 = 0x55;

fn checksum(bytes: &[u8]) -> u8 {
    let sum: u8 = bytes.iter().fold(0u8, |a, b| a.wrapping_add(*b));
    MAGIC.wrapping_sub(sum)
}

/// Nordic 52840：17 字节报告 0x08，子命令 0x04
pub fn read_52(api: &HidApi, iface: &HidInterface, timeout_ms: i32) -> Result<RawReading, String> {
    let h = open(api, &iface.path)?;
    drain(&h, 8);
    // 报告长度必须等于设备的 OutputReportByteLength（多数 17，部分 dongle 是 64）
    let len = if iface.output_len >= 17 { iface.output_len as usize } else { 17 };
    let mut req = vec![0u8; len];
    req[0] = 0x08;
    req[1] = 0x04;
    req[16] = checksum(&req[..16]);
    h.write(&req).map_err(|e| format!("写入失败: {e}"))?;
    for resp in read_reports(&h, 4, timeout_ms) {
        if resp.len() < 17 || resp[0] != 0x08 || resp[1] != 0x04 {
            continue;
        }
        let percent = resp[6];
        let wired = resp[7] == 1;
        let voltage = ((resp[8] as u16) << 8) | resp[9] as u16;
        if percent > 100 {
            continue;
        }
        let state = if wired && percent >= 100 {
            BatteryState::Full
        } else if wired {
            BatteryState::Charging
        } else {
            BatteryState::Discharging
        };
        return Ok(RawReading {
            percent,
            state,
            voltage_mv: (voltage > 2000 && voltage < 6000).then_some(voltage),
            source: "Nordic 52840 · Report 0x08 / 子命令 0x04".into(),
        });
    }
    Err("Nordic 52840 无有效回复".into())
}

/// Nordic 54L15：64 字节，命令 0x72，电压小端
pub fn read_54(api: &HidApi, iface: &HidInterface, timeout_ms: i32) -> Result<RawReading, String> {
    let h: HidDevice = open(api, &iface.path)?;
    drain(&h, 8);
    let len = if iface.output_len >= 64 { iface.output_len as usize } else { 64 };
    let mut req = vec![0u8; len];
    req[0] = 0x08;
    req[1] = 0x7d;
    req[2] = 0x72;
    req[3] = 0x02;
    req[4] = 0x00;
    req[5] = 0x00;
    req[6] = 0x07;
    req[7] = 0x01;
    h.write(&req).map_err(|e| format!("写入失败: {e}"))?;
    for resp in read_reports(&h, 4, timeout_ms) {
        if resp.len() < 10 || resp[0] != 0x08 || resp[2] != 0x72 {
            continue;
        }
        if resp[2] == 0x01 {
            continue;
        }
        let percent = resp[7];
        if percent > 100 {
            continue;
        }
        let voltage = (resp[8] as u16) | ((resp[9] as u16) << 8);
        let wired = resp[5] == 0;
        let state = if wired && percent >= 100 {
            BatteryState::Full
        } else if wired {
            BatteryState::Charging
        } else {
            BatteryState::Discharging
        };
        return Ok(RawReading {
            percent,
            state,
            voltage_mv: (voltage > 2000 && voltage < 6000).then_some(voltage),
            source: "Nordic 54L15 · 命令 0x72".into(),
        });
    }
    Err("Nordic 54L15 无有效回复".into())
}