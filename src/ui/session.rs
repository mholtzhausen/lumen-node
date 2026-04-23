use crate::{config, db};
use crate::sort::sort_index_for_key;
use crate::window_math::{pct_to_px, px_to_pct};
use gtk4::prelude::*;
use gtk4::{glib, CustomFilter};
use libadwaita as adw;
use std::{
    cell::Cell,
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::Duration,
};

pub(crate) struct ClosePersistenceDeps {
    pub(crate) current_folder: Rc<RefCell<Option<PathBuf>>>,
    pub(crate) outer_paned: gtk4::Paned,
    pub(crate) inner_paned: gtk4::Paned,
    pub(crate) meta_paned: gtk4::Paned,
    pub(crate) meta_split_before_auto_collapse: Rc<Cell<Option<i32>>>,
    pub(crate) sort_key: Rc<RefCell<String>>,
    pub(crate) search_text: Rc<RefCell<String>>,
    pub(crate) thumbnail_size: Rc<RefCell<i32>>,
    pub(crate) recent_folders: Rc<RefCell<Vec<PathBuf>>>,
    pub(crate) left_toggle: gtk4::ToggleButton,
    pub(crate) right_toggle: gtk4::ToggleButton,
    pub(crate) window: adw::ApplicationWindow,
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
}

pub(crate) fn install_close_persistence_handler(deps: ClosePersistenceDeps) {
    let window = deps.window.clone();
    window.connect_close_request(move |_| {
        let window_width = deps.window.width().max(1);
        let window_height = deps.window.height().max(1);
        let window_maximized = deps.window.is_maximized();
        let left_pos = deps.outer_paned.position();
        let inner_pos = deps.inner_paned.position();
        let raw_meta_pos = deps
            .meta_split_before_auto_collapse
            .get()
            .unwrap_or_else(|| deps.meta_paned.position());
        let meta_total_height = deps.meta_paned.height().max(1);
        let meta_upper_bound = meta_total_height.saturating_sub(deps.min_meta_split_px);
        let meta_pos = if meta_upper_bound < deps.min_meta_split_px {
            // Window too short to preserve both minimum panes; persist midpoint.
            (meta_total_height / 2).max(1)
        } else {
            raw_meta_pos.clamp(deps.min_meta_split_px, meta_upper_bound)
        };
        let recent_folders = deps.recent_folders.borrow();
        let left_pos_for_save = if deps.outer_split_dirty.get() {
            left_pos
        } else {
            deps.configured_left_pane_pos.unwrap_or(left_pos)
        };
        let inner_pos_for_save = if deps.inner_split_dirty.get() {
            inner_pos
        } else {
            deps.configured_right_pane_pos.unwrap_or(inner_pos)
        };
        let right_width_for_save = window_width.saturating_sub(left_pos_for_save + inner_pos_for_save);
        let meta_pos_for_save = if deps.meta_split_dirty.get() {
            meta_pos
        } else {
            deps.configured_meta_pane_pos.unwrap_or(meta_pos)
        };
        let left_pct_for_save = if deps.outer_split_dirty.get() {
            px_to_pct(left_pos_for_save, window_width)
        } else {
            deps.configured_left_pane_width_pct
                .unwrap_or(px_to_pct(left_pos_for_save, window_width))
        };
        let right_pct_for_save = if deps.inner_split_dirty.get() {
            px_to_pct(right_width_for_save, window_width)
        } else {
            deps.configured_right_pane_width_pct
                .unwrap_or(px_to_pct(right_width_for_save, window_width))
        };
        let meta_pct_for_save = if deps.meta_split_dirty.get() {
            px_to_pct(meta_pos_for_save, meta_total_height)
        } else {
            deps.configured_meta_pane_height_pct
                .unwrap_or(px_to_pct(meta_pos_for_save, meta_total_height))
        };

        config::save(
            deps.current_folder.borrow().as_deref(),
            &recent_folders,
            window_width,
            window_height,
            window_maximized,
            left_pos_for_save,
            inner_pos_for_save,
            meta_pos_for_save,
            left_pct_for_save,
            right_pct_for_save,
            meta_pct_for_save,
            deps.left_toggle.is_active(),
            deps.right_toggle.is_active(),
        );
        if let Some(folder) = deps.current_folder.borrow().as_ref() {
            let _ = db::save_ui_state(
                folder.as_path(),
                &db::UiState {
                    sort_key: deps.sort_key.borrow().clone(),
                    search_text: deps.search_text.borrow().clone(),
                    thumbnail_size: *deps.thumbnail_size.borrow(),
                },
            );
        }
        glib::Propagation::Proceed
    });
}

pub(crate) struct RestoreSessionDeps {
    pub(crate) app_config: crate::config::AppConfig,
    pub(crate) open_folder_action: Rc<dyn Fn(PathBuf, bool)>,
    pub(crate) sort_key: Rc<RefCell<String>>,
    pub(crate) search_text: Rc<RefCell<String>>,
    pub(crate) sort_dropdown: gtk4::DropDown,
    pub(crate) search_entry: gtk4::SearchEntry,
    pub(crate) filter: CustomFilter,
    pub(crate) window: adw::ApplicationWindow,
    pub(crate) outer_paned: gtk4::Paned,
    pub(crate) inner_paned: gtk4::Paned,
    pub(crate) meta_paned: gtk4::Paned,
    pub(crate) outer_position_programmatic: Rc<Cell<u32>>,
    pub(crate) inner_position_programmatic: Rc<Cell<u32>>,
    pub(crate) meta_position_programmatic: Rc<Cell<u32>>,
    pub(crate) pane_restore_complete: Rc<Cell<bool>>,
    pub(crate) min_left_pane_px: i32,
    pub(crate) min_right_pane_px: i32,
    pub(crate) min_center_pane_px: i32,
    pub(crate) min_meta_split_px: i32,
}

