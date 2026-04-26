use crate::config::AppConfig;
use crate::ui::left_chrome_wiring::LeftChromeWiring;
use crate::ui::shell::{build_header_controls, HeaderControls};
use crate::ui::tree::{build_tree_widgets, TreeWidgets};
use libadwaita as adw;

/// Adw header chrome plus left navigation tree, built in dependency order.
pub(crate) struct LeftChrome {
    pub(crate) header: HeaderControls,
    pub(crate) tree: TreeWidgets,
}

pub(crate) fn build_left_chrome(
    app_config: &AppConfig,
    initial_thumbnail_size: i32,
    window: &adw::ApplicationWindow,
    runtime_report: String,
) -> LeftChrome {
    let header = build_header_controls(app_config, initial_thumbnail_size, window, runtime_report);
    let tree = build_tree_widgets(
        app_config.last_folder.as_ref(),
        header.initial_left_sidebar_visible,
    );
    LeftChrome { header, tree }
}

impl LeftChrome {
    pub(crate) fn wiring_handles(&self) -> LeftChromeWiring {
        LeftChromeWiring::new(&self.header, &self.tree)
    }
}
