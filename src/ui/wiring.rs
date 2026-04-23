use crate::metadata::ImageMetadata;
use crate::thumbnail_sizing::thumbnail_size_options;
use crate::ui::actions::install_context_menu;
use crate::ui::controls::{
    install_clear_button_handler, install_search_entry_handler, install_sort_dropdown_handler,
    install_thumbnail_size_handlers,
};
use crate::ui::open_folder::{build_open_folder_action, OpenFolderActionDeps};
use crate::ui::selection::{handle_selection_change_event, ClickTrace};
use crate::ui::shell::{install_history_popover_handler, install_open_button_handler};
use crate::ui::sidebar::populate_metadata_sidebar;
use gtk4::prelude::*;
use gtk4::{gio, glib, StringObject};
use libadwaita as adw;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
};

pub(crate) struct ContextMenuWiringDeps {
    pub(crate) window: adw::ApplicationWindow,
    pub(crate) toast_overlay: adw::ToastOverlay,
    pub(crate) selection_model: gtk4::SingleSelection,
    pub(crate) meta_cache: Rc<RefCell<HashMap<String, ImageMetadata>>>,
    pub(crate) hash_cache: Rc<RefCell<HashMap<String, String>>>,
    pub(crate) thumbnail_size: Rc<RefCell<i32>>,
    pub(crate) meta_expander: gtk4::Expander,
    pub(crate) meta_paned: gtk4::Paned,
    pub(crate) meta_split_before_auto_collapse: Rc<Cell<Option<i32>>>,
    pub(crate) meta_position_programmatic: Rc<Cell<u32>>,
    pub(crate) min_meta_split_px: i32,
    pub(crate) current_folder: Rc<RefCell<Option<PathBuf>>>,
    pub(crate) start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    pub(crate) list_store: gio::ListStore,
    pub(crate) meta_listbox: gtk4::ListBox,
    pub(crate) grid_view: gtk4::GridView,
    pub(crate) single_picture: gtk4::Picture,
    pub(crate) meta_preview: gtk4::Picture,
}

pub(crate) fn install_context_menu_wiring(deps: ContextMenuWiringDeps) {
    let refresh_metadata_sidebar: Rc<dyn Fn(&ImageMetadata)> = Rc::new({
        let meta_listbox = deps.meta_listbox.clone();
        move |meta: &ImageMetadata| populate_metadata_sidebar(&meta_listbox, meta)
    });
    let start_scan_for_folder: Rc<dyn Fn(PathBuf)> = deps.start_scan_for_folder.clone();
    install_context_menu(
        &deps.window,
        &deps.toast_overlay,
        &deps.selection_model,
        &deps.meta_cache,
        &deps.hash_cache,
        &deps.thumbnail_size,
        &deps.meta_expander,
        &deps.meta_paned,
        &deps.meta_split_before_auto_collapse,
        &deps.meta_position_programmatic,
        deps.min_meta_split_px,
        &deps.current_folder,
        &start_scan_for_folder,
        &deps.list_store,
        &refresh_metadata_sidebar,
        &deps.grid_view,
        &deps.single_picture,
        &deps.meta_preview,
    );
}

pub(crate) struct SelectionWiringDeps {
    pub(crate) selection_model: gtk4::SingleSelection,
    pub(crate) meta_cache: Rc<RefCell<HashMap<String, ImageMetadata>>>,
    pub(crate) meta_listbox: gtk4::ListBox,
    pub(crate) meta_expander: gtk4::Expander,
    pub(crate) meta_paned: gtk4::Paned,
    pub(crate) meta_split_before_auto_collapse: Rc<Cell<Option<i32>>>,
    pub(crate) meta_position_programmatic: Rc<Cell<u32>>,
    pub(crate) meta_preview: gtk4::Picture,
    pub(crate) realized_thumb_images: Rc<RefCell<Vec<glib::WeakRef<gtk4::Image>>>>,
    pub(crate) thumbnail_size: Rc<RefCell<i32>>,
    pub(crate) hash_cache: Rc<RefCell<HashMap<String, String>>>,
}

