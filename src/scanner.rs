//! Directory scanning for LumenNode.
//!
//! [`scan_directory`] spawns a background thread that walks the immediate
//! contents of a folder and emits [`ScanMessage::ImageFound`] for every
//! recognised image file, followed by [`ScanMessage::ScanComplete`].
//!
//! The scan is intentionally *non-recursive*: for power users the containing
//! folder is the primary organisational unit. Recursive scanning can be added
//! behind a future option toggle.

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
                // If the receiver has been dropped (app closed mid-scan), stop.
                if sender.send_blocking(ScanMessage::ImageFound(path_str)).is_err() {
                    return;
                }
            }
        }

        let _ = sender.send_blocking(ScanMessage::ScanComplete);
    });
}

fn is_image(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}
