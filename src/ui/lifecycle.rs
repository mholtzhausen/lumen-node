use crate::config::AppConfig;
use crate::services::update_checker::install_update_checker;
use crate::ui::keyboard::{
    install_keyboard_handler, install_scroll_navigation_handlers, KeyboardDeps,
};
use crate::ui::left_chrome_wiring::LeftChromeWiring;
use crate::ui::session::{
    install_close_persistence_handler, restore_session_state, ClosePersistenceDeps,
    RestoreSessionDeps,
};
use libadwaita as adw;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

pub(crate) struct LifecycleDeps {
    pub(crate) update_banner: adw::Banner,
    pub(crate) window: adw::ApplicationWindow,
    pub(crate) view_stack: adw::ViewStack,
    pub(crate) selection_model: gtk4::SingleSelection,
    pub(crate) single_picture: gtk4::Picture,
    pub(crate) grid_view: gtk4::GridView,
    pub(crate) grid_scroll: gtk4::ScrolledWindow,
    pub(crate) thumbnail_size: Rc<RefCell<i32>>,
    pub(crate) toast_overlay: adw::ToastOverlay,
    pub(crate) current_folder: Rc<RefCell<Option<PathBuf>>>,
    pub(crate) start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    pub(crate) chrome: LeftChromeWiring,
    pub(crate) pre_fullview_left: Rc<Cell<bool>>,
    pub(crate) pre_fullview_right: Rc<Cell<bool>>,
    pub(crate) meta_preview: gtk4::Picture,
    pub(crate) outer_paned: gtk4::Paned,
    pub(crate) inner_paned: gtk4::Paned,
    pub(crate) meta_paned: gtk4::Paned,
    pub(crate) meta_split_before_auto_collapse: Rc<Cell<Option<i32>>>,
    pub(crate) sort_key: Rc<RefCell<String>>,
    pub(crate) search_text: Rc<RefCell<String>>,
    pub(crate) recent_folders: Rc<RefCell<Vec<PathBuf>>>,
    pub(crate) outer_split_dirty: Rc<Cell<bool>>,
    pub(crate) inner_split_dirty: Rc<Cell<bool>>,
    pub(crate) meta_split_dirty: Rc<Cell<bool>>,
    pub(crate) configured_left_pane_pos: Option<i32>,
    pub(crate) configured_right_pane_pos: Option<i32>,
    pub(crate) configured_meta_pane_pos: Option<i32>,
    pub(crate) configured_left_pane_width_pct: Option<f64>,
    pub(crate) configured_right_pane_width_pct: Option<f64>,
    pub(crate) configured_meta_pane_height_pct: Option<f64>,
    pub(crate) min_meta_split_px: i32,
    pub(crate) app_config: AppConfig,
    pub(crate) open_folder_action: Rc<dyn Fn(PathBuf, bool)>,
    pub(crate) filter: gtk4::CustomFilter,
    pub(crate) outer_position_programmatic: Rc<Cell<u32>>,
    pub(crate) inner_position_programmatic: Rc<Cell<u32>>,
    pub(crate) meta_position_programmatic: Rc<Cell<u32>>,
    pub(crate) pane_restore_complete: Rc<Cell<bool>>,
    pub(crate) min_left_pane_px: i32,
    pub(crate) min_right_pane_px: i32,
    pub(crate) min_center_pane_px: i32,
}

pub(crate) fn install_lifecycle(deps: LifecycleDeps) {
    install_update_checker(deps.update_banner.clone());

    install_keyboard_handler(KeyboardDeps {
        window: deps.window.clone(),
        view_stack: deps.view_stack.clone(),
        selection_model: deps.selection_model.clone(),
        single_picture: deps.single_picture.clone(),
        grid_view: deps.grid_view.clone(),
        grid_scroll: deps.grid_scroll.clone(),
        thumbnail_size: deps.thumbnail_size.clone(),
        toast_overlay: deps.toast_overlay.clone(),
        current_folder: deps.current_folder.clone(),
        start_scan_for_folder: deps.start_scan_for_folder.clone(),
        left_toggle: deps.chrome.left_toggle.clone(),
        right_toggle: deps.chrome.right_toggle.clone(),
        pre_fullview_left: deps.pre_fullview_left.clone(),
        pre_fullview_right: deps.pre_fullview_right.clone(),
    });

    // Scroll on single-view / meta-preview -> navigate images.
    install_scroll_navigation_handlers(
        &deps.selection_model,
        &deps.single_picture,
        &deps.meta_preview,
    );

    install_close_persistence_handler(ClosePersistenceDeps {
        current_folder: deps.current_folder.clone(),
        outer_paned: deps.outer_paned.clone(),
        inner_paned: deps.inner_paned.clone(),
        meta_paned: deps.meta_paned.clone(),
        meta_split_before_auto_collapse: deps.meta_split_before_auto_collapse.clone(),
        sort_key: deps.sort_key.clone(),
        search_text: deps.search_text.clone(),
        thumbnail_size: deps.thumbnail_size.clone(),
        recent_folders: deps.recent_folders.clone(),
        left_toggle: deps.chrome.left_toggle.clone(),
        right_toggle: deps.chrome.right_toggle.clone(),
        window: deps.window.clone(),
        outer_split_dirty: deps.outer_split_dirty.clone(),
        inner_split_dirty: deps.inner_split_dirty.clone(),
        meta_split_dirty: deps.meta_split_dirty.clone(),
        configured_left_pane_pos: deps.configured_left_pane_pos,
        configured_right_pane_pos: deps.configured_right_pane_pos,
        configured_meta_pane_pos: deps.configured_meta_pane_pos,
        configured_left_pane_width_pct: deps.configured_left_pane_width_pct,
        configured_right_pane_width_pct: deps.configured_right_pane_width_pct,
        configured_meta_pane_height_pct: deps.configured_meta_pane_height_pct,
        min_meta_split_px: deps.min_meta_split_px,
    });

    restore_session_state(RestoreSessionDeps {
        app_config: deps.app_config,
        open_folder_action: deps.open_folder_action,
        sort_key: deps.sort_key,
        search_text: deps.search_text,
        sort_dropdown: deps.chrome.sort_dropdown,
        search_entry: deps.chrome.search_entry,
        filter: deps.filter,
        window: deps.window,
        outer_paned: deps.outer_paned,
        inner_paned: deps.inner_paned,
        meta_paned: deps.meta_paned,
        outer_position_programmatic: deps.outer_position_programmatic,
        inner_position_programmatic: deps.inner_position_programmatic,
        meta_position_programmatic: deps.meta_position_programmatic,
        pane_restore_complete: deps.pane_restore_complete,
        min_left_pane_px: deps.min_left_pane_px,
        min_right_pane_px: deps.min_right_pane_px,
        min_center_pane_px: deps.min_center_pane_px,
        min_meta_split_px: deps.min_meta_split_px,
    });
}
