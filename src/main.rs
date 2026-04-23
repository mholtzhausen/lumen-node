mod byte_format;
mod config;
mod core;
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
mod services;
mod sort;
mod sort_flags;
mod thumbnail_sizing;
mod thumbnails;
mod timing_report;
mod tree_sidebar;
mod ui;
mod updater;
mod view_helpers;
mod window_math;

use byte_format::human_readable_bytes;
use core::app_state::build_app_state;
use core::scan_coordinator::{build_start_scan_for_folder, ScanCoordinatorDeps};
use metadata::ImageMetadata;
use scan::ScanMessage;
use sort::SORT_KEY_NAME_ASC;
use ui::center::{build_center_content, CenterContentDeps};
use ui::chrome::build_left_chrome;
use ui::layout::{assemble_and_mount_layout, compute_startup_pane_metrics, LayoutMountDeps};
use ui::lifecycle::{install_lifecycle, LifecycleDeps};
use ui::models::{build_model_bundle, ModelAssemblyDeps};
use ui::navigation::{install_navigation_handlers, NavigationDeps};
use ui::right_sidebar::{build_right_sidebar, RightSidebarDeps};
use ui::scan_runtime::{install_scan_runtime, ScanRuntimeDeps};
use ui::shell::{create_progress_widgets, create_window_with_defaults};
use ui::sidebar::connect_sidebar_visibility_toggles;
use ui::tree::{install_tree_folder_selection, TreeFolderSelectionDeps};
use ui::wiring::{
    install_context_menu_wiring, install_controls_wiring, install_open_folder_wiring,
    install_selection_wiring, ContextMenuWiringDeps, ControlsWiringDeps, OpenFolderWiringDeps,
    SelectionWiringDeps,
};

use std::{cell::Cell, rc::Rc, sync::atomic::AtomicU64};

