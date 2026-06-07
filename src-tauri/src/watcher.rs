use crate::db::{upsert_batch, Db, FileRecord};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::{HashMap, HashSet},
    sync::mpsc::channel,
    time::{Duration, Instant},
};

pub fn start(db: Db, folders: Vec<String>, user_excluded: Vec<String>) {
    if folders.is_empty() {
        return;
    }
    let excluded = build_excluded(&user_excluded);

    std::thread::spawn(move || {
        let (tx, rx) = channel::<notify::Result<Event>>();
        let mut watcher = match RecommendedWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            Config::default(),
        ) {
            Ok(w) => w,
            Err(_) => return,
        };

        for folder in &folders {
            let _ = watcher.watch(std::path::Path::new(folder), RecursiveMode::Recursive);
        }

        let mut pending_upsert: HashMap<String, FileRecord> = HashMap::new();
        let mut pending_remove: HashSet<String> = HashSet::new();
        let mut last_flush = Instant::now();

        loop {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Ok(event)) => {
                    handle_event(event, &excluded, &mut pending_upsert, &mut pending_remove)
                }
                Ok(Err(_)) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }

            let should_flush = last_flush.elapsed() >= Duration::from_secs(2)
                || pending_upsert.len() + pending_remove.len() > 200;

            if should_flush {
                if !pending_upsert.is_empty() {
                    let records: Vec<FileRecord> =
                        pending_upsert.drain().map(|(_, v)| v).collect();
                    let _ = upsert_batch(&db, &records);
                }
                if !pending_remove.is_empty() {
                    let paths: Vec<String> = pending_remove.drain().collect();
                    let _ = crate::db::remove_paths(&db, &paths);
                }
                last_flush = Instant::now();
            }
        }
    });
}

fn handle_event(
    event: Event,
    excluded: &[String],
    pending_upsert: &mut HashMap<String, FileRecord>,
    pending_remove: &mut HashSet<String>,
) {
    use notify::event::{ModifyKind, RenameMode};

    match event.kind {
        EventKind::Create(_) | EventKind::Modify(ModifyKind::Data(_)) => {
            for path in event.paths {
                let s = path.to_string_lossy().to_string();
                if is_excluded(&s, excluded) {
                    continue;
                }
                if let Some(r) = path_to_record(&path) {
                    pending_remove.remove(&s);
                    pending_upsert.insert(s, r);
                }
            }
        }
        EventKind::Remove(_) | EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            for path in event.paths {
                let s = path.to_string_lossy().to_string();
                pending_upsert.remove(&s);
                pending_remove.insert(s);
            }
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To))
        | EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => {
            for path in event.paths {
                let s = path.to_string_lossy().to_string();
                if is_excluded(&s, excluded) {
                    continue;
                }
                if let Some(r) = path_to_record(&path) {
                    pending_remove.remove(&s);
                    pending_upsert.insert(s, r);
                }
            }
        }
        _ => {}
    }
}

fn path_to_record(path: &std::path::Path) -> Option<FileRecord> {
    let filename = path.file_name()?.to_string_lossy().to_string();
    let is_dir = path.is_dir();
    let extension = if is_dir {
        None
    } else {
        path.extension()
            .map(|e| e.to_string_lossy().to_lowercase().to_string())
    };
    let meta = std::fs::metadata(path).ok();
    let size = meta
        .as_ref()
        .map(|m| if is_dir { None } else { Some(m.len() as i64) })
        .flatten();
    let modified_time = meta.and_then(|m| {
        m.modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs() as i64)
    });
    Some(FileRecord {
        id: 0,
        path: path.to_string_lossy().to_string(),
        filename,
        extension,
        size,
        modified_time,
        is_dir,
    })
}

fn build_excluded(user: &[String]) -> Vec<String> {
    let mut v: Vec<String> = user.iter().map(|p| normalise(p)).collect();
    for s in &[
        r"appdata\local\temp",
        r"appdata\locallow",
        r"appdata\local\packages",
        r"appdata\local\google\chrome",
        r"appdata\local\microsoft\edge",
        r"appdata\local\brave software",
        r"appdata\roaming\mozilla",
        r"appdata\local\microsoft\teams",
        r"appdata\local\microsoft\onedrive",
        r"appdata\local\microsoft\windows",
        r"appdata\roaming\microsoft",
        r"\cache\",
        r"\caches\",
        r"\node_modules\",
        r"\.git\",
        r"\__pycache__\",
        r"\target\debug\",
        r"\target\release\",
        r"\.next\",
        r"\$windows.~ws\",
        r"\$windows.~bt\",
    ] {
        v.push(s.to_lowercase());
    }
    v
}

fn normalise(p: &str) -> String {
    format!("{}\\", p.trim_end_matches(['\\', '/']).to_lowercase())
}

fn is_excluded(path: &str, excluded: &[String]) -> bool {
    let lower = path.to_lowercase();
    let lower_slash = format!("{}\\", lower);
    excluded.iter().any(|ex| {
        if ex.contains(":\\") {
            lower.starts_with(ex.as_str()) || lower_slash == *ex
        } else {
            lower.contains(ex.as_str())
        }
    })
}
