//! 三种形态的窗口管理：简洁面板 / 悬浮框 / 任务栏 / 完整调试版
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, WebviewWindowBuilder};

use crate::engine::{Engine, EVENT_SETTINGS};

pub const MODE_PANEL: u8 = 0;
pub const MODE_FLOATING: u8 = 1;
pub const MODE_DEBUG: u8 = 2;
pub const MODE_TASKBAR: u8 = 3;

static CURRENT: AtomicU8 = AtomicU8::new(MODE_PANEL);

pub fn current_mode() -> String {
    match CURRENT.load(Ordering::SeqCst) {
        MODE_FLOATING => "floating".into(),
        MODE_DEBUG => "debug".into(),
        MODE_TASKBAR => "taskbar".into(),
        _ => "panel".into(),
    }
}

/// 把最新设置推给所有窗口
pub fn broadcast(app: &AppHandle, settings: &crate::model::AppSettings) {
    let _ = app.emit(EVENT_SETTINGS, settings);
}

pub fn switch_mode(app: &AppHandle, engine: &Arc<Engine>, mode: &str) -> Result<(), String> {
    match mode {
        "panel" => {
            ensure_window(app, "panel", "2.4G 外设电量", 460.0, 660.0, false)?;
            if let Some(w) = app.get_webview_window("panel") {
                let _ = w.unminimize();
                let _ = w.set_size(tauri::LogicalSize::new(460.0, 660.0));
                let _ = w.show();
                let _ = w.set_focus();
            }
            if let Some(w) = app.get_webview_window("debug") {
                let _ = w.hide();
            }
            CURRENT.store(MODE_PANEL, Ordering::SeqCst);
        }
        "debug" => {
            ensure_window(app, "debug", "HID 调试台", 1020.0, 720.0, false)?;
            if let Some(w) = app.get_webview_window("debug") {
                if let Ok(size) = w.inner_size() {
                    if size.width < 600 || size.height < 400 {
                        let _ = w.set_size(tauri::LogicalSize::new(1020.0, 720.0));
                    }
                }
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
            CURRENT.store(MODE_DEBUG, Ordering::SeqCst);
        }
        "floating" => {
            ensure_window(app, "float", "电量悬浮框", 260.0, 34.0, true)?;
            if let Some(w) = app.get_webview_window("float") {
                apply_float_settings(app, engine);
                let _ = w.show();
            }
            {
                let mut s = engine.shared.settings.write();
                s.float_visible = true;
                let snap = s.clone();
                drop(s);
                let _ = crate::store::save(&snap);
                broadcast(app, &snap);
            }
            CURRENT.store(MODE_FLOATING, Ordering::SeqCst);
        }
        "taskbar" => {
            // 任务栏形态：无边框小窗，随后停靠到任务栏
            ensure_taskbar_window(app, engine)?;
            let snapshot = {
                let mut s = engine.shared.settings.write();
                s.taskbar_enabled = true;
                s.clone()
            };
            let _ = crate::store::save(&snapshot);
            broadcast(app, &snapshot);
            // 窗口刚创建时原生句柄还没就绪，延迟一小会儿再停靠（失败会由看门狗重试）
            {
                let handle = app.clone();
                let eng = Arc::clone(engine);
                std::thread::spawn(move || {
                    for attempt in 0..12 {
                        std::thread::sleep(std::time::Duration::from_millis(if attempt == 0 { 300 } else { 400 }));
                        if crate::taskbar::embed(&handle, &eng).is_ok() {
                            break;
                        }
                    }
                });
            }
            CURRENT.store(MODE_TASKBAR, Ordering::SeqCst);
        }
        other => return Err(format!("未知形态 {other}")),
    }
    Ok(())
}

fn ensure_window(app: &AppHandle, label: &str, title: &str, w: f64, h: f64, floating: bool) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(label) {
        let _ = existing.show();
        return Ok(());
    }
    let url = tauri::WebviewUrl::App(format!("index.html?label={label}").into());
    let mut builder = WebviewWindowBuilder::new(app, label, url)
        .title(title)
        .inner_size(w, h)
        .min_inner_size(if floating { 120.0 } else { 360.0 }, if floating { 28.0 } else { 320.0 })
        .resizable(!floating)
        .decorations(!floating)
        .always_on_top(floating)
        // 常驻托盘：不给任何窗口占任务栏按钮，否则托盘图标 + 任务栏按钮看起来像"两个图标"
        .skip_taskbar(true)
        .transparent(floating);
    if floating {
        builder = builder.shadow(false);
    }
    builder.build().map_err(|e| e.to_string())?;
    Ok(())
}

pub fn apply_float_settings(app: &AppHandle, engine: &Arc<Engine>) {
    let settings = engine.shared.settings.read().clone();
    if let Some(w) = app.get_webview_window("float") {
        let _ = w.set_always_on_top(true);
        let _ = w.set_skip_taskbar(true);
        if settings.float_x >= 0 && settings.float_y >= 0 {
            let _ = w.set_position(tauri::PhysicalPosition::new(settings.float_x, settings.float_y));
        } else if let Ok(Some(mon)) = w.current_monitor() {
            // 首次出现：放在屏幕右上角，避免叠在主面板上
            let size = mon.size();
            let x = size.width.saturating_sub(380) as i32;
            let _ = w.set_position(tauri::PhysicalPosition::new(x, 60));
        }
    }
}

