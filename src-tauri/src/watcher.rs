use crate::{
    db::{upsert_batch, Db, FileRecord},
    indexer::Exclusions,
};
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
    let excluded = Exclusions::new(&user_excluded);

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
    excluded: &Exclusions,
    pending_upsert: &mut HashMap<String, FileRecord>,
    pending_remove: &mut HashSet<String>,
) {
    use notify::event::{ModifyKind, RenameMode};

    match event.kind {
        EventKind::Create(_) | EventKind::Modify(ModifyKind::Data(_)) => {
            for path in event.paths {
                let s = path.to_string_lossy().to_string();
                if excluded.matches(&s) {
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
            let mut paths = event.paths;
            // Both (inotify on Linux) carries [from, to] - the old path is gone.
            if matches!(event.kind, EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
                && paths.len() == 2
            {
                let from = paths.remove(0).to_string_lossy().to_string();
                pending_upsert.remove(&from);
                pending_remove.insert(from);
            }
            for path in paths {
                let s = path.to_string_lossy().to_string();
                if excluded.matches(&s) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{ModifyKind, RenameMode};
    use std::path::PathBuf;

    #[test]
    fn rename_both_removes_old_path_and_adds_new() {
        let dir = std::env::temp_dir().join(format!("locafetch-w-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let new = dir.join("new name.txt");
        std::fs::write(&new, b"x").unwrap();
        let old = dir.join("old.txt");

        let mut up = HashMap::new();
        let mut rm = HashSet::new();
        let ev = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path(old.clone())
            .add_path(new.clone());
        handle_event(ev, &Exclusions::new(&[]), &mut up, &mut rm);
        let _ = std::fs::remove_dir_all(&dir);

        let s = |p: &PathBuf| p.to_string_lossy().to_string();
        assert!(rm.contains(&s(&old)));
        assert_eq!(up.keys().cloned().collect::<Vec<_>>(), [s(&new)]);
        assert_eq!(up[&s(&new)].filename, "new name.txt");
    }

    #[test]
    fn excluded_creates_are_ignored() {
        let mut up = HashMap::new();
        let mut rm = HashSet::new();
        let ev = Event::new(EventKind::Create(notify::event::CreateKind::File))
            .add_path(PathBuf::from("/home/me/proj/node_modules/x.js"));
        handle_event(ev, &Exclusions::new(&[]), &mut up, &mut rm);
        assert!(up.is_empty() && rm.is_empty());
    }
}
