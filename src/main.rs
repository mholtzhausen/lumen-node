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
use sort_flags::{compute_sort_fields, SortFields};
use sort::{
    normalize_sort_key, sort_index_for_key, SORT_KEY_DATE_DESC,
    SORT_KEY_NAME_ASC, SORT_KEY_NAME_DESC, SORT_KEY_SIZE_DESC,
};
use thumbnail_sizing::{normalize_thumbnail_size, thumbnail_size_options};
use tree_sidebar::reset_tree_root;
use ui::actions::install_context_menu;
use ui::controls::{
    install_clear_button_handler, install_search_entry_handler, install_sort_dropdown_handler,
    install_thumbnail_size_handlers,
};
use ui::grid::{
    add_scroll_flag_overlay, attach_grid_page, attach_single_page,
    bind_grid_list_item, build_scroll_flag_overlay, create_center_box, create_grid_overlay,
    create_grid_scroll, create_grid_view, create_single_picture,
    install_grid_scroll_speed_gate, make_delete_action, make_rename_action,
    refresh_realized_grid_thumbnails, set_default_grid_page, setup_grid_list_item,
    unbind_grid_list_item,
    DEFER_GRID_THUMBNAILS_UNTIL_ENUM_COMPLETE,
};
use ui::keyboard::{install_keyboard_handler, install_scroll_navigation_handlers, KeyboardDeps};
use ui::navigation::{install_navigation_handlers, NavigationDeps};
use ui::open_folder::{build_open_folder_action, OpenFolderActionDeps};
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
    collections::VecDeque,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering as AtomicOrdering},
    time::{Duration, Instant},
};

use libadwaita as adw;
use adw::prelude::*;
use gtk4::{
    gio, glib, CustomFilter, CustomSorter, FilterListModel, Image, Label, ListItem,
    ProgressBar, SignalListItemFactory, SingleSelection,
    SortListModel, StringObject, TreeListRow,
};

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

