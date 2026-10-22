//! 最小化的 Win32 FFI：只声明本程序用得到的函数，避免 windows crate 的版本差异。
#![allow(non_snake_case, non_camel_case_types, dead_code)]
use std::ffi::c_void;

pub type BOOL = i32;
pub type DWORD = u32;
pub type HANDLE = isize;
pub type HDEVINFO = isize;
pub type ULONG = u32;
pub type LSTATUS = i32;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GUID {
    pub Data1: u32,
    pub Data2: u16,
    pub Data3: u16,
    pub Data4: [u8; 8],
}

impl GUID {
    pub fn from_u128(v: u128) -> Self {
        Self {
            Data1: (v >> 96) as u32,
            Data2: (v >> 80) as u16,
            Data3: (v >> 64) as u16,
            Data4: [
                ((v >> 56) & 0xff) as u8,
                ((v >> 48) & 0xff) as u8,
                ((v >> 40) & 0xff) as u8,
                ((v >> 32) & 0xff) as u8,
                ((v >> 24) & 0xff) as u8,
                ((v >> 16) & 0xff) as u8,
                ((v >> 8) & 0xff) as u8,
                (v & 0xff) as u8,
            ],
        }
    }
}

pub const HID_GUID: GUID = GUID {
    Data1: 0x4d1e55b2,
    Data2: 0xf16f,
    Data3: 0x11cf,
    Data4: [0x88, 0xcb, 0x00, 0x11, 0x11, 0x00, 0x00, 0x30],
};

pub const DIGCF_PRESENT: DWORD = 0x02;
pub const DIGCF_DEVICEINTERFACE: DWORD = 0x10;
pub const FILE_GENERIC_READ: DWORD = 0x80000000;
pub const FILE_GENERIC_WRITE: DWORD = 0x40000000;
pub const FILE_SHARE_READ: DWORD = 0x01;
pub const FILE_SHARE_WRITE: DWORD = 0x02;
pub const OPEN_EXISTING: DWORD = 3;
pub const OPEN_ALWAYS: DWORD = 4;
pub const FILE_ATTRIBUTE_NORMAL: DWORD = 0x80;
pub const SPDRP_FRIENDLYNAME: DWORD = 0x0C;
pub const SPDRP_BUSREPORTEDDEVICEDESC: DWORD = 0x11;
pub const STATUS_SUCCESS: i32 = 0;
/// HidP_GetCaps 成功时返回 HIDP_STATUS_SUCCESS (0x00110000)，不是 0
pub const HIDP_STATUS_SUCCESS: i32 = 0x0011_0000;

#[repr(C)]
pub struct SP_DEVICE_INTERFACE_DATA {
    pub cbSize: DWORD,
    pub InterfaceClassGuid: GUID,
    pub Flags: DWORD,
    pub Reserved: usize,
}

#[repr(C)]
pub struct SP_DEVICE_INTERFACE_DETAIL_DATA_W {
    pub cbSize: DWORD,
    pub DevicePath: [u16; 1],
}

#[repr(C)]
pub struct SP_DEVINFO_DATA {
    pub cbSize: DWORD,
    pub ClassGuid: GUID,
    pub DevInst: DWORD,
    pub Reserved: usize,
}

#[repr(C)]
#[derive(Default)]
pub struct HIDD_ATTRIBUTES {
    pub Size: u32,
    pub VendorID: u16,
    pub ProductID: u16,
    pub VersionNumber: u16,
}

#[repr(C)]
pub struct HIDP_CAPS {
    pub Usage: u16,
    pub UsagePage: u16,
    pub InputReportByteLength: u16,
    pub OutputReportByteLength: u16,
    pub FeatureReportByteLength: u16,
    pub Reserved: [u16; 17],
    pub NumberLinkCollectionNodes: u16,
    pub NumberInputButtonCaps: u16,
    pub NumberInputValueCaps: u16,
    pub NumberInputDataIndices: u16,
    pub NumberOutputButtonCaps: u16,
    pub NumberOutputValueCaps: u16,
    pub NumberOutputDataIndices: u16,
    pub NumberFeatureButtonCaps: u16,
    pub NumberFeatureValueCaps: u16,
    pub NumberFeatureDataIndices: u16,
    pub Reserved2: [u16; 10],
}

impl Default for HIDP_CAPS {
    fn default() -> Self {
        unsafe { std::mem::zeroed() }
    }
}

