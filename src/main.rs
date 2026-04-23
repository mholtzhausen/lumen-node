mod config;
mod db;
mod metadata;
mod scanner;
mod thumbnails;
mod updater;

use metadata::{ImageMetadata, ScanMessage};
use scanner::scan_directory;

use std::{
    cell::Cell,
    cell::RefCell,
    collections::HashMap,
    collections::VecDeque,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering as AtomicOrdering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use libadwaita as adw;
use adw::prelude::*;
use gtk4::{
    gdk, gio, glib, CustomFilter, CustomSorter, EventControllerKey, EventControllerScroll,
    EventControllerMotion, EventControllerScrollFlags, FilterListModel,
    GestureClick,
    GridView, Image, Label, ListItem, ListView, ListScrollFlags, Orientation, Overlay, Paned, Picture,
    PopoverMenu, ScrolledWindow, SignalListItemFactory, SingleSelection, SortListModel,
    ProgressBar, StringObject, TreeExpander, TreeListModel, TreeListRow, Expander,
};
use serde_json::Value as JsonValue;

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
static DEFER_GRID_THUMBNAILS_UNTIL_ENUM_COMPLETE: AtomicU64 = AtomicU64::new(0);
const SCAN_DRAIN_BATCH_SIZE: u64 = 50;
const DEFAULT_WINDOW_WIDTH: i32 = 1280;
const DEFAULT_WINDOW_HEIGHT: i32 = 800;
const MIN_LEFT_PANE_PX: i32 = 120;
const MIN_RIGHT_PANE_PX: i32 = 180;
const MIN_CENTER_PANE_PX: i32 = 260;
const MIN_META_SPLIT_PX: i32 = 120;
const RECENT_FOLDERS_LIMIT: usize = 50;
const ENUM_PHASE_WEIGHT: f64 = 0.10;
const THUMB_PHASE_WEIGHT: f64 = 0.35;
const ENRICH_PHASE_WEIGHT: f64 = 0.55;

fn clamp_f64(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

fn pct_to_px(total: i32, pct: f64) -> i32 {
    let total = total.max(1) as f64;
    ((clamp_f64(pct, 0.0, 100.0) / 100.0) * total).round() as i32
}

fn px_to_pct(px: i32, total: i32) -> f64 {
    let total = total.max(1) as f64;
    clamp_f64(((px.max(0) as f64) / total) * 100.0, 0.0, 100.0)
}


fn monitor_bounds_for_window(window: &adw::ApplicationWindow) -> (i32, i32) {
    let display = gtk4::prelude::WidgetExt::display(window);
    if let Some(surface) = window.surface() {
        if let Some(monitor) = display.monitor_at_surface(&surface) {
            let geometry = monitor.geometry();
            return (geometry.width().max(1), geometry.height().max(1));
        }
    }

    let monitors = display.monitors();
    for i in 0..monitors.n_items() {
        if let Some(monitor) = monitors.item(i).and_downcast::<gdk::Monitor>() {
            let geometry = monitor.geometry();
            return (geometry.width().max(1), geometry.height().max(1));
        }
    }

    (DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT)
}

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
            return format!(
                "Images {} | Folder size {}",
                self.folder_image_count,
                human_readable_bytes(self.folder_total_size_bytes)
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

fn human_readable_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit_idx = 0;
    while value >= 1024.0 && unit_idx < UNITS.len() - 1 {
        value /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{} {}", bytes, UNITS[unit_idx])
    } else {
        format!("{value:.1} {}", UNITS[unit_idx])
    }
}

fn invalid_filename_reason(name: &str) -> Option<&'static str> {
    if name.is_empty() {
        return Some("Name cannot be empty");
    }
    if name == "." || name == ".." {
        return Some("Name cannot be '.' or '..'");
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return Some("Name cannot end with a space or dot");
    }
    if name.chars().any(|c| c == '\0' || c.is_control()) {
        return Some("Name cannot contain control characters");
    }
    if name
        .chars()
        .any(|c| matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
    {
        return Some("Name contains illegal characters");
    }
    let upper = name.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return Some("Reserved filename");
    }
    if let Some(n) = upper.strip_prefix("COM") {
        if matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9") {
            return Some("Reserved filename");
        }
    }
    if let Some(n) = upper.strip_prefix("LPT") {
        if matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9") {
            return Some("Reserved filename");
        }
    }
    None
}

fn split_filename(path: &std::path::Path) -> (String, Option<String>) {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if let Some(ext) = path.extension().map(|e| e.to_string_lossy().into_owned()) {
        let suffix = format!(".{}", ext);
        if let Some(stem) = file_name.strip_suffix(&suffix) {
            return (stem.to_string(), Some(ext));
        }
    }
    (file_name, None)
}

fn clipboard_base_name_hint(raw_text: &str) -> Option<String> {
    for line in raw_text.lines() {
        let candidate = line.trim();
        if candidate.is_empty() || candidate.starts_with('#') {
            continue;
        }
        let name = if candidate.starts_with("file://") {
            let file = gio::File::for_uri(candidate);
            file.basename()
                .map(|s| s.to_string_lossy().into_owned())
        } else {
            std::path::Path::new(candidate)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
        };
        if let Some(name) = name {
            let base = std::path::Path::new(&name)
                .file_stem()
                .map(|s| s.to_string_lossy().trim().to_string())
                .unwrap_or_default();
            if !base.is_empty() {
                return Some(base);
            }
        }
    }
    None
}

fn build_renamed_target(
    source_path: &std::path::Path,
    input_base_name: &str,
) -> Result<std::path::PathBuf, String> {
    let trimmed = input_base_name.trim();
    if let Some(reason) = invalid_filename_reason(trimmed) {
        return Err(reason.to_string());
    }
    let (current_base, ext) = split_filename(source_path);
    if trimmed == current_base {
        return Err("Enter a different name".to_string());
    }
    let Some(parent) = source_path.parent() else {
        return Err("Cannot determine parent folder".to_string());
    };
    let candidate_name = if let Some(ext) = ext {
        format!("{trimmed}.{ext}")
    } else {
        trimmed.to_string()
    };
    let target = parent.join(candidate_name);
    if target.exists() {
        return Err("A file with this name already exists".to_string());
    }
    Ok(target)
}

