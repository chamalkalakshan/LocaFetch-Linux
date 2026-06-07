use rusqlite::{params, Connection, Result as SqlResult};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager};

pub type Db = Arc<Mutex<Connection>>;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileRecord {
    pub id: i64,
    pub path: String,
    pub filename: String,
    pub extension: Option<String>,
    pub size: Option<i64>,
    pub modified_time: Option<i64>,
    pub is_dir: bool,
}

pub fn init(app: &AppHandle) -> Result<Db, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let conn = Connection::open(dir.join("index.db")).map_err(|e| e.to_string())?;
    setup_schema(&conn).map_err(|e| e.to_string())?;

    Ok(Arc::new(Mutex::new(conn)))
}

fn setup_schema(conn: &Connection) -> SqlResult<()> {
    // journal_mode returns a row, so must be called via query_row, not execute_batch
    conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))?;
    conn.execute("PRAGMA synchronous=NORMAL", [])?;

    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS files (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            path          TEXT    NOT NULL UNIQUE,
            filename      TEXT    NOT NULL,
            extension     TEXT,
            size          INTEGER,
            modified_time INTEGER,
            is_dir        INTEGER NOT NULL DEFAULT 0,
            indexed_at    INTEGER NOT NULL DEFAULT (strftime('%s','now'))
        );

        -- FTS5 with trigram tokenizer for substring matching
        CREATE VIRTUAL TABLE IF NOT EXISTS files_fts USING fts5(
            filename,
            path       UNINDEXED,
            content    = files,
            content_rowid = id,
            tokenize   = 'trigram'
        );

        CREATE TRIGGER IF NOT EXISTS files_ai AFTER INSERT ON files BEGIN
            INSERT INTO files_fts(rowid, filename, path)
            VALUES (new.id, new.filename, new.path);
        END;

        CREATE TRIGGER IF NOT EXISTS files_ad AFTER DELETE ON files BEGIN
            INSERT INTO files_fts(files_fts, rowid, filename, path)
            VALUES ('delete', old.id, old.filename, old.path);
        END;

        CREATE TRIGGER IF NOT EXISTS files_au AFTER UPDATE ON files BEGIN
            INSERT INTO files_fts(files_fts, rowid, filename, path)
            VALUES ('delete', old.id, old.filename, old.path);
            INSERT INTO files_fts(rowid, filename, path)
            VALUES (new.id, new.filename, new.path);
        END;
        "#,
    )
}

fn filter_sql_fts(filter: &str) -> &'static str {
    match filter {
        "folder"  => "AND f.is_dir = 1",
        "video"   => "AND lower(f.extension) IN ('mp4','mov','avi','mkv','webm','wmv','m4v','flv','ts','m2ts')",
        "audio"   => "AND lower(f.extension) IN ('mp3','wav','flac','aac','ogg','m4a','wma','opus')",
        "image"   => "AND lower(f.extension) IN ('jpg','jpeg','png','gif','webp','svg','bmp','ico','tiff','heic','raw')",
        "doc"     => "AND lower(f.extension) IN ('pdf','doc','docx','xls','xlsx','ppt','pptx','txt','md','csv','rtf')",
        "archive" => "AND lower(f.extension) IN ('zip','rar','7z','tar','gz','bz2','xz')",
        _         => "",
    }
}

fn filter_sql_like(filter: &str) -> &'static str {
    match filter {
        "folder"  => "AND is_dir = 1",
        "video"   => "AND lower(extension) IN ('mp4','mov','avi','mkv','webm','wmv','m4v','flv','ts','m2ts')",
        "audio"   => "AND lower(extension) IN ('mp3','wav','flac','aac','ogg','m4a','wma','opus')",
        "image"   => "AND lower(extension) IN ('jpg','jpeg','png','gif','webp','svg','bmp','ico','tiff','heic','raw')",
        "doc"     => "AND lower(extension) IN ('pdf','doc','docx','xls','xlsx','ppt','pptx','txt','md','csv','rtf')",
        "archive" => "AND lower(extension) IN ('zip','rar','7z','tar','gz','bz2','xz')",
        _         => "",
    }
}

