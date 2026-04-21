//! Freedesktop Thumbnail Standard implementation.
//!
//! Specification: <https://specifications.freedesktop.org/thumbnail-spec/latest/>
//!
//! Thumbnails live under `$XDG_CACHE_HOME/thumbnails/normal/` (128×128) and
//! `large/` (256×256). Each file is named `{lowercase-md5(file:// URI)}.png`
//! and carries two `tEXt` chunks:
//!
//! | key              | value                                       |
//! |------------------|---------------------------------------------|
//! | `Thumb::URI`     | absolute `file://` URI of the source image  |
//! | `Thumb::MTime`   | Unix mtime seconds of the source file       |
//!
//! A cached thumbnail is considered *valid* only when its stored `Thumb::MTime`
//! matches the current mtime of the source file.

use gdk_pixbuf::Pixbuf;
use md5::{Digest, Md5};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Edge size for "normal" thumbnails (128 px).
pub const THUMB_NORMAL_SIZE: i32 = 128;

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Returns `$XDG_CACHE_HOME/thumbnails/normal`.
pub fn normal_cache_dir() -> PathBuf {
    xdg_cache_home().join("thumbnails/normal")
}

/// Returns `$XDG_CACHE_HOME/thumbnails/lumen-node` for hash-based thumbnails.
pub fn hash_cache_dir() -> PathBuf {
    xdg_cache_home().join("thumbnails/lumen-node")
}

fn xdg_cache_home() -> PathBuf {
    std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            PathBuf::from(home).join(".cache")
        })
}

/// Returns the canonical `file://` URI for an absolute path.
pub fn file_uri(path: &Path) -> String {
    // Paths are always absolute here (from directory scans).
    format!("file://{}", path.display())
}

/// Returns the expected thumbnail cache path for a given source image.
pub fn thumb_path(source: &Path) -> PathBuf {
    let uri = file_uri(source);
    let mut hasher = Md5::new();
    hasher.update(uri.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    normal_cache_dir().join(format!("{digest}.png"))
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Returns the mtime of `source` as a Unix-seconds string, or `None`.
fn source_mtime(source: &Path) -> Option<String> {
    source
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs().to_string())
}

/// Checks whether the cached thumbnail at `thumb` is still valid for `source`.
///
/// Validity requires the thumbnail to exist **and** its stored `Thumb::MTime`
/// to equal the current mtime of the source file.
pub fn is_valid(thumb: &Path, source: &Path) -> bool {
    if !thumb.exists() {
        return false;
    }
    let Some(expected) = source_mtime(source) else {
        return false;
    };
    let Ok(file) = std::fs::File::open(thumb) else {
        return false;
    };
    let decoder = png::Decoder::new(BufReader::new(file));
    let Ok(reader) = decoder.read_info() else {
        return false;
    };
    let info = reader.info();

    // Check tEXt chunks first (most common writer)
    let stored = info
        .uncompressed_latin1_text
        .iter()
        .find(|c| c.keyword == "Thumb::MTime")
        .map(|c| c.text.clone())
        // Fall back to iTXt chunks
        .or_else(|| {
            info.utf8_text
                .iter()
                .find(|c| c.keyword == "Thumb::MTime")
                .and_then(|c| c.get_text().ok())
        });

    stored.as_deref() == Some(expected.as_str())
}

// ---------------------------------------------------------------------------
// Load / generate
// ---------------------------------------------------------------------------

/// Ensures a valid thumbnail for `source` is present in the Freedesktop cache.
/// Returns the path to the cached thumbnail PNG on success.
///
/// Safe to call from any thread — no interaction with the GTK main loop.
/// Used by the tile-grid factory to generate thumbnails off the UI thread.
pub fn ensure_thumbnail(source: &Path) -> Option<PathBuf> {
    let thumb = thumb_path(source);
    if is_valid(&thumb, source) {
        return Some(thumb);
    }
    // generate_and_cache writes the PNG to `thumb`; ignore the returned Pixbuf.
    generate_and_cache(source, &thumb).map(|_| thumb)
}

/// Scales `source` to `THUMB_NORMAL_SIZE` and writes it to `thumb` with the
/// required Freedesktop `tEXt` metadata.
fn generate_and_cache(source: &Path, thumb: &Path) -> Option<Pixbuf> {
    let pixbuf = Pixbuf::from_file_at_scale(
        source,
        THUMB_NORMAL_SIZE,
        THUMB_NORMAL_SIZE,
        true, // preserve aspect ratio
    )
    .ok()?;

    // Ensure the cache directory exists; ignore errors (read-only FS, etc.)
    if let Some(parent) = thumb.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let uri = file_uri(source);
    let mtime = source_mtime(source).unwrap_or_default();

    // `savev` options follow the `tEXt::<key>` convention understood by gdk-pixbuf.
    let _ = pixbuf.savev(
        thumb,
        "png",
        &[
            ("tEXt::Thumb::URI", uri.as_str()),
            ("tEXt::Thumb::MTime", mtime.as_str()),
        ],
    );

    Some(pixbuf)
}

// ---------------------------------------------------------------------------
// Hash-based thumbnail storage (content-addressed)
// ---------------------------------------------------------------------------

/// Returns the path to a hash-addressed thumbnail.
pub fn hash_thumb_path(hash: &str) -> PathBuf {
    hash_cache_dir().join(format!("{hash}.png"))
}

/// Returns the path to a hash-addressed thumbnail for a specific size.
///
/// The default 128px thumbnail keeps the existing `{hash}.png` format.
/// Non-default sizes use `{hash}_{size}.png`.
pub fn hash_thumb_path_for_size(hash: &str, size: i32) -> PathBuf {
    if size == THUMB_NORMAL_SIZE {
        return hash_thumb_path(hash);
    }
    hash_cache_dir().join(format!("{hash}_{size}.png"))
}

/// Returns the size-specific hash-based thumbnail path if it already exists on disk.
pub fn hash_thumb_if_exists_for_size(hash: &str, size: i32) -> Option<PathBuf> {
    let p = hash_thumb_path_for_size(hash, size);
    if p.exists() { Some(p) } else { None }
}

/// Generates and saves a thumbnail keyed by content hash.
/// Returns the path to the generated thumbnail, or `None` on failure.
pub fn generate_hash_thumbnail(source: &Path, hash: &str) -> Option<PathBuf> {
    generate_hash_thumbnail_for_size(source, hash, THUMB_NORMAL_SIZE)
}

/// Generates and saves a size-specific thumbnail keyed by content hash.
/// Returns the path to the generated thumbnail, or `None` on failure.
pub fn generate_hash_thumbnail_for_size(source: &Path, hash: &str, size: i32) -> Option<PathBuf> {
    let thumb = hash_thumb_path_for_size(hash, size);
    if thumb.exists() {
        return Some(thumb);
    }
    let pixbuf = Pixbuf::from_file_at_scale(
        source,
        size,
        size,
        true,
    )
    .ok()?;

    if let Some(parent) = thumb.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let _ = pixbuf.savev(&thumb, "png", &[]);
    Some(thumb)
}

/// Removes the default and any size-specific hash thumbnail variants for `hash`.
pub fn remove_hash_thumbnail_variants(hash: &str) {
    let _ = std::fs::remove_file(hash_thumb_path(hash));

    let Ok(entries) = std::fs::read_dir(hash_cache_dir()) else {
        return;
    };
    let prefix = format!("{hash}_");
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name.starts_with(&prefix) && name.ends_with(".png") {
            let _ = std::fs::remove_file(path);
        }
    }
}
