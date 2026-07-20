use crate::core::app_state::AppState;
use crate::db;
use crate::sort::sort_key_for_index;
use crate::ui::grid::apply_thumbnail_size_change;
use gtk4::prelude::*;
use gtk4::gio::ListStore;
use gtk4::{CustomFilter, CustomSorter, SingleSelection};
use libadwaita as adw;
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    path::PathBuf,
    rc::Rc,
};

/// Snapshot active tag filter as a sorted Vec for persistence / UiState.
pub(crate) fn active_tags_vec(active_tags: &Rc<RefCell<HashSet<String>>>) -> Vec<String> {
    let mut tags: Vec<String> = active_tags.borrow().iter().cloned().collect();
    tags.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    tags
}

pub(crate) fn sync_tags_filter_button_style(
    tags_filter_btn: &gtk4::MenuButton,
    active_tags: &Rc<RefCell<HashSet<String>>>,
) {
    if active_tags.borrow().is_empty() {
        tags_filter_btn.remove_css_class("tags-filter-active");
    } else {
        tags_filter_btn.add_css_class("tags-filter-active");
    }
}

pub(crate) fn set_similar_filter_chrome(similar_filter_btn: &gtk4::Button, active: bool) {
    if active {
        similar_filter_btn.add_css_class("similar-filter-active");
        similar_filter_btn.set_sensitive(true);
    } else {
        similar_filter_btn.remove_css_class("similar-filter-active");
        similar_filter_btn.set_sensitive(false);
    }
}

pub(crate) fn clear_similar_filter(
    similar_paths: &Rc<RefCell<Option<HashSet<String>>>>,
    filter: &CustomFilter,
    similar_filter_btn: &gtk4::Button,
) {
    *similar_paths.borrow_mut() = None;
    set_similar_filter_chrome(similar_filter_btn, false);
    filter.changed(gtk4::FilterChange::Different);
}

/// Rebuilds the tag-filter popover checkboxes from folder tags + active set.
/// Checkbox toggles update `active_tags` + button style and mark dirty; grid/DB
/// apply is deferred to [`install_tags_filter_popover_handler`] on popover close.
pub(crate) fn rebuild_tag_filter_list(
    tags_filter_list: &gtk4::Box,
    tags_filter_btn: &gtk4::MenuButton,
    active_tags: &Rc<RefCell<HashSet<String>>>,
    tags_filter_dirty: &Rc<Cell<bool>>,
    filter: &CustomFilter,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    known_tags: &[String],
) {
    while let Some(child) = tags_filter_list.first_child() {
        tags_filter_list.remove(&child);
    }

    let heading = gtk4::Label::new(Some("Filter by tags (AND)"));
    heading.add_css_class("caption-heading");
    heading.set_halign(gtk4::Align::Start);
    tags_filter_list.append(&heading);

    if known_tags.is_empty() {
        let empty = gtk4::Label::new(Some("No tags in this folder yet."));
        empty.add_css_class("caption");
        empty.set_halign(gtk4::Align::Start);
        tags_filter_list.append(&empty);
        sync_tags_filter_button_style(tags_filter_btn, active_tags);
        return;
    }

    let active_snapshot = active_tags.borrow().clone();
    for tag in known_tags {
        let check = gtk4::CheckButton::with_label(tag);
        check.set_active(active_snapshot.contains(tag));
        let tag_owned = tag.clone();
        let active_tags_cb = active_tags.clone();
        let dirty_cb = tags_filter_dirty.clone();
        let btn_cb = tags_filter_btn.clone();
        check.connect_toggled(move |btn| {
            {
                let mut active = active_tags_cb.borrow_mut();
                if btn.is_active() {
                    active.insert(tag_owned.clone());
                } else {
                    active.remove(&tag_owned);
                }
            }
            sync_tags_filter_button_style(&btn_cb, &active_tags_cb);
            dirty_cb.set(true);
        });
        tags_filter_list.append(&check);
    }

    if !active_snapshot.is_empty() {
        let clear = gtk4::Button::with_label("Clear tag filter");
        clear.add_css_class("flat");
        let active_tags_clear = active_tags.clone();
        let dirty_clear = tags_filter_dirty.clone();
        let filter_clear = filter.clone();
        let folder_clear = current_folder.clone();
        let btn_clear = tags_filter_btn.clone();
        let list_clear = tags_filter_list.clone();
        let known_clear: Vec<String> = known_tags.to_vec();
        clear.connect_clicked(move |_| {
            active_tags_clear.borrow_mut().clear();
            if let Some(folder) = folder_clear.borrow().as_ref() {
                let _ = db::set_ui_state_value(folder.as_path(), "active_tags", "[]");
            }
            dirty_clear.set(false);
            rebuild_tag_filter_list(
                &list_clear,
                &btn_clear,
                &active_tags_clear,
                &dirty_clear,
                &filter_clear,
                &folder_clear,
                &known_clear,
            );
            filter_clear.changed(gtk4::FilterChange::Different);
        });
        tags_filter_list.append(&clear);
    }

    sync_tags_filter_button_style(tags_filter_btn, active_tags);
}