#[link(name = "setupapi")]
extern "system" {
    pub fn SetupDiGetClassDevsW(
        ClassGuid: *const GUID,
        Enumerator: *const u16,
        hwndParent: isize,
        Flags: DWORD,
    ) -> HDEVINFO;
    pub fn SetupDiEnumDeviceInterfaces(
        DeviceInfoSet: HDEVINFO,
        DeviceInfoData: *const SP_DEVINFO_DATA,
        InterfaceClassGuid: *const GUID,
        MemberIndex: DWORD,
        DeviceInterfaceData: *mut SP_DEVICE_INTERFACE_DATA,
    ) -> BOOL;
    pub fn SetupDiGetDeviceInterfaceDetailW(
        DeviceInfoSet: HDEVINFO,
        DeviceInterfaceData: *const SP_DEVICE_INTERFACE_DATA,
        DeviceInterfaceDetailData: *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W,
        DeviceInterfaceDetailDataSize: DWORD,
        RequiredSize: *mut DWORD,
        DeviceInfoData: *mut SP_DEVINFO_DATA,
    ) -> BOOL;
    pub fn SetupDiEnumDeviceInfo(
        DeviceInfoSet: HDEVINFO,
        MemberIndex: DWORD,
        DeviceInfoData: *mut SP_DEVINFO_DATA,
    ) -> BOOL;
    pub fn SetupDiGetDeviceInstanceIdW(
        DeviceInfoSet: HDEVINFO,
        DeviceInfoData: *const SP_DEVINFO_DATA,
        DeviceInstanceId: *mut u16,
        DeviceInstanceIdSize: DWORD,
        RequiredSize: *mut DWORD,
    ) -> BOOL;
    pub fn SetupDiGetDeviceRegistryPropertyW(
        DeviceInfoSet: HDEVINFO,
        DeviceInfoData: *const SP_DEVINFO_DATA,
        Property: DWORD,
        PropertyRegDataType: *mut DWORD,
        PropertyBuffer: *mut u8,
        PropertyBufferSize: DWORD,
        RequiredSize: *mut DWORD,
    ) -> BOOL;
    pub fn SetupDiDestroyDeviceInfoList(DeviceInfoSet: HDEVINFO) -> BOOL;
}

#[link(name = "hid")]
extern "system" {
    pub fn HidD_GetHidGuid(HidGuid: *mut GUID);
    pub fn HidD_GetAttributes(DeviceObject: HANDLE, Attributes: *mut HIDD_ATTRIBUTES) -> BOOL;
    pub fn HidD_GetPreparsedData(DeviceObject: HANDLE, PreparsedData: *mut *mut c_void) -> BOOL;
    pub fn HidD_FreePreparsedData(PreparsedData: *mut c_void) -> BOOL;
    pub fn HidP_GetCaps(PreparsedData: *mut c_void, Capabilities: *mut HIDP_CAPS) -> i32;
    pub fn HidD_GetProductString(DeviceObject: HANDLE, Buffer: *mut c_void, BufferLength: u32) -> BOOL;
    pub fn HidD_GetManufacturerString(DeviceObject: HANDLE, Buffer: *mut c_void, BufferLength: u32) -> BOOL;
    pub fn HidD_GetSerialNumberString(DeviceObject: HANDLE, Buffer: *mut c_void, BufferLength: u32) -> BOOL;
    /// 轮询式读取输入报告（部分厂商固件只响应这个，不产生中断上报）
    pub fn HidD_GetInputReport(DeviceObject: HANDLE, ReportBuffer: *mut c_void, ReportBufferLength: u32) -> BOOL;
    pub fn HidD_SetOutputReport(DeviceObject: HANDLE, ReportBuffer: *const c_void, ReportBufferLength: u32) -> BOOL;
    pub fn HidD_GetFeature(DeviceObject: HANDLE, ReportBuffer: *mut c_void, ReportBufferLength: u32) -> BOOL;
    pub fn HidD_SetFeature(DeviceObject: HANDLE, ReportBuffer: *const c_void, ReportBufferLength: u32) -> BOOL;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn CreateFileW(
        lpFileName: *const u16,
        dwDesiredAccess: DWORD,
        dwShareMode: DWORD,
        lpSecurityAttributes: *mut c_void,
        dwCreationDisposition: DWORD,
        dwFlagsAndAttributes: DWORD,
        hTemplateFile: HANDLE,
    ) -> HANDLE;
    pub fn CloseHandle(hObject: HANDLE) -> BOOL;
    pub fn GetLastError() -> DWORD;
}

