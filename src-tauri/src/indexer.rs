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

    let excluded = Exclusions::new(&settings.excluded_folders);

    let mut batch: Vec<FileRecord> = Vec::with_capacity(BATCH_SIZE);

    for folder in &settings.indexed_folders {
        let walker = WalkDir::new(folder)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !excluded.matches(&e.path().to_string_lossy()));

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

/// Built-in high-noise locations skipped regardless of user settings. Matched as
/// substrings against a normalised path (lowercase, `/` separators, trailing `/`).
const ALWAYS_SKIP: &[&str] = &[
    // Windows: temp / low-integrity AppData
    "appdata/local/temp",
    "appdata/locallow",
    // Windows: browser caches
    "appdata/local/google/chrome",
    "appdata/local/microsoft/edge",
    "appdata/local/brave software",
    "appdata/roaming/mozilla",
    // Windows: Microsoft / Office / Teams noise
    "appdata/local/microsoft/teams",
    "appdata/local/microsoft/onedrive",
    "appdata/local/microsoft/windows",
    "appdata/roaming/microsoft",
    // Windows: UWP / Store app sandboxes
    "appdata/local/packages",
    // Windows: update staging
    "/$windows.~ws/",
    "/$windows.~bt/",
    // Linux: caches, trash, browser profiles, Flatpak sandboxes, toolchains
    "/.cache/",
    "/.local/share/trash/",
    "/.trash-", // per-drive trash on removable/NTFS drives (.Trash-1000)
    // Windows system folders on NTFS drives mounted under Linux
    "/$recycle.bin/",
    "/system volume information/",
    "/.mozilla/",
    "/.config/google-chrome/",
    "/.config/chromium/",
    "/.config/bravesoftware/",
    "/.var/app/",
    "/.npm/",
    "/.cargo/registry/",
    "/.rustup/",
    // Generic folder names - catch any "cache" or "caches" dir anywhere
    "/cache/",
    "/caches/",
    // Dev noise
    "/node_modules/",
    "/.git/",
    "/__pycache__/",
    "/target/debug/",
    "/target/release/",
    "/.next/",
];

/// Folder exclusions shared by the indexer and the file watcher.
pub(crate) struct Exclusions {
    /// Absolute folders - match the folder itself and everything below it.
    prefixes: Vec<String>,
    /// Bare name patterns - match anywhere in the path.
    patterns: Vec<String>,
}

impl Exclusions {
    pub(crate) fn new(user: &[String]) -> Self {
        let mut prefixes = Vec::new();
        let mut patterns: Vec<String> = ALWAYS_SKIP.iter().map(|s| s.to_string()).collect();
        for p in user {
            let n = normalise(p);
            if n.starts_with('/') || n.contains(":/") {
                prefixes.push(n);
            } else {
                patterns.push(n);
            }
        }
        Self { prefixes, patterns }
    }

    pub(crate) fn matches(&self, path: &str) -> bool {
        let p = normalise(path);
        self.prefixes.iter().any(|ex| p.starts_with(ex.as_str()))
            || self.patterns.iter().any(|pat| p.contains(pat.as_str()))
    }
}

/// Lowercase, `/` separators, exactly one trailing `/` - so `C:\Foo` and
/// `/home/me/foo` compare the same way on every platform.
fn normalise(path: &str) -> String {
    let mut s = path.replace('\\', "/").trim_end_matches('/').to_lowercase();
    s.push('/');
    s
}

fn set_status(handle: &StatusHandle, f: impl FnOnce(&mut IndexStatus)) {
    if let Ok(mut s) = handle.lock() {
        f(&mut s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusions_match_linux_and_windows_paths() {
        let e = Exclusions::new(&[
            "/proc".into(),
            "/home/me/Secret/".into(),
            r"C:\Windows".into(),
            "build".into(),
        ]);
        for (path, want) in [
            ("/proc", true),
            ("/proc/1/fd", true),
            ("/processes", false),
            ("/home/me/Secret", true),
            ("/home/me/Secret/a.txt", true),
            ("/home/me/SecretX", false),
            ("/home/me/proj/node_modules", true),
            ("/home/me/proj/node_modules/x/y.js", true),
            ("/home/me/.cache/thumbnails", true),
            ("/home/me/cachefile", false),
            ("/home/me/.local/share/Trash/files/x", true),
            ("/run/media/me/LAP/.Trash-1000/files/x.srt", true),
            ("/run/media/me/LAP/System Volume Information/x", true),
            ("/run/media/me/LAP/$RECYCLE.BIN/S-1-5/x", true),
            ("/run/media/me/LAP/Movies/trash talk.mkv", false),
            (r"C:\Windows\System32", true),
            (r"C:\WindowsApps", false),
            (r"C:\Users\me\AppData\Local\Temp\x", true),
            ("/home/me/build/out", true),
            ("/home/me/docs/a.pdf", false),
        ] {
            assert_eq!(e.matches(path), want, "{path}");
        }
    }

    #[test]
    fn indexes_tree_and_skips_excluded() {
        let root = std::env::temp_dir().join(format!("locafetch-idx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["Docs/sub dir", "proj/node_modules/pkg", ".cache/thumbs", "private"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        for f in ["Docs/sub dir/Report 2024.PDF", "proj/node_modules/pkg/index.js", ".cache/thumbs/a.png", "private/key.txt", "top.md"] {
            std::fs::write(root.join(f), b"x").unwrap();
        }

        let db = crate::db::tests::memory_db();
        let settings = Settings {
            indexed_folders: vec![root.to_string_lossy().to_string()],
            excluded_folders: vec![root.join("private").to_string_lossy().to_string()],
            ..Default::default()
        };
        let status = StatusHandle::default();
        index_sync(db.clone(), settings, status.clone());

        let conn = db.lock().unwrap();
        let mut paths: Vec<String> = conn
            .prepare("SELECT path FROM files").unwrap()
            .query_map([], |r| r.get(0)).unwrap()
            .map(|r| r.unwrap())
            .collect();
        drop(conn);
        let _ = std::fs::remove_dir_all(&root);
        let prefix = root.to_string_lossy().to_string();
        paths = paths.into_iter().map(|p| p.replacen(&prefix, "", 1)).collect();
        paths.sort();
        assert_eq!(paths, ["", "/Docs", "/Docs/sub dir", "/Docs/sub dir/Report 2024.PDF", "/proj", "/top.md"]);

        let s = status.lock().unwrap();
        assert!(!s.is_indexing && s.last_indexed_at.is_some() && s.error.is_none());
    }
}
