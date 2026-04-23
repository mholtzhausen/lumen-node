use crate::ui::sidebar::{
    append_meta_paned_to_sidebar, connect_meta_paned_dirty_tracking, create_meta_content_container,
    create_meta_expander, create_meta_paned, create_meta_position_programmatic,
    create_meta_preview_picture, create_meta_scroll_list, create_meta_split_before_auto_collapse,
    create_meta_split_dirty_flag, create_pane_restore_complete_flag, create_right_sidebar,
    initialize_meta_paned_position,
};
use gtk4::prelude::*;

pub(crate) struct RightSidebarDeps {
    pub(crate) initial_right_sidebar_visible: bool,
    pub(crate) meta_pane_start_px: i32,
}

#[derive(Clone)]
pub(crate) struct RightSidebarBundle {
    pub(crate) right_sidebar: gtk4::Box,
    pub(crate) meta_preview: gtk4::Picture,
    pub(crate) meta_listbox: gtk4::ListBox,
    pub(crate) meta_expander: gtk4::Expander,
    pub(crate) meta_split_before_auto_collapse: std::rc::Rc<std::cell::Cell<Option<i32>>>,
    pub(crate) meta_paned: gtk4::Paned,
    pub(crate) meta_position_programmatic: std::rc::Rc<std::cell::Cell<u32>>,
    pub(crate) meta_split_dirty: std::rc::Rc<std::cell::Cell<bool>>,
    pub(crate) pane_restore_complete: std::rc::Rc<std::cell::Cell<bool>>,
}

pub(crate) fn build_right_sidebar(deps: RightSidebarDeps) -> RightSidebarBundle {
    let right_sidebar = create_right_sidebar(deps.initial_right_sidebar_visible);

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
    initialize_meta_paned_position(
        &meta_paned,
        &meta_position_programmatic,
        deps.meta_pane_start_px,
    );
    connect_meta_paned_dirty_tracking(
        &meta_paned,
        &meta_position_programmatic,
        &meta_split_dirty,
        &pane_restore_complete,
    );
    append_meta_paned_to_sidebar(&right_sidebar, &meta_paned);

    RightSidebarBundle {
        right_sidebar,
        meta_preview,
        meta_listbox,
        meta_expander,
        meta_split_before_auto_collapse,
        meta_paned,
        meta_position_programmatic,
        meta_split_dirty,
        pane_restore_complete,
    }
}
