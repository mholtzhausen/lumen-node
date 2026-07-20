use crate::config;
use crate::db;
use crate::recent_folders::push_recent_folder_entry;
use crate::sort::sort_index_for_key;
use crate::thumbnail_sizing::{normalize_thumbnail_size, thumbnail_size_options};
use crate::tree_sidebar::{reset_tree_root, sync_tree_to_path};
use crate::ui::controls::refresh_tag_filter_from_folder;
use crate::ScanProgressState;
use gtk4::gio;
use gtk4::prelude::*;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
};

pub(crate) struct OpenFolderActionDeps {
    pub(crate) current_folder: Rc<RefCell<Option<PathBuf>>>,
    pub(crate) start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    pub(crate) tree_root: gio::ListStore,
    pub(crate) tree_model: gtk4::TreeListModel,
    pub(crate) tree_list_view: gtk4::ListView,
    pub(crate) recent_folders: Rc<RefCell<Vec<PathBuf>>>,
    pub(crate) sort_key: Rc<RefCell<String>>,
    pub(crate) search_text: Rc<RefCell<String>>,
    pub(crate) favorites_only: Rc<Cell<bool>>,
    pub(crate) active_tag_filters:
        Rc<RefCell<std::collections::HashMap<String, crate::db::TagFilterMode>>>,
    pub(crate) tag_filter_debounce_gen: Rc<Cell<u64>>,
    pub(crate) thumbnail_size: Rc<RefCell<i32>>,
    pub(crate) sort_dropdown: gtk4::DropDown,
    pub(crate) favourites_filter_btn: gtk4::ToggleButton,
    pub(crate) tags_filter_btn: gtk4::MenuButton,
    pub(crate) tags_filter_list: gtk4::Box,
    pub(crate) search_entry: gtk4::SearchEntry,
    pub(crate) filter: gtk4::CustomFilter,
    pub(crate) sorter: gtk4::CustomSorter,
    pub(crate) size_buttons: Rc<Vec<gtk4::ToggleButton>>,
    pub(crate) progress_state: Rc<RefCell<ScanProgressState>>,
    pub(crate) recent_folders_limit: usize,
    pub(crate) grid_loading: Rc<RefCell<Option<crate::ui::grid_loading::GridLoadingOverlay>>>,
}

pub(crate) fn build_open_folder_action(deps: OpenFolderActionDeps) -> Rc<dyn Fn(PathBuf, bool)> {
    Rc::new(move |path: PathBuf, sync_tree: bool| {
        if deps.current_folder.borrow().as_deref() == Some(path.as_path()) {
            return;
        }

        if let Some(saved_ui_state) = db::load_ui_state(path.as_path()) {
            let selected_sort = sort_index_for_key(&saved_ui_state.sort_key);
            *deps.sort_key.borrow_mut() = saved_ui_state.sort_key;
            *deps.search_text.borrow_mut() = saved_ui_state.search_text.clone();
            deps.favorites_only.set(saved_ui_state.favorites_only);
            *deps.active_tag_filters.borrow_mut() = saved_ui_state.active_tag_filters.clone();
            deps.tag_filter_debounce_gen.set(0);
            *deps.thumbnail_size.borrow_mut() =
                normalize_thumbnail_size(saved_ui_state.thumbnail_size);

            if deps.sort_dropdown.selected() != selected_sort {
                deps.sort_dropdown.set_selected(selected_sort);
            }
            deps.favourites_filter_btn.set_active(saved_ui_state.favorites_only);
            if saved_ui_state.favorites_only {
                deps.favourites_filter_btn
                    .add_css_class("favorites-filter-active");
            } else {
                deps.favourites_filter_btn
                    .remove_css_class("favorites-filter-active");
            }
            deps.search_entry.set_text(&saved_ui_state.search_text);
            deps.filter.changed(gtk4::FilterChange::Different);
            deps.sorter.changed(gtk4::SorterChange::Different);
            for (i, btn) in deps.size_buttons.iter().enumerate() {
                btn.set_active(thumbnail_size_options()[i] == *deps.thumbnail_size.borrow());
            }
        } else {
            deps.active_tag_filters.borrow_mut().clear();
            deps.tag_filter_debounce_gen.set(0);
            let seeded_state = db::UiState {
                sort_key: deps.sort_key.borrow().clone(),
                search_text: deps.search_text.borrow().clone(),
                favorites_only: deps.favorites_only.get(),
                active_tag_filters: HashMap::new(),
                thumbnail_size: *deps.thumbnail_size.borrow(),
            };
            let _ = db::save_ui_state(path.as_path(), &seeded_state);
        }

        *deps.current_folder.borrow_mut() = Some(path.clone());
        deps.progress_state.borrow_mut().current_folder_path = path.display().to_string();
        {
            let mut history = deps.recent_folders.borrow_mut();
            push_recent_folder_entry(&mut history, path.as_path(), deps.recent_folders_limit);
            config::save_recent_state(Some(path.as_path()), &history);
        }
        refresh_tag_filter_from_folder(
            &deps.tags_filter_list,
            &deps.tags_filter_btn,
            &deps.active_tag_filters,
            &deps.tag_filter_debounce_gen,
            &deps.filter,
            &deps.current_folder,
            &deps.grid_loading,
        );
        reset_tree_root(&deps.tree_root, path.as_path());
        (deps.start_scan_for_folder)(path.clone());
        if sync_tree {
            sync_tree_to_path(&deps.tree_model, &deps.tree_list_view, &path);
        }
    })
}
