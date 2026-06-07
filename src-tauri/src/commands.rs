use crate::{db, indexer::IndexStatus, settings, settings::Settings};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};

pub struct AppState {
    pub db: db::Db,
    pub index_status: Arc<Mutex<IndexStatus>>,
}

// ── Search ────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn search_files(
    state: State<'_, AppState>,
    query: String,
    filter: String,
) -> Result<Vec<db::FileRecord>, String> {
    let results = db::search(&state.db, &query, 50, &filter)?;

    let mut deleted: Vec<String> = Vec::new();
    let existing: Vec<db::FileRecord> = results
        .into_iter()
        .filter(|r| {
            if std::path::Path::new(&r.path).exists() {
                true
            } else {
                deleted.push(r.path.clone());
                false
            }
        })
        .collect();

    if !deleted.is_empty() {
        let _ = db::remove_paths(&state.db, &deleted);
    }

    Ok(existing)
}

// ── Indexer ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn start_indexing(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if state
        .index_status
        .lock()
        .map(|s| s.is_indexing)
        .unwrap_or(false)
    {
        return Err("Already indexing".into());
    }

    let cfg = settings::load(&app)?;
    let db = state.db.clone();
    let status = state.index_status.clone();

    tauri::async_runtime::spawn(crate::indexer::run(db, cfg, status));
    Ok(())
}

#[tauri::command]
pub fn get_index_status(state: State<'_, AppState>) -> Result<IndexStatus, String> {
    state
        .index_status
        .lock()
        .map(|s| s.clone())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_file_count(state: State<'_, AppState>) -> Result<i64, String> {
    db::file_count(&state.db)
}

// ── File actions ──────────────────────────────────────────────────────────────

#[tauri::command]
pub fn open_file(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("cmd")
            .args(["/c", "start", "", &path])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn reveal_in_explorer(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::UI::Shell::{ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems};
        use windows::Win32::UI::Shell::Common::ITEMIDLIST;
        use windows::core::PCWSTR;

        let wide: Vec<u16> = OsStr::new(&path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        unsafe {
            // ILCreateFromPathW gives us the full PIDL for the file.
            // Passing it as pidlFolder with cidl=0 tells the shell to open
            // the parent folder and scroll to + select this specific item -
            // even if that folder window is already open.
            let pidl: *mut ITEMIDLIST = ILCreateFromPathW(PCWSTR(wide.as_ptr()));
            if !pidl.is_null() {
                let _ = SHOpenFolderAndSelectItems(pidl as *const ITEMIDLIST, None, 0);
                ILFree(Some(pidl as *const ITEMIDLIST));
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn copy_path(path: String) -> Result<(), String> {
    let mut cb = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    cb.set_text(path).map_err(|e| e.to_string())
}

// ── Settings ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<Settings, String> {
    settings::load(&app)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    apply_autostart(settings.launch_at_startup);
    settings::save(&app, &settings)
}

pub fn apply_autostart(enabled: bool) {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
        use winreg::RegKey;

        let Ok(run) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(
            r"Software\Microsoft\Windows\CurrentVersion\Run",
            KEY_SET_VALUE,
        ) else { return; };

        if enabled {
            if let Ok(exe) = std::env::current_exe() {
                let _ = run.set_value("LocaFetch", &exe.to_string_lossy().to_string());
            }
        } else {
            let _ = run.delete_value("LocaFetch");
        }
    }
}

#[tauri::command]
pub fn open_with_dialog(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::UI::Shell::{SHOpenWithDialog, OPENASINFO, OPEN_AS_INFO_FLAGS};
        use windows::core::PCWSTR;

        let path_w: Vec<u16> = OsStr::new(&path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        // OAIF_ALLOW_REGISTRATION (0x1) | OAIF_EXEC (0x4) - show dialog and open on confirm
        let info = OPENASINFO {
            pcszFile: PCWSTR(path_w.as_ptr()),
            pcszClass: PCWSTR::null(),
            oaifInFlags: OPEN_AS_INFO_FLAGS(0x0005),
        };

        unsafe {
            let _ = SHOpenWithDialog(None, &info);
        }
    }
    Ok(())
}

// ── System ────────────────────────────────────────────────────────────────────

/// Returns every available local drive root (C:\, D:\, …).
#[tauri::command]
pub fn get_drives() -> Vec<String> {
    (b'C'..=b'Z')
        .map(|l| format!("{}:\\", l as char))
        .filter(|p| std::path::Path::new(p).exists())
        .collect()
}

// ── Window ────────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn hide_window(app: AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        w.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn update_hotkey(app: AppHandle, hotkey: String) -> Result<(), String> {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    let shortcut = crate::parse_hotkey(&hotkey)
        .ok_or_else(|| format!("Invalid hotkey: '{}'", hotkey))?;
    app.global_shortcut().unregister_all().map_err(|e| e.to_string())?;
    let handle = app.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                crate::toggle_window(&handle);
            }
        })
        .map_err(|e| e.to_string())
}
