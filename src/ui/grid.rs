use gtk4::prelude::*;
use gtk4::{
    gdk, gio, glib, Box as GtkBox, Button, EventControllerMotion, Image, Label, ListItem,
    Orientation, StringObject,
};
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use crate::{
    db, dialogs::{open_delete_dialog, open_rename_dialog}, thumbnails,
};

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

pub fn setup_grid_list_item(
    list_item: &ListItem,
    thumbnail_size: &Rc<RefCell<i32>>,
    realized_cell_boxes: &Rc<RefCell<Vec<glib::WeakRef<GtkBox>>>>,
    realized_thumb_images: &Rc<RefCell<Vec<glib::WeakRef<Image>>>>,
    on_rename: Rc<dyn Fn(std::path::PathBuf)>,
    on_delete: Rc<dyn Fn(std::path::PathBuf)>,
) {
    let cell_box = GtkBox::new(Orientation::Vertical, 4);
    cell_box.add_css_class("thumbnail-card");
    cell_box.set_halign(gtk4::Align::Center);
    cell_box.set_margin_top(4);
    cell_box.set_margin_bottom(4);
    cell_box.set_margin_start(4);
    cell_box.set_margin_end(4);
    let size = *thumbnail_size.borrow();
    cell_box.set_size_request(size + 12, size + 28);
    let thumb_image = Image::new();
    thumb_image.set_pixel_size(size);
    let generation_token = Rc::new(Cell::new(0_u64));
    unsafe {
        thumb_image.set_data("thumb-generation", generation_token);
    }
    track_realized_grid_widgets(realized_cell_boxes, realized_thumb_images, &cell_box, &thumb_image);
    let name_label = Label::new(None);
    name_label.set_max_width_chars(16);
    name_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    name_label.add_css_class("caption");
    name_label.set_hexpand(true);
    name_label.set_halign(gtk4::Align::Start);
    let rename_btn = Button::from_icon_name("document-edit-symbolic");
    rename_btn.add_css_class("flat");
    rename_btn.set_tooltip_text(Some("Rename file"));
    rename_btn.set_opacity(0.0);
    rename_btn.set_focus_on_click(false);
    let delete_btn = Button::from_icon_name("user-trash-symbolic");
    delete_btn.add_css_class("flat");
    delete_btn.add_css_class("destructive-action");
    delete_btn.set_tooltip_text(Some("Delete file"));
    delete_btn.set_opacity(0.0);
    delete_btn.set_focus_on_click(false);
    let on_rename_btn = on_rename.clone();
    rename_btn.connect_clicked(move |btn| {
        let path = unsafe { btn.data::<String>("bound-path").map(|s| s.as_ref().clone()) };
        let Some(path) = path else { return };
        on_rename_btn(std::path::PathBuf::from(path));
    });
    let on_delete_btn = on_delete.clone();
    delete_btn.connect_clicked(move |btn| {
        let path = unsafe { btn.data::<String>("bound-path").map(|s| s.as_ref().clone()) };
        let Some(path) = path else { return };
        on_delete_btn(std::path::PathBuf::from(path));
    });
    let name_row = GtkBox::new(Orientation::Horizontal, 4);
    name_row.set_hexpand(true);
    name_row.set_halign(gtk4::Align::Fill);
    let action_box = GtkBox::new(Orientation::Horizontal, 2);
    action_box.append(&rename_btn);
    action_box.append(&delete_btn);
    name_row.append(&name_label);
    name_row.append(&action_box);
    let rename_btn_enter = rename_btn.clone();
    let rename_btn_leave = rename_btn.clone();
    let delete_btn_enter = delete_btn.clone();
    let delete_btn_leave = delete_btn.clone();
    let motion = EventControllerMotion::new();
    motion.connect_enter(move |_, _, _| {
        rename_btn_enter.set_opacity(1.0);
        delete_btn_enter.set_opacity(1.0);
    });
    motion.connect_leave(move |_| {
        rename_btn_leave.set_opacity(0.0);
        delete_btn_leave.set_opacity(0.0);
    });
    cell_box.add_controller(motion);
    cell_box.append(&thumb_image);
    cell_box.append(&name_row);
    list_item.set_child(Some(&cell_box));
}

