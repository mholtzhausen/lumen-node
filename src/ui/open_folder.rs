use crate::config;
use crate::db;
use crate::recent_folders::push_recent_folder_entry;
use crate::sort::sort_index_for_key;
use crate::thumbnail_sizing::{normalize_thumbnail_size, thumbnail_size_options};
use crate::tree_sidebar::{reset_tree_root, sync_tree_to_path};
use crate::ScanProgressState;
use gtk4::gio;
use gtk4::prelude::*;
use std::{cell::RefCell, path::PathBuf, rc::Rc};

pub(crate) struct OpenFolderActionDeps {
    pub(crate) current_folder: Rc<RefCell<Option<PathBuf>>>,
    pub(crate) start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    pub(crate) tree_root: gio::ListStore,
    pub(crate) tree_model: gtk4::TreeListModel,
    pub(crate) tree_list_view: gtk4::ListView,
    pub(crate) recent_folders: Rc<RefCell<Vec<PathBuf>>>,
    pub(crate) sort_key: Rc<RefCell<String>>,
    pub(crate) search_text: Rc<RefCell<String>>,
    pub(crate) thumbnail_size: Rc<RefCell<i32>>,
    pub(crate) sort_dropdown: gtk4::DropDown,
    pub(crate) search_entry: gtk4::SearchEntry,
    pub(crate) filter: gtk4::CustomFilter,
    pub(crate) sorter: gtk4::CustomSorter,
    pub(crate) size_buttons: Rc<Vec<gtk4::ToggleButton>>,
    pub(crate) progress_state: Rc<RefCell<ScanProgressState>>,
    pub(crate) recent_folders_limit: usize,
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
            *deps.thumbnail_size.borrow_mut() =
                normalize_thumbnail_size(saved_ui_state.thumbnail_size);

            if deps.sort_dropdown.selected() != selected_sort {
                deps.sort_dropdown.set_selected(selected_sort);
            }
            deps.search_entry.set_text(&saved_ui_state.search_text);
            deps.filter.changed(gtk4::FilterChange::Different);
            deps.sorter.changed(gtk4::SorterChange::Different);
            for (i, btn) in deps.size_buttons.iter().enumerate() {
                btn.set_active(thumbnail_size_options()[i] == *deps.thumbnail_size.borrow());
            }
        } else {
            let seeded_state = db::UiState {
                sort_key: deps.sort_key.borrow().clone(),
                search_text: deps.search_text.borrow().clone(),
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
        reset_tree_root(&deps.tree_root, path.as_path());
        (deps.start_scan_for_folder)(path.clone());
        if sync_tree {
            sync_tree_to_path(&deps.tree_model, &deps.tree_list_view, &path);
        }
    })
}
