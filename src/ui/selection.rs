use crate::metadata::ImageMetadata;
use crate::metadata_section::apply_metadata_section_state;
use crate::timing_report::write_timing_report;
use crate::ui::grid::{refresh_realized_grid_thumbnails, THUMB_UI_CALLBACKS_TOTAL};
use crate::ui::preview::{load_picture_async, PreviewLoadMetrics, PreviewLoadOutcome};
use crate::ui::sidebar::populate_metadata_sidebar;
use crate::{
    CLICK_TRACE_COUNTER, MIN_META_SPLIT_PX, PREVIEW_REQUEST_PENDING, SCAN_BUFFER_DEPTH,
    SCAN_DRAIN_BATCH_SIZE, SCAN_DRAIN_SCHEDULED, SUPPRESS_SIDEBAR_DURING_PREVIEW,
    THUMB_UI_CALLBACKS_SKIPPED_WHILE_PREVIEW,
};
use gtk4::{glib, Image, StringObject};
use std::{
    cell::Cell,
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::atomic::Ordering as AtomicOrdering,
    time::{Duration, Instant},
};

#[derive(Clone)]
struct ClickStepTiming {
    name: String,
    elapsed_ms: f64,
}

#[derive(Clone)]
pub(crate) struct ClickTrace {
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

struct ClickRuntimeSnapshot {
    id: u64,
    active_thumbnail_jobs_at_click: u64,
    active_preview_jobs_at_click: u64,
    scan_buffer_depth_at_click: u64,
    idle_drain_scheduled_at_click: bool,
    pending_idle_drain_cycles_est_at_click: u64,
    thumb_ui_callbacks_total_at_click: u64,
    thumb_ui_callbacks_skipped_at_click: u64,
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

fn capture_click_runtime_snapshot() -> ClickRuntimeSnapshot {
    let scan_buffer_depth_at_click = SCAN_BUFFER_DEPTH.load(AtomicOrdering::Relaxed);
    let idle_drain_scheduled_at_click = SCAN_DRAIN_SCHEDULED.load(AtomicOrdering::Relaxed) != 0;
    ClickRuntimeSnapshot {
        id: CLICK_TRACE_COUNTER.fetch_add(1, AtomicOrdering::Relaxed),
        active_thumbnail_jobs_at_click: crate::ui::grid::ACTIVE_THUMBNAIL_TASKS
            .load(AtomicOrdering::Relaxed),
        active_preview_jobs_at_click: crate::ui::preview::ACTIVE_PREVIEW_TASKS
            .load(AtomicOrdering::Relaxed),
        scan_buffer_depth_at_click,
        idle_drain_scheduled_at_click,
        pending_idle_drain_cycles_est_at_click: if scan_buffer_depth_at_click == 0 {
            0
        } else {
            scan_buffer_depth_at_click.div_ceil(SCAN_DRAIN_BATCH_SIZE)
        },
        thumb_ui_callbacks_total_at_click: THUMB_UI_CALLBACKS_TOTAL.load(AtomicOrdering::Relaxed),
        thumb_ui_callbacks_skipped_at_click: THUMB_UI_CALLBACKS_SKIPPED_WHILE_PREVIEW
            .load(AtomicOrdering::Relaxed),
    }
}

fn start_click_trace(
    trace_state: &Rc<RefCell<Option<ClickTrace>>>,
    path_str: String,
    snapshot: &ClickRuntimeSnapshot,
) {
    let mut state = trace_state.borrow_mut();
    if let Some(prev) = state.as_mut() {
        if !prev.finished {
            prev.mark_step("superseded_by_new_click");
            prev.outcome = "superseded".to_string();
            prev.finished = true;
            emit_click_report(prev);
        }
    }
    let mut trace = ClickTrace::new(
        snapshot.id,
        path_str,
        snapshot.active_thumbnail_jobs_at_click,
        snapshot.active_preview_jobs_at_click,
        snapshot.scan_buffer_depth_at_click,
        snapshot.idle_drain_scheduled_at_click,
        snapshot.pending_idle_drain_cycles_est_at_click,
        snapshot.thumb_ui_callbacks_total_at_click,
        snapshot.thumb_ui_callbacks_skipped_at_click,
    );
    trace.mark_step("selection_changed");
    trace.mark_step(&format!(
        "active_jobs_captured thumb={} preview={} scan_depth={} drain_scheduled={} thumb_cb_total={} thumb_cb_skipped={}",
        snapshot.active_thumbnail_jobs_at_click,
        snapshot.active_preview_jobs_at_click,
        snapshot.scan_buffer_depth_at_click,
        snapshot.idle_drain_scheduled_at_click,
        snapshot.thumb_ui_callbacks_total_at_click,
        snapshot.thumb_ui_callbacks_skipped_at_click
    ));
    *state = Some(trace);
}

fn apply_click_preview_metrics(trace: &mut ClickTrace, metrics: &PreviewLoadMetrics) {
    trace.preview_queue_wait_ms = Some(metrics.queue_wait_ms);
    trace.preview_file_open_ms = Some(metrics.file_open_ms);
    trace.preview_decode_ms = Some(metrics.decode_ms);
    trace.preview_texture_create_ms = Some(metrics.texture_create_ms);
    trace.preview_worker_total_ms = Some(metrics.worker_total_ms);
    trace.preview_main_thread_dispatch_ms = Some(metrics.main_thread_dispatch_ms);
    trace.preview_texture_apply_ms = Some(metrics.texture_apply_ms);
}

fn handle_selection_preview_outcome(
    metrics: PreviewLoadMetrics,
    click_trace_state: &Rc<RefCell<Option<ClickTrace>>>,
    click_id: u64,
    realized_thumb_images: &Rc<RefCell<Vec<glib::WeakRef<Image>>>>,
    thumbnail_size: &Rc<RefCell<i32>>,
    hash_cache: &Rc<RefCell<HashMap<String, String>>>,
) {
    PREVIEW_REQUEST_PENDING.store(0, AtomicOrdering::Relaxed);
    SUPPRESS_SIDEBAR_DURING_PREVIEW.store(0, AtomicOrdering::Relaxed);
    refresh_realized_grid_thumbnails(realized_thumb_images, thumbnail_size, hash_cache);

    if let Some(trace) = click_trace_state.borrow_mut().as_mut() {
        if trace.id == click_id && !trace.finished {
            apply_click_preview_metrics(trace, &metrics);
            match metrics.outcome {
                PreviewLoadOutcome::Displayed => {
                    let thumb_total_now = THUMB_UI_CALLBACKS_TOTAL.load(AtomicOrdering::Relaxed);
                    let thumb_skipped_now =
                        THUMB_UI_CALLBACKS_SKIPPED_WHILE_PREVIEW.load(AtomicOrdering::Relaxed);
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
                PreviewLoadOutcome::Failed => {
                    trace.mark_step("preview_failed");
                    trace.preview_done = true;
                }
                PreviewLoadOutcome::StaleOrCancelled => {
                    trace.mark_step("preview_stale_or_cancelled");
                    trace.outcome = "cancelled".to_string();
                    trace.finished = true;
                    emit_click_report(trace);
                }
            }
        }
    }
}

fn get_cached_metadata_for_selection(
    meta_cache: &Rc<RefCell<HashMap<String, ImageMetadata>>>,
    item: &StringObject,
) -> ImageMetadata {
    let cache = meta_cache.borrow();
    cache
        .get(item.string().as_str())
        .cloned()
        .unwrap_or_default()
}

fn begin_selection_preview_load(trace_state: &Rc<RefCell<Option<ClickTrace>>>, click_id: u64) {
    mark_click_step(trace_state, click_id, "preview_load_dispatched");
    PREVIEW_REQUEST_PENDING.store(1, AtomicOrdering::Relaxed);
    SUPPRESS_SIDEBAR_DURING_PREVIEW.store(1, AtomicOrdering::Relaxed);
}

fn dispatch_selection_preview_load(
    meta_preview: &gtk4::Picture,
    path_str: &str,
    click_trace_state: Rc<RefCell<Option<ClickTrace>>>,
    click_id: u64,
    realized_thumb_images: Rc<RefCell<Vec<glib::WeakRef<Image>>>>,
    thumbnail_size: Rc<RefCell<i32>>,
    hash_cache: Rc<RefCell<HashMap<String, String>>>,
) {
    load_picture_async(
        meta_preview,
        path_str,
        Some(520),
        Some(Box::new(move |metrics| {
            handle_selection_preview_outcome(
                metrics,
                &click_trace_state,
                click_id,
                &realized_thumb_images,
                &thumbnail_size,
                &hash_cache,
            );
        })),
    );
}

fn dispatch_selection_metadata_load(
    metadata: ImageMetadata,
    listbox: gtk4::ListBox,
    meta_expander: gtk4::Expander,
    meta_paned: gtk4::Paned,
    meta_split_before_auto_collapse: Rc<Cell<Option<i32>>>,
    meta_position_programmatic: Rc<Cell<u32>>,
    trace_state: Rc<RefCell<Option<ClickTrace>>>,
    click_id: u64,
) {
    load_metadata_async(
        metadata,
        listbox,
        meta_expander,
        meta_paned,
        meta_split_before_auto_collapse,
        meta_position_programmatic,
        trace_state,
        click_id,
    );
}

pub(crate) fn handle_selection_change_event(
    item: &StringObject,
    click_trace_state: &Rc<RefCell<Option<ClickTrace>>>,
    meta_cache: &Rc<RefCell<HashMap<String, ImageMetadata>>>,
    meta_listbox: &gtk4::ListBox,
    meta_expander: &gtk4::Expander,
    meta_paned: &gtk4::Paned,
    meta_split_before_auto_collapse: &Rc<Cell<Option<i32>>>,
    meta_position_programmatic: &Rc<Cell<u32>>,
    meta_preview: &gtk4::Picture,
    realized_thumb_images: &Rc<RefCell<Vec<glib::WeakRef<Image>>>>,
    thumbnail_size: &Rc<RefCell<i32>>,
    hash_cache: &Rc<RefCell<HashMap<String, String>>>,
) {
    let path_str = item.string().to_string();
    let click_snapshot = capture_click_runtime_snapshot();
    start_click_trace(click_trace_state, path_str.clone(), &click_snapshot);
    let click_id = click_snapshot.id;

    mark_click_step(click_trace_state, click_id, "selected_item_resolved");

    // Load the preview image off-thread so the UI stays responsive.
    // Decode at 2x sidebar width (520px) for fast display on HiDPI.
    begin_selection_preview_load(click_trace_state, click_id);

    // Load metadata asynchronously (cancellable if user navigates away).
    mark_click_step(click_trace_state, click_id, "metadata_lookup_started");
    let meta = get_cached_metadata_for_selection(meta_cache, item);
    mark_click_step(click_trace_state, click_id, "metadata_lookup_finished");
    mark_click_step(click_trace_state, click_id, "metadata_render_started");

    // Start metadata + preview load in background (non-blocking).
    dispatch_selection_metadata_load(
        meta.clone(),
        meta_listbox.clone(),
        meta_expander.clone(),
        meta_paned.clone(),
        meta_split_before_auto_collapse.clone(),
        meta_position_programmatic.clone(),
        click_trace_state.clone(),
        click_id,
    );
    dispatch_selection_preview_load(
        meta_preview,
        &path_str,
        click_trace_state.clone(),
        click_id,
        realized_thumb_images.clone(),
        thumbnail_size.clone(),
        hash_cache.clone(),
    );
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
            if trace.id == click_id && trace.preview_done && trace.metadata_done && !trace.finished {
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

fn load_metadata_async(
    metadata: ImageMetadata,
    listbox: gtk4::ListBox,
    meta_expander: gtk4::Expander,
    meta_paned: gtk4::Paned,
    meta_split_before_auto_collapse: Rc<Cell<Option<i32>>>,
    meta_position_programmatic: Rc<Cell<u32>>,
    trace_state: Rc<RefCell<Option<ClickTrace>>>,
    click_id: u64,
) {
    glib::MainContext::default().spawn_local(async move {
        // Populate the sidebar (this is fast, all UI calls).
        populate_metadata_sidebar(&listbox, &metadata);
        meta_position_programmatic.set(meta_position_programmatic.get().saturating_add(1));
        apply_metadata_section_state(
            &metadata,
            &meta_expander,
            &meta_paned,
            &meta_split_before_auto_collapse,
            MIN_META_SPLIT_PX,
        );
        meta_position_programmatic.set(meta_position_programmatic.get().saturating_sub(1));

        // Mark metadata as complete in trace if it's still the current click.
        let should_finalize = if let Some(trace) = trace_state.borrow_mut().as_mut() {
            if trace.id == click_id && !trace.finished {
                trace.mark_step("metadata_shown");
                trace.metadata_done = true;
                true
            } else {
                false
            }
        } else {
            false
        };
        if should_finalize {
            try_finalize_click_trace(&trace_state, click_id);
        }
    });
}
