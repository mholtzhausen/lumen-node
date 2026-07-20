//! Bundled symbolic icons (hicolor) registration for GTK IconTheme.
//!
//! All UI chrome icons are vendored under `data/icons/hicolor/scalable/actions/`
//! so they resolve without a host Adwaita/Yaru icon theme.

use std::path::PathBuf;

/// Outline price-tag (image has no tags yet).
pub const TAG_ICON_NAME: &str = "lumen-tag-symbolic";
/// Filled price-tag (image has one or more tags).
pub const TAG_ICON_FILLED_NAME: &str = "lumen-tag-filled-symbolic";

pub const STARRED: &str = "lumen-starred-symbolic";
pub const NON_STARRED: &str = "lumen-non-starred-symbolic";
pub const SEMI_STARRED: &str = "lumen-semi-starred-symbolic";
pub const COPY: &str = "lumen-copy-symbolic";
pub const CLEAR: &str = "lumen-clear-symbolic";
pub const DELETE: &str = "lumen-delete-symbolic";
pub const FOLDER_OPEN: &str = "lumen-folder-open-symbolic";
pub const FOLDER: &str = "lumen-folder-symbolic";
pub const RECENT: &str = "lumen-recent-symbolic";
pub const SIDEBAR_LEFT: &str = "lumen-sidebar-left-symbolic";
pub const SIDEBAR_RIGHT: &str = "lumen-sidebar-right-symbolic";
pub const BRIGHTNESS: &str = "lumen-brightness-symbolic";
pub const SUN: &str = "lumen-sun-symbolic";
pub const MOON: &str = "lumen-moon-symbolic";
pub const TEXT: &str = "lumen-text-symbolic";
pub const ADD: &str = "lumen-add-symbolic";
pub const IMAGE: &str = "lumen-image-symbolic";
pub const SELECT: &str = "lumen-select-symbolic";
pub const CLOSE: &str = "lumen-close-symbolic";
pub const CHECKBOX: &str = "lumen-checkbox-symbolic";
pub const SEARCH: &str = "lumen-search-symbolic";
pub const GRID: &str = "lumen-grid-symbolic";
pub const FULLSCREEN: &str = "lumen-fullscreen-symbolic";
pub const COMPARE: &str = "lumen-compare-symbolic";
pub const EDIT: &str = "lumen-edit-symbolic";
pub const TRASH: &str = "lumen-trash-symbolic";
pub const MISSING: &str = "lumen-missing-symbolic";
pub const SETTINGS: &str = "lumen-settings-symbolic";
pub const APPEARANCE: &str = "lumen-appearance-symbolic";

/// Adds search paths so bundled icons resolve in dev, install, and AppImage.
pub fn register_bundled_icons() {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let theme = gtk4::IconTheme::for_display(&display);
    for path in icon_search_candidates() {
        if path.is_dir() {
            theme.add_search_path(&path);
        }
    }
}

fn icon_search_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        out.push(PathBuf::from(manifest).join("data/icons"));
    }
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        out.push(PathBuf::from(xdg).join("icons"));
    } else if let Ok(home) = std::env::var("HOME") {
        out.push(PathBuf::from(home).join(".local/share/icons"));
    }
    if let Ok(appdir) = std::env::var("APPDIR") {
        out.push(PathBuf::from(appdir).join("usr/share/icons"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("../share/icons"));
            out.push(dir.join("data/icons"));
        }
    }
    out
}
