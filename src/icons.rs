//! Bundled symbolic icons (hicolor) registration for GTK IconTheme.

use std::path::PathBuf;

/// Outline price-tag (image has no tags yet).
pub const TAG_ICON_NAME: &str = "lumen-tag-symbolic";
/// Filled price-tag (image has one or more tags).
pub const TAG_ICON_FILLED_NAME: &str = "lumen-tag-filled-symbolic";

/// Adds search paths so bundled tag icons resolve in dev, install, and AppImage.
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
