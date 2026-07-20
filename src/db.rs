//! Per-folder SQLite database (`.lumen-node.db`) for image metadata caching.
//!
//! Each scanned folder gets its own database. The `images` table stores:
//! - file identity: path, filename, content hash (SHA-256), mtime, size
//! - EXIF metadata: camera make/model, exposure, ISO
//! - AI generation metadata: prompt, negative prompt, raw parameters, workflow
//!
//! Thumbnails are stored under `$XDG_CACHE_HOME/thumbnails/lumen-node/{hash}.png`,
//! keyed by content hash so duplicate images share a single thumbnail.

use rusqlite::{params, Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::metadata::{DefaultMetadataDispatcher, ImageMetadata, MetadataDispatcher};

/// Row returned from the database for a single image.
#[derive(Debug, Clone)]
pub struct ImageRow {
    pub path: String,
    pub filename: String,
    pub hash: String,
    pub mtime: i64,
    pub size: i64,
    pub favourite: i32,
    pub meta: ImageMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexOutcome {
    Cached,
    Generated,
}

#[derive(Debug, Clone)]
pub struct UiState {
    pub sort_key: String,
    pub search_text: String,
    pub favorites_only: bool,
    /// Active tag filter (AND). Persisted as JSON array under `active_tags`.
    pub active_tags: Vec<String>,
    pub thumbnail_size: i32,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            sort_key: "name_asc".to_string(),
            search_text: String::new(),
            favorites_only: false,
            active_tags: Vec::new(),
            thumbnail_size: crate::thumbnails::THUMB_NORMAL_SIZE,
        }
    }
}

const UI_STATE_SORT_KEY: &str = "sort_key";
const UI_STATE_SEARCH_TEXT: &str = "search_text";
const UI_STATE_FAVORITES_ONLY: &str = "favorites_only";
const UI_STATE_ACTIVE_TAGS: &str = "active_tags";
const UI_STATE_THUMBNAIL_SIZE: &str = "thumbnail_size";

/// Normalizes a free-form tag: trim whitespace. Empty after trim is rejected.
pub fn normalize_tag(tag: &str) -> Option<String> {
    let trimmed = tag.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Serializes active tags for `ui_state` (JSON array).
pub fn encode_active_tags(tags: &[String]) -> String {
    serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string())
}

/// Parses `active_tags` from `ui_state` (JSON array, or comma-separated fallback).
pub fn decode_active_tags(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if let Ok(parsed) = serde_json::from_str::<Vec<String>>(trimmed) {
        return parsed
            .into_iter()
            .filter_map(|t| normalize_tag(&t))
            .collect();
    }
    trimmed
        .split(',')
        .filter_map(normalize_tag)
        .collect()
}

// ---------------------------------------------------------------------------
// Database path & connection
// ---------------------------------------------------------------------------

/// Returns the path to `.lumen-node.db` inside `folder`.
pub fn db_path(folder: &Path) -> PathBuf {
    folder.join(".lumen-node.db")
}

/// Removes the per-folder DB file and SQLite sidecar files if they exist.
pub fn remove_db_files(folder: &Path) {
    let db = db_path(folder);
    let wal = folder.join(".lumen-node.db-wal");
    let shm = folder.join(".lumen-node.db-shm");
    let _ = std::fs::remove_file(db);
    let _ = std::fs::remove_file(wal);
    let _ = std::fs::remove_file(shm);
}

/// Opens (or creates) the per-folder database and ensures the schema exists.
pub fn open(folder: &Path) -> rusqlite::Result<Connection> {
    let path = db_path(folder);
    let conn = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
    create_schema(&conn)?;
    Ok(conn)
}

