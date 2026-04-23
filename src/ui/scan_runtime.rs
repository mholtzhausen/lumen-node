use crate::scan::ScanMessage;
use crate::sort_flags::compute_sort_fields;
use crate::ui::grid::{
    refresh_realized_grid_thumbnails, DEFER_GRID_THUMBNAILS_UNTIL_ENUM_COMPLETE,
};
use crate::{
    sync_progress_widgets, ImageMetadata, ScanProgressState, SCAN_BUFFER_DEPTH,
    SCAN_DRAIN_BATCH_SIZE, SCAN_DRAIN_SCHEDULED,
};
use gtk4::prelude::*;
use gtk4::{gio, glib, Image, Label, ProgressBar, StringObject};
use libadwaita as adw;
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    rc::Rc,
    sync::atomic::Ordering as AtomicOrdering,
    time::Duration,
};

pub(crate) struct ScanRuntimeDeps {
    pub(crate) receiver: async_channel::Receiver<ScanMessage>,
    pub(crate) list_store: gio::ListStore,
    pub(crate) toast_overlay: adw::ToastOverlay,
    pub(crate) meta_cache: Rc<RefCell<HashMap<String, ImageMetadata>>>,
    pub(crate) hash_cache: Rc<RefCell<HashMap<String, String>>>,
    pub(crate) sort_fields_cache: Rc<RefCell<HashMap<String, crate::sort_flags::SortFields>>>,
    pub(crate) active_scan_generation: Rc<Cell<u64>>,
    pub(crate) scan_in_progress: Rc<Cell<bool>>,
    pub(crate) thumbnail_size: Rc<RefCell<i32>>,
    pub(crate) realized_thumb_images: Rc<RefCell<Vec<glib::WeakRef<Image>>>>,
    pub(crate) progress_state: Rc<RefCell<ScanProgressState>>,
    pub(crate) progress_box: gtk4::Box,
    pub(crate) progress_label: Label,
    pub(crate) progress_bar: ProgressBar,
}