fn open_rename_dialog(
    window: &adw::ApplicationWindow,
    toast_overlay: &adw::ToastOverlay,
    start_scan_for_folder: &Rc<dyn Fn(std::path::PathBuf)>,
    current_folder: &Rc<RefCell<Option<std::path::PathBuf>>>,
    source_path: std::path::PathBuf,
    initial_base_name: Option<String>,
) {
    let (current_base, ext) = split_filename(&source_path);
    let dialog = gtk4::Window::builder()
        .transient_for(window)
        .modal(true)
        .title("Rename file")
        .default_width(420)
        .build();

    let content = gtk4::Box::new(Orientation::Vertical, 8);
    content.set_spacing(8);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    dialog.set_child(Some(&content));

    let prompt = Label::new(Some("Enter a new base name:"));
    prompt.set_halign(gtk4::Align::Start);
    content.append(&prompt);

    let entry = gtk4::Entry::new();
    entry.set_text(initial_base_name.as_deref().unwrap_or(&current_base));
    entry.set_hexpand(true);
    entry.select_region(0, -1);
    content.append(&entry);

    let extension_hint = if let Some(ext) = &ext {
        format!("Extension '.{ext}' will be preserved")
    } else {
        "File has no extension".to_string()
    };
    let hint_label = Label::new(Some(&extension_hint));
    hint_label.add_css_class("caption");
    hint_label.set_halign(gtk4::Align::Start);
    content.append(&hint_label);

    let error_label = Label::new(None);
    error_label.add_css_class("caption");
    error_label.add_css_class("error");
    error_label.set_halign(gtk4::Align::Start);
    content.append(&error_label);

    let button_row = gtk4::Box::new(Orientation::Horizontal, 6);
    button_row.set_halign(gtk4::Align::End);
    let cancel_btn = gtk4::Button::with_label("Cancel");
    let rename_btn = gtk4::Button::with_label("Rename");
    rename_btn.set_sensitive(false);
    button_row.append(&cancel_btn);
    button_row.append(&rename_btn);
    content.append(&button_row);

    let candidate_target: Rc<RefCell<Option<std::path::PathBuf>>> = Rc::new(RefCell::new(None));
    let validate_input: Rc<dyn Fn(&str)> = Rc::new({
        let source_path = source_path.clone();
        let candidate_target = candidate_target.clone();
        let rename_btn = rename_btn.clone();
        let error_label = error_label.clone();
        move |value: &str| {
            match build_renamed_target(&source_path, value) {
                Ok(path) => {
                    *candidate_target.borrow_mut() = Some(path);
                    error_label.set_text("");
                    rename_btn.set_sensitive(true);
                }
                Err(message) => {
                    *candidate_target.borrow_mut() = None;
                    error_label.set_text(&message);
                    rename_btn.set_sensitive(false);
                }
            }
        }
    });
    (validate_input.as_ref())(entry.text().as_str());

    let validate_on_change = validate_input.clone();
    entry.connect_changed(move |e| {
        (validate_on_change.as_ref())(e.text().as_str());
    });

    let start_scan_for_folder = start_scan_for_folder.clone();
    let current_folder = current_folder.clone();
    let toast_overlay = toast_overlay.clone();
    let dialog_for_cancel = dialog.clone();
    cancel_btn.connect_clicked(move |_| {
        dialog_for_cancel.close();
    });

    let dialog_for_rename = dialog.clone();
    let rename_btn_activate = rename_btn.clone();
    entry.connect_activate(move |_| {
        rename_btn_activate.emit_clicked();
    });

    rename_btn.connect_clicked(move |_| {
        if let Some(target) = candidate_target.borrow().clone() {
            match std::fs::rename(&source_path, &target) {
                Ok(()) => {
                    if let Some(folder) = current_folder.borrow().as_ref().cloned() {
                        start_scan_for_folder(folder);
                    }
                    toast_overlay.add_toast(adw::Toast::new("File renamed"));
                }
                Err(err) => {
                    toast_overlay.add_toast(adw::Toast::new(&format!("Rename failed: {}", err)));
                }
            }
        }
        dialog_for_rename.close();
    });

    dialog.present();
}

fn open_delete_dialog(
    window: &adw::ApplicationWindow,
    toast_overlay: &adw::ToastOverlay,
    start_scan_for_folder: &Rc<dyn Fn(std::path::PathBuf)>,
    current_folder: &Rc<RefCell<Option<std::path::PathBuf>>>,
    source_path: std::path::PathBuf,
) {
    let file_name = source_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| source_path.to_string_lossy().into_owned());
    let dialog = gtk4::Window::builder()
        .transient_for(window)
        .modal(true)
        .title("Delete file")
        .default_width(420)
        .build();

    let content = gtk4::Box::new(Orientation::Vertical, 10);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    dialog.set_child(Some(&content));

    let prompt = Label::new(Some(&format!("Delete '{}'?", file_name)));
    prompt.set_halign(gtk4::Align::Start);
    content.append(&prompt);

    let hint = Label::new(Some("This cannot be undone."));
    hint.add_css_class("caption");
    hint.set_halign(gtk4::Align::Start);
    content.append(&hint);

    let button_row = gtk4::Box::new(Orientation::Horizontal, 6);
    button_row.set_halign(gtk4::Align::End);
    let cancel_btn = gtk4::Button::with_label("Cancel");
    let delete_btn = gtk4::Button::with_label("Delete");
    delete_btn.add_css_class("destructive-action");
    button_row.append(&cancel_btn);
    button_row.append(&delete_btn);
    content.append(&button_row);

    let dialog_for_cancel = dialog.clone();
    cancel_btn.connect_clicked(move |_| {
        dialog_for_cancel.close();
    });

    let dialog_for_delete = dialog.clone();
    let toast_overlay = toast_overlay.clone();
    let start_scan_for_folder = start_scan_for_folder.clone();
    let current_folder = current_folder.clone();
    delete_btn.connect_clicked(move |_| {
        match std::fs::remove_file(&source_path) {
            Ok(()) => {
                if let Some(folder) = current_folder.borrow().as_ref().cloned() {
                    start_scan_for_folder(folder);
                }
                let toast = adw::Toast::new("File deleted");
                toast.set_timeout(2);
                toast_overlay.add_toast(toast);
            }
            Err(err) => {
                let toast = adw::Toast::new(&format!("Delete failed: {}", err));
                toast.set_timeout(3);
                toast_overlay.add_toast(toast);
            }
        }
        dialog_for_delete.close();
    });

    dialog.present();
}

