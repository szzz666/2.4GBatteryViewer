//! AULA（狼蛛）协议 —— 逆向自官方网页驱动 https://hub.aulacn.com/mouse/keySet
//!
//! ## 鼠标 / 接收器（usage page 0xFFA8，OUT report 0x0B，IN report 0x0A）
//!
//! 网页驱动**不使用** 0x55/0xAA 长帧取电量，而是两条短命令：
//!
//!   发送  sendReport(0x0B, [0x81, 0x00])      // 0x81 = 0x01 | 0x80
//!   接收  inputreport(0x0A): [0x81, len, payload...]
//!
//! payload 布局（len = 0x2F）：
//!   idx  0..14  设备信息（MAC、型号等）
//!   idx 15      电量百分比（71 表示 71%）
//!   idx 16      充电标志（0 = 放电，非 0 = 充电中）
//!   idx 17..    保留
//!
//! 其余已确认命令：
//!   0xA0 -> 0x13 字节 固件版本
//!   0x83 -> 0x16 字节 设备能力位
//!   0x85 -> 0x27 字节 DPI / 性能
//!   0xC4 / 0xD0 -> 短状态回包
//!
//! ## 键盘 / 耳机（部分型号 usage page 0xFF90）
//! report 0x55、63 字节，payload[0]=0x65 查询电量，
//! 回包 [0x65, 电量, 状态]，状态 0x03/0x04 视为充电。
use hidapi::HidApi;

use super::{drain, open, read_reports, RawReading};
use crate::model::{BatteryState, HidInterface};

const MOUSE_OUT_RID: u8 = 0x0b;
const MOUSE_IN_RID: u8 = 0x0a;
const CMD_INFO_BATTERY: u8 = 0x81;
const CMD_FIRMWARE: u8 = 0xa0;
const CMD_CAPS: u8 = 0x83;
const CMD_DPI: u8 = 0x85;
const CMD_STATUS_C4: u8 = 0xc4;
const CMD_STATUS_D0: u8 = 0xd0;

const OFF_BATTERY: usize = 15;
const OFF_CHARGING: usize = 16;

const REPORT_ID_KB: u8 = 0x55;
const REPORT_LEN_KB: usize = 63;
const CMD_KB_BATTERY: u8 = 0x65;

pub fn pick_mouse(ifaces: &[HidInterface]) -> Option<&HidInterface> {
    ifaces
        .iter()
        .find(|i| i.usage_page == 0xffa8)
        .or_else(|| ifaces.iter().find(|i| i.usage_page == 0xff90))
}

pub fn pick_keyboard(ifaces: &[HidInterface]) -> Option<&HidInterface> {
    ifaces.iter().find(|i| i.usage_page == 0xff90)
}

fn transact(
    dev: &hidapi::HidDevice,
    payload: &[u8],
    expect_cmd: u8,
    timeout_ms: i32,
) -> Option<Vec<u8>> {
    let mut buf = vec![MOUSE_OUT_RID];
    buf.extend_from_slice(payload);
    dev.write(&buf).ok()?;
    for resp in read_reports(dev, 6, timeout_ms) {
        let body = if resp.first() == Some(&MOUSE_IN_RID) {
            &resp[1..]
        } else {
            &resp[..]
        };
        if body.len() >= 2 && body[0] == expect_cmd {
            let len = body[1] as usize;
            let start = 2;
            let end = (start + len).min(body.len());
            return Some(body[start..end].to_vec());
        }
    }
    None
}

pub fn read_mouse(api: &HidApi, iface: &HidInterface, timeout_ms: i32) -> Result<RawReading, String> {
    let dev = open(api, &iface.path)?;
    drain(&dev, 6);

    let payload = transact(&dev, &[CMD_INFO_BATTERY, 0x00], CMD_INFO_BATTERY, timeout_ms)
        .ok_or_else(|| "AULA 接收器未回应电量查询（0x81）".to_string())?;

    if payload.len() <= OFF_BATTERY {
        return Err(format!("AULA 电量回包过短（{} 字节）", payload.len()));
    }
    let pct = payload[OFF_BATTERY];
    if pct > 100 {
        return Err(format!("AULA 电量值异常：{pct}"));
    }
    let charging = payload.get(OFF_CHARGING).copied().unwrap_or(0) != 0;
    let state = if charging {
        if pct >= 100 { BatteryState::Full } else { BatteryState::Charging }
    } else if pct >= 100 {
        BatteryState::Full
    } else {
        BatteryState::Discharging
    };

    Ok(RawReading {
        percent: pct,
        state,
        voltage_mv: None,
        source: "AULA 网页驱动 · OUT 0x0B / IN 0x0A · CMD 0x81".into(),
    })
}

pub fn read_keyboard(api: &HidApi, iface: &HidInterface, timeout_ms: i32) -> Result<RawReading, String> {
    let dev = open(api, &iface.path)?;
    drain(&dev, 6);
    let mut req = vec![0u8; REPORT_LEN_KB];
    req[0] = CMD_KB_BATTERY;
    let frame = [&[REPORT_ID_KB][..], &req[..]].concat();
    if dev.send_feature_report(&frame).is_err() {
        dev.write(&frame).map_err(|e| format!("写入失败: {e}"))?;
    }
    for resp in read_reports(&dev, 6, timeout_ms) {
        let body = if resp.first() == Some(&REPORT_ID_KB) { &resp[1..] } else { &resp[..] };
        if body.len() >= 3 && body[0] == CMD_KB_BATTERY {
            let pct = body[1];
            let status = body[2];
            if pct > 100 {
                continue;
            }
            let state = match status {
                0x03 | 0x04 => BatteryState::Charging,
                _ if pct >= 100 => BatteryState::Full,
                _ => BatteryState::Discharging,
            };
            return Ok(RawReading {
                percent: pct,
                state,
                voltage_mv: None,
                source: "AULA WebHID · Report 0x55 / Cmd 0x65".into(),
            });
        }
    }
    Err("AULA 设备无电量应答".into())
}

pub fn read(api: &HidApi, iface: &HidInterface, timeout_ms: i32) -> Result<RawReading, String> {
    if iface.usage_page == 0xff90 {
        read_keyboard(api, iface, timeout_ms)
    } else {
        read_mouse(api, iface, timeout_ms)
    }
}
