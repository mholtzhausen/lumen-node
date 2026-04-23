use std::path::{Path, PathBuf};

pub struct AppConfig {
    pub last_folder: Option<PathBuf>,
    pub recent_folders: Vec<PathBuf>,
    pub window_width: Option<i32>,
    pub window_height: Option<i32>,
    pub window_maximized: Option<bool>,
    pub left_pane_pos: Option<i32>,
    pub right_pane_pos: Option<i32>,
    pub meta_pane_pos: Option<i32>,
    pub left_pane_width_pct: Option<f64>,
    pub right_pane_width_pct: Option<f64>,
    pub meta_pane_height_pct: Option<f64>,
    pub left_sidebar_visible: Option<bool>,
    pub right_sidebar_visible: Option<bool>,
    pub sort_key: Option<String>,
    pub search_text: Option<String>,
    pub thumbnail_size: Option<i32>,
}

/// Loads `~/.lumen-node/config.yml`.  Missing file → empty config.
pub fn load() -> AppConfig {
    let mut last_folder = None;
    let mut recent_folders = Vec::new();
    let mut window_width = None;
    let mut window_height = None;
    let mut window_maximized = None;
    let mut left_pane_pos = None;
    let mut right_pane_pos = None;
    let mut meta_pane_pos = None;
    let mut left_pane_width_pct = None;
    let mut right_pane_width_pct = None;
    let mut meta_pane_height_pct = None;
    let mut left_sidebar_visible = None;
    let mut right_sidebar_visible = None;
    let mut sort_key = None;
    let mut search_text = None;
    let mut thumbnail_size = None;
    if let Ok(content) = std::fs::read_to_string(config_path()) {
        for line in content.lines() {
            if let Some(val) = line.strip_prefix("last_folder: ") {
                let val = val.trim();
                if !val.is_empty() {
                    last_folder = Some(PathBuf::from(val));
                }
            } else if let Some(val) = line.strip_prefix("recent_folder: ") {
                let val = val.trim();
                if !val.is_empty() {
                    recent_folders.push(PathBuf::from(val));
                }
            } else if let Some(val) = line.strip_prefix("window_width: ") {
                window_width = val.trim().parse::<i32>().ok();
            } else if let Some(val) = line.strip_prefix("window_height: ") {
                window_height = val.trim().parse::<i32>().ok();
            } else if let Some(val) = line.strip_prefix("window_maximized: ") {
                window_maximized = val.trim().parse::<bool>().ok();
            } else if let Some(val) = line.strip_prefix("left_pane_pos: ") {
                left_pane_pos = val.trim().parse::<i32>().ok();
            } else if let Some(val) = line.strip_prefix("right_pane_pos: ") {
                right_pane_pos = val.trim().parse::<i32>().ok();
            } else if let Some(val) = line.strip_prefix("meta_pane_pos: ") {
                meta_pane_pos = val.trim().parse::<i32>().ok();
            } else if let Some(val) = line.strip_prefix("left_pane_width_pct: ") {
                left_pane_width_pct = val.trim().parse::<f64>().ok();
            } else if let Some(val) = line.strip_prefix("right_pane_width_pct: ") {
                right_pane_width_pct = val.trim().parse::<f64>().ok();
            } else if let Some(val) = line.strip_prefix("meta_pane_height_pct: ") {
                meta_pane_height_pct = val.trim().parse::<f64>().ok();
            } else if let Some(val) = line.strip_prefix("left_sidebar_visible: ") {
                left_sidebar_visible = val.trim().parse::<bool>().ok();
            } else if let Some(val) = line.strip_prefix("right_sidebar_visible: ") {
                right_sidebar_visible = val.trim().parse::<bool>().ok();
            } else if let Some(val) = line.strip_prefix("sort_key: ") {
                let val = val.trim();
                if !val.is_empty() {
                    sort_key = Some(val.to_string());
                }
            } else if let Some(val) = line.strip_prefix("search_text: ") {
                // Value may be empty (empty search = no prefix match, so use raw line remainder)
                search_text = Some(val.to_string());
            } else if let Some(val) = line.strip_prefix("thumbnail_size: ") {
                thumbnail_size = val.trim().parse::<i32>().ok();
            }
        }
    }
    AppConfig {
        last_folder,
        recent_folders,
        window_width,
        window_height,
        window_maximized,
        left_pane_pos,
        right_pane_pos,
        meta_pane_pos,
        left_pane_width_pct,
        right_pane_width_pct,
        meta_pane_height_pct,
        left_sidebar_visible,
        right_sidebar_visible,
        sort_key,
        search_text,
        thumbnail_size,
    }
}

/// Writes config to `~/.lumen-node/config.yml`, creating the directory if needed.
pub fn save(
    last_folder: Option<&Path>,
    recent_folders: &[PathBuf],
    window_width: i32,
    window_height: i32,
    window_maximized: bool,
    left_pane_pos: i32,
    right_pane_pos: i32,
    meta_pane_pos: i32,
    left_pane_width_pct: f64,
    right_pane_width_pct: f64,
    meta_pane_height_pct: f64,
    left_sidebar_visible: bool,
    right_sidebar_visible: bool,
) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let folder_str = last_folder
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let recent_folder_lines = recent_folders
        .iter()
        .map(|p| format!("recent_folder: {}", p.display()))
        .collect::<Vec<_>>()
        .join("\n");
    let content = format!(
        "last_folder: {}\n{}\nwindow_width: {}\nwindow_height: {}\nwindow_maximized: {}\nleft_pane_pos: {}\nright_pane_pos: {}\nmeta_pane_pos: {}\nleft_pane_width_pct: {:.6}\nright_pane_width_pct: {:.6}\nmeta_pane_height_pct: {:.6}\nleft_sidebar_visible: {}\nright_sidebar_visible: {}\n",
        folder_str,
        recent_folder_lines,
        window_width,
        window_height,
        window_maximized,
        left_pane_pos,
        right_pane_pos,
        meta_pane_pos,
        left_pane_width_pct,
        right_pane_width_pct,
        meta_pane_height_pct,
        left_sidebar_visible,
        right_sidebar_visible,
    );
    let _ = std::fs::write(&path, content);
}

/// Updates persisted folder history without changing other config keys.
pub fn save_recent_state(last_folder: Option<&Path>, recent_folders: &[PathBuf]) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let folder_str = last_folder
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let recent_folder_lines = recent_folders
        .iter()
        .map(|p| format!("recent_folder: {}", p.display()))
        .collect::<Vec<_>>()
        .join("\n");

    let mut lines: Vec<String> = Vec::new();
    for line in existing.lines() {
        if !line.starts_with("last_folder: ") && !line.starts_with("recent_folder: ") {
            lines.push(line.to_string());
        }
    }
    let suffix = lines.join("\n");
    let content = if suffix.is_empty() {
        format!("last_folder: {}\n{}\n", folder_str, recent_folder_lines)
    } else {
        format!(
            "last_folder: {}\n{}\n{}\n",
            folder_str, recent_folder_lines, suffix
        )
    };
    let _ = std::fs::write(&path, content);
}

fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| String::from("/tmp"));
    PathBuf::from(home).join(".lumen-node").join("config.yml")
}
