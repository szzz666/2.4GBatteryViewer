//! SteelSeries / HyperX / Corsair 耳机电量协议。
use hidapi::HidApi;

use super::{drain, open, read_reports, RawReading};
use crate::model::{BatteryState, HidInterface};

fn write_and_read(api: &HidApi, iface: &HidInterface, req: &[u8], len: usize, timeout_ms: i32) -> Result<Vec<Vec<u8>>, String> {
    let h = open(api, &iface.path)?;
    drain(&h, 4);
    let mut buf = vec![0u8; len];
    let n = req.len().min(len);
    buf[..n].copy_from_slice(&req[..n]);
    h.write(&buf).map_err(|e| format!("写入失败: {e}"))?;
    Ok(read_reports(&h, 4, timeout_ms))
}

/// SteelSeries Arctis：先试 06 18（31 字节），再试 00 B0 / 06 B0
pub fn read(api: &HidApi, iface: &HidInterface, timeout_ms: i32) -> Result<RawReading, String> {
    let mut last = String::new();
    // 旧款 Arctis 7 / Pro
    let len = iface.output_len.max(31) as usize;
    match write_and_read(api, iface, &[0x06, 0x18], len, timeout_ms) {
        Ok(reports) => {
            for r in &reports {
                if r.len() > 2 && r[2] <= 100 && r[2] > 0 {
                    return Ok(RawReading {
                        percent: r[2],
                        state: if r[2] >= 100 { BatteryState::Full } else { BatteryState::Discharging },
                        voltage_mv: None,
                        source: "SteelSeries · 06 18".into(),
                    });
                }
            }
        }
        Err(e) => last = e,
    }
    // Nova 系列
    for req in [[0x00u8, 0xb0], [0x06, 0xb0]] {
        match write_and_read(api, iface, &req, 64, timeout_ms) {
            Ok(reports) => {
                for r in &reports {
                    if r.len() < 4 {
                        continue;
                    }
                    let online = r[3];
                    if online == 0x00 {
                        continue;
                    }
                    let raw = r[2];
                    let percent = if raw <= 8 { (raw as f32 / 8.0 * 100.0).round() as u8 } else { raw.min(100) };
                    let state = if online == 0x01 || online == 0x02 {
                        BatteryState::Charging
                    } else if percent >= 100 {
                        BatteryState::Full
                    } else {
                        BatteryState::Discharging
                    };
                    if percent <= 100 {
                        return Ok(RawReading {
                            percent,
                            state,
                            voltage_mv: None,
                            source: "SteelSeries Nova · 状态查询".into(),
                        });
                    }
                }
            }
            Err(e) => last = e,
        }
    }
    Err(if last.is_empty() { "SteelSeries 无响应".into() } else { last })
}

/// HyperX Cloud II Wireless：06 FF BB 02（电量）/ 06 FF BB 03（充电）
pub fn read_hyperx(api: &HidApi, iface: &HidInterface, timeout_ms: i32) -> Result<RawReading, String> {
    let reports = write_and_read(api, iface, &[0x06, 0xff, 0xbb, 0x02, 0x00], 52, timeout_ms)?;
    for r in &reports {
        if r.len() > 7 && r[0] == 0x06 && r[1] == 0xff && r[2] == 0xbb {
            let percent = r[7];
            if percent > 100 {
                continue;
            }
            let charging = write_and_read(api, iface, &[0x06, 0xff, 0xbb, 0x03, 0x00], 52, timeout_ms)
                .ok()
                .and_then(|rs| rs.into_iter().find(|x| x.len() > 4 && x[0] == 0x06))
                .map(|x| x[4] == 1)
                .unwrap_or(false);
            return Ok(RawReading {
                percent,
                state: if charging {
                    BatteryState::Charging
                } else if percent >= 100 {
                    BatteryState::Full
                } else {
                    BatteryState::Discharging
                },
                voltage_mv: None,
                source: "HyperX · 06 FF BB".into(),
            });
        }
    }
    Err("HyperX 无有效回复".into())
}

/// Corsair：00 02 <endpoint> 02 0F，电量 16 位小端 1..1000
pub fn read_corsair(api: &HidApi, iface: &HidInterface, timeout_ms: i32) -> Result<RawReading, String> {
    for endpoint in [0x01u8, 0x00, 0x02, 0x03] {
        let req = [0x00, 0x02, endpoint, 0x02, 0x0f];
        let Ok(reports) = write_and_read(api, iface, &req, 64, timeout_ms) else {
            continue;
        };
        // 接收器可能回传配对设备 ID 而非电量，多试几轮
        for attempt in 0..3 {
            for r in &reports {
                if r.len() < 6 {
                    continue;
                }
                let raw = (r[4] as u16) | ((r[5] as u16) << 8);
                if raw == 0 || raw > 1000 {
                    continue;
                }
                let percent = ((raw as f32 / 10.0).round() as u8).min(100);
                return Ok(RawReading {
                    percent,
                    state: if percent >= 100 { BatteryState::Full } else { BatteryState::Discharging },
                    voltage_mv: None,
                    source: format!("Corsair · 端点 {endpoint} · 第 {} 次", attempt + 1),
                });
            }
        }
    }
    Err("Corsair 无有效回复".into())
}