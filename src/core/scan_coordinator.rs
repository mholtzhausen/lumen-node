use crate::core::app_state::AppState;
use crate::scan::ScanMessage;
use crate::scanner::scan_directory;
use crate::sync_progress_widgets;
use crate::ui::grid::DEFER_GRID_THUMBNAILS_UNTIL_ENUM_COMPLETE;
use gtk4::{Label, ProgressBar};
use std::{path::PathBuf, rc::Rc, sync::atomic::Ordering as AtomicOrdering};

pub(crate) struct ScanCoordinatorDeps {
    pub(crate) app_state: AppState,
    pub(crate) sender: async_channel::Sender<ScanMessage>,
    pub(crate) progress_box: gtk4::Box,
    pub(crate) progress_label: Label,
    pub(crate) progress_bar: ProgressBar,
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

        deps.app_state.list_store.remove_all();
        deps.app_state.hash_cache.borrow_mut().clear();
        deps.app_state.meta_cache.borrow_mut().clear();
        deps.app_state.sort_fields_cache.borrow_mut().clear();
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
