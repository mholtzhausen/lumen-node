use crate::db;
use crate::sort::{sort_key_for_index, SORT_KEY_NAME_ASC};
use crate::ui::grid::apply_thumbnail_size_change;
use gtk4::prelude::*;
use gtk4::{glib, CustomFilter, CustomSorter, Image};
use std::{cell::Cell, cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc};

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
    sort_key: &Rc<RefCell<String>>,
    filter: &CustomFilter,
    sorter: &CustomSorter,
    search_entry: &gtk4::SearchEntry,
    sort_dropdown: &gtk4::DropDown,
    thumbnail_size: &Rc<RefCell<i32>>,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
) {
    let search_text_clear = search_text.clone();
    let sort_key_clear = sort_key.clone();
    let filter_clear = filter.clone();
    let sorter_clear = sorter.clone();
    let search_entry_clear = search_entry.clone();
    let sort_dropdown_clear = sort_dropdown.clone();
    let thumbnail_size_clear = thumbnail_size.clone();
    let current_folder_clear = current_folder.clone();
    clear_btn.connect_clicked(move |_| {
        *search_text_clear.borrow_mut() = String::new();
        *sort_key_clear.borrow_mut() = SORT_KEY_NAME_ASC.to_string();
        search_entry_clear.set_text("");
        sort_dropdown_clear.set_selected(0);
        if let Some(folder) = current_folder_clear.borrow().as_ref() {
            let _ = db::save_ui_state(
                folder.as_path(),
                &db::UiState {
                    sort_key: sort_key_clear.borrow().clone(),
                    search_text: search_text_clear.borrow().clone(),
                    thumbnail_size: *thumbnail_size_clear.borrow(),
                },
            );
        }
        filter_clear.changed(gtk4::FilterChange::LessStrict);
        sorter_clear.changed(gtk4::SorterChange::Different);
    });
}

pub(crate) fn install_thumbnail_size_handlers(
    size_buttons: &Rc<Vec<gtk4::ToggleButton>>,
    size_options: [i32; 4],
    thumbnail_size: &Rc<RefCell<i32>>,
    grid_view: &gtk4::GridView,
    realized_thumb_images: &Rc<RefCell<Vec<glib::WeakRef<Image>>>>,
    realized_cell_boxes: &Rc<RefCell<Vec<glib::WeakRef<gtk4::Box>>>>,
    hash_cache: &Rc<RefCell<HashMap<String, String>>>,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
) {
    let grid_view_toggle = grid_view.clone();
    let realized_thumb_images_toggle = realized_thumb_images.clone();
    let realized_cell_boxes_toggle = realized_cell_boxes.clone();
    let hash_cache_toggle = hash_cache.clone();
    for (idx, button) in size_buttons.iter().enumerate() {
        let options = size_options;
        let buttons = size_buttons.clone();
        let thumbnail_size_toggle = thumbnail_size.clone();
        let grid_view_toggle = grid_view_toggle.clone();
        let realized_thumb_images_toggle = realized_thumb_images_toggle.clone();
        let realized_cell_boxes_toggle = realized_cell_boxes_toggle.clone();
        let hash_cache_toggle = hash_cache_toggle.clone();
        let current_folder_toggle = current_folder.clone();
        button.connect_clicked(move |_| {
            let selected_size = options[idx];
            let was_selected = *thumbnail_size_toggle.borrow() == selected_size;
            *thumbnail_size_toggle.borrow_mut() = selected_size;

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
                &realized_cell_boxes_toggle,
                &realized_thumb_images_toggle,
                &thumbnail_size_toggle,
                &hash_cache_toggle,
                &grid_view_toggle,
            );
        });
    }
}
