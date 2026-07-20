//! Tabbed preferences dialog (`adw::PreferencesDialog`) for `~/.lumen-node/config.yml`
//! and per-folder tag renaming.

use crate::config::{self, ColorSchemePref};
use crate::core::app_state::AppState;
use crate::db;
use crate::sort::{normalize_sort_key, sort_index_for_key, sort_key_for_index};
use crate::thumbnail_sizing::{normalize_thumbnail_size, thumbnail_size_options};
use crate::ui::grid::refresh_realized_grid_chrome_sizes;
use crate::ui::shell::{
    apply_color_scheme_pref, apply_thumbnail_chrome_scale, sync_theme_button,
};
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

pub(crate) struct PreferencesDeps {
    pub(crate) color_scheme: Rc<Cell<ColorSchemePref>>,
    pub(crate) theme_btn: gtk4::Button,
    pub(crate) thumbnail_chrome_scale: Rc<Cell<f64>>,
    pub(crate) thumbnail_chrome_css: gtk4::CssProvider,
    pub(crate) app_state: AppState,
}

pub(crate) fn present_preferences_window(parent: &adw::ApplicationWindow, deps: PreferencesDeps) {
    let cfg = config::load();

    let prefs = adw::PreferencesDialog::new();
    prefs.set_title("Preferences");
    prefs.set_search_enabled(false);
    prefs.set_content_width(760);
    prefs.set_content_height(520);
    prefs.set_presentation_mode(adw::DialogPresentationMode::Floating);

    prefs.add(&build_general_page(&cfg));
    prefs.add(&build_appearance_page(&cfg, &deps));
    prefs.add(&build_startup_page(&cfg));
    prefs.add(&build_tags_page(&prefs, &deps));

    prefs.present(Some(parent));
}

fn build_general_page(cfg: &config::AppConfig) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("General")
        .icon_name(crate::icons::SETTINGS)
        .name("general")
        .build();

    let group = adw::PreferencesGroup::builder()
        .title("Editor and full view")
        .description("Saved to ~/.lumen-node/config.yml. Favourite HUD changes apply on next launch.")
        .build();

    let editor_row = adw::EntryRow::builder()
        .title("External editor")
        .show_apply_button(true)
        .build();
    if let Some(path) = cfg.external_editor.as_ref() {
        editor_row.set_text(&path.display().to_string());
    }
    editor_row.connect_apply(|row| {
        let text = row.text().trim().to_string();
        if text.is_empty() {
            config::save_external_editor(None);
        } else {
            config::save_external_editor(Some(PathBuf::from(text).as_path()));
        }
    });
    group.add(&editor_row);

    let fav_switch = adw::SwitchRow::builder()
        .title("Favourite star in full view")
        .subtitle("Show a brief favourite indicator when entering single view")
        .active(cfg.full_view_favourite_icon.unwrap_or(true))
        .build();

    let seconds = cfg.full_view_favourite_icon_seconds.unwrap_or(2.0).max(0.0);
    let seconds_row = adw::SpinRow::with_range(0.0, 60.0, 0.5);
    seconds_row.set_title("Favourite star duration (seconds)");
    seconds_row.set_subtitle("How long the full-view favourite star stays visible");
    seconds_row.set_digits(1);
    seconds_row.set_value(seconds);

    let persist_fav = {
        let fav_switch = fav_switch.clone();
        let seconds_row = seconds_row.clone();
        Rc::new(move || {
            config::save_full_view_favourite_prefs(fav_switch.is_active(), seconds_row.value());
        })
    };

    {
        let persist_fav = persist_fav.clone();
        fav_switch.connect_active_notify(move |_| persist_fav());
    }
    {
        let persist_fav = persist_fav.clone();
        seconds_row.connect_value_notify(move |_| persist_fav());
    }

    group.add(&fav_switch);
    group.add(&seconds_row);
    page.add(&group);
    page
}