pub(crate) fn restore_session_state(deps: RestoreSessionDeps) {
    if let Some(last_folder) = deps.app_config.last_folder.as_ref() {
        if last_folder.is_dir() {
            (deps.open_folder_action)(last_folder.clone(), true);
        }
    }

    let initial_sort_idx: u32 = sort_index_for_key(deps.sort_key.borrow().as_str());
    if initial_sort_idx != 0 {
        // fires connect_selected_notify → updates sort_key + calls sorter.changed()
        deps.sort_dropdown.set_selected(initial_sort_idx);
    }
    let initial_search = deps.search_text.borrow().clone();
    if !initial_search.is_empty() {
        deps.search_entry.set_text(&initial_search);
        deps.filter.changed(gtk4::FilterChange::Different);
    }

    if deps.app_config.window_maximized.unwrap_or(false) {
        deps.window.maximize();
    }

    deps.window.present();
    let window_for_pane_restore = deps.window.clone();
    let outer_paned_restore = deps.outer_paned.clone();
    let inner_paned_restore = deps.inner_paned.clone();
    let meta_paned_restore = deps.meta_paned.clone();
    let outer_position_programmatic_restore = deps.outer_position_programmatic.clone();
    let inner_position_programmatic_restore = deps.inner_position_programmatic.clone();
    let meta_position_programmatic_restore = deps.meta_position_programmatic.clone();
    let pane_restore_complete_restore = deps.pane_restore_complete.clone();
    let pane_restore_attempts = Rc::new(Cell::new(0_u8));
    let pane_restore_attempts_tick = pane_restore_attempts.clone();
    let configured_right_pane_width_pct = deps.app_config.right_pane_width_pct;
    let configured_right_pane_pos = deps.app_config.right_pane_pos;
    let configured_meta_pane_height_pct = deps.app_config.meta_pane_height_pct;
    let configured_meta_pane_pos = deps.app_config.meta_pane_pos;
    glib::timeout_add_local(Duration::from_millis(16), move || {
        let attempts = pane_restore_attempts_tick.get();
        if attempts >= 60 {
            pane_restore_complete_restore.set(true);
            return glib::ControlFlow::Break;
        }
        pane_restore_attempts_tick.set(attempts.saturating_add(1));

        let window_width = window_for_pane_restore.width();
        let inner_width = inner_paned_restore.width();
        if window_width <= 1 || inner_width <= 1 {
            return glib::ControlFlow::Continue;
        }

        let left_limit = (window_width - deps.min_center_pane_px - deps.min_right_pane_px)
            .max(deps.min_left_pane_px);
        let left_pos = outer_paned_restore
            .position()
            .clamp(deps.min_left_pane_px, left_limit);
        let max_right_pane_width_px = window_width
            .saturating_sub(left_pos + deps.min_center_pane_px)
            .max(deps.min_right_pane_px);
        let right_pane_width_px = configured_right_pane_width_pct
            .map(|pct| pct_to_px(window_width, pct))
            .or_else(|| {
                configured_right_pane_pos
                    .map(|inner_pos| window_width.saturating_sub(left_pos + inner_pos))
            })
            .unwrap_or(260)
            .clamp(deps.min_right_pane_px, max_right_pane_width_px);
        let inner_pane_start_px = window_width
            .saturating_sub(left_pos + right_pane_width_px)
            .max(deps.min_center_pane_px);
        inner_position_programmatic_restore
            .set(inner_position_programmatic_restore.get().saturating_add(1));
        inner_paned_restore.set_position(inner_pane_start_px);
        inner_position_programmatic_restore
            .set(inner_position_programmatic_restore.get().saturating_sub(1));
        let meta_total_height = meta_paned_restore.height().max(1);
        let configured_meta_pos = configured_meta_pane_height_pct
            .map(|pct| pct_to_px(meta_total_height, pct))
            .or(configured_meta_pane_pos)
            .unwrap_or(200);
        let meta_upper_bound = meta_total_height.saturating_sub(deps.min_meta_split_px);
        let meta_pane_start_px = if meta_upper_bound < deps.min_meta_split_px {
            // Window is too short to enforce both minimum split sizes.
            (meta_total_height / 2).max(1)
        } else {
            configured_meta_pos.clamp(deps.min_meta_split_px, meta_upper_bound)
        };
        outer_position_programmatic_restore
            .set(outer_position_programmatic_restore.get().saturating_add(1));
        outer_paned_restore.set_position(left_pos);
        outer_position_programmatic_restore
            .set(outer_position_programmatic_restore.get().saturating_sub(1));
        meta_position_programmatic_restore
            .set(meta_position_programmatic_restore.get().saturating_add(1));
        meta_paned_restore.set_position(meta_pane_start_px);
        meta_position_programmatic_restore
            .set(meta_position_programmatic_restore.get().saturating_sub(1));
        pane_restore_complete_restore.set(true);
        glib::ControlFlow::Break
    });
}
