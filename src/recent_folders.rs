use std::path::{Path, PathBuf};

pub fn push_recent_folder_entry(history: &mut Vec<PathBuf>, folder: &Path, limit: usize) {
    history.retain(|p| p != folder);
    history.insert(0, folder.to_path_buf());
    if history.len() > limit {
        history.truncate(limit);
    }
}
