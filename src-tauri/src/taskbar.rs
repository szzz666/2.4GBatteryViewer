//! 任务栏形态：把无边框置顶窗口停靠在任务栏右侧的空白处。
//!
//! 历史：最早用 SetParent 把窗口塞进 ReBarWindow32 当真正的子窗口，
//! 但 Explorer 重启会连带销毁它，而 Tauri 不允许用同一个 label 重建 webview，
//! 结果药丸永久消失。现在改为顶层置顶窗口 + 屏幕坐标停靠：
//! 不属于 Explorer 窗口树，Explorer 怎么重启都不影响，位置由看门狗持续校正。
use std::sync::atomic::{AtomicIsize, Ordering};

use tauri::{AppHandle, Manager};

use crate::model::AppSettings;
use crate::winapi as w;

/// 最近一次停靠时任务栏窗口的句柄（只用于健康检查）
static DOCKED_ON: AtomicIsize = AtomicIsize::new(0);

pub fn is_embedded() -> bool {
    DOCKED_ON.load(Ordering::SeqCst) != 0
}

/// 计算药丸在屏幕上的矩形：贴任务栏右侧、垂直居中于任务栏
fn dock_rect(width: i32) -> (i32, i32, i32, i32) {
    let width = width.max(120);
    let shell = w::taskbar_hwnd();
    let sr = w::window_rect(shell);
    if sr.right <= sr.left || sr.bottom <= sr.top {
        // 任务栏暂时不可用（Explorer 正在重启）—— 先放到屏幕底部右侧
        let sw = unsafe { w::GetSystemMetrics(w::SM_CXSCREEN) };
        let sh = unsafe { w::GetSystemMetrics(w::SM_CYSCREEN) };
        let x = (if sw > 0 { sw } else { 1920 } - width - 320).max(0);
        let y = (if sh > 0 { sh } else { 1080 } - 44).max(0);
        return (x, y, width, 32);
    }
    let bar_h = sr.bottom - sr.top;
    let h = (bar_h - 10).clamp(20, 40);

    let tray = unsafe { w::FindWindowExW(shell, 0, w::wide("TrayNotifyWnd").as_ptr(), std::ptr::null()) };
    let tr = w::window_rect(tray);
    let right_edge = if tr.left > sr.left && tr.left < sr.right {
        tr.left
    } else {
        sr.right - 260
    };
    // 无背景，只显示内容：贴到托盘边上，只留 3px 呼吸感
    let x = (right_edge - width - 3).max(sr.left);
    let y = sr.top + (bar_h - h) / 2;
    (x, y, width, h)
}

/// 把窗口停靠到任务栏右侧
pub fn dock(app: &AppHandle, width: i32) {
    let Some(win) = app.get_webview_window("taskbar") else {
        return;
    };
    if !win.is_visible().unwrap_or(false) {
        // 药丸被关掉（隐藏）时不要擅自 show 出来
        return;
    }
    let Ok(h) = win.hwnd() else { return };
    let hwnd = h.0 as isize;
    if hwnd == 0 || unsafe { w::IsWindow(hwnd) } == 0 {
        return;
    }
    let (x, y, cx, cy) = dock_rect(width);
    unsafe {
        w::SetWindowPos(
            hwnd,
            w::HWND_TOPMOST,
            x,
            y,
            cx,
            cy,
            w::SWP_NOACTIVATE | w::SWP_SHOWWINDOW,
        );
    }
    let _ = win.show();
}