fn create_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS images (
            path            TEXT PRIMARY KEY,
            filename        TEXT NOT NULL,
            hash            TEXT NOT NULL,
            mtime           INTEGER NOT NULL,
            size            INTEGER NOT NULL,
            favourite       INTEGER NOT NULL DEFAULT 0,
            camera_make     TEXT,
            camera_model    TEXT,
            exposure        TEXT,
            iso             TEXT,
            prompt          TEXT,
            negative_prompt TEXT,
            raw_parameters  TEXT,
            workflow_json   TEXT
        );
        CREATE TABLE IF NOT EXISTS ui_state (
            key             TEXT PRIMARY KEY,
            value           TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS image_tags (
            path            TEXT NOT NULL,
            tag             TEXT NOT NULL,
            PRIMARY KEY (path, tag)
        );
        CREATE INDEX IF NOT EXISTS idx_images_hash ON images(hash);
        CREATE INDEX IF NOT EXISTS idx_image_tags_tag ON image_tags(tag);",
    )?;

    // Migration path for databases created before the `favourite` column existed.
    let mut stmt = conn.prepare("PRAGMA table_info(images)")?;
    let cols: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(|r| r.ok())
        .collect();
    if !cols.iter().any(|c| c == "favourite") {
        conn.execute_batch("ALTER TABLE images ADD COLUMN favourite INTEGER NOT NULL DEFAULT 0;")?;
    }

    Ok(())
}

pub fn load_ui_state(folder: &Path) -> Option<UiState> {
    let conn = open(folder).ok()?;
    let mut stmt = conn.prepare("SELECT key, value FROM ui_state").ok()?;
    let rows = stmt
        .query_map([], |row| {
            let key: String = row.get(0)?;
            let value: String = row.get(1)?;
            Ok((key, value))
        })
        .ok()?;

    let mut state = UiState::default();
    let mut has_any_value = false;
    for row in rows {
        let Ok((key, value)) = row else { continue };
        has_any_value = true;
        match key.as_str() {
            UI_STATE_SORT_KEY => {
                if !value.trim().is_empty() {
                    state.sort_key = value;
                }
            }
            UI_STATE_SEARCH_TEXT => {
                state.search_text = value;
            }
            UI_STATE_FAVORITES_ONLY => {
                let normalized = value.trim().to_ascii_lowercase();
                state.favorites_only = normalized == "1" || normalized == "true";
            }
            UI_STATE_ACTIVE_TAGS => {
                state.active_tags = decode_active_tags(&value);
            }
            UI_STATE_THUMBNAIL_SIZE => {
                if let Ok(parsed) = value.trim().parse::<i32>() {
                    state.thumbnail_size = parsed;
                }
            }
            _ => {}
        }
    }

    if has_any_value {
        Some(state)
    } else {
        None
    }
}

pub fn save_ui_state(folder: &Path, state: &UiState) -> rusqlite::Result<()> {
    let conn = open(folder)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO ui_state(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![UI_STATE_SORT_KEY, state.sort_key.as_str()],
    )?;
    tx.execute(
        "INSERT INTO ui_state(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![UI_STATE_SEARCH_TEXT, state.search_text.as_str()],
    )?;
    tx.execute(
        "INSERT INTO ui_state(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![
            UI_STATE_FAVORITES_ONLY,
            if state.favorites_only { "1" } else { "0" }
        ],
    )?;
    tx.execute(
        "INSERT INTO ui_state(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![
            UI_STATE_ACTIVE_TAGS,
            encode_active_tags(&state.active_tags)
        ],
    )?;
    tx.execute(
        "INSERT INTO ui_state(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![UI_STATE_THUMBNAIL_SIZE, state.thumbnail_size.to_string()],
    )?;
    tx.commit()?;
    Ok(())
}

