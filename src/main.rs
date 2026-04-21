mod config;
mod db;
mod metadata;
mod scanner;
mod thumbnails;

use metadata::{ImageMetadata, ScanMessage};
use scanner::scan_directory;

use std::{
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
    gdk, gio, glib, CustomFilter, CustomSorter, EventControllerKey, FilterListModel,
    GestureClick,
    GridView, Image, Label, ListItem, ListView, ListScrollFlags, Orientation, Paned, Picture,
    PopoverMenu, ScrolledWindow, SignalListItemFactory, SingleSelection, SortListModel,
    Spinner, StringObject, TreeExpander, TreeListModel, TreeListRow,
};

static CLICK_TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);
static FULL_VIEW_TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);
static ACTIVE_THUMBNAIL_TASKS: AtomicU64 = AtomicU64::new(0);
static ACTIVE_PREVIEW_TASKS: AtomicU64 = AtomicU64::new(0);
static PREVIEW_REQUEST_PENDING: AtomicU64 = AtomicU64::new(0);
static SUPPRESS_SIDEBAR_DURING_PREVIEW: AtomicU64 = AtomicU64::new(0);
static THUMB_UI_CALLBACKS_TOTAL: AtomicU64 = AtomicU64::new(0);
static THUMB_UI_CALLBACKS_SKIPPED_WHILE_PREVIEW: AtomicU64 = AtomicU64::new(0);
static SCAN_BUFFER_DEPTH: AtomicU64 = AtomicU64::new(0);
static SCAN_DRAIN_SCHEDULED: AtomicU64 = AtomicU64::new(0);
const SCAN_DRAIN_BATCH_SIZE: u64 = 50;

struct AtomicTaskGuard {
    counter: &'static AtomicU64,
}

impl AtomicTaskGuard {
    fn new(counter: &'static AtomicU64) -> Self {
        counter.fetch_add(1, AtomicOrdering::Relaxed);
        Self { counter }
    }
}

impl Drop for AtomicTaskGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, AtomicOrdering::Relaxed);
    }
}

#[derive(Clone)]
struct ClickStepTiming {
    name: String,
    elapsed_ms: f64,
}

#[derive(Clone)]
struct ClickTrace {
    id: u64,
    path: String,
    started: Instant,
    steps: Vec<ClickStepTiming>,
    preview_done: bool,
    metadata_done: bool,
    finalize_scheduled: bool,
    finished: bool,
    outcome: String,
    active_thumbnail_jobs_at_click: u64,
    active_preview_jobs_at_click: u64,
    scan_buffer_depth_at_click: u64,
    idle_drain_scheduled_at_click: bool,
    pending_idle_drain_cycles_est_at_click: u64,
    thumb_ui_callbacks_total_at_click: u64,
    thumb_ui_callbacks_skipped_at_click: u64,
    thumb_ui_callbacks_total_until_preview: Option<u64>,
    thumb_ui_callbacks_skipped_until_preview: Option<u64>,
    scan_buffer_depth_at_idle_settled: Option<u64>,
    idle_drain_scheduled_at_idle_settled: Option<bool>,
    preview_displayed_at_ms: Option<f64>,
    preview_queue_wait_ms: Option<f64>,
    preview_file_open_ms: Option<f64>,
    preview_decode_ms: Option<f64>,
    preview_texture_create_ms: Option<f64>,
    preview_worker_total_ms: Option<f64>,
    preview_main_thread_dispatch_ms: Option<f64>,
    preview_texture_apply_ms: Option<f64>,
    main_loop_settle_ms: Option<f64>,
}

impl ClickTrace {
    fn new(
        id: u64,
        path: String,
        active_thumbnail_jobs_at_click: u64,
        active_preview_jobs_at_click: u64,
        scan_buffer_depth_at_click: u64,
        idle_drain_scheduled_at_click: bool,
        pending_idle_drain_cycles_est_at_click: u64,
        thumb_ui_callbacks_total_at_click: u64,
        thumb_ui_callbacks_skipped_at_click: u64,
    ) -> Self {
        Self {
            id,
            path,
            started: Instant::now(),
            steps: Vec::new(),
            preview_done: false,
            metadata_done: false,
            finalize_scheduled: false,
            finished: false,
            outcome: "pending".to_string(),
            active_thumbnail_jobs_at_click,
            active_preview_jobs_at_click,
            scan_buffer_depth_at_click,
            idle_drain_scheduled_at_click,
            pending_idle_drain_cycles_est_at_click,
            thumb_ui_callbacks_total_at_click,
            thumb_ui_callbacks_skipped_at_click,
            thumb_ui_callbacks_total_until_preview: None,
            thumb_ui_callbacks_skipped_until_preview: None,
            scan_buffer_depth_at_idle_settled: None,
            idle_drain_scheduled_at_idle_settled: None,
            preview_displayed_at_ms: None,
            preview_queue_wait_ms: None,
            preview_file_open_ms: None,
            preview_decode_ms: None,
            preview_texture_create_ms: None,
            preview_worker_total_ms: None,
            preview_main_thread_dispatch_ms: None,
            preview_texture_apply_ms: None,
            main_loop_settle_ms: None,
        }
    }

    fn mark_step(&mut self, name: &str) {
        let elapsed_ms = self.started.elapsed().as_secs_f64() * 1000.0;
        self.steps.push(ClickStepTiming {
            name: name.to_string(),
            elapsed_ms,
        });
    }

    fn total_ms(&self) -> f64 {
        self.started.elapsed().as_secs_f64() * 1000.0
    }
}

enum PreviewLoadOutcome {
    Displayed,
    Failed,
    StaleOrCancelled,
}

fn thumbnail_size_options() -> [i32; 4] {
    let base = thumbnails::THUMB_NORMAL_SIZE;
    [
        base,
        (((base as f64) * 1.3 / 16.0).round() as i32) * 16,
        (((base as f64) * 1.6 / 16.0).round() as i32) * 16,
        (((base as f64) * 1.9 / 16.0).round() as i32) * 16,
    ]
}

fn normalize_thumbnail_size(size: i32) -> i32 {
    let options = thumbnail_size_options();
    options
        .iter()
        .copied()
        .min_by_key(|opt| (opt - size).abs())
        .unwrap_or(thumbnails::THUMB_NORMAL_SIZE)
}

