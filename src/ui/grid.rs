use gtk4::prelude::*;
use gtk4::{
    gdk, gio, glib, Box as GtkBox, Button, EventControllerMotion, GridView, Image, Label, ListItem,
    Orientation, ScrolledWindow, SignalListItemFactory, SingleSelection, StringObject,
};
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::time::{Duration, Instant};

use crate::{
    db,
    dialogs::{open_delete_dialog, open_rename_dialog},
    sort_flags::{sort_flag_text_for_path, SortFields},
    thumbnails,
};

pub fn create_grid_view(
    selection_model: &SingleSelection,
    factory: &gtk4::SignalListItemFactory,
) -> GridView {
    let grid_view = GridView::new(Some(selection_model.clone()), Some(factory.clone()));
    grid_view.set_max_columns(12);
    grid_view.set_min_columns(2);
    grid_view
}

pub struct GridFactoryDeps {
    pub thumbnail_size: Rc<RefCell<i32>>,
    pub realized_cell_boxes: Rc<RefCell<Vec<glib::WeakRef<GtkBox>>>>,
    pub realized_thumb_images: Rc<RefCell<Vec<glib::WeakRef<Image>>>>,
    pub fast_scroll_active: Rc<Cell<bool>>,
    pub hash_cache: Rc<RefCell<HashMap<String, String>>>,
    pub window: adw::ApplicationWindow,
    pub toast_overlay: adw::ToastOverlay,
    pub start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    pub current_folder: Rc<RefCell<Option<PathBuf>>>,
}

pub fn install_grid_factory(deps: GridFactoryDeps) -> SignalListItemFactory {
    let factory = SignalListItemFactory::new();

    let on_rename = make_rename_action(
        deps.window.clone(),
        deps.toast_overlay.clone(),
        deps.start_scan_for_folder.clone(),
        deps.current_folder.clone(),
    );
    let on_delete = make_delete_action(
        deps.window.clone(),
        deps.toast_overlay.clone(),
        deps.start_scan_for_folder.clone(),
        deps.current_folder.clone(),
    );

    let thumbnail_size_setup = deps.thumbnail_size.clone();
    let realized_thumb_images_setup = deps.realized_thumb_images.clone();
    let realized_cell_boxes_setup = deps.realized_cell_boxes.clone();
    factory.connect_setup(move |_, obj| {
        let list_item = obj.downcast_ref::<ListItem>().unwrap();
        setup_grid_list_item(
            list_item,
            &thumbnail_size_setup,
            &realized_cell_boxes_setup,
            &realized_thumb_images_setup,
            on_rename.clone(),
            on_delete.clone(),
        );
    });

    let hash_cache_bind = deps.hash_cache.clone();
    let thumbnail_size_bind = deps.thumbnail_size.clone();
    let fast_scroll_active_bind = deps.fast_scroll_active.clone();
    factory.connect_bind(move |_, obj| {
        let list_item = obj.downcast_ref::<ListItem>().unwrap();
        bind_grid_list_item(
            list_item,
            &thumbnail_size_bind,
            &fast_scroll_active_bind,
            hash_cache_bind.clone(),
        );
    });

    factory.connect_unbind(|_, obj| {
        let list_item = obj.downcast_ref::<ListItem>().unwrap();
        unbind_grid_list_item(list_item);
    });

    factory
}

pub fn create_grid_scroll(grid_view: &GridView) -> ScrolledWindow {
    let grid_scroll = ScrolledWindow::new();
    grid_scroll.set_vexpand(true);
    grid_scroll.set_hexpand(true);
    grid_scroll.set_child(Some(grid_view));
    grid_scroll
}

pub fn create_grid_overlay(grid_scroll: &ScrolledWindow) -> gtk4::Overlay {
    let grid_overlay = gtk4::Overlay::new();
    grid_overlay.set_hexpand(true);
    grid_overlay.set_vexpand(true);
    grid_overlay.set_child(Some(grid_scroll));
    grid_overlay
}

pub fn attach_grid_page(view_stack: &adw::ViewStack, grid_overlay: &gtk4::Overlay) {
    let grid_page = view_stack.add_titled(grid_overlay, Some("grid"), "Grid");
    grid_page.set_icon_name(Some("view-grid-symbolic"));
}

