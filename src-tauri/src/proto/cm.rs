//! C-Media 系 0x55/0x65 协议，以及 MCHOSE、Realtek 推送式报告。
//!
//! 关键：不少 C-Media 固件（例如 iKF-V11 Pro）不会产生中断上报，
//! 必须用 HidD_GetInputReport 主动轮询，且报告 ID 是 0xFD（控制）/0xBC（状态）。
use hidapi::HidApi;

use super::{drain, open, read_reports, RawReading};
use crate::hidwin;
use crate::model::{BatteryState, HidInterface};

/// C-Media 通道使用的报告 ID：0xFD 控制，0xBC 状态
const RID_CTRL: u8 = 0xFD;
const RID_STATUS: u8 = 0xBC;

fn is_cmedia_like(iface: &HidInterface) -> bool {
    iface.usage_page == 0x000c && iface.output_len >= 8
        || (iface.output_len >= 17 && iface.output_len <= 34)
}

/// 用轮询方式读一次状态报告，返回 (report_id, 数据)
fn poll_status(iface: &HidInterface) -> Vec<(u8, Vec<u8>)> {
    let len = if iface.input_len > 0 {
        iface.input_len as usize
    } else {
        64
    };
    let mut out = Vec::new();
    for rid in [RID_STATUS, RID_CTRL, 0x00] {
        if let Some(data) = hidwin::poll_input_report(&iface.path, rid, len) {
            if data.iter().any(|b| *b != 0) {
                out.push((rid, data));
            }
        }
    }
    out
}

/// 从 C-Media 状态报告里解析电量。
///
/// 真正的 C-Media 回复格式是 55 65 <电量> <状态>（耳机类命令还会带子命令）。
///
/// 曾经这里还接受 55 01 开头的块，来源是 iKF-V11 Pro 的 0xBC 报告：
///   00*16 | 01 55 01 03 3C 10 01 06 | 00*8
/// 后来实测发现这条报告是**固件里的静态块**——连发 12 种不同命令、连续轮询 5 秒，
/// 它一个字节都不变，0xFD 报告同样纹丝不动。把里面的 0x3C 当成 60% 是误判，
/// 所以这里只认 55 65。
/// 识别 iKF-V11 Pro 那种"固定块"：前 16 字节全 0，接着 01 55 01 …
fn looks_like_static_block(data: &[u8]) -> bool {
    data.len() >= 20
        && data[0..16].iter().all(|b| *b == 0)
        && data[16] == 0x01
        && data[17] == 0x55
        && data[18] == 0x01
}

fn parse_cmedia(data: &[u8]) -> Option<(u8, BatteryState)> {
    for i in 0..data.len().saturating_sub(4) {
        if data[i] != 0x55 || data[i + 1] != 0x65 {
            continue;
        }
        let pct = data[i + 2];
        let status = data[i + 3];
        if pct > 100 {
            continue;
        }
        // 状态位语义（逆向自 iKF 官方应用 PanelBattery.UpdateUI 的同类实现）：
        //   3 = 充电中；26 = 电池信息丢失；其余按电量判断是否充满
        let state = match status {
            3 => BatteryState::Charging,
            26 => BatteryState::Unknown,
            _ if pct >= 100 => BatteryState::Full,
            _ => BatteryState::Discharging,
        };
        return Some((pct, state));
    }
    None
}