fn build_appearance_page(cfg: &config::AppConfig, deps: &PreferencesDeps) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("Appearance")
        .icon_name(crate::icons::APPEARANCE)
        .name("appearance")
        .build();

    let theme_group = adw::PreferencesGroup::builder()
        .title("Theme")
        .description("Applies immediately and syncs with the header theme toggle.")
        .build();

    let scheme_labels = gtk4::StringList::new(&["System", "Light", "Dark"]);
    let scheme_row = adw::ComboRow::builder()
        .title("Color scheme")
        .model(&scheme_labels)
        .build();

    let initial = cfg.color_scheme.unwrap_or(ColorSchemePref::System);
    let initial_idx = match initial {
        ColorSchemePref::System => 0,
        ColorSchemePref::Light => 1,
        ColorSchemePref::Dark => 2,
    };
    scheme_row.set_selected(initial_idx);

    let color_scheme = deps.color_scheme.clone();
    let theme_btn = deps.theme_btn.clone();
    scheme_row.connect_selected_notify(move |row| {
        let pref = match row.selected() {
            1 => ColorSchemePref::Light,
            2 => ColorSchemePref::Dark,
            _ => ColorSchemePref::System,
        };
        if color_scheme.get() == pref {
            return;
        }
        color_scheme.set(pref);
        apply_color_scheme_pref(pref);
        sync_theme_button(&theme_btn, pref);
        config::save_color_scheme(pref);
    });

    theme_group.add(&scheme_row);
    page.add(&theme_group);

    let chrome_group = adw::PreferencesGroup::builder()
        .title("Grid chrome")
        .description("Thumbnail favourite and tag button size. Applies immediately.")
        .build();

    let chrome_row = adw::ActionRow::builder()
        .title("Button size")
        .subtitle("Scale of the right-hand favourite/tag controls on thumbnails")
        .build();

    let initial_scale = deps.thumbnail_chrome_scale.get();
    let adjustment = gtk4::Adjustment::new(initial_scale, 0.4, 1.0, 0.05, 0.1, 0.0);
    let scale = gtk4::Scale::new(gtk4::Orientation::Horizontal, Some(&adjustment));
    scale.set_draw_value(true);
    scale.set_value_pos(gtk4::PositionType::Right);
    scale.set_digits(2);
    scale.set_hexpand(true);
    scale.set_width_request(180);
    scale.add_mark(0.6, gtk4::PositionType::Bottom, Some("60%"));
    scale.add_mark(1.0, gtk4::PositionType::Bottom, Some("100%"));
    scale.set_format_value_func(|_, value| format!("{:.0}%", value * 100.0));

    let chrome_scale_cell = deps.thumbnail_chrome_scale.clone();
    let chrome_css = deps.thumbnail_chrome_css.clone();
    let app_state = deps.app_state.clone();
    scale.connect_value_changed(move |s| {
        let value = config::normalize_thumbnail_chrome_scale(s.value());
        chrome_scale_cell.set(value);
        apply_thumbnail_chrome_scale(&chrome_css, value);
        refresh_realized_grid_chrome_sizes(&app_state);
        config::save_thumbnail_chrome_scale(value);
    });

    chrome_row.add_suffix(&scale);
    chrome_row.set_activatable_widget(Some(&scale));
    chrome_group.add(&chrome_row);
    page.add(&chrome_group);

    page
}

fn build_startup_page(cfg: &config::AppConfig) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("Startup")
        .icon_name(crate::icons::RECENT)
        .name("startup")
        .build();

    let group = adw::PreferencesGroup::builder()
        .title("Global defaults")
        .description(
            "Used when a folder has no saved ui_state yet. Per-folder sort, search, and thumbnail size still live in .lumen-node.db.",
        )
        .build();

    let sort_labels =
        gtk4::StringList::new(&["Name ↑", "Name ↓", "Date ↑", "Date ↓", "Size ↑", "Size ↓"]);
    let sort_row = adw::ComboRow::builder()
        .title("Default sort")
        .model(&sort_labels)
        .build();
    let sort_key = cfg
        .sort_key
        .as_deref()
        .map(normalize_sort_key)
        .unwrap_or("name_asc");
    sort_row.set_selected(sort_index_for_key(sort_key));

    let size_options = thumbnail_size_options();
    let size_labels = gtk4::StringList::new(&["1x", "1.3x", "1.6x", "1.9x"]);
    let size_row = adw::ComboRow::builder()
        .title("Default thumbnail size")
        .model(&size_labels)
        .build();
    let thumb = normalize_thumbnail_size(
        cfg.thumbnail_size
            .unwrap_or(crate::thumbnails::THUMB_NORMAL_SIZE),
    );
    let size_idx = size_options
        .iter()
        .position(|px| *px == thumb)
        .unwrap_or(0) as u32;
    size_row.set_selected(size_idx);

    let search_row = adw::EntryRow::builder()
        .title("Default search text")
        .show_apply_button(true)
        .build();
    if let Some(text) = cfg.search_text.as_ref() {
        search_row.set_text(text);
    }

    let persist_startup = {
        let sort_row = sort_row.clone();
        let size_row = size_row.clone();
        let search_row = search_row.clone();
        let size_options = size_options;
        Rc::new(move || {
            let key = sort_key_for_index(sort_row.selected());
            let idx = size_row.selected() as usize;
            let thumb = size_options
                .get(idx)
                .copied()
                .unwrap_or(crate::thumbnails::THUMB_NORMAL_SIZE);
            let search = search_row.text().to_string();
            config::save_startup_defaults(key, &search, thumb);
        })
    };

    {
        let persist_startup = persist_startup.clone();
        sort_row.connect_selected_notify(move |_| persist_startup());
    }
    {
        let persist_startup = persist_startup.clone();
        size_row.connect_selected_notify(move |_| persist_startup());
    }
    {
        let persist_startup = persist_startup.clone();
        search_row.connect_apply(move |_| persist_startup());
    }

    group.add(&sort_row);
    group.add(&size_row);
    group.add(&search_row);
    page.add(&group);
    page
}

