use crate::ui::shell::HeaderControls;
use crate::ui::tree::TreeWidgets;
use gtk4::gio;
use gtk4::ListView;
use libadwaita as adw;
use std::rc::Rc;

/// Cloned GTK handles for the header toolbar plus left tree panel, shared across installers.
#[derive(Clone)]
pub(crate) struct LeftChromeWiring {
    pub(crate) header_bar: adw::HeaderBar,
    pub(crate) sort_dropdown: gtk4::DropDown,
    pub(crate) size_buttons: Rc<Vec<gtk4::ToggleButton>>,
    pub(crate) search_entry: gtk4::SearchEntry,
    pub(crate) clear_btn: gtk4::Button,
    pub(crate) left_toggle: gtk4::ToggleButton,
    pub(crate) right_toggle: gtk4::ToggleButton,
    pub(crate) open_btn: gtk4::Button,
    pub(crate) history_list: gtk4::Box,
    pub(crate) history_popover: gtk4::Popover,
    pub(crate) initial_right_sidebar_visible: bool,
    pub(crate) left_sidebar: gtk4::Box,
    pub(crate) tree_root: gio::ListStore,
    pub(crate) tree_model: gtk4::TreeListModel,
    pub(crate) tree_selection: gtk4::SingleSelection,
    pub(crate) tree_list_view: ListView,
}

impl LeftChromeWiring {
    pub(crate) fn new(header: &HeaderControls, tree: &TreeWidgets) -> Self {
        Self {
            header_bar: header.header_bar.clone(),
            sort_dropdown: header.sort_dropdown.clone(),
            size_buttons: header.size_buttons.clone(),
            search_entry: header.search_entry.clone(),
            clear_btn: header.clear_btn.clone(),
            left_toggle: header.left_toggle.clone(),
            right_toggle: header.right_toggle.clone(),
            open_btn: header.open_btn.clone(),
            history_list: header.history_list.clone(),
            history_popover: header.history_popover.clone(),
            initial_right_sidebar_visible: header.initial_right_sidebar_visible,
            left_sidebar: tree.left_sidebar.clone(),
            tree_root: tree.tree_root.clone(),
            tree_model: tree.tree_model.clone(),
            tree_selection: tree.tree_selection.clone(),
            tree_list_view: tree.tree_list_view.clone(),
        }
    }
}
