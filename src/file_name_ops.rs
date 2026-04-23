use gtk4::gio;
use gtk4::prelude::FileExt;
use std::path::{Path, PathBuf};

pub fn invalid_filename_reason(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("Name cannot be empty");
    }
    if name == "." || name == ".." {
        return Some("Name cannot be '.' or '..'");
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return Some("Name cannot end with a space or dot");
    }
    if name.chars().any(|c| c == '\0' || c.is_control()) {
        return Some("Name cannot contain control characters");
    }
    if name
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
    {
        return Some("Name contains illegal characters");
    }
    let upper = name.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return Some("Reserved filename");
    }
    if let Some(n) = upper.strip_prefix("COM") {
        if matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9") {
            return Some("Reserved filename");
        }
    }
    if let Some(n) = upper.strip_prefix("LPT") {
        if matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9") {
            return Some("Reserved filename");
        }
    }
    None
}

pub fn split_filename(path: &Path) -> (String, Option<String>) {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(ext) = path.extension().map(|e| e.to_string_lossy().into_owned()) {
        let suffix = format!(".{}", ext);
        if let Some(stem) = file_name.strip_suffix(&suffix) {
            return (stem.to_string(), Some(ext));
        }
    }
    (file_name, None)
}

pub fn clipboard_base_name_hint(raw_text: &str) -> Option<String> {
    for line in raw_text.lines() {
        let candidate = line.trim();
        if candidate.is_empty() || candidate.starts_with('#') {
            continue;
        }
        let name = if candidate.starts_with("file://") {
            let file = gio::File::for_uri(candidate);
            file.basename().map(|s| s.to_string_lossy().into_owned())
        } else {
            Path::new(candidate)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
        };
        if let Some(name) = name {
            let base = Path::new(&name)
                .file_stem()
                .map(|s| s.to_string_lossy().trim().to_string())
                .unwrap_or_default();
            if !base.is_empty() {
                return Some(base);
            }
        }
    }
    None
}

pub fn build_renamed_target(source_path: &Path, input_base_name: &str) -> Result<PathBuf, String> {
    let trimmed = input_base_name.trim();
    if let Some(reason) = invalid_filename_reason(trimmed) {
        return Err(reason.to_string());
    }
    let (current_base, ext) = split_filename(source_path);
    if trimmed == current_base {
        return Err("Enter a different name".to_string());
    }
    let Some(parent) = source_path.parent() else {
        return Err("Cannot determine parent folder".to_string());
    };
    let candidate_name = if let Some(ext) = ext {
        format!("{trimmed}.{ext}")
    } else {
        trimmed.to_string()
    };
    let target = parent.join(candidate_name);
    if target.exists() {
        return Err("A file with this name already exists".to_string());
    }
    Ok(target)
}