fn build_tags_page(prefs: &adw::PreferencesDialog, deps: &PreferencesDeps) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("Tags")
        .icon_name(crate::icons::TAG_ICON_NAME)
        .name("tags")
        .build();

    let group = adw::PreferencesGroup::builder()
        .title("Rename tags")
        .description(
            "Edits tag names for the current folder. Renames update every image that uses the tag.",
        )
        .build();

    let folder = deps.app_state.current_folder.borrow().clone();
    let tags = folder
        .as_ref()
        .and_then(|folder| db::open(folder).ok())
        .and_then(|conn| db::list_all_tags_in_folder(&conn).ok())
        .unwrap_or_default();

    if folder.is_none() {
        let empty = adw::ActionRow::builder()
            .title("No folder open")
            .subtitle("Open a folder to rename its tags")
            .build();
        group.add(&empty);
        page.add(&group);
        return page;
    }

    if tags.is_empty() {
        let empty = adw::ActionRow::builder()
            .title("No tags yet")
            .subtitle("Add tags from the thumbnail chrome or context menu first")
            .build();
        group.add(&empty);
        page.add(&group);
        return page;
    }

    for tag in tags {
        let row = adw::EntryRow::builder()
            .title("Tag name")
            .show_apply_button(true)
            .build();
        row.set_text(&tag);

        let app_state = deps.app_state.clone();
        let prefs = prefs.clone();
        let current_name = Rc::new(RefCell::new(tag));
        row.connect_apply(move |row| {
            let old_tag = current_name.borrow().clone();
            let new_text = row.text().to_string();
            match apply_tag_rename(&app_state, &old_tag, &new_text) {
                Ok(RenameOutcome::Unchanged) => {
                    row.set_text(&old_tag);
                }
                Ok(RenameOutcome::Renamed) => {
                    if let Some(normalized) = db::normalize_tag(&new_text) {
                        *current_name.borrow_mut() = normalized.clone();
                        row.set_text(&normalized);
                    }
                }
                Err(message) => {
                    row.set_text(&old_tag);
                    prefs.add_toast(adw::Toast::new(&message));
                }
            }
        });
        group.add(&row);
    }

    page.add(&group);
    page
}

enum RenameOutcome {
    Unchanged,
    Renamed,
}

fn apply_tag_rename(
    app_state: &AppState,
    old_tag: &str,
    new_tag: &str,
) -> Result<RenameOutcome, String> {
    let Some(old_norm) = db::normalize_tag(old_tag) else {
        return Err("Tag name cannot be empty".to_string());
    };
    let Some(new_norm) = db::normalize_tag(new_tag) else {
        return Err("Tag name cannot be empty".to_string());
    };
    if old_norm == new_norm {
        return Ok(RenameOutcome::Unchanged);
    }

    let Some(folder) = app_state.current_folder.borrow().as_ref().cloned() else {
        return Err("No folder open".to_string());
    };
    let conn = db::open(&folder).map_err(|_| "Could not open folder database".to_string())?;
    let changed = db::rename_tag(&conn, &old_norm, &new_norm)
        .map_err(|_| "Could not rename tag".to_string())?;
    if changed == 0 {
        return Ok(RenameOutcome::Unchanged);
    }

    {
        let mut cache = app_state.tags_cache.borrow_mut();
        for tags in cache.values_mut() {
            let mut dirty = false;
            for tag in tags.iter_mut() {
                if tag == &old_norm {
                    *tag = new_norm.clone();
                    dirty = true;
                }
            }
            if dirty {
                tags.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
                tags.dedup();
            }
        }
    }

    {
        let mut filters = app_state.active_tag_filters.borrow_mut();
        if let Some(mode) = filters.remove(&old_norm) {
            filters.insert(new_norm.clone(), mode);
        }
        let _ = db::set_ui_state_value(
            folder.as_path(),
            "active_tags",
            &db::encode_active_tag_filters(&filters),
        );
    }

    if let Some(cb) = app_state.on_folder_tags_changed.borrow().as_ref() {
        cb();
    }

    Ok(RenameOutcome::Renamed)
}
