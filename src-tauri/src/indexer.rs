use crate::{
    db::{upsert_batch, Db, FileRecord},
    settings::Settings,
};
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use walkdir::WalkDir;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct IndexStatus {
    pub is_indexing: bool,
    pub files_indexed: u64,
    pub current_path: String,
    pub last_indexed_at: Option<i64>,
    pub error: Option<String>,
}

pub type StatusHandle = Arc<Mutex<IndexStatus>>;

const BATCH_SIZE: usize = 500;

pub async fn run(db: Db, settings: Settings, status: StatusHandle) {
    tokio::task::spawn_blocking(move || index_sync(db, settings, status))
        .await
        .ok();
}

fn index_sync(db: Db, settings: Settings, status: StatusHandle) {
    set_status(&status, |s| {
        s.is_indexing = true;
        s.files_indexed = 0;
        s.error = None;
    });

    let mut excluded: Vec<String> = settings
        .excluded_folders
        .iter()
        .map(|p| normalise(p))
        .collect();

    // Always skip these high-noise dirs regardless of user settings.
    // Entries containing ":\" are treated as path-prefix matches;
    // everything else is a substring match anywhere in the path.
    for always in &[
        // Temp / low-integrity AppData
        r"appdata\local\temp",
        r"appdata\locallow",
        // Browser caches
        r"appdata\local\google\chrome",
        r"appdata\local\microsoft\edge",
        r"appdata\local\brave software",
        r"appdata\roaming\mozilla",
        // Microsoft / Office / Teams noise
        r"appdata\local\microsoft\teams",
        r"appdata\local\microsoft\onedrive",
        r"appdata\local\microsoft\windows",
        r"appdata\roaming\microsoft",
        // UWP / Store app sandboxes
        r"appdata\local\packages",
        // Generic folder names - catch any "cache" or "caches" dir anywhere
        r"\cache\",
        r"\caches\",
        // Dev noise
        r"\node_modules\",
        r"\.git\",
        r"\__pycache__\",
        r"\target\debug\",
        r"\target\release\",
        r"\.next\",
        // Windows update staging
        r"\$windows.~ws\",
        r"\$windows.~bt\",
    ] {
        excluded.push(always.to_lowercase());
    }

    let mut batch: Vec<FileRecord> = Vec::with_capacity(BATCH_SIZE);

    for folder in &settings.indexed_folders {
        let walker = WalkDir::new(folder)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !is_excluded(&e.path().to_string_lossy(), &excluded));

        for entry in walker.flatten() {
            let path_str = entry.path().to_string_lossy().to_string();
            let filename = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().is_dir();

            let extension = if is_dir {
                None
            } else {
                entry
                    .path()
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase().to_string())
            };

            let (size, modified_time) = entry
                .metadata()
                .map(|m| {
                    let sz = if is_dir { None } else { Some(m.len() as i64) };
                    let mt = m
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64);
                    (sz, mt)
                })
                .unwrap_or((None, None));

            set_status(&status, |s| {
                s.current_path = path_str.clone();
                s.files_indexed += 1;
            });

            batch.push(FileRecord {
                id: 0,
                path: path_str,
                filename,
                extension,
                size,
                modified_time,
                is_dir,
            });

            if batch.len() >= BATCH_SIZE {
                if let Err(e) = upsert_batch(&db, &batch) {
                    set_status(&status, |s| s.error = Some(e));
                }
                batch.clear();
            }
        }
    }

    if !batch.is_empty() {
        if let Err(e) = upsert_batch(&db, &batch) {
            set_status(&status, |s| s.error = Some(e));
        }
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    set_status(&status, |s| {
        s.is_indexing = false;
        s.last_indexed_at = Some(now);
        s.current_path = String::new();
    });
}

fn normalise(path: &str) -> String {
    let lower = path.trim_end_matches(['\\', '/']).to_lowercase();
    format!("{}\\", lower)
}

fn is_excluded(path: &str, excluded: &[String]) -> bool {
    let lower = path.to_lowercase();
    let lower_slash = format!("{}\\", lower);
    excluded.iter().any(|ex| {
        // Drive-root exclusions: must match as a path prefix
        if ex.contains(":\\") {
            lower.starts_with(ex.as_str()) || lower_slash == *ex
        } else {
            // Bare name patterns (e.g. \node_modules\) - substring match
            lower.contains(ex.as_str())
        }
    })
}

fn set_status(handle: &StatusHandle, f: impl FnOnce(&mut IndexStatus)) {
    if let Ok(mut s) = handle.lock() {
        f(&mut s);
    }
}
