use std::path::{Path, PathBuf};

pub struct AppConfig {
    pub last_folder: Option<PathBuf>,
    pub left_pane_pos: Option<i32>,
    pub right_pane_pos: Option<i32>,
    pub meta_pane_pos: Option<i32>,
}

/// Loads `~/.lumen-node/config.yml`.  Missing file → empty config.
pub fn load() -> AppConfig {
    let mut last_folder = None;
    let mut left_pane_pos = None;
    let mut right_pane_pos = None;
    let mut meta_pane_pos = None;
    if let Ok(content) = std::fs::read_to_string(config_path()) {
        for line in content.lines() {
            if let Some(val) = line.strip_prefix("last_folder: ") {
                let val = val.trim();
                if !val.is_empty() {
                    last_folder = Some(PathBuf::from(val));
                }
            } else if let Some(val) = line.strip_prefix("left_pane_pos: ") {
                left_pane_pos = val.trim().parse::<i32>().ok();
            } else if let Some(val) = line.strip_prefix("right_pane_pos: ") {
                right_pane_pos = val.trim().parse::<i32>().ok();
            } else if let Some(val) = line.strip_prefix("meta_pane_pos: ") {
                meta_pane_pos = val.trim().parse::<i32>().ok();
            }
        }
    }
    AppConfig { last_folder, left_pane_pos, right_pane_pos, meta_pane_pos }
}

/// Writes config to `~/.lumen-node/config.yml`, creating the directory if needed.
pub fn save(last_folder: Option<&Path>, left_pane_pos: i32, right_pane_pos: i32, meta_pane_pos: i32) {
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let folder_str = last_folder
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let content = format!(
        "last_folder: {}\nleft_pane_pos: {}\nright_pane_pos: {}\nmeta_pane_pos: {}\n",
        folder_str, left_pane_pos, right_pane_pos, meta_pane_pos,
    );
    let _ = std::fs::write(&path, content);
}

fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| String::from("/tmp"));
    PathBuf::from(home).join(".lumen-node").join("config.yml")
}
