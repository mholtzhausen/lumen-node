use crate::core::app_state::AppState;
use crate::scan::ScanMessage;
use crate::scanner::scan_directory;
use crate::sync_progress_widgets;
use crate::ui::controls::set_similar_filter_chrome;
use crate::ui::grid::{set_default_grid_page, DEFER_GRID_THUMBNAILS_UNTIL_ENUM_COMPLETE};
use crate::ui::grid_loading::show_grid_loading;
use crate::ui::preview::clear_picture;
use gtk4::{Label, Picture, ProgressBar};
use libadwaita as adw;
use std::{cell::RefCell, path::PathBuf, rc::Rc, sync::atomic::Ordering as AtomicOrdering};

/// Widgets cleared when a folder scan starts (late-bound after center/sidebar exist).
pub(crate) struct ImmersiveResetHandles {
    pub(crate) view_stack: adw::ViewStack,
    pub(crate) single_picture: Picture,
    pub(crate) compare_left_picture: Picture,
    pub(crate) compare_right_picture: Picture,
    pub(crate) meta_preview: Picture,
}

pub(crate) struct ScanCoordinatorDeps {
    pub(crate) app_state: AppState,
    pub(crate) sender: async_channel::Sender<ScanMessage>,
    pub(crate) progress_box: gtk4::Box,
    pub(crate) progress_label: Label,
    pub(crate) progress_bar: ProgressBar,
    pub(crate) similar_filter_btn: gtk4::Button,
    pub(crate) immersive_reset: Rc<RefCell<Option<ImmersiveResetHandles>>>,
}

pub(crate) fn build_start_scan_for_folder(deps: ScanCoordinatorDeps) -> Rc<dyn Fn(PathBuf)> {
    Rc::new(move |folder: PathBuf| {
        let generation = deps
            .app_state
            .active_scan_generation
            .get()
            .saturating_add(1);
        deps.app_state.active_scan_generation.set(generation);
        deps.app_state.scan_in_progress.set(true);

        if let Some(handles) = deps.immersive_reset.borrow().as_ref() {
            set_default_grid_page(&handles.view_stack);
            clear_picture(&handles.single_picture);
            clear_picture(&handles.compare_left_picture);
            clear_picture(&handles.compare_right_picture);
            clear_picture(&handles.meta_preview);
        }

        deps.app_state.list_store.remove_all();
        deps.app_state.hash_cache.borrow_mut().clear();
        deps.app_state.meta_cache.borrow_mut().clear();
        deps.app_state.favourite_cache.borrow_mut().clear();
        deps.app_state.tags_cache.borrow_mut().clear();
        deps.app_state.prompt_similarity_index.borrow_mut().clear();
        *deps.app_state.similar_paths.borrow_mut() = None;
        set_similar_filter_chrome(&deps.similar_filter_btn, false);
        deps.app_state.sort_fields_cache.borrow_mut().clear();
        *deps.app_state.pinned_compare_path.borrow_mut() = None;
        show_grid_loading(&deps.app_state.grid_loading, "Loading…");
        {
            let mut progress = deps.app_state.progress_state.borrow_mut();
            progress.start_pending(generation);
            sync_progress_widgets(
                &progress,
                &deps.progress_box,
                &deps.progress_label,
                &deps.progress_bar,
            );
        }
        DEFER_GRID_THUMBNAILS_UNTIL_ENUM_COMPLETE.store(1, AtomicOrdering::Relaxed);
        scan_directory(
            folder,
            deps.sender.clone(),
            deps.app_state.sort_key.borrow().clone(),
            generation,
        );
    })
}
