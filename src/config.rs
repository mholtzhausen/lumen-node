use std::path::{Path, PathBuf};

/// Persisted appearance preference (`color_scheme` in config.yml).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorSchemePref {
    System,
    Light,
    Dark,
}

impl ColorSchemePref {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim() {
            "system" => Some(Self::System),
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::System => Self::Light,
            Self::Light => Self::Dark,
            Self::Dark => Self::System,
        }
    }

    pub fn icon_name(self) -> &'static str {
        match self {
            Self::System => "display-brightness-symbolic",
            Self::Light => "weather-clear-symbolic",
            Self::Dark => "weather-clear-night-symbolic",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Self::System => "Theme: System",
            Self::Light => "Theme: Light",
            Self::Dark => "Theme: Dark",
        }
    }
}

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
    /// Whether the Metadata expander in the right pane is expanded. Default when unset: true.
    pub meta_section_expanded: Option<bool>,
    pub sort_key: Option<String>,
    pub search_text: Option<String>,
    pub thumbnail_size: Option<i32>,
    /// Optional executable used by "Open in External Editor" in the context menu.
    /// When unset, the default application for the image MIME type is used.
    pub external_editor: Option<PathBuf>,
    /// Appearance: `system` | `light` | `dark`. Default when unset: system.
    pub color_scheme: Option<ColorSchemePref>,
    /// Show the favourite star HUD in full/single view. Default when unset: true.
    /// Not rewritten by session `save`; updated via Preferences / `save_full_view_favourite_prefs`.
    pub full_view_favourite_icon: Option<bool>,
    /// Seconds the full-view favourite star stays visible before fading. Default: 2.
    /// Not rewritten by session `save`; updated via Preferences / `save_full_view_favourite_prefs`.
    pub full_view_favourite_icon_seconds: Option<f64>,
    /// Scale for grid thumbnail chrome buttons (0.4–1.0). Default when unset: 0.6.
    /// Not rewritten by session `save`; updated via Preferences / `save_thumbnail_chrome_scale`.
    pub thumbnail_chrome_scale: Option<f64>,
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
    let mut meta_section_expanded = None;
    let mut sort_key = None;
    let mut search_text = None;
    let mut thumbnail_size = None;
    let mut external_editor = None;
    let mut color_scheme = None;
    let mut full_view_favourite_icon = None;
    let mut full_view_favourite_icon_seconds = None;
    let mut thumbnail_chrome_scale = None;
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
            } else if let Some(val) = line.strip_prefix("meta_section_expanded: ") {
                meta_section_expanded = val.trim().parse::<bool>().ok();
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
            } else if let Some(val) = line.strip_prefix("external_editor: ") {
                let val = val.trim();
                if !val.is_empty() {
                    external_editor = Some(PathBuf::from(val));
                }
            } else if let Some(val) = line.strip_prefix("color_scheme: ") {
                color_scheme = ColorSchemePref::parse(val);
            } else if let Some(val) = line.strip_prefix("full_view_favourite_icon: ") {
                full_view_favourite_icon = val.trim().parse::<bool>().ok();
            } else if let Some(val) = line.strip_prefix("full_view_favourite_icon_seconds: ") {
                full_view_favourite_icon_seconds = val.trim().parse::<f64>().ok().filter(|v| *v >= 0.0);
            } else if let Some(val) = line.strip_prefix("thumbnail_chrome_scale: ") {
                thumbnail_chrome_scale = val
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .map(normalize_thumbnail_chrome_scale);
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
        meta_section_expanded,
        sort_key,
        search_text,
        thumbnail_size,
        external_editor,
        color_scheme,
        full_view_favourite_icon,
        full_view_favourite_icon_seconds,
        thumbnail_chrome_scale,
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
    meta_section_expanded: bool,
    color_scheme: ColorSchemePref,
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
        "last_folder: {}\n{}\nwindow_width: {}\nwindow_height: {}\nwindow_maximized: {}\nleft_pane_pos: {}\nright_pane_pos: {}\nmeta_pane_pos: {}\nleft_pane_width_pct: {:.6}\nright_pane_width_pct: {:.6}\nmeta_pane_height_pct: {:.6}\nleft_sidebar_visible: {}\nright_sidebar_visible: {}\nmeta_section_expanded: {}\ncolor_scheme: {}\n",
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
        meta_section_expanded,
        color_scheme.as_str(),
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

/// Updates only the `color_scheme` key, preserving other config lines.
pub fn save_color_scheme(color_scheme: ColorSchemePref) {
    update_config_keys(&[("color_scheme", Some(color_scheme.as_str().to_string()))]);
}

/// Updates `external_editor`, preserving other keys. Pass `None` to remove the key.
pub fn save_external_editor(editor: Option<&Path>) {
    let value = editor
        .map(|p| p.display().to_string())
        .filter(|s| !s.trim().is_empty());
    update_config_keys(&[("external_editor", value)]);
}

/// Updates full-view favourite HUD keys, preserving other config lines.
pub fn save_full_view_favourite_prefs(show_icon: bool, seconds: f64) {
    let seconds = if seconds.is_finite() && seconds >= 0.0 {
        seconds
    } else {
        2.0
    };
    update_config_keys(&[
        ("full_view_favourite_icon", Some(show_icon.to_string())),
        (
            "full_view_favourite_icon_seconds",
            Some(format_config_f64(seconds)),
        ),
    ]);
}

/// Default grid chrome button scale (60% of the 28px base size).
pub const DEFAULT_THUMBNAIL_CHROME_SCALE: f64 = 0.6;

/// Clamps chrome scale to the supported Preferences slider range.
pub fn normalize_thumbnail_chrome_scale(scale: f64) -> f64 {
    if !scale.is_finite() {
        return DEFAULT_THUMBNAIL_CHROME_SCALE;
    }
    scale.clamp(0.4, 1.0)
}

/// Updates `thumbnail_chrome_scale`, preserving other config lines.
pub fn save_thumbnail_chrome_scale(scale: f64) {
    let scale = normalize_thumbnail_chrome_scale(scale);
    update_config_keys(&[(
        "thumbnail_chrome_scale",
        Some(format_config_f64(scale)),
    )]);
}

/// Updates global startup defaults (`sort_key`, `search_text`, `thumbnail_size`).
/// Does not affect per-folder SQLite `ui_state`.
pub fn save_startup_defaults(sort_key: &str, search_text: &str, thumbnail_size: i32) {
    update_config_keys(&[
        ("sort_key", Some(sort_key.to_string())),
        ("search_text", Some(search_text.to_string())),
        ("thumbnail_size", Some(thumbnail_size.to_string())),
    ]);
}

/// Upsert or remove `key: value` lines without clobbering unknown keys.
/// `None` value removes every matching key line.
fn update_config_keys(updates: &[(&str, Option<String>)]) {
    if updates.is_empty() {
        return;
    }
    let path = config_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let prefixes: Vec<String> = updates.iter().map(|(k, _)| format!("{k}: ")).collect();
    let mut lines: Vec<String> = Vec::new();
    let mut seen = vec![false; updates.len()];

    for line in existing.lines() {
        let mut matched = false;
        for (i, prefix) in prefixes.iter().enumerate() {
            if line.starts_with(prefix.as_str()) {
                matched = true;
                if !seen[i] {
                    seen[i] = true;
                    if let Some(ref val) = updates[i].1 {
                        lines.push(format!("{}: {}", updates[i].0, val));
                    }
                }
                // Duplicate key lines are dropped once replaced/removed.
                break;
            }
        }
        if !matched {
            lines.push(line.to_string());
        }
    }

    for (i, (key, val)) in updates.iter().enumerate() {
        if !seen[i] {
            if let Some(ref v) = val {
                lines.push(format!("{key}: {v}"));
            }
        }
    }

    let mut content = lines.join("\n");
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    let _ = std::fs::write(&path, content);
}

fn format_config_f64(value: f64) -> String {
    if (value - value.round()).abs() < f64::EPSILON {
        format!("{}", value.round() as i64)
    } else {
        format!("{value}")
    }
}

fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| String::from("/tmp"));
    PathBuf::from(home).join(".lumen-node").join("config.yml")
}
