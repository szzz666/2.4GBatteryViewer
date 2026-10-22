//! 开机自启：写 HKCU\Software\Microsoft\Windows\CurrentVersion\Run
//! （当前用户级，无需管理员权限）
use tauri::AppHandle;

#[allow(non_snake_case)]
mod reg {
    use std::ffi::c_void;
    pub type HKEY = isize;
    pub type LSTATUS = i32;
    pub const HKEY_CURRENT_USER: HKEY = 0x80000001u64 as isize;
    pub const KEY_QUERY_VALUE: u32 = 0x0001;
    pub const KEY_SET_VALUE: u32 = 0x0002;
    pub const REG_SZ: u32 = 1;
    pub const REG_OPTION_NON_VOLATILE: u32 = 0;

    #[link(name = "advapi32")]
    extern "system" {
        pub fn RegCreateKeyExW(
            hKey: HKEY,
            lpSubKey: *const u16,
            Reserved: u32,
            lpClass: *const u16,
            dwOptions: u32,
            samDesired: u32,
            lpSecurityAttributes: *mut c_void,
            phkResult: *mut HKEY,
            lpdwDisposition: *mut u32,
        ) -> LSTATUS;
        pub fn RegOpenKeyExW(
            hKey: HKEY,
            lpSubKey: *const u16,
            ulOptions: u32,
            samDesired: u32,
            phkResult: *mut HKEY,
        ) -> LSTATUS;
        pub fn RegSetValueExW(
            hKey: HKEY,
            lpValueName: *const u16,
            Reserved: u32,
            dwType: u32,
            lpData: *const u8,
            cbData: u32,
        ) -> LSTATUS;
        pub fn RegQueryValueExW(
            hKey: HKEY,
            lpValueName: *const u16,
            lpReserved: *mut u32,
            lpType: *mut u32,
            lpData: *mut u8,
            lpcbData: *mut u32,
        ) -> LSTATUS;
        pub fn RegDeleteValueW(hKey: HKEY, lpValueName: *const u16) -> LSTATUS;
        pub fn RegCloseKey(hKey: HKEY) -> LSTATUS;
    }
}

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "2.4GBatteryViewer";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn open_run_key(create: bool) -> Option<reg::HKEY> {
    let mut sub = wide(RUN_KEY);
    let mut key: reg::HKEY = 0;
    unsafe {
        let status = if create {
            let mut disposition: u32 = 0;
            reg::RegCreateKeyExW(
                reg::HKEY_CURRENT_USER,
                sub.as_mut_ptr(),
                0,
                std::ptr::null(),
                reg::REG_OPTION_NON_VOLATILE,
                reg::KEY_QUERY_VALUE | reg::KEY_SET_VALUE,
                std::ptr::null_mut(),
                &mut key,
                &mut disposition,
            )
        } else {
            reg::RegOpenKeyExW(
                reg::HKEY_CURRENT_USER,
                sub.as_mut_ptr(),
                0,
                reg::KEY_QUERY_VALUE,
                &mut key,
            )
        };
        if status == 0 && key != 0 {
            Some(key)
        } else {
            None
        }
    }
}

pub fn is_enabled() -> bool {
    let Some(key) = open_run_key(false) else {
        return false;
    };
    let name = wide(VALUE_NAME);
    let mut data = vec![0u8; 2048];
    let mut size = data.len() as u32;
    let mut ty: u32 = 0;
    let status = unsafe {
        reg::RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut ty,
            data.as_mut_ptr(),
            &mut size,
        )
    };
    unsafe {
        reg::RegCloseKey(key);
    }
    status == 0 && size > 0 && ty == reg::REG_SZ
}

pub fn apply(_app: &AppHandle, enabled: bool) {
    let Some(key) = open_run_key(true) else {
        return;
    };
    let name = wide(VALUE_NAME);
    unsafe {
        if enabled {
            if let Ok(path) = std::env::current_exe() {
                let cmd = format!("\"{}\" --silent", path.display());
                let mut data: Vec<u16> = cmd.encode_utf16().chain(std::iter::once(0)).collect();
                let bytes = std::slice::from_raw_parts(
                    data.as_mut_ptr() as *const u8,
                    data.len() * 2,
                );
                let _ = reg::RegSetValueExW(
                    key,
                    name.as_ptr(),
                    0,
                    reg::REG_SZ,
                    bytes.as_ptr(),
                    bytes.len() as u32,
                );
            }
        } else {
            let _ = reg::RegDeleteValueW(key, name.as_ptr());
        }
        reg::RegCloseKey(key);
    }
}