use crate::config::AppConfig;
use crate::ui::shell::{assemble_paned_layout, mount_window_content, PanedLayout};
use crate::window_math::pct_to_px;
use libadwaita as adw;
use std::{cell::Cell, rc::Rc};

pub(crate) struct StartupPaneMetrics {
    pub(crate) left_pane_start_px: i32,
    pub(crate) inner_pane_start_px: i32,
    pub(crate) meta_pane_start_px: i32,
}

pub(crate) fn compute_startup_pane_metrics(
    app_config: &AppConfig,
    startup_window_width: i32,
    startup_window_height: i32,
    min_left_pane_px: i32,
    min_center_pane_px: i32,
    min_right_pane_px: i32,
    min_meta_split_px: i32,
) -> StartupPaneMetrics {
    let left_pane_start_px = app_config
        .left_pane_width_pct
        .map(|pct| pct_to_px(startup_window_width, pct))
        .or(app_config.left_pane_pos)
        .unwrap_or(220)
        .clamp(
            min_left_pane_px,
            startup_window_width - min_center_pane_px - min_right_pane_px,
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
        .saturating_sub(left_pane_start_px + min_center_pane_px)
        .max(min_right_pane_px);
    let right_pane_width_px = right_pane_width_px.clamp(min_right_pane_px, max_right_pane_width_px);
    let inner_pane_start_px = startup_window_width
        .saturating_sub(left_pane_start_px + right_pane_width_px)
        .max(min_center_pane_px);
    let meta_pane_start_px = app_config
        .meta_pane_height_pct
        .map(|pct| pct_to_px(startup_window_height, pct))
        .or(app_config.meta_pane_pos)
        .unwrap_or(200)
        .clamp(min_meta_split_px, startup_window_height - min_meta_split_px);

    StartupPaneMetrics {
        left_pane_start_px,
        inner_pane_start_px,
        meta_pane_start_px,
    }
}

pub(crate) struct LayoutMountDeps {
    pub(crate) left_sidebar: gtk4::Box,
    pub(crate) center_box: gtk4::Box,
    pub(crate) right_sidebar: gtk4::Box,
    pub(crate) pane_restore_complete: Rc<Cell<bool>>,
    pub(crate) left_pane_start_px: i32,
    pub(crate) inner_pane_start_px: i32,
    pub(crate) window: adw::ApplicationWindow,
    pub(crate) header_bar: adw::HeaderBar,
    pub(crate) toast_overlay: adw::ToastOverlay,
    pub(crate) progress_box: gtk4::Box,
}

pub(crate) struct LayoutMountBundle {
    pub(crate) paned_layout: PanedLayout,
    pub(crate) update_banner: adw::Banner,
}

pub(crate) fn assemble_and_mount_layout(deps: LayoutMountDeps) -> LayoutMountBundle {
    let paned_layout = assemble_paned_layout(
        &deps.left_sidebar,
        &deps.center_box,
        &deps.right_sidebar,
        &deps.pane_restore_complete,
        deps.left_pane_start_px,
        deps.inner_pane_start_px,
    );
    let update_banner = mount_window_content(
        &deps.window,
        &deps.header_bar,
        &deps.toast_overlay,
        &paned_layout.outer_paned,
        &deps.progress_box,
    );
    LayoutMountBundle {
        paned_layout,
        update_banner,
    }
}