/// 停靠（保持"置顶、不抢焦点"的窗口样式）
pub fn embed(app: &AppHandle, engine: &std::sync::Arc<crate::engine::Engine>) -> Result<(), String> {
    let Some(win) = app.get_webview_window("taskbar") else {
        return Err("任务栏窗口未创建".into());
    };
    let hwnd = match win.hwnd() {
        Ok(h) if !h.0.is_null() => h.0 as isize,
        _ => return Err("窗口句柄尚未就绪".into()),
    };
    if unsafe { w::IsWindow(hwnd) } == 0 {
        return Err("窗口句柄已失效".into());
    }

    let width = engine.shared.settings.read().taskbar_width.max(140);
    let (x, y, cx, cy) = dock_rect(width);

    unsafe {
        // 顶层置顶窗口：不 SetParent，避免随 Explorer 一起被销毁
        let style = w::GetWindowLongPtrW(hwnd, w::GWL_STYLE);
        w::SetWindowLongPtrW(hwnd, w::GWL_STYLE, (style | w::WS_POPUP) & !w::WS_CHILD);
        let ex = w::GetWindowLongPtrW(hwnd, w::GWL_EXSTYLE);
        w::SetWindowLongPtrW(
            hwnd,
            w::GWL_EXSTYLE,
            (ex | w::WS_EX_TOOLWINDOW | w::WS_EX_NOACTIVATE) & !w::WS_EX_APPWINDOW,
        );
        w::SetWindowPos(
            hwnd,
            w::HWND_TOPMOST,
            x,
            y,
            cx,
            cy,
            w::SWP_NOACTIVATE | w::SWP_SHOWWINDOW,
        );
    }
    DOCKED_ON.store(unsafe { w::taskbar_hwnd() }, Ordering::SeqCst);
    let _ = win.show();
    Ok(())
}

/// 解除停靠：隐藏并把标记清掉
pub fn unembed(app: &AppHandle) -> Result<(), String> {
    let Some(win) = app.get_webview_window("taskbar") else {
        return Ok(());
    };
    if let Ok(h) = win.hwnd() {
        let hwnd = h.0 as isize;
        if hwnd != 0 && unsafe { w::IsWindow(hwnd) } != 0 {
            unsafe {
                let ex = w::GetWindowLongPtrW(hwnd, w::GWL_EXSTYLE);
                w::SetWindowLongPtrW(hwnd, w::GWL_EXSTYLE, ex & !w::WS_EX_NOACTIVATE);
            }
        }
    }
    DOCKED_ON.store(0, Ordering::SeqCst);
    let _ = win.hide();
    Ok(())
}

/// 设置变化后重新摆位（顶层窗口用屏幕坐标，之前误用了子窗口的相对坐标，
/// 结果药丸被挪到屏幕左上角，看着就像"任务栏显示功能坏了"）
pub fn relayout(app: &AppHandle, settings: &AppSettings) {
    if !is_embedded() {
        return;
    }
    let parent = DOCKED_ON.load(Ordering::SeqCst);
    if parent == 0 || unsafe { w::IsWindow(parent) } == 0 {
        // 任务栏换了（Explorer 重启），交给 ensure_embedded 重新停靠
        return;
    }
    dock(app, settings.taskbar_width.max(140));
}

/// 看门狗：任务栏位置/尺寸变化或窗口被移动后重新贴回去
pub fn ensure_embedded(app: &AppHandle, engine: &std::sync::Arc<crate::engine::Engine>) {
    if !engine.shared.settings.read().taskbar_enabled {
        return;
    }
    let Some(win) = app.get_webview_window("taskbar") else {
        return;
    };
    let hwnd = match win.hwnd() {
        Ok(h) if !h.0.is_null() => h.0 as isize,
        _ => return,
    };
    if unsafe { w::IsWindow(hwnd) } == 0 {
        return;
    }

    let settings = engine.shared.settings.read().clone();
    let (x, y, cx, cy) = dock_rect(settings.taskbar_width.max(140));
    let r = w::window_rect(hwnd);
    let drift = (r.left - x).abs() + (r.top - y).abs() + (r.right - r.left - cx).abs();
    if drift > 6 || !win.is_visible().unwrap_or(false) {
        unsafe {
            w::SetWindowPos(
                hwnd,
                w::HWND_TOPMOST,
                x,
                y,
                cx,
                cy,
                w::SWP_NOACTIVATE | w::SWP_SHOWWINDOW,
            );
        }
        let _ = win.show();
        DOCKED_ON.store(unsafe { w::taskbar_hwnd() }, Ordering::SeqCst);
    } else {
        // 位置没变也不能不管：Explorer 重启后新任务栏是后创建的，会盖在药丸上面，
        // 表现为"药丸明明在，但屏幕上什么都没有"。每个周期重新抬到最顶层。
        unsafe {
            w::SetWindowPos(
                hwnd,
                w::HWND_TOPMOST,
                0,
                0,
                0,
                0,
                w::SWP_NOMOVE | w::SWP_NOSIZE | w::SWP_NOACTIVATE,
            );
        }
    }
}