use adw::prelude::*;
use gtk4::{glib, Label, ProgressBar};
use libadwaita as adw;

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
    let configured_right_pane_width_pct = app_config.right_pane_width_pct;
    let configured_right_pane_pos = app_config.right_pane_pos;
    let configured_left_pane_width_pct = app_config.left_pane_width_pct;
    let configured_left_pane_pos = app_config.left_pane_pos;
    let configured_meta_pane_height_pct = app_config.meta_pane_height_pct;
    let configured_meta_pane_pos = app_config.meta_pane_pos;

    let app_state = build_app_state(
        &app_config,
        RECENT_FOLDERS_LIMIT,
        SORT_KEY_NAME_ASC,
        thumbnails::THUMB_NORMAL_SIZE,
    );
    let current_folder = app_state.current_folder.clone();
    let recent_folders = app_state.recent_folders.clone();

    // Async channel: background scan thread → GTK main thread.
    // Bounded to provide backpressure when the UI can't keep up.
    let (sender, receiver) = async_channel::bounded::<ScanMessage>(200);

    // Reusable multi-phase progress indicator shown while scanning/indexing.
    let (progress_box, progress_label, progress_bar) = create_progress_widgets();

    // Toast overlay wraps all main content for non-intrusive notifications.
    let toast_overlay = adw::ToastOverlay::new();

    let hash_cache = app_state.hash_cache.clone();
    let sort_fields_cache = app_state.sort_fields_cache.clone();
    let sort_key = app_state.sort_key.clone();
    let search_text = app_state.search_text.clone();
    let initial_thumbnail_size = app_state.initial_thumbnail_size;
    let thumbnail_size = app_state.thumbnail_size.clone();
    let realized_thumb_images = app_state.realized_thumb_images.clone();
    let realized_cell_boxes = app_state.realized_cell_boxes.clone();
    let fast_scroll_active = app_state.fast_scroll_active.clone();
    let scroll_last_pos = app_state.scroll_last_pos.clone();
    let scroll_last_time = app_state.scroll_last_time.clone();
    let scroll_debounce_gen = app_state.scroll_debounce_gen.clone();

    install_scan_runtime(ScanRuntimeDeps {
        receiver,
        app_state: app_state.clone(),
        toast_overlay: toast_overlay.clone(),
        progress_box: progress_box.clone(),
        progress_label: progress_label.clone(),
        progress_bar: progress_bar.clone(),
    });

    // -----------------------------------------------------------------------
    // Header chrome + left file-system tree (tree visibility follows header toggle)
    // -----------------------------------------------------------------------
    let left_chrome = build_left_chrome(&app_config, initial_thumbnail_size);
    let chrome = left_chrome.wiring_handles();

    let start_scan_for_folder = build_start_scan_for_folder(ScanCoordinatorDeps {
        app_state: app_state.clone(),
        sender: sender.clone(),
        progress_box: progress_box.clone(),
        progress_label: progress_label.clone(),
        progress_bar: progress_bar.clone(),
    });

    install_tree_folder_selection(TreeFolderSelectionDeps {
        app_state: app_state.clone(),
        chrome: chrome.clone(),
        start_scan_for_folder: start_scan_for_folder.clone(),
        recent_folders_limit: RECENT_FOLDERS_LIMIT,
    });

    let model_bundle = build_model_bundle(ModelAssemblyDeps {
        app_state: app_state.clone(),
    });
    let filter = model_bundle.filter;
    let sorter = model_bundle.sorter;
    let _filter_model = model_bundle.filter_model;
    let _sort_model = model_bundle.sort_model;
    let selection_model = model_bundle.selection_model;

    let center = build_center_content(CenterContentDeps {
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

    // --- Right sidebar: preview (top) + metadata list (bottom) ---

    let startup_window_width = DEFAULT_WINDOW_WIDTH;
    let startup_window_height = DEFAULT_WINDOW_HEIGHT;
    let pane_metrics = compute_startup_pane_metrics(
        &app_config,
        startup_window_width,
        startup_window_height,
        MIN_LEFT_PANE_PX,
        MIN_CENTER_PANE_PX,
        MIN_RIGHT_PANE_PX,
        MIN_META_SPLIT_PX,
    );

    let right = build_right_sidebar(RightSidebarDeps {
        initial_right_sidebar_visible: chrome.initial_right_sidebar_visible,
        meta_pane_start_px: pane_metrics.meta_pane_start_px,
    });

    install_context_menu_wiring(ContextMenuWiringDeps {
        app_state: app_state.clone(),
        window: window.clone(),
        toast_overlay: toast_overlay.clone(),
        selection_model: selection_model.clone(),
        center: center.clone(),
        right: right.clone(),
        min_meta_split_px: MIN_META_SPLIT_PX,
        start_scan_for_folder: start_scan_for_folder.clone(),
    });

    // -----------------------------------------------------------------------
    // Wire: sidebar toggle buttons → show/hide panels
    // -----------------------------------------------------------------------
    connect_sidebar_visibility_toggles(
        &chrome.left_toggle,
        &chrome.left_sidebar,
        &chrome.right_toggle,
        &right.right_sidebar,
    );

    let pre_fullview_left: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let pre_fullview_right: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    install_navigation_handlers(NavigationDeps {
        center: center.clone(),
        right: right.clone(),
        selection_model: selection_model.clone(),
        left_toggle: chrome.left_toggle.clone(),
        right_toggle: chrome.right_toggle.clone(),
        pre_fullview_left: pre_fullview_left.clone(),
        pre_fullview_right: pre_fullview_right.clone(),
    });

    install_selection_wiring(SelectionWiringDeps {
        app_state: app_state.clone(),
        selection_model: selection_model.clone(),
        right: right.clone(),
    });

    // -----------------------------------------------------------------------
    // Wire: open_btn → FileDialog → start scan (quick-jump shortcut)
    // -----------------------------------------------------------------------
    let open_folder_action = install_open_folder_wiring(OpenFolderWiringDeps {
        app_state: app_state.clone(),
        start_scan_for_folder: start_scan_for_folder.clone(),
        chrome: chrome.clone(),
        filter: filter.clone(),
        sorter: sorter.clone(),
        recent_folders_limit: RECENT_FOLDERS_LIMIT,
        window: window.clone(),
    });

    // -----------------------------------------------------------------------
    // Wire: sort/search/clear/thumbnail-size controls
    // -----------------------------------------------------------------------
    install_controls_wiring(ControlsWiringDeps {
        app_state: app_state.clone(),
        chrome: chrome.clone(),
        center: center.clone(),
        sorter: sorter.clone(),
        start_scan_for_folder: start_scan_for_folder.clone(),
        filter: filter.clone(),
    });

    let layout_bundle = assemble_and_mount_layout(LayoutMountDeps {
        left_sidebar: chrome.left_sidebar.clone(),
        center_box: center.center_box.clone(),
        right_sidebar: right.right_sidebar.clone(),
        pane_restore_complete: right.pane_restore_complete.clone(),
        left_pane_start_px: pane_metrics.left_pane_start_px,
        inner_pane_start_px: pane_metrics.inner_pane_start_px,
        window: window.clone(),
        header_bar: chrome.header_bar.clone(),
        toast_overlay: toast_overlay.clone(),
        progress_box: progress_box.clone(),
    });
    let paned_layout = layout_bundle.paned_layout;
    let inner_paned = paned_layout.inner_paned.clone();
    let outer_paned = paned_layout.outer_paned.clone();
    let inner_position_programmatic = paned_layout.inner_position_programmatic.clone();
    let inner_split_dirty = paned_layout.inner_split_dirty.clone();
    let outer_position_programmatic = paned_layout.outer_position_programmatic.clone();
    let outer_split_dirty = paned_layout.outer_split_dirty.clone();
    let update_banner = layout_bundle.update_banner;

    install_lifecycle(LifecycleDeps {
        update_banner,
        window: window.clone(),
        center: center.clone(),
        right: right.clone(),
        selection_model: selection_model.clone(),
        thumbnail_size: thumbnail_size.clone(),
        toast_overlay: toast_overlay.clone(),
        current_folder: current_folder.clone(),
        start_scan_for_folder: start_scan_for_folder.clone(),
        chrome: chrome.clone(),
        pre_fullview_left: pre_fullview_left.clone(),
        pre_fullview_right: pre_fullview_right.clone(),
        outer_paned: outer_paned.clone(),
        inner_paned: inner_paned.clone(),
        sort_key: sort_key.clone(),
        search_text: search_text.clone(),
        recent_folders: recent_folders.clone(),
        outer_split_dirty: outer_split_dirty.clone(),
        inner_split_dirty: inner_split_dirty.clone(),
        configured_left_pane_pos,
        configured_right_pane_pos,
        configured_meta_pane_pos,
        configured_left_pane_width_pct,
        configured_right_pane_width_pct,
        configured_meta_pane_height_pct,
        min_meta_split_px: MIN_META_SPLIT_PX,
        app_config,
        open_folder_action: open_folder_action.clone(),
        filter: filter.clone(),
        outer_position_programmatic: outer_position_programmatic.clone(),
        inner_position_programmatic: inner_position_programmatic.clone(),
        min_left_pane_px: MIN_LEFT_PANE_PX,
        min_right_pane_px: MIN_RIGHT_PANE_PX,
        min_center_pane_px: MIN_CENTER_PANE_PX,
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
