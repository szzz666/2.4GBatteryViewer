//! HID collection 枚举：路径、usage、报告长度、USB 父设备描述。
//! 读写交由 hidapi（带超时），这里只做元数据。
use std::ffi::c_void;

use crate::model::HidInterface;
use crate::winapi::*;

pub fn enumerate_interfaces() -> Vec<HidInterface> {
    let mut out = Vec::new();
    unsafe {
        let set = SetupDiGetClassDevsW(
            &HID_GUID,
            std::ptr::null(),
            0,
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        );
        
        if set == 0 || set == -1 {
            
            return out;
        }
        let mut index = 0u32;
        loop {
            let mut iface = SP_DEVICE_INTERFACE_DATA {
                cbSize: std::mem::size_of::<SP_DEVICE_INTERFACE_DATA>() as u32,
                InterfaceClassGuid: GUID::default(),
                Flags: 0,
                Reserved: 0,
            };
            if SetupDiEnumDeviceInterfaces(set, std::ptr::null(), &HID_GUID, index, &mut iface) == 0 {
                
                break;
            }
            let this_index = index;
            index += 1;

            let mut required: DWORD = 0;
            SetupDiGetDeviceInterfaceDetailW(
                set,
                &iface,
                std::ptr::null_mut(),
                0,
                &mut required,
                std::ptr::null_mut(),
            );
            if required == 0 {
                
                continue;
            }
            let mut buffer = vec![0u8; required as usize];
            let detail = buffer.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = std::mem::size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>() as u32;
            if SetupDiGetDeviceInterfaceDetailW(
                set,
                &iface,
                detail,
                required,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ) == 0
            {
                continue;
            }
            let path_ptr = (*detail).DevicePath.as_ptr();
            let mut len = 0usize;
            while *path_ptr.add(len) != 0 {
                len += 1;
            }
            let path = String::from_utf16_lossy(std::slice::from_raw_parts(path_ptr, len));

            let Some((product, manufacturer, serial)) = read_strings(&path) else {
                
                continue;
            };
            let Some((caps, attr)) = read_caps(&path) else {
                
                continue;
            };
            let (instance_id, bus_reported_desc) = devnode_info(set, this_index);

            let mut interface_number = -1i32;
            let lower = path.to_lowercase();
            if let Some(pos) = lower.find("&mi_") {
                if let Some(hex) = lower.get(pos + 4..pos + 6) {
                    if let Ok(v) = u8::from_str_radix(hex, 16) {
                        interface_number = v as i32;
                    }
                }
            }

            out.push(HidInterface {
                path,
                vendor_id: attr.VendorID,
                product_id: attr.ProductID,
                manufacturer,
                product,
                serial,
                usage_page: caps.UsagePage,
                usage: caps.Usage,
                interface_number,
                input_len: caps.InputReportByteLength,
                output_len: caps.OutputReportByteLength,
                feature_len: caps.FeatureReportByteLength,
                report_ids: Vec::new(),
                instance_id,
                bus_reported_desc,
            });
        }
        SetupDiDestroyDeviceInfoList(set);
    }
    out
}

fn open_meta(path: &str) -> Option<HANDLE> {
    let w = wide(path);
    unsafe {
        let h = CreateFileW(
            w.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            0,
        );
        if h == INVALID_HANDLE_VALUE || h == 0 {
            None
        } else {
            Some(h)
        }
    }
}

fn read_strings(path: &str) -> Option<(Option<String>, Option<String>, Option<String>)> {
    let h = open_meta(path)?;
    let mut p = [0u16; 256];
    let mut m = [0u16; 256];
    let mut s = [0u16; 256];
    unsafe {
        let product = if HidD_GetProductString(h, p.as_mut_ptr() as *mut c_void, 512) != 0 {
            from_wide(&p)
        } else {
            None
        };
        let manufacturer = if HidD_GetManufacturerString(h, m.as_mut_ptr() as *mut c_void, 512) != 0 {
            from_wide(&m)
        } else {
            None
        };
        let serial = if HidD_GetSerialNumberString(h, s.as_mut_ptr() as *mut c_void, 512) != 0 {
            from_wide(&s)
        } else {
            None
        };
        CloseHandle(h);
        Some((product, manufacturer, serial))
    }
}