/// Refresh tag filter UI from the current folder DB (known tags).
pub(crate) fn refresh_tag_filter_from_folder(
    tags_filter_list: &gtk4::Box,
    tags_filter_btn: &gtk4::MenuButton,
    active_tags: &Rc<RefCell<HashSet<String>>>,
    tags_filter_dirty: &Rc<Cell<bool>>,
    filter: &CustomFilter,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
) {
    let known = current_folder
        .borrow()
        .as_ref()
        .and_then(|folder| db::open(folder).ok())
        .and_then(|conn| db::list_all_tags_in_folder(&conn).ok())
        .unwrap_or_default();
    rebuild_tag_filter_list(
        tags_filter_list,
        tags_filter_btn,
        active_tags,
        tags_filter_dirty,
        filter,
        current_folder,
        &known,
    );
}

/// Once-only: on tags popover close, persist + refilter if checkboxes marked dirty.
pub(crate) fn install_tags_filter_popover_handler(
    tags_filter_btn: &gtk4::MenuButton,
    active_tags: &Rc<RefCell<HashSet<String>>>,
    tags_filter_dirty: &Rc<Cell<bool>>,
    filter: &CustomFilter,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
) {
    let Some(popover) = tags_filter_btn.popover() else {
        return;
    };
    let active_tags = active_tags.clone();
    let dirty = tags_filter_dirty.clone();
    let filter = filter.clone();
    let current_folder = current_folder.clone();
    popover.connect_closed(move |_| {
        if !dirty.get() {
            return;
        }
        if let Some(folder) = current_folder.borrow().as_ref() {
            let _ = db::set_ui_state_value(
                folder.as_path(),
                "active_tags",
                &db::encode_active_tags(&active_tags_vec(&active_tags)),
            );
        }
        filter.changed(gtk4::FilterChange::Different);
        dirty.set(false);
    });
}

pub(crate) fn install_sort_dropdown_handler(
    sort_dropdown: &gtk4::DropDown,
    sort_key: &Rc<RefCell<String>>,
    sorter: &CustomSorter,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    scan_in_progress: &Rc<Cell<bool>>,
    start_scan_for_folder: &Rc<dyn Fn(PathBuf)>,
) {
    let sort_key_dd = sort_key.clone();
    let sorter_dd = sorter.clone();
    let current_folder_dd = current_folder.clone();
    let scan_in_progress_dd = scan_in_progress.clone();
    let start_scan_dd = start_scan_for_folder.clone();
    sort_dropdown.connect_selected_notify(move |dd| {
        let key = sort_key_for_index(dd.selected());
        let new_key = key.to_string();
        if *sort_key_dd.borrow() == new_key {
            return;
        }
        *sort_key_dd.borrow_mut() = new_key;
        if let Some(folder) = current_folder_dd.borrow().as_ref() {
            let _ = db::set_ui_state_value(folder.as_path(), "sort_key", &sort_key_dd.borrow());
        }

        if scan_in_progress_dd.get() {
            if let Some(folder) = current_folder_dd.borrow().as_ref().cloned() {
                start_scan_dd(folder);
                return;
            }
        }

        sorter_dd.changed(gtk4::SorterChange::Different);
    });
}

pub(crate) fn install_search_entry_handler(
    search_entry: &gtk4::SearchEntry,
    search_text: &Rc<RefCell<String>>,
    filter: &CustomFilter,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
) {
    let search_text_entry = search_text.clone();
    let filter_entry = filter.clone();
    let current_folder_search = current_folder.clone();
    search_entry.connect_search_changed(move |entry| {
        *search_text_entry.borrow_mut() = entry.text().to_lowercase();
        if let Some(folder) = current_folder_search.borrow().as_ref() {
            let _ = db::set_ui_state_value(
                folder.as_path(),
                "search_text",
                &search_text_entry.borrow(),
            );
        }
        filter_entry.changed(gtk4::FilterChange::Different);
    });
}