fn load_grid_thumbnail(
    thumb_image: &Image,
    path_str: String,
    size: i32,
    hash_cache: Rc<RefCell<HashMap<String, String>>>,
) {
    thumb_image.set_icon_name(Some("image-x-generic-symbolic"));
    unsafe { thumb_image.set_data("bound-path", path_str.clone()); }

    if PREVIEW_REQUEST_PENDING.load(AtomicOrdering::Relaxed) != 0 {
        return;
    }

    let cached_hash = hash_cache.borrow().get(&path_str).cloned();
    let already_loaded = if let Some(ref hash) = cached_hash {
        if let Some(thumb) = thumbnails::hash_thumb_if_exists_for_size(hash, size) {
            if let Ok(pb) = gdk_pixbuf::Pixbuf::from_file(&thumb) {
                let tex = gdk::Texture::for_pixbuf(&pb);
                thumb_image.set_paintable(Some(&tex));
                true
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    if already_loaded {
        return;
    }

    let path_for_thread = std::path::PathBuf::from(&path_str);
    let cached_hash_for_task = cached_hash.clone();
    let task = gio::spawn_blocking(move || {
        let _guard = AtomicTaskGuard::new(&ACTIVE_THUMBNAIL_TASKS);

        if let Some(hash) = cached_hash_for_task {
            let thumb = thumbnails::generate_hash_thumbnail_for_size(&path_for_thread, &hash, size);
            return (thumb, Some(hash));
        }

        if size == thumbnails::THUMB_NORMAL_SIZE {
            return (thumbnails::ensure_thumbnail(&path_for_thread), None);
        }

        let Ok(hash) = db::hash_file(&path_for_thread) else {
            return (None, None);
        };
        let thumb = thumbnails::generate_hash_thumbnail_for_size(&path_for_thread, &hash, size);
        (thumb, Some(hash))
    });

    let image_weak = thumb_image.downgrade();
    glib::MainContext::default().spawn_local(async move {
        THUMB_UI_CALLBACKS_TOTAL.fetch_add(1, AtomicOrdering::Relaxed);
        let Ok((maybe_cache, resolved_hash)) = task.await else { return };
        if PREVIEW_REQUEST_PENDING.load(AtomicOrdering::Relaxed) != 0 {
            THUMB_UI_CALLBACKS_SKIPPED_WHILE_PREVIEW.fetch_add(1, AtomicOrdering::Relaxed);
            return;
        }
        let Some(image) = image_weak.upgrade() else { return };
        let is_current = unsafe {
            image
                .data::<String>("bound-path")
                .map(|p| p.as_ref().as_str() == path_str.as_str())
                .unwrap_or(false)
        };
        if !is_current {
            return;
        }
        if let Some(hash) = resolved_hash {
            hash_cache.borrow_mut().insert(path_str.clone(), hash);
        }
        match maybe_cache.and_then(|p| gdk_pixbuf::Pixbuf::from_file(&p).ok()) {
            Some(pb) => {
                let tex = gdk::Texture::for_pixbuf(&pb);
                image.set_paintable(Some(&tex));
            }
            None => image.set_icon_name(Some("image-missing-symbolic")),
        }
    });
}

struct PreviewLoadMetrics {
    outcome: PreviewLoadOutcome,
    queue_wait_ms: f64,
    file_open_ms: f64,
    decode_ms: f64,
    texture_create_ms: f64,
    worker_total_ms: f64,
    worker_done_since_enqueue_ms: f64,
    main_thread_dispatch_ms: f64,
    texture_apply_ms: f64,
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

fn write_timing_report(_report: &str) {
    // Timing debug output disabled.
}

fn emit_click_report(trace: &ClickTrace) {
    let mut report = String::new();
    report.push_str(&format!(
        "CLICK {} | {} | outcome={}\n",
        trace.id, trace.path, trace.outcome
    ));
    for step in &trace.steps {
        report.push_str(&format!("{:>8.3}ms {}\n", step.elapsed_ms, step.name));
    }
    report.push_str("METRICS\n");
    report.push_str(&format!(
        "active_thumbnail_jobs_at_click={}\n",
        trace.active_thumbnail_jobs_at_click
    ));
    report.push_str(&format!(
        "active_preview_jobs_at_click={}\n",
        trace.active_preview_jobs_at_click
    ));
    report.push_str(&format!(
        "scan_buffer_depth_at_click={}\n",
        trace.scan_buffer_depth_at_click
    ));
    report.push_str(&format!(
        "idle_drain_scheduled_at_click={}\n",
        trace.idle_drain_scheduled_at_click
    ));
    report.push_str(&format!(
        "pending_idle_drain_cycles_est_at_click={}\n",
        trace.pending_idle_drain_cycles_est_at_click
    ));
    report.push_str(&format!(
        "thumb_ui_callbacks_total_at_click={}\n",
        trace.thumb_ui_callbacks_total_at_click
    ));
    report.push_str(&format!(
        "thumb_ui_callbacks_skipped_at_click={}\n",
        trace.thumb_ui_callbacks_skipped_at_click
    ));
    if let Some(v) = trace.thumb_ui_callbacks_total_until_preview {
        report.push_str(&format!("thumb_ui_callbacks_total_until_preview={}\n", v));
    }
    if let Some(v) = trace.thumb_ui_callbacks_skipped_until_preview {
        report.push_str(&format!("thumb_ui_callbacks_skipped_until_preview={}\n", v));
    }
    if let Some(v) = trace.scan_buffer_depth_at_idle_settled {
        report.push_str(&format!("scan_buffer_depth_at_idle_settled={}\n", v));
    }
    if let Some(v) = trace.idle_drain_scheduled_at_idle_settled {
        report.push_str(&format!("idle_drain_scheduled_at_idle_settled={}\n", v));
    }
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
    if let Some(v) = trace.main_loop_settle_ms {
        report.push_str(&format!("main_loop_settle_ms={:.3}\n", v));
    }
    report.push_str(&format!("TOTAL {:>8.3}ms\n\n", trace.total_ms()));

    write_timing_report(&report);
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

fn mark_click_step(trace_state: &Rc<RefCell<Option<ClickTrace>>>, click_id: u64, step: &str) {
    if let Some(trace) = trace_state.borrow_mut().as_mut() {
        if trace.id == click_id && !trace.finished {
            trace.mark_step(step);
        }
    }
}

fn try_finalize_click_trace(trace_state: &Rc<RefCell<Option<ClickTrace>>>, click_id: u64) {
    let should_schedule = {
        let mut state = trace_state.borrow_mut();
        if let Some(trace) = state.as_mut() {
            if trace.id == click_id
                && trace.preview_done
                && trace.metadata_done
                && !trace.finished
                && !trace.finalize_scheduled
            {
                trace.finalize_scheduled = true;
                true
            } else {
                false
            }
        } else {
            false
        }
    };

    if !should_schedule {
        return;
    }

    // Wait roughly one frame to allow the latest UI updates to paint,
    // without relying on low-priority idle handlers that can starve.
    let trace_state_idle = trace_state.clone();
    glib::timeout_add_local_once(Duration::from_millis(16), move || {
        if let Some(trace) = trace_state_idle.borrow_mut().as_mut() {
            if trace.id == click_id
                && trace.preview_done
                && trace.metadata_done
                && !trace.finished
            {
                let ui_idle_ms = trace.started.elapsed().as_secs_f64() * 1000.0;
                trace.mark_step("ui_idle_settled");
                if let Some(preview_ms) = trace.preview_displayed_at_ms {
                    trace.main_loop_settle_ms = Some((ui_idle_ms - preview_ms).max(0.0));
                }
                trace.scan_buffer_depth_at_idle_settled =
                    Some(SCAN_BUFFER_DEPTH.load(AtomicOrdering::Relaxed));
                trace.idle_drain_scheduled_at_idle_settled =
                    Some(SCAN_DRAIN_SCHEDULED.load(AtomicOrdering::Relaxed) != 0);
                trace.outcome = "done".to_string();
                trace.finished = true;
                emit_click_report(trace);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// UI construction
// ---------------------------------------------------------------------------

fn build_ui(app: &adw::Application) {
    let window = adw::ApplicationWindow::new(app);
    window.set_title(Some("LumenNode"));
    window.set_default_size(1280, 800);

    // Load persisted config (last folder).
    let app_config = config::load();

    // Tracks the most recently scanned folder for config persistence.
    let current_folder: Rc<RefCell<Option<std::path::PathBuf>>> =
        Rc::new(RefCell::new(None));

    // Shared model: each item holds the absolute path of one image.
    let list_store = gio::ListStore::new::<StringObject>();

    // Async channel: background scan thread → GTK main thread.
    // Bounded to provide backpressure when the UI can't keep up.
    let (sender, receiver) = async_channel::bounded::<ScanMessage>(200);

    // ViewStack — toggled programmatically (no visible tab switcher).
    let view_stack = adw::ViewStack::new();

    // Scan-progress indicator (shown in header while a scan is running).
    let spinner = Spinner::new();

    // Toast overlay wraps all main content for non-intrusive notifications.
    let toast_overlay = adw::ToastOverlay::new();

    // Hash cache: path → content hash (for hash-based thumbnail lookup).
    let hash_cache: Rc<RefCell<HashMap<String, String>>> =
        Rc::new(RefCell::new(HashMap::new()));

    // Metadata cache: path → extracted metadata (for search filtering).
    let meta_cache: Rc<RefCell<HashMap<String, ImageMetadata>>> =
        Rc::new(RefCell::new(HashMap::new()));

    // Sort key: "name_asc" | "name_desc" | "date_asc" | "date_desc" | "size_asc" | "size_desc"
    let sort_key: Rc<RefCell<String>> = Rc::new(RefCell::new(
        app_config.sort_key.clone().unwrap_or_else(|| "name_asc".to_string()),
    ));

    // Search text.
    let search_text: Rc<RefCell<String>> = Rc::new(RefCell::new(
        app_config.search_text.clone().unwrap_or_default(),
    ));

    let initial_thumbnail_size =
        normalize_thumbnail_size(app_config.thumbnail_size.unwrap_or(thumbnails::THUMB_NORMAL_SIZE));
    let thumbnail_size: Rc<RefCell<i32>> = Rc::new(RefCell::new(initial_thumbnail_size));

    // -----------------------------------------------------------------------
    // Receiver task: buffer messages and drain in idle-priority batches
    // -----------------------------------------------------------------------
    let list_store_recv = list_store.clone();
    let spinner_recv = spinner.clone();
    let toast_recv = toast_overlay.clone();
    let meta_cache_recv = meta_cache.clone();
    let hash_cache_recv = hash_cache.clone();

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
        let spinner_recv = spinner_recv.clone();
        let toast_recv = toast_recv.clone();
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
            let spinner_recv = spinner_recv.clone();
            let toast_recv = toast_recv.clone();
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

                for msg in batch {
                    match msg {
                        ScanMessage::ImageEnumerated { path } => {
                            new_paths.push(StringObject::new(&path));
                        }
                        ScanMessage::EnumerationComplete => {
                            // Nothing special needed on the UI side.
                        }
                        ScanMessage::ImageEnriched { path, hash, meta } => {
                            if !hash.is_empty() {
                                hash_cache_recv
                                    .borrow_mut()
                                    .insert(path.clone(), hash);
                            }
                            meta_cache_recv.borrow_mut().insert(path, meta);
                        }
                        ScanMessage::ScanComplete => {
                            scan_complete = true;
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

                if scan_complete {
                    spinner_recv.stop();
                    let n = list_store_recv.n_items();
                    let text = format!(
                        "Found {} image{}",
                        n,
                        if n == 1 { "" } else { "s" }
                    );
                    let toast = adw::Toast::new(&text);
                    toast.set_timeout(3);
                    toast_recv.add_toast(toast);
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
    let header_bar = adw::HeaderBar::new();

    // "Open Folder" button in the start slot.
    let open_btn = gtk4::Button::from_icon_name("folder-open-symbolic");
    open_btn.set_tooltip_text(Some("Open Folder…"));
    header_bar.pack_start(&open_btn);

    // --- Sort dropdown ---
    let sort_options = gtk4::StringList::new(&[
        "Name ↑",
        "Name ↓",
        "Date ↑",
        "Date ↓",
        "Size ↑",
        "Size ↓",
    ]);
    let sort_dropdown = gtk4::DropDown::new(Some(sort_options), gtk4::Expression::NONE);
    sort_dropdown.set_tooltip_text(Some("Sort order"));

    // --- Thumbnail size toggles ---
    let size_options = thumbnail_size_options();
    let size_selector = gtk4::Box::new(Orientation::Horizontal, 0);
    size_selector.add_css_class("linked");
    size_selector.set_tooltip_text(Some("Thumbnail size"));
    let size_labels = ["1x", "1.3x", "1.6x", "1.9x"];
    let mut size_buttons_vec = Vec::new();
    for (idx, px) in size_options.iter().enumerate() {
        let btn = gtk4::ToggleButton::with_label(size_labels[idx]);
        btn.set_tooltip_text(Some(&format!("{} px", px)));
        btn.set_active(*px == initial_thumbnail_size);
        size_selector.append(&btn);
        size_buttons_vec.push(btn);
    }
    let size_buttons = Rc::new(size_buttons_vec);

    // --- Search entry ---
    let search_entry = gtk4::SearchEntry::new();
    search_entry.set_placeholder_text(Some("Search…"));
    search_entry.set_width_request(220);

    // --- Clear button ---
    let clear_btn = gtk4::Button::from_icon_name("edit-clear-symbolic");
    clear_btn.set_tooltip_text(Some("Clear filters"));

    // Center widget: sort + search + clear grouped together.
    let toolbar_center = gtk4::Box::new(Orientation::Horizontal, 6);
    toolbar_center.set_valign(gtk4::Align::Center);
    toolbar_center.append(&sort_dropdown);
    toolbar_center.append(&size_selector);
    toolbar_center.append(&search_entry);
    toolbar_center.append(&clear_btn);
    header_bar.set_title_widget(Some(&toolbar_center));

    // Spinner in the end slot — visible while scanning.
    header_bar.pack_end(&spinner);

    // Sidebar toggle buttons — collapse/expand left and right panels.
    let left_toggle = gtk4::ToggleButton::new();
    left_toggle.set_icon_name("sidebar-show-symbolic");
    left_toggle.set_active(true);
    left_toggle.set_tooltip_text(Some("Toggle left panel"));
    header_bar.pack_start(&left_toggle);

    let right_toggle = gtk4::ToggleButton::new();
    right_toggle.set_icon_name("sidebar-show-right-symbolic");
    right_toggle.set_active(true);
    right_toggle.set_tooltip_text(Some("Toggle right panel"));
    header_bar.pack_end(&right_toggle);

    // -----------------------------------------------------------------------
    // Three-pane layout: [left sidebar] | [center] | [right sidebar]
    // -----------------------------------------------------------------------
    // --- Left sidebar: file system tree ---
    let left_sidebar = gtk4::Box::new(Orientation::Vertical, 0);
    left_sidebar.set_width_request(200);

    // Root items: home directory + real mount points.
    let tree_root = build_tree_root();

    // TreeListModel lazily loads subdirectories when a node is expanded.
    let tree_model = TreeListModel::new(tree_root, false, false, move |item: &glib::Object| -> Option<gio::ListModel> {
        let file = item.downcast_ref::<gio::File>()?;
        let store = gio::ListStore::new::<gio::File>();
        if let Ok(enumerator) = file.enumerate_children(
            "standard::name,standard::type",
            gio::FileQueryInfoFlags::NONE,
            None::<&gio::Cancellable>,
        ) {
            let mut children: Vec<gio::FileInfo> = enumerator
                .filter_map(|r| r.ok())
                .filter(|info| {
                    info.file_type() == gio::FileType::Directory
                        && !info.name().to_string_lossy().starts_with('.')
                })
                .collect();
            children.sort_by_key(|info| info.name().to_string_lossy().to_lowercase().to_string());
            for info in children {
                store.append(&file.child(info.name()));
            }
        }
        if store.n_items() > 0 { Some(store.upcast::<gio::ListModel>()) } else { None }
    });

    let tree_selection = SingleSelection::new(Some(tree_model.clone()));

    // Wire tree folder selection → clear grid, start scan.
    let sender_tree = sender.clone();
    let list_store_tree = list_store.clone();
    let spinner_tree = spinner.clone();
    let current_folder_tree = current_folder.clone();
    let meta_cache_tree = meta_cache.clone();
    let hash_cache_tree = hash_cache.clone();
    let sort_key_tree = sort_key.clone();
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
        *current_folder_tree.borrow_mut() = Some(path.clone());
        list_store_tree.remove_all();
        meta_cache_tree.borrow_mut().clear();
        hash_cache_tree.borrow_mut().clear();
        spinner_tree.start();
        scan_directory(path, sender_tree.clone(), sort_key_tree.borrow().clone());
    });

    let tree_factory = SignalListItemFactory::new();
    tree_factory.connect_setup(|_, obj| {
        let list_item = obj.downcast_ref::<ListItem>().unwrap();
        let expander = TreeExpander::new();
        let row_box = gtk4::Box::new(Orientation::Horizontal, 4);
        row_box.set_margin_top(3);
        row_box.set_margin_bottom(3);
        let icon = Image::from_icon_name("folder-symbolic");
        let label = Label::new(None);
        label.set_halign(gtk4::Align::Start);
        label.set_hexpand(true);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
        row_box.append(&icon);
        row_box.append(&label);
        expander.set_child(Some(&row_box));
        list_item.set_child(Some(&expander));
    });
    tree_factory.connect_bind(|_, obj| {
        let list_item = obj.downcast_ref::<ListItem>().unwrap();
        let expander = list_item.child().and_downcast::<TreeExpander>().unwrap();
        let row = list_item.item().and_downcast::<TreeListRow>().unwrap();
        expander.set_list_row(Some(&row));
        let file = row.item().and_downcast::<gio::File>().unwrap();
        let row_box = expander.child().and_downcast::<gtk4::Box>().unwrap();
        let label = row_box.last_child().and_downcast::<Label>().unwrap();
        let name = if let Some(p) = file.path() {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.to_string_lossy().into_owned())
        } else {
            file.uri().to_string()
        };
        label.set_text(&name);
    });

    let tree_list_view = ListView::new(Some(tree_selection), Some(tree_factory));
    tree_list_view.add_css_class("navigation-sidebar");
    // Disable natural-width propagation so the ScrolledWindow can clip the
    // ListView and show a horizontal scrollbar for deeply-nested long names.
    tree_list_view.set_hexpand(false);

    let tree_scroll = ScrolledWindow::new();
    tree_scroll.set_vexpand(true);
    tree_scroll.set_hscrollbar_policy(gtk4::PolicyType::Automatic);
    tree_scroll.set_propagate_natural_width(false);
    tree_scroll.set_child(Some(&tree_list_view));
    left_sidebar.append(&tree_scroll);

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
        let ord = match key.as_str() {
            "name_asc" | "name_desc" => {
                let na = std::path::Path::new(&path_a)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                let nb = std::path::Path::new(&path_b)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                let cmp = na.cmp(&nb);
                if key == "name_desc" { cmp.reverse() } else { cmp }
            }
            "date_asc" | "date_desc" => {
                let mt = |p: &str| {
                    std::fs::metadata(p)
                        .and_then(|m| m.modified())
                        .ok()
                };
                let ta = mt(&path_a);
                let tb = mt(&path_b);
                let cmp = ta.cmp(&tb);
                if key == "date_desc" { cmp.reverse() } else { cmp }
            }
            "size_asc" | "size_desc" => {
                let sz = |p: &str| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                let cmp = sz(&path_a).cmp(&sz(&path_b));
                if key == "size_desc" { cmp.reverse() } else { cmp }
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

    let factory = SignalListItemFactory::new();
    let realized_thumb_images: Rc<RefCell<Vec<glib::WeakRef<Image>>>> =
        Rc::new(RefCell::new(Vec::new()));
    let realized_cell_boxes: Rc<RefCell<Vec<glib::WeakRef<gtk4::Box>>>> =
        Rc::new(RefCell::new(Vec::new()));

    let thumbnail_size_setup = thumbnail_size.clone();
    let realized_thumb_images_setup = realized_thumb_images.clone();
    let realized_cell_boxes_setup = realized_cell_boxes.clone();
    factory.connect_setup(move |_, obj| {
        let list_item = obj.downcast_ref::<ListItem>().unwrap();
        let cell_box = gtk4::Box::new(Orientation::Vertical, 4);
        cell_box.set_halign(gtk4::Align::Center);
        let size = *thumbnail_size_setup.borrow();
        cell_box.set_size_request(size + 4, size + 20);
        let thumb_image = Image::new();
        thumb_image.set_pixel_size(size);
        realized_cell_boxes_setup.borrow_mut().push(cell_box.downgrade());
        realized_thumb_images_setup.borrow_mut().push(thumb_image.downgrade());
        let name_label = Label::new(None);
        name_label.set_max_width_chars(16);
        name_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        name_label.add_css_class("caption");
        cell_box.append(&thumb_image);
        cell_box.append(&name_label);
        list_item.set_child(Some(&cell_box));
    });

    let hash_cache_bind = hash_cache.clone();
    let thumbnail_size_bind = thumbnail_size.clone();
    factory.connect_bind(move |_, obj| {
        let list_item = obj.downcast_ref::<ListItem>().unwrap();
        let path_str = list_item
            .item()
            .and_downcast::<StringObject>()
            .map(|s| s.string().to_string())
            .unwrap_or_default();

        let cell_box = list_item.child().and_downcast::<gtk4::Box>().unwrap();
        let thumb_image = cell_box.first_child().and_downcast::<Image>().unwrap();
        let name_label = cell_box.last_child().and_downcast::<Label>().unwrap();
        let size = *thumbnail_size_bind.borrow();
        cell_box.set_size_request(size + 4, size + 20);
        thumb_image.set_pixel_size(size);

        // Set filename label and placeholder icon synchronously (zero I/O).
        let filename = std::path::Path::new(&path_str)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        name_label.set_text(&filename);
        load_grid_thumbnail(&thumb_image, path_str, size, hash_cache_bind.clone());
    });

    factory.connect_unbind(|_, obj| {
        let list_item = obj.downcast_ref::<ListItem>().unwrap();
        if let Some(cell_box) = list_item.child().and_downcast::<gtk4::Box>() {
            if let Some(image) = cell_box.first_child().and_downcast::<Image>() {
                unsafe { image.steal_data::<String>("bound-path"); }
                image.set_icon_name(Some("image-x-generic-symbolic"));
            }
        }
    });

    let grid_view = GridView::new(Some(selection_model.clone()), Some(factory));
    grid_view.set_max_columns(12);
    grid_view.set_min_columns(2);

    let grid_scroll = ScrolledWindow::new();
    grid_scroll.set_vexpand(true);
    grid_scroll.set_hexpand(true);
    grid_scroll.set_child(Some(&grid_view));

    // add_titled returns ViewStackPage — use it to set the page icon.
    let grid_page = view_stack.add_titled(&grid_scroll, Some("grid"), "Grid");
    grid_page.set_icon_name(Some("view-grid-symbolic"));

    let single_picture = Picture::new();
    single_picture.set_vexpand(true);
    single_picture.set_hexpand(true);
    single_picture.set_can_shrink(true);
    let single_page = view_stack.add_titled(&single_picture, Some("single"), "Single");
    single_page.set_icon_name(Some("view-fullscreen-symbolic"));
    view_stack.set_visible_child_name("grid");

    let center_box = gtk4::Box::new(Orientation::Vertical, 0);
    center_box.set_hexpand(true);
    center_box.append(&view_stack);

    // --- Right sidebar: preview (top) + metadata list (bottom) ---
    let right_sidebar = gtk4::Box::new(Orientation::Vertical, 0);
    right_sidebar.set_width_request(260);
    right_sidebar.set_margin_top(0);
    right_sidebar.set_margin_bottom(0);
    right_sidebar.set_margin_start(0);
    right_sidebar.set_margin_end(0);

    // Top pane: image preview
    let meta_preview = Picture::new();
    meta_preview.set_vexpand(true);
    meta_preview.set_hexpand(true);
    meta_preview.set_can_shrink(true);

    // Bottom pane: metadata list
    let meta_content = gtk4::Box::new(Orientation::Vertical, 6);
    meta_content.set_vexpand(true);
    meta_content.set_margin_top(12);
    meta_content.set_margin_bottom(12);
    meta_content.set_margin_start(4);
    meta_content.set_margin_end(8);

    let meta_title = Label::new(Some("Metadata"));
    meta_title.add_css_class("title-4");
    meta_title.set_halign(gtk4::Align::Start);
    meta_content.append(&meta_title);

    let meta_scroll = ScrolledWindow::new();
    meta_scroll.set_vexpand(true);
    let meta_listbox = gtk4::ListBox::new();
    meta_listbox.add_css_class("boxed-list");
    meta_listbox.set_selection_mode(gtk4::SelectionMode::None);
    meta_scroll.set_child(Some(&meta_listbox));
    meta_content.append(&meta_scroll);

    // Vertical paned: preview (top) | metadata (bottom)
    let meta_paned = Paned::new(Orientation::Vertical);
    meta_paned.set_vexpand(true);
    meta_paned.set_start_child(Some(&meta_preview));
    meta_paned.set_end_child(Some(&meta_content));
    meta_paned.set_resize_start_child(true);
    meta_paned.set_resize_end_child(true);
    meta_paned.set_shrink_start_child(false);
    meta_paned.set_shrink_end_child(false);
    meta_paned.set_position(app_config.meta_pane_pos.unwrap_or(200));
    right_sidebar.append(&meta_paned);

    // -----------------------------------------------------------------------
    // Context menu: actions + menu model + right-click attachment
    // -----------------------------------------------------------------------
    let action_group = gio::SimpleActionGroup::new();

    let copy_prompt_action = gio::SimpleAction::new("copy-prompt", None);
    let copy_negative_prompt_action = gio::SimpleAction::new("copy-negative-prompt", None);
    let copy_seed_action = gio::SimpleAction::new("copy-seed", None);
    let copy_generation_command_action = gio::SimpleAction::new("copy-generation-command", None);
    let copy_image_action = gio::SimpleAction::new("copy-image", None);
    let copy_path_action = gio::SimpleAction::new("copy-path", None);
    let copy_metadata_action = gio::SimpleAction::new("copy-metadata", None);
    let refresh_thumb_action = gio::SimpleAction::new("refresh-thumbnail", None);
    let refresh_meta_action = gio::SimpleAction::new("refresh-metadata", None);
    let refresh_folder_thumbs_action =
        gio::SimpleAction::new("refresh-folder-thumbnails", None);
    let refresh_folder_meta_action =
        gio::SimpleAction::new("refresh-folder-metadata", None);

    let selection_for_actions = selection_model.clone();
    let meta_cache_for_actions = meta_cache.clone();
    let window_for_actions = window.clone();
    copy_prompt_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let path_key = path.to_string_lossy().to_string();
        let text = meta_cache_for_actions
            .borrow()
            .get(&path_key)
            .and_then(|meta| meta.prompt.clone())
            .unwrap_or_else(|| "No prompt found".to_string());
        gtk4::prelude::WidgetExt::display(&window_for_actions)
            .clipboard()
            .set_text(&text);
    });

    let selection_for_actions = selection_model.clone();
    let meta_cache_for_actions = meta_cache.clone();
    let window_for_actions = window.clone();
    copy_negative_prompt_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let path_key = path.to_string_lossy().to_string();
        let text = meta_cache_for_actions
            .borrow()
            .get(&path_key)
            .and_then(|meta| meta.negative_prompt.clone())
            .unwrap_or_else(|| "No negative prompt found".to_string());
        gtk4::prelude::WidgetExt::display(&window_for_actions)
            .clipboard()
            .set_text(&text);
    });

    let selection_for_actions = selection_model.clone();
    let meta_cache_for_actions = meta_cache.clone();
    let window_for_actions = window.clone();
    copy_seed_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let path_key = path.to_string_lossy().to_string();
        let text = meta_cache_for_actions
            .borrow()
            .get(&path_key)
            .and_then(|meta| extract_seed_from_parameters(&meta))
            .unwrap_or_else(|| "No seed found".to_string());
        gtk4::prelude::WidgetExt::display(&window_for_actions)
            .clipboard()
            .set_text(&text);
    });

    let selection_for_actions = selection_model.clone();
    let meta_cache_for_actions = meta_cache.clone();
    let window_for_actions = window.clone();
    copy_generation_command_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let path_key = path.to_string_lossy().to_string();
        let text = meta_cache_for_actions
            .borrow()
            .get(&path_key)
            .map(|meta| format_generation_command(meta))
            .unwrap_or_else(|| "No generation parameters found".to_string());
        gtk4::prelude::WidgetExt::display(&window_for_actions)
            .clipboard()
            .set_text(&text);
    });

    let selection_for_actions = selection_model.clone();
    let window_for_actions = window.clone();
    copy_image_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let file = gio::File::for_path(&path);
        if let Ok(texture) = gdk::Texture::from_file(&file) {
            gtk4::prelude::WidgetExt::display(&window_for_actions)
                .clipboard()
                .set_texture(&texture);
        }
    });

    let selection_for_actions = selection_model.clone();
    let window_for_actions = window.clone();
    copy_path_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        gtk4::prelude::WidgetExt::display(&window_for_actions)
            .clipboard()
            .set_text(&path.to_string_lossy());
    });

    let selection_for_actions = selection_model.clone();
    let meta_cache_for_actions = meta_cache.clone();
    let window_for_actions = window.clone();
    copy_metadata_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let path_key = path.to_string_lossy().to_string();
        let text = meta_cache_for_actions
            .borrow()
            .get(&path_key)
            .map(format_metadata_text)
            .unwrap_or_else(|| "No metadata found".to_string());
        gtk4::prelude::WidgetExt::display(&window_for_actions)
            .clipboard()
            .set_text(&text);
    });

    let selection_for_actions = selection_model.clone();
    let hash_cache_for_actions = hash_cache.clone();
    let toast_overlay_for_actions = toast_overlay.clone();
    let thumbnail_size_for_actions = thumbnail_size.clone();
    refresh_thumb_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let hash = hash_cache_for_actions
            .borrow()
            .get(&path.to_string_lossy().to_string())
            .cloned()
            .or_else(|| db::hash_file(&path).ok());
        let Some(hash) = hash else { return };

        thumbnails::remove_hash_thumbnail_variants(&hash);
        let _ = thumbnails::generate_hash_thumbnail(&path, &hash);
        let current_size = *thumbnail_size_for_actions.borrow();
        if current_size != thumbnails::THUMB_NORMAL_SIZE {
            let _ = thumbnails::generate_hash_thumbnail_for_size(&path, &hash, current_size);
        }
        hash_cache_for_actions
            .borrow_mut()
            .insert(path.to_string_lossy().to_string(), hash);

        let toast = adw::Toast::new("Thumbnail refreshed");
        toast.set_timeout(2);
        toast_overlay_for_actions.add_toast(toast);
    });

    let selection_for_actions = selection_model.clone();
    let meta_cache_for_actions = meta_cache.clone();
    let hash_cache_for_actions = hash_cache.clone();
    let meta_listbox_for_actions = meta_listbox.clone();
    let toast_overlay_for_actions = toast_overlay.clone();
    refresh_meta_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let Some(folder) = path.parent().map(|p| p.to_path_buf()) else {
            return;
        };

        let Ok(conn) = db::open(&folder) else {
            return;
        };
        if let Some(row) = db::refresh_indexed(&conn, &path) {
            let path_key = path.to_string_lossy().to_string();
            meta_cache_for_actions
                .borrow_mut()
                .insert(path_key.clone(), row.meta.clone());
            hash_cache_for_actions
                .borrow_mut()
                .insert(path_key, row.hash);
            populate_metadata_sidebar(&meta_listbox_for_actions, &row.meta);

            let toast = adw::Toast::new("Metadata refreshed");
            toast.set_timeout(2);
            toast_overlay_for_actions.add_toast(toast);
        }
    });

    let current_folder_for_actions = current_folder.clone();
    let list_store_for_actions = list_store.clone();
    let hash_cache_for_actions = hash_cache.clone();
    let meta_cache_for_actions = meta_cache.clone();
    let spinner_for_actions = spinner.clone();
    let sender_for_actions = sender.clone();
    let sort_key_for_actions = sort_key.clone();
    refresh_folder_thumbs_action.connect_activate(move |_, _| {
        let Some(folder) = current_folder_for_actions.borrow().as_ref().cloned() else {
            return;
        };

        // Force thumbnail regeneration by deleting existing hash-based cache files.
        let cached_hashes: Vec<String> =
            hash_cache_for_actions.borrow().values().cloned().collect();
        for hash in cached_hashes {
            thumbnails::remove_hash_thumbnail_variants(&hash);
        }

        list_store_for_actions.remove_all();
        hash_cache_for_actions.borrow_mut().clear();
        meta_cache_for_actions.borrow_mut().clear();
        spinner_for_actions.start();
        scan_directory(folder, sender_for_actions.clone(), sort_key_for_actions.borrow().clone());
    });

    let current_folder_for_actions = current_folder.clone();
    let list_store_for_actions = list_store.clone();
    let hash_cache_for_actions = hash_cache.clone();
    let meta_cache_for_actions = meta_cache.clone();
    let spinner_for_actions = spinner.clone();
    let sender_for_actions = sender.clone();
    let sort_key_for_actions = sort_key.clone();
    refresh_folder_meta_action.connect_activate(move |_, _| {
        let Some(folder) = current_folder_for_actions.borrow().as_ref().cloned() else {
            return;
        };

        let mut paths = Vec::new();
        for i in 0..list_store_for_actions.n_items() {
            if let Some(item) = list_store_for_actions.item(i).and_downcast::<StringObject>() {
                paths.push(std::path::PathBuf::from(item.string().as_str()));
            }
        }

        if let Ok(conn) = db::open(&folder) {
            for p in &paths {
                let _ = db::refresh_indexed(&conn, p);
            }
        }

        list_store_for_actions.remove_all();
        hash_cache_for_actions.borrow_mut().clear();
        meta_cache_for_actions.borrow_mut().clear();
        spinner_for_actions.start();
        scan_directory(folder, sender_for_actions.clone(), sort_key_for_actions.borrow().clone());
    });

    action_group.add_action(&copy_prompt_action);
    action_group.add_action(&copy_negative_prompt_action);
    action_group.add_action(&copy_seed_action);
    action_group.add_action(&copy_generation_command_action);
    action_group.add_action(&copy_image_action);
    action_group.add_action(&copy_path_action);
    action_group.add_action(&copy_metadata_action);
    action_group.add_action(&refresh_thumb_action);
    action_group.add_action(&refresh_meta_action);
    action_group.add_action(&refresh_folder_thumbs_action);
    action_group.add_action(&refresh_folder_meta_action);
    window.insert_action_group("ctx", Some(&action_group));

    let menu_model = gio::Menu::new();
    let prompt_section = gio::Menu::new();
    prompt_section.append(Some("Copy Prompt"), Some("ctx.copy-prompt"));
    prompt_section.append(Some("Copy Negative Prompt"), Some("ctx.copy-negative-prompt"));
    prompt_section.append(Some("Copy Seed"), Some("ctx.copy-seed"));
    prompt_section.append(Some("Copy Generation Command"), Some("ctx.copy-generation-command"));
    menu_model.append_section(None, &prompt_section);

    let clipboard_section = gio::Menu::new();
    clipboard_section.append(Some("Copy Image"), Some("ctx.copy-image"));
    clipboard_section.append(Some("Copy Path"), Some("ctx.copy-path"));
    clipboard_section.append(Some("Copy Metadata"), Some("ctx.copy-metadata"));
    menu_model.append_section(None, &clipboard_section);

    let refresh_submenu = gio::Menu::new();
    refresh_submenu.append(Some("Refresh Thumbnail"), Some("ctx.refresh-thumbnail"));
    refresh_submenu.append(Some("Refresh Metadata"), Some("ctx.refresh-metadata"));
    refresh_submenu.append(
        Some("Refresh Folder Thumbnails"),
        Some("ctx.refresh-folder-thumbnails"),
    );
    refresh_submenu.append(
        Some("Refresh Folder Metadata"),
        Some("ctx.refresh-folder-metadata"),
    );
    menu_model.append_submenu(Some("Refresh"), &refresh_submenu);

    attach_context_menu(&grid_view, &menu_model);
    attach_context_menu(&single_picture, &menu_model);
    attach_context_menu(&meta_preview, &menu_model);

    // -----------------------------------------------------------------------
    // Wire: sidebar toggle buttons → show/hide panels
    // -----------------------------------------------------------------------
    let left_sidebar_toggle = left_sidebar.clone();
    left_toggle.connect_toggled(move |btn| {
        left_sidebar_toggle.set_visible(btn.is_active());
    });

    let right_sidebar_toggle = right_sidebar.clone();
    right_toggle.connect_toggled(move |btn| {
        right_sidebar_toggle.set_visible(btn.is_active());
    });

    // -----------------------------------------------------------------------
    // Wire: grid item activate → switch to single view
    // -----------------------------------------------------------------------
    let stack_for_grid = view_stack.clone();
    let picture_for_grid = single_picture.clone();
    let selection_for_grid = selection_model.clone();
    let left_toggle_grid = left_toggle.clone();
    let right_toggle_grid = right_toggle.clone();
    grid_view.connect_activate(move |_, pos| {
        if let Some(item) = selection_for_grid.item(pos).and_downcast::<StringObject>() {
            let path_str = item.string().to_string();
            let full_view_id = FULL_VIEW_TRACE_COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
            let active_thumbnail_jobs_at_activate =
                ACTIVE_THUMBNAIL_TASKS.load(AtomicOrdering::Relaxed);
            let active_preview_jobs_at_activate =
                ACTIVE_PREVIEW_TASKS.load(AtomicOrdering::Relaxed);
            let trace = Rc::new(RefCell::new(FullViewTrace::new(
                full_view_id,
                path_str.clone(),
                active_thumbnail_jobs_at_activate,
                active_preview_jobs_at_activate,
            )));
            let trace_for_cb = trace.clone();

            load_picture_async(
                &picture_for_grid,
                &path_str,
                None,
                Some(Box::new(move |metrics| {
                    let mut t = trace_for_cb.borrow_mut();
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
                })),
            );
        }
        stack_for_grid.set_visible_child_name("single");
        left_toggle_grid.set_active(false);
        right_toggle_grid.set_active(false);
    });

    // -----------------------------------------------------------------------
    // Wire: selection change → populate metadata sidebar
    // -----------------------------------------------------------------------
    let meta_listbox_sel = meta_listbox.clone();
    let meta_preview_sel = meta_preview.clone();
    let meta_cache_sel = meta_cache.clone();
    let click_trace_state: Rc<RefCell<Option<ClickTrace>>> = Rc::new(RefCell::new(None));
    let click_trace_state_sel = click_trace_state.clone();
    selection_model.connect_selection_changed(move |model, _, _| {
        let Some(item) = model.selected_item().and_downcast::<StringObject>() else {
            return;
        };
        let path_str = item.string().to_string();
        let click_id = CLICK_TRACE_COUNTER.fetch_add(1, AtomicOrdering::Relaxed);
        let active_thumbnail_jobs_at_click =
            ACTIVE_THUMBNAIL_TASKS.load(AtomicOrdering::Relaxed);
        let active_preview_jobs_at_click =
            ACTIVE_PREVIEW_TASKS.load(AtomicOrdering::Relaxed);
        let scan_buffer_depth_at_click = SCAN_BUFFER_DEPTH.load(AtomicOrdering::Relaxed);
        let idle_drain_scheduled_at_click =
            SCAN_DRAIN_SCHEDULED.load(AtomicOrdering::Relaxed) != 0;
        let thumb_ui_callbacks_total_at_click =
            THUMB_UI_CALLBACKS_TOTAL.load(AtomicOrdering::Relaxed);
        let thumb_ui_callbacks_skipped_at_click =
            THUMB_UI_CALLBACKS_SKIPPED_WHILE_PREVIEW.load(AtomicOrdering::Relaxed);
        let pending_idle_drain_cycles_est_at_click = if scan_buffer_depth_at_click == 0 {
            0
        } else {
            scan_buffer_depth_at_click.div_ceil(SCAN_DRAIN_BATCH_SIZE)
        };

        {
            let mut state = click_trace_state_sel.borrow_mut();
            if let Some(prev) = state.as_mut() {
                if !prev.finished {
                    prev.mark_step("superseded_by_new_click");
                    prev.outcome = "superseded".to_string();
                    prev.finished = true;
                    emit_click_report(prev);
                }
            }
            let mut trace = ClickTrace::new(
                click_id,
                path_str.clone(),
                active_thumbnail_jobs_at_click,
                active_preview_jobs_at_click,
                scan_buffer_depth_at_click,
                idle_drain_scheduled_at_click,
                pending_idle_drain_cycles_est_at_click,
                thumb_ui_callbacks_total_at_click,
                thumb_ui_callbacks_skipped_at_click,
            );
            trace.mark_step("selection_changed");
            trace.mark_step(&format!(
                "active_jobs_captured thumb={} preview={} scan_depth={} drain_scheduled={} thumb_cb_total={} thumb_cb_skipped={}",
                active_thumbnail_jobs_at_click,
                active_preview_jobs_at_click,
                scan_buffer_depth_at_click,
                idle_drain_scheduled_at_click,
                thumb_ui_callbacks_total_at_click,
                thumb_ui_callbacks_skipped_at_click
            ));
            *state = Some(trace);
        }

        mark_click_step(&click_trace_state_sel, click_id, "selected_item_resolved");

        // Load the preview image off-thread so the UI stays responsive.
        // Decode at 2× sidebar width (520px) for fast display on HiDPI.
        mark_click_step(&click_trace_state_sel, click_id, "preview_load_dispatched");
        PREVIEW_REQUEST_PENDING.store(1, AtomicOrdering::Relaxed);
        SUPPRESS_SIDEBAR_DURING_PREVIEW.store(1, AtomicOrdering::Relaxed);
        
        // Load metadata asynchronously (cancellable if user navigates away).
        mark_click_step(&click_trace_state_sel, click_id, "metadata_lookup_started");
        let cache = meta_cache_sel.borrow();
        let meta = cache
            .get(item.string().as_str())
            .cloned()
            .unwrap_or_default();
        mark_click_step(&click_trace_state_sel, click_id, "metadata_lookup_finished");
        mark_click_step(&click_trace_state_sel, click_id, "metadata_render_started");
        
        let click_trace_for_preview = click_trace_state_sel.clone();
        let meta_listbox_for_metadata = meta_listbox_sel.clone();
        let click_trace_for_metadata = click_trace_state_sel.clone();
        
        // Start metadata load in background (non-blocking).
        load_metadata_async(
            meta.clone(),
            meta_listbox_for_metadata,
            click_trace_for_metadata,
            click_id,
        );
        load_picture_async(
            &meta_preview_sel,
            &path_str,
            Some(520),
            Some(Box::new(move |metrics| {
                match metrics.outcome {
                    PreviewLoadOutcome::Displayed => {
                        PREVIEW_REQUEST_PENDING.store(0, AtomicOrdering::Relaxed);
                        SUPPRESS_SIDEBAR_DURING_PREVIEW.store(0, AtomicOrdering::Relaxed);
                        if let Some(trace) = click_trace_for_preview.borrow_mut().as_mut() {
                            if trace.id == click_id && !trace.finished {
                                trace.preview_queue_wait_ms = Some(metrics.queue_wait_ms);
                                trace.preview_file_open_ms = Some(metrics.file_open_ms);
                                trace.preview_decode_ms = Some(metrics.decode_ms);
                                trace.preview_texture_create_ms = Some(metrics.texture_create_ms);
                                trace.preview_worker_total_ms = Some(metrics.worker_total_ms);
                                trace.preview_main_thread_dispatch_ms =
                                    Some(metrics.main_thread_dispatch_ms);
                                trace.preview_texture_apply_ms = Some(metrics.texture_apply_ms);
                                // Mark preview display complete; metadata will complete separately.
                                let thumb_total_now =
                                    THUMB_UI_CALLBACKS_TOTAL.load(AtomicOrdering::Relaxed);
                                let thumb_skipped_now =
                                    THUMB_UI_CALLBACKS_SKIPPED_WHILE_PREVIEW
                                        .load(AtomicOrdering::Relaxed);
                                trace.thumb_ui_callbacks_total_until_preview = Some(
                                    thumb_total_now.saturating_sub(trace.thumb_ui_callbacks_total_at_click),
                                );
                                trace.thumb_ui_callbacks_skipped_until_preview = Some(
                                    thumb_skipped_now.saturating_sub(trace.thumb_ui_callbacks_skipped_at_click),
                                );
                                trace.mark_step("preview_displayed");
                                trace.preview_displayed_at_ms =
                                    Some(trace.started.elapsed().as_secs_f64() * 1000.0);
                                trace.preview_done = true;
                            }
                        }
                    }
                    PreviewLoadOutcome::Failed => {
                        PREVIEW_REQUEST_PENDING.store(0, AtomicOrdering::Relaxed);
                        SUPPRESS_SIDEBAR_DURING_PREVIEW.store(0, AtomicOrdering::Relaxed);
                        if let Some(trace) = click_trace_for_preview.borrow_mut().as_mut() {
                            if trace.id == click_id && !trace.finished {
                                trace.preview_queue_wait_ms = Some(metrics.queue_wait_ms);
                                trace.preview_file_open_ms = Some(metrics.file_open_ms);
                                trace.preview_decode_ms = Some(metrics.decode_ms);
                                trace.preview_texture_create_ms = Some(metrics.texture_create_ms);
                                trace.preview_worker_total_ms = Some(metrics.worker_total_ms);
                                trace.preview_main_thread_dispatch_ms =
                                    Some(metrics.main_thread_dispatch_ms);
                                trace.preview_texture_apply_ms = Some(metrics.texture_apply_ms);
                                trace.mark_step("preview_failed");
                                trace.preview_done = true;
                                // Metadata load continues in background; don't finalize yet.
                            }
                        }
                    }
                    PreviewLoadOutcome::StaleOrCancelled => {
                        PREVIEW_REQUEST_PENDING.store(0, AtomicOrdering::Relaxed);
                        SUPPRESS_SIDEBAR_DURING_PREVIEW.store(0, AtomicOrdering::Relaxed);
                        if let Some(trace) = click_trace_for_preview.borrow_mut().as_mut() {
                            if trace.id == click_id && !trace.finished {
                                trace.preview_queue_wait_ms = Some(metrics.queue_wait_ms);
                                trace.preview_file_open_ms = Some(metrics.file_open_ms);
                                trace.preview_decode_ms = Some(metrics.decode_ms);
                                trace.preview_texture_create_ms = Some(metrics.texture_create_ms);
                                trace.preview_worker_total_ms = Some(metrics.worker_total_ms);
                                trace.preview_main_thread_dispatch_ms =
                                    Some(metrics.main_thread_dispatch_ms);
                                trace.preview_texture_apply_ms = Some(metrics.texture_apply_ms);
                                trace.mark_step("preview_stale_or_cancelled");
                                trace.outcome = "cancelled".to_string();
                                trace.finished = true;
                                emit_click_report(trace);
                            }
                        }
                    }
                }
            })),
        );
    });

    // -----------------------------------------------------------------------
    // Wire: open_btn → FileDialog → start scan (quick-jump shortcut)
    // -----------------------------------------------------------------------
    let sender_btn = sender.clone();
    let list_store_btn = list_store.clone();
    let window_ref = window.clone();
    let spinner_btn = spinner.clone();
    let current_folder_btn = current_folder.clone();
    let meta_cache_btn = meta_cache.clone();
    let hash_cache_btn = hash_cache.clone();
    let sort_key_btn = sort_key.clone();
    open_btn.connect_clicked(move |_| {
        let dialog = gtk4::FileDialog::builder().title("Choose a Folder").build();
        let sender2 = sender_btn.clone();
        let list_store2 = list_store_btn.clone();
        let spinner2 = spinner_btn.clone();
        let cf2 = current_folder_btn.clone();
        let cache2 = meta_cache_btn.clone();
        let hash2 = hash_cache_btn.clone();
        let sk2 = sort_key_btn.clone();
        dialog.select_folder(
            Some(&window_ref),
            None::<&gio::Cancellable>,
            move |result| {
                let Ok(file) = result else { return };
                let Some(path) = file.path() else { return };
                *cf2.borrow_mut() = Some(path.clone());
                list_store2.remove_all();
                cache2.borrow_mut().clear();
                hash2.borrow_mut().clear();
                spinner2.start();
                scan_directory(path, sender2.clone(), sk2.borrow().clone());
            },
        );
    });

    // -----------------------------------------------------------------------
    // Wire: sort dropdown → update sort key and invalidate sorter
    // -----------------------------------------------------------------------
    let sort_key_dd = sort_key.clone();
    let sorter_dd = sorter.clone();
    sort_dropdown.connect_selected_notify(move |dd| {
        let key = match dd.selected() {
            0 => "name_asc",
            1 => "name_desc",
            2 => "date_asc",
            3 => "date_desc",
            4 => "size_asc",
            5 => "size_desc",
            _ => "name_asc",
        };
        *sort_key_dd.borrow_mut() = key.to_string();
        sorter_dd.changed(gtk4::SorterChange::Different);
    });

    // -----------------------------------------------------------------------
    // Wire: search entry → update search text and invalidate filter
    // -----------------------------------------------------------------------
    let search_text_entry = search_text.clone();
    let filter_entry = filter.clone();
    search_entry.connect_search_changed(move |entry| {
        *search_text_entry.borrow_mut() = entry.text().to_lowercase();
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
    clear_btn.connect_clicked(move |_| {
        *search_text_clear.borrow_mut() = String::new();
        *sort_key_clear.borrow_mut() = "name_asc".to_string();
        search_entry_clear.set_text("");
        sort_dropdown_clear.set_selected(0);
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

            {
                let mut boxes = realized_cell_boxes_toggle.borrow_mut();
                boxes.retain(|weak| weak.upgrade().is_some());
                for weak in boxes.iter() {
                    if let Some(cell_box) = weak.upgrade() {
                        cell_box.set_size_request(selected_size + 4, selected_size + 20);
                    }
                }
            }

            {
                let mut images = realized_thumb_images_toggle.borrow_mut();
                images.retain(|weak| weak.upgrade().is_some());
                for weak in images.iter() {
                    if let Some(image) = weak.upgrade() {
                        image.set_pixel_size(selected_size);
                        let bound_path = unsafe {
                            image
                                .data::<String>("bound-path")
                                .map(|path| path.as_ref().clone())
                        };
                        if let Some(path_str) = bound_path {
                            load_grid_thumbnail(
                                &image,
                                path_str,
                                selected_size,
                                hash_cache_toggle.clone(),
                            );
                        }
                    }
                }
            }

            grid_view_toggle.queue_resize();
            grid_view_toggle.queue_draw();
        });
    }

    // -----------------------------------------------------------------------
    // Assemble three-pane layout with resizable Paned dividers
    // -----------------------------------------------------------------------
    // Inner paned: center | right sidebar
    let inner_paned = Paned::new(Orientation::Horizontal);
    inner_paned.set_start_child(Some(&center_box));
    inner_paned.set_end_child(Some(&right_sidebar));
    inner_paned.set_resize_start_child(true);
    inner_paned.set_resize_end_child(false);
    inner_paned.set_shrink_start_child(false);
    inner_paned.set_shrink_end_child(false);
    inner_paned.set_position(app_config.right_pane_pos.unwrap_or(800));

    // Outer paned: left sidebar | (center + right)
    let outer_paned = Paned::new(Orientation::Horizontal);
    outer_paned.set_start_child(Some(&left_sidebar));
    outer_paned.set_end_child(Some(&inner_paned));
    outer_paned.set_resize_start_child(false);
    outer_paned.set_resize_end_child(true);
    outer_paned.set_shrink_start_child(false);
    outer_paned.set_shrink_end_child(false);
    outer_paned.set_position(app_config.left_pane_pos.unwrap_or(220));

    // Wrap content in ToastOverlay → ToolbarView → window
    toast_overlay.set_child(Some(&outer_paned));

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header_bar);
    toolbar_view.set_content(Some(&toast_overlay));

    window.set_content(Some(&toolbar_view));

    // -----------------------------------------------------------------------
    // Keyboard: Escape → grid; Left/Right (single view) → prev/next image
    // -----------------------------------------------------------------------
    let key_controller = EventControllerKey::new();
    let stack_for_keys = view_stack.clone();
    let selection_for_keys = selection_model.clone();
    let picture_for_keys = single_picture.clone();
    let left_toggle_keys = left_toggle.clone();
    let right_toggle_keys = right_toggle.clone();
    key_controller.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            stack_for_keys.set_visible_child_name("grid");
            left_toggle_keys.set_active(true);
            right_toggle_keys.set_active(true);
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
    // Save config on window close (folder + pane positions)
    // -----------------------------------------------------------------------
    let cf_close = current_folder.clone();
    let outer_paned_close = outer_paned.clone();
    let inner_paned_close = inner_paned.clone();
    let meta_paned_close = meta_paned.clone();
    let sort_key_close = sort_key.clone();
    let search_text_close = search_text.clone();
    let thumbnail_size_close = thumbnail_size.clone();
    window.connect_close_request(move |_| {
        config::save(
            cf_close.borrow().as_deref(),
            outer_paned_close.position(),
            inner_paned_close.position(),
            meta_paned_close.position(),
            &sort_key_close.borrow(),
            &search_text_close.borrow(),
            *thumbnail_size_close.borrow(),
        );
        glib::Propagation::Proceed
    });

    // -----------------------------------------------------------------------
    // Restore last folder from config + sync tree
    // -----------------------------------------------------------------------
    if let Some(last_folder) = app_config.last_folder {
        if last_folder.is_dir() {
            *current_folder.borrow_mut() = Some(last_folder.clone());
            list_store.remove_all();
            spinner.start();
            scan_directory(last_folder.clone(), sender.clone(), sort_key.borrow().clone());
            sync_tree_to_path(&tree_model, &tree_list_view, &last_folder);
        }
    }

    // -----------------------------------------------------------------------
    // Restore persisted sort + search state into the UI controls
    // -----------------------------------------------------------------------
    {
        let initial_sort_idx: u32 = match sort_key.borrow().as_str() {
            "name_desc" => 1,
            "date_asc"  => 2,
            "date_desc" => 3,
            "size_asc"  => 4,
            "size_desc" => 5,
            _           => 0,
        };
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

    window.present();
}

