use crate::sort_flags::SortFields;
use crate::ui::grid::{
    add_scroll_flag_overlay, attach_grid_page, attach_single_page, build_scroll_flag_overlay,
    create_center_box, create_grid_overlay, create_grid_scroll, create_grid_view,
    create_single_picture, install_grid_factory, install_grid_scroll_speed_gate,
    set_default_grid_page, GridFactoryDeps,
};
use gtk4::{glib, SingleSelection};
use libadwaita as adw;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    time::Instant,
};

pub(crate) struct CenterContentDeps {
    pub(crate) view_stack: adw::ViewStack,
    pub(crate) selection_model: SingleSelection,
    pub(crate) thumbnail_size: Rc<RefCell<i32>>,
    pub(crate) realized_cell_boxes: Rc<RefCell<Vec<glib::WeakRef<gtk4::Box>>>>,
    pub(crate) realized_thumb_images: Rc<RefCell<Vec<glib::WeakRef<gtk4::Image>>>>,
    pub(crate) fast_scroll_active: Rc<Cell<bool>>,
    pub(crate) scroll_last_pos: Rc<Cell<f64>>,
    pub(crate) scroll_last_time: Rc<Cell<Option<Instant>>>,
    pub(crate) scroll_debounce_gen: Rc<Cell<u64>>,
    pub(crate) hash_cache: Rc<RefCell<HashMap<String, String>>>,
    pub(crate) sort_key: Rc<RefCell<String>>,
    pub(crate) sort_fields_cache: Rc<RefCell<HashMap<String, SortFields>>>,
    pub(crate) window: adw::ApplicationWindow,
    pub(crate) toast_overlay: adw::ToastOverlay,
    pub(crate) start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    pub(crate) current_folder: Rc<RefCell<Option<PathBuf>>>,
}

pub(crate) struct CenterContentBundle {
    pub(crate) center_box: gtk4::Box,
    pub(crate) grid_view: gtk4::GridView,
    pub(crate) grid_scroll: gtk4::ScrolledWindow,
    pub(crate) single_picture: gtk4::Picture,
}

pub(crate) fn build_center_content(deps: CenterContentDeps) -> CenterContentBundle {
    let factory = install_grid_factory(GridFactoryDeps {
        thumbnail_size: deps.thumbnail_size.clone(),
        realized_cell_boxes: deps.realized_cell_boxes.clone(),
        realized_thumb_images: deps.realized_thumb_images.clone(),
        fast_scroll_active: deps.fast_scroll_active.clone(),
        hash_cache: deps.hash_cache.clone(),
        window: deps.window.clone(),
        toast_overlay: deps.toast_overlay.clone(),
        start_scan_for_folder: deps.start_scan_for_folder.clone(),
        current_folder: deps.current_folder.clone(),
    });

    let grid_view = create_grid_view(&deps.selection_model, &factory);
    let grid_scroll = create_grid_scroll(&grid_view);
    let grid_overlay = create_grid_overlay(&grid_scroll);

    let (scroll_flag_box, scroll_flag) = build_scroll_flag_overlay();
    add_scroll_flag_overlay(&grid_overlay, &scroll_flag_box);

    install_grid_scroll_speed_gate(
        &grid_scroll,
        &grid_view,
        &deps.fast_scroll_active,
        &deps.scroll_last_pos,
        &deps.scroll_last_time,
        &deps.scroll_debounce_gen,
        &deps.thumbnail_size,
        &deps.realized_thumb_images,
        &deps.hash_cache,
        &deps.selection_model,
        &deps.sort_key,
        &deps.sort_fields_cache,
        &scroll_flag_box,
        &scroll_flag,
    );

    attach_grid_page(&deps.view_stack, &grid_overlay);
    let single_picture = create_single_picture();
    attach_single_page(&deps.view_stack, &single_picture);
    set_default_grid_page(&deps.view_stack);
    let center_box = create_center_box(&deps.view_stack);

    CenterContentBundle {
        center_box,
        grid_view,
        grid_scroll,
        single_picture,
    }
}
