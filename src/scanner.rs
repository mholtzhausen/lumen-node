//! Directory scanning for LumenNode.
//!
//! [`scan_directory`] spawns a background thread that walks the immediate
//! contents of a folder in two phases:
//!
//! **Phase 1 (enumerate):** Quickly lists all recognised image files and
//! sends [`ScanMessage::ImageEnumerated`] for each path. This lets the UI
//! populate the grid almost instantly.
//!
//! **Phase 2 (enrich):** Opens (or creates) the per-folder `.lumen-node.db`
//! and processes each image in the user's current sort order (so thumbnails
//! appear top-down). Sends [`ScanMessage::ImageEnriched`] with hash +
//! metadata, followed by [`ScanMessage::ScanComplete`].

use crate::db;
use crate::metadata::ScanMessage;
use async_channel::Sender;
use std::path::PathBuf;

/// Image file extensions recognised by LumenNode.
const IMAGE_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "tiff", "tif", "bmp", "avif",
];

/// Spawns a background thread that scans `dir` for image files.
///
/// `sort_key` determines the order in which files are enriched during phase 2
/// so that thumbnails appear from the top of the sorted grid downward.
pub fn scan_directory(dir: PathBuf, sender: Sender<ScanMessage>, sort_key: String) {
    std::thread::spawn(move || {
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            let _ = sender.send_blocking(ScanMessage::ScanComplete);
            return;
        };

        // Collect image paths (phase 1: fast enumerate).
        let mut paths: Vec<PathBuf> = Vec::new();
        for entry in read_dir.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file() && is_image(&path) {
                let path_str = path.to_string_lossy().into_owned();
                let _ = sender.send_blocking(ScanMessage::ImageEnumerated { path: path_str });
                paths.push(path);
            }
        }

        let _ = sender.send_blocking(ScanMessage::EnumerationComplete);

        // Sort paths according to the user's current sort key so that
        // enrichment (hash + metadata + thumbnail) proceeds top-down.
        sort_paths(&mut paths, &sort_key);

        // Open (or create) the per-folder database.
        let conn = match db::open(&dir) {
            Ok(c) => c,
            Err(_) => {
                let _ = sender.send_blocking(ScanMessage::ScanComplete);
                return;
            }
        };

        // Phase 2: enrich each file in sort order.
        for path in &paths {
            let path_str = path.to_string_lossy().into_owned();
            let maybe_row = db::ensure_indexed(&conn, path);

            let (hash, meta) = match maybe_row {
                Some(row) => (row.hash, row.meta),
                None => (String::new(), Default::default()),
            };

            if sender
                .send_blocking(ScanMessage::ImageEnriched {
                    path: path_str,
                    hash,
                    meta,
                })
                .is_err()
            {
                return;
            }

            // Yield to let the OS scheduler favour the UI thread.
            std::thread::yield_now();
        }

        // Prune DB entries for files that no longer exist on disk.
        let _ = db::prune_missing(&conn);

        let _ = sender.send_blocking(ScanMessage::ScanComplete);
    });
}

/// Sort paths to match the user-facing sort order.
fn sort_paths(paths: &mut Vec<PathBuf>, sort_key: &str) {
    match sort_key {
        "name_asc" => paths.sort_by(|a, b| {
            let na = a.file_name().map(|n| n.to_ascii_lowercase()).unwrap_or_default();
            let nb = b.file_name().map(|n| n.to_ascii_lowercase()).unwrap_or_default();
            na.cmp(&nb)
        }),
        "name_desc" => paths.sort_by(|a, b| {
            let na = a.file_name().map(|n| n.to_ascii_lowercase()).unwrap_or_default();
            let nb = b.file_name().map(|n| n.to_ascii_lowercase()).unwrap_or_default();
            nb.cmp(&na)
        }),
        "date_asc" => paths.sort_by(|a, b| {
            let ta = std::fs::metadata(a).and_then(|m| m.modified()).ok();
            let tb = std::fs::metadata(b).and_then(|m| m.modified()).ok();
            ta.cmp(&tb)
        }),
        "date_desc" => paths.sort_by(|a, b| {
            let ta = std::fs::metadata(a).and_then(|m| m.modified()).ok();
            let tb = std::fs::metadata(b).and_then(|m| m.modified()).ok();
            tb.cmp(&ta)
        }),
        "size_asc" => paths.sort_by(|a, b| {
            let sa = std::fs::metadata(a).map(|m| m.len()).unwrap_or(0);
            let sb = std::fs::metadata(b).map(|m| m.len()).unwrap_or(0);
            sa.cmp(&sb)
        }),
        "size_desc" => paths.sort_by(|a, b| {
            let sa = std::fs::metadata(a).map(|m| m.len()).unwrap_or(0);
            let sb = std::fs::metadata(b).map(|m| m.len()).unwrap_or(0);
            sb.cmp(&sa)
        }),
        _ => paths.sort_by(|a, b| {
            let na = a.file_name().map(|n| n.to_ascii_lowercase()).unwrap_or_default();
            let nb = b.file_name().map(|n| n.to_ascii_lowercase()).unwrap_or_default();
            na.cmp(&nb)
        }),
    }
}

fn is_image(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}
