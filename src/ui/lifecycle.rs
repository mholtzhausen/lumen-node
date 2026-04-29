use crate::config::AppConfig;
use crate::core::app_state::AppState;
use crate::db;
use crate::image_types::is_supported_image_path;
use crate::sort_flags::compute_sort_fields;
use crate::services::update_checker::install_update_checker;
use crate::ui::center::CenterContentBundle;
use crate::ui::keyboard::{
    install_keyboard_handler, install_scroll_navigation_handlers, KeyboardDeps,
};
use crate::ui::left_chrome_wiring::LeftChromeWiring;
use crate::ui::list_mutation::ListMutationContext;
use crate::ui::right_sidebar::RightSidebarBundle;
use crate::ui::session::{
    install_close_persistence_handler, restore_session_state, ClosePersistenceDeps,
    RestoreSessionDeps,
};
use gtk4::glib;
use gtk4::prelude::{CastNone, ListModelExt, SorterExt};
use libadwaita as adw;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    time::Duration,
};

pub(crate) struct LifecycleDeps {
    pub(crate) update_banner: adw::Banner,
    pub(crate) app_state: AppState,
    pub(crate) window: adw::ApplicationWindow,
    pub(crate) center: CenterContentBundle,
    pub(crate) right: RightSidebarBundle,
    pub(crate) selection_model: gtk4::SingleSelection,
    pub(crate) thumbnail_size: Rc<RefCell<i32>>,
    pub(crate) toast_overlay: adw::ToastOverlay,
    pub(crate) current_folder: Rc<RefCell<Option<PathBuf>>>,
    pub(crate) start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    pub(crate) chrome: LeftChromeWiring,
    pub(crate) pre_fullview_left: Rc<Cell<bool>>,
    pub(crate) pre_fullview_right: Rc<Cell<bool>>,
    pub(crate) outer_paned: gtk4::Paned,
    pub(crate) inner_paned: gtk4::Paned,
    pub(crate) sort_key: Rc<RefCell<String>>,
    pub(crate) search_text: Rc<RefCell<String>>,
    pub(crate) recent_folders: Rc<RefCell<Vec<PathBuf>>>,
    pub(crate) outer_split_dirty: Rc<Cell<bool>>,
    pub(crate) inner_split_dirty: Rc<Cell<bool>>,
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
    pub(crate) min_left_pane_px: i32,
    pub(crate) min_right_pane_px: i32,
    pub(crate) min_center_pane_px: i32,
    pub(crate) sorter: gtk4::CustomSorter,
}

