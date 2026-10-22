//! 枚举本机所有 HID collection，并按 USB 父设备归组。
use std::collections::BTreeMap;

use crate::hidwin;
use crate::model::{DeviceInfo, HidInterface};

/// 设备真实类别（依据 collection 组合判断）
fn classify(ifaces: &[HidInterface], name: &str) -> String {
    let lower = name.to_lowercase();
    // 耳机优先：音频 usage 或名字里带耳机/headset
    let has_audio = ifaces.iter().any(|i| i.usage_page == 0x0b && i.usage >= 0x01 && i.usage <= 0x08);
    if has_audio
        || lower.contains("headset")
        || lower.contains("headphone")
        || lower.contains("耳机")
        || lower.contains("ikf-v")
        || lower.contains("(7.1)")
    {
        return "headset".into();
    }
    let has_mouse = ifaces
        .iter()
        .any(|i| i.usage_page == 0x01 && i.usage == 0x02 && i.input_len > 3);
    let has_keyboard = ifaces
        .iter()
        .any(|i| i.usage_page == 0x01 && (i.usage == 0x06 || i.usage == 0x07));
    let has_consumer = ifaces.iter().any(|i| i.usage_page == 0x0c && i.usage == 0x01);
    let vendor = ifaces.iter().filter(|i| i.usage_page >= 0xff00).count();

    // 2.4G 接收器的描述就叫 "XXX Receiver"，它自己不是设备，只是把
    // 鼠标/键盘的 collection 转发出来。这时按"第一个 collection"判断真实设备：
    // AULA 接收器的第一个 collection 就是 0x01:0x02（鼠标）。
    let is_dongle =
        lower.contains("receiver") || lower.contains("dongle") || lower.contains("接收器");
    if is_dongle {
        if let Some(first) = ifaces
            .iter()
            .min_by_key(|i| (i.interface_number, i.usage_page, i.usage))
        {
            if first.usage_page == 0x01 && first.usage == 0x02 {
                return "mouse".into();
            }
            if first.usage_page == 0x01 && (first.usage == 0x06 || first.usage == 0x07) {
                return "keyboard".into();
            }
        }
        return "receiver".into();
    }
    if has_mouse && has_keyboard {
        return "combo".into();
    }
    if has_mouse {
        return "mouse".into();
    }
    if has_keyboard {
        return "keyboard".into();
    }
    if has_consumer && vendor > 0 {
        return "headset".into();
    }
    "other".into()
}

/// 设备唯一键：同一 VID/PID + 同一物理父设备哈希
/// 形如 "3837:300d#8&6c44d3b"，哈希由 USB 端口决定。
pub fn group_key(iface: &HidInterface) -> String {
    let base = format!("{:04x}:{:04x}", iface.vendor_id, iface.product_id);
    if let Some(inst) = &iface.instance_id {
        if let Some(pos) = inst.find('\\') {
            let tail = &inst[pos + 1..];
            let parent: &str = tail.split('&').next().unwrap_or(tail);
            return format!("{base}#{parent}");
        }
        return format!("{base}#{}", inst.to_lowercase());
    }
    base
}

/// 轮询线程用的快速枚举：不探测 Feature Report ID。
///
/// 探测要对每个 collection 逐个尝试 0x00~0x40 共 65 次 HidD_GetFeature，
/// 放在每分钟一次的重扫里会让整台机器的 HID 卡顿好几秒。
pub fn scan_all() -> Vec<DeviceInfo> {
    scan_all_with_ids(false)
}

/// 调试台用的完整枚举（带 Report ID 探测，较慢）
pub fn scan_all_full() -> Vec<DeviceInfo> {
    scan_all_with_ids(true)
}

/// with_ids=false 时不探测 Feature Report（更快）
pub fn scan_all_with_ids(with_ids: bool) -> Vec<DeviceInfo> {
    let mut ifaces: Vec<HidInterface> = hidwin::enumerate_interfaces();
    if with_ids {
        // 每个 collection 要试 65 个 report id，全系统几十个 collection 会拖到几十秒；
        // 调试台只要“大概能读哪些 id”，给 3 秒总预算即可。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        for i in &mut ifaces {
            if std::time::Instant::now() > deadline {
                break;
            }
            if i.feature_len > 0 {
                i.report_ids = hidwin::probe_feature_ids(&i.path, i.feature_len);
            }
        }
    }
    let mut groups: BTreeMap<String, Vec<HidInterface>> = BTreeMap::new();
    for i in ifaces {
        groups.entry(group_key(&i)).or_default().push(i);
    }

    let mut devices = Vec::new();
    for (key, mut list) in groups {
        list.sort_by_key(|i| (i.interface_number, i.usage_page, i.usage));
        let first = &list[0];
        let name = first
            .bus_reported_desc
            .clone()
            .or_else(|| first.product.clone())
            .unwrap_or_else(|| format!("设备 {:04X}:{:04X}", first.vendor_id, first.product_id));
        let kind = classify(&list, &name);
        devices.push(DeviceInfo {
            key,
            vendor_id: first.vendor_id,
            product_id: first.product_id,
            name,
            manufacturer: first.manufacturer.clone(),
            kind,
            interfaces: list,
        });
    }
    devices.sort_by(|a, b| a.name.cmp(&b.name));
    devices
}

/// 是否值得显示电量
pub fn is_battery_candidate(dev: &DeviceInfo) -> bool {
    // 虚拟设备（GVInput、远程桌面、合成 HID）不显示
    let lower = dev.name.to_lowercase();
    if lower.contains("gvinput")
        || lower.contains("virtual")
        || lower.contains("虚拟")
        || lower.contains("root hub")
        || lower.contains("ite device")
    {
        return false;
    }
    if matches!(dev.kind.as_str(), "mouse" | "keyboard" | "combo" | "headset" | "receiver") {
        return true;
    }
    dev.interfaces
        .iter()
        .any(|i| i.usage_page >= 0xff00 && i.input_len >= 8)
}