pub fn add_scroll_flag_overlay(grid_overlay: &gtk4::Overlay, scroll_flag_box: &GtkBox) {
    grid_overlay.add_overlay(scroll_flag_box);
}

pub fn create_single_picture() -> gtk4::Picture {
    let single_picture = gtk4::Picture::new();
    single_picture.set_vexpand(true);
    single_picture.set_hexpand(true);
    single_picture.set_can_shrink(true);
    single_picture
}

pub fn attach_single_page(view_stack: &adw::ViewStack, single_picture: &gtk4::Picture) {
    let single_page = view_stack.add_titled(single_picture, Some("single"), "Single");
    single_page.set_icon_name(Some("view-fullscreen-symbolic"));
}

pub fn set_default_grid_page(view_stack: &adw::ViewStack) {
    view_stack.set_visible_child_name("grid");
}

pub fn create_center_box(view_stack: &adw::ViewStack) -> GtkBox {
    let center_box = GtkBox::new(Orientation::Vertical, 0);
    center_box.set_hexpand(true);
    center_box.append(view_stack);
    center_box
}

pub fn enter_single_view_mode(
    view_stack: &adw::ViewStack,
    left_toggle: &gtk4::ToggleButton,
    right_toggle: &gtk4::ToggleButton,
    pre_fullview_left: &Rc<Cell<bool>>,
    pre_fullview_right: &Rc<Cell<bool>>,
) {
    pre_fullview_left.set(left_toggle.is_active());
    pre_fullview_right.set(right_toggle.is_active());
    view_stack.set_visible_child_name("single");
    left_toggle.set_active(false);
    right_toggle.set_active(false);
}

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

pub fn build_scroll_flag_overlay() -> (GtkBox, Label) {
    let scroll_flag_box = GtkBox::new(Orientation::Horizontal, 0);
    scroll_flag_box.set_visible(false);
    scroll_flag_box.set_halign(gtk4::Align::End);
    scroll_flag_box.set_valign(gtk4::Align::Start);
    scroll_flag_box.set_margin_end(12);
    scroll_flag_box.set_margin_top(12);
    scroll_flag_box.set_margin_start(12);
    scroll_flag_box.set_margin_bottom(12);

    let scroll_flag = Label::new(None);
    scroll_flag.add_css_class("title-4");
    scroll_flag.add_css_class("scroll-flag-bubble");
    scroll_flag.set_xalign(0.5);
    scroll_flag.set_margin_start(10);
    scroll_flag.set_margin_end(6);
    scroll_flag.set_margin_top(4);
    scroll_flag.set_margin_bottom(4);

    let scroll_flag_pointer = Label::new(Some("▶"));
    scroll_flag_pointer.add_css_class("title-4");
    scroll_flag_pointer.add_css_class("scroll-flag-pointer");
    scroll_flag_pointer.set_margin_start(0);
    scroll_flag_pointer.set_margin_end(0);
    scroll_flag_pointer.set_margin_top(0);
    scroll_flag_pointer.set_margin_bottom(0);

    scroll_flag_box.append(&scroll_flag);
    scroll_flag_box.append(&scroll_flag_pointer);
    (scroll_flag_box, scroll_flag)
}