pub fn set_locked(app: &AppHandle, engine: &Arc<Engine>, locked: bool) -> Result<(), String> {
    let snapshot = {
        let mut s = engine.shared.settings.write();
        s.float_locked = locked;
        s.clone()
    };
    crate::store::save(&snapshot)?;
    if let Some(w) = app.get_webview_window("float") {
        let _ = w.set_always_on_top(true);
    }
    broadcast(app, &snapshot);
    Ok(())
}

/// 悬浮框移动后保存位置（锁定状态下不记录）
pub fn remember_float_position(engine: &Arc<Engine>, x: i32, y: i32) {
    let mut settings = engine.shared.settings.write();
    if settings.float_locked {
        return;
    }
    if settings.float_x == x && settings.float_y == y {
        return;
    }
    settings.float_x = x;
    settings.float_y = y;
    let snapshot = settings.clone();
    drop(settings);
    // 拖动时 onMoved 会高频触发，这里节流到 400ms 一次，避免狂写磁盘
    static LAST: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let now = std::time::Instant::now();
    let due = last.map(|t| now.duration_since(t).as_millis() > 400).unwrap_or(true);
    if due {
        *last = Some(now);
        let _ = crate::store::save(&snapshot);
    }
}
/// 创建/复用任务栏窗口
pub fn ensure_taskbar_window(app: &AppHandle, engine: &Arc<Engine>) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window("taskbar") {
        let alive = existing
            .hwnd()
            .map(|h| !h.0.is_null() && unsafe { crate::winapi::IsWindow(h.0 as isize) } != 0)
            .unwrap_or(false);
        if alive {
            return Ok(());
        }
    }

    let width = engine.shared.settings.read().taskbar_width.max(140);
    let url = tauri::WebviewUrl::App("index.html?label=taskbar".into());
    WebviewWindowBuilder::new(app, "taskbar", url)
        .title("2.4G 任务栏电量")
        .inner_size(width as f64, 32.0)
        .resizable(false)
        .decorations(false)
        // 顶层置顶窗口：不依附 Explorer，所以 Explorer 重启不会把它带走
        .always_on_top(true)
        .skip_taskbar(true)
        .transparent(true)
        .shadow(false)
        .focused(false)
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 关闭任务栏形态：只把药丸收起来，不再顺手弹出主面板
pub fn disable_taskbar(app: &AppHandle, engine: &Arc<Engine>) -> Result<(), String> {
    let snapshot = {
        let mut s = engine.shared.settings.write();
        s.taskbar_enabled = false;
        s.clone()
    };
    crate::store::save(&snapshot)?;
    let _ = crate::taskbar::unembed(app);
    if let Some(w) = app.get_webview_window("taskbar") {
        let _ = w.hide();
    }
    if current_mode() == "taskbar" {
        CURRENT.store(MODE_PANEL, Ordering::SeqCst);
    }
    broadcast(app, &snapshot);
    Ok(())
}
/// 关闭悬浮框（同时记住“下次启动不自动显示”）
pub fn hide_float(app: &AppHandle, engine: &Arc<Engine>) {
    if let Some(w) = app.get_webview_window("float") {
        let _ = w.hide();
    }
    let snapshot = {
        let mut s = engine.shared.settings.write();
        s.float_visible = false;
        s.clone()
    };
    let _ = crate::store::save(&snapshot);
    broadcast(app, &snapshot);
}

/// 启动时按设置恢复上次的形态（命令行没有显式指定时）
///
/// show_panel=false 用于 --silent（开机自启）：只恢复任务栏药丸和悬浮框，
/// 不弹主面板。
pub fn restore_on_launch(app: &AppHandle, engine: &Arc<Engine>, show_panel: bool) {
    let settings = engine.shared.settings.read().clone();
    if settings.taskbar_enabled {
        // 用 ensure_taskbar_visible：它内部带重试，不会因为句柄没就绪就整体失败
        ensure_taskbar_visible(app, engine);
    }
    if settings.float_visible {
        let _ = switch_mode(app, engine, "floating");
    }
    // 面板最后再显示一次，确保它没有被 restore 过程隐藏
    if show_panel {
        if let Some(win) = app.get_webview_window("panel") {
            let _ = win.show();
        }
    }
    broadcast(app, &settings);
}
/// 确保任务栏药丸存在且已停靠（不改变“当前形态”记录）
pub fn ensure_taskbar_visible(app: &AppHandle, engine: &Arc<Engine>) {
    if ensure_taskbar_window(app, engine).is_err() {
        return;
    }
    if let Some(w) = app.get_webview_window("taskbar") {
        let _ = w.show();
    }
    let snapshot = {
        let mut s = engine.shared.settings.write();
        s.taskbar_enabled = true;
        s.clone()
    };
    let _ = crate::store::save(&snapshot);
    let handle = app.clone();
    let eng = Arc::clone(engine);
    std::thread::spawn(move || {
        for attempt in 0..12 {
            std::thread::sleep(std::time::Duration::from_millis(if attempt == 0 { 300 } else { 400 }));
            if crate::taskbar::embed(&handle, &eng).is_ok() {
                break;
            }
        }
    });
}