fn attach_context_menu<W: IsA<gtk4::Widget>>(widget: &W, menu_model: &gio::Menu) {
    let widget_obj = widget.as_ref().clone();
    let menu_model = menu_model.clone();
    let click = GestureClick::new();
    click.set_button(3);
    click.connect_pressed(move |_, _, x, y| {
        let pop = PopoverMenu::from_model(Some(&menu_model));
        pop.set_parent(&widget_obj);
        pop.set_has_arrow(true);
        pop.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        pop.popup();
    });
    widget.add_controller(click);
}

fn selected_image_path(selection: &SingleSelection) -> Option<std::path::PathBuf> {
    selection
        .selected_item()
        .and_downcast::<StringObject>()
        .map(|s| std::path::PathBuf::from(s.string().as_str()))
}

fn format_metadata_text(meta: &ImageMetadata) -> String {
    let mut out = Vec::new();
    if let Some(v) = &meta.camera_make {
        out.push(format!("Make: {v}"));
    }
    if let Some(v) = &meta.camera_model {
        out.push(format!("Model: {v}"));
    }
    if let Some(v) = &meta.exposure {
        out.push(format!("Exposure: {v}"));
    }
    if let Some(v) = &meta.iso {
        out.push(format!("ISO: {v}"));
    }
    if let Some(v) = &meta.prompt {
        out.push(format!("Prompt: {v}"));
    }
    if let Some(v) = &meta.negative_prompt {
        out.push(format!("Neg. Prompt: {v}"));
    }
    if let Some(v) = &meta.raw_parameters {
        out.push(format!("Parameters: {v}"));
    }
    if let Some(v) = &meta.workflow_json {
        out.push(format!("Workflow: {v}"));
    }
    if out.is_empty() {
        "No metadata found".to_string()
    } else {
        out.join("\n\n")
    }
}