pub fn install_grid_scroll_speed_gate(
    grid_scroll: &ScrolledWindow,
    grid_view: &GridView,
    fast_scroll_active: &Rc<Cell<bool>>,
    scroll_last_pos: &Rc<Cell<f64>>,
    scroll_last_time: &Rc<Cell<Option<Instant>>>,
    scroll_debounce_gen: &Rc<Cell<u64>>,
    thumbnail_size: &Rc<RefCell<i32>>,
    realized_thumb_images: &Rc<RefCell<Vec<glib::WeakRef<Image>>>>,
    hash_cache: &Rc<RefCell<HashMap<String, String>>>,
    selection_model: &SingleSelection,
    sort_key: &Rc<RefCell<String>>,
    sort_fields_cache: &Rc<RefCell<HashMap<String, SortFields>>>,
    scroll_flag_box: &GtkBox,
    scroll_flag: &Label,
) {
    let adj = grid_scroll.vadjustment();
    let fast_scroll_active_adj = fast_scroll_active.clone();
    let scroll_last_pos_adj = scroll_last_pos.clone();
    let scroll_last_time_adj = scroll_last_time.clone();
    let scroll_debounce_gen_adj = scroll_debounce_gen.clone();
    let thumbnail_size_adj = thumbnail_size.clone();
    let realized_adj = realized_thumb_images.clone();
    let hash_cache_adj = hash_cache.clone();
    let selection_model_adj = selection_model.clone();
    let sort_key_adj = sort_key.clone();
    let sort_fields_cache_adj = sort_fields_cache.clone();
    let scroll_flag_adj = scroll_flag.clone();
    let scroll_flag_box_adj = scroll_flag_box.clone();
    let grid_scroll_adj = grid_scroll.clone();
    let _grid_view = grid_view;

    adj.connect_value_changed(move |adj| {
        let now = Instant::now();
        let pos = adj.value();
        let cell_height = (*thumbnail_size_adj.borrow() + 24) as f64;
        let rows_per_sec = scroll_last_time_adj
            .get()
            .map(|last| {
                let dt = now.duration_since(last).as_secs_f64();
                if dt > 0.001 {
                    (pos - scroll_last_pos_adj.get()).abs() / cell_height / dt
                } else {
                    f64::INFINITY
                }
            })
            .unwrap_or(0.0);
        scroll_last_pos_adj.set(pos);
        scroll_last_time_adj.set(Some(now));
        fast_scroll_active_adj.set(rows_per_sec > 5.0);

        let gen = scroll_debounce_gen_adj.get().wrapping_add(1);
        scroll_debounce_gen_adj.set(gen);
        let fsa = fast_scroll_active_adj.clone();
        let realized = realized_adj.clone();
        let hash_cache = hash_cache_adj.clone();
        let thumbnail_size = thumbnail_size_adj.clone();
        let debounce_gen = scroll_debounce_gen_adj.clone();
        let scroll_flag = scroll_flag_adj.clone();
        let scroll_flag_box = scroll_flag_box_adj.clone();

        let total_items = selection_model_adj.n_items();
        if total_items > 0 {
            let thumb_size = (*thumbnail_size_adj.borrow()).max(1);
            let cell_width = (thumb_size + 4).max(1);
            let cell_height = (thumb_size + 20).max(1);
            let viewport_width = grid_scroll_adj.width().max(cell_width);
            let columns = (viewport_width / cell_width).max(1) as u32;
            let row = ((adj.value() / (cell_height as f64)).floor() as u32).saturating_mul(columns);
            let idx = row.min(total_items.saturating_sub(1));

            let text = selection_model_adj
                .item(idx)
                .and_downcast::<StringObject>()
                .and_then(|obj| {
                    let path = obj.string().to_string();
                    sort_flag_text_for_path(&path, &sort_key_adj.borrow(), &sort_fields_cache_adj.borrow())
                });

            if let Some(text) = text.filter(|t| !t.is_empty()) {
                scroll_flag.set_text(&text);
                let viewport_height = grid_scroll_adj.height().max(1) as f64;
                let upper = adj.upper().max(1.0);
                let page_size = adj.page_size().clamp(1.0, upper);
                let range = (upper - page_size).max(1.0);
                let ratio = (adj.value() / range).clamp(0.0, 1.0);
                let thumb_height = ((page_size / upper) * viewport_height).clamp(18.0, viewport_height);
                let thumb_top = ratio * (viewport_height - thumb_height);
                let thumb_center = thumb_top + (thumb_height * 0.5);
                let flag_height = 32.0;
                let y = (thumb_center - (flag_height * 0.5))
                    .clamp(0.0, (viewport_height - flag_height).max(0.0)) as i32;
                scroll_flag_box.set_margin_top(y);
                scroll_flag_box.set_visible(true);
            } else {
                scroll_flag_box.set_visible(false);
            }
        } else {
            scroll_flag_box.set_visible(false);
        }

        glib::timeout_add_local_once(Duration::from_millis(150), move || {
            if debounce_gen.get() != gen {
                return;
            }
            fsa.set(false);
            refresh_realized_grid_thumbnails(&realized, &thumbnail_size, &hash_cache);
        });
        let hide_gen = scroll_debounce_gen_adj.clone();
        glib::timeout_add_local_once(Duration::from_millis(450), move || {
            if hide_gen.get() != gen {
                return;
            }
            scroll_flag_box.set_visible(false);
        });
    });
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