fn sync_progress_widgets(
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

    // -----------------------------------------------------------------------
    // Receiver task: buffer messages and drain in idle-priority batches
    // -----------------------------------------------------------------------
    let list_store_recv = list_store.clone();
    let toast_recv = toast_overlay.clone();
    let meta_cache_recv = meta_cache.clone();
    let hash_cache_recv = hash_cache.clone();
    let sort_fields_cache_recv = sort_fields_cache.clone();
    let active_scan_generation_recv = active_scan_generation.clone();
    let scan_in_progress_recv = scan_in_progress.clone();
    let thumbnail_size_recv = thumbnail_size.clone();
    let realized_thumb_images_recv = realized_thumb_images.clone();
    let progress_state_recv = progress_state.clone();
    let progress_box_recv = progress_box.clone();
    let progress_label_recv = progress_label.clone();
    let progress_bar_recv = progress_bar.clone();

    /// Maximum items drained from the buffer per idle tick.
    const BATCH_SIZE: usize = SCAN_DRAIN_BATCH_SIZE as usize;

    let buffer: Rc<RefCell<VecDeque<ScanMessage>>> =
        Rc::new(RefCell::new(VecDeque::new()));
    let drain_scheduled: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    // Idle-priority drain callback: processes up to BATCH_SIZE messages per
    // tick, then yields control back to GTK for user-input events.
    let schedule_drain = {
        let buffer = buffer.clone();
        let drain_scheduled = drain_scheduled.clone();
        let list_store_recv = list_store_recv.clone();
        let hash_cache_recv = hash_cache_recv.clone();
        let meta_cache_recv = meta_cache_recv.clone();
        let sort_fields_cache_recv = sort_fields_cache_recv.clone();
        let active_scan_generation_recv = active_scan_generation_recv.clone();
        let scan_in_progress_recv = scan_in_progress_recv.clone();
        let toast_recv = toast_recv.clone();
        let progress_state_recv = progress_state_recv.clone();
        let progress_box_recv = progress_box_recv.clone();
        let progress_label_recv = progress_label_recv.clone();
        let progress_bar_recv = progress_bar_recv.clone();
        Rc::new(move || {
            if *drain_scheduled.borrow() {
                return;
            }
            *drain_scheduled.borrow_mut() = true;
            SCAN_DRAIN_SCHEDULED.store(1, AtomicOrdering::Relaxed);

            let buffer = buffer.clone();
            let drain_scheduled = drain_scheduled.clone();
            let list_store_recv = list_store_recv.clone();
            let hash_cache_recv = hash_cache_recv.clone();
            let meta_cache_recv = meta_cache_recv.clone();
            let sort_fields_cache_recv = sort_fields_cache_recv.clone();
            let active_scan_generation_recv = active_scan_generation_recv.clone();
            let scan_in_progress_recv = scan_in_progress_recv.clone();
            let toast_recv = toast_recv.clone();
            let thumbnail_size_recv = thumbnail_size_recv.clone();
            let realized_thumb_images_recv = realized_thumb_images_recv.clone();
            let progress_state_recv = progress_state_recv.clone();
            let progress_box_recv = progress_box_recv.clone();
            let progress_label_recv = progress_label_recv.clone();
            let progress_bar_recv = progress_bar_recv.clone();
            glib::idle_add_local(move || {
                *drain_scheduled.borrow_mut() = false;
                SCAN_DRAIN_SCHEDULED.store(0, AtomicOrdering::Relaxed);

                let mut batch: Vec<ScanMessage> =
                    Vec::with_capacity(BATCH_SIZE);
                {
                    let mut buf = buffer.borrow_mut();
                    for _ in 0..BATCH_SIZE {
                        if let Some(msg) = buf.pop_front() {
                            batch.push(msg);
                        } else {
                            break;
                        }
                    }
                }
                if !batch.is_empty() {
                    SCAN_BUFFER_DEPTH.fetch_sub(batch.len() as u64, AtomicOrdering::Relaxed);
                }

                // Collect enumerated paths for batch splice.
                let mut new_paths: Vec<StringObject> = Vec::new();
                let mut scan_complete = false;
                let mut unlock_thumbnail_dispatch = false;
                let mut progress_changed = false;
                let active_generation = active_scan_generation_recv.get();

                for msg in batch {
                    match msg {
                        ScanMessage::ScanStarted {
                            total_count,
                            generation,
                        } => {
                            if generation != active_generation {
                                continue;
                            }
                            let mut progress = progress_state_recv.borrow_mut();
                            progress.begin_with_total(generation, total_count);
                            progress_changed = true;
                        }
                        ScanMessage::ImageEnumerated { path, generation } => {
                            if generation != active_generation {
                                continue;
                            }
                            sort_fields_cache_recv
                                .borrow_mut()
                                .entry(path.clone())
                                .or_insert_with(|| compute_sort_fields(&path));
                            new_paths.push(StringObject::new(&path));
                            let mut progress = progress_state_recv.borrow_mut();
                            if progress.generation == generation {
                                progress.enumerated_done = progress
                                    .enumerated_done
                                    .saturating_add(1)
                                    .min(progress.total_or_one());
                                progress_changed = true;
                            }
                        }
                        ScanMessage::EnumerationComplete { generation } => {
                            if generation != active_generation {
                                continue;
                            }
                            unlock_thumbnail_dispatch = true;
                            let mut progress = progress_state_recv.borrow_mut();
                            if progress.generation == generation {
                                progress.enumerated_done = progress.total_files;
                                progress_changed = true;
                            }
                        }
                        ScanMessage::ImageEnriched {
                            path,
                            hash,
                            meta,
                            indexed_from_cache,
                            generation,
                        } => {
                            if generation != active_generation {
                                continue;
                            }
                            let has_thumbnail_hash = !hash.is_empty();
                            if has_thumbnail_hash {
                                hash_cache_recv
                                    .borrow_mut()
                                    .insert(path.clone(), hash);
                            }
                            meta_cache_recv.borrow_mut().insert(path, meta);
                            let mut progress = progress_state_recv.borrow_mut();
                            if progress.generation == generation {
                                progress.enriched_done = progress
                                    .enriched_done
                                    .saturating_add(1)
                                    .min(progress.total_or_one());
                                if indexed_from_cache {
                                    progress.enriched_cached =
                                        progress.enriched_cached.saturating_add(1);
                                } else {
                                    progress.enriched_generated =
                                        progress.enriched_generated.saturating_add(1);
                                }
                                if has_thumbnail_hash {
                                    progress.thumbnails_ready_done = progress
                                        .thumbnails_ready_done
                                        .saturating_add(1)
                                        .min(progress.total_or_one());
                                }
                                progress_changed = true;
                            }
                        }
                        ScanMessage::ScanComplete { generation } => {
                            if generation != active_generation {
                                continue;
                            }
                            scan_complete = true;
                            let mut progress = progress_state_recv.borrow_mut();
                            if progress.generation == generation {
                                progress.enumerated_done = progress.total_files;
                                progress.enriched_done = progress.total_files;
                                progress.thumbnails_ready_done = progress.total_files;
                                progress_changed = true;
                            }
                        }
                    }
                }

                // Batch-insert enumerated paths via splice (single model notification).
                if !new_paths.is_empty() {
                    list_store_recv.splice(
                        list_store_recv.n_items(),
                        0,
                        &new_paths,
                    );
                }

                if unlock_thumbnail_dispatch {
                    DEFER_GRID_THUMBNAILS_UNTIL_ENUM_COMPLETE
                        .store(0, AtomicOrdering::Relaxed);
                    refresh_realized_grid_thumbnails(
                        &realized_thumb_images_recv,
                        &thumbnail_size_recv,
                        &hash_cache_recv,
                    );
                }

                if scan_complete {
                    DEFER_GRID_THUMBNAILS_UNTIL_ENUM_COMPLETE
                        .store(0, AtomicOrdering::Relaxed);
                    scan_in_progress_recv.set(false);
                    let n = list_store_recv.n_items();
                    let mut total_size_bytes = 0_u64;
                    {
                        let cache = sort_fields_cache_recv.borrow();
                        for i in 0..list_store_recv.n_items() {
                            if let Some(item) = list_store_recv.item(i).and_downcast::<StringObject>() {
                                if let Some(fields) = cache.get(item.string().as_str()) {
                                    total_size_bytes = total_size_bytes.saturating_add(fields.size);
                                }
                            }
                        }
                    }
                    let text = format!(
                        "Found {} image{}",
                        n,
                        if n == 1 { "" } else { "s" }
                    );
                    let toast = adw::Toast::new(&text);
                    toast.set_timeout(3);
                    toast_recv.add_toast(toast);

                    let done_generation = active_generation;
                    let progress_state_done = progress_state_recv.clone();
                    let progress_box_done = progress_box_recv.clone();
                    let progress_label_done = progress_label_recv.clone();
                    let progress_bar_done = progress_bar_recv.clone();
                    glib::timeout_add_local_once(Duration::from_millis(900), move || {
                        let mut progress = progress_state_done.borrow_mut();
                        if progress.generation == done_generation {
                            progress.folder_image_count = n;
                            progress.folder_total_size_bytes = total_size_bytes;
                            progress.visible = false;
                            sync_progress_widgets(
                                &progress,
                                &progress_box_done,
                                &progress_label_done,
                                &progress_bar_done,
                            );
                        }
                    });
                }

                if progress_changed {
                    let progress = progress_state_recv.borrow();
                    sync_progress_widgets(
                        &progress,
                        &progress_box_recv,
                        &progress_label_recv,
                        &progress_bar_recv,
                    );
                }

                // Re-schedule if the buffer still has items.
                if !buffer.borrow().is_empty() {
                    *drain_scheduled.borrow_mut() = true;
                    SCAN_DRAIN_SCHEDULED.store(1, AtomicOrdering::Relaxed);
                    glib::ControlFlow::Continue
                } else {
                    glib::ControlFlow::Break
                }
            });
        })
    };

    // Async receiver: pushes messages into the buffer and triggers idle drain.
    let buffer_recv = buffer.clone();
    let schedule_drain_recv = schedule_drain.clone();
    glib::MainContext::default().spawn_local(async move {
        while let Ok(msg) = receiver.recv().await {
            buffer_recv.borrow_mut().push_back(msg);
            SCAN_BUFFER_DEPTH.fetch_add(1, AtomicOrdering::Relaxed);
            schedule_drain_recv();
        }
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

    // --- Filter model: wraps list_store, applies search text ---
    let meta_cache_filter = meta_cache.clone();
    let search_text_filter = search_text.clone();
    let filter = CustomFilter::new(move |obj| {
        let query = search_text_filter.borrow().to_lowercase();
        if query.is_empty() {
            return true;
        }
        let path_str = obj
            .downcast_ref::<StringObject>()
            .map(|s| s.string().to_string())
            .unwrap_or_default();
        // Match against filename.
        let filename = std::path::Path::new(&path_str)
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if filename.contains(&query) {
            return true;
        }
        // Match against cached metadata fields.
        let cache = meta_cache_filter.borrow();
        if let Some(meta) = cache.get(&path_str) {
            let fields: [Option<&str>; 8] = [
                meta.camera_make.as_deref(),
                meta.camera_model.as_deref(),
                meta.exposure.as_deref(),
                meta.iso.as_deref(),
                meta.prompt.as_deref(),
                meta.negative_prompt.as_deref(),
                meta.raw_parameters.as_deref(),
                meta.workflow_json.as_deref(),
            ];
            for field in fields.iter().flatten() {
                if field.to_lowercase().contains(&query) {
                    return true;
                }
            }
        }
        false
    });
    let filter_model = FilterListModel::new(Some(list_store.clone()), Some(filter.clone()));

    // --- Sort model: wraps filter_model, applies selected sort key ---
    let sort_key_sorter = sort_key.clone();
    let sort_fields_cache_sorter = sort_fields_cache.clone();
    let sorter = CustomSorter::new(move |a, b| {
        let path_a = a
            .downcast_ref::<StringObject>()
            .map(|s| s.string().to_string())
            .unwrap_or_default();
        let path_b = b
            .downcast_ref::<StringObject>()
            .map(|s| s.string().to_string())
            .unwrap_or_default();
        let key = sort_key_sorter.borrow().clone();
        let cache = sort_fields_cache_sorter.borrow();
        let fallback_a;
        let fallback_b;
        let fields_a = if let Some(fields) = cache.get(&path_a) {
            fields
        } else {
            fallback_a = compute_sort_fields(&path_a);
            &fallback_a
        };
        let fields_b = if let Some(fields) = cache.get(&path_b) {
            fields
        } else {
            fallback_b = compute_sort_fields(&path_b);
            &fallback_b
        };
        let ord = match normalize_sort_key(key.as_str()) {
            "name_asc" | "name_desc" => {
                let cmp = fields_a
                    .filename_lower
                    .cmp(&fields_b.filename_lower)
                    .then_with(|| path_a.cmp(&path_b));
                if key == SORT_KEY_NAME_DESC { cmp.reverse() } else { cmp }
            }
            "date_asc" | "date_desc" => {
                let cmp = fields_a
                    .modified
                    .cmp(&fields_b.modified)
                    .then_with(|| fields_a.filename_lower.cmp(&fields_b.filename_lower))
                    .then_with(|| path_a.cmp(&path_b));
                if key == SORT_KEY_DATE_DESC { cmp.reverse() } else { cmp }
            }
            "size_asc" | "size_desc" => {
                let cmp = fields_a
                    .size
                    .cmp(&fields_b.size)
                    .then_with(|| fields_a.filename_lower.cmp(&fields_b.filename_lower))
                    .then_with(|| path_a.cmp(&path_b));
                if key == SORT_KEY_SIZE_DESC { cmp.reverse() } else { cmp }
            }
            _ => std::cmp::Ordering::Equal,
        };
        match ord {
            std::cmp::Ordering::Less => gtk4::Ordering::Smaller,
            std::cmp::Ordering::Greater => gtk4::Ordering::Larger,
            std::cmp::Ordering::Equal => gtk4::Ordering::Equal,
        }
    });
    let sort_model = SortListModel::new(Some(filter_model.clone()), Some(sorter.clone()));

    // --- Center: ViewStack with Grid + Single pages ---
    let selection_model = SingleSelection::new(Some(sort_model.clone()));
    let selection_for_default = selection_model.clone();
    sort_model.connect_items_changed(move |model, _, _, _| {
        if model.n_items() > 0 && selection_for_default.selected_item().is_none() {
            selection_for_default.set_selected(0);
        }
    });

    let factory = SignalListItemFactory::new();

    let thumbnail_size_setup = thumbnail_size.clone();
    let realized_thumb_images_setup = realized_thumb_images.clone();
    let realized_cell_boxes_setup = realized_cell_boxes.clone();
    let on_rename = make_rename_action(
        window.clone(),
        toast_overlay.clone(),
        start_scan_for_folder.clone(),
        current_folder.clone(),
    );
    let on_delete = make_delete_action(
        window.clone(),
        toast_overlay.clone(),
        start_scan_for_folder.clone(),
        current_folder.clone(),
    );
    factory.connect_setup(move |_, obj| {
        let list_item = obj.downcast_ref::<ListItem>().unwrap();
        setup_grid_list_item(
            list_item,
            &thumbnail_size_setup,
            &realized_cell_boxes_setup,
            &realized_thumb_images_setup,
            on_rename.clone(),
            on_delete.clone(),
        );
    });

    let hash_cache_bind = hash_cache.clone();
    let thumbnail_size_bind = thumbnail_size.clone();
    let fast_scroll_active_bind = fast_scroll_active.clone();
    factory.connect_bind(move |_, obj| {
        let list_item = obj.downcast_ref::<ListItem>().unwrap();
        bind_grid_list_item(
            list_item,
            &thumbnail_size_bind,
            &fast_scroll_active_bind,
            hash_cache_bind.clone(),
        );
    });

    factory.connect_unbind(|_, obj| {
        let list_item = obj.downcast_ref::<ListItem>().unwrap();
        unbind_grid_list_item(list_item);
    });

    let grid_view = create_grid_view(&selection_model, &factory);
    let grid_scroll = create_grid_scroll(&grid_view);
    let grid_overlay = create_grid_overlay(&grid_scroll);

    let (scroll_flag_box, scroll_flag) = build_scroll_flag_overlay();
    add_scroll_flag_overlay(&grid_overlay, &scroll_flag_box);

    // Scroll-speed gate: suppress thumbnail spawning while scrolling faster than
    // 5 rows/sec, then refresh visible cells 150 ms after scrolling quiets down.
    install_grid_scroll_speed_gate(
        &grid_scroll,
        &grid_view,
        &fast_scroll_active,
        &scroll_last_pos,
        &scroll_last_time,
        &scroll_debounce_gen,
        &thumbnail_size,
        &realized_thumb_images,
        &hash_cache,
        &selection_model,
        &sort_key,
        &sort_fields_cache,
        &scroll_flag_box,
        &scroll_flag,
    );

    attach_grid_page(&view_stack, &grid_overlay);

    let single_picture = create_single_picture();
    attach_single_page(&view_stack, &single_picture);
    set_default_grid_page(&view_stack);
    let center_box = create_center_box(&view_stack);

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