/// Extracts seed value from raw parameters string (Automatic1111 format: "Seed: 123456, ...")
fn extract_seed_from_parameters(meta: &ImageMetadata) -> Option<String> {
    if let Some(params) = &meta.raw_parameters {
        // Try to find "Seed: <number>" pattern
        for part in params.split(',') {
            if let Some(seed_part) = part.trim().strip_prefix("Seed:") {
                if let Ok(seed_val) = seed_part.trim().parse::<u64>() {
                    return Some(seed_val.to_string());
                }
            }
        }
    }
    None
}

/// Formats a CLI-style generation command from available metadata
fn format_generation_command(meta: &ImageMetadata) -> String {
    let mut parts = Vec::new();

    if let Some(prompt) = &meta.prompt {
        parts.push(format!("--prompt \"{}\" ", prompt.replace('"', "\\\"")));
    }

    if let Some(neg_prompt) = &meta.negative_prompt {
        parts.push(format!("--negative \"{}\" ", neg_prompt.replace('"', "\\\"")));
    }

    if let Some(seed) = extract_seed_from_parameters(meta) {
        parts.push(format!("--seed {} ", seed));
    }

    if parts.is_empty() {
        "comfy-ui-cli".to_string()
    } else {
        format!("comfy-ui-cli {}", parts.join("").trim())
    }
}

