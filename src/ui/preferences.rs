//! Tabbed preferences dialog (`adw::PreferencesWindow`) for `~/.lumen-node/config.yml`.

use crate::config::{self, ColorSchemePref};
use crate::sort::{normalize_sort_key, sort_index_for_key, sort_key_for_index};
use crate::thumbnail_sizing::{normalize_thumbnail_size, thumbnail_size_options};
use crate::ui::shell::{apply_color_scheme_pref, sync_theme_button};
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

pub(crate) struct PreferencesDeps {
    pub(crate) color_scheme: Rc<Cell<ColorSchemePref>>,
    pub(crate) theme_btn: gtk4::Button,
}

pub(crate) fn present_preferences_window(parent: &adw::ApplicationWindow, deps: PreferencesDeps) {
    let cfg = config::load();

    let prefs = adw::PreferencesWindow::new();
    prefs.set_transient_for(Some(parent));
    prefs.set_modal(true);
    prefs.set_title(Some("Preferences"));
    prefs.set_search_enabled(false);
    prefs.set_default_width(560);
    prefs.set_default_height(480);

    prefs.add(&build_general_page(&cfg));
    prefs.add(&build_appearance_page(&cfg, &deps));
    prefs.add(&build_startup_page(&cfg));

    prefs.present();
}

fn build_general_page(cfg: &config::AppConfig) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("General")
        .icon_name("preferences-system-symbolic")
        .name("general")
        .build();

    let group = adw::PreferencesGroup::builder()
        .title("Editor & full view")
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
        .icon_name("preferences-desktop-appearance-symbolic")
        .name("appearance")
        .build();

    let group = adw::PreferencesGroup::builder()
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

    group.add(&scheme_row);
    page.add(&group);
    page
}

fn build_startup_page(cfg: &config::AppConfig) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("Startup")
        .icon_name("document-open-recent-symbolic")
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
