use crate::byte_format::human_readable_bytes;
use gtk4::glib;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Default)]
pub struct SortFields {
    pub filename_lower: String,
    pub modified: Option<SystemTime>,
    pub size: u64,
}

pub fn compute_sort_fields(path_str: &str) -> SortFields {
    let path = std::path::Path::new(path_str);
    let filename_lower = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let (modified, size) = match std::fs::metadata(path) {
        Ok(meta) => (meta.modified().ok(), meta.len()),
        Err(_) => (None, 0),
    };

    SortFields {
        filename_lower,
        modified,
        size,
    }
}

pub fn format_sort_flag_date(modified: Option<SystemTime>) -> Option<String> {
    let modified = modified?;
    let secs = modified.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    let dt = glib::DateTime::from_unix_local(secs).ok()?;
    dt.format("%Y-%m-%d").ok().map(|s| s.to_string())
}

pub fn first_filename_character(filename_lower: &str) -> String {
    let ch = filename_lower
        .chars()
        .find(|c| c.is_alphanumeric())
        .unwrap_or('#');
    ch.to_uppercase().collect()
}

pub fn sort_flag_text_for_path(
    path: &str,
    sort_key: &str,
    sort_fields_cache: &HashMap<String, SortFields>,
) -> Option<String> {
    let fallback;
    let fields = if let Some(fields) = sort_fields_cache.get(path) {
        fields
    } else {
        fallback = compute_sort_fields(path);
        &fallback
    };

    if sort_key.starts_with("name_") {
        let source = if fields.filename_lower.is_empty() {
            std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            fields.filename_lower.clone()
        };
        return Some(first_filename_character(&source));
    }

    if sort_key.starts_with("date_") {
        return format_sort_flag_date(fields.modified);
    }

    if sort_key.starts_with("size_") {
        return Some(human_readable_bytes(fields.size));
    }

    None
}
