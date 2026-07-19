//! In-app keyboard shortcuts cheat sheet (`GtkShortcutsWindow`).
//! Keep in sync with README.md § Keyboard Shortcuts.

use gtk4::prelude::*;
use gtk4::{ShortcutType, ShortcutsGroup, ShortcutsSection, ShortcutsShortcut, ShortcutsWindow};
use libadwaita as adw;

fn key_shortcut(title: &str, accelerator: &str, subtitle: Option<&str>) -> ShortcutsShortcut {
    let mut builder = ShortcutsShortcut::builder().title(title).accelerator(accelerator);
    if let Some(subtitle) = subtitle {
        builder = builder.subtitle(subtitle);
    }
    builder.build()
}

fn action_shortcut(title: &str, action_name: &str, subtitle: Option<&str>) -> ShortcutsShortcut {
    let mut builder = ShortcutsShortcut::builder().title(title).action_name(action_name);
    if let Some(subtitle) = subtitle {
        builder = builder.subtitle(subtitle);
    }
    builder.build()
}

fn gesture_shortcut(title: &str, subtitle: &str) -> ShortcutsShortcut {
    ShortcutsShortcut::builder()
        .title(title)
        .subtitle(subtitle)
        .shortcut_type(ShortcutType::Gesture)
        .build()
}

fn add_key(group: &ShortcutsGroup, title: &str, accelerator: &str, subtitle: Option<&str>) {
    group.add_shortcut(&key_shortcut(title, accelerator, subtitle));
}

fn add_action(group: &ShortcutsGroup, title: &str, action_name: &str, subtitle: Option<&str>) {
    group.add_shortcut(&action_shortcut(title, action_name, subtitle));
}

pub(crate) fn present_shortcuts_window(parent: &adw::ApplicationWindow) {
    let window = ShortcutsWindow::builder()
        .title("Keyboard Shortcuts")
        .modal(true)
        .transient_for(parent)
        .build();

    // ── Navigation ───────────────────────────────────────────────────────────
    let navigation = ShortcutsSection::builder()
        .title("Navigation")
        .section_name("navigation")
        .build();

    let grid_nav = ShortcutsGroup::builder().title("Grid").build();
    add_key(&grid_nav, "Scroll one page up", "Page_Up", None);
    add_key(&grid_nav, "Scroll one page down", "Page_Down", None);
    add_key(&grid_nav, "Jump to first image", "Home", None);
    add_key(&grid_nav, "Jump to last image", "End", None);
    add_key(
        &grid_nav,
        "Quit",
        "Escape",
        Some("Toast warns; second press confirms"),
    );
    navigation.add_group(&grid_nav);

    let single_nav = ShortcutsGroup::builder().title("Single view").build();
    add_key(&single_nav, "Previous image", "Left", None);
    add_key(&single_nav, "Next image", "Right", None);
    add_key(&single_nav, "Return to grid", "Escape", None);
    single_nav.add_shortcut(&gesture_shortcut(
        "Toggle window fullscreen",
        "Double-click or middle-click",
    ));
    navigation.add_group(&single_nav);

    window.add_section(&navigation);

    // ── Clipboard ────────────────────────────────────────────────────────────
    let clipboard = ShortcutsSection::builder()
        .title("Clipboard")
        .section_name("clipboard")
        .build();
    let clipboard_group = ShortcutsGroup::builder().build();
    add_key(
        &clipboard_group,
        "Copy image pixels to clipboard",
        "<Primary>c",
        Some("Selection"),
    );
    add_key(
        &clipboard_group,
        "Mark image to move",
        "<Primary>x",
        Some("Selection; Ctrl+V into an open folder completes the move"),
    );
    add_key(
        &clipboard_group,
        "Paste clipboard image or complete cut-move",
        "<Primary>v",
        Some("Grid when a folder is open"),
    );
    clipboard.add_group(&clipboard_group);
    window.add_section(&clipboard);

    // ── Organise ─────────────────────────────────────────────────────────────
    let organise = ShortcutsSection::builder()
        .title("Organise")
        .section_name("organise")
        .build();
    let organise_group = ShortcutsGroup::builder().build();
    add_key(
        &organise_group,
        "Move selection to trash",
        "Delete",
        Some("Grid selection, not in a text field"),
    );
    add_key(
        &organise_group,
        "Permanent delete",
        "<Shift>Delete",
        Some("Grid; confirmation dialog"),
    );
    add_key(
        &organise_group,
        "Toggle favourite",
        "f",
        Some("Selection, not in a text field"),
    );
    add_key(
        &organise_group,
        "Rename selected image",
        "F2",
        Some("Selection, not in a text field"),
    );
    organise.add_group(&organise_group);
    window.add_section(&organise);

    // ── AI copy ──────────────────────────────────────────────────────────────
    let ai_copy = ShortcutsSection::builder()
        .title("AI copy")
        .section_name("ai-copy")
        .build();
    let ai_group = ShortcutsGroup::builder().build();
    add_action(
        &ai_group,
        "Copy prompt",
        "ctx.copy-prompt",
        Some("Selection"),
    );
    add_action(
        &ai_group,
        "Copy negative prompt",
        "ctx.copy-negative-prompt",
        Some("Selection"),
    );
    add_action(&ai_group, "Copy seed", "ctx.copy-seed", Some("Selection"));
    add_action(&ai_group, "Copy path", "ctx.copy-path", Some("Selection"));
    add_action(
        &ai_group,
        "Copy metadata",
        "ctx.copy-metadata",
        Some("Selection"),
    );
    add_action(
        &ai_group,
        "Copy generation command",
        "ctx.copy-generation-command",
        Some("Selection"),
    );
    ai_copy.add_group(&ai_group);
    window.add_section(&ai_copy);

    // ── Refresh ──────────────────────────────────────────────────────────────
    let refresh = ShortcutsSection::builder()
        .title("Refresh")
        .section_name("refresh")
        .build();
    let refresh_group = ShortcutsGroup::builder().build();
    add_action(
        &refresh_group,
        "Refresh folder thumbnails",
        "ctx.refresh-folder-thumbnails",
        Some("Folder open"),
    );
    add_action(
        &refresh_group,
        "Refresh folder metadata",
        "ctx.refresh-folder-metadata",
        Some("Folder open"),
    );
    refresh.add_group(&refresh_group);
    window.add_section(&refresh);

    window.present();
}
