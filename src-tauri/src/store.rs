//! 设置持久化：保存到 %APPDATA%/<identifier>/settings.json
use std::path::PathBuf;
use std::sync::OnceLock;

use crate::model::AppSettings;

fn settings_path() -> PathBuf {
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("2.4GBatteryViewer").join("settings.json")
}

fn lock_path() -> PathBuf {
    settings_path().with_file_name("instance.lock")
}

/// 串行化所有写盘操作：轮询线程（改任务栏宽度）和 UI 线程（保存设置）可能
/// 同时写同一个文件，并发写会把 JSON 写坏。
fn write_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

/// 单实例锁：成功拿到句柄说明本进程是唯一实例。
///
/// 用文件独占句柄（不删除文件），进程退出时 Windows 自动释放，
/// 不会因为残留文件导致下次启动被误判。
pub fn acquire_single_instance() -> Option<std::fs::File> {
    let path = lock_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    // 用 Win32 CreateFileW 以独占方式打开（dwShareMode = 0）
    let wide = crate::winapi::wide(&path.to_string_lossy());
    let h = unsafe {
        crate::winapi::CreateFileW(
            wide.as_ptr(),
            crate::winapi::FILE_GENERIC_WRITE,
            0, // 不共享 -> 独占
            std::ptr::null_mut(),
            crate::winapi::OPEN_ALWAYS,
            crate::winapi::FILE_ATTRIBUTE_NORMAL,
            0,
        )
    };
    if h == crate::winapi::INVALID_HANDLE_VALUE || h == 0 {
        return None;
    }
    // 用 File 接管句柄，进程退出时自动关闭
    Some(unsafe { <std::fs::File as std::os::windows::io::FromRawHandle>::from_raw_handle(h as *mut _) })
}

pub fn load() -> AppSettings {
    let path = settings_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str(text.trim_start_matches('\u{feff}').trim()) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[store] 解析设置失败: {e}（文件 {}），使用默认值", path.display());
                AppSettings::default()
            }
        },
        Err(e) => {
            eprintln!("[store] 读取设置失败: {e}，使用默认值");
            AppSettings::default()
        }
    }
}

/// 原子保存：先写临时文件再替换，避免写到一半断电/被杀留下半个 JSON。
pub fn save(settings: &AppSettings) -> Result<(), String> {
    let _guard = write_lock().lock().unwrap_or_else(|x| x.into_inner());
    let path = settings_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|e| format!("序列化设置失败: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text.as_bytes()).map_err(|e| format!("写入设置失败: {e}"))?;
    if std::fs::rename(&tmp, &path).is_err() {
        // 某些情况下（杀软占用）rename 失败，退化为直接覆盖
        let _ = std::fs::remove_file(&path);
        std::fs::rename(&tmp, &path).map_err(|e2| format!("保存设置失败: {e2}"))?;
    }
    Ok(())
}

/// 删除配置文件（恢复出厂设置）
pub fn reset() -> Result<(), String> {
    let _guard = write_lock().lock().unwrap_or_else(|x| x.into_inner());
    let path = settings_path();
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("删除设置失败: {e}"))?;
    }
    Ok(())
}
