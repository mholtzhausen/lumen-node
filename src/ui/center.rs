use crate::core::app_state::AppState;
use crate::ui::empty_state::{add_empty_status_overlay, create_empty_status_page};
use crate::ui::grid_loading::{add_grid_loading_overlay, create_grid_loading_overlay};
use crate::ui::grid::{
    add_scroll_flag_overlay, attach_compare_page, attach_grid_page, attach_single_page,
    build_scroll_flag_overlay, create_center_box, create_compare_view, create_grid_overlay,
    create_grid_scroll, create_grid_view, create_single_view, install_grid_factory,
    install_grid_scroll_speed_gate, set_default_grid_page, FullViewFavouriteHud, GridFactoryDeps,
};
use crate::ui::list_mutation::ListMutationContext;
use gtk4::{glib, MultiSelection};
use libadwaita as adw;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
};

pub(crate) struct CenterContentDeps {
    pub(crate) app_state: AppState,
    pub(crate) selection_model: MultiSelection,
    pub(crate) thumbnail_size: Rc<RefCell<i32>>,
    pub(crate) realized_cell_boxes: Rc<RefCell<Vec<glib::WeakRef<gtk4::Box>>>>,
    pub(crate) realized_thumb_images: Rc<RefCell<Vec<glib::WeakRef<gtk4::Image>>>>,
    pub(crate) fast_scroll_active: Rc<Cell<bool>>,
    pub(crate) filter: gtk4::CustomFilter,
    pub(crate) window: adw::ApplicationWindow,
    pub(crate) toast_overlay: adw::ToastOverlay,
    pub(crate) start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    pub(crate) thumb_generations: Rc<RefCell<HashMap<usize, Rc<Cell<u64>>>>>,
    pub(crate) bound_paths: Rc<RefCell<HashMap<usize, String>>>,
    pub(crate) tags_filter_btn: gtk4::MenuButton,
    pub(crate) tags_filter_list: gtk4::Box,
    pub(crate) full_view_favourite_icon: bool,
    pub(crate) full_view_favourite_icon_seconds: f64,
}

#[derive(Clone)]
pub(crate) struct CenterContentBundle {
    pub(crate) view_stack: adw::ViewStack,
    pub(crate) center_box: gtk4::Box,
    pub(crate) grid_view: gtk4::GridView,
    pub(crate) grid_scroll: gtk4::ScrolledWindow,
    pub(crate) single_picture: gtk4::Picture,
    /// Left (pinned) picture in side-by-side compare mode.
    pub(crate) compare_left_picture: gtk4::Picture,
    /// Right (selection) picture in side-by-side compare mode.
    pub(crate) compare_right_picture: gtk4::Picture,
    pub(crate) full_view_favourite_hud: FullViewFavouriteHud,
    pub(crate) empty_status_page: adw::StatusPage,
    pub(crate) empty_state_action_btn: gtk4::Button,
}

pub(crate) fn build_center_content(deps: CenterContentDeps) -> CenterContentBundle {
    let view_stack = adw::ViewStack::new();

    let factory = install_grid_factory(GridFactoryDeps {
        app_state: deps.app_state.clone(),
        thumbnail_size: deps.thumbnail_size.clone(),
        realized_cell_boxes: deps.realized_cell_boxes.clone(),
        realized_thumb_images: deps.realized_thumb_images.clone(),
        fast_scroll_active: deps.fast_scroll_active.clone(),
        filter: deps.filter.clone(),
        hash_cache: deps.app_state.hash_cache.clone(),
        window: deps.window.clone(),
        toast_overlay: deps.toast_overlay.clone(),
        mutation_ctx: ListMutationContext {
            app_state: deps.app_state.clone(),
            selection_model: deps.selection_model.clone(),
            start_scan_for_folder: deps.start_scan_for_folder.clone(),
        },
        thumb_generations: deps.thumb_generations.clone(),
        bound_paths: deps.bound_paths.clone(),
        tags_filter_btn: deps.tags_filter_btn.clone(),
        tags_filter_list: deps.tags_filter_list.clone(),
    });

    let grid_view = create_grid_view(&deps.selection_model, &factory);
    let grid_scroll = create_grid_scroll(&grid_view);
    let grid_overlay = create_grid_overlay(&grid_scroll);

    let (scroll_flag_box, scroll_flag) = build_scroll_flag_overlay();
    add_scroll_flag_overlay(&grid_overlay, &scroll_flag_box);

    let empty_state = create_empty_status_page();
    add_empty_status_overlay(&grid_overlay, &empty_state.page);

    let grid_loading = create_grid_loading_overlay();
    add_grid_loading_overlay(&grid_overlay, &grid_loading);
    *deps.app_state.grid_loading.borrow_mut() = Some(grid_loading.clone());

    install_grid_scroll_speed_gate(
        &grid_scroll,
        &grid_view,
        &deps.app_state,
        &deps.selection_model,
        &scroll_flag_box,
        &scroll_flag,
    );

    attach_grid_page(&view_stack, &grid_overlay);
    let (single_overlay, single_picture, full_view_favourite_hud) = create_single_view(
        deps.full_view_favourite_icon,
        deps.full_view_favourite_icon_seconds,
    );
    attach_single_page(&view_stack, &single_overlay);
    let (compare_overlay, compare_left_picture, compare_right_picture) = create_compare_view();
    attach_compare_page(&view_stack, &compare_overlay);
    set_default_grid_page(&view_stack);
    let center_box = create_center_box(&view_stack);

    CenterContentBundle {
        view_stack,
        center_box,
        grid_view,
        grid_scroll,
        single_picture,
        compare_left_picture,
        compare_right_picture,
        full_view_favourite_hud,
        empty_status_page: empty_state.page,
        empty_state_action_btn: empty_state.action_btn,
    }
}
