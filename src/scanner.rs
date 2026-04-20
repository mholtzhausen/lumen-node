//! Directory scanning for LumenNode.
//!
//! [`scan_directory`] spawns a background thread that walks the immediate
//! contents of a folder, opens (or creates) the per-folder `.lumen-node.db`,
//! and emits [`ScanMessage::ImageFound`] for every recognised image file,
//! followed by [`ScanMessage::ScanComplete`].
//!
//! Each discovered image is indexed into the database (hash, metadata,
//! thumbnail) via [`crate::db::ensure_indexed`]. The database is pruned of
//! entries for files that no longer exist.

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
/// Each discovered file is sent as [`ScanMessage::ImageFound`] (absolute path
/// string). After the scan finishes, [`ScanMessage::ScanComplete`] is sent.
/// Errors opening `dir` are silently swallowed (the sender simply never fires).
pub fn scan_directory(dir: PathBuf, sender: Sender<ScanMessage>) {
    std::thread::spawn(move || {
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            // Directory unreadable — still signal completion so the UI unblocks.
            let _ = sender.send_blocking(ScanMessage::ScanComplete);
            return;
        };

        // Open (or create) the per-folder database.
        let conn = match db::open(&dir) {
            Ok(c) => c,
            Err(_) => {
                let _ = sender.send_blocking(ScanMessage::ScanComplete);
                return;
            }
        };

        // Collect and sort entries for a stable, predictable display order.
        let mut entries: Vec<_> = read_dir.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if is_image(&path) {
                let path_str = path.to_string_lossy().into_owned();

                // Index the image: hash, extract metadata, generate thumbnail,
                // upsert into DB. `ensure_indexed` uses the DB cache when the
                // file hasn't changed.
                let maybe_row = db::ensure_indexed(&conn, &path);

                // Send the ImageFound message along with metadata if available.
                if let Some(row) = maybe_row {
                    if sender
                        .send_blocking(ScanMessage::ImageFound {
                            path: path_str,
                            hash: row.hash,
                            meta: row.meta,
                        })
                        .is_err()
                    {
                        return;
                    }
                } else {
                    // Indexing failed — still display the image, just without cache data.
                    if sender
                        .send_blocking(ScanMessage::ImageFound {
                            path: path_str,
                            hash: String::new(),
                            meta: Default::default(),
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            }
        }

        // Prune DB entries for files that no longer exist on disk.
        let _ = db::prune_missing(&conn);

        let _ = sender.send_blocking(ScanMessage::ScanComplete);
    });
}

fn is_image(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}