pub(crate) fn apply_clear_filters(
    search_text: &Rc<RefCell<String>>,
    favorites_only: &Rc<Cell<bool>>,
    active_tags: &Rc<RefCell<HashSet<String>>>,
    tags_filter_dirty: &Rc<Cell<bool>>,
    similar_paths: &Rc<RefCell<Option<HashSet<String>>>>,
    sort_key: &Rc<RefCell<String>>,
    filter: &CustomFilter,
    _sorter: &CustomSorter,
    favourites_filter_btn: &gtk4::ToggleButton,
    tags_filter_btn: &gtk4::MenuButton,
    tags_filter_list: &gtk4::Box,
    search_entry: &gtk4::SearchEntry,
    _sort_dropdown: &gtk4::DropDown,
    thumbnail_size: &Rc<RefCell<i32>>,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    similar_filter_btn: &gtk4::Button,
) {
    *search_text.borrow_mut() = String::new();
    favorites_only.set(false);
    active_tags.borrow_mut().clear();
    tags_filter_dirty.set(false);
    *similar_paths.borrow_mut() = None;
    set_similar_filter_chrome(similar_filter_btn, false);
    favourites_filter_btn.remove_css_class("favorites-filter-active");
    favourites_filter_btn.set_active(false);
    search_entry.set_text("");
    if let Some(folder) = current_folder.borrow().as_ref() {
        let _ = db::save_ui_state(
            folder.as_path(),
            &db::UiState {
                sort_key: sort_key.borrow().clone(),
                search_text: search_text.borrow().clone(),
                favorites_only: favorites_only.get(),
                active_tags: Vec::new(),
                thumbnail_size: *thumbnail_size.borrow(),
            },
        );
    }
    refresh_tag_filter_from_folder(
        tags_filter_list,
        tags_filter_btn,
        active_tags,
        tags_filter_dirty,
        filter,
        current_folder,
    );
    filter.changed(gtk4::FilterChange::LessStrict);
}

pub(crate) fn deactivate_tag_filter(
    active_tags: &Rc<RefCell<HashSet<String>>>,
    tags_filter_dirty: &Rc<Cell<bool>>,
    filter: &CustomFilter,
    tags_filter_btn: &gtk4::MenuButton,
    tags_filter_list: &gtk4::Box,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
) {
    active_tags.borrow_mut().clear();
    tags_filter_dirty.set(false);
    if let Some(folder) = current_folder.borrow().as_ref() {
        let _ = db::set_ui_state_value(folder.as_path(), "active_tags", "[]");
    }
    refresh_tag_filter_from_folder(
        tags_filter_list,
        tags_filter_btn,
        active_tags,
        tags_filter_dirty,
        filter,
        current_folder,
    );
    filter.changed(gtk4::FilterChange::Different);
}

pub(crate) fn deactivate_favorites_filter(
    favorites_only: &Rc<Cell<bool>>,
    filter: &CustomFilter,
    favourites_filter_btn: &gtk4::ToggleButton,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
) {
    favorites_only.set(false);
    favourites_filter_btn.remove_css_class("favorites-filter-active");
    favourites_filter_btn.set_active(false);
    if let Some(folder) = current_folder.borrow().as_ref() {
        let _ = db::set_ui_state_value(folder.as_path(), "favorites_only", "0");
    }
    filter.changed(gtk4::FilterChange::Different);
}

pub(crate) fn install_clear_button_handler(
    clear_btn: &gtk4::Button,
    search_text: &Rc<RefCell<String>>,
    favorites_only: &Rc<Cell<bool>>,
    active_tags: &Rc<RefCell<HashSet<String>>>,
    tags_filter_dirty: &Rc<Cell<bool>>,
    similar_paths: &Rc<RefCell<Option<HashSet<String>>>>,
    sort_key: &Rc<RefCell<String>>,
    filter: &CustomFilter,
    sorter: &CustomSorter,
    favourites_filter_btn: &gtk4::ToggleButton,
    tags_filter_btn: &gtk4::MenuButton,
    tags_filter_list: &gtk4::Box,
    search_entry: &gtk4::SearchEntry,
    sort_dropdown: &gtk4::DropDown,
    thumbnail_size: &Rc<RefCell<i32>>,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    similar_filter_btn: &gtk4::Button,
) {
    let search_text_clear = search_text.clone();
    let favorites_only_clear = favorites_only.clone();
    let active_tags_clear = active_tags.clone();
    let tags_filter_dirty_clear = tags_filter_dirty.clone();
    let similar_paths_clear = similar_paths.clone();
    let sort_key_clear = sort_key.clone();
    let filter_clear = filter.clone();
    let sorter_clear = sorter.clone();
    let favourites_filter_btn_clear = favourites_filter_btn.clone();
    let tags_filter_btn_clear = tags_filter_btn.clone();
    let tags_filter_list_clear = tags_filter_list.clone();
    let search_entry_clear = search_entry.clone();
    let sort_dropdown_clear = sort_dropdown.clone();
    let thumbnail_size_clear = thumbnail_size.clone();
    let current_folder_clear = current_folder.clone();
    let similar_filter_btn_clear = similar_filter_btn.clone();
    clear_btn.connect_clicked(move |_| {
        apply_clear_filters(
            &search_text_clear,
            &favorites_only_clear,
            &active_tags_clear,
            &tags_filter_dirty_clear,
            &similar_paths_clear,
            &sort_key_clear,
            &filter_clear,
            &sorter_clear,
            &favourites_filter_btn_clear,
            &tags_filter_btn_clear,
            &tags_filter_list_clear,
            &search_entry_clear,
            &sort_dropdown_clear,
            &thumbnail_size_clear,
            &current_folder_clear,
            &similar_filter_btn_clear,
        );
    });
}

