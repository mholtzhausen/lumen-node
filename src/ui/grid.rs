use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, Box as GtkBox, Image};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use crate::{db, thumbnails};

pub static ACTIVE_THUMBNAIL_TASKS: AtomicU64 = AtomicU64::new(0);
pub static THUMB_UI_CALLBACKS_TOTAL: AtomicU64 = AtomicU64::new(0);
pub static DEFER_GRID_THUMBNAILS_UNTIL_ENUM_COMPLETE: AtomicU64 = AtomicU64::new(0);

struct AtomicTaskGuard {
    counter: &'static AtomicU64,
}

impl AtomicTaskGuard {
    fn new(counter: &'static AtomicU64) -> Self {
        counter.fetch_add(1, AtomicOrdering::Relaxed);
        Self { counter }
    }
}

impl Drop for AtomicTaskGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, AtomicOrdering::Relaxed);
    }
}

pub fn track_realized_grid_widgets(
    realized_cell_boxes: &Rc<RefCell<Vec<glib::WeakRef<GtkBox>>>>,
    realized_thumb_images: &Rc<RefCell<Vec<glib::WeakRef<Image>>>>,
    cell_box: &GtkBox,
    thumb_image: &Image,
) {
    realized_cell_boxes.borrow_mut().push(cell_box.downgrade());
    realized_thumb_images.borrow_mut().push(thumb_image.downgrade());
}

pub fn refresh_realized_grid_cell_sizes(
    realized_cell_boxes: &Rc<RefCell<Vec<glib::WeakRef<GtkBox>>>>,
    selected_size: i32,
) {
    let mut boxes = realized_cell_boxes.borrow_mut();
    boxes.retain(|weak| weak.upgrade().is_some());
    for weak in boxes.iter() {
        if let Some(cell_box) = weak.upgrade() {
            cell_box.set_size_request(selected_size + 4, selected_size + 20);
        }
    }
}

pub fn load_grid_thumbnail(
    thumb_image: &Image,
    path_str: String,
    size: i32,
    hash_cache: Rc<RefCell<HashMap<String, String>>>,
    generation_token: Rc<Cell<u64>>,
    expected_generation: u64,
) {
    thumb_image.set_icon_name(Some("image-x-generic-symbolic"));
    unsafe {
        thumb_image.set_data("bound-path", path_str.clone());
    }

    if DEFER_GRID_THUMBNAILS_UNTIL_ENUM_COMPLETE.load(AtomicOrdering::Relaxed) != 0 {
        return;
    }

    let cached_hash = hash_cache.borrow().get(&path_str).cloned();
    let already_loaded = if let Some(ref hash) = cached_hash {
        if let Some(thumb) = thumbnails::hash_thumb_if_exists_for_size(hash, size) {
            if let Ok(pb) = gdk_pixbuf::Pixbuf::from_file(&thumb) {
                let tex = gdk::Texture::for_pixbuf(&pb);
                thumb_image.set_paintable(Some(&tex));
                true
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    if already_loaded {
        return;
    }

    const MAX_THUMBNAIL_TASKS: u64 = 64;
    if ACTIVE_THUMBNAIL_TASKS.load(AtomicOrdering::Relaxed) >= MAX_THUMBNAIL_TASKS {
        return;
    }
    let task_guard = AtomicTaskGuard::new(&ACTIVE_THUMBNAIL_TASKS);

    let path_for_thread = std::path::PathBuf::from(&path_str);
    let cached_hash_for_task = cached_hash.clone();
    let task = gio::spawn_blocking(move || {
        let _guard = task_guard;

        if let Some(hash) = cached_hash_for_task {
            let thumb = thumbnails::generate_hash_thumbnail_for_size(&path_for_thread, &hash, size);
            return (thumb, Some(hash));
        }

        if size == thumbnails::THUMB_NORMAL_SIZE {
            return (thumbnails::ensure_thumbnail(&path_for_thread), None);
        }

        let Ok(hash) = db::hash_file(&path_for_thread) else {
            return (None, None);
        };
        let thumb = thumbnails::generate_hash_thumbnail_for_size(&path_for_thread, &hash, size);
        (thumb, Some(hash))
    });

    let image_weak = thumb_image.downgrade();
    glib::MainContext::default().spawn_local(async move {
        THUMB_UI_CALLBACKS_TOTAL.fetch_add(1, AtomicOrdering::Relaxed);
        let Ok((maybe_cache, resolved_hash)) = task.await else {
            return;
        };
        if generation_token.get() != expected_generation {
            return;
        }
        let Some(image) = image_weak.upgrade() else {
            return;
        };
        let is_current = unsafe {
            image
                .data::<String>("bound-path")
                .map(|p| p.as_ref().as_str() == path_str.as_str())
                .unwrap_or(false)
        };
        if !is_current {
            return;
        }
        if let Some(hash) = resolved_hash {
            hash_cache.borrow_mut().insert(path_str.clone(), hash);
        }
        match maybe_cache.and_then(|p| gdk_pixbuf::Pixbuf::from_file(&p).ok()) {
            Some(pb) => {
                let tex = gdk::Texture::for_pixbuf(&pb);
                image.set_paintable(Some(&tex));
            }
            None => image.set_icon_name(Some("image-missing-symbolic")),
        }
    });
}

pub fn refresh_realized_grid_thumbnails(
    realized_thumb_images: &Rc<RefCell<Vec<glib::WeakRef<Image>>>>,
    thumbnail_size: &Rc<RefCell<i32>>,
    hash_cache: &Rc<RefCell<HashMap<String, String>>>,
) {
    let size = *thumbnail_size.borrow();
    let mut images = realized_thumb_images.borrow_mut();
    images.retain(|weak| weak.upgrade().is_some());
    for weak in images.iter() {
        if let Some(image) = weak.upgrade() {
            image.set_pixel_size(size);
            let bound_path = unsafe {
                image
                    .data::<String>("bound-path")
                    .map(|path| path.as_ref().clone())
            };
            if let Some(path_str) = bound_path {
                let generation_token = unsafe {
                    image
                        .data::<Rc<Cell<u64>>>("thumb-generation")
                        .map(|token| token.as_ref().clone())
                };
                if let Some(generation_token) = generation_token {
                    let expected_generation = generation_token.get();
                    load_grid_thumbnail(
                        &image,
                        path_str,
                        size,
                        hash_cache.clone(),
                        generation_token,
                        expected_generation,
                    );
                }
            }
        }
    }
}
