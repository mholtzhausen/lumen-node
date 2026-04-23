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
use dialogs::{open_delete_dialog, open_rename_dialog};
use file_name_ops::{
    clipboard_base_name_hint,
};
use byte_format::human_readable_bytes;
use scan::ScanMessage;
use recent_folders::push_recent_folder_entry;
use scanner::scan_directory;
use sort_flags::{compute_sort_fields, SortFields};
use sort::{
    normalize_sort_key, sort_index_for_key, sort_key_for_index, SORT_KEY_DATE_DESC,
    SORT_KEY_NAME_ASC, SORT_KEY_NAME_DESC, SORT_KEY_SIZE_DESC,
};
use thumbnail_sizing::{normalize_thumbnail_size, thumbnail_size_options};
use timing_report::write_timing_report;
use tree_sidebar::{reset_tree_root, sync_tree_to_path};
use ui::actions::install_context_menu;
use ui::grid::{
    add_scroll_flag_overlay, apply_thumbnail_size_change, attach_grid_page, attach_single_page,
    bind_grid_list_item, build_scroll_flag_overlay, create_center_box, create_grid_overlay,
    create_grid_scroll, create_grid_view, create_single_picture, enter_single_view_mode,
    install_grid_scroll_speed_gate, make_delete_action, make_rename_action,
    refresh_realized_grid_thumbnails, set_default_grid_page, setup_grid_list_item,
    unbind_grid_list_item, ACTIVE_THUMBNAIL_TASKS,
    DEFER_GRID_THUMBNAILS_UNTIL_ENUM_COMPLETE,
};
use ui::preview::{load_picture_async, PreviewLoadMetrics, PreviewLoadOutcome, ACTIVE_PREVIEW_TASKS};
use ui::selection::{handle_selection_change_event, ClickTrace};
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
use view_helpers::selected_image_path;
use window_math::{pct_to_px, px_to_pct};

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
    gdk, gio, glib, CustomFilter, CustomSorter, EventControllerKey, EventControllerScroll,
    EventControllerScrollFlags, FilterListModel, GestureClick, Image, Label, ListItem,
    ListScrollFlags, ProgressBar, SignalListItemFactory, SingleSelection,
    SortListModel, StringObject, TreeListRow,
};

pub(crate) static CLICK_TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);
static FULL_VIEW_TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);
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

struct FullViewTrace {
    id: u64,
    path: String,
    started: Instant,
    outcome: String,
    active_thumbnail_jobs_at_activate: u64,
    active_preview_jobs_at_activate: u64,
    preview_queue_wait_ms: Option<f64>,
    preview_file_open_ms: Option<f64>,
    preview_decode_ms: Option<f64>,
    preview_texture_create_ms: Option<f64>,
    preview_worker_total_ms: Option<f64>,
    preview_main_thread_dispatch_ms: Option<f64>,
    preview_texture_apply_ms: Option<f64>,
    displayed_ms: Option<f64>,
}

impl FullViewTrace {
    fn new(
        id: u64,
        path: String,
        active_thumbnail_jobs_at_activate: u64,
        active_preview_jobs_at_activate: u64,
    ) -> Self {
        Self {
            id,
            path,
            started: Instant::now(),
            outcome: "pending".to_string(),
            active_thumbnail_jobs_at_activate,
            active_preview_jobs_at_activate,
            preview_queue_wait_ms: None,
            preview_file_open_ms: None,
            preview_decode_ms: None,
            preview_texture_create_ms: None,
            preview_worker_total_ms: None,
            preview_main_thread_dispatch_ms: None,
            preview_texture_apply_ms: None,
            displayed_ms: None,
        }
    }

    fn total_ms(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }
}