pub(crate) fn install_similar_filter_button_handler(
    similar_filter_btn: &gtk4::Button,
    similar_paths: &Rc<RefCell<Option<HashSet<String>>>>,
    filter: &CustomFilter,
) {
    let similar_paths = similar_paths.clone();
    let filter = filter.clone();
    let btn = similar_filter_btn.clone();
    similar_filter_btn.connect_clicked(move |_| {
        clear_similar_filter(&similar_paths, &filter, &btn);
    });
}

pub(crate) fn install_favorites_only_handler(
    favourites_filter_btn: &gtk4::ToggleButton,
    favorites_only: &Rc<Cell<bool>>,
    filter: &CustomFilter,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    toast_overlay: &adw::ToastOverlay,
    selection_model: &SingleSelection,
    list_store: &ListStore,
) {
    let favourites_filter_btn_toggle = favourites_filter_btn.clone();
    let favorites_only_toggle = favorites_only.clone();
    let filter_toggle = filter.clone();
    let current_folder_toggle = current_folder.clone();
    let toast_overlay_toggle = toast_overlay.clone();
    let selection_model_toggle = selection_model.clone();
    let list_store_toggle = list_store.clone();
    favourites_filter_btn.connect_toggled(move |btn| {
        let active = btn.is_active();
        favorites_only_toggle.set(active);
        if active {
            favourites_filter_btn_toggle.add_css_class("favorites-filter-active");
        } else {
            favourites_filter_btn_toggle.remove_css_class("favorites-filter-active");
        }
        if let Some(folder) = current_folder_toggle.borrow().as_ref() {
            let _ = db::set_ui_state_value(
                folder.as_path(),
                "favorites_only",
                if active { "1" } else { "0" },
            );
        }
        filter_toggle.changed(gtk4::FilterChange::Different);
        if active
            && list_store_toggle.n_items() > 0
            && selection_model_toggle.n_items() == 0
        {
            let toast = adw::Toast::new(
                "No favourites match — turn off the favourites filter to see all images.",
            );
            toast.set_timeout(2);
            toast_overlay_toggle.add_toast(toast);
        }
    });
}

pub(crate) fn install_thumbnail_size_handlers(
    size_buttons: &Rc<Vec<gtk4::ToggleButton>>,
    size_options: [i32; 4],
    app_state: &AppState,
    grid_view: &gtk4::GridView,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
) {
    let app_state_toggle = app_state.clone();
    let grid_view_toggle = grid_view.clone();
    for (idx, button) in size_buttons.iter().enumerate() {
        let options = size_options;
        let buttons = size_buttons.clone();
        let app_state_toggle = app_state_toggle.clone();
        let grid_view_toggle = grid_view_toggle.clone();
        let current_folder_toggle = current_folder.clone();
        button.connect_clicked(move |_| {
            let selected_size = options[idx];
            let was_selected = *app_state_toggle.thumbnail_size.borrow() == selected_size;
            *app_state_toggle.thumbnail_size.borrow_mut() = selected_size;

            for (i, btn) in buttons.iter().enumerate() {
                btn.set_active(i == idx);
            }

            if was_selected {
                return;
            }
            if let Some(folder) = current_folder_toggle.borrow().as_ref() {
                let _ = db::set_ui_state_value(
                    folder.as_path(),
                    "thumbnail_size",
                    &selected_size.to_string(),
                );
            }

            apply_thumbnail_size_change(
                selected_size,
                &app_state_toggle,
                &grid_view_toggle,
            );
        });
    }
}