// ---------------------------------------------------------------------------
// Async image loading for Picture widgets
// ---------------------------------------------------------------------------

/// Loads an image off the main thread and sets it on a [`Picture`] widget
/// once decoded.  A tag on the widget guards against stale loads when the
/// user navigates away before decoding finishes.
///
/// When `max_dimension` is `Some(dim)`, the image is decoded at a reduced
/// resolution (longest side capped to `dim` pixels) for faster display.
/// Pass `None` to decode at the file's native resolution.
fn load_picture_async(
    picture: &Picture,
    path: &str,
    max_dimension: Option<i32>,
    on_complete: Option<Box<dyn Fn(PreviewLoadMetrics) + 'static>>,
) {
    // Clear immediately so the user sees something is happening.
    picture.set_paintable(gdk::Paintable::NONE);

    // Cancel any in-flight load for this widget.
    let prev_cancel: Option<gio::Cancellable> =
        unsafe { picture.steal_data("loading-cancel") };
    if let Some(c) = prev_cancel {
        c.cancel();
    }

    // Fresh cancellable for this load.
    let cancel = gio::Cancellable::new();
    unsafe { picture.set_data("loading-cancel", cancel.clone()); }

    // Tag to detect stale loads (user clicked a different image before this one finished).
    unsafe { picture.set_data("loading-path", path.to_owned()); }

    let path_owned = path.to_owned();
    let path_check = path_owned.clone();
    let cancel_bg = cancel.clone();
    let weak = picture.downgrade();
    let enqueued_at = Instant::now();
    let enqueued_at_main = enqueued_at;
    let task = gio::spawn_blocking(move || {
        let worker_started_at = Instant::now();
        let started_at = Instant::now();
        let queue_wait_ms = started_at.duration_since(enqueued_at).as_secs_f64() * 1000.0;
        let _guard = AtomicTaskGuard::new(&ACTIVE_PREVIEW_TASKS);

        if cancel_bg.is_cancelled() {
            let worker_total_ms = worker_started_at.elapsed().as_secs_f64() * 1000.0;
            let worker_done_since_enqueue_ms =
                worker_started_at.duration_since(enqueued_at).as_secs_f64() * 1000.0
                    + worker_total_ms;
            return (
                None,
                PreviewLoadMetrics {
                    outcome: PreviewLoadOutcome::StaleOrCancelled,
                    queue_wait_ms,
                    file_open_ms: 0.0,
                    decode_ms: 0.0,
                    texture_create_ms: 0.0,
                    worker_total_ms,
                    worker_done_since_enqueue_ms,
                    main_thread_dispatch_ms: 0.0,
                    texture_apply_ms: 0.0,
                },
            );
        }

        let file = gio::File::for_path(&path_owned);
        let file_open_started = Instant::now();
        let Some(stream) = file.read(Some(&cancel_bg)).ok() else {
            let worker_total_ms = worker_started_at.elapsed().as_secs_f64() * 1000.0;
            let worker_done_since_enqueue_ms =
                worker_started_at.duration_since(enqueued_at).as_secs_f64() * 1000.0
                    + worker_total_ms;
            return (
                None,
                PreviewLoadMetrics {
                    outcome: PreviewLoadOutcome::Failed,
                    queue_wait_ms,
                    file_open_ms: file_open_started.elapsed().as_secs_f64() * 1000.0,
                    decode_ms: 0.0,
                    texture_create_ms: 0.0,
                    worker_total_ms,
                    worker_done_since_enqueue_ms,
                    main_thread_dispatch_ms: 0.0,
                    texture_apply_ms: 0.0,
                },
            );
        };
        let file_open_ms = file_open_started.elapsed().as_secs_f64() * 1000.0;

        let decode_started = Instant::now();
        let pixbuf = match max_dimension {
            Some(dim) => gdk_pixbuf::Pixbuf::from_stream_at_scale(
                &stream, dim, dim, true, Some(&cancel_bg),
            ),
            None => gdk_pixbuf::Pixbuf::from_stream(&stream, Some(&cancel_bg)),
        };
        let decode_ms = decode_started.elapsed().as_secs_f64() * 1000.0;

        match pixbuf {
            Ok(pb) => {
                let texture_create_started = Instant::now();
                let tex = gdk::Texture::for_pixbuf(&pb);
                let texture_create_ms =
                    texture_create_started.elapsed().as_secs_f64() * 1000.0;
                let worker_total_ms = worker_started_at.elapsed().as_secs_f64() * 1000.0;
                let worker_done_since_enqueue_ms =
                    worker_started_at.duration_since(enqueued_at).as_secs_f64() * 1000.0
                        + worker_total_ms;
                (
                    Some(tex),
                    PreviewLoadMetrics {
                        outcome: PreviewLoadOutcome::Displayed,
                        queue_wait_ms,
                        file_open_ms,
                        decode_ms,
                        texture_create_ms,
                        worker_total_ms,
                        worker_done_since_enqueue_ms,
                        main_thread_dispatch_ms: 0.0,
                        texture_apply_ms: 0.0,
                    },
                )
            }
            Err(_) => {
                let worker_total_ms = worker_started_at.elapsed().as_secs_f64() * 1000.0;
                let worker_done_since_enqueue_ms =
                    worker_started_at.duration_since(enqueued_at).as_secs_f64() * 1000.0
                        + worker_total_ms;
                (
                    None,
                    PreviewLoadMetrics {
                        outcome: PreviewLoadOutcome::Failed,
                        queue_wait_ms,
                        file_open_ms,
                        decode_ms,
                        texture_create_ms: 0.0,
                        worker_total_ms,
                        worker_done_since_enqueue_ms,
                        main_thread_dispatch_ms: 0.0,
                        texture_apply_ms: 0.0,
                    },
                )
            }
        }
    });
    glib::MainContext::default().spawn_local(async move {
        let mut on_complete = on_complete;

        let Ok((maybe_tex, mut metrics)) = task.await else {
            if let Some(cb) = on_complete.take() {
                cb(PreviewLoadMetrics {
                    outcome: PreviewLoadOutcome::Failed,
                    queue_wait_ms: 0.0,
                    file_open_ms: 0.0,
                    decode_ms: 0.0,
                    texture_create_ms: 0.0,
                    worker_total_ms: 0.0,
                    worker_done_since_enqueue_ms: 0.0,
                    main_thread_dispatch_ms: 0.0,
                    texture_apply_ms: 0.0,
                });
            }
            return;
        };
        let callback_started_since_enqueue_ms =
            Instant::now().duration_since(enqueued_at_main).as_secs_f64() * 1000.0;
        metrics.main_thread_dispatch_ms =
            (callback_started_since_enqueue_ms - metrics.worker_done_since_enqueue_ms).max(0.0);
        let Some(pic) = weak.upgrade() else {
            if let Some(cb) = on_complete.take() {
                metrics.outcome = PreviewLoadOutcome::StaleOrCancelled;
                cb(metrics);
            }
            return;
        };
        // Check the widget is still expecting this path.
        let is_current = unsafe {
            pic.data::<String>("loading-path")
                .map(|p| p.as_ref() == &path_check)
                .unwrap_or(false)
        };
        if !is_current {
            if let Some(cb) = on_complete.take() {
                metrics.outcome = PreviewLoadOutcome::StaleOrCancelled;
                cb(metrics);
            }
            return;
        }
        if let Some(tex) = maybe_tex {
            let apply_started = Instant::now();
            pic.set_paintable(Some(&tex));
            metrics.texture_apply_ms = apply_started.elapsed().as_secs_f64() * 1000.0;
        }
        if let Some(cb) = on_complete.take() {
            cb(metrics);
        }
    });
}

