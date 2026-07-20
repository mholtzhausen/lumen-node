use crate::metadata_section::connect_meta_expander_paned_sync;
use crate::ui::sidebar::{
    append_meta_paned_to_sidebar, connect_meta_paned_dirty_tracking, create_meta_content_container,
    create_meta_expander, create_meta_paned, create_meta_position_programmatic,
    create_meta_preview, create_meta_scroll_list, create_meta_split_before_auto_collapse,
    create_meta_split_dirty_flag, create_pane_restore_complete_flag, create_right_sidebar,
    initialize_meta_paned_position, PreviewFavouriteIndicator,
};
use gtk4::prelude::*;
use std::{cell::Cell, rc::Rc};

pub(crate) struct RightSidebarDeps {
    pub(crate) initial_right_sidebar_visible: bool,
    pub(crate) initial_meta_section_expanded: bool,
    pub(crate) meta_pane_start_px: i32,
    pub(crate) min_meta_split_px: i32,
}

#[derive(Clone)]
pub(crate) struct RightSidebarBundle {
    pub(crate) right_sidebar: gtk4::Box,
    pub(crate) meta_preview: gtk4::Picture,
    pub(crate) meta_listbox: gtk4::ListBox,
    pub(crate) meta_expander: gtk4::Expander,
    pub(crate) preview_favourite: PreviewFavouriteIndicator,
    pub(crate) meta_split_before_auto_collapse: Rc<Cell<Option<i32>>>,
    pub(crate) meta_section_expanded_pref: Rc<Cell<bool>>,
    pub(crate) meta_paned: gtk4::Paned,
    pub(crate) meta_position_programmatic: Rc<Cell<u32>>,
    pub(crate) meta_split_dirty: Rc<Cell<bool>>,
    pub(crate) pane_restore_complete: Rc<Cell<bool>>,
}

pub(crate) fn build_right_sidebar(deps: RightSidebarDeps) -> RightSidebarBundle {
    let right_sidebar = create_right_sidebar(deps.initial_right_sidebar_visible);

    // Top pane: image preview (overlay hosts zoom-level HUD)
    let (meta_preview_host, meta_preview) = create_meta_preview();

    // Bottom pane: metadata list
    let meta_content = create_meta_content_container();
    let (meta_scroll, meta_listbox) = create_meta_scroll_list();
    let (meta_expander, preview_favourite) =
        create_meta_expander(&meta_scroll, deps.initial_meta_section_expanded);
    meta_content.append(&meta_expander);
    let meta_split_before_auto_collapse = create_meta_split_before_auto_collapse();
    let meta_section_expanded_pref = Rc::new(Cell::new(deps.initial_meta_section_expanded));

    // Vertical paned: preview (top) | metadata (bottom)
    let meta_paned = create_meta_paned(&meta_preview_host, &meta_content);
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
    connect_meta_expander_paned_sync(
        &meta_expander,
        &meta_paned,
        &meta_split_before_auto_collapse,
        &meta_position_programmatic,
        deps.min_meta_split_px,
        &meta_section_expanded_pref,
    );
    append_meta_paned_to_sidebar(&right_sidebar, &meta_paned);

    RightSidebarBundle {
        right_sidebar,
        meta_preview,
        meta_listbox,
        meta_expander,
        preview_favourite,
        meta_split_before_auto_collapse,
        meta_section_expanded_pref,
        meta_paned,
        meta_position_programmatic,
        meta_split_dirty,
        pane_restore_complete,
    }
}
