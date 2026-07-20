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

/// Parse `{index}` or `{index:N}` from a batch rename pattern. Returns pad width (explicit or None).
pub fn parse_batch_index_placeholder(pattern: &str) -> Result<Option<usize>, String> {
    let re = regex_lite_find_index(pattern)?;
    Ok(re)
}

fn regex_lite_find_index(pattern: &str) -> Result<Option<usize>, String> {
    if !pattern.contains("{index") {
        return Err("Pattern must include {index} or {index:N}".to_string());
    }
    let bytes = pattern.as_bytes();
    let mut i = 0;
    let mut found = None;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = pattern[i..].find('}') {
                let token = &pattern[i..i + end + 1];
                if token == "{index}" {
                    if found.is_some() {
                        return Err("Pattern may contain only one {index} placeholder".to_string());
                    }
                    found = Some(None);
                } else if let Some(rest) = token.strip_prefix("{index:") {
                    let num = rest.trim_end_matches('}');
                    let width: usize = num
                        .parse()
                        .map_err(|_| "Invalid {index:N} width".to_string())?;
                    if width == 0 || width > 12 {
                        return Err("{index:N} width must be 1–12".to_string());
                    }
                    if found.is_some() {
                        return Err("Pattern may contain only one {index} placeholder".to_string());
                    }
                    found = Some(Some(width));
                } else if token.starts_with("{index") {
                    return Err("Use {index} or {index:N}".to_string());
                }
                i += end + 1;
                continue;
            }
        }
        i += 1;
    }
    found.ok_or_else(|| "Pattern must include {index} or {index:N}".to_string())
}

pub fn default_index_pad_width(count: usize) -> usize {
    if count == 0 {
        return 1;
    }
    ((count as f64).log10().floor() as usize) + 1
}

/// Expand pattern for 1-based index; preserves nothing about extension (caller appends).
pub fn expand_batch_rename_stem(pattern: &str, index: usize, pad_width: usize) -> Result<String, String> {
    let explicit = parse_batch_index_placeholder(pattern)?;
    let width = explicit.unwrap_or(pad_width).max(1);
    let index_str = format!("{index:0width$}", width = width);
    let mut out = pattern.to_string();
    if let Some(start) = out.find("{index:") {
        if let Some(rel_end) = out[start..].find('}') {
            out.replace_range(start..start + rel_end + 1, &index_str);
        }
    } else if let Some(start) = out.find("{index}") {
        out.replace_range(start..start + "{index}".len(), &index_str);
    } else {
        return Err("Pattern must include {index} or {index:N}".to_string());
    }
    if let Some(reason) = invalid_filename_reason(&out) {
        return Err(reason.to_string());
    }
    Ok(out)
}

pub fn batch_rename_target(source_path: &Path, pattern: &str, index: usize, pad_width: usize) -> Result<PathBuf, String> {
    let stem = expand_batch_rename_stem(pattern, index, pad_width)?;
    let (_, ext) = split_filename(source_path);
    let Some(parent) = source_path.parent() else {
        return Err("Cannot determine parent folder".to_string());
    };
    let name = if let Some(ext) = ext {
        format!("{stem}.{ext}")
    } else {
        stem
    };
    Ok(parent.join(name))
}

/// Returns collision messages if any target collides with another target or an existing file
/// outside the rename set (sources may be overwritten by swap within set).
pub fn find_batch_rename_collisions(
    sources: &[PathBuf],
    targets: &[PathBuf],
) -> Vec<String> {
    let mut msgs = Vec::new();
    let source_set: std::collections::HashSet<_> = sources.iter().collect();
    let mut seen_targets: std::collections::HashMap<PathBuf, usize> = std::collections::HashMap::new();
    for (i, target) in targets.iter().enumerate() {
        if let Some(prev) = seen_targets.insert(target.clone(), i) {
            msgs.push(format!(
                "Items {} and {} map to the same name",
                prev + 1,
                i + 1
            ));
        }
        if target.exists() && !source_set.contains(target) {
            msgs.push(format!(
                "“{}” already exists",
                target.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
            ));
        }
        // No-op rename (same path) is fine.
    }
    msgs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_index_placeholder() {
        assert_eq!(parse_batch_index_placeholder("a_{index}").unwrap(), None);
        assert_eq!(parse_batch_index_placeholder("a_{index:3}").unwrap(), Some(3));
        assert!(parse_batch_index_placeholder("nope").is_err());
    }

    #[test]
    fn expand_pads_default_and_explicit() {
        assert_eq!(
            expand_batch_rename_stem("img_{index}", 7, 3).unwrap(),
            "img_007"
        );
        assert_eq!(
            expand_batch_rename_stem("img_{index:2}", 7, 5).unwrap(),
            "img_07"
        );
    }

    #[test]
    fn default_pad_width() {
        assert_eq!(default_index_pad_width(9), 1);
        assert_eq!(default_index_pad_width(10), 2);
        assert_eq!(default_index_pad_width(100), 3);
    }
}