pub(crate) fn install_scan_runtime(deps: ScanRuntimeDeps) {
    /// Maximum items drained from the buffer per idle tick.
    const BATCH_SIZE: usize = SCAN_DRAIN_BATCH_SIZE as usize;

    let buffer: Rc<RefCell<VecDeque<ScanMessage>>> = Rc::new(RefCell::new(VecDeque::new()));
    let drain_scheduled: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

    // Idle-priority drain callback: processes up to BATCH_SIZE messages per
    // tick, then yields control back to GTK for user-input events.
    let schedule_drain = {
        let buffer = buffer.clone();
        let drain_scheduled = drain_scheduled.clone();
        let list_store_recv = deps.list_store.clone();
        let hash_cache_recv = deps.hash_cache.clone();
        let meta_cache_recv = deps.meta_cache.clone();
        let sort_fields_cache_recv = deps.sort_fields_cache.clone();
        let active_scan_generation_recv = deps.active_scan_generation.clone();
        let scan_in_progress_recv = deps.scan_in_progress.clone();
        let toast_recv = deps.toast_overlay.clone();
        let progress_state_recv = deps.progress_state.clone();
        let progress_box_recv = deps.progress_box.clone();
        let progress_label_recv = deps.progress_label.clone();
        let progress_bar_recv = deps.progress_bar.clone();
        let thumbnail_size_recv = deps.thumbnail_size.clone();
        let realized_thumb_images_recv = deps.realized_thumb_images.clone();
        Rc::new(move || {
            if *drain_scheduled.borrow() {
                return;
            }
            *drain_scheduled.borrow_mut() = true;
            SCAN_DRAIN_SCHEDULED.store(1, AtomicOrdering::Relaxed);

            let buffer = buffer.clone();
            let drain_scheduled = drain_scheduled.clone();
            let list_store_recv = list_store_recv.clone();
            let hash_cache_recv = hash_cache_recv.clone();
            let meta_cache_recv = meta_cache_recv.clone();
            let sort_fields_cache_recv = sort_fields_cache_recv.clone();
            let active_scan_generation_recv = active_scan_generation_recv.clone();
            let scan_in_progress_recv = scan_in_progress_recv.clone();
            let toast_recv = toast_recv.clone();
            let thumbnail_size_recv = thumbnail_size_recv.clone();
            let realized_thumb_images_recv = realized_thumb_images_recv.clone();
            let progress_state_recv = progress_state_recv.clone();
            let progress_box_recv = progress_box_recv.clone();
            let progress_label_recv = progress_label_recv.clone();
            let progress_bar_recv = progress_bar_recv.clone();
            glib::idle_add_local(move || {
                *drain_scheduled.borrow_mut() = false;
                SCAN_DRAIN_SCHEDULED.store(0, AtomicOrdering::Relaxed);

                let mut batch: Vec<ScanMessage> = Vec::with_capacity(BATCH_SIZE);
                {
                    let mut buf = buffer.borrow_mut();
                    for _ in 0..BATCH_SIZE {
                        if let Some(msg) = buf.pop_front() {
                            batch.push(msg);
                        } else {
                            break;
                        }
                    }
                }
                if !batch.is_empty() {
                    SCAN_BUFFER_DEPTH.fetch_sub(batch.len() as u64, AtomicOrdering::Relaxed);
                }

                // Collect enumerated paths for batch splice.
                let mut new_paths: Vec<StringObject> = Vec::new();
                let mut scan_complete = false;
                let mut unlock_thumbnail_dispatch = false;
                let mut progress_changed = false;
                let active_generation = active_scan_generation_recv.get();

                for msg in batch {
                    match msg {
                        ScanMessage::ScanStarted {
                            total_count,
                            generation,
                        } => {
                            if generation != active_generation {
                                continue;
                            }
                            let mut progress = progress_state_recv.borrow_mut();
                            progress.begin_with_total(generation, total_count);
                            progress_changed = true;
                        }
                        ScanMessage::ImageEnumerated { path, generation } => {
                            if generation != active_generation {
                                continue;
                            }
                            sort_fields_cache_recv
                                .borrow_mut()
                                .entry(path.clone())
                                .or_insert_with(|| compute_sort_fields(&path));
                            new_paths.push(StringObject::new(&path));
                            let mut progress = progress_state_recv.borrow_mut();
                            if progress.generation == generation {
                                progress.enumerated_done = progress
                                    .enumerated_done
                                    .saturating_add(1)
                                    .min(progress.total_or_one());
                                progress_changed = true;
                            }
                        }
                        ScanMessage::EnumerationComplete { generation } => {
                            if generation != active_generation {
                                continue;
                            }
                            unlock_thumbnail_dispatch = true;
                            let mut progress = progress_state_recv.borrow_mut();
                            if progress.generation == generation {
                                progress.enumerated_done = progress.total_files;
                                progress_changed = true;
                            }
                        }
                        ScanMessage::ImageEnriched {
                            path,
                            hash,
                            meta,
                            indexed_from_cache,
                            generation,
                        } => {
                            if generation != active_generation {
                                continue;
                            }
                            let has_thumbnail_hash = !hash.is_empty();
                            if has_thumbnail_hash {
                                hash_cache_recv.borrow_mut().insert(path.clone(), hash);
                            }
                            meta_cache_recv.borrow_mut().insert(path, meta);
                            let mut progress = progress_state_recv.borrow_mut();
                            if progress.generation == generation {
                                progress.enriched_done = progress
                                    .enriched_done
                                    .saturating_add(1)
                                    .min(progress.total_or_one());
                                if indexed_from_cache {
                                    progress.enriched_cached =
                                        progress.enriched_cached.saturating_add(1);
                                } else {
                                    progress.enriched_generated =
                                        progress.enriched_generated.saturating_add(1);
                                }
                                if has_thumbnail_hash {
                                    progress.thumbnails_ready_done = progress
                                        .thumbnails_ready_done
                                        .saturating_add(1)
                                        .min(progress.total_or_one());
                                }
                                progress_changed = true;
                            }
                        }
                        ScanMessage::ScanComplete { generation } => {
                            if generation != active_generation {
                                continue;
                            }
                            scan_complete = true;
                            let mut progress = progress_state_recv.borrow_mut();
                            if progress.generation == generation {
                                progress.enumerated_done = progress.total_files;
                                progress.enriched_done = progress.total_files;
                                progress.thumbnails_ready_done = progress.total_files;
                                progress_changed = true;
                            }
                        }
                    }
                }

                // Batch-insert enumerated paths via splice (single model notification).
                if !new_paths.is_empty() {
                    list_store_recv.splice(list_store_recv.n_items(), 0, &new_paths);
                }

                if unlock_thumbnail_dispatch {
                    DEFER_GRID_THUMBNAILS_UNTIL_ENUM_COMPLETE.store(0, AtomicOrdering::Relaxed);
                    refresh_realized_grid_thumbnails(
                        &realized_thumb_images_recv,
                        &thumbnail_size_recv,
                        &hash_cache_recv,
                    );
                }

                if scan_complete {
                    DEFER_GRID_THUMBNAILS_UNTIL_ENUM_COMPLETE.store(0, AtomicOrdering::Relaxed);
                    scan_in_progress_recv.set(false);
                    let n = list_store_recv.n_items();
                    let mut total_size_bytes = 0_u64;
                    {
                        let cache = sort_fields_cache_recv.borrow();
                        for i in 0..list_store_recv.n_items() {
                            if let Some(item) =
                                list_store_recv.item(i).and_downcast::<StringObject>()
                            {
                                if let Some(fields) = cache.get(item.string().as_str()) {
                                    total_size_bytes = total_size_bytes.saturating_add(fields.size);
                                }
                            }
                        }
                    }
                    let text = format!("Found {} image{}", n, if n == 1 { "" } else { "s" });
                    let toast = adw::Toast::new(&text);
                    toast.set_timeout(3);
                    toast_recv.add_toast(toast);

                    let done_generation = active_generation;
                    let progress_state_done = progress_state_recv.clone();
                    let progress_box_done = progress_box_recv.clone();
                    let progress_label_done = progress_label_recv.clone();
                    let progress_bar_done = progress_bar_recv.clone();
                    glib::timeout_add_local_once(Duration::from_millis(900), move || {
                        let mut progress = progress_state_done.borrow_mut();
                        if progress.generation == done_generation {
                            progress.folder_image_count = n;
                            progress.folder_total_size_bytes = total_size_bytes;
                            progress.visible = false;
                            sync_progress_widgets(
                                &progress,
                                &progress_box_done,
                                &progress_label_done,
                                &progress_bar_done,
                            );
                        }
                    });
                }

                if progress_changed {
                    let progress = progress_state_recv.borrow();
                    sync_progress_widgets(
                        &progress,
                        &progress_box_recv,
                        &progress_label_recv,
                        &progress_bar_recv,
                    );
                }

                // Re-schedule if the buffer still has items.
                if !buffer.borrow().is_empty() {
                    *drain_scheduled.borrow_mut() = true;
                    SCAN_DRAIN_SCHEDULED.store(1, AtomicOrdering::Relaxed);
                    glib::ControlFlow::Continue
                } else {
                    glib::ControlFlow::Break
                }
            });
        })
    };

    // Async receiver: pushes messages into the buffer and triggers idle drain.
    let buffer_recv = buffer.clone();
    let schedule_drain_recv = schedule_drain.clone();
    glib::MainContext::default().spawn_local(async move {
        while let Ok(msg) = deps.receiver.recv().await {
            buffer_recv.borrow_mut().push_back(msg);
            SCAN_BUFFER_DEPTH.fetch_add(1, AtomicOrdering::Relaxed);
            schedule_drain_recv();
        }
    });
}