// ---------------------------------------------------------------------------
// Metadata sidebar population
// ---------------------------------------------------------------------------

fn populate_metadata_sidebar(listbox: &gtk4::ListBox, meta: &ImageMetadata) {
    // Clear existing rows
    while let Some(child) = listbox.first_child() {
        listbox.remove(&child);
    }

    // Short fields: render as ActionRow title+subtitle (fast, text is short).
    let short_rows: &[(&str, Option<&str>)] = &[
        ("Make", meta.camera_make.as_deref()),
        ("Model", meta.camera_model.as_deref()),
        ("Exposure", meta.exposure.as_deref()),
        ("ISO", meta.iso.as_deref()),
    ];

    for (key, maybe_val) in short_rows {
        let Some(val) = maybe_val else { continue };
        let row = adw::ActionRow::new();
        row.set_title(key);
        row.set_subtitle(&glib::markup_escape_text(val));
        row.set_subtitle_selectable(true);
        let copy_text = val.to_string();
        let copy_button = gtk4::Button::from_icon_name("edit-copy-symbolic");
        copy_button.add_css_class("flat");
        copy_button.set_tooltip_text(Some("Copy"));
        copy_button.connect_clicked(move |btn| {
            gtk4::prelude::WidgetExt::display(btn).clipboard().set_text(&copy_text);
        });
        row.add_suffix(&copy_button);
        listbox.append(&row);
    }

    // Long / potentially large fields: use a TextView so Pango only lays out
    // visible lines (lazy), instead of forcing full layout on the main thread.
    let long_rows: &[(&str, Option<&str>)] = &[
        ("Prompt", meta.prompt.as_deref()),
        ("Neg. Prompt", meta.negative_prompt.as_deref()),
        ("Parameters", meta.raw_parameters.as_deref()),
        ("Workflow", meta.workflow_json.as_deref()),
    ];

    for (key, maybe_val) in long_rows {
        let Some(val) = maybe_val else { continue };

        // Outer box acts as a list row container.
        let row_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        row_box.set_margin_top(8);
        row_box.set_margin_bottom(4);
        row_box.set_margin_start(12);
        row_box.set_margin_end(8);

        // Header: label + copy button.
        let header_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        header_box.set_hexpand(true);

        let key_label = gtk4::Label::new(Some(key));
        key_label.add_css_class("caption-heading");
        key_label.set_halign(gtk4::Align::Start);
        key_label.set_hexpand(true);
        header_box.append(&key_label);

        let copy_text = val.to_string();
        let copy_button = gtk4::Button::from_icon_name("edit-copy-symbolic");
        copy_button.add_css_class("flat");
        copy_button.set_tooltip_text(Some("Copy"));
        copy_button.connect_clicked(move |btn| {
            gtk4::prelude::WidgetExt::display(btn).clipboard().set_text(&copy_text);
        });
        header_box.append(&copy_button);
        row_box.append(&header_box);

        // TextView: non-editable, word-wrapped; Pango layout is lazy/incremental.
        let text_view = gtk4::TextView::new();
        text_view.set_editable(false);
        text_view.set_cursor_visible(false);
        text_view.set_wrap_mode(gtk4::WrapMode::WordChar);
        text_view.set_hexpand(true);
        text_view.add_css_class("caption");
        text_view.add_css_class("metadata-text-view");
        text_view.buffer().set_text(val);
        row_box.append(&text_view);

        // Wrap in a ListBoxRow.
        let list_row = gtk4::ListBoxRow::new();
        list_row.set_child(Some(&row_box));
        list_row.set_activatable(false);
        list_row.set_selectable(false);
        listbox.append(&list_row);
    }

    if listbox.first_child().is_none() {
        let empty = adw::ActionRow::new();
        empty.set_title("No metadata found");
        listbox.append(&empty);
    }
}