pub(crate) fn install_lifecycle(deps: LifecycleDeps) {
    install_update_checker(deps.update_banner.clone());

    install_keyboard_handler(KeyboardDeps {
        app_state: deps.app_state.clone(),
        window: deps.window.clone(),
        center: deps.center.clone(),
        selection_model: deps.selection_model.clone(),
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
        &deps.center.single_picture,
        &deps.right.meta_preview,
    );

    install_close_persistence_handler(ClosePersistenceDeps {
        current_folder: deps.current_folder.clone(),
        outer_paned: deps.outer_paned.clone(),
        inner_paned: deps.inner_paned.clone(),
        meta_paned: deps.right.meta_paned.clone(),
        meta_split_before_auto_collapse: deps.right.meta_split_before_auto_collapse.clone(),
        sort_key: deps.sort_key.clone(),
        search_text: deps.search_text.clone(),
        thumbnail_size: deps.thumbnail_size.clone(),
        recent_folders: deps.recent_folders.clone(),
        left_toggle: deps.chrome.left_toggle.clone(),
        right_toggle: deps.chrome.right_toggle.clone(),
        window: deps.window.clone(),
        outer_split_dirty: deps.outer_split_dirty.clone(),
        inner_split_dirty: deps.inner_split_dirty.clone(),
        meta_split_dirty: deps.right.meta_split_dirty.clone(),
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
        meta_paned: deps.right.meta_paned,
        outer_position_programmatic: deps.outer_position_programmatic,
        inner_position_programmatic: deps.inner_position_programmatic,
        meta_position_programmatic: deps.right.meta_position_programmatic,
        pane_restore_complete: deps.right.pane_restore_complete,
        min_left_pane_px: deps.min_left_pane_px,
        min_right_pane_px: deps.min_right_pane_px,
        min_center_pane_px: deps.min_center_pane_px,
        min_meta_split_px: deps.min_meta_split_px,
    });

    install_folder_delta_watcher(
        deps.app_state,
        deps.selection_model,
        deps.start_scan_for_folder,
        deps.sorter,
    );
}

fn install_folder_delta_watcher(
    app_state: AppState,
    selection_model: gtk4::SingleSelection,
    start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    sorter: gtk4::CustomSorter,
) {
    #[derive(Clone)]
    struct FolderSnapshot {
        folder: PathBuf,
        files: HashMap<String, (i64, i64)>,
    }

    let snapshot: Rc<RefCell<Option<FolderSnapshot>>> = Rc::new(RefCell::new(None));
    let mutation_ctx = ListMutationContext {
        app_state: app_state.clone(),
        selection_model,
        start_scan_for_folder,
    };
    let snapshot_tick = snapshot.clone();
    glib::timeout_add_local(Duration::from_millis(1200), move || {
        if app_state.scan_in_progress.get() {
            return glib::ControlFlow::Continue;
        }
        let Some(folder) = app_state.current_folder.borrow().as_ref().cloned() else {
            *snapshot_tick.borrow_mut() = None;
            return glib::ControlFlow::Continue;
        };

        let next_files = collect_folder_image_state(&folder);
        let Some(next_files) = next_files else {
            return glib::ControlFlow::Continue;
        };

        let mut snapshot_guard = snapshot_tick.borrow_mut();
        let Some(prev) = snapshot_guard.as_ref() else {
            *snapshot_guard = Some(FolderSnapshot {
                folder,
                files: next_files,
            });
            return glib::ControlFlow::Continue;
        };
        if prev.folder != folder {
            *snapshot_guard = Some(FolderSnapshot {
                folder,
                files: next_files,
            });
            return glib::ControlFlow::Continue;
        }

        let mut added: Vec<String> = Vec::new();
        let mut removed: Vec<String> = Vec::new();
        let mut changed: Vec<String> = Vec::new();

        for path in next_files.keys() {
            if !prev.files.contains_key(path) {
                added.push(path.clone());
            } else if prev.files.get(path) != next_files.get(path) {
                changed.push(path.clone());
            }
        }
        for path in prev.files.keys() {
            if !next_files.contains_key(path) {
                removed.push(path.clone());
            }
        }

        let total_changes = added.len() + removed.len() + changed.len();
        if total_changes == 0 {
            return glib::ControlFlow::Continue;
        }
        if total_changes > 25 {
            mutation_ctx.fallback_rescan();
            *snapshot_guard = Some(FolderSnapshot {
                folder,
                files: next_files,
            });
            return glib::ControlFlow::Continue;
        }

        let mut had_failure = false;
        for path in &removed {
            // Delete/trash actions may have already removed the row locally before
            // the watcher observes the filesystem delta. Treat "already absent"
            // as a successful no-op to avoid unnecessary fallback rescans.
            if list_store_contains_path(&app_state.list_store, path)
                && !mutation_ctx.remove_path(PathBuf::from(path).as_path())
            {
                had_failure = true;
            }
            app_state.sort_fields_cache.borrow_mut().remove(path);
            app_state.hash_cache.borrow_mut().remove(path);
            app_state.meta_cache.borrow_mut().remove(path);
        }
        for path in &added {
            if !mutation_ctx.insert_path(PathBuf::from(path).as_path(), false) {
                had_failure = true;
            }
            if let Ok(conn) = db::open(&folder) {
                if let Some(row) = db::ensure_indexed_with_outcome(&conn, PathBuf::from(path).as_path()) {
                    app_state
                        .hash_cache
                        .borrow_mut()
                        .insert(path.clone(), row.0.hash.clone());
                    app_state
                        .meta_cache
                        .borrow_mut()
                        .insert(path.clone(), row.0.meta.clone());
                }
            }
        }
        for path in &changed {
            app_state
                .sort_fields_cache
                .borrow_mut()
                .insert(path.clone(), compute_sort_fields(path));
            app_state.hash_cache.borrow_mut().remove(path);
            app_state.meta_cache.borrow_mut().remove(path);
            if let Ok(conn) = db::open(&folder) {
                if let Some(row) = db::refresh_indexed(&conn, PathBuf::from(path).as_path()) {
                    app_state
                        .hash_cache
                        .borrow_mut()
                        .insert(path.clone(), row.hash.clone());
                    app_state
                        .meta_cache
                        .borrow_mut()
                        .insert(path.clone(), row.meta.clone());
                }
            }
        }
        if had_failure {
            mutation_ctx.fallback_rescan();
        } else {
            sorter.changed(gtk4::SorterChange::Different);
        }
        *snapshot_guard = Some(FolderSnapshot {
            folder,
            files: next_files,
        });
        glib::ControlFlow::Continue
    });
}

fn collect_folder_image_state(folder: &std::path::Path) -> Option<HashMap<String, (i64, i64)>> {
    let read_dir = std::fs::read_dir(folder).ok()?;
    let mut files = HashMap::new();
    for entry in read_dir.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() || !is_supported_image_path(&path) {
            continue;
        }
        let Ok(meta) = path.metadata() else { continue };
        let modified = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        files.insert(path.to_string_lossy().to_string(), (modified, meta.len() as i64));
    }
    Some(files)
}

fn list_store_contains_path(list_store: &gtk4::gio::ListStore, path: &str) -> bool {
    for idx in 0..list_store.n_items() {
        let is_match = list_store
            .item(idx)
            .and_downcast::<gtk4::StringObject>()
            .map(|obj| obj.string().as_str() == path)
            .unwrap_or(false);
        if is_match {
            return true;
        }
    }
    false
}