pub fn set_ui_state_value(folder: &Path, key: &str, value: &str) -> rusqlite::Result<()> {
    let conn = open(folder)?;
    conn.execute(
        "INSERT INTO ui_state(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// File hashing
// ---------------------------------------------------------------------------

/// Computes the SHA-256 hex digest of the file at `path`.
pub fn hash_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Returns file mtime as Unix seconds.
pub fn file_mtime(path: &Path) -> Option<i64> {
    path.metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

/// Returns file size in bytes.
pub fn file_size(path: &Path) -> Option<i64> {
    path.metadata().ok().map(|m| m.len() as i64)
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

/// Looks up a cached row. Returns `Some` only if mtime + size still match.
pub fn get_cached(conn: &Connection, path: &Path) -> Option<ImageRow> {
    let path_str = path.to_string_lossy();
    let current_mtime = file_mtime(path)?;
    let current_size = file_size(path)?;

    let mut stmt = conn
        .prepare_cached(
            "SELECT path, filename, hash, mtime, size, favourite,
                    camera_make, camera_model, exposure, iso,
                    prompt, negative_prompt, raw_parameters, workflow_json
             FROM images WHERE path = ?1",
        )
        .ok()?;

    let row = stmt
        .query_row(params![path_str.as_ref()], |row| {
            Ok(ImageRow {
                path: row.get(0)?,
                filename: row.get(1)?,
                hash: row.get(2)?,
                mtime: row.get(3)?,
                size: row.get(4)?,
                favourite: row.get(5)?,
                meta: ImageMetadata {
                    camera_make: row.get(6)?,
                    camera_model: row.get(7)?,
                    exposure: row.get(8)?,
                    iso: row.get(9)?,
                    prompt: row.get(10)?,
                    negative_prompt: row.get(11)?,
                    raw_parameters: row.get(12)?,
                    workflow_json: row.get(13)?,
                },
            })
        })
        .ok()?;

    // Stale check: mtime or size changed → need re-index.
    if row.mtime == current_mtime && row.size == current_size {
        Some(row)
    } else {
        None
    }
}

/// Inserts or replaces a row for the given image.
pub fn upsert(conn: &Connection, row: &ImageRow) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO images
         (path, filename, hash, mtime, size, favourite,
          camera_make, camera_model, exposure, iso,
          prompt, negative_prompt, raw_parameters, workflow_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            row.path,
            row.filename,
            row.hash,
            row.mtime,
            row.size,
            row.favourite,
            row.meta.camera_make,
            row.meta.camera_model,
            row.meta.exposure,
            row.meta.iso,
            row.meta.prompt,
            row.meta.negative_prompt,
            row.meta.raw_parameters,
            row.meta.workflow_json,
        ],
    )?;
    Ok(())
}

fn favourite_for_path(conn: &Connection, path: &Path) -> i32 {
    let path_str = path.to_string_lossy();
    conn.query_row(
        "SELECT favourite FROM images WHERE path = ?1",
        params![path_str.as_ref()],
        |row| row.get::<_, i32>(0),
    )
    .unwrap_or(0)
}

fn build_index_row(conn: &Connection, path: &Path) -> Option<ImageRow> {
    let hash = hash_file(path).ok()?;
    let mtime = file_mtime(path)?;
    let size = file_size(path)?;
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let dispatcher = DefaultMetadataDispatcher;
    let meta = dispatcher.extract(path).unwrap_or_default();

    // Generate thumbnail (keyed by content hash).
    crate::thumbnails::generate_hash_thumbnail(path, &hash);

    Some(ImageRow {
        path: path.to_string_lossy().into_owned(),
        filename,
        hash,
        mtime,
        size,
        favourite: favourite_for_path(conn, path),
        meta,
    })
}

/// Removes rows for paths that no longer exist on disk.
pub fn prune_missing(conn: &Connection) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare("SELECT path FROM images")?;
    let paths: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();
    for p in &paths {
        if !Path::new(p).exists() {
            conn.execute("DELETE FROM image_tags WHERE path = ?1", params![p])?;
            conn.execute("DELETE FROM images WHERE path = ?1", params![p])?;
        }
    }
    Ok(())
}

/// Ensures the image at `path` is fully indexed (hash, metadata, thumbnail),
/// and returns whether work came from cache or required fresh generation.
pub fn ensure_indexed_with_outcome(
    conn: &Connection,
    path: &Path,
) -> Option<(ImageRow, IndexOutcome)> {
    // Fast path: DB cache hit with matching mtime+size.
    if let Some(cached) = get_cached(conn, path) {
        // Also ensure the thumbnail file still exists on disk.
        let thumb = crate::thumbnails::hash_thumb_path(&cached.hash);
        if thumb.exists() {
            return Some((cached, IndexOutcome::Cached));
        }
        // Thumbnail missing — regenerate it but keep cached metadata.
        crate::thumbnails::generate_hash_thumbnail(path, &cached.hash);
        return Some((cached, IndexOutcome::Generated));
    }

    // Slow path: hash + extract + thumbnail + DB upsert.
    let row = build_index_row(conn, path)?;

    let _ = upsert(conn, &row);
    Some((row, IndexOutcome::Generated))
}

/// Forces full re-indexing even when mtime + size are unchanged.
pub fn refresh_indexed(conn: &Connection, path: &Path) -> Option<ImageRow> {
    let row = build_index_row(conn, path)?;
    let _ = upsert(conn, &row);
    Some(row)
}

/// Flips the `favourite` flag for an indexed image. Returns `None` if there is no DB row yet.
/// Returns whether an `images` row exists for `path` (image has been indexed in this folder DB).
pub fn image_row_exists(conn: &Connection, path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    matches!(
        conn.query_row(
            "SELECT 1 FROM images WHERE path = ?1 LIMIT 1",
            params![path_str.as_ref()],
            |_| Ok(()),
        ),
        Ok(())
    )
}

/// Returns whether the image row exists and is marked favourite (`None` if no row).
pub fn get_favourite(conn: &Connection, path: &Path) -> rusqlite::Result<Option<bool>> {
    let path_str = path.to_string_lossy();
    match conn.query_row(
        "SELECT favourite FROM images WHERE path = ?1",
        params![path_str.as_ref()],
        |row| row.get::<_, i32>(0),
    ) {
        Ok(v) => Ok(Some(v != 0)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Sets `favourite` for an indexed image. Returns `false` if no row exists for `path`.
pub fn set_favourite(conn: &Connection, path: &Path, favourite: bool) -> rusqlite::Result<bool> {
    let path_str = path.to_string_lossy();
    let v: i32 = if favourite { 1 } else { 0 };
    let n = conn.execute(
        "UPDATE images SET favourite = ?1 WHERE path = ?2",
        params![v, path_str.as_ref()],
    )?;
    Ok(n > 0)
}

/// Deletes one indexed image row by absolute file path.
pub fn remove_image_row(conn: &Connection, path: &Path) -> rusqlite::Result<bool> {
    let path_str = path.to_string_lossy();
    conn.execute(
        "DELETE FROM image_tags WHERE path = ?1",
        params![path_str.as_ref()],
    )?;
    let affected = conn.execute(
        "DELETE FROM images WHERE path = ?1",
        params![path_str.as_ref()],
    )?;
    Ok(affected > 0)
}

/// Same-folder path move: migrate favourite + tags, remove the old row, re-index the destination.
pub fn move_image_row(conn: &Connection, old_path: &Path, new_path: &Path) -> Option<ImageRow> {
    let favourite = favourite_for_path(conn, old_path);
    let _ = move_tags(conn, old_path, new_path);
    let _ = remove_image_row(conn, old_path);
    let mut row = refresh_indexed(conn, new_path)?;
    if favourite != 0 {
        let _ = set_favourite(conn, new_path, true);
        row.favourite = favourite;
    }
    Some(row)
}

/// Cross-folder move: copy favourite + tags from `source_conn`, remove the source row, index into `dest_conn`.
pub fn relocate_image_row(
    source_conn: &Connection,
    dest_conn: &Connection,
    old_path: &Path,
    new_path: &Path,
) -> Option<ImageRow> {
    let favourite = favourite_for_path(source_conn, old_path);
    let tags = list_tags_for_path(source_conn, old_path).unwrap_or_default();
    let _ = remove_image_row(source_conn, old_path);
    let mut row = refresh_indexed(dest_conn, new_path)?;
    if favourite != 0 {
        let _ = set_favourite(dest_conn, new_path, true);
        row.favourite = favourite;
    }
    for tag in &tags {
        let _ = add_tag(dest_conn, new_path, tag);
    }
    Some(row)
}

// ---------------------------------------------------------------------------
// Tags
// ---------------------------------------------------------------------------

/// Lists tags for one image path (sorted).
pub fn list_tags_for_path(conn: &Connection, path: &Path) -> rusqlite::Result<Vec<String>> {
    let path_str = path.to_string_lossy();
    let mut stmt = conn.prepare(
        "SELECT tag FROM image_tags WHERE path = ?1 ORDER BY tag COLLATE NOCASE",
    )?;
    let rows = stmt.query_map(params![path_str.as_ref()], |row| row.get::<_, String>(0))?;
    let mut tags = Vec::new();
    for row in rows {
        tags.push(row?);
    }
    Ok(tags)
}

/// Lists distinct tags used in this folder DB (sorted).
pub fn list_all_tags_in_folder(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT DISTINCT tag FROM image_tags ORDER BY tag COLLATE NOCASE")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut tags = Vec::new();
    for row in rows {
        tags.push(row?);
    }
    Ok(tags)
}

/// Adds a tag to an image. Returns `false` if the tag is empty after normalize
/// or no `images` row exists for `path`. Idempotent when the tag already exists.
pub fn add_tag(conn: &Connection, path: &Path, tag: &str) -> rusqlite::Result<bool> {
    let Some(tag) = normalize_tag(tag) else {
        return Ok(false);
    };
    if !image_row_exists(conn, path) {
        return Ok(false);
    }
    let path_str = path.to_string_lossy();
    conn.execute(
        "INSERT OR IGNORE INTO image_tags(path, tag) VALUES(?1, ?2)",
        params![path_str.as_ref(), tag],
    )?;
    Ok(true)
}

/// Removes a tag from an image. Returns `true` if a row was deleted.
pub fn remove_tag(conn: &Connection, path: &Path, tag: &str) -> rusqlite::Result<bool> {
    let Some(tag) = normalize_tag(tag) else {
        return Ok(false);
    };
    let path_str = path.to_string_lossy();
    let n = conn.execute(
        "DELETE FROM image_tags WHERE path = ?1 AND tag = ?2",
        params![path_str.as_ref(), tag],
    )?;
    Ok(n > 0)
}

/// Moves all tags from `old_path` to `new_path` (e.g. after rename).
pub fn move_tags(conn: &Connection, old_path: &Path, new_path: &Path) -> rusqlite::Result<()> {
    let old = old_path.to_string_lossy();
    let new = new_path.to_string_lossy();
    if old.as_ref() == new.as_ref() {
        return Ok(());
    }
    conn.execute(
        "UPDATE OR IGNORE image_tags SET path = ?1 WHERE path = ?2",
        params![new.as_ref(), old.as_ref()],
    )?;
    conn.execute(
        "DELETE FROM image_tags WHERE path = ?1",
        params![old.as_ref()],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::ops::Deref;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestDir {
        path: PathBuf,
    }

    impl Deref for TestDir {
        type Target = Path;

        fn deref(&self) -> &Self::Target {
            self.path.as_path()
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    /// Creates a unique temporary directory per test call.
    fn temp_dir() -> TestDir {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("lumen-node-test-{}-{}", std::process::id(), id));
        let _ = std::fs::create_dir_all(&dir);
        TestDir { path: dir }
    }

    /// Writes known content to a file, returns the path.
    fn write_temp_file(dir: &Path, name: &str, content: &[u8]) -> PathBuf {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content).unwrap();
        path
    }

    #[test]
    fn test_hash_file_known_content() {
        let dir = temp_dir();
        let path = write_temp_file(&dir, "test.bin", b"hello world");
        let hash = hash_file(&path).unwrap();
        // SHA-256 of "hello world"
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_hash_file_empty() {
        let dir = temp_dir();
        let path = write_temp_file(&dir, "empty.bin", b"");
        let hash = hash_file(&path).unwrap();
        // SHA-256 of empty string
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_file_mtime_and_size() {
        let dir = temp_dir();
        let path = write_temp_file(&dir, "meta.txt", b"1234567890");
        assert_eq!(file_size(&path), Some(10));
        let mtime = file_mtime(&path);
        assert!(mtime.is_some());
        assert!(mtime.unwrap() > 1_700_000_000); // reasonable Unix timestamp for 2024+
    }

    #[test]
    fn test_db_open_creates_schema() {
        let dir = temp_dir();
        let db_path = db_path(&dir);
        assert!(!db_path.exists());
        let conn = open(&dir).unwrap();
        // DB file should exist now
        assert!(db_path.exists());
        // images table should be queryable
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM images", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        // ui_state table should exist
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM ui_state", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        // image_tags table should exist
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM image_tags", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_upsert_and_get_cached() {
        let dir = temp_dir();
        let conn = open(&dir).unwrap();
        let path = write_temp_file(&dir, "photo.jpg", b"fake-jpeg-data");
        let hash = hash_file(&path).unwrap();
        let mtime = file_mtime(&path).unwrap();
        let size = file_size(&path).unwrap();

        let row = ImageRow {
            path: path.to_string_lossy().into_owned(),
            filename: "photo.jpg".to_string(),
            hash: hash.clone(),
            mtime,
            size,
            favourite: 0,
            meta: ImageMetadata::default(),
        };
        upsert(&conn, &row).unwrap();

        // get_cached should find it
        let cached = get_cached(&conn, &path);
        assert!(cached.is_some());
        let cached = cached.unwrap();
        assert_eq!(cached.hash, hash);
        assert_eq!(cached.filename, "photo.jpg");
        assert_eq!(cached.favourite, 0);
    }

    #[test]
    fn test_get_cached_stale_on_mtime_change() {
        let dir = temp_dir();
        let conn = open(&dir).unwrap();
        let path = write_temp_file(&dir, "stale.jpg", b"original content");
        let hash = hash_file(&path).unwrap();
        let mtime = file_mtime(&path).unwrap();
        let size = file_size(&path).unwrap();

        let row = ImageRow {
            path: path.to_string_lossy().into_owned(),
            filename: "stale.jpg".to_string(),
            hash,
            mtime,
            size,
            favourite: 0,
            meta: ImageMetadata::default(),
        };
        upsert(&conn, &row).unwrap();

        // Modify file with different content + size so staleness is unambiguous
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&path, b"modified!").unwrap();

        // get_cached should return None (stale — mtime and size differ)
        let cached = get_cached(&conn, &path);
        assert!(cached.is_none());
    }

    #[test]
    fn test_prune_missing() {
        let dir = temp_dir();
        let conn = open(&dir).unwrap();

        // Insert a row for a file that exists
        let existing = write_temp_file(&dir, "exists.jpg", b"data");
        let hash = hash_file(&existing).unwrap();
        let mtime = file_mtime(&existing).unwrap();
        let size = file_size(&existing).unwrap();
        upsert(
            &conn,
            &ImageRow {
                path: existing.to_string_lossy().into_owned(),
                filename: "exists.jpg".to_string(),
                hash,
                mtime,
                size,
                favourite: 0,
                meta: ImageMetadata::default(),
            },
        )
        .unwrap();

        // Insert a row for a file that does not exist
        let missing = dir.join("missing.jpg");
        upsert(
            &conn,
            &ImageRow {
                path: missing.to_string_lossy().into_owned(),
                filename: "missing.jpg".to_string(),
                hash: "fakehash".to_string(),
                mtime: 1_000_000,
                size: 100,
                favourite: 0,
                meta: ImageMetadata::default(),
            },
        )
        .unwrap();

        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM images", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );

        prune_missing(&conn).unwrap();

        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM images", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn test_favourite_roundtrip() {
        let dir = temp_dir();
        let conn = open(&dir).unwrap();
        let path = write_temp_file(&dir, "fav.jpg", b"data");
        let hash = hash_file(&path).unwrap();
        let mtime = file_mtime(&path).unwrap();
        let size = file_size(&path).unwrap();

        // Not indexed yet — get_favourite returns None
        assert!(get_favourite(&conn, &path).unwrap().is_none());

        // Index the image
        let row = ImageRow {
            path: path.to_string_lossy().into_owned(),
            filename: "fav.jpg".to_string(),
            hash,
            mtime,
            size,
            favourite: 0,
            meta: ImageMetadata::default(),
        };
        upsert(&conn, &row).unwrap();

        assert_eq!(get_favourite(&conn, &path).unwrap(), Some(false));

        set_favourite(&conn, &path, true).unwrap();
        assert_eq!(get_favourite(&conn, &path).unwrap(), Some(true));

        set_favourite(&conn, &path, false).unwrap();
        assert_eq!(get_favourite(&conn, &path).unwrap(), Some(false));
    }

    #[test]
    fn test_tag_roundtrip() {
        let dir = temp_dir();
        let conn = open(&dir).unwrap();
        let path = write_temp_file(&dir, "tagged.jpg", b"data");
        let hash = hash_file(&path).unwrap();
        let mtime = file_mtime(&path).unwrap();
        let size = file_size(&path).unwrap();

        // Not indexed — add_tag returns false
        assert!(!add_tag(&conn, &path, "keep").unwrap());
        assert!(list_tags_for_path(&conn, &path).unwrap().is_empty());

        upsert(
            &conn,
            &ImageRow {
                path: path.to_string_lossy().into_owned(),
                filename: "tagged.jpg".to_string(),
                hash,
                mtime,
                size,
                favourite: 0,
                meta: ImageMetadata::default(),
            },
        )
        .unwrap();

        assert!(add_tag(&conn, &path, "  keep ").unwrap());
        assert!(add_tag(&conn, &path, "style").unwrap());
        // Idempotent
        assert!(add_tag(&conn, &path, "keep").unwrap());
        // Empty rejected
        assert!(!add_tag(&conn, &path, "   ").unwrap());

        let tags = list_tags_for_path(&conn, &path).unwrap();
        assert_eq!(tags, vec!["keep".to_string(), "style".to_string()]);
        assert_eq!(
            list_all_tags_in_folder(&conn).unwrap(),
            vec!["keep".to_string(), "style".to_string()]
        );

        assert!(remove_tag(&conn, &path, "keep").unwrap());
        assert_eq!(
            list_tags_for_path(&conn, &path).unwrap(),
            vec!["style".to_string()]
        );
        assert!(!remove_tag(&conn, &path, "missing").unwrap());

        let renamed = dir.join("renamed.jpg");
        std::fs::rename(&path, &renamed).unwrap();
        move_tags(&conn, &path, &renamed).unwrap();
        assert!(list_tags_for_path(&conn, &path).unwrap().is_empty());
        assert_eq!(
            list_tags_for_path(&conn, &renamed).unwrap(),
            vec!["style".to_string()]
        );
    }

    #[test]
    fn test_move_image_row_preserves_favourite_and_tags() {
        let dir = temp_dir();
        let conn = open(&dir).unwrap();
        let path = write_temp_file(&dir, "kept.jpg", b"move-row-data");
        let hash = hash_file(&path).unwrap();
        let mtime = file_mtime(&path).unwrap();
        let size = file_size(&path).unwrap();

        upsert(
            &conn,
            &ImageRow {
                path: path.to_string_lossy().into_owned(),
                filename: "kept.jpg".to_string(),
                hash,
                mtime,
                size,
                favourite: 0,
                meta: ImageMetadata::default(),
            },
        )
        .unwrap();
        set_favourite(&conn, &path, true).unwrap();
        assert!(add_tag(&conn, &path, "keep").unwrap());
        assert!(add_tag(&conn, &path, "style").unwrap());

        let renamed = dir.join("kept-renamed.jpg");
        std::fs::rename(&path, &renamed).unwrap();
        let row = move_image_row(&conn, &path, &renamed).expect("move_image_row");
        assert_eq!(row.favourite, 1);
        assert_eq!(get_favourite(&conn, &renamed).unwrap(), Some(true));
        assert!(get_favourite(&conn, &path).unwrap().is_none());
        assert!(list_tags_for_path(&conn, &path).unwrap().is_empty());
        assert_eq!(
            list_tags_for_path(&conn, &renamed).unwrap(),
            vec!["keep".to_string(), "style".to_string()]
        );
    }

    #[test]
    fn test_relocate_image_row_preserves_favourite_and_tags() {
        let src_dir = temp_dir();
        let dest_dir = temp_dir();
        let source_conn = open(&src_dir).unwrap();
        let dest_conn = open(&dest_dir).unwrap();
        let path = write_temp_file(&src_dir, "xfer.jpg", b"relocate-data");
        let hash = hash_file(&path).unwrap();
        let mtime = file_mtime(&path).unwrap();
        let size = file_size(&path).unwrap();

        upsert(
            &source_conn,
            &ImageRow {
                path: path.to_string_lossy().into_owned(),
                filename: "xfer.jpg".to_string(),
                hash,
                mtime,
                size,
                favourite: 0,
                meta: ImageMetadata::default(),
            },
        )
        .unwrap();
        set_favourite(&source_conn, &path, true).unwrap();
        assert!(add_tag(&source_conn, &path, "archive").unwrap());

        let dest = dest_dir.join("xfer.jpg");
        std::fs::rename(&path, &dest).unwrap();
        let row =
            relocate_image_row(&source_conn, &dest_conn, &path, &dest).expect("relocate_image_row");
        assert_eq!(row.favourite, 1);
        assert!(get_favourite(&source_conn, &path).unwrap().is_none());
        assert_eq!(get_favourite(&dest_conn, &dest).unwrap(), Some(true));
        assert!(list_tags_for_path(&source_conn, &path).unwrap().is_empty());
        assert_eq!(
            list_tags_for_path(&dest_conn, &dest).unwrap(),
            vec!["archive".to_string()]
        );
    }

    #[test]
    fn test_ui_state_roundtrip() {
        let dir = temp_dir();
        let state = UiState {
            sort_key: "date_desc".to_string(),
            search_text: "sunset".to_string(),
            favorites_only: true,
            active_tags: vec!["keep".to_string(), "archive".to_string()],
            thumbnail_size: 256,
        };
        save_ui_state(&dir, &state).unwrap();

        let loaded = load_ui_state(&dir).unwrap();
        assert_eq!(loaded.sort_key, "date_desc");
        assert_eq!(loaded.search_text, "sunset");
        assert!(loaded.favorites_only);
        assert_eq!(
            loaded.active_tags,
            vec!["keep".to_string(), "archive".to_string()]
        );
        assert_eq!(loaded.thumbnail_size, 256);
    }

    #[test]
    fn test_decode_active_tags_formats() {
        assert_eq!(
            decode_active_tags(r#"["a","b"]"#),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            decode_active_tags("keep, archive"),
            vec!["keep".to_string(), "archive".to_string()]
        );
        assert!(decode_active_tags("").is_empty());
    }
}