/// 任务栏嵌入所需的 user32 接口
#[link(name = "user32")]
extern "system" {
    pub fn FindWindowW(lpClassName: *const u16, lpWindowName: *const u16) -> isize;
    pub fn FindWindowExW(hWndParent: isize, hWndChildAfter: isize, lpszClass: *const u16, lpszWindow: *const u16) -> isize;
    pub fn SetParent(hWndChild: isize, hWndNewParent: isize) -> isize;
    pub fn SetWindowLongPtrW(hWnd: isize, nIndex: i32, dwNewLong: isize) -> isize;
    pub fn GetWindowLongPtrW(hWnd: isize, nIndex: i32) -> isize;
    pub fn GetParent(hWnd: isize) -> isize;
    pub fn GetClassNameW(hWnd: isize, lpClassName: *mut u16, nMaxCount: i32) -> i32;
    pub fn SetWindowPos(
        hWnd: isize,
        hWndInsertAfter: isize,
        X: i32,
        Y: i32,
        cx: i32,
        cy: i32,
        uFlags: u32,
    ) -> BOOL;
    pub fn GetWindowRect(hWnd: isize, lpRect: *mut RECT) -> BOOL;
    pub fn IsWindow(hWnd: isize) -> BOOL;
    pub fn ShowWindow(hWnd: isize, nCmdShow: i32) -> BOOL;
    pub fn GetSystemMetrics(nIndex: i32) -> i32;
    pub fn MessageBoxW(hWnd: isize, lpText: *const u16, lpCaption: *const u16, uType: u32) -> i32;
}

/// MessageBox 按钮/图标
pub const MB_OK: u32 = 0x0000_0000;
pub const MB_ICONINFORMATION: u32 = 0x0000_0040;
pub const MB_SETFOREGROUND: u32 = 0x0001_0000;

/// GetSystemMetrics 索引
pub const SM_CXSCREEN: i32 = 0;
pub const SM_CYSCREEN: i32 = 1;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

pub const GWLP_HWNDPARENT: i32 = -8;
pub const GWL_STYLE: i32 = -16;
pub const GWL_EXSTYLE: i32 = -20;
pub const WS_CHILD: isize = 0x4000_0000;
pub const WS_POPUP: isize = 0x8000_0000;
pub const WS_EX_TOOLWINDOW: isize = 0x0000_0080;
pub const WS_EX_NOACTIVATE: isize = 0x0800_0000;
pub const WS_EX_APPWINDOW: isize = 0x0004_0000;
pub const HWND_TOP: isize = 0;
pub const HWND_TOPMOST: isize = -1;
pub const SWP_NOSIZE: u32 = 0x0001;
pub const SWP_NOMOVE: u32 = 0x0002;
pub const SWP_NOACTIVATE: u32 = 0x0010;
pub const SWP_SHOWWINDOW: u32 = 0x0040;
pub const SWP_NOZORDER: u32 = 0x0004;
pub const SWP_FRAMECHANGED: u32 = 0x0020;

/// Windows 10 经典任务栏句柄
pub fn taskbar_hwnd() -> isize {
    unsafe { FindWindowW(wide("Shell_TrayWnd").as_ptr(), std::ptr::null()) }
}

/// 任务栏右侧的 ReBar（Win10 用来承载 Deskband / 工具栏）
pub fn taskbar_rebar() -> isize {
    let shell = taskbar_hwnd();
    if shell == 0 {
        return 0;
    }
    unsafe {
        FindWindowExW(
            shell,
            0,
            wide("ReBarWindow32").as_ptr(),
            std::ptr::null(),
        )
    }
}

/// Win11 的任务栏内容容器（嵌入失败时的兜底）
pub fn taskbar_win11_content() -> isize {
    let shell = taskbar_hwnd();
    if shell == 0 {
        return 0;
    }
    unsafe {
        let c = FindWindowExW(shell, 0, wide("Windows.UI.Composition.DesktopWindowContentBridge").as_ptr(), std::ptr::null());
        if c != 0 {
            return c;
        }
        FindWindowExW(shell, 0, wide("Windows.UI.Input.InputSite.WindowClass").as_ptr(), std::ptr::null())
    }
}

/// 注册 "TaskbarCreated" 消息，Explorer 重启后可重新嵌入
pub fn register_taskbar_created() -> u32 {
    #[link(name = "user32")]
    extern "system" {
        fn RegisterWindowMessageW(lpString: *const u16) -> u32;
    }
    unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) }
}

pub fn window_rect(hwnd: isize) -> RECT {
    let mut r = RECT::default();
    if hwnd != 0 {
        unsafe {
            GetWindowRect(hwnd, &mut r);
        }
    }
    r
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn from_wide(buf: &[u16]) -> Option<String> {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    if end == 0 {
        return None;
    }
    let s = String::from_utf16_lossy(&buf[..end]).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

pub const INVALID_HANDLE_VALUE: HANDLE = -1;