pub fn bind_grid_list_item(
    list_item: &ListItem,
    thumbnail_size: &Rc<RefCell<i32>>,
    fast_scroll_active: &Rc<Cell<bool>>,
    hash_cache: Rc<RefCell<HashMap<String, String>>>,
) {
    let path_str = list_item
        .item()
        .and_downcast::<StringObject>()
        .map(|s| s.string().to_string())
        .unwrap_or_default();

    let cell_box = list_item.child().and_downcast::<GtkBox>().unwrap();
    let thumb_image = cell_box.first_child().and_downcast::<Image>().unwrap();
    let name_row = cell_box.last_child().and_downcast::<GtkBox>().unwrap();
    let name_label = name_row.first_child().and_downcast::<Label>().unwrap();
    let action_box = name_row.last_child().and_downcast::<GtkBox>().unwrap();
    let rename_btn = action_box.first_child().and_downcast::<Button>().unwrap();
    let delete_btn = action_box.last_child().and_downcast::<Button>().unwrap();
    let size = *thumbnail_size.borrow();
    cell_box.set_size_request(size + 12, size + 28);
    thumb_image.set_pixel_size(size);

    let filename = Path::new(&path_str)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name_label.set_text(&filename);
    unsafe {
        rename_btn.set_data("bound-path", path_str.clone());
    }
    unsafe {
        delete_btn.set_data("bound-path", path_str.clone());
    }
    let generation_token = unsafe {
        thumb_image
            .data::<Rc<Cell<u64>>>("thumb-generation")
            .map(|token| token.as_ref().clone())
    };
    if let Some(generation_token) = generation_token {
        let expected_generation = generation_token.get().saturating_add(1);
        generation_token.set(expected_generation);
        if fast_scroll_active.get() {
            thumb_image.set_icon_name(Some("image-x-generic-symbolic"));
            unsafe {
                thumb_image.set_data("bound-path", path_str);
            }
        } else {
            load_grid_thumbnail(
                &thumb_image,
                path_str,
                size,
                hash_cache,
                generation_token,
                expected_generation,
            );
        }
    }
}

pub fn unbind_grid_list_item(list_item: &ListItem) {
    if let Some(cell_box) = list_item.child().and_downcast::<GtkBox>() {
        if let Some(image) = cell_box.first_child().and_downcast::<Image>() {
            let generation_token = unsafe {
                image
                    .data::<Rc<Cell<u64>>>("thumb-generation")
                    .map(|token| token.as_ref().clone())
            };
            if let Some(generation_token) = generation_token {
                generation_token.set(generation_token.get().saturating_add(1));
            }
            unsafe {
                image.steal_data::<String>("bound-path");
            }
            if let Some(name_row) = cell_box.last_child().and_downcast::<GtkBox>() {
                if let Some(action_box) = name_row.last_child().and_downcast::<GtkBox>() {
                    if let Some(rename_btn) = action_box.first_child().and_downcast::<Button>() {
                        unsafe {
                            rename_btn.steal_data::<String>("bound-path");
                        }
                    }
                    if let Some(delete_btn) = action_box.last_child().and_downcast::<Button>() {
                        unsafe {
                            delete_btn.steal_data::<String>("bound-path");
                        }
                    }
                }
            }
            image.set_icon_name(Some("image-x-generic-symbolic"));
        }
    }
}

pub fn apply_thumbnail_size_change(
    selected_size: i32,
    realized_cell_boxes: &Rc<RefCell<Vec<glib::WeakRef<GtkBox>>>>,
    realized_thumb_images: &Rc<RefCell<Vec<glib::WeakRef<Image>>>>,
    thumbnail_size: &Rc<RefCell<i32>>,
    hash_cache: &Rc<RefCell<HashMap<String, String>>>,
    grid_view: &gtk4::GridView,
) {
    refresh_realized_grid_cell_sizes(realized_cell_boxes, selected_size);
    refresh_realized_grid_thumbnails(realized_thumb_images, thumbnail_size, hash_cache);
    grid_view.queue_resize();
    grid_view.queue_draw();
}

pub fn make_rename_action(
    window: adw::ApplicationWindow,
    toast_overlay: adw::ToastOverlay,
    start_scan_for_folder: Rc<dyn Fn(std::path::PathBuf)>,
    current_folder: Rc<RefCell<Option<std::path::PathBuf>>>,
) -> Rc<dyn Fn(std::path::PathBuf)> {
    Rc::new(move |path| {
        open_rename_dialog(
            &window,
            &toast_overlay,
            &start_scan_for_folder,
            &current_folder,
            path,
            None,
        );
    })
}

pub fn make_delete_action(
    window: adw::ApplicationWindow,
    toast_overlay: adw::ToastOverlay,
    start_scan_for_folder: Rc<dyn Fn(std::path::PathBuf)>,
    current_folder: Rc<RefCell<Option<std::path::PathBuf>>>,
) -> Rc<dyn Fn(std::path::PathBuf)> {
    Rc::new(move |path| {
        open_delete_dialog(
            &window,
            &toast_overlay,
            &start_scan_for_folder,
            &current_folder,
            path,
        );
    })
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
