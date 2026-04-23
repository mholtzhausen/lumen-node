use crate::scan::ScanMessage;
use crate::scanner::scan_directory;
use crate::ui::grid::DEFER_GRID_THUMBNAILS_UNTIL_ENUM_COMPLETE;
use crate::{sync_progress_widgets, ScanProgressState};
use gtk4::{gio, Label, ProgressBar};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    sync::atomic::Ordering as AtomicOrdering,
};

pub(crate) struct ScanCoordinatorDeps {
    pub(crate) list_store: gio::ListStore,
    pub(crate) sender: async_channel::Sender<ScanMessage>,
    pub(crate) hash_cache: Rc<RefCell<HashMap<String, String>>>,
    pub(crate) meta_cache: Rc<RefCell<HashMap<String, crate::ImageMetadata>>>,
    pub(crate) sort_fields_cache: Rc<RefCell<HashMap<String, crate::sort_flags::SortFields>>>,
    pub(crate) sort_key: Rc<RefCell<String>>,
    pub(crate) active_scan_generation: Rc<Cell<u64>>,
    pub(crate) scan_in_progress: Rc<Cell<bool>>,
    pub(crate) progress_state: Rc<RefCell<ScanProgressState>>,
    pub(crate) progress_box: gtk4::Box,
    pub(crate) progress_label: Label,
    pub(crate) progress_bar: ProgressBar,
}

pub(crate) fn build_start_scan_for_folder(
    deps: ScanCoordinatorDeps,
) -> Rc<dyn Fn(PathBuf)> {
    Rc::new(move |folder: PathBuf| {
        let generation = deps.active_scan_generation.get().saturating_add(1);
        deps.active_scan_generation.set(generation);
        deps.scan_in_progress.set(true);

        deps.list_store.remove_all();
        deps.hash_cache.borrow_mut().clear();
        deps.meta_cache.borrow_mut().clear();
        deps.sort_fields_cache.borrow_mut().clear();
        {
            let mut progress = deps.progress_state.borrow_mut();
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
            deps.sort_key.borrow().clone(),
            generation,
        );
    })
}
