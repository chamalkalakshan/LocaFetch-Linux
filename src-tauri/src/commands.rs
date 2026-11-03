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
            let path = std::path::Path::new(&r.path);
            if path.exists() {
                return true;
            }
            // Only forget it if its folder is still there - otherwise the drive
            // is probably just unplugged and the file will be back.
            if path.parent().is_some_and(|p| p.exists()) {
                deleted.push(r.path.clone());
            }
            false
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
    #[cfg(target_os = "linux")]
    spawn_reaped(std::process::Command::new("xdg-open").arg(&path))?;
    #[cfg(target_os = "macos")]
    spawn_reaped(std::process::Command::new("open").arg(&path))?;
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
    #[cfg(target_os = "linux")]
    std::thread::spawn(move || {
        use std::process::{Command, Stdio};
        // Ask the file manager (Dolphin, Nautilus, Nemo, …) to open the parent
        // folder with this item selected. --print-reply makes the exit status
        // reflect whether a file manager actually handled it.
        let shown = Command::new("dbus-send")
            .args([
                "--session",
                "--print-reply",
                "--reply-timeout=5000",
                "--dest=org.freedesktop.FileManager1",
                "--type=method_call",
                "/org/freedesktop/FileManager1",
                "org.freedesktop.FileManager1.ShowItems",
            ])
            .arg(format!("array:string:{}", file_uri(&path)))
            .arg("string:")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !shown {
            let parent = std::path::Path::new(&path)
                .parent()
                .unwrap_or(std::path::Path::new("/"));
            let _ = Command::new("xdg-open").arg(parent).status();
        }
    });
    #[cfg(target_os = "macos")]
    spawn_reaped(std::process::Command::new("open").args(["-R", &path]))?;
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
    // Dev builds load the UI from the Vite dev server, which isn't running at
    // login - never register (or overwrite) the startup entry from them.
    if cfg!(debug_assertions) { return; }

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
    #[cfg(target_os = "linux")]
    {
        use std::path::PathBuf;

        let Some(config) = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        else { return; };
        let dir = config.join("autostart");
        let file = dir.join("locafetch.desktop");

        if enabled {
            // Inside an AppImage, current_exe() is a temporary mount point;
            // $APPIMAGE is the real file to launch at login.
            let Some(exe) = std::env::var_os("APPIMAGE")
                .map(PathBuf::from)
                .or_else(|| std::env::current_exe().ok())
            else { return; };
            let entry = format!(
                "[Desktop Entry]\nType=Application\nName=LocaFetch\nComment=Fast file search\nExec={}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
                desktop_exec_quote(&exe.to_string_lossy()),
            );
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(&file, entry);
        } else {
            let _ = std::fs::remove_file(&file);
        }
    }
}

/// Quotes a path for a .desktop `Exec=` key: the Exec quoting rules, then the
/// general string escaping applied on top of them (so backslashes double twice).
#[cfg(target_os = "linux")]
fn desktop_exec_quote(path: &str) -> String {
    let mut out = String::from("\"");
    for c in path.chars() {
        match c {
            '\\' => out.push_str(r"\\\\"),
            '"' | '`' | '$' => {
                out.push_str(r"\\");
                out.push(c);
            }
            '%' => out.push_str("%%"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
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
    #[cfg(target_os = "linux")]
    std::thread::spawn(move || {
        // Without the portal, at least open it in the default app.
        if portal_open_with(&path).is_err() {
            let _ = std::process::Command::new("xdg-open").arg(&path).status();
        }
    });
    #[cfg(target_os = "macos")]
    spawn_reaped(std::process::Command::new("open").arg(&path))?;
    Ok(())
}

/// Shows the desktop's own "Open with" chooser through the XDG desktop portal.
/// OpenFile (with a file descriptor) is used because the portal rejects
/// `file://` URIs in OpenURI. Blocks until the user picks an app or cancels:
/// the portal drops the request if our connection closes before then.
#[cfg(target_os = "linux")]
fn portal_open_with(path: &str) -> Result<(), Box<dyn std::error::Error>> {
    use std::collections::HashMap;
    use std::os::fd::AsFd;
    use zbus::zvariant::{Fd, OwnedValue, Value};

    let file = std::fs::File::open(path)?;
    let conn = zbus::blocking::Connection::session()?;

    // Subscribe to the Response signal before calling, so it can't be missed.
    let token = format!("locafetch{}", std::process::id());
    let sender = conn
        .unique_name()
        .ok_or("no D-Bus unique name")?
        .trim_start_matches(':')
        .replace('.', "_");
    let handle = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    let rule = zbus::MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.portal.Request")?
        .member("Response")?
        .path(handle.as_str())?
        .build();
    let mut responses = zbus::blocking::MessageIterator::for_match_rule(rule, &conn, Some(1))?;

    let mut options: HashMap<&str, Value> = HashMap::new();
    options.insert("ask", Value::from(true));
    options.insert("handle_token", Value::from(token.as_str()));
    conn.call_method(
        Some("org.freedesktop.portal.Desktop"),
        "/org/freedesktop/portal/desktop",
        Some("org.freedesktop.portal.OpenURI"),
        "OpenFile",
        &("", Fd::from(file.as_fd()), options),
    )?;

    // 0 = opened, 1 = cancelled by the user, 2 = failed
    let msg = responses.next().ok_or("portal closed without responding")??;
    let (code, _): (u32, HashMap<String, OwnedValue>) = msg.body().deserialize()?;
    if code == 2 {
        return Err("portal could not open the file".into());
    }
    Ok(())
}

/// Spawns a helper without blocking; a thread reaps it so it doesn't linger
/// as a zombie process.
#[cfg(not(target_os = "windows"))]
fn spawn_reaped(cmd: &mut std::process::Command) -> Result<(), String> {
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// `file://` URI for an absolute path, percent-encoding everything but
/// unreserved characters and `/`.
#[cfg(target_os = "linux")]
fn file_uri(path: &str) -> String {
    let mut uri = String::from("file://");
    for &b in path.as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{:02X}", b));
        }
    }
    uri
}

// ── System ────────────────────────────────────────────────────────────────────

/// Returns every available local drive root (C:\, D:\, …), or `/` on Linux/macOS.
#[tauri::command]
pub fn get_drives() -> Vec<String> {
    settings::detect_drives()
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn file_uri_percent_encodes() {
        assert_eq!(
            file_uri("/run/media/me/Kodak SSD/a,b'c\"%#?é.txt"),
            "file:///run/media/me/Kodak%20SSD/a%2Cb%27c%22%25%23%3F%C3%A9.txt"
        );
    }

    #[test]
    fn desktop_exec_quote_escapes_twice() {
        assert_eq!(desktop_exec_quote("/opt/my app/run"), r#""/opt/my app/run""#);
        assert_eq!(desktop_exec_quote(r#"/a$b"c`d\e%f"#), r#""/a\\$b\\"c\\`d\\\\e%%f""#);
    }
}