fn emit_full_view_report(trace: &FullViewTrace) {
    let mut report = String::new();
    report.push_str(&format!(
        "FULLVIEW {} | {} | outcome={}\n",
        trace.id, trace.path, trace.outcome
    ));
    if let Some(v) = trace.displayed_ms {
        report.push_str(&format!("{:>8.3}ms fullview_displayed\n", v));
    }
    report.push_str("METRICS\n");
    report.push_str(&format!(
        "active_thumbnail_jobs_at_activate={}\n",
        trace.active_thumbnail_jobs_at_activate
    ));
    report.push_str(&format!(
        "active_preview_jobs_at_activate={}\n",
        trace.active_preview_jobs_at_activate
    ));
    if let Some(v) = trace.preview_queue_wait_ms {
        report.push_str(&format!("preview_queue_wait_ms={:.3}\n", v));
    }
    if let Some(v) = trace.preview_file_open_ms {
        report.push_str(&format!("preview_file_open_ms={:.3}\n", v));
    }
    if let Some(v) = trace.preview_decode_ms {
        report.push_str(&format!("preview_decode_ms={:.3}\n", v));
    }
    if let Some(v) = trace.preview_texture_create_ms {
        report.push_str(&format!("preview_texture_create_ms={:.3}\n", v));
    }
    if let Some(v) = trace.preview_worker_total_ms {
        report.push_str(&format!("preview_worker_total_ms={:.3}\n", v));
    }
    if let Some(v) = trace.preview_main_thread_dispatch_ms {
        report.push_str(&format!("preview_main_thread_dispatch_ms={:.3}\n", v));
    }
    if let Some(v) = trace.preview_texture_apply_ms {
        report.push_str(&format!("preview_texture_apply_ms={:.3}\n", v));
    }
    report.push_str(&format!("TOTAL {:>8.3}ms\n\n", trace.total_ms()));

    write_timing_report(&report);
}

fn new_full_view_trace(path_str: String) -> Rc<RefCell<FullViewTrace>> {
    let full_view_id = FULL_VIEW_TRACE_COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
    let active_thumbnail_jobs_at_activate = ACTIVE_THUMBNAIL_TASKS.load(AtomicOrdering::Relaxed);
    let active_preview_jobs_at_activate = ACTIVE_PREVIEW_TASKS.load(AtomicOrdering::Relaxed);
    Rc::new(RefCell::new(FullViewTrace::new(
        full_view_id,
        path_str,
        active_thumbnail_jobs_at_activate,
        active_preview_jobs_at_activate,
    )))
}

fn apply_full_view_metrics(trace: &Rc<RefCell<FullViewTrace>>, metrics: PreviewLoadMetrics) {
    let mut t = trace.borrow_mut();
    t.preview_queue_wait_ms = Some(metrics.queue_wait_ms);
    t.preview_file_open_ms = Some(metrics.file_open_ms);
    t.preview_decode_ms = Some(metrics.decode_ms);
    t.preview_texture_create_ms = Some(metrics.texture_create_ms);
    t.preview_worker_total_ms = Some(metrics.worker_total_ms);
    t.preview_main_thread_dispatch_ms = Some(metrics.main_thread_dispatch_ms);
    t.preview_texture_apply_ms = Some(metrics.texture_apply_ms);
    t.displayed_ms = Some(t.started.elapsed().as_secs_f64() * 1000.0);
    t.outcome = match metrics.outcome {
        PreviewLoadOutcome::Displayed => "done".to_string(),
        PreviewLoadOutcome::Failed => "failed".to_string(),
        PreviewLoadOutcome::StaleOrCancelled => "cancelled".to_string(),
    };
    emit_full_view_report(&t);
}

