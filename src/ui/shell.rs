use crate::config::AppConfig;
use crate::window_math::monitor_bounds_for_window;
use gtk4::{Orientation, Paned, ProgressBar};
use gtk4::prelude::*;
use libadwaita as adw;
use adw::prelude::*;
use std::{cell::Cell, rc::Rc};

pub(crate) struct PanedLayout {
    pub(crate) inner_paned: Paned,
    pub(crate) outer_paned: Paned,
    pub(crate) inner_position_programmatic: Rc<Cell<u32>>,
    pub(crate) inner_split_dirty: Rc<Cell<bool>>,
    pub(crate) outer_position_programmatic: Rc<Cell<u32>>,
    pub(crate) outer_split_dirty: Rc<Cell<bool>>,
}

pub(crate) fn create_window_with_defaults(
    app: &adw::Application,
    app_config: &AppConfig,
    default_window_width: i32,
    default_window_height: i32,
    min_left_pane_px: i32,
    min_center_pane_px: i32,
    min_right_pane_px: i32,
    min_meta_split_px: i32,
) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::new(app);
    window.set_title(Some("LumenNode"));

    let (monitor_width, monitor_height) = monitor_bounds_for_window(&window);
    let min_window_width = min_left_pane_px + min_center_pane_px + min_right_pane_px;
    let min_window_height = (min_meta_split_px * 2).max(360);
    let initial_window_width = app_config
        .window_width
        .unwrap_or(default_window_width)
        .clamp(min_window_width, monitor_width.max(min_window_width));
    let initial_window_height = app_config
        .window_height
        .unwrap_or(default_window_height)
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

    window
}

pub(crate) fn assemble_paned_layout(
    left_sidebar: &gtk4::Box,
    center_box: &gtk4::Box,
    right_sidebar: &gtk4::Box,
    pane_restore_complete: &Rc<Cell<bool>>,
    left_pane_start_px: i32,
    inner_pane_start_px: i32,
) -> PanedLayout {
    let inner_paned = Paned::new(Orientation::Horizontal);
    inner_paned.set_start_child(Some(center_box));
    inner_paned.set_end_child(Some(right_sidebar));
    inner_paned.set_resize_start_child(true);
    inner_paned.set_resize_end_child(false);
    inner_paned.set_shrink_start_child(false);
    inner_paned.set_shrink_end_child(false);
    let inner_position_programmatic = Rc::new(Cell::new(0_u32));
    let inner_split_dirty = Rc::new(Cell::new(false));
    inner_position_programmatic.set(inner_position_programmatic.get().saturating_add(1));
    inner_paned.set_position(inner_pane_start_px);
    inner_position_programmatic.set(inner_position_programmatic.get().saturating_sub(1));
    {
        let inner_position_programmatic = inner_position_programmatic.clone();
        let inner_split_dirty = inner_split_dirty.clone();
        let pane_restore_complete = pane_restore_complete.clone();
        inner_paned.connect_notify_local(Some("position"), move |_, _| {
            if !pane_restore_complete.get() {
                return;
            }
            if inner_position_programmatic.get() != 0 {
                return;
            }
            inner_split_dirty.set(true);
        });
    }

    let outer_paned = Paned::new(Orientation::Horizontal);
    outer_paned.set_start_child(Some(left_sidebar));
    outer_paned.set_end_child(Some(&inner_paned));
    outer_paned.set_resize_start_child(false);
    outer_paned.set_resize_end_child(true);
    outer_paned.set_shrink_start_child(false);
    outer_paned.set_shrink_end_child(false);
    let outer_position_programmatic = Rc::new(Cell::new(0_u32));
    let outer_split_dirty = Rc::new(Cell::new(false));
    outer_position_programmatic.set(outer_position_programmatic.get().saturating_add(1));
    outer_paned.set_position(left_pane_start_px);
    outer_position_programmatic.set(outer_position_programmatic.get().saturating_sub(1));
    {
        let outer_position_programmatic = outer_position_programmatic.clone();
        let outer_split_dirty = outer_split_dirty.clone();
        let pane_restore_complete = pane_restore_complete.clone();
        outer_paned.connect_notify_local(Some("position"), move |_, _| {
            if !pane_restore_complete.get() {
                return;
            }
            if outer_position_programmatic.get() != 0 {
                return;
            }
            outer_split_dirty.set(true);
        });
    }

    PanedLayout {
        inner_paned,
        outer_paned,
        inner_position_programmatic,
        inner_split_dirty,
        outer_position_programmatic,
        outer_split_dirty,
    }
}

pub(crate) fn mount_window_content(
    window: &adw::ApplicationWindow,
    header_bar: &adw::HeaderBar,
    toast_overlay: &adw::ToastOverlay,
    outer_paned: &Paned,
    progress_box: &gtk4::Box,
) -> adw::Banner {
    toast_overlay.set_child(Some(outer_paned));
    toast_overlay.set_hexpand(true);
    toast_overlay.set_vexpand(true);

    let status_bar = gtk4::Box::new(Orientation::Horizontal, 0);
    status_bar.set_hexpand(true);
    status_bar.set_halign(gtk4::Align::Fill);
    status_bar.set_margin_start(8);
    status_bar.set_margin_end(8);
    status_bar.set_margin_top(2);
    status_bar.set_margin_bottom(2);
    status_bar.append(progress_box);

    let update_banner = adw::Banner::new("");
    update_banner.set_button_label(Some("View release"));
    update_banner.set_revealed(false);

    let content_with_status = gtk4::Box::new(Orientation::Vertical, 0);
    content_with_status.set_hexpand(true);
    content_with_status.set_vexpand(true);
    content_with_status.append(&update_banner);
    content_with_status.append(toast_overlay);
    content_with_status.append(&status_bar);

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(header_bar);
    toolbar_view.set_content(Some(&content_with_status));
    window.set_content(Some(&toolbar_view));

    update_banner
}

pub(crate) fn create_progress_widgets() -> (gtk4::Box, gtk4::Label, ProgressBar) {
    let progress_box = gtk4::Box::new(Orientation::Horizontal, 6);
    progress_box.set_visible(true);
    progress_box.set_halign(gtk4::Align::Start);
    progress_box.set_valign(gtk4::Align::Center);

    let progress_label = gtk4::Label::new(Some("Scanning folder..."));
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
    (progress_box, progress_label, progress_bar)
}