fn push_recent_folder_entry(history: &mut Vec<std::path::PathBuf>, folder: &std::path::Path) {
    history.retain(|entry| entry != folder);
    history.insert(0, folder.to_path_buf());
    history.truncate(RECENT_FOLDERS_LIMIT);
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

#[derive(Clone, Default)]
struct SortFields {
    filename_lower: String,
    modified: Option<SystemTime>,
    size: u64,
}

fn compute_sort_fields(path_str: &str) -> SortFields {
    let path = std::path::Path::new(path_str);
    let filename_lower = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let (modified, size) = match std::fs::metadata(path) {
        Ok(meta) => (meta.modified().ok(), meta.len()),
        Err(_) => (None, 0),
    };

    SortFields {
        filename_lower,
        modified,
        size,
    }
}

fn format_sort_flag_date(modified: Option<SystemTime>) -> Option<String> {
    let modified = modified?;
    let secs = modified.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    let dt = glib::DateTime::from_unix_local(secs).ok()?;
    dt.format("%Y-%m-%d").ok().map(|s| s.to_string())
}

fn first_filename_character(filename_lower: &str) -> String {
    let ch = filename_lower
        .chars()
        .find(|c| c.is_alphanumeric())
        .unwrap_or('#');
    ch.to_uppercase().collect()
}

fn sort_flag_text_for_path(
    path: &str,
    sort_key: &str,
    sort_fields_cache: &HashMap<String, SortFields>,
) -> Option<String> {
    let fallback;
    let fields = if let Some(fields) = sort_fields_cache.get(path) {
        fields
    } else {
        fallback = compute_sort_fields(path);
        &fallback
    };

    if sort_key.starts_with("name_") {
        let source = if fields.filename_lower.is_empty() {
            std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        } else {
            fields.filename_lower.clone()
        };
        return Some(first_filename_character(&source));
    }

    if sort_key.starts_with("date_") {
        return format_sort_flag_date(fields.modified);
    }

    if sort_key.starts_with("size_") {
        return Some(human_readable_bytes(fields.size));
    }

    None
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

fn sort_index_for_key(sort_key: &str) -> u32 {
    match sort_key {
        "name_desc" => 1,
        "date_asc" => 2,
        "date_desc" => 3,
        "size_asc" => 4,
        "size_desc" => 5,
        _ => 0,
    }
}

fn load_grid_thumbnail(
    thumb_image: &Image,
    path_str: String,
    size: i32,
    hash_cache: Rc<RefCell<HashMap<String, String>>>,
    generation_token: Rc<Cell<u64>>,
    expected_generation: u64,
) {
    thumb_image.set_icon_name(Some("image-x-generic-symbolic"));
    unsafe { thumb_image.set_data("bound-path", path_str.clone()); }

    if DEFER_GRID_THUMBNAILS_UNTIL_ENUM_COMPLETE.load(AtomicOrdering::Relaxed) != 0 {
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

    // Count the task before spawning so ACTIVE_THUMBNAIL_TASKS reflects queued+running
    // tasks, not just running ones. Without this, the counter stays near zero during
    // fast scrolling because tasks pile up in the thread pool queue before any start.
    // Cap at 64: enough to cover a full viewport refresh without allowing the
    // unbounded backlog that causes the direction-change lag.
    const MAX_THUMBNAIL_TASKS: u64 = 64;
    if ACTIVE_THUMBNAIL_TASKS.load(AtomicOrdering::Relaxed) >= MAX_THUMBNAIL_TASKS {
        return;
    }
    let task_guard = AtomicTaskGuard::new(&ACTIVE_THUMBNAIL_TASKS);

    let path_for_thread = std::path::PathBuf::from(&path_str);
    let cached_hash_for_task = cached_hash.clone();
    let task = gio::spawn_blocking(move || {
        let _guard = task_guard; // moves in; drops (decrements) when closure ends

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
        if generation_token.get() != expected_generation {
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

fn refresh_realized_grid_thumbnails(
    realized_thumb_images: &Rc<RefCell<Vec<glib::WeakRef<Image>>>>,
    thumbnail_size: &Rc<RefCell<i32>>,
    hash_cache: &Rc<RefCell<HashMap<String, String>>>,
) {
    let size = *thumbnail_size.borrow();
    let mut images = realized_thumb_images.borrow_mut();
    images.retain(|weak| weak.upgrade().is_some());
    for weak in images.iter() {
        if let Some(image) = weak.upgrade() {
            image.set_pixel_size(size);
            let bound_path = unsafe {
                image
                    .data::<String>("bound-path")
                    .map(|path| path.as_ref().clone())
            };
            if let Some(path_str) = bound_path {
                let generation_token = unsafe {
                    image
                        .data::<Rc<Cell<u64>>>("thumb-generation")
                        .map(|token| token.as_ref().clone())
                };
                if let Some(generation_token) = generation_token {
                    let expected_generation = generation_token.get();
                    load_grid_thumbnail(
                        &image,
                        path_str,
                        size,
                        hash_cache.clone(),
                        generation_token,
                        expected_generation,
                    );
                }
            }
        }
    }
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

    // Load persisted config (last folder).
    let app_config = config::load();
    let initial_recent_folders = app_config.recent_folders.clone();
    let (monitor_width, monitor_height) = monitor_bounds_for_window(&window);
    let min_window_width = MIN_LEFT_PANE_PX + MIN_CENTER_PANE_PX + MIN_RIGHT_PANE_PX;
    let min_window_height = (MIN_META_SPLIT_PX * 2).max(360);
    let initial_window_width = app_config
        .window_width
        .unwrap_or(DEFAULT_WINDOW_WIDTH)
        .clamp(min_window_width, monitor_width.max(min_window_width));
    let initial_window_height = app_config
        .window_height
        .unwrap_or(DEFAULT_WINDOW_HEIGHT)
        .clamp(min_window_height, monitor_height.max(min_window_height));
    window.set_default_size(initial_window_width, initial_window_height);
    let css = gtk4::CssProvider::new();
    css.load_from_string(
        "
        .scroll-flag-bubble {
            background-color: alpha(@theme_bg_color, 0.86);
            border-radius: 8px;
            padding: 6px 12px;
        }
        .scroll-flag-pointer {
            color: alpha(@theme_fg_color, 0.95);
        }
        .thumbnail-card {
            background-color: alpha(@theme_fg_color, 0.04);
            border-radius: 8px;
            padding: 4px;
        }
        gridview > child {
            background-color: transparent;
            border-color: transparent;
            box-shadow: none;
        }
        gridview > child:hover {
            background-color: transparent;
        }
        gridview > child:selected {
            background-color: transparent;
        }
        gridview > child:hover .thumbnail-card {
            background-color: alpha(@theme_fg_color, 0.10);
            box-shadow: 0 2px 6px alpha(black, 0.14);
        }
        gridview > child:selected .thumbnail-card {
            background-color: alpha(@accent_bg_color, 0.28);
        }
        ",
    );
    gtk4::style_context_add_provider_for_display(
        &gtk4::prelude::WidgetExt::display(&window),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    let configured_right_pane_width_pct = app_config.right_pane_width_pct;
    let configured_right_pane_pos = app_config.right_pane_pos;
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
    let progress_box = gtk4::Box::new(Orientation::Horizontal, 6);
    progress_box.set_visible(true);
    progress_box.set_halign(gtk4::Align::Start);
    progress_box.set_valign(gtk4::Align::Center);
    let progress_label = Label::new(Some("Scanning folder..."));
    progress_label.add_css_class("caption");
    progress_label.set_halign(gtk4::Align::Start);
    let progress_bar = ProgressBar::new();
    progress_bar.set_hexpand(false);
    progress_bar.set_show_text(true);
    progress_bar.set_width_request(180);
    progress_bar.set_height_request(8);
    progress_bar.set_text(Some("--%"));
    progress_box.append(&progress_label);
    progress_box.append(&progress_bar);
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
    let header_bar = adw::HeaderBar::new();

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
    search_entry.set_hexpand(true);

    // --- Clear button ---
    let clear_btn = gtk4::Button::from_icon_name("edit-clear-symbolic");
    clear_btn.set_tooltip_text(Some("Clear filters"));

    // Center widget: sort + search + clear grouped together.
    let toolbar_center = gtk4::Box::new(Orientation::Horizontal, 6);
    toolbar_center.set_valign(gtk4::Align::Center);
    toolbar_center.set_hexpand(true);
    toolbar_center.append(&sort_dropdown);
    toolbar_center.append(&size_selector);
    toolbar_center.append(&search_entry);
    toolbar_center.append(&clear_btn);
    header_bar.set_title_widget(Some(&toolbar_center));

    // Sidebar toggle buttons — collapse/expand left and right panels.
    let left_toggle = gtk4::ToggleButton::new();
    left_toggle.set_icon_name("sidebar-show-symbolic");
    let initial_left_sidebar_visible = app_config.left_sidebar_visible.unwrap_or(false);
    left_toggle.set_active(initial_left_sidebar_visible);
    left_toggle.set_tooltip_text(Some("Toggle left panel"));
    header_bar.pack_start(&left_toggle);

    // "Open Folder" button in the start slot.
    let open_btn = gtk4::Button::from_icon_name("folder-open-symbolic");
    open_btn.set_tooltip_text(Some("Open Folder…"));
    header_bar.pack_start(&open_btn);

    let history_btn = gtk4::MenuButton::new();
    history_btn.set_icon_name("document-open-recent-symbolic");
    history_btn.set_tooltip_text(Some("Recent folders"));
    let history_popover = gtk4::Popover::new();
    let history_list = gtk4::Box::new(Orientation::Vertical, 0);
    history_list.set_margin_top(6);
    history_list.set_margin_bottom(6);
    history_list.set_margin_start(6);
    history_list.set_margin_end(6);
    history_popover.set_child(Some(&history_list));
    history_btn.set_popover(Some(&history_popover));
    header_bar.pack_start(&history_btn);

    let right_toggle = gtk4::ToggleButton::new();
    right_toggle.set_icon_name("sidebar-show-right-symbolic");
    let initial_right_sidebar_visible = app_config.right_sidebar_visible.unwrap_or(true);
    right_toggle.set_active(initial_right_sidebar_visible);
    right_toggle.set_tooltip_text(Some("Toggle right panel"));
    header_bar.pack_end(&right_toggle);

    // -----------------------------------------------------------------------
    // Three-pane layout: [left sidebar] | [center] | [right sidebar]
    // -----------------------------------------------------------------------
    // --- Left sidebar: file system tree ---
    let left_sidebar = gtk4::Box::new(Orientation::Vertical, 0);
    left_sidebar.set_width_request(200);
    left_sidebar.set_visible(initial_left_sidebar_visible);

    // Root item: currently selected folder (fallback to home until selection).
    let tree_root = build_tree_root(app_config.last_folder.as_ref());

    // TreeListModel lazily loads subdirectories when a node is expanded.
    let tree_model = TreeListModel::new(tree_root.clone(), false, false, move |item: &glib::Object| -> Option<gio::ListModel> {
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
        {
            let mut history = recent_folders_tree.borrow_mut();
            push_recent_folder_entry(&mut history, path.as_path());
            config::save_recent_state(Some(path.as_path()), &history);
        }
        reset_tree_root_deferred(tree_root_tree.clone(), path.clone());
        start_scan_tree(path);
    });

    let tree_factory = SignalListItemFactory::new();
    tree_factory.connect_setup(|_, obj| {
        let Some(list_item) = obj.downcast_ref::<ListItem>() else {
            return;
        };
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
        let Some(list_item) = obj.downcast_ref::<ListItem>() else {
            return;
        };
        let Some(expander) = list_item.child().and_downcast::<TreeExpander>() else {
            return;
        };
        let Some(row) = list_item.item().and_downcast::<TreeListRow>() else {
            expander.set_list_row(None::<&TreeListRow>);
            return;
        };
        expander.set_list_row(Some(&row));
        let Some(file) = row.item().and_downcast::<gio::File>() else {
            return;
        };
        let Some(row_box) = expander.child().and_downcast::<gtk4::Box>() else {
            return;
        };
        let Some(label) = row_box.last_child().and_downcast::<Label>() else {
            return;
        };
        let name = if let Some(p) = file.path() {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.to_string_lossy().into_owned())
        } else {
            file.uri().to_string()
        };
        label.set_text(&name);
    });
    tree_factory.connect_unbind(|_, obj| {
        let Some(list_item) = obj.downcast_ref::<ListItem>() else {
            return;
        };
        let Some(expander) = list_item.child().and_downcast::<TreeExpander>() else {
            return;
        };
        expander.set_list_row(None::<&TreeListRow>);
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
        let ord = match key.as_str() {
            "name_asc" | "name_desc" => {
                let cmp = fields_a
                    .filename_lower
                    .cmp(&fields_b.filename_lower)
                    .then_with(|| path_a.cmp(&path_b));
                if key == "name_desc" { cmp.reverse() } else { cmp }
            }
            "date_asc" | "date_desc" => {
                let cmp = fields_a
                    .modified
                    .cmp(&fields_b.modified)
                    .then_with(|| fields_a.filename_lower.cmp(&fields_b.filename_lower))
                    .then_with(|| path_a.cmp(&path_b));
                if key == "date_desc" { cmp.reverse() } else { cmp }
            }
            "size_asc" | "size_desc" => {
                let cmp = fields_a
                    .size
                    .cmp(&fields_b.size)
                    .then_with(|| fields_a.filename_lower.cmp(&fields_b.filename_lower))
                    .then_with(|| path_a.cmp(&path_b));
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
    let window_for_rename = window.clone();
    let toast_for_rename = toast_overlay.clone();
    let current_folder_for_rename = current_folder.clone();
    let start_scan_for_folder_rename = start_scan_for_folder.clone();
    let window_for_delete = window.clone();
    let toast_for_delete = toast_overlay.clone();
    let current_folder_for_delete = current_folder.clone();
    let start_scan_for_folder_delete = start_scan_for_folder.clone();
    factory.connect_setup(move |_, obj| {
        let list_item = obj.downcast_ref::<ListItem>().unwrap();
        let cell_box = gtk4::Box::new(Orientation::Vertical, 4);
        cell_box.add_css_class("thumbnail-card");
        cell_box.set_halign(gtk4::Align::Center);
        cell_box.set_margin_top(4);
        cell_box.set_margin_bottom(4);
        cell_box.set_margin_start(4);
        cell_box.set_margin_end(4);
        let size = *thumbnail_size_setup.borrow();
        cell_box.set_size_request(size + 12, size + 28);
        let thumb_image = Image::new();
        thumb_image.set_pixel_size(size);
        let generation_token = Rc::new(Cell::new(0_u64));
        unsafe { thumb_image.set_data("thumb-generation", generation_token); }
        realized_cell_boxes_setup.borrow_mut().push(cell_box.downgrade());
        realized_thumb_images_setup.borrow_mut().push(thumb_image.downgrade());
        let name_label = Label::new(None);
        name_label.set_max_width_chars(16);
        name_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        name_label.add_css_class("caption");
        name_label.set_hexpand(true);
        name_label.set_halign(gtk4::Align::Start);
        let rename_btn = gtk4::Button::from_icon_name("document-edit-symbolic");
        rename_btn.add_css_class("flat");
        rename_btn.set_tooltip_text(Some("Rename file"));
        rename_btn.set_opacity(0.0);
        rename_btn.set_focus_on_click(false);
        let delete_btn = gtk4::Button::from_icon_name("user-trash-symbolic");
        delete_btn.add_css_class("flat");
        delete_btn.add_css_class("destructive-action");
        delete_btn.set_tooltip_text(Some("Delete file"));
        delete_btn.set_opacity(0.0);
        delete_btn.set_focus_on_click(false);
        let window_for_btn = window_for_rename.clone();
        let toast_for_btn = toast_for_rename.clone();
        let current_folder_for_btn = current_folder_for_rename.clone();
        let start_scan_for_folder_btn: Rc<dyn Fn(std::path::PathBuf)> =
            start_scan_for_folder_rename.clone();
        rename_btn.connect_clicked(move |btn| {
            let path = unsafe { btn.data::<String>("bound-path").map(|s| s.as_ref().clone()) };
            let Some(path) = path else { return };
            open_rename_dialog(
                &window_for_btn,
                &toast_for_btn,
                &start_scan_for_folder_btn,
                &current_folder_for_btn,
                std::path::PathBuf::from(path),
                None,
            );
        });
        let window_for_btn = window_for_delete.clone();
        let toast_for_btn = toast_for_delete.clone();
        let current_folder_for_btn = current_folder_for_delete.clone();
        let start_scan_for_folder_btn: Rc<dyn Fn(std::path::PathBuf)> =
            start_scan_for_folder_delete.clone();
        delete_btn.connect_clicked(move |btn| {
            let path = unsafe { btn.data::<String>("bound-path").map(|s| s.as_ref().clone()) };
            let Some(path) = path else { return };
            open_delete_dialog(
                &window_for_btn,
                &toast_for_btn,
                &start_scan_for_folder_btn,
                &current_folder_for_btn,
                std::path::PathBuf::from(path),
            );
        });
        let name_row = gtk4::Box::new(Orientation::Horizontal, 4);
        name_row.set_hexpand(true);
        name_row.set_halign(gtk4::Align::Fill);
        let action_box = gtk4::Box::new(Orientation::Horizontal, 2);
        action_box.append(&rename_btn);
        action_box.append(&delete_btn);
        name_row.append(&name_label);
        name_row.append(&action_box);
        let rename_btn_enter = rename_btn.clone();
        let rename_btn_leave = rename_btn.clone();
        let delete_btn_enter = delete_btn.clone();
        let delete_btn_leave = delete_btn.clone();
        let motion = EventControllerMotion::new();
        motion.connect_enter(move |_, _, _| {
            rename_btn_enter.set_opacity(1.0);
            delete_btn_enter.set_opacity(1.0);
        });
        motion.connect_leave(move |_| {
            rename_btn_leave.set_opacity(0.0);
            delete_btn_leave.set_opacity(0.0);
        });
        cell_box.add_controller(motion);
        cell_box.append(&thumb_image);
        cell_box.append(&name_row);
        list_item.set_child(Some(&cell_box));
    });

    let hash_cache_bind = hash_cache.clone();
    let thumbnail_size_bind = thumbnail_size.clone();
    let fast_scroll_active_bind = fast_scroll_active.clone();
    factory.connect_bind(move |_, obj| {
        let list_item = obj.downcast_ref::<ListItem>().unwrap();
        let path_str = list_item
            .item()
            .and_downcast::<StringObject>()
            .map(|s| s.string().to_string())
            .unwrap_or_default();

        let cell_box = list_item.child().and_downcast::<gtk4::Box>().unwrap();
        let thumb_image = cell_box.first_child().and_downcast::<Image>().unwrap();
        let name_row = cell_box.last_child().and_downcast::<gtk4::Box>().unwrap();
        let name_label = name_row.first_child().and_downcast::<Label>().unwrap();
        let action_box = name_row.last_child().and_downcast::<gtk4::Box>().unwrap();
        let rename_btn = action_box.first_child().and_downcast::<gtk4::Button>().unwrap();
        let delete_btn = action_box.last_child().and_downcast::<gtk4::Button>().unwrap();
        let size = *thumbnail_size_bind.borrow();
        cell_box.set_size_request(size + 12, size + 28);
        thumb_image.set_pixel_size(size);

        // Set filename label and placeholder icon synchronously (zero I/O).
        let filename = std::path::Path::new(&path_str)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        name_label.set_text(&filename);
        unsafe { rename_btn.set_data("bound-path", path_str.clone()); }
        unsafe { delete_btn.set_data("bound-path", path_str.clone()); }
        let generation_token = unsafe {
            thumb_image
                .data::<Rc<Cell<u64>>>("thumb-generation")
                .map(|token| token.as_ref().clone())
        };
        if let Some(generation_token) = generation_token {
            let expected_generation = generation_token.get().saturating_add(1);
            generation_token.set(expected_generation);
            if fast_scroll_active_bind.get() {
                // Fast scroll: set placeholder + path so the debounce refresh can find this cell.
                thumb_image.set_icon_name(Some("image-x-generic-symbolic"));
                unsafe { thumb_image.set_data("bound-path", path_str); }
            } else {
                load_grid_thumbnail(
                    &thumb_image,
                    path_str,
                    size,
                    hash_cache_bind.clone(),
                    generation_token,
                    expected_generation,
                );
            }
        }
    });

    factory.connect_unbind(|_, obj| {
        let list_item = obj.downcast_ref::<ListItem>().unwrap();
        if let Some(cell_box) = list_item.child().and_downcast::<gtk4::Box>() {
            if let Some(image) = cell_box.first_child().and_downcast::<Image>() {
                let generation_token = unsafe {
                    image
                        .data::<Rc<Cell<u64>>>("thumb-generation")
                        .map(|token| token.as_ref().clone())
                };
                if let Some(generation_token) = generation_token {
                    generation_token.set(generation_token.get().saturating_add(1));
                }
                unsafe { image.steal_data::<String>("bound-path"); }
                if let Some(name_row) = cell_box.last_child().and_downcast::<gtk4::Box>() {
                    if let Some(action_box) = name_row.last_child().and_downcast::<gtk4::Box>() {
                        if let Some(rename_btn) =
                            action_box.first_child().and_downcast::<gtk4::Button>()
                        {
                            unsafe { rename_btn.steal_data::<String>("bound-path"); }
                        }
                        if let Some(delete_btn) =
                            action_box.last_child().and_downcast::<gtk4::Button>()
                        {
                            unsafe { delete_btn.steal_data::<String>("bound-path"); }
                        }
                    }
                }
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

    let grid_overlay = Overlay::new();
    grid_overlay.set_hexpand(true);
    grid_overlay.set_vexpand(true);
    grid_overlay.set_child(Some(&grid_scroll));

    let scroll_flag_box = gtk4::Box::new(Orientation::Horizontal, 0);
    scroll_flag_box.set_visible(false);
    scroll_flag_box.set_halign(gtk4::Align::End);
    scroll_flag_box.set_valign(gtk4::Align::Start);
    scroll_flag_box.set_margin_end(12);
    scroll_flag_box.set_margin_top(12);
    scroll_flag_box.set_margin_start(12);
    scroll_flag_box.set_margin_bottom(12);

    let scroll_flag = Label::new(None);
    scroll_flag.add_css_class("title-4");
    scroll_flag.add_css_class("scroll-flag-bubble");
    scroll_flag.set_xalign(0.5);
    scroll_flag.set_margin_start(10);
    scroll_flag.set_margin_end(6);
    scroll_flag.set_margin_top(4);
    scroll_flag.set_margin_bottom(4);

    let scroll_flag_pointer = Label::new(Some("▶"));
    scroll_flag_pointer.add_css_class("title-4");
    scroll_flag_pointer.add_css_class("scroll-flag-pointer");
    scroll_flag_pointer.set_margin_start(0);
    scroll_flag_pointer.set_margin_end(0);
    scroll_flag_pointer.set_margin_top(0);
    scroll_flag_pointer.set_margin_bottom(0);

    scroll_flag_box.append(&scroll_flag);
    scroll_flag_box.append(&scroll_flag_pointer);
    grid_overlay.add_overlay(&scroll_flag_box);

    // Scroll-speed gate: suppress thumbnail spawning while scrolling faster than
    // 5 rows/sec, then refresh visible cells 150 ms after scrolling quiets down.
    {
        let adj = grid_scroll.vadjustment();
        let fast_scroll_active_adj = fast_scroll_active.clone();
        let scroll_last_pos_adj    = scroll_last_pos.clone();
        let scroll_last_time_adj   = scroll_last_time.clone();
        let scroll_debounce_gen_adj = scroll_debounce_gen.clone();
        let thumbnail_size_adj      = thumbnail_size.clone();
        let realized_adj            = realized_thumb_images.clone();
        let hash_cache_adj          = hash_cache.clone();
        let selection_model_adj     = selection_model.clone();
        let sort_key_adj            = sort_key.clone();
        let sort_fields_cache_adj   = sort_fields_cache.clone();
        let scroll_flag_adj         = scroll_flag.clone();
        let scroll_flag_box_adj     = scroll_flag_box.clone();
        let grid_scroll_adj         = grid_scroll.clone();
        adj.connect_value_changed(move |adj| {
            let now = Instant::now();
            let pos = adj.value();
            let cell_height = (*thumbnail_size_adj.borrow() + 24) as f64;
            let rows_per_sec = scroll_last_time_adj.get()
                .map(|last| {
                    let dt = now.duration_since(last).as_secs_f64();
                    if dt > 0.001 { (pos - scroll_last_pos_adj.get()).abs() / cell_height / dt }
                    else          { f64::INFINITY }
                })
                .unwrap_or(0.0);
            scroll_last_pos_adj.set(pos);
            scroll_last_time_adj.set(Some(now));
            fast_scroll_active_adj.set(rows_per_sec > 5.0);

            // Bump the generation so any previous pending timeout becomes a no-op.
            // We never cancel the old SourceId because one-shot timers are removed
            // by GLib when they fire, making SourceId::remove() panic on the stale id.
            let gen = scroll_debounce_gen_adj.get().wrapping_add(1);
            scroll_debounce_gen_adj.set(gen);
            let fsa            = fast_scroll_active_adj.clone();
            let realized       = realized_adj.clone();
            let hash_cache     = hash_cache_adj.clone();
            let thumbnail_size = thumbnail_size_adj.clone();
            let debounce_gen   = scroll_debounce_gen_adj.clone();
            let scroll_flag    = scroll_flag_adj.clone();
            let scroll_flag_box = scroll_flag_box_adj.clone();

            let total_items = selection_model_adj.n_items();
            if total_items > 0 {
                let thumb_size = (*thumbnail_size_adj.borrow()).max(1);
                let cell_width = (thumb_size + 4).max(1);
                let cell_height = (thumb_size + 20).max(1);
                let viewport_width = grid_scroll_adj.width().max(cell_width);
                let columns = (viewport_width / cell_width).max(1) as u32;
                let row = ((adj.value() / (cell_height as f64)).floor() as u32).saturating_mul(columns);
                let idx = row.min(total_items.saturating_sub(1));

                let text = selection_model_adj
                    .item(idx)
                    .and_downcast::<StringObject>()
                    .and_then(|obj| {
                        let path = obj.string().to_string();
                        sort_flag_text_for_path(
                            &path,
                            &sort_key_adj.borrow(),
                            &sort_fields_cache_adj.borrow(),
                        )
                    });

                if let Some(text) = text.filter(|t| !t.is_empty()) {
                    scroll_flag.set_text(&text);
                    let viewport_height = grid_scroll_adj.height().max(1) as f64;
                    let upper = adj.upper().max(1.0);
                    let page_size = adj.page_size().clamp(1.0, upper);
                    let range = (upper - page_size).max(1.0);
                    let ratio = (adj.value() / range).clamp(0.0, 1.0);
                    let thumb_height = ((page_size / upper) * viewport_height).clamp(18.0, viewport_height);
                    let thumb_top = ratio * (viewport_height - thumb_height);
                    let thumb_center = thumb_top + (thumb_height * 0.5);
                    let flag_height = 32.0;
                    let y = (thumb_center - (flag_height * 0.5))
                        .clamp(0.0, (viewport_height - flag_height).max(0.0)) as i32;
                    scroll_flag_box.set_margin_top(y);
                    scroll_flag_box.set_visible(true);
                } else {
                    scroll_flag_box.set_visible(false);
                }
            } else {
                scroll_flag_box.set_visible(false);
            }

            glib::timeout_add_local_once(Duration::from_millis(150), move || {
                if debounce_gen.get() != gen { return; }
                fsa.set(false);
                refresh_realized_grid_thumbnails(&realized, &thumbnail_size, &hash_cache);
            });
            let hide_gen = scroll_debounce_gen_adj.clone();
            glib::timeout_add_local_once(Duration::from_millis(450), move || {
                if hide_gen.get() != gen { return; }
                scroll_flag_box.set_visible(false);
            });
        });
    }

    // add_titled returns ViewStackPage — use it to set the page icon.
    let grid_page = view_stack.add_titled(&grid_overlay, Some("grid"), "Grid");
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
    right_sidebar.set_visible(initial_right_sidebar_visible);
    right_sidebar.set_margin_top(0);
    right_sidebar.set_margin_bottom(0);
    right_sidebar.set_margin_start(0);
    right_sidebar.set_margin_end(0);

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

    let meta_scroll = ScrolledWindow::new();
    meta_scroll.set_vexpand(true);
    meta_scroll.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);
    let meta_listbox = gtk4::ListBox::new();
    meta_listbox.add_css_class("boxed-list");
    meta_listbox.set_selection_mode(gtk4::SelectionMode::None);
    meta_scroll.set_child(Some(&meta_listbox));

    let meta_expander = Expander::new(Some("Metadata"));
    meta_expander.set_expanded(true);
    meta_expander.set_child(Some(&meta_scroll));
    meta_content.append(&meta_expander);
    let meta_split_before_auto_collapse: Rc<Cell<Option<i32>>> = Rc::new(Cell::new(None));

    // Vertical paned: preview (top) | metadata (bottom)
    let meta_paned = Paned::new(Orientation::Vertical);
    meta_paned.set_vexpand(true);
    meta_paned.set_start_child(Some(&meta_preview));
    meta_paned.set_end_child(Some(&meta_content));
    meta_paned.set_resize_start_child(true);
    meta_paned.set_resize_end_child(true);
    meta_paned.set_shrink_start_child(false);
    meta_paned.set_shrink_end_child(false);
    meta_paned.set_position(meta_pane_start_px);
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
    let toast_overlay_for_actions = toast_overlay.clone();
    copy_image_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let file = gio::File::for_path(&path);
        if let Ok(texture) = gdk::Texture::from_file(&file) {
            gtk4::prelude::WidgetExt::display(&window_for_actions)
                .clipboard()
                .set_texture(&texture);
            let toast = adw::Toast::new("Image copied to clipboard");
            toast.set_timeout(2);
            toast_overlay_for_actions.add_toast(toast);
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
    let meta_expander_for_actions = meta_expander.clone();
    let meta_paned_for_actions = meta_paned.clone();
    let meta_split_before_auto_collapse_for_actions = meta_split_before_auto_collapse.clone();
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
            apply_metadata_section_state(
                &row.meta,
                &meta_expander_for_actions,
                &meta_paned_for_actions,
                &meta_split_before_auto_collapse_for_actions,
            );

            let toast = adw::Toast::new("Metadata refreshed");
            toast.set_timeout(2);
            toast_overlay_for_actions.add_toast(toast);
        }
    });

    let current_folder_for_actions = current_folder.clone();
    let hash_cache_for_actions = hash_cache.clone();
    let start_scan_for_actions = start_scan_for_folder.clone();
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
        start_scan_for_actions(folder);
    });

    let current_folder_for_actions = current_folder.clone();
    let list_store_for_actions = list_store.clone();
    let start_scan_for_actions = start_scan_for_folder.clone();
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
        start_scan_for_actions(folder);
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
        pre_fullview_left_grid.set(left_toggle_grid.is_active());
        pre_fullview_right_grid.set(right_toggle_grid.is_active());
        stack_for_grid.set_visible_child_name("single");
        left_toggle_grid.set_active(false);
        right_toggle_grid.set_active(false);
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
                &picture_for_preview,
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
            pre_fullview_left_preview.set(left_toggle_preview.is_active());
            pre_fullview_right_preview.set(right_toggle_preview.is_active());
            stack_for_preview.set_visible_child_name("single");
            left_toggle_preview.set_active(false);
            right_toggle_preview.set_active(false);
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
        let realized_thumb_images_for_preview = realized_thumb_images_sel.clone();
        let thumbnail_size_for_preview = thumbnail_size_sel.clone();
        let hash_cache_for_preview = hash_cache_sel.clone();
        
        // Start metadata load in background (non-blocking).
        load_metadata_async(
            meta.clone(),
            meta_listbox_for_metadata,
            meta_expander_sel.clone(),
            meta_paned_sel.clone(),
            meta_split_before_auto_collapse_sel.clone(),
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
                        refresh_realized_grid_thumbnails(
                            &realized_thumb_images_for_preview,
                            &thumbnail_size_for_preview,
                            &hash_cache_for_preview,
                        );
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
                        refresh_realized_grid_thumbnails(
                            &realized_thumb_images_for_preview,
                            &thumbnail_size_for_preview,
                            &hash_cache_for_preview,
                        );
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
                        refresh_realized_grid_thumbnails(
                            &realized_thumb_images_for_preview,
                            &thumbnail_size_for_preview,
                            &hash_cache_for_preview,
                        );
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
        {
            let mut history = recent_folders_open_action.borrow_mut();
            push_recent_folder_entry(&mut history, path.as_path());
            config::save_recent_state(Some(path.as_path()), &history);
        }
        reset_tree_root_deferred(tree_root_open_action.clone(), path.clone());
        start_scan_open_action(path.clone());
        if sync_tree {
            sync_tree_to_path(&tree_model_open_action, &tree_list_view_open_action, &path);
        }
    });

    let history_list_show = history_list.clone();
    let history_popover_show = history_popover.clone();
    let recent_folders_show = recent_folders.clone();
    let current_folder_history = current_folder.clone();
    let open_folder_from_history = open_folder_action.clone();
    history_popover.connect_show(move |_| {
        while let Some(child) = history_list_show.first_child() {
            history_list_show.remove(&child);
        }

        let folders = recent_folders_show.borrow().clone();
        if folders.is_empty() {
            let empty_label = Label::new(Some("No recent folders"));
            empty_label.set_halign(gtk4::Align::Start);
            empty_label.add_css_class("dim-label");
            history_list_show.append(&empty_label);
            return;
        }

        for folder in folders.iter().take(RECENT_FOLDERS_LIMIT) {
            let label = folder.display().to_string();
            let row = gtk4::Box::new(Orientation::Horizontal, 6);
            row.set_halign(gtk4::Align::Fill);
            row.set_hexpand(true);

            let btn = gtk4::Button::new();
            btn.set_halign(gtk4::Align::Fill);
            btn.set_hexpand(true);
            btn.set_tooltip_text(Some(&label));
            btn.add_css_class("flat");
            let btn_label = Label::new(Some(&label));
            btn_label.set_xalign(0.0);
            btn.set_child(Some(&btn_label));

            let remove_btn = gtk4::Button::from_icon_name("edit-delete-symbolic");
            remove_btn.add_css_class("flat");
            remove_btn.set_tooltip_text(Some("Remove from history"));
            remove_btn.set_visible(false);

            row.append(&btn);
            row.append(&remove_btn);

            let path = folder.clone();
            let open_folder = open_folder_from_history.clone();
            let popover = history_popover_show.clone();
            btn.connect_clicked(move |_| {
                open_folder(path.clone(), true);
                popover.popdown();
            });

            let motion = EventControllerMotion::new();
            let remove_btn_enter = remove_btn.clone();
            motion.connect_enter(move |_, _, _| {
                remove_btn_enter.set_visible(true);
            });
            let remove_btn_leave = remove_btn.clone();
            motion.connect_leave(move |_| {
                remove_btn_leave.set_visible(false);
            });
            row.add_controller(motion);

            let path = folder.clone();
            let recent_folders_remove = recent_folders_show.clone();
            let history_list_remove = history_list_show.clone();
            let row_remove = row.clone();
            let current_folder_remove = current_folder_history.clone();
            remove_btn.connect_clicked(move |_| {
                recent_folders_remove
                    .borrow_mut()
                    .retain(|entry| entry != &path);
                {
                    let history = recent_folders_remove.borrow();
                    config::save_recent_state(current_folder_remove.borrow().as_deref(), &history);
                }
                history_list_remove.remove(&row_remove);
            });

            history_list_show.append(&row);
        }
    });

    let window_ref = window.clone();
    let current_folder_btn = current_folder.clone();
    let open_folder_btn = open_folder_action.clone();
    open_btn.connect_clicked(move |_| {
        let dialog = gtk4::FileDialog::builder().title("Choose a Folder").build();
        if let Some(folder) = current_folder_btn.borrow().as_ref() {
            let file = gio::File::for_path(folder);
            dialog.set_initial_folder(Some(&file));
        }
        let open_folder = open_folder_btn.clone();
        dialog.select_folder(
            Some(&window_ref),
            None::<&gio::Cancellable>,
            move |result| {
                let Ok(file) = result else { return };
                let Some(path) = file.path() else { return };
                open_folder(path, true);
            },
        );
    });

    // -----------------------------------------------------------------------
    // Wire: sort dropdown → update sort key and invalidate sorter
    // -----------------------------------------------------------------------
    let sort_key_dd = sort_key.clone();
    let sorter_dd = sorter.clone();
    let current_folder_dd = current_folder.clone();
    let scan_in_progress_dd = scan_in_progress.clone();
    let start_scan_dd = start_scan_for_folder.clone();
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
        *sort_key_clear.borrow_mut() = "name_asc".to_string();
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
                refresh_realized_grid_thumbnails(
                    &realized_thumb_images_toggle,
                    &thumbnail_size_toggle,
                    &hash_cache_toggle,
                );
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
    inner_paned.set_position(inner_pane_start_px);

    // Outer paned: left sidebar | (center + right)
    let outer_paned = Paned::new(Orientation::Horizontal);
    outer_paned.set_start_child(Some(&left_sidebar));
    outer_paned.set_end_child(Some(&inner_paned));
    outer_paned.set_resize_start_child(false);
    outer_paned.set_resize_end_child(true);
    outer_paned.set_shrink_start_child(false);
    outer_paned.set_shrink_end_child(false);
    outer_paned.set_position(left_pane_start_px);

    // Wrap content in ToastOverlay + bottom status bar → ToolbarView → window
    toast_overlay.set_child(Some(&outer_paned));
    toast_overlay.set_hexpand(true);
    toast_overlay.set_vexpand(true);

    let status_bar = gtk4::Box::new(Orientation::Horizontal, 0);
    status_bar.set_hexpand(true);
    status_bar.set_halign(gtk4::Align::Fill);
    status_bar.set_margin_start(8);
    status_bar.set_margin_end(8);
    status_bar.set_margin_top(2);
    status_bar.set_margin_bottom(2);
    status_bar.append(&progress_box);

    let update_banner = adw::Banner::new("");
    update_banner.set_button_label(Some("View release"));
    update_banner.set_revealed(false);

    let content_with_status = gtk4::Box::new(Orientation::Vertical, 0);
    content_with_status.set_hexpand(true);
    content_with_status.set_vexpand(true);
    content_with_status.append(&update_banner);
    content_with_status.append(&toast_overlay);
    content_with_status.append(&status_bar);

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header_bar);
    toolbar_view.set_content(Some(&content_with_status));

    window.set_content(Some(&toolbar_view));

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
    window.connect_close_request(move |_| {
        let window_width = window_for_close.width().max(1);
        let window_height = window_for_close.height().max(1);
        let window_maximized = window_for_close.is_maximized();
        let left_pos = outer_paned_close.position();
        let inner_pos = inner_paned_close.position();
        let meta_pos = meta_split_before_auto_collapse_close
            .get()
            .unwrap_or_else(|| meta_paned_close.position());
        let right_width = window_width.saturating_sub(left_pos + inner_pos);
        let meta_total_height = meta_paned_close.height().max(1);
        let recent_folders = recent_folders_close.borrow();

        config::save(
            cf_close.borrow().as_deref(),
            &recent_folders,
            window_width,
            window_height,
            window_maximized,
            left_pos,
            inner_pos,
            meta_pos,
            px_to_pct(left_pos, window_width),
            px_to_pct(right_width, window_width),
            px_to_pct(meta_pos, meta_total_height),
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
    let pane_restore_attempts = Rc::new(Cell::new(0_u8));
    let pane_restore_attempts_tick = pane_restore_attempts.clone();
    glib::timeout_add_local(Duration::from_millis(16), move || {
        let attempts = pane_restore_attempts_tick.get();
        if attempts >= 60 {
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
        inner_paned_restore.set_position(inner_pane_start_px);
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
        meta_paned_restore.set_position(meta_pane_start_px);
        glib::ControlFlow::Break
    });
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
        out.push(format!("Make: {}", v.as_str()));
    }
    if let Some(v) = &meta.camera_model {
        out.push(format!("Model: {}", v.as_str()));
    }
    if let Some(v) = &meta.exposure {
        out.push(format!("Exposure: {}", v.as_str()));
    }
    if let Some(v) = &meta.iso {
        out.push(format!("ISO: {}", v.as_str()));
    }
    if let Some(v) = &meta.prompt {
        out.push(format!("Prompt: {}", v.as_str()));
    }
    if let Some(v) = &meta.negative_prompt {
        out.push(format!("Neg. Prompt: {}", v.as_str()));
    }
    if let Some(v) = &meta.raw_parameters {
        out.push(format!("Parameters: {}", v.as_str()));
    }
    if let Some(v) = &meta.workflow_json {
        out.push(format!("Workflow: {}", v.as_str()));
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

fn json_copy_text(value: &JsonValue) -> String {
    match value {
        JsonValue::String(v) => v.clone(),
        JsonValue::Bool(v) => v.to_string(),
        JsonValue::Number(v) => v.to_string(),
        JsonValue::Null => "null".to_string(),
        JsonValue::Array(_) | JsonValue::Object(_) => {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        }
    }
}

fn json_display_value(value: &JsonValue) -> String {
    match value {
        JsonValue::String(v) => format!("\"{}\"", v),
        JsonValue::Bool(v) => v.to_string(),
        JsonValue::Number(v) => v.to_string(),
        JsonValue::Null => "null".to_string(),
        JsonValue::Array(values) => format!("[...] ({} items)", values.len()),
        JsonValue::Object(map) => format!("{{...}} ({} keys)", map.len()),
    }
}

fn add_copy_button_hover(row: &gtk4::Box, copy_button: &gtk4::Button) {
    copy_button.set_opacity(0.0);
    let motion = gtk4::EventControllerMotion::new();
    let copy_button_enter = copy_button.clone();
    motion.connect_enter(move |_, _, _| {
        copy_button_enter.set_opacity(1.0);
    });
    let copy_button_leave = copy_button.clone();
    motion.connect_leave(move |_| {
        copy_button_leave.set_opacity(0.0);
    });
    row.add_controller(motion);
}

fn append_json_node(parent: &gtk4::Box, key: Option<&str>, value: &JsonValue, depth: usize) {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    row.set_margin_start((depth as i32) * 14);
    row.set_hexpand(true);

    match value {
        JsonValue::Object(map) => {
            let title = match key {
                Some(k) => format!("\"{}\": {}", k, json_display_value(value)),
                None => json_display_value(value),
            };
            let expander = gtk4::Expander::new(Some(&title));
            expander.set_expanded(depth == 0);
            expander.set_hexpand(true);

            let children = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
            for (child_key, child_val) in map {
                append_json_node(&children, Some(child_key), child_val, depth + 1);
            }
            expander.set_child(Some(&children));
            row.append(&expander);
        }
        JsonValue::Array(items) => {
            let title = match key {
                Some(k) => format!("\"{}\": {}", k, json_display_value(value)),
                None => json_display_value(value),
            };
            let expander = gtk4::Expander::new(Some(&title));
            expander.set_expanded(depth == 0);
            expander.set_hexpand(true);

            let children = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
            for (idx, child_val) in items.iter().enumerate() {
                let idx_key = format!("[{}]", idx);
                append_json_node(&children, Some(&idx_key), child_val, depth + 1);
            }
            expander.set_child(Some(&children));
            row.append(&expander);
        }
        _ => {
            let text = match key {
                Some(k) => format!("\"{}\": {}", k, json_display_value(value)),
                None => json_display_value(value),
            };
            let label = gtk4::Label::new(Some(&text));
            label.set_halign(gtk4::Align::Start);
            label.set_xalign(0.0);
            label.set_hexpand(true);
            label.set_selectable(true);
            label.add_css_class("monospace");
            row.append(&label);
        }
    }

    let copy_text = json_copy_text(value);
    let copy_button = gtk4::Button::from_icon_name("edit-copy-symbolic");
    copy_button.add_css_class("flat");
    copy_button.add_css_class("circular");
    copy_button.set_tooltip_text(Some("Copy"));
    copy_button.connect_clicked(move |btn| {
        gtk4::prelude::WidgetExt::display(btn)
            .clipboard()
            .set_text(&copy_text);
    });
    add_copy_button_hover(&row, &copy_button);
    row.append(&copy_button);

    parent.append(&row);
}

fn build_json_metadata_widget(raw: &str) -> Option<gtk4::ScrolledWindow> {
    let value: JsonValue = serde_json::from_str(raw.trim()).ok()?;
    let tree = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    append_json_node(&tree, None, &value, 0);

    let scroller = gtk4::ScrolledWindow::new();
    scroller.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);
    scroller.set_min_content_height(130);
    scroller.set_child(Some(&tree));
    Some(scroller)
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
        let display_val = val.to_string();
        let row = adw::ActionRow::new();
        row.set_title(key);
        row.set_subtitle(&glib::markup_escape_text(&display_val));
        row.set_subtitle_selectable(true);
        let copy_text = display_val.clone();
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
        let display_val = val.to_string();

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

        let copy_text = display_val.clone();
        let copy_button = gtk4::Button::from_icon_name("edit-copy-symbolic");
        copy_button.add_css_class("flat");
        copy_button.set_tooltip_text(Some("Copy"));
        copy_button.connect_clicked(move |btn| {
            gtk4::prelude::WidgetExt::display(btn).clipboard().set_text(&copy_text);
        });
        header_box.append(&copy_button);
        row_box.append(&header_box);

        if let Some(json_widget) = build_json_metadata_widget(&display_val) {
            row_box.append(&json_widget);
        } else {
            // TextView: non-editable, word-wrapped; Pango layout is lazy/incremental.
            let text_view = gtk4::TextView::new();
            text_view.set_editable(false);
            text_view.set_cursor_visible(false);
            text_view.set_wrap_mode(gtk4::WrapMode::WordChar);
            text_view.set_hexpand(true);
            text_view.add_css_class("caption");
            text_view.add_css_class("metadata-text-view");
            text_view.buffer().set_text(&display_val);
            row_box.append(&text_view);
        }

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

fn metadata_has_content(meta: &ImageMetadata) -> bool {
    [
        meta.camera_make.as_ref(),
        meta.camera_model.as_ref(),
        meta.exposure.as_ref(),
        meta.iso.as_ref(),
        meta.prompt.as_ref(),
        meta.negative_prompt.as_ref(),
        meta.raw_parameters.as_ref(),
        meta.workflow_json.as_ref(),
    ]
    .iter()
    .any(|v| v.is_some())
}

fn apply_metadata_section_state(
    metadata: &ImageMetadata,
    meta_expander: &gtk4::Expander,
    meta_paned: &gtk4::Paned,
    meta_split_before_auto_collapse: &Rc<Cell<Option<i32>>>,
) {
    let has_content = metadata_has_content(metadata);
    let meta_total_height = meta_paned.height().max(1);
    let meta_upper_bound = meta_total_height.saturating_sub(MIN_META_SPLIT_PX);

    if has_content {
        meta_expander.set_expanded(true);
        if let Some(previous_pos) = meta_split_before_auto_collapse.get() {
            let restored_pos = if meta_upper_bound < MIN_META_SPLIT_PX {
                (meta_total_height / 2).max(1)
            } else {
                previous_pos.clamp(MIN_META_SPLIT_PX, meta_upper_bound)
            };
            meta_paned.set_position(restored_pos);
            meta_split_before_auto_collapse.set(None);
        }
    } else {
        if meta_split_before_auto_collapse.get().is_none() {
            meta_split_before_auto_collapse.set(Some(meta_paned.position()));
        }
        meta_expander.set_expanded(false);
        let collapsed_pos = if meta_upper_bound < MIN_META_SPLIT_PX {
            (meta_total_height / 2).max(1)
        } else {
            meta_upper_bound
        };
        meta_paned.set_position(collapsed_pos);
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
    meta_expander: gtk4::Expander,
    meta_paned: gtk4::Paned,
    meta_split_before_auto_collapse: Rc<Cell<Option<i32>>>,
    trace_state: Rc<RefCell<Option<ClickTrace>>>,
    click_id: u64,
) {
    glib::MainContext::default().spawn_local(async move {
        // Populate the sidebar (this is fast, all UI calls).
        populate_metadata_sidebar(&listbox, &metadata);
        apply_metadata_section_state(
            &metadata,
            &meta_expander,
            &meta_paned,
            &meta_split_before_auto_collapse,
        );

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

/// Replaces the tree root with exactly one folder entry.
fn reset_tree_root(tree_root: &gio::ListStore, root_path: &std::path::Path) {
    tree_root.remove_all();
    tree_root.append(&gio::File::for_path(root_path));
}

/// Schedules root replacement on the next main-loop tick to avoid mutating
/// the tree model while GTK is dispatching selection/model-change signals.
fn reset_tree_root_deferred(tree_root: gio::ListStore, root_path: std::path::PathBuf) {
    glib::timeout_add_local_once(Duration::from_millis(0), move || {
        reset_tree_root(&tree_root, root_path.as_path());
    });
}

/// Builds the root `ListStore` for the file tree.
/// Uses last opened folder when present, otherwise falls back to home.
fn build_tree_root(last_folder: Option<&std::path::PathBuf>) -> gio::ListStore {
    let store = gio::ListStore::new::<gio::File>();
    let root = match last_folder {
        Some(path) if path.is_dir() => path.clone(),
        _ => glib::home_dir(),
    };
    store.append(&gio::File::for_path(root));
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
