use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

mod commands;
mod db;
mod indexer;
mod settings;
mod watcher;

use commands::AppState;
use indexer::IndexStatus;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let first_run = settings::is_first_run(app.handle());

            // ── Database ──────────────────────────────────────────────────
            let db = db::init(app.handle())
                .map_err(|e| Box::<dyn std::error::Error>::from(e))?;

            let index_status = Arc::new(Mutex::new(IndexStatus::default()));

            app.manage(AppState {
                db: db.clone(),
                index_status: index_status.clone(),
            });

            // ── Apply settings (load() writes defaults on first run) ──────
            let cfg = settings::load(app.handle()).unwrap_or_default();

            commands::apply_autostart(cfg.launch_at_startup);

            if first_run {
                tauri::async_runtime::spawn(crate::indexer::run(
                    db.clone(), cfg.clone(), index_status.clone(),
                ));
            }

            // File watcher - live index updates
            watcher::start(db.clone(), cfg.indexed_folders.clone(), cfg.excluded_folders.clone());

            // Auto re-index timer
            if cfg.reindex_interval_hours > 0 {
                let db_t = db.clone();
                let status_t = index_status.clone();
                let app_t = app.handle().clone();
                let hours = cfg.reindex_interval_hours;
                tauri::async_runtime::spawn(async move {
                    let interval = tokio::time::Duration::from_secs(hours as u64 * 3600);
                    loop {
                        tokio::time::sleep(interval).await;
                        if let Ok(c) = settings::load(&app_t) {
                            if c.reindex_interval_hours > 0 {
                                crate::indexer::run(db_t.clone(), c, status_t.clone()).await;
                            }
                        }
                    }
                });
            }

            // Show window if start_minimized is off
            if !cfg.start_minimized {
                if let Some(w) = app.get_webview_window("main") {
                    w.show().ok();
                    w.set_focus().ok();
                }
            }

            // ── System tray ───────────────────────────────────────────────
            setup_tray(app)?;

            // ── Global shortcut from settings ─────────────────────────────
            let handle = app.handle().clone();
            if let Some(shortcut) = parse_hotkey(&cfg.hotkey) {
                app.global_shortcut().on_shortcut(
                    shortcut,
                    move |_app, _shortcut, event| {
                        if event.state() == ShortcutState::Pressed {
                            toggle_window(&handle);
                        }
                    },
                )?;
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::search_files,
            commands::start_indexing,
            commands::get_index_status,
            commands::get_file_count,
            commands::open_file,
            commands::reveal_in_explorer,
            commands::copy_path,
            commands::get_settings,
            commands::save_settings,
            commands::hide_window,
            commands::get_drives,
            commands::update_hotkey,
            commands::open_with_dialog,
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let minimize = settings::load(window.app_handle())
                    .map(|s| s.minimize_to_tray)
                    .unwrap_or(true);
                if minimize {
                    window.hide().ok();
                    api.prevent_close();
                } else {
                    window.app_handle().exit(0);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running application");
}

pub(crate) fn toggle_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            window.hide().ok();
        } else {
            // Snap to collapsed size, position center-x at 30% from top
            if let Ok(Some(monitor)) = window.primary_monitor() {
                let screen = monitor.size();
                let scale = monitor.scale_factor();
                let win_w = (680.0 * scale) as u32;
                let win_h = (68.0 * scale) as u32;
                let x = ((screen.width as f64 - win_w as f64) / 2.0) as i32;
                let y = (screen.height as f64 * 0.30) as i32;
                window.set_size(tauri::PhysicalSize::new(win_w, win_h)).ok();
                window.set_position(tauri::PhysicalPosition::new(x, y)).ok();
            }
            window.show().ok();
            window.set_focus().ok();
            window.emit("search-shown", ()).ok();
        }
    }
}

pub(crate) fn parse_hotkey(s: &str) -> Option<Shortcut> {
    let mut modifiers = Modifiers::empty();
    let mut key_code: Option<Code> = None;
    for part in s.to_lowercase().split('+') {
        match part.trim() {
            "ctrl" | "control" => modifiers |= Modifiers::CONTROL,
            "alt"              => modifiers |= Modifiers::ALT,
            "shift"            => modifiers |= Modifiers::SHIFT,
            "meta" | "win" | "super" | "cmd" => modifiers |= Modifiers::META,
            key => key_code = Some(str_to_code(key)?),
        }
    }
    let code = key_code?;
    let mods = if modifiers.is_empty() { None } else { Some(modifiers) };
    Some(Shortcut::new(mods, code))
}

fn str_to_code(s: &str) -> Option<Code> {
    Some(match s {
        "space" => Code::Space, "enter" => Code::Enter,
        "tab" => Code::Tab, "escape" | "esc" => Code::Escape,
        "backspace" => Code::Backspace, "delete" | "del" => Code::Delete,
        "f1"=>Code::F1,"f2"=>Code::F2,"f3"=>Code::F3,"f4"=>Code::F4,
        "f5"=>Code::F5,"f6"=>Code::F6,"f7"=>Code::F7,"f8"=>Code::F8,
        "f9"=>Code::F9,"f10"=>Code::F10,"f11"=>Code::F11,"f12"=>Code::F12,
        "a"=>Code::KeyA,"b"=>Code::KeyB,"c"=>Code::KeyC,"d"=>Code::KeyD,
        "e"=>Code::KeyE,"f"=>Code::KeyF,"g"=>Code::KeyG,"h"=>Code::KeyH,
        "i"=>Code::KeyI,"j"=>Code::KeyJ,"k"=>Code::KeyK,"l"=>Code::KeyL,
        "m"=>Code::KeyM,"n"=>Code::KeyN,"o"=>Code::KeyO,"p"=>Code::KeyP,
        "q"=>Code::KeyQ,"r"=>Code::KeyR,"s"=>Code::KeyS,"t"=>Code::KeyT,
        "u"=>Code::KeyU,"v"=>Code::KeyV,"w"=>Code::KeyW,"x"=>Code::KeyX,
        "y"=>Code::KeyY,"z"=>Code::KeyZ,
        "0"=>Code::Digit0,"1"=>Code::Digit1,"2"=>Code::Digit2,"3"=>Code::Digit3,
        "4"=>Code::Digit4,"5"=>Code::Digit5,"6"=>Code::Digit6,"7"=>Code::Digit7,
        "8"=>Code::Digit8,"9"=>Code::Digit9,
        _ => return None,
    })
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    use tauri::{
        menu::{Menu, MenuItem},
        tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    };

    let show = MenuItem::with_id(app, "show", "Show LocaFetch", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .tooltip("LocaFetch")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => toggle_window(app),
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
                toggle_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}
