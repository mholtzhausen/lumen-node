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

// ---------------------------------------------------------------------------
// Database path & connection
// ---------------------------------------------------------------------------

/// Returns the path to `.lumen-node.db` inside `folder`.
pub fn db_path(folder: &Path) -> PathBuf {
    folder.join(".lumen-node.db")
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
        CREATE INDEX IF NOT EXISTS idx_images_hash ON images(hash);",
    )?;

    // Migration path for databases created before the `favourite` column existed.
    let mut stmt = conn.prepare("PRAGMA table_info(images)")?;
    let cols: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .filter_map(|r| r.ok())
        .collect();
    if !cols.iter().any(|c| c == "favourite") {
        conn.execute_batch(
            "ALTER TABLE images ADD COLUMN favourite INTEGER NOT NULL DEFAULT 0;",
        )?;
    }

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
            conn.execute("DELETE FROM images WHERE path = ?1", params![p])?;
        }
    }
    Ok(())
}

/// Ensures the image at `path` is fully indexed: hashed, metadata extracted,
/// thumbnail generated, and stored in the DB. Returns the `ImageRow`.
pub fn ensure_indexed(conn: &Connection, path: &Path) -> Option<ImageRow> {
    // Fast path: DB cache hit with matching mtime+size.
    if let Some(cached) = get_cached(conn, path) {
        // Also ensure the thumbnail file still exists on disk.
        let thumb = crate::thumbnails::hash_thumb_path(&cached.hash);
        if thumb.exists() {
            return Some(cached);
        }
        // Thumbnail missing — regenerate it but keep cached metadata.
        crate::thumbnails::generate_hash_thumbnail(path, &cached.hash);
        return Some(cached);
    }

    // Slow path: hash + extract + thumbnail + DB upsert.
    let row = build_index_row(conn, path)?;

    let _ = upsert(conn, &row);
    Some(row)
}

/// Forces full re-indexing even when mtime + size are unchanged.
pub fn refresh_indexed(conn: &Connection, path: &Path) -> Option<ImageRow> {
    let row = build_index_row(conn, path)?;
    let _ = upsert(conn, &row);
    Some(row)
}
