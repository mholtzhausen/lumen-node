use crate::core::app_state::AppState;
use crate::metadata::ImageMetadata;
use crate::thumbnail_sizing::thumbnail_size_options;
use crate::ui::actions::install_context_menu;
use crate::ui::center::CenterContentBundle;
use crate::ui::controls::{
    install_clear_button_handler, install_search_entry_handler, install_sort_dropdown_handler,
    install_thumbnail_size_handlers,
};
use crate::ui::left_chrome_wiring::LeftChromeWiring;
use crate::ui::open_folder::{build_open_folder_action, OpenFolderActionDeps};
use crate::ui::right_sidebar::RightSidebarBundle;
use crate::ui::selection::{handle_selection_change_event, ClickTrace};
use crate::ui::shell::{install_history_popover_handler, install_open_button_handler};
use crate::ui::sidebar::populate_metadata_sidebar;
use gtk4::prelude::*;
use gtk4::StringObject;
use libadwaita as adw;
use std::{cell::RefCell, path::PathBuf, rc::Rc};

pub(crate) struct ContextMenuWiringDeps {
    pub(crate) app_state: AppState,
    pub(crate) window: adw::ApplicationWindow,
    pub(crate) toast_overlay: adw::ToastOverlay,
    pub(crate) selection_model: gtk4::SingleSelection,
    pub(crate) center: CenterContentBundle,
    pub(crate) right: RightSidebarBundle,
    pub(crate) min_meta_split_px: i32,
    pub(crate) start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    pub(crate) external_editor: Option<PathBuf>,
}

pub(crate) fn install_context_menu_wiring(deps: ContextMenuWiringDeps) -> Rc<dyn Fn()> {
    let refresh_metadata_sidebar: Rc<dyn Fn(&ImageMetadata)> = Rc::new({
        let meta_listbox = deps.right.meta_listbox.clone();
        move |meta: &ImageMetadata| populate_metadata_sidebar(&meta_listbox, meta)
    });
    let start_scan_for_folder: Rc<dyn Fn(PathBuf)> = deps.start_scan_for_folder.clone();
    install_context_menu(
        &deps.window,
        &deps.toast_overlay,
        &deps.selection_model,
        &deps.app_state.meta_cache,
        &deps.app_state.hash_cache,
        &deps.app_state.thumbnail_size,
        &deps.right.meta_expander,
        &deps.right.meta_paned,
        &deps.right.meta_split_before_auto_collapse,
        &deps.right.meta_position_programmatic,
        deps.min_meta_split_px,
        &deps.app_state.current_folder,
        &start_scan_for_folder,
        &deps.app_state.list_store,
        &refresh_metadata_sidebar,
        deps.external_editor.as_ref(),
        &deps.center.grid_view,
        &deps.center.single_picture,
        &deps.right.meta_preview,
    )
}

pub(crate) struct SelectionWiringDeps {
    pub(crate) app_state: AppState,
    pub(crate) selection_model: gtk4::SingleSelection,
    pub(crate) right: RightSidebarBundle,
}

pub(crate) fn install_selection_wiring(deps: SelectionWiringDeps) {
    let click_trace_state: Rc<RefCell<Option<ClickTrace>>> = Rc::new(RefCell::new(None));
    let click_trace_state_sel = click_trace_state.clone();
    let meta_listbox_sel = deps.right.meta_listbox.clone();
    let meta_expander_sel = deps.right.meta_expander.clone();
    let meta_paned_sel = deps.right.meta_paned.clone();
    let meta_split_before_auto_collapse_sel = deps.right.meta_split_before_auto_collapse.clone();
    let meta_position_programmatic_sel = deps.right.meta_position_programmatic.clone();
    let meta_preview_sel = deps.right.meta_preview.clone();
    let meta_cache_sel = deps.app_state.meta_cache.clone();
    let realized_thumb_images_sel = deps.app_state.realized_thumb_images.clone();
    let thumbnail_size_sel = deps.app_state.thumbnail_size.clone();
    let hash_cache_sel = deps.app_state.hash_cache.clone();
    deps.selection_model
        .connect_selection_changed(move |model, _, _| {
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
    pub(crate) app_state: AppState,
    pub(crate) start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    pub(crate) chrome: LeftChromeWiring,
    pub(crate) filter: gtk4::CustomFilter,
    pub(crate) sorter: gtk4::CustomSorter,
    pub(crate) recent_folders_limit: usize,
    pub(crate) window: adw::ApplicationWindow,
}

pub(crate) fn install_open_folder_wiring(deps: OpenFolderWiringDeps) -> Rc<dyn Fn(PathBuf, bool)> {
    let open_folder_action = build_open_folder_action(OpenFolderActionDeps {
        current_folder: deps.app_state.current_folder.clone(),
        start_scan_for_folder: deps.start_scan_for_folder,
        tree_root: deps.chrome.tree_root,
        tree_model: deps.chrome.tree_model,
        tree_list_view: deps.chrome.tree_list_view,
        recent_folders: deps.app_state.recent_folders.clone(),
        sort_key: deps.app_state.sort_key.clone(),
        search_text: deps.app_state.search_text.clone(),
        thumbnail_size: deps.app_state.thumbnail_size.clone(),
        sort_dropdown: deps.chrome.sort_dropdown,
        search_entry: deps.chrome.search_entry,
        filter: deps.filter,
        sorter: deps.sorter,
        size_buttons: deps.chrome.size_buttons,
        progress_state: deps.app_state.progress_state.clone(),
        recent_folders_limit: deps.recent_folders_limit,
    });

    install_history_popover_handler(
        &deps.chrome.history_popover,
        &deps.chrome.history_list,
        &deps.app_state.recent_folders,
        &deps.app_state.current_folder,
        open_folder_action.clone(),
        deps.recent_folders_limit,
    );

    install_open_button_handler(
        &deps.chrome.open_btn,
        &deps.window,
        &deps.app_state.current_folder,
        open_folder_action.clone(),
    );

    open_folder_action
}

pub(crate) struct ControlsWiringDeps {
    pub(crate) app_state: AppState,
    pub(crate) chrome: LeftChromeWiring,
    pub(crate) center: CenterContentBundle,
    pub(crate) sorter: gtk4::CustomSorter,
    pub(crate) start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    pub(crate) filter: gtk4::CustomFilter,
}

pub(crate) fn install_controls_wiring(deps: ControlsWiringDeps) {
    install_sort_dropdown_handler(
        &deps.chrome.sort_dropdown,
        &deps.app_state.sort_key,
        &deps.sorter,
        &deps.app_state.current_folder,
        &deps.app_state.scan_in_progress,
        &deps.start_scan_for_folder,
    );
    install_search_entry_handler(
        &deps.chrome.search_entry,
        &deps.app_state.search_text,
        &deps.filter,
        &deps.app_state.current_folder,
    );
    install_clear_button_handler(
        &deps.chrome.clear_btn,
        &deps.app_state.search_text,
        &deps.app_state.sort_key,
        &deps.filter,
        &deps.sorter,
        &deps.chrome.search_entry,
        &deps.chrome.sort_dropdown,
        &deps.app_state.thumbnail_size,
        &deps.app_state.current_folder,
    );
    install_thumbnail_size_handlers(
        &deps.chrome.size_buttons,
        thumbnail_size_options(),
        &deps.app_state.thumbnail_size,
        &deps.center.grid_view,
        &deps.app_state.realized_thumb_images,
        &deps.app_state.realized_cell_boxes,
        &deps.app_state.hash_cache,
        &deps.app_state.current_folder,
    );
}