pub fn search(db: &Db, query: &str, limit: usize, filter: &str) -> Result<Vec<FileRecord>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(vec![]);
    }

    let conn = db.lock().map_err(|e| e.to_string())?;

    // Trigram needs >=3 chars per token; fall back to LIKE for short queries
    if query.len() < 3 {
        return search_like_inner(&conn, query, limit, filter);
    }

    let fts_query = build_fts_query(query);
    if fts_query.is_empty() {
        return search_like_inner(&conn, query, limit, filter);
    }

    let sql = format!(
        "SELECT f.id, f.path, f.filename, f.extension, f.size, f.modified_time, f.is_dir
         FROM files_fts
         JOIN files f ON files_fts.rowid = f.id
         WHERE files_fts MATCH ?1
         {}
         ORDER BY rank
         LIMIT ?2",
        filter_sql_fts(filter)
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![fts_query, limit as i64], map_row)
        .map_err(|e| e.to_string())?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| e.to_string())?);
    }
    // If FTS returned nothing, try LIKE as fallback (handles edge cases)
    if results.is_empty() {
        return search_like_inner(&conn, query, limit, filter);
    }
    Ok(results)
}

fn search_like_inner(conn: &Connection, query: &str, limit: usize, filter: &str) -> Result<Vec<FileRecord>, String> {
    let pattern = format!("%{}%", query);
    let sql = format!(
        "SELECT id, path, filename, extension, size, modified_time, is_dir
         FROM files WHERE filename LIKE ?1 {} ORDER BY filename LIMIT ?2",
        filter_sql_like(filter)
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![pattern, limit as i64], map_row)
        .map_err(|e| e.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}

/// Wraps each whitespace-separated token in quotes so FTS5 treats them
/// as literal substring searches (not boolean operators).
fn build_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .filter(|w| w.len() >= 3)
        .map(|w| format!("\"{}\"", w.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileRecord> {
    Ok(FileRecord {
        id: row.get(0)?,
        path: row.get(1)?,
        filename: row.get(2)?,
        extension: row.get(3)?,
        size: row.get(4)?,
        modified_time: row.get(5)?,
        is_dir: row.get::<_, i32>(6)? != 0,
    })
}

/// Batch-upsert a slice of records inside a single transaction.
/// Uses INSERT … ON CONFLICT DO UPDATE which fires the UPDATE trigger,
/// keeping the FTS index consistent.
pub fn upsert_batch(db: &Db, records: &[FileRecord]) -> Result<(), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;

    conn.execute("BEGIN", []).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare(
            "INSERT INTO files (path, filename, extension, size, modified_time, is_dir)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(path) DO UPDATE SET
               filename      = excluded.filename,
               extension     = excluded.extension,
               size          = excluded.size,
               modified_time = excluded.modified_time,
               is_dir        = excluded.is_dir,
               indexed_at    = strftime('%s','now')",
        )
        .map_err(|e| e.to_string())?;

    for r in records {
        stmt.execute(params![
            r.path,
            r.filename,
            r.extension,
            r.size,
            r.modified_time,
            r.is_dir as i32,
        ])
        .map_err(|e| e.to_string())?;
    }
    drop(stmt);
    conn.execute("COMMIT", []).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn remove_paths(db: &Db, paths: &[String]) -> Result<(), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    conn.execute("BEGIN", []).map_err(|e| e.to_string())?;
    for path in paths {
        conn.execute("DELETE FROM files WHERE path = ?1", params![path])
            .map_err(|e| e.to_string())?;
    }
    conn.execute("COMMIT", []).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn file_count(db: &Db) -> Result<i64, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
        .map_err(|e| e.to_string())
}
