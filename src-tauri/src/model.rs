//! 共享数据模型：命令返回给前端的结构体。
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 单个 HID 接口（一个 HID collection 路径）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HidInterface {
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub serial: Option<String>,
    pub usage_page: u16,
    pub usage: u16,
    pub interface_number: i32,
    pub input_len: u16,
    pub output_len: u16,
    pub feature_len: u16,
    pub report_ids: Vec<u8>,
    /// 从注册表读到的设备实例 ID（USB\VID_...），用于归组
    pub instance_id: Option<String>,
    /// 物理父设备描述，例如 "AULA Receiver"
    pub bus_reported_desc: Option<String>,
}

/// 把若干 HID 接口聚合成“一台外设”。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub key: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub name: String,
    pub manufacturer: Option<String>,
    pub kind: String,
    pub interfaces: Vec<HidInterface>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BatteryState {
    Discharging,
    Charging,
    Full,
    Unknown,
}

/// 一次电量读数。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    /// 设备唯一 key（与 DeviceInfo.key 对应）
    pub key: String,
    /// 0-100，读不到时 None
    pub percent: Option<u8>,
    pub state: BatteryState,
    /// 电压 mV（部分协议提供）
    pub voltage_mv: Option<u16>,
    /// 数据来源，例如 "HID++ 2.0 · 0x1004"
    pub source: String,
    /// 协议内部标识，例如 "logitech", "razer", "nordic52", "mchose-cm", "ikal"
    pub protocol: String,
    /// 状态描述（用于 UI 展示，例如“休眠无响应”）
    pub note: Option<String>,
    pub updated_at: i64,
    pub latency_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSnapshot {
    pub device: DeviceInfo,
    pub reading: Option<Reading>,
    /// 用户在设置里隐藏了这台设备（仍然保留在列表里，方便随时恢复）
    pub hidden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// 刷新间隔（毫秒）
    pub refresh_ms: u64,
    /// 开机自启
    pub autostart: bool,
    /// 是否显示悬浮框
    pub float_visible: bool,
    /// 悬浮框位置
    pub float_x: i32,
    pub float_y: i32,
    /// 悬浮框锁定（锁定后不可拖动）
    pub float_locked: bool,
    /// 主题: "auto" | "dark" | "light"
    pub theme: String,
    /// 手动标定的协议覆盖: deviceKey -> protocolId
    pub overrides: BTreeMap<String, String>,
    /// 隐藏的设备 key
    pub hidden: Vec<String>,
    /// 低电量告警阈值
    pub warn_percent: u8,

    // ---------- 任务栏形态 ----------
    /// 任务栏形态是否启用
    pub taskbar_enabled: bool,
    /// 任务栏窗口宽度（像素，随内容自适应）
    pub taskbar_width: i32,
    /// 任务栏显示的设备 key（空 = 显示所有能读到电量的设备）
    pub taskbar_devices: Vec<String>,
    /// 任务栏字号档位: "small" | "normal" | "large"
    pub taskbar_size: String,

    // ---------- 悬浮框 ----------
    /// 悬浮框整体缩放倍数（0.8 ~ 1.8）
    pub float_zoom: f64,
    /// 悬浮框背景不透明度（0.15 ~ 1.0）
    pub float_opacity: f64,
    /// 悬浮框显示的设备 key（空 = 全部）
    pub float_devices: Vec<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            refresh_ms: 30_000,
            autostart: false,
            float_visible: false,
            float_x: -1,
            float_y: -1,
            float_locked: false,
            theme: "auto".into(),
            overrides: BTreeMap::new(),
            hidden: Vec::new(),
            warn_percent: 15,
            taskbar_enabled: false,
            taskbar_width: 240,
            taskbar_devices: Vec::new(),
            taskbar_size: "normal".into(),
            float_zoom: 1.0,
            float_opacity: 0.72,
            float_devices: Vec::new(),
        }
    }
}

/// 调试：Feature Report 读写结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeatureResult {
    pub ok: bool,
    pub data: Vec<u8>,
    pub error: Option<String>,
    pub elapsed_ms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub ok: bool,
    pub sent: Vec<u8>,
    pub received: Vec<Vec<u8>>,
    pub error: Option<String>,
    pub elapsed_ms: u32,
}

/// 调试：Report ID 探测结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportIdProbe {
    pub report_id: u8,
    pub feature_len: usize,
    pub non_zero: bool,
    pub data: Vec<u8>,
}

/// 调试：对某个协议做一次试读
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolTry {
    pub protocol: String,
    pub name: String,
    pub ok: bool,
    pub percent: Option<u8>,
    pub state: BatteryState,
    pub voltage_mv: Option<u16>,
    pub detail: String,
    pub raw: Vec<String>,
    pub elapsed_ms: u32,
}