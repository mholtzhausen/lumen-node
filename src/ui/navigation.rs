use crate::timing_report::write_timing_report;
use crate::ui::center::CenterContentBundle;
use crate::ui::grid::{enter_single_view_mode, ACTIVE_THUMBNAIL_TASKS};
use crate::ui::preview::{
    load_picture_async, PreviewLoadMetrics, PreviewLoadOutcome, ACTIVE_PREVIEW_TASKS,
};
use crate::ui::right_sidebar::RightSidebarBundle;
use gtk4::prelude::*;
use gtk4::{GestureClick, StringObject};
use std::{
    cell::Cell,
    cell::RefCell,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering as AtomicOrdering},
    time::Instant,
};

static FULL_VIEW_TRACE_COUNTER: AtomicU64 = AtomicU64::new(1);

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

pub(crate) struct NavigationDeps {
    pub(crate) center: CenterContentBundle,
    pub(crate) right: RightSidebarBundle,
    pub(crate) selection_model: gtk4::SingleSelection,
    pub(crate) left_toggle: gtk4::ToggleButton,
    pub(crate) right_toggle: gtk4::ToggleButton,
    pub(crate) pre_fullview_left: Rc<Cell<bool>>,
    pub(crate) pre_fullview_right: Rc<Cell<bool>>,
}

pub(crate) fn install_navigation_handlers(deps: NavigationDeps) {
    let stack_for_grid = deps.center.view_stack.clone();
    let picture_for_grid = deps.center.single_picture.clone();
    let selection_for_grid = deps.selection_model.clone();
    let left_toggle_grid = deps.left_toggle.clone();
    let right_toggle_grid = deps.right_toggle.clone();
    let pre_fullview_left_grid = deps.pre_fullview_left.clone();
    let pre_fullview_right_grid = deps.pre_fullview_right.clone();
    deps.center.grid_view.connect_activate(move |_, pos| {
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

    let stack_for_preview = deps.center.view_stack.clone();
    let picture_for_preview = deps.center.single_picture.clone();
    let selection_for_preview = deps.selection_model.clone();
    let left_toggle_preview = deps.left_toggle.clone();
    let right_toggle_preview = deps.right_toggle.clone();
    let pre_fullview_left_preview = deps.pre_fullview_left.clone();
    let pre_fullview_right_preview = deps.pre_fullview_right.clone();
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
    deps.right.meta_preview.add_controller(dbl_click);
}