fn dispatch_full_view_load(
    picture: &gtk4::Picture,
    path_str: &str,
    trace: Rc<RefCell<FullViewTrace>>,
) {
    let trace_for_cb = trace.clone();
    load_picture_async(
        picture,
        path_str,
        None,
        Some(Box::new(move |metrics| {
            apply_full_view_metrics(&trace_for_cb, metrics);
        })),
    );
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
    let size_options = thumbnail_size_options();

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

    // -----------------------------------------------------------------------
    // Wire: grid item activate → switch to single view
    // -----------------------------------------------------------------------
    let pre_fullview_left: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let pre_fullview_right: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let stack_for_grid = view_stack.clone();
    let picture_for_grid = single_picture.clone();
    let selection_for_grid = selection_model.clone();
    let left_toggle_grid = left_toggle.clone();
    let right_toggle_grid = right_toggle.clone();
    let pre_fullview_left_grid = pre_fullview_left.clone();
    let pre_fullview_right_grid = pre_fullview_right.clone();
    grid_view.connect_activate(move |_, pos| {
        if let Some(item) = selection_for_grid.item(pos).and_downcast::<StringObject>() {
            let path_str = item.string().to_string();
            let trace = new_full_view_trace(path_str.clone());
            dispatch_full_view_load(&picture_for_grid, &path_str, trace);
        }
        enter_single_view_mode(
            &stack_for_grid,
            &left_toggle_grid,
            &right_toggle_grid,
            &pre_fullview_left_grid,
            &pre_fullview_right_grid,
        );
    });

    // -----------------------------------------------------------------------
    // Wire: double-click on preview image → switch to single view
    // -----------------------------------------------------------------------
    {
        let stack_for_preview = view_stack.clone();
        let picture_for_preview = single_picture.clone();
        let selection_for_preview = selection_model.clone();
        let left_toggle_preview = left_toggle.clone();
        let right_toggle_preview = right_toggle.clone();
        let pre_fullview_left_preview = pre_fullview_left.clone();
        let pre_fullview_right_preview = pre_fullview_right.clone();
        let dbl_click = GestureClick::new();
        dbl_click.connect_pressed(move |_, n_press, _, _| {
            if n_press < 2 {
                return;
            }
            let Some(item) = selection_for_preview
                .selected_item()
                .and_downcast::<StringObject>()
            else {
                return;
            };
            let path_str = item.string().to_string();
            let trace = new_full_view_trace(path_str.clone());
            dispatch_full_view_load(&picture_for_preview, &path_str, trace);
            enter_single_view_mode(
                &stack_for_preview,
                &left_toggle_preview,
                &right_toggle_preview,
                &pre_fullview_left_preview,
                &pre_fullview_right_preview,
            );
        });
        meta_preview.add_controller(dbl_click);
    }

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
    let current_folder_open_action = current_folder.clone();
    let start_scan_open_action = start_scan_for_folder.clone();
    let tree_root_open_action = tree_root.clone();
    let tree_model_open_action = tree_model.clone();
    let tree_list_view_open_action = tree_list_view.clone();
    let recent_folders_open_action = recent_folders.clone();
    let sort_key_open_action = sort_key.clone();
    let search_text_open_action = search_text.clone();
    let thumbnail_size_open_action = thumbnail_size.clone();
    let sort_dropdown_open_action = sort_dropdown.clone();
    let search_entry_open_action = search_entry.clone();
    let filter_open_action = filter.clone();
    let sorter_open_action = sorter.clone();
    let size_buttons_open_action = size_buttons.clone();
    let progress_state_open_action = progress_state.clone();
    let open_folder_action = Rc::new(move |path: std::path::PathBuf, sync_tree: bool| {
        if current_folder_open_action.borrow().as_deref() == Some(path.as_path()) {
            return;
        }

        if let Some(saved_ui_state) = db::load_ui_state(path.as_path()) {
            let selected_sort = sort_index_for_key(&saved_ui_state.sort_key);
            *sort_key_open_action.borrow_mut() = saved_ui_state.sort_key;
            *search_text_open_action.borrow_mut() = saved_ui_state.search_text.clone();
            *thumbnail_size_open_action.borrow_mut() =
                normalize_thumbnail_size(saved_ui_state.thumbnail_size);

            if sort_dropdown_open_action.selected() != selected_sort {
                sort_dropdown_open_action.set_selected(selected_sort);
            }
            search_entry_open_action.set_text(&saved_ui_state.search_text);
            filter_open_action.changed(gtk4::FilterChange::Different);
            sorter_open_action.changed(gtk4::SorterChange::Different);
            for (i, btn) in size_buttons_open_action.iter().enumerate() {
                btn.set_active(thumbnail_size_options()[i] == *thumbnail_size_open_action.borrow());
            }
        } else {
            let seeded_state = db::UiState {
                sort_key: sort_key_open_action.borrow().clone(),
                search_text: search_text_open_action.borrow().clone(),
                thumbnail_size: *thumbnail_size_open_action.borrow(),
            };
            let _ = db::save_ui_state(path.as_path(), &seeded_state);
        }

        *current_folder_open_action.borrow_mut() = Some(path.clone());
        progress_state_open_action.borrow_mut().current_folder_path = path.display().to_string();
        {
            let mut history = recent_folders_open_action.borrow_mut();
            push_recent_folder_entry(&mut history, path.as_path(), RECENT_FOLDERS_LIMIT);
            config::save_recent_state(Some(path.as_path()), &history);
        }
        reset_tree_root(&tree_root_open_action, path.as_path());
        start_scan_open_action(path.clone());
        if sync_tree {
            sync_tree_to_path(&tree_model_open_action, &tree_list_view_open_action, &path);
        }
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
    // Wire: sort dropdown → update sort key and invalidate sorter
    // -----------------------------------------------------------------------
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

    // -----------------------------------------------------------------------
    // Wire: search entry → update search text and invalidate filter
    // -----------------------------------------------------------------------
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

    // -----------------------------------------------------------------------
    // Wire: clear button → reset search and sort
    // -----------------------------------------------------------------------
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

    // -----------------------------------------------------------------------
    // Wire: thumbnail size toggles → update size and refresh cell bindings
    // -----------------------------------------------------------------------
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
    let esc_pending: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let key_controller = EventControllerKey::new();
    key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let stack_for_keys = view_stack.clone();
    let selection_for_keys = selection_model.clone();
    let picture_for_keys = single_picture.clone();
    let grid_view_for_keys = grid_view.clone();
    let grid_scroll_for_keys = grid_scroll.clone();
    let thumbnail_size_for_keys = thumbnail_size.clone();
    let toast_overlay_for_keys = toast_overlay.clone();
    let window_for_keys = window.clone();
    let current_folder_for_keys = current_folder.clone();
    let start_scan_for_folder_keys: Rc<dyn Fn(std::path::PathBuf)> = start_scan_for_folder.clone();
    let left_toggle_for_keys = left_toggle.clone();
    let right_toggle_for_keys = right_toggle.clone();
    let pre_fullview_left_keys = pre_fullview_left.clone();
    let pre_fullview_right_keys = pre_fullview_right.clone();
    key_controller.connect_key_pressed(move |_, key, _, state| {
        let ctrl_pressed = state.contains(gdk::ModifierType::CONTROL_MASK);
        if ctrl_pressed && key == gdk::Key::c {
            let Some(path) = selected_image_path(&selection_for_keys) else {
                return glib::Propagation::Stop;
            };
            let file = gio::File::for_path(&path);
            if let Ok(texture) = gdk::Texture::from_file(&file) {
                gtk4::prelude::WidgetExt::display(&window_for_keys)
                    .clipboard()
                    .set_texture(&texture);
                let toast = adw::Toast::new("Image copied to clipboard");
                toast.set_timeout(2);
                toast_overlay_for_keys.add_toast(toast);
            }
            return glib::Propagation::Stop;
        }
        if ctrl_pressed && key == gdk::Key::v {
            let Some(folder) = current_folder_for_keys.borrow().as_ref().cloned() else {
                let toast = adw::Toast::new("Open a folder before pasting");
                toast.set_timeout(2);
                toast_overlay_for_keys.add_toast(toast);
                return glib::Propagation::Stop;
            };
            let display = gtk4::prelude::WidgetExt::display(&window_for_keys);
            let clipboard = display.clipboard();
            let toast_overlay = toast_overlay_for_keys.clone();
            let window = window_for_keys.clone();
            let current_folder = current_folder_for_keys.clone();
            let start_scan_for_folder = start_scan_for_folder_keys.clone();
            glib::MainContext::default().spawn_local(async move {
                let Ok(Some(texture)) = clipboard.read_texture_future().await else {
                    let toast = adw::Toast::new("Clipboard does not contain an image");
                    toast.set_timeout(2);
                    toast_overlay.add_toast(toast);
                    return;
                };
                let suggested_name = clipboard
                    .read_text_future()
                    .await
                    .ok()
                    .flatten()
                    .as_ref()
                    .and_then(|text| clipboard_base_name_hint(text.as_str()));
                let uuid_base = glib::uuid_string_random().to_string();
                let target_path = folder.join(format!("{uuid_base}.png"));
                match texture.save_to_png(&target_path) {
                    Ok(()) => {
                        start_scan_for_folder(folder.clone());
                        open_rename_dialog(
                            &window,
                            &toast_overlay,
                            &start_scan_for_folder,
                            &current_folder,
                            target_path,
                            Some(suggested_name.unwrap_or(uuid_base)),
                        );
                    }
                    Err(err) => {
                        let toast = adw::Toast::new(&format!("Paste failed: {}", err));
                        toast.set_timeout(3);
                        toast_overlay.add_toast(toast);
                    }
                }
            });
            return glib::Propagation::Stop;
        }
        if key == gdk::Key::Delete {
            let Some(path) = selected_image_path(&selection_for_keys) else {
                return glib::Propagation::Stop;
            };
            open_delete_dialog(
                &window_for_keys,
                &toast_overlay_for_keys,
                &start_scan_for_folder_keys,
                &current_folder_for_keys,
                path,
            );
            return glib::Propagation::Stop;
        }
        if key == gdk::Key::Escape {
            let in_grid = stack_for_keys.visible_child_name().as_deref() == Some("grid");
            if in_grid {
                if esc_pending.get() {
                    window_for_keys.application().unwrap().quit();
                } else {
                    esc_pending.set(true);
                    let toast = adw::Toast::new("Press Escape again to quit");
                    toast.set_timeout(2);
                    toast_overlay_for_keys.add_toast(toast);
                    let esc_pending_clone = esc_pending.clone();
                    glib::timeout_add_local_once(Duration::from_millis(2000), move || {
                        esc_pending_clone.set(false);
                    });
                }
            } else {
                stack_for_keys.set_visible_child_name("grid");
                left_toggle_for_keys.set_active(pre_fullview_left_keys.get());
                right_toggle_for_keys.set_active(pre_fullview_right_keys.get());
            }
            return glib::Propagation::Stop;
        }
        let in_grid = stack_for_keys.visible_child_name().as_deref() == Some("grid");
        if in_grid
            && (key == gdk::Key::Page_Up
                || key == gdk::Key::Page_Down
                || key == gdk::Key::Home
                || key == gdk::Key::End)
        {
            let count = selection_for_keys.n_items();
            if count == 0 {
                return glib::Propagation::Proceed;
            }
            let has_selection = selection_for_keys.selected_item().is_some();
            let cur = selection_for_keys.selected();
            let thumb_size = (*thumbnail_size_for_keys.borrow()).max(1);
            let cell_width = (thumb_size + 4).max(1);
            let cell_height = (thumb_size + 20).max(1);
            let viewport_width = grid_scroll_for_keys.width().max(cell_width);
            let viewport_height = grid_scroll_for_keys.height().max(cell_height);
            let columns = (viewport_width / cell_width).max(1) as u32;
            let rows = (viewport_height / cell_height).max(1) as u32;
            let page_step = (columns * rows).max(1);

            let next = match key {
                gdk::Key::Home => 0,
                gdk::Key::End => count - 1,
                gdk::Key::Page_Up => {
                    if !has_selection {
                        0
                    } else {
                        cur.saturating_sub(page_step)
                    }
                }
                gdk::Key::Page_Down => {
                    if !has_selection {
                        0
                    } else {
                        cur.saturating_add(page_step).min(count - 1)
                    }
                }
                _ => cur,
            };

            if !has_selection || next != cur {
                selection_for_keys.set_selected(next);
                grid_view_for_keys.scroll_to(next, ListScrollFlags::FOCUS | ListScrollFlags::SELECT, None);
            }
            return glib::Propagation::Stop;
        }
        let in_single = stack_for_keys.visible_child_name().as_deref() == Some("single");
        if in_single && (key == gdk::Key::Left || key == gdk::Key::Right) {
            let count = selection_for_keys.n_items();
            if count == 0 {
                return glib::Propagation::Proceed;
            }
            let cur = selection_for_keys.selected();
            let next = if key == gdk::Key::Left {
                cur.saturating_sub(1)
            } else {
                (cur + 1).min(count - 1)
            };
            if next != cur {
                selection_for_keys.set_selected(next);
                if let Some(item) =
                    selection_for_keys.selected_item().and_downcast::<StringObject>()
                {
                    load_picture_async(
                        &picture_for_keys,
                        &item.string().to_string(),
                        None,
                        None,
                    );
                }
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_controller);

    // -----------------------------------------------------------------------
    // Scroll on single-view / meta-preview → navigate images
    // Accumulate delta so smooth-scroll trackpads don't flood set_selected.
    // -----------------------------------------------------------------------
    {
        let selection = selection_model.clone();
        let picture = single_picture.clone();
        let accum: Rc<Cell<f64>> = Rc::new(Cell::new(0.0));
        let scroll = EventControllerScroll::new(EventControllerScrollFlags::VERTICAL);
        scroll.connect_scroll(move |_, _dx, dy| {
            let count = selection.n_items();
            if count == 0 {
                return glib::Propagation::Proceed;
            }
            accum.set(accum.get() + dy);
            let steps = accum.get().trunc() as i32;
            if steps == 0 {
                return glib::Propagation::Stop;
            }
            accum.set(accum.get().fract());
            let cur = selection.selected() as i32;
            let next = (cur + steps).clamp(0, count as i32 - 1) as u32;
            if next != cur as u32 {
                selection.set_selected(next);
                if let Some(item) = selection.selected_item().and_downcast::<StringObject>() {
                    load_picture_async(&picture, &item.string().to_string(), None, None);
                }
            }
            glib::Propagation::Stop
        });
        single_picture.add_controller(scroll);
    }
    {
        let selection = selection_model.clone();
        let accum: Rc<Cell<f64>> = Rc::new(Cell::new(0.0));
        let scroll = EventControllerScroll::new(EventControllerScrollFlags::VERTICAL);
        scroll.connect_scroll(move |_, _dx, dy| {
            let count = selection.n_items();
            if count == 0 {
                return glib::Propagation::Proceed;
            }
            accum.set(accum.get() + dy);
            let steps = accum.get().trunc() as i32;
            if steps == 0 {
                return glib::Propagation::Stop;
            }
            accum.set(accum.get().fract());
            let cur = selection.selected() as i32;
            let next = (cur + steps).clamp(0, count as i32 - 1) as u32;
            if next != cur as u32 {
                selection.set_selected(next);
            }
            glib::Propagation::Stop
        });
        meta_preview.add_controller(scroll);
    }

    // -----------------------------------------------------------------------
    // Save config on window close (folder + pane positions)
    // -----------------------------------------------------------------------
    let cf_close = current_folder.clone();
    let outer_paned_close = outer_paned.clone();
    let inner_paned_close = inner_paned.clone();
    let meta_paned_close = meta_paned.clone();
    let meta_split_before_auto_collapse_close = meta_split_before_auto_collapse.clone();
    let sort_key_close = sort_key.clone();
    let search_text_close = search_text.clone();
    let thumbnail_size_close = thumbnail_size.clone();
    let recent_folders_close = recent_folders.clone();
    let left_toggle_close = left_toggle.clone();
    let right_toggle_close = right_toggle.clone();
    let window_for_close = window.clone();
    let outer_split_dirty_close = outer_split_dirty.clone();
    let inner_split_dirty_close = inner_split_dirty.clone();
    let meta_split_dirty_close = meta_split_dirty.clone();
    window.connect_close_request(move |_| {
        let window_width = window_for_close.width().max(1);
        let window_height = window_for_close.height().max(1);
        let window_maximized = window_for_close.is_maximized();
        let left_pos = outer_paned_close.position();
        let inner_pos = inner_paned_close.position();
        let raw_meta_pos = meta_split_before_auto_collapse_close
            .get()
            .unwrap_or_else(|| meta_paned_close.position());
        let meta_total_height = meta_paned_close.height().max(1);
        let meta_upper_bound = meta_total_height.saturating_sub(MIN_META_SPLIT_PX);
        let meta_pos = if meta_upper_bound < MIN_META_SPLIT_PX {
            // Window too short to preserve both minimum panes; persist midpoint.
            (meta_total_height / 2).max(1)
        } else {
            raw_meta_pos.clamp(MIN_META_SPLIT_PX, meta_upper_bound)
        };
        let recent_folders = recent_folders_close.borrow();
        let left_pos_for_save = if outer_split_dirty_close.get() {
            left_pos
        } else {
            configured_left_pane_pos.unwrap_or(left_pos)
        };
        let inner_pos_for_save = if inner_split_dirty_close.get() {
            inner_pos
        } else {
            configured_right_pane_pos.unwrap_or(inner_pos)
        };
        let right_width_for_save = window_width.saturating_sub(left_pos_for_save + inner_pos_for_save);
        let meta_pos_for_save = if meta_split_dirty_close.get() {
            meta_pos
        } else {
            configured_meta_pane_pos.unwrap_or(meta_pos)
        };
        let left_pct_for_save = if outer_split_dirty_close.get() {
            px_to_pct(left_pos_for_save, window_width)
        } else {
            configured_left_pane_width_pct.unwrap_or(px_to_pct(left_pos_for_save, window_width))
        };
        let right_pct_for_save = if inner_split_dirty_close.get() {
            px_to_pct(right_width_for_save, window_width)
        } else {
            configured_right_pane_width_pct
                .unwrap_or(px_to_pct(right_width_for_save, window_width))
        };
        let meta_pct_for_save = if meta_split_dirty_close.get() {
            px_to_pct(meta_pos_for_save, meta_total_height)
        } else {
            configured_meta_pane_height_pct
                .unwrap_or(px_to_pct(meta_pos_for_save, meta_total_height))
        };

        config::save(
            cf_close.borrow().as_deref(),
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
            left_toggle_close.is_active(),
            right_toggle_close.is_active(),
        );
        if let Some(folder) = cf_close.borrow().as_ref() {
            let _ = db::save_ui_state(
                folder.as_path(),
                &db::UiState {
                    sort_key: sort_key_close.borrow().clone(),
                    search_text: search_text_close.borrow().clone(),
                    thumbnail_size: *thumbnail_size_close.borrow(),
                },
            );
        }
        glib::Propagation::Proceed
    });

    // -----------------------------------------------------------------------
    // Restore last folder from config + sync tree
    // -----------------------------------------------------------------------
    if let Some(last_folder) = app_config.last_folder.as_ref() {
        if last_folder.is_dir() {
            open_folder_action(last_folder.clone(), true);
        }
    }

    // -----------------------------------------------------------------------
    // Restore persisted sort + search state into the UI controls
    // -----------------------------------------------------------------------
    {
        let initial_sort_idx: u32 = sort_index_for_key(sort_key.borrow().as_str());
        if initial_sort_idx != 0 {
            // fires connect_selected_notify → updates sort_key + calls sorter.changed()
            sort_dropdown.set_selected(initial_sort_idx);
        }
        let initial_search = search_text.borrow().clone();
        if !initial_search.is_empty() {
            search_entry.set_text(&initial_search);
            filter.changed(gtk4::FilterChange::Different);
        }
    }

    if app_config.window_maximized.unwrap_or(false) {
        window.maximize();
    }

    window.present();
    let window_for_pane_restore = window.clone();
    let outer_paned_restore = outer_paned.clone();
    let inner_paned_restore = inner_paned.clone();
    let meta_paned_restore = meta_paned.clone();
    let outer_position_programmatic_restore = outer_position_programmatic.clone();
    let inner_position_programmatic_restore = inner_position_programmatic.clone();
    let meta_position_programmatic_restore = meta_position_programmatic.clone();
    let pane_restore_complete_restore = pane_restore_complete.clone();
    let pane_restore_attempts = Rc::new(Cell::new(0_u8));
    let pane_restore_attempts_tick = pane_restore_attempts.clone();
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

        let left_limit =
            (window_width - MIN_CENTER_PANE_PX - MIN_RIGHT_PANE_PX).max(MIN_LEFT_PANE_PX);
        let left_pos = outer_paned_restore
            .position()
            .clamp(MIN_LEFT_PANE_PX, left_limit);
        let max_right_pane_width_px = window_width
            .saturating_sub(left_pos + MIN_CENTER_PANE_PX)
            .max(MIN_RIGHT_PANE_PX);
        let right_pane_width_px = configured_right_pane_width_pct
            .map(|pct| pct_to_px(window_width, pct))
            .or_else(|| {
                configured_right_pane_pos
                    .map(|inner_pos| window_width.saturating_sub(left_pos + inner_pos))
            })
            .unwrap_or(260)
            .clamp(MIN_RIGHT_PANE_PX, max_right_pane_width_px);
        let inner_pane_start_px = window_width
            .saturating_sub(left_pos + right_pane_width_px)
            .max(MIN_CENTER_PANE_PX);
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
        let meta_upper_bound = meta_total_height.saturating_sub(MIN_META_SPLIT_PX);
        let meta_pane_start_px = if meta_upper_bound < MIN_META_SPLIT_PX {
            // Window is too short to enforce both minimum split sizes.
            (meta_total_height / 2).max(1)
        } else {
            configured_meta_pos.clamp(MIN_META_SPLIT_PX, meta_upper_bound)
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
