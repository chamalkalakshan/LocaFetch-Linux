use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Settings {
    pub indexed_folders: Vec<String>,
    pub excluded_folders: Vec<String>,
    pub max_results: usize,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default = "default_autostart")]
    pub launch_at_startup: bool,
    #[serde(default = "default_true")]
    pub start_minimized: bool,
    #[serde(default = "default_true")]
    pub minimize_to_tray: bool,
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    #[serde(default)]
    pub reindex_interval_hours: u32,
}

fn default_theme() -> String {
    "dark".to_string()
}

fn default_autostart() -> bool {
    true
}

fn default_true() -> bool {
    true
}

fn default_hotkey() -> String {
    "ctrl+space".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            indexed_folders: Vec::new(),
            excluded_folders: default_exclusions(),
            max_results: 50,
            theme: default_theme(),
            launch_at_startup: default_autostart(),
            start_minimized: default_true(),
            minimize_to_tray: default_true(),
            hotkey: default_hotkey(),
            reindex_interval_hours: 0,
        }
    }
}

fn detect_drives() -> Vec<String> {
    (b'C'..=b'Z')
        .map(|l| format!("{}:\\", l as char))
        .filter(|p| std::path::Path::new(p).exists())
        .collect()
}

#[allow(dead_code)]
pub fn is_first_run(app: &AppHandle) -> bool {
    settings_path(app).map(|p| !p.exists()).unwrap_or(false)
}

fn default_exclusions() -> Vec<String> {
    vec![
        // Core system
        r"C:\Windows",
        r"C:\Recovery",
        r"C:\PerfLogs",
        r"C:\$Recycle.Bin",
        r"C:\$RECYCLE.BIN",
        r"C:\System Volume Information",
        // Program files - mostly binaries/DLLs, not user content
        r"C:\Program Files",
        r"C:\Program Files (x86)",
        // System-wide app data
        r"C:\ProgramData",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("settings.json"))
}

pub fn load(app: &AppHandle) -> Result<Settings, String> {
    let path = settings_path(app)?;
    if !path.exists() {
        let mut s = Settings::default();
        s.indexed_folders = detect_drives();
        // Persist immediately so the next load sees a real file.
        let data = serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?;
        std::fs::write(&path, data).map_err(|e| e.to_string())?;
        return Ok(s);
    }
    let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&data).map_err(|e| e.to_string())
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    let data = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(path, data).map_err(|e| e.to_string())
}
