use crate::core::app_state::AppState;
use crate::db;
use crate::sort::{sort_key_for_index, SORT_KEY_NAME_ASC};
use crate::ui::grid::apply_thumbnail_size_change;
use gtk4::prelude::*;
use gtk4::{CustomFilter, CustomSorter};
use std::{cell::Cell, cell::RefCell, path::PathBuf, rc::Rc};

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

pub(crate) fn install_clear_button_handler(
    clear_btn: &gtk4::Button,
    search_text: &Rc<RefCell<String>>,
    favorites_only: &Rc<Cell<bool>>,
    sort_key: &Rc<RefCell<String>>,
    filter: &CustomFilter,
    sorter: &CustomSorter,
    favourites_filter_btn: &gtk4::ToggleButton,
    search_entry: &gtk4::SearchEntry,
    sort_dropdown: &gtk4::DropDown,
    thumbnail_size: &Rc<RefCell<i32>>,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
) {
    let search_text_clear = search_text.clone();
    let favorites_only_clear = favorites_only.clone();
    let sort_key_clear = sort_key.clone();
    let filter_clear = filter.clone();
    let sorter_clear = sorter.clone();
    let favourites_filter_btn_clear = favourites_filter_btn.clone();
    let search_entry_clear = search_entry.clone();
    let sort_dropdown_clear = sort_dropdown.clone();
    let thumbnail_size_clear = thumbnail_size.clone();
    let current_folder_clear = current_folder.clone();
    clear_btn.connect_clicked(move |_| {
        *search_text_clear.borrow_mut() = String::new();
        favorites_only_clear.set(false);
        *sort_key_clear.borrow_mut() = SORT_KEY_NAME_ASC.to_string();
        favourites_filter_btn_clear.remove_css_class("favorites-filter-active");
        favourites_filter_btn_clear.set_active(false);
        search_entry_clear.set_text("");
        sort_dropdown_clear.set_selected(0);
        if let Some(folder) = current_folder_clear.borrow().as_ref() {
            let _ = db::save_ui_state(
                folder.as_path(),
                &db::UiState {
                    sort_key: sort_key_clear.borrow().clone(),
                    search_text: search_text_clear.borrow().clone(),
                    favorites_only: favorites_only_clear.get(),
                    thumbnail_size: *thumbnail_size_clear.borrow(),
                },
            );
        }
        filter_clear.changed(gtk4::FilterChange::LessStrict);
        sorter_clear.changed(gtk4::SorterChange::Different);
    });
}

pub(crate) fn install_favorites_only_handler(
    favourites_filter_btn: &gtk4::ToggleButton,
    favorites_only: &Rc<Cell<bool>>,
    filter: &CustomFilter,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
) {
    let favourites_filter_btn_toggle = favourites_filter_btn.clone();
    let favorites_only_toggle = favorites_only.clone();
    let filter_toggle = filter.clone();
    let current_folder_toggle = current_folder.clone();
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
