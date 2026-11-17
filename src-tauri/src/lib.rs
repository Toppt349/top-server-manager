//! Tauri shell: window, tray, single instance, close behaviour and the typed
//! IPC bridge to the core engine. No infrastructure logic lives here.

mod commands;

use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, WindowEvent};
use tsm_core::config::CloseBehavior;
use tsm_core::paths::AppPaths;
use tsm_core::Engine;

pub struct AppState {
    pub engine: Arc<Engine>,
}

fn show_main(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

async fn quit(app: tauri::AppHandle, stop_servers: bool) {
    let engine = app.state::<AppState>().engine.clone();
    engine.shutdown(stop_servers).await;
    app.exit(0);
}

pub fn run() {
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
    tauri::async_runtime::set(runtime.handle().clone());
    let engine = runtime.block_on(Engine::new(AppPaths::system_default())).expect("failed to initialise engine");
    tsm_core::i18n::set_language(&engine.settings().language);
    let autostart = std::env::args().any(|a| a == "--autostart");

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState { engine: engine.clone() })
        .setup(move |app| {
            let handle = app.handle().clone();
            // Forward engine events to the UI.
            let mut rx = engine.events.subscribe();
            let h2 = handle.clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(ev) => {
                            let _ = h2.emit("tsm://event", ev);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
            });
            // Recovery: re-attach servers, restore hosting, auto start.
            let e2 = engine.clone();
            tauri::async_runtime::spawn(async move {
                let re = e2.startup().await;
                if !re.is_empty() {
                    e2.emit_notice("info", &format!("Re-attached {} server(s) that kept running while the app was closed.", re.len()));
                }
            });

            // Tray
            let show = MenuItem::with_id(app, "show", "Open TOP Server Manager", true, None::<&str>)?;
            let quit_keep = MenuItem::with_id(app, "quit_keep", "Quit (keep servers running)", true, None::<&str>)?;
            let quit_stop = MenuItem::with_id(app, "quit_stop", "Stop all servers and quit", true, None::<&str>)?;
            let sep = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(app, &[&show, &sep, &quit_keep, &quit_stop])?;
            let mut tray = TrayIconBuilder::with_id("main-tray").tooltip("TOP Server Manager").menu(&menu).show_menu_on_left_click(false);
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.on_menu_event(|app, ev| match ev.id().as_ref() {
                "show" => show_main(app),
                "quit_keep" => {
                    let a = app.clone();
                    tauri::async_runtime::spawn(quit(a, false));
                }
                "quit_stop" => {
                    let a = app.clone();
                    tauri::async_runtime::spawn(quit(a, true));
                }
                _ => {}
            })
            .on_tray_icon_event(|tray, ev| {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                    show_main(tray.app_handle());
                }
            })
            .build(app)?;

            if autostart && engine.settings().start_minimized {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle().clone();
                let behavior = app.state::<AppState>().engine.settings().close_behavior;
                match behavior {
                    CloseBehavior::MinimizeToTray => {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                    CloseBehavior::ExitKeepServers => {
                        api.prevent_close();
                        tauri::async_runtime::spawn(quit(app, false));
                    }
                    CloseBehavior::ExitStopServers => {
                        api.prevent_close();
                        tauri::async_runtime::spawn(quit(app, true));
                    }
                }
            }
        })
        .invoke_handler(commands::handler())
        .run(tauri::generate_context!())
        .expect("error while running TOP Server Manager");
    drop(runtime);
}