pub(crate) fn install_selection_wiring(deps: SelectionWiringDeps) {
    let click_trace_state: Rc<RefCell<Option<ClickTrace>>> = Rc::new(RefCell::new(None));
    let click_trace_state_sel = click_trace_state.clone();
    let meta_listbox_sel = deps.meta_listbox.clone();
    let meta_expander_sel = deps.meta_expander.clone();
    let meta_paned_sel = deps.meta_paned.clone();
    let meta_split_before_auto_collapse_sel = deps.meta_split_before_auto_collapse.clone();
    let meta_position_programmatic_sel = deps.meta_position_programmatic.clone();
    let meta_preview_sel = deps.meta_preview.clone();
    let meta_cache_sel = deps.meta_cache.clone();
    let realized_thumb_images_sel = deps.realized_thumb_images.clone();
    let thumbnail_size_sel = deps.thumbnail_size.clone();
    let hash_cache_sel = deps.hash_cache.clone();
    deps.selection_model.connect_selection_changed(move |model, _, _| {
        let Some(item) = model.selected_item().and_downcast::<StringObject>() else {
            return;
        };
        handle_selection_change_event(
            &item,
            &click_trace_state_sel,
            &meta_cache_sel,
            &meta_listbox_sel,
            &meta_expander_sel,
            &meta_paned_sel,
            &meta_split_before_auto_collapse_sel,
            &meta_position_programmatic_sel,
            &meta_preview_sel,
            &realized_thumb_images_sel,
            &thumbnail_size_sel,
            &hash_cache_sel,
        );
    });
}

pub(crate) struct OpenFolderWiringDeps {
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
    pub(crate) progress_state: Rc<RefCell<crate::ScanProgressState>>,
    pub(crate) recent_folders_limit: usize,
    pub(crate) history_popover: gtk4::Popover,
    pub(crate) history_list: gtk4::Box,
    pub(crate) open_btn: gtk4::Button,
    pub(crate) window: adw::ApplicationWindow,
}

pub(crate) fn install_open_folder_wiring(
    deps: OpenFolderWiringDeps,
) -> Rc<dyn Fn(PathBuf, bool)> {
    let open_folder_action = build_open_folder_action(OpenFolderActionDeps {
        current_folder: deps.current_folder.clone(),
        start_scan_for_folder: deps.start_scan_for_folder,
        tree_root: deps.tree_root,
        tree_model: deps.tree_model,
        tree_list_view: deps.tree_list_view,
        recent_folders: deps.recent_folders.clone(),
        sort_key: deps.sort_key,
        search_text: deps.search_text,
        thumbnail_size: deps.thumbnail_size,
        sort_dropdown: deps.sort_dropdown,
        search_entry: deps.search_entry,
        filter: deps.filter,
        sorter: deps.sorter,
        size_buttons: deps.size_buttons,
        progress_state: deps.progress_state,
        recent_folders_limit: deps.recent_folders_limit,
    });

    install_history_popover_handler(
        &deps.history_popover,
        &deps.history_list,
        &deps.recent_folders,
        &deps.current_folder,
        open_folder_action.clone(),
        deps.recent_folders_limit,
    );

    install_open_button_handler(
        &deps.open_btn,
        &deps.window,
        &deps.current_folder,
        open_folder_action.clone(),
    );

    open_folder_action
}

pub(crate) struct ControlsWiringDeps {
    pub(crate) sort_dropdown: gtk4::DropDown,
    pub(crate) sort_key: Rc<RefCell<String>>,
    pub(crate) sorter: gtk4::CustomSorter,
    pub(crate) current_folder: Rc<RefCell<Option<PathBuf>>>,
    pub(crate) scan_in_progress: Rc<Cell<bool>>,
    pub(crate) start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    pub(crate) search_entry: gtk4::SearchEntry,
    pub(crate) search_text: Rc<RefCell<String>>,
    pub(crate) filter: gtk4::CustomFilter,
    pub(crate) clear_btn: gtk4::Button,
    pub(crate) thumbnail_size: Rc<RefCell<i32>>,
    pub(crate) size_buttons: Rc<Vec<gtk4::ToggleButton>>,
    pub(crate) grid_view: gtk4::GridView,
    pub(crate) realized_thumb_images: Rc<RefCell<Vec<glib::WeakRef<gtk4::Image>>>>,
    pub(crate) realized_cell_boxes: Rc<RefCell<Vec<glib::WeakRef<gtk4::Box>>>>,
    pub(crate) hash_cache: Rc<RefCell<HashMap<String, String>>>,
}

pub(crate) fn install_controls_wiring(deps: ControlsWiringDeps) {
    install_sort_dropdown_handler(
        &deps.sort_dropdown,
        &deps.sort_key,
        &deps.sorter,
        &deps.current_folder,
        &deps.scan_in_progress,
        &deps.start_scan_for_folder,
    );
    install_search_entry_handler(
        &deps.search_entry,
        &deps.search_text,
        &deps.filter,
        &deps.current_folder,
    );
    install_clear_button_handler(
        &deps.clear_btn,
        &deps.search_text,
        &deps.sort_key,
        &deps.filter,
        &deps.sorter,
        &deps.search_entry,
        &deps.sort_dropdown,
        &deps.thumbnail_size,
        &deps.current_folder,
    );
    install_thumbnail_size_handlers(
        &deps.size_buttons,
        thumbnail_size_options(),
        &deps.thumbnail_size,
        &deps.grid_view,
        &deps.realized_thumb_images,
        &deps.realized_cell_boxes,
        &deps.hash_cache,
        &deps.current_folder,
    );
}