fn read_caps(path: &str) -> Option<(HIDP_CAPS, HIDD_ATTRIBUTES)> {
    // 用 0 权限句柄：系统独占设备（键盘/鼠标）拒绝读权限，但仍可读 preparsed data
    let h = open_meta(path)?;
    unsafe {
        let mut preparsed: *mut c_void = std::ptr::null_mut();
        if HidD_GetPreparsedData(h, &mut preparsed) == 0 || preparsed.is_null() {
            CloseHandle(h);
            return None;
        }
        let mut caps = HIDP_CAPS::default();
        let status = HidP_GetCaps(preparsed, &mut caps);
        HidD_FreePreparsedData(preparsed);
        if status != HIDP_STATUS_SUCCESS {
            CloseHandle(h);
            return None;
        }
        let mut attr = HIDD_ATTRIBUTES {
            Size: std::mem::size_of::<HIDD_ATTRIBUTES>() as u32,
            ..Default::default()
        };
        HidD_GetAttributes(h, &mut attr);
        CloseHandle(h);
        Some((caps, attr))
    }
}

fn devnode_info(set: HDEVINFO, idx: u32) -> (Option<String>, Option<String>) {
    unsafe {
        let mut devinfo = SP_DEVINFO_DATA {
            cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
            ClassGuid: GUID::default(),
            DevInst: 0,
            Reserved: 0,
        };
        if SetupDiEnumDeviceInfo(set, idx, &mut devinfo) == 0 {
            return (None, None);
        }
        let mut id_buf = [0u16; 512];
        let instance = if SetupDiGetDeviceInstanceIdW(
            set,
            &devinfo,
            id_buf.as_mut_ptr(),
            512,
            std::ptr::null_mut(),
        ) != 0
        {
            from_wide(&id_buf)
        } else {
            None
        };

        let desc = registry_string(set, &devinfo, SPDRP_BUSREPORTEDDEVICEDESC)
            .or_else(|| registry_string(set, &devinfo, SPDRP_FRIENDLYNAME));
        (instance, desc)
    }
}

unsafe fn registry_string(set: HDEVINFO, devinfo: &SP_DEVINFO_DATA, prop: DWORD) -> Option<String> {
    let mut buf = [0u16; 512];
    let mut needed: DWORD = 0;
    let mut ty: DWORD = 0;
    let ok = SetupDiGetDeviceRegistryPropertyW(
        set,
        devinfo,
        prop,
        &mut ty,
        buf.as_mut_ptr() as *mut u8,
        (buf.len() * 2) as u32,
        &mut needed,
    );
    if ok == 0 {
        return None;
    }
    from_wide(&buf)
}
/// 用 HidD_GetInputReport 轮询一次输入报告（某些固件只支持这种方式）。
pub fn poll_input_report(path: &str, report_id: u8, length: usize) -> Option<Vec<u8>> {
    let w = wide(path);
    unsafe {
        let handle = CreateFileW(
            w.as_ptr(),
            FILE_GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            0,
        );
        if handle == INVALID_HANDLE_VALUE || handle == 0 {
            return None;
        }
        let mut buf = vec![0u8; length.max(1)];
        buf[0] = report_id;
        let ok = HidD_GetInputReport(handle, buf.as_mut_ptr() as *mut c_void, buf.len() as u32);
        CloseHandle(handle);
        if ok == 0 {
            None
        } else {
            Some(buf)
        }
    }
}

/// 用 HidD_SetOutputReport 发送一次输出报告（不需要读写句柄权限组合）。
pub fn set_output_report(path: &str, data: &[u8]) -> bool {
    let w = wide(path);
    unsafe {
        let handle = CreateFileW(
            w.as_ptr(),
            FILE_GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            0,
        );
        if handle == INVALID_HANDLE_VALUE || handle == 0 {
            return false;
        }
        let ok = HidD_SetOutputReport(handle, data.as_ptr() as *const c_void, data.len() as u32);
        CloseHandle(handle);
        ok != 0
    }
}
/// 探测某个 collection 可读的 Feature Report ID
pub fn probe_feature_ids(path: &str, feature_len: u16) -> Vec<u8> {
    if feature_len == 0 {
        return Vec::new();
    }
    let w = wide(path);
    unsafe {
        let handle = CreateFileW(
            w.as_ptr(),
           0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            0,
        );
        if handle == INVALID_HANDLE_VALUE || handle == 0 {
            return Vec::new();
        }
        let mut ids = Vec::new();
        for rid in 0u8..=0x40 {
            let mut buf = vec![0u8; feature_len as usize];
            buf[0] = rid;
            if HidD_GetFeature(handle, buf.as_mut_ptr() as *mut c_void, buf.len() as u32) != 0 {
                ids.push(rid);
            }
        }
        CloseHandle(handle);
        ids
    }
}