// ---------------------------------------------------------------------------
// Asynchronous metadata sidebar loading (cancellable)
// ---------------------------------------------------------------------------

/// Loads metadata asynchronously and populates the sidebar when complete.
/// If a new click supersedes this load, the trace state will have changed
/// and the load will be silently skipped.
fn load_metadata_async(
    metadata: ImageMetadata,
    listbox: gtk4::ListBox,
    trace_state: Rc<RefCell<Option<ClickTrace>>>,
    click_id: u64,
) {
    glib::MainContext::default().spawn_local(async move {
        // Populate the sidebar (this is fast, all UI calls).
        populate_metadata_sidebar(&listbox, &metadata);

        // Mark metadata as complete in trace if it's still the current click.
        if let Some(trace) = trace_state.borrow_mut().as_mut() {
            if trace.id == click_id && !trace.finished {
                trace.mark_step("metadata_shown");
                trace.metadata_done = true;
                try_finalize_click_trace(&trace_state, click_id);
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Tree-view path sync: expand ancestors and scroll to the target folder
// ---------------------------------------------------------------------------

/// Expands ancestor rows in the `TreeListModel` so `target` is visible, then
/// selects and scrolls to it.  Expansion is synchronous because our
/// `create_model` callback is synchronous.
fn sync_tree_to_path(
    tree_model: &TreeListModel,
    tree_list_view: &ListView,
    target: &std::path::Path,
) {
    // Find the root item that is either equal to `target` or its deepest
    // ancestor that appears as a root row (depth 0).
    let n = tree_model.n_items();
    let mut best_root: Option<(u32, std::path::PathBuf)> = None;
    for pos in 0..n {
        if let Some(row) = tree_model.item(pos).and_downcast::<TreeListRow>() {
            if row.depth() != 0 {
                continue;
            }
            if let Some(file) = row.item().and_downcast::<gio::File>() {
                if let Some(p) = file.path() {
                    if target.starts_with(&p) {
                        let depth = p.components().count();
                        let better = best_root
                            .as_ref()
                            .map_or(true, |(_, b)| depth > b.components().count());
                        if better {
                            best_root = Some((pos, p));
                        }
                    }
                }
            }
        }
    }
    let (_, root_path) = match best_root {
        Some(v) => v,
        None => return,
    };

    // Build the chain: root_path → … → target (each step one component deeper)
    let rel = match target.strip_prefix(&root_path) {
        Ok(r) => r,
        Err(_) => return,
    };
    let mut segments: Vec<std::path::PathBuf> = vec![root_path.clone()];
    let mut acc = root_path;
    for component in rel.components() {
        acc.push(component);
        segments.push(acc.clone());
    }

    // Walk segments: find each in the flat model, expand non-last ones.
    for (i, seg) in segments.iter().enumerate() {
        let is_last = i == segments.len() - 1;
        let n = tree_model.n_items();
        for pos in 0..n {
            if let Some(row) = tree_model.item(pos).and_downcast::<TreeListRow>() {
                if let Some(file) = row.item().and_downcast::<gio::File>() {
                    if file.path().as_deref() == Some(seg.as_path()) {
                        if is_last {
                            tree_list_view.scroll_to(pos, ListScrollFlags::SELECT, None::<gtk4::ScrollInfo>);
                        } else if row.is_expandable() {
                            row.set_expanded(true);
                        }
                        break;
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// File system helpers for the tree sidebar
// ---------------------------------------------------------------------------

/// Returns real mount points (block devices, network mounts) from /proc/mounts,
/// excluding pseudo-filesystems and kernel-internal mounts.
fn get_mount_points() -> Vec<std::path::PathBuf> {
    let pseudo_fs = [
        "tmpfs", "proc", "sysfs", "devtmpfs", "devpts", "cgroup", "cgroup2",
        "pstore", "bpf", "tracefs", "debugfs", "securityfs", "fusectl",
        "hugetlbfs", "mqueue", "configfs", "binfmt_misc", "ramfs", "squashfs",
        "overlay", "nsfs", "autofs", "efivarfs", "rpc_pipefs",
    ];
    let mut points = vec![std::path::PathBuf::from("/")];
    if let Ok(content) = std::fs::read_to_string("/proc/mounts") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 3 {
                continue;
            }
            let mount_point = parts[1];
            let fs_type = parts[2];
            if pseudo_fs.contains(&fs_type) {
                continue;
            }
            if mount_point.starts_with("/proc")
                || mount_point.starts_with("/sys")
                || mount_point.starts_with("/dev")
                || mount_point.starts_with("/run")
            {
                continue;
            }
            if mount_point == "/" {
                continue; // already in vec
            }
            points.push(std::path::PathBuf::from(mount_point));
        }
    }
    points.sort();
    points.dedup();
    points
}

/// Builds the root `ListStore` for the file tree: home directory first,
/// then all real mount points (deduplicating home if it is also a mount point).
fn build_tree_root() -> gio::ListStore {
    let store = gio::ListStore::new::<gio::File>();
    let home = glib::home_dir();
    store.append(&gio::File::for_path(&home));
    for mp in get_mount_points() {
        if mp != home {
            store.append(&gio::File::for_path(&mp));
        }
    }
    store
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