/// 通用 C-Media：先发 55 65 00 初始化，再发 55 65 01 查询，然后轮询状态报告。
pub fn read_cm55(api: &HidApi, iface: &HidInterface, timeout_ms: i32) -> Result<RawReading, String> {
    let cmedia = is_cmedia_like(iface);
    // 输出报告长度必须与设备描述符一致，否则 Windows 直接返回 ERROR_INVALID_PARAMETER(87)
    let len = if iface.output_len > 0 { iface.output_len as usize } else { 32 };

    // 1) 先尝试普通写入（多数设备支持）
    let mut wrote = false;
    if let Ok(h) = open(api, &iface.path) {
        drain(&h, 4);
        let mut init = vec![0u8; len];
        init[0] = RID_CTRL;
        init[1] = 0x55;
        init[2] = 0x65;
        init[3] = 0x00;
        let _ = h.write(&init);
        let mut req = init.clone();
        req[3] = 0x01;
        std::thread::sleep(std::time::Duration::from_millis(40));
        wrote = h.write(&req).is_ok();

        // 中断上报通道
        for resp in read_reports(&h, 4, timeout_ms.min(400)) {
            if let Some((pct, state)) = parse_cmedia(&resp) {
                return Ok(RawReading {
                    percent: pct,
                    state,
                    voltage_mv: None,
                    source: format!("C-Media · 报告 0x{:02X}", resp[0]),
                });
            }
        }
    }

    // 2) 用 HidD_SetOutputReport 补发（有的固件只接受这条路径）
    if !wrote && cmedia {
        let mut req = vec![0u8; len];
        req[0] = RID_CTRL;
        req[1] = 0x55;
        req[2] = 0x65;
        req[3] = 0x01;
        let _ = hidwin::set_output_report(&iface.path, &req);
        std::thread::sleep(std::time::Duration::from_millis(60));
    }

    // 3) 轮询状态报告（关键路径）
    let mut static_block = false;
    for (rid, data) in poll_status(iface) {
        if looks_like_static_block(&data) {
            static_block = true;
            continue;
        }
        if let Some((pct, state)) = parse_cmedia(&data) {
            return Ok(RawReading {
                percent: pct,
                state,
                voltage_mv: None,
                source: format!("C-Media · 轮询报告 0x{rid:02X}"),
            });
        }
    }

    if static_block {
        // 典型：iKF-V11 Pro。0xBC/0xFD 永远是同一串字节，换 12 种命令都不变，
        // 说明固件根本没实现电量查询——不是"睡眠中"，也不会自己好起来。
        return Err("该耳机固件不通过 USB 上报电量（0xBC 报告是固定数据）".into());
    }
    Err("C-Media 协议无有效回复（设备可能需先连接/唤醒）".into())
}

/// MCHOSE：64 字节 report 0x55（usage page 0xFF90）
pub fn read_mchose(api: &HidApi, iface: &HidInterface, timeout_ms: i32) -> Result<RawReading, String> {
    let h = open(api, &iface.path)?;
    drain(&h, 8);
    let len = if iface.output_len > 0 { iface.output_len as usize } else { 64 };
    let mut req = vec![0u8; len];
    req[0] = 0x55;
    req[1] = 0x65;
    req[2] = 0x01;
    h.write(&req).map_err(|e| format!("写入失败: {e}"))?;
    for resp in read_reports(&h, 6, timeout_ms) {
        if resp.len() < 4 || resp[0] != 0x55 || resp[1] != 0x65 {
            continue;
        }
        let pct = resp[2];
        if pct > 100 {
            continue;
        }
        // resp[3] 是字段 id：0x02 = 电量
        return Ok(RawReading {
            percent: pct,
            state: if pct >= 100 { BatteryState::Full } else { BatteryState::Discharging },
            voltage_mv: None,
            source: "MCHOSE · 55 65 查询".into(),
        });
    }
    Err("MCHOSE 协议无有效回复".into())
}

/// Realtek 推送式：report 0x13 / 子类型 0x1D，负载异或 0xFF
pub fn read_realtek(api: &HidApi, iface: &HidInterface, timeout_ms: i32) -> Result<RawReading, String> {
    let h = open(api, &iface.path)?;
    drain(&h, 4);
    for resp in read_reports(&h, 8, timeout_ms.max(600)) {
        if resp.len() < 6 || resp[0] != 0x13 || resp[1] != 0x1d {
            continue;
        }
        let percent = resp[5] ^ 0xff;
        let wired = (resp[4] ^ 0xff) == 1;
        if percent > 100 {
            continue;
        }
        return Ok(RawReading {
            percent,
            state: if wired { BatteryState::Charging } else { BatteryState::Discharging },
            voltage_mv: None,
            source: "Realtek · 推送 Report 0x13/0x1D".into(),
        });
    }
    Err("未收到 Realtek 推送报告".into())
}