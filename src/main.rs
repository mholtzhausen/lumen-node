mod byte_format;
mod config;
mod db;
mod dialogs;
mod file_name_ops;
mod image_types;
mod json_tree;
mod metadata;
mod metadata_section;
mod metadata_view;
mod recent_folders;
mod scan;
mod scanner;
mod sort;
mod sort_flags;
mod thumbnails;
mod thumbnail_sizing;
mod timing_report;
mod tree_sidebar;
mod ui;
mod updater;
mod view_helpers;
mod window_math;

use metadata::ImageMetadata;
use byte_format::human_readable_bytes;
use scan::ScanMessage;
use recent_folders::push_recent_folder_entry;
use scanner::scan_directory;
use sort_flags::SortFields;
use sort::{
    normalize_sort_key, sort_index_for_key, SORT_KEY_NAME_ASC,
};
use thumbnail_sizing::{normalize_thumbnail_size, thumbnail_size_options};
use tree_sidebar::reset_tree_root;
use ui::actions::install_context_menu;
use ui::center::{build_center_content, CenterContentDeps};
use ui::controls::{
    install_clear_button_handler, install_search_entry_handler, install_sort_dropdown_handler,
    install_thumbnail_size_handlers,
};
use ui::grid::DEFER_GRID_THUMBNAILS_UNTIL_ENUM_COMPLETE;
use ui::keyboard::{install_keyboard_handler, install_scroll_navigation_handlers, KeyboardDeps};
use ui::models::{build_model_bundle, ModelAssemblyDeps};
use ui::navigation::{install_navigation_handlers, NavigationDeps};
use ui::open_folder::{build_open_folder_action, OpenFolderActionDeps};
use ui::scan_runtime::{install_scan_runtime, ScanRuntimeDeps};
use ui::selection::{handle_selection_change_event, ClickTrace};
use ui::session::{
    install_close_persistence_handler, restore_session_state, ClosePersistenceDeps,
    RestoreSessionDeps,
};
use ui::shell::{
    assemble_paned_layout, build_header_controls, create_progress_widgets,
    create_window_with_defaults, install_history_popover_handler, install_open_button_handler,
    mount_window_content,
};
use ui::sidebar::{
    append_meta_paned_to_sidebar, connect_meta_paned_dirty_tracking, connect_sidebar_visibility_toggles,
    create_meta_content_container, create_meta_expander, create_meta_paned, create_meta_position_programmatic,
    create_meta_preview_picture, create_meta_scroll_list, create_meta_split_before_auto_collapse,
    create_meta_split_dirty_flag, create_pane_restore_complete_flag, create_right_sidebar,
    initialize_meta_paned_position, populate_metadata_sidebar,
};
use ui::tree::build_tree_widgets;
use window_math::pct_to_px;

use std::{
    cell::Cell,
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering as AtomicOrdering},
    time::Instant,
};

use libadwaita as adw;
use adw::prelude::*;
use gtk4::{gio, glib, Image, Label, ProgressBar, StringObject, TreeListRow};

pub(crate) static CLICK_TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);
pub(crate) static PREVIEW_REQUEST_PENDING: AtomicU64 = AtomicU64::new(0);
pub(crate) static SUPPRESS_SIDEBAR_DURING_PREVIEW: AtomicU64 = AtomicU64::new(0);
pub(crate) static THUMB_UI_CALLBACKS_SKIPPED_WHILE_PREVIEW: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCAN_BUFFER_DEPTH: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCAN_DRAIN_SCHEDULED: AtomicU64 = AtomicU64::new(0);
pub(crate) const SCAN_DRAIN_BATCH_SIZE: u64 = 50;
const DEFAULT_WINDOW_WIDTH: i32 = 1280;
const DEFAULT_WINDOW_HEIGHT: i32 = 800;
const MIN_LEFT_PANE_PX: i32 = 120;
const MIN_RIGHT_PANE_PX: i32 = 180;
const MIN_CENTER_PANE_PX: i32 = 260;
pub(crate) const MIN_META_SPLIT_PX: i32 = 120;
const RECENT_FOLDERS_LIMIT: usize = 50;
const ENUM_PHASE_WEIGHT: f64 = 0.10;
const THUMB_PHASE_WEIGHT: f64 = 0.35;
const ENRICH_PHASE_WEIGHT: f64 = 0.55;

#[derive(Default)]
struct ScanProgressState {
    generation: u64,
    total_files: u32,
    enumerated_done: u32,
    thumbnails_ready_done: u32,
    enriched_done: u32,
    enriched_generated: u32,
    enriched_cached: u32,
    folder_image_count: u32,
    folder_total_size_bytes: u64,
    current_folder_path: String,
    visible: bool,
}

impl ScanProgressState {
    fn start_pending(&mut self, generation: u64) {
        self.generation = generation;
        self.total_files = 0;
        self.enumerated_done = 0;
        self.thumbnails_ready_done = 0;
        self.enriched_done = 0;
        self.enriched_generated = 0;
        self.enriched_cached = 0;
        self.visible = true;
    }

    fn begin_with_total(&mut self, generation: u64, total_files: u32) {
        self.start_pending(generation);
        self.total_files = total_files;
    }

    fn total_or_one(&self) -> u32 {
        self.total_files.max(1)
    }

    fn overall_fraction(&self) -> f64 {
        if self.total_files == 0 {
            return 0.0;
        }
        let total = self.total_or_one() as f64;
        let enum_fraction = (self.enumerated_done as f64 / total).min(1.0);
        let thumb_fraction = (self.thumbnails_ready_done as f64 / total).min(1.0);
        let enrich_fraction = (self.enriched_done as f64 / total).min(1.0);
        (ENUM_PHASE_WEIGHT * enum_fraction
            + THUMB_PHASE_WEIGHT * thumb_fraction
            + ENRICH_PHASE_WEIGHT * enrich_fraction)
            .min(1.0)
    }

    fn status_text(&self) -> String {
        if !self.visible {
            let location_text = if self.current_folder_path.is_empty() {
                "Folder location unknown".to_string()
            } else {
                format!("Location {}", self.current_folder_path)
            };
            return format!(
                "Images {} | Folder size {} | {}",
                self.folder_image_count,
                human_readable_bytes(self.folder_total_size_bytes),
                location_text
            );
        }
        format!(
            "Enum {}/{} | Thumbs {}/{} | Index {}/{} (gen {}, cached {})",
            self.enumerated_done,
            self.total_files,
            self.thumbnails_ready_done,
            self.total_files,
            self.enriched_done,
            self.total_files,
            self.enriched_generated,
            self.enriched_cached
        )
    }
}

pub(crate) fn sync_progress_widgets(
    state: &ScanProgressState,
    progress_box: &gtk4::Box,
    progress_label: &Label,
    progress_bar: &ProgressBar,
) {
    progress_box.set_visible(true);
    progress_label.set_text(&state.status_text());
    progress_bar.set_visible(state.visible);

    if !state.visible {
        return;
    }

    let fraction = state.overall_fraction();
    if state.total_files == 0 {
        progress_bar.set_fraction(0.0);
        progress_bar.set_text(Some("--%"));
        return;
    }

    progress_bar.set_fraction(fraction);
    progress_bar.set_text(Some(&format!("{:.0}%", fraction * 100.0)));
}


// ---------------------------------------------------------------------------
// UI construction
// ---------------------------------------------------------------------------

fn build_ui(app: &adw::Application) {
    let app_config = config::load();
    let window = create_window_with_defaults(
        app,
        &app_config,
        DEFAULT_WINDOW_WIDTH,
        DEFAULT_WINDOW_HEIGHT,
        MIN_LEFT_PANE_PX,
        MIN_CENTER_PANE_PX,
        MIN_RIGHT_PANE_PX,
        MIN_META_SPLIT_PX,
    );

    // Load persisted config (last folder).
    let initial_recent_folders = app_config.recent_folders.clone();
    let configured_right_pane_width_pct = app_config.right_pane_width_pct;
    let configured_right_pane_pos = app_config.right_pane_pos;
    let configured_left_pane_width_pct = app_config.left_pane_width_pct;
    let configured_left_pane_pos = app_config.left_pane_pos;
    let configured_meta_pane_height_pct = app_config.meta_pane_height_pct;
    let configured_meta_pane_pos = app_config.meta_pane_pos;

    // Tracks the most recently scanned folder for config persistence.
    let current_folder: Rc<RefCell<Option<std::path::PathBuf>>> =
        Rc::new(RefCell::new(None));
    let recent_folders: Rc<RefCell<Vec<std::path::PathBuf>>> =
        Rc::new(RefCell::new(initial_recent_folders));
    {
        let mut history = recent_folders.borrow_mut();
        let mut sanitized = Vec::new();
        for folder in history.iter() {
            if folder.is_dir() && !sanitized.iter().any(|entry| entry == folder) {
                sanitized.push(folder.clone());
            }
        }
        *history = sanitized;
        history.truncate(RECENT_FOLDERS_LIMIT);
    }

    // Shared model: each item holds the absolute path of one image.
    let list_store = gio::ListStore::new::<StringObject>();

    // Async channel: background scan thread → GTK main thread.
    // Bounded to provide backpressure when the UI can't keep up.
    let (sender, receiver) = async_channel::bounded::<ScanMessage>(200);

    // ViewStack — toggled programmatically (no visible tab switcher).
    let view_stack = adw::ViewStack::new();

    // Reusable multi-phase progress indicator shown while scanning/indexing.
    let (progress_box, progress_label, progress_bar) = create_progress_widgets();
    let progress_state: Rc<RefCell<ScanProgressState>> =
        Rc::new(RefCell::new(ScanProgressState::default()));

    // Toast overlay wraps all main content for non-intrusive notifications.
    let toast_overlay = adw::ToastOverlay::new();

    // Hash cache: path → content hash (for hash-based thumbnail lookup).
    let hash_cache: Rc<RefCell<HashMap<String, String>>> =
        Rc::new(RefCell::new(HashMap::new()));

    // Metadata cache: path → extracted metadata (for search filtering).
    let meta_cache: Rc<RefCell<HashMap<String, ImageMetadata>>> =
        Rc::new(RefCell::new(HashMap::new()));

    // Sort cache: path -> precomputed fields used by UI comparator.
    let sort_fields_cache: Rc<RefCell<HashMap<String, SortFields>>> =
        Rc::new(RefCell::new(HashMap::new()));

    // Generation guards stale scan messages after restarts/superseding scans.
    let active_scan_generation = Rc::new(Cell::new(0_u64));
    let scan_in_progress = Rc::new(Cell::new(false));

    // Sort key: name/date/size ascending or descending.
    let sort_key: Rc<RefCell<String>> = Rc::new(RefCell::new(
        app_config
            .sort_key
            .as_deref()
            .map(normalize_sort_key)
            .unwrap_or(SORT_KEY_NAME_ASC)
            .to_string(),
    ));

    // Search text.
    let search_text: Rc<RefCell<String>> = Rc::new(RefCell::new(
        app_config.search_text.clone().unwrap_or_default(),
    ));

    let initial_thumbnail_size =
        normalize_thumbnail_size(app_config.thumbnail_size.unwrap_or(thumbnails::THUMB_NORMAL_SIZE));
    let thumbnail_size: Rc<RefCell<i32>> = Rc::new(RefCell::new(initial_thumbnail_size));
    let realized_thumb_images: Rc<RefCell<Vec<glib::WeakRef<Image>>>> =
        Rc::new(RefCell::new(Vec::new()));
    let realized_cell_boxes: Rc<RefCell<Vec<glib::WeakRef<gtk4::Box>>>> =
        Rc::new(RefCell::new(Vec::new()));

    let fast_scroll_active: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let scroll_last_pos: Rc<Cell<f64>> = Rc::new(Cell::new(0.0));
    let scroll_last_time: Rc<Cell<Option<Instant>>> = Rc::new(Cell::new(None));
    let scroll_debounce_gen: Rc<Cell<u64>> = Rc::new(Cell::new(0));

    install_scan_runtime(ScanRuntimeDeps {
        receiver,
        list_store: list_store.clone(),
        toast_overlay: toast_overlay.clone(),
        meta_cache: meta_cache.clone(),
        hash_cache: hash_cache.clone(),
        sort_fields_cache: sort_fields_cache.clone(),
        active_scan_generation: active_scan_generation.clone(),
        scan_in_progress: scan_in_progress.clone(),
        thumbnail_size: thumbnail_size.clone(),
        realized_thumb_images: realized_thumb_images.clone(),
        progress_state: progress_state.clone(),
        progress_box: progress_box.clone(),
        progress_label: progress_label.clone(),
        progress_bar: progress_bar.clone(),
    });

    // -----------------------------------------------------------------------
    // AdwHeaderBar — window chrome
    // -----------------------------------------------------------------------
    let header_controls = build_header_controls(&app_config, initial_thumbnail_size);
    let header_bar = header_controls.header_bar;
    let sort_dropdown = header_controls.sort_dropdown;
    let size_buttons = header_controls.size_buttons;
    let search_entry = header_controls.search_entry;
    let clear_btn = header_controls.clear_btn;
    let left_toggle = header_controls.left_toggle;
    let right_toggle = header_controls.right_toggle;
    let open_btn = header_controls.open_btn;
    let history_list = header_controls.history_list;
    let history_popover = header_controls.history_popover;
    let initial_left_sidebar_visible = header_controls.initial_left_sidebar_visible;
    let initial_right_sidebar_visible = header_controls.initial_right_sidebar_visible;

    // -----------------------------------------------------------------------
    // Three-pane layout: [left sidebar] | [center] | [right sidebar]
    // -----------------------------------------------------------------------
    // --- Left sidebar: file system tree ---
    let tree_widgets = build_tree_widgets(
        app_config.last_folder.as_ref(),
        initial_left_sidebar_visible,
    );
    let left_sidebar = tree_widgets.left_sidebar;
    let tree_root = tree_widgets.tree_root;
    let tree_model = tree_widgets.tree_model;
    let tree_selection = tree_widgets.tree_selection;
    let tree_list_view = tree_widgets.tree_list_view;

    let start_scan_for_folder = {
        let list_store = list_store.clone();
        let sender = sender.clone();
        let hash_cache = hash_cache.clone();
        let meta_cache = meta_cache.clone();
        let sort_fields_cache = sort_fields_cache.clone();
        let sort_key = sort_key.clone();
        let active_scan_generation = active_scan_generation.clone();
        let scan_in_progress = scan_in_progress.clone();
        let progress_state = progress_state.clone();
        let progress_box = progress_box.clone();
        let progress_label = progress_label.clone();
        let progress_bar = progress_bar.clone();
        Rc::new(move |folder: std::path::PathBuf| {
            let generation = active_scan_generation
                .get()
                .saturating_add(1);
            active_scan_generation.set(generation);
            scan_in_progress.set(true);

            list_store.remove_all();
            hash_cache.borrow_mut().clear();
            meta_cache.borrow_mut().clear();
            sort_fields_cache.borrow_mut().clear();
            {
                let mut progress = progress_state.borrow_mut();
                progress.start_pending(generation);
                sync_progress_widgets(&progress, &progress_box, &progress_label, &progress_bar);
            }
            DEFER_GRID_THUMBNAILS_UNTIL_ENUM_COMPLETE.store(1, AtomicOrdering::Relaxed);
            scan_directory(
                folder,
                sender.clone(),
                sort_key.borrow().clone(),
                generation,
            );
        })
    };

    // Wire tree folder selection → clear grid, start scan.
    let current_folder_tree = current_folder.clone();
    let start_scan_tree = start_scan_for_folder.clone();
    let recent_folders_tree = recent_folders.clone();
    let tree_root_tree = tree_root.clone();
    let sort_key_tree = sort_key.clone();
    let search_text_tree = search_text.clone();
    let thumbnail_size_tree = thumbnail_size.clone();
    let sort_dropdown_tree = sort_dropdown.clone();
    let search_entry_tree = search_entry.clone();
    let size_buttons_tree = size_buttons.clone();
    let progress_state_tree = progress_state.clone();
    tree_selection.connect_selection_changed(move |model, _, _| {
        let Some(row) = model.selected_item().and_downcast::<TreeListRow>() else {
            return;
        };
        let Some(file) = row.item().and_downcast::<gio::File>() else {
            return;
        };
        let Some(path) = file.path() else { return };
        // Skip if this folder is already loaded (e.g. during startup restore).
        if current_folder_tree.borrow().as_deref() == Some(path.as_path()) {
            return;
        }

        if let Some(saved_ui_state) = db::load_ui_state(path.as_path()) {
            let selected_sort = sort_index_for_key(&saved_ui_state.sort_key);
            *sort_key_tree.borrow_mut() = saved_ui_state.sort_key;
            *search_text_tree.borrow_mut() = saved_ui_state.search_text.clone();
            *thumbnail_size_tree.borrow_mut() = normalize_thumbnail_size(saved_ui_state.thumbnail_size);

            if sort_dropdown_tree.selected() != selected_sort {
                sort_dropdown_tree.set_selected(selected_sort);
            }
            search_entry_tree.set_text(&saved_ui_state.search_text);
            for (i, btn) in size_buttons_tree.iter().enumerate() {
                btn.set_active(thumbnail_size_options()[i] == *thumbnail_size_tree.borrow());
            }
        } else {
            let seeded_state = db::UiState {
                sort_key: sort_key_tree.borrow().clone(),
                search_text: search_text_tree.borrow().clone(),
                thumbnail_size: *thumbnail_size_tree.borrow(),
            };
            let _ = db::save_ui_state(path.as_path(), &seeded_state);
        }

        *current_folder_tree.borrow_mut() = Some(path.clone());
        progress_state_tree.borrow_mut().current_folder_path = path.display().to_string();
        {
            let mut history = recent_folders_tree.borrow_mut();
            push_recent_folder_entry(&mut history, path.as_path(), RECENT_FOLDERS_LIMIT);
            config::save_recent_state(Some(path.as_path()), &history);
        }
        reset_tree_root(&tree_root_tree, path.as_path());
        start_scan_tree(path);
    });

    let model_bundle = build_model_bundle(ModelAssemblyDeps {
        list_store: list_store.clone(),
        meta_cache: meta_cache.clone(),
        search_text: search_text.clone(),
        sort_key: sort_key.clone(),
        sort_fields_cache: sort_fields_cache.clone(),
    });
    let filter = model_bundle.filter;
    let sorter = model_bundle.sorter;
    let _filter_model = model_bundle.filter_model;
    let _sort_model = model_bundle.sort_model;
    let selection_model = model_bundle.selection_model;

    let center_content = build_center_content(CenterContentDeps {
        view_stack: view_stack.clone(),
        selection_model: selection_model.clone(),
        thumbnail_size: thumbnail_size.clone(),
        realized_cell_boxes: realized_cell_boxes.clone(),
        realized_thumb_images: realized_thumb_images.clone(),
        fast_scroll_active: fast_scroll_active.clone(),
        scroll_last_pos: scroll_last_pos.clone(),
        scroll_last_time: scroll_last_time.clone(),
        scroll_debounce_gen: scroll_debounce_gen.clone(),
        hash_cache: hash_cache.clone(),
        sort_key: sort_key.clone(),
        sort_fields_cache: sort_fields_cache.clone(),
        window: window.clone(),
        toast_overlay: toast_overlay.clone(),
        start_scan_for_folder: start_scan_for_folder.clone(),
        current_folder: current_folder.clone(),
    });
    let center_box = center_content.center_box;
    let grid_view = center_content.grid_view;
    let grid_scroll = center_content.grid_scroll;
    let single_picture = center_content.single_picture;

    // --- Right sidebar: preview (top) + metadata list (bottom) ---
    let right_sidebar = create_right_sidebar(initial_right_sidebar_visible);

    let startup_window_width = DEFAULT_WINDOW_WIDTH;
    let startup_window_height = DEFAULT_WINDOW_HEIGHT;
    let left_pane_start_px = app_config
        .left_pane_width_pct
        .map(|pct| pct_to_px(startup_window_width, pct))
        .or(app_config.left_pane_pos)
        .unwrap_or(220)
        .clamp(
            MIN_LEFT_PANE_PX,
            startup_window_width - MIN_CENTER_PANE_PX - MIN_RIGHT_PANE_PX,
        );
    let right_pane_width_px = app_config
        .right_pane_width_pct
        .map(|pct| pct_to_px(startup_window_width, pct))
        .or_else(|| {
            app_config.right_pane_pos.map(|inner_pos| {
                startup_window_width.saturating_sub(left_pane_start_px + inner_pos)
            })
        })
        .unwrap_or(260);
    let max_right_pane_width_px = startup_window_width
        .saturating_sub(left_pane_start_px + MIN_CENTER_PANE_PX)
        .max(MIN_RIGHT_PANE_PX);
    let right_pane_width_px = right_pane_width_px.clamp(MIN_RIGHT_PANE_PX, max_right_pane_width_px);
    let inner_pane_start_px = startup_window_width
        .saturating_sub(left_pane_start_px + right_pane_width_px)
        .max(MIN_CENTER_PANE_PX);
    let meta_pane_start_px = app_config
        .meta_pane_height_pct
        .map(|pct| pct_to_px(startup_window_height, pct))
        .or(app_config.meta_pane_pos)
        .unwrap_or(200)
        .clamp(MIN_META_SPLIT_PX, startup_window_height - MIN_META_SPLIT_PX);

    // Top pane: image preview
    let meta_preview = create_meta_preview_picture();

    // Bottom pane: metadata list
    let meta_content = create_meta_content_container();
    let (meta_scroll, meta_listbox) = create_meta_scroll_list();
    let meta_expander = create_meta_expander(&meta_scroll);
    meta_content.append(&meta_expander);
    let meta_split_before_auto_collapse = create_meta_split_before_auto_collapse();

    // Vertical paned: preview (top) | metadata (bottom)
    let meta_paned = create_meta_paned(&meta_preview, &meta_content);
    let meta_position_programmatic = create_meta_position_programmatic();
    let meta_split_dirty = create_meta_split_dirty_flag();
    let pane_restore_complete = create_pane_restore_complete_flag();
    initialize_meta_paned_position(&meta_paned, &meta_position_programmatic, meta_pane_start_px);
    connect_meta_paned_dirty_tracking(
        &meta_paned,
        &meta_position_programmatic,
        &meta_split_dirty,
        &pane_restore_complete,
    );
    append_meta_paned_to_sidebar(&right_sidebar, &meta_paned);

    let refresh_metadata_sidebar_for_actions: Rc<dyn Fn(&ImageMetadata)> = Rc::new({
        let meta_listbox = meta_listbox.clone();
        move |meta: &ImageMetadata| populate_metadata_sidebar(&meta_listbox, meta)
    });
    let start_scan_for_folder_actions: Rc<dyn Fn(std::path::PathBuf)> =
        start_scan_for_folder.clone();
    install_context_menu(
        &window,
        &toast_overlay,
        &selection_model,
        &meta_cache,
        &hash_cache,
        &thumbnail_size,
        &meta_expander,
        &meta_paned,
        &meta_split_before_auto_collapse,
        &meta_position_programmatic,
        MIN_META_SPLIT_PX,
        &current_folder,
        &start_scan_for_folder_actions,
        &list_store,
        &refresh_metadata_sidebar_for_actions,
        &grid_view,
        &single_picture,
        &meta_preview,
    );

    // -----------------------------------------------------------------------
    // Wire: sidebar toggle buttons → show/hide panels
    // -----------------------------------------------------------------------
    connect_sidebar_visibility_toggles(&left_toggle, &left_sidebar, &right_toggle, &right_sidebar);

    let pre_fullview_left: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let pre_fullview_right: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    install_navigation_handlers(NavigationDeps {
        grid_view: grid_view.clone(),
        view_stack: view_stack.clone(),
        single_picture: single_picture.clone(),
        selection_model: selection_model.clone(),
        left_toggle: left_toggle.clone(),
        right_toggle: right_toggle.clone(),
        pre_fullview_left: pre_fullview_left.clone(),
        pre_fullview_right: pre_fullview_right.clone(),
        meta_preview: meta_preview.clone(),
    });

    // -----------------------------------------------------------------------
    // Wire: selection change → populate metadata sidebar
    // -----------------------------------------------------------------------
    let meta_listbox_sel = meta_listbox.clone();
    let meta_expander_sel = meta_expander.clone();
    let meta_paned_sel = meta_paned.clone();
    let meta_split_before_auto_collapse_sel = meta_split_before_auto_collapse.clone();
    let meta_position_programmatic_sel = meta_position_programmatic.clone();
    let meta_preview_sel = meta_preview.clone();
    let meta_cache_sel = meta_cache.clone();
    let realized_thumb_images_sel = realized_thumb_images.clone();
    let thumbnail_size_sel = thumbnail_size.clone();
    let hash_cache_sel = hash_cache.clone();
    let click_trace_state: Rc<RefCell<Option<ClickTrace>>> = Rc::new(RefCell::new(None));
    let click_trace_state_sel = click_trace_state.clone();
    selection_model.connect_selection_changed(move |model, _, _| {
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

    // -----------------------------------------------------------------------
    // Wire: open_btn → FileDialog → start scan (quick-jump shortcut)
    // -----------------------------------------------------------------------
    let start_scan_for_folder_open_action: Rc<dyn Fn(std::path::PathBuf)> = start_scan_for_folder.clone();
    let open_folder_action = build_open_folder_action(OpenFolderActionDeps {
        current_folder: current_folder.clone(),
        start_scan_for_folder: start_scan_for_folder_open_action,
        tree_root: tree_root.clone(),
        tree_model: tree_model.clone(),
        tree_list_view: tree_list_view.clone(),
        recent_folders: recent_folders.clone(),
        sort_key: sort_key.clone(),
        search_text: search_text.clone(),
        thumbnail_size: thumbnail_size.clone(),
        sort_dropdown: sort_dropdown.clone(),
        search_entry: search_entry.clone(),
        filter: filter.clone(),
        sorter: sorter.clone(),
        size_buttons: size_buttons.clone(),
        progress_state: progress_state.clone(),
        recent_folders_limit: RECENT_FOLDERS_LIMIT,
    });

    install_history_popover_handler(
        &history_popover,
        &history_list,
        &recent_folders,
        &current_folder,
        open_folder_action.clone(),
        RECENT_FOLDERS_LIMIT,
    );

    install_open_button_handler(
        &open_btn,
        &window,
        &current_folder,
        open_folder_action.clone(),
    );

    // -----------------------------------------------------------------------
    // Wire: sort/search/clear/thumbnail-size controls
    // -----------------------------------------------------------------------
    let start_scan_for_folder_controls: Rc<dyn Fn(std::path::PathBuf)> = start_scan_for_folder.clone();
    install_sort_dropdown_handler(
        &sort_dropdown,
        &sort_key,
        &sorter,
        &current_folder,
        &scan_in_progress,
        &start_scan_for_folder_controls,
    );
    install_search_entry_handler(&search_entry, &search_text, &filter, &current_folder);
    install_clear_button_handler(
        &clear_btn,
        &search_text,
        &sort_key,
        &filter,
        &sorter,
        &search_entry,
        &sort_dropdown,
        &thumbnail_size,
        &current_folder,
    );
    install_thumbnail_size_handlers(
        &size_buttons,
        thumbnail_size_options(),
        &thumbnail_size,
        &grid_view,
        &realized_thumb_images,
        &realized_cell_boxes,
        &hash_cache,
        &current_folder,
    );

    // -----------------------------------------------------------------------
    // Assemble three-pane layout with resizable Paned dividers
    // -----------------------------------------------------------------------
    let paned_layout = assemble_paned_layout(
        &left_sidebar,
        &center_box,
        &right_sidebar,
        &pane_restore_complete,
        left_pane_start_px,
        inner_pane_start_px,
    );
    let inner_paned = paned_layout.inner_paned.clone();
    let outer_paned = paned_layout.outer_paned.clone();
    let inner_position_programmatic = paned_layout.inner_position_programmatic.clone();
    let inner_split_dirty = paned_layout.inner_split_dirty.clone();
    let outer_position_programmatic = paned_layout.outer_position_programmatic.clone();
    let outer_split_dirty = paned_layout.outer_split_dirty.clone();

    let update_banner = mount_window_content(
        &window,
        &header_bar,
        &toast_overlay,
        &outer_paned,
        &progress_box,
    );

    // Check for updates in a background thread; show banner if a newer release exists.
    let (update_tx, update_rx) = async_channel::bounded::<updater::UpdateInfo>(1);
    std::thread::spawn(move || {
        if let Some(info) = updater::check_for_update() {
            let _ = update_tx.send_blocking(info);
        }
    });
    glib::MainContext::default().spawn_local(async move {
        if let Ok(info) = update_rx.recv().await {
            update_banner.set_title(&format!("Version {} available", info.version));
            update_banner.set_revealed(true);
            update_banner.connect_button_clicked(move |_| {
                let _ = gio::AppInfo::launch_default_for_uri(
                    &info.url,
                    None::<&gio::AppLaunchContext>,
                );
            });
        }
    });

    // -----------------------------------------------------------------------
    // Keyboard: Escape / Left / Right / Page navigation
    // -----------------------------------------------------------------------
    let start_scan_for_folder_keys: Rc<dyn Fn(std::path::PathBuf)> = start_scan_for_folder.clone();
    install_keyboard_handler(KeyboardDeps {
        window: window.clone(),
        view_stack: view_stack.clone(),
        selection_model: selection_model.clone(),
        single_picture: single_picture.clone(),
        grid_view: grid_view.clone(),
        grid_scroll: grid_scroll.clone(),
        thumbnail_size: thumbnail_size.clone(),
        toast_overlay: toast_overlay.clone(),
        current_folder: current_folder.clone(),
        start_scan_for_folder: start_scan_for_folder_keys,
        left_toggle: left_toggle.clone(),
        right_toggle: right_toggle.clone(),
        pre_fullview_left: pre_fullview_left.clone(),
        pre_fullview_right: pre_fullview_right.clone(),
    });

    // -----------------------------------------------------------------------
    // Scroll on single-view / meta-preview → navigate images
    // Accumulate delta so smooth-scroll trackpads don't flood set_selected.
    // -----------------------------------------------------------------------
    install_scroll_navigation_handlers(&selection_model, &single_picture, &meta_preview);

    // -----------------------------------------------------------------------
    // Save config on close + restore session state
    // -----------------------------------------------------------------------
    install_close_persistence_handler(ClosePersistenceDeps {
        current_folder: current_folder.clone(),
        outer_paned: outer_paned.clone(),
        inner_paned: inner_paned.clone(),
        meta_paned: meta_paned.clone(),
        meta_split_before_auto_collapse: meta_split_before_auto_collapse.clone(),
        sort_key: sort_key.clone(),
        search_text: search_text.clone(),
        thumbnail_size: thumbnail_size.clone(),
        recent_folders: recent_folders.clone(),
        left_toggle: left_toggle.clone(),
        right_toggle: right_toggle.clone(),
        window: window.clone(),
        outer_split_dirty: outer_split_dirty.clone(),
        inner_split_dirty: inner_split_dirty.clone(),
        meta_split_dirty: meta_split_dirty.clone(),
        configured_left_pane_pos,
        configured_right_pane_pos,
        configured_meta_pane_pos,
        configured_left_pane_width_pct,
        configured_right_pane_width_pct,
        configured_meta_pane_height_pct,
        min_meta_split_px: MIN_META_SPLIT_PX,
    });

    let open_folder_action_restore: Rc<dyn Fn(std::path::PathBuf, bool)> = open_folder_action.clone();
    restore_session_state(RestoreSessionDeps {
        app_config,
        open_folder_action: open_folder_action_restore,
        sort_key: sort_key.clone(),
        search_text: search_text.clone(),
        sort_dropdown: sort_dropdown.clone(),
        search_entry: search_entry.clone(),
        filter: filter.clone(),
        window: window.clone(),
        outer_paned: outer_paned.clone(),
        inner_paned: inner_paned.clone(),
        meta_paned: meta_paned.clone(),
        outer_position_programmatic: outer_position_programmatic.clone(),
        inner_position_programmatic: inner_position_programmatic.clone(),
        meta_position_programmatic: meta_position_programmatic.clone(),
        pane_restore_complete: pane_restore_complete.clone(),
        min_left_pane_px: MIN_LEFT_PANE_PX,
        min_right_pane_px: MIN_RIGHT_PANE_PX,
        min_center_pane_px: MIN_CENTER_PANE_PX,
        min_meta_split_px: MIN_META_SPLIT_PX,
    });
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id("com.lumennode.app")
        .build();
    app.connect_activate(build_ui);
    app.run()
}
