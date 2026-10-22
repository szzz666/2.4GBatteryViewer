mod autostart;
mod commands;
mod engine;
mod hidwin;
mod model;
mod proto;
mod scan;
mod store;
mod taskbar;
pub mod winapi;
mod windows;

use std::sync::Arc;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager};

use commands::EngineState;

/// 单实例锁句柄（由 Tauri 托管，进程退出时随 state 一起析构）
struct InstanceGuard(#[allow(dead_code)] std::fs::File);
use engine::Engine;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 单实例：已经在跑就不再开第二个（否则会出现两条任务栏药丸、两个托盘图标）
    let instance_guard = match store::acquire_single_instance() {
        Some(f) => f,
        None => {
            eprintln!("[app] 已有实例在运行，本次启动退出");
            // 静默退出会让人以为程序没启动，给个提示（程序常驻托盘）
            unsafe {
                winapi::MessageBoxW(
                    0,
                    winapi::wide("程序已经在运行了，请到系统托盘里找 ⚡ 图标。").as_ptr(),
                    winapi::wide("2.4G 外设电量").as_ptr(),
                    winapi::MB_OK | winapi::MB_ICONINFORMATION | winapi::MB_SETFOREGROUND,
                );
            }
            return;
        }
    };
    // 句柄交给 Tauri 托管：进程存活期间一直持有，退出时正常释放（不能 forget，
    // 否则句柄泄漏会让进程变成无法回收的僵尸）
    let guard_state = InstanceGuard(instance_guard);

    let engine = Engine::new();
    {
        let mut s = engine.shared.settings.write();
        *s = store::load();
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(EngineState(Arc::clone(&engine)))
        .manage(guard_state)
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::rescan,
            commands::refresh_now,
            commands::get_settings,
            commands::save_settings,
            commands::list_devices,
            commands::enumerate_all,
            commands::list_protocols,
            commands::aula_diagnose,
            commands::read_feature,
            commands::send_probe,
            commands::probe_report_ids,
            commands::diagnose_device,
            commands::try_protocol,
            commands::read_input,
            commands::write_feature,
            commands::calibrate,
            commands::clear_override,
            commands::set_mode,
            commands::get_mode,
            commands::set_float_locked,
            commands::start_drag,
            commands::save_float_position,
            commands::toggle_autostart,
            commands::is_autostart_enabled,
            commands::hide_window,
            commands::show_window,
            commands::quit_app,
            commands::bump_refresh,
            commands::dump_interface,
            commands::list_interfaces,
            commands::snapshot_for,
            commands::emit_debug,
            commands::set_taskbar_enabled,
            commands::resize_taskbar,
            commands::resize_float,
            commands::close_float,
            commands::reset_settings,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            // 启动时把设置里的自启状态同步到注册表
            autostart::apply(&handle, engine.shared.settings.read().autostart);
            setup_tray(&handle)?;
            let eng = Arc::clone(&engine);
            engine::spawn_worker(handle.clone(), eng);
            let eng2 = Arc::clone(&engine);
            engine::spawn_taskbar_watchdog(handle.clone(), eng2);

            // 命令行参数：--silent 只驻留托盘；--float 启动即显示悬浮框；--debug 打开调试台
            let args: Vec<String> = std::env::args().collect();
            let explicit_form = args
                .iter()
                .any(|a| a == "--float" || a == "--debug" || a == "--taskbar");
            let silent = args.iter().any(|a| a == "--silent");
            if args.iter().any(|a| a == "--taskbar") {
                // 任务栏形态：启动后直接嵌入
                let _ = windows::switch_mode(&handle, &engine, "taskbar");
            }
            if args.iter().any(|a| a == "--silent") {
                if let Some(w) = app.get_webview_window("panel") {
                    let _ = w.hide();
                }
            }
            // 面板窗口始终确保尺寸正确（避免被 restore / 上次隐藏状态影响）
            if let Some(w) = app.get_webview_window("panel") {
                let _ = w.unminimize();
                let _ = w.set_size(tauri::LogicalSize::new(460.0, 660.0));
                let _ = w.center();
                if !silent {
                    let _ = w.show();
                }
            }
            // 没有显式指定形态时，恢复上次的形态（任务栏 / 悬浮框）
            if !explicit_form {
                windows::restore_on_launch(&handle, &engine, !silent);
            }
            // --taskbar 与 --float 可以同时给（常驻任务栏 + 顺手浮一个窗口）
            if args.iter().any(|a| a == "--float") {
                let _ = windows::switch_mode(&handle, &engine, "floating");
            }
            if args.iter().any(|a| a == "--debug") {
                let _ = windows::switch_mode(&handle, &engine, "debug");
                // 确保调试台在最前（面板窗口可能抢占焦点）
                if let Some(w) = app.get_webview_window("debug") {
                    let _ = w.show();
                    let _ = w.unminimize();
                    let _ = w.set_focus();
                }
            }
            // 任务栏是常驻形态，最后再确认一次（避免被上面 switch_mode 的模式记录覆盖）
            if args.iter().any(|a| a == "--taskbar") {
                windows::ensure_taskbar_visible(&handle, &engine);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let label = window.label().to_string();
                // 任务栏药丸是 Explorer 的子窗口，隐藏/销毁会打断嵌入，这里只吞掉关闭请求
                if label == "taskbar" {
                    api.prevent_close();
                    return;
                }
                if label != "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn setup_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    let panel = MenuItem::with_id(app, "panel", "简洁面板", true, None::<&str>)?;
    let taskbar = MenuItem::with_id(app, "taskbar", "任务栏显示", true, None::<&str>)?;
    let floating = MenuItem::with_id(app, "floating", "悬浮框", true, None::<&str>)?;
    let debug = MenuItem::with_id(app, "debug", "完整调试版", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "立即刷新", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&panel, &taskbar, &floating, &debug, &refresh, &quit])?;

    let _tray = TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .tooltip("2.4G 外设电量")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "panel" => {
                let _ = windows::switch_mode(app, &engine_of(app), "panel");
            }
            "taskbar" => {
                let e = engine_of(app);
                if e.shared.settings.read().taskbar_enabled {
                    let _ = windows::disable_taskbar(app, &e);
                } else {
                    let _ = windows::switch_mode(app, &e, "taskbar");
                }
            }
            "floating" => {
                let _ = windows::switch_mode(app, &engine_of(app), "floating");
            }
            "debug" => {
                let _ = windows::switch_mode(app, &engine_of(app), "debug");
            }
            "refresh" => {
                if let Some(e) = app.try_state::<EngineState>() {
                    let eng = e.0.clone();
                    let a = app.clone();
                    std::thread::spawn(move || eng.refresh_all(&a));
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("panel") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        })
        .build(app)?;
    Ok(())
}

fn engine_of(app: &tauri::AppHandle) -> Arc<Engine> {
    app.state::<EngineState>().0.clone()
}

#[allow(dead_code)]
fn emit_to_all(app: &tauri::AppHandle, event: &str, payload: serde_json::Value) {
    let _ = app.emit(event, payload);
}
pub fn load_settings_dbg() -> model::AppSettings {
    store::load()
}