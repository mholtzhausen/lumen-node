use crate::dialogs::{open_delete_dialog, open_rename_dialog};
use crate::file_name_ops::clipboard_base_name_hint;
use crate::ui::preview::load_picture_async;
use crate::view_helpers::selected_image_path;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib, EventControllerKey, ListScrollFlags, StringObject};
use libadwaita as adw;
use std::{
    cell::Cell,
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::Duration,
};

pub(crate) struct KeyboardDeps {
    pub(crate) window: adw::ApplicationWindow,
    pub(crate) view_stack: adw::ViewStack,
    pub(crate) selection_model: gtk4::SingleSelection,
    pub(crate) single_picture: gtk4::Picture,
    pub(crate) grid_view: gtk4::GridView,
    pub(crate) grid_scroll: gtk4::ScrolledWindow,
    pub(crate) thumbnail_size: Rc<RefCell<i32>>,
    pub(crate) toast_overlay: adw::ToastOverlay,
    pub(crate) current_folder: Rc<RefCell<Option<PathBuf>>>,
    pub(crate) start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    pub(crate) left_toggle: gtk4::ToggleButton,
    pub(crate) right_toggle: gtk4::ToggleButton,
    pub(crate) pre_fullview_left: Rc<Cell<bool>>,
    pub(crate) pre_fullview_right: Rc<Cell<bool>>,
}

pub(crate) fn install_keyboard_handler(deps: KeyboardDeps) {
    let esc_pending: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let key_controller = EventControllerKey::new();
    key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let stack_for_keys = deps.view_stack.clone();
    let selection_for_keys = deps.selection_model.clone();
    let picture_for_keys = deps.single_picture.clone();
    let grid_view_for_keys = deps.grid_view.clone();
    let grid_scroll_for_keys = deps.grid_scroll.clone();
    let thumbnail_size_for_keys = deps.thumbnail_size.clone();
    let toast_overlay_for_keys = deps.toast_overlay.clone();
    let window_for_keys = deps.window.clone();
    let current_folder_for_keys = deps.current_folder.clone();
    let start_scan_for_folder_keys = deps.start_scan_for_folder.clone();
    let left_toggle_for_keys = deps.left_toggle.clone();
    let right_toggle_for_keys = deps.right_toggle.clone();
    let pre_fullview_left_keys = deps.pre_fullview_left.clone();
    let pre_fullview_right_keys = deps.pre_fullview_right.clone();
    key_controller.connect_key_pressed(move |_, key, _, state| {
        let ctrl_pressed = state.contains(gdk::ModifierType::CONTROL_MASK);
        if ctrl_pressed && key == gdk::Key::c {
            let Some(path) = selected_image_path(&selection_for_keys) else {
                return glib::Propagation::Stop;
            };
            let file = gio::File::for_path(&path);
            if let Ok(texture) = gdk::Texture::from_file(&file) {
                gtk4::prelude::WidgetExt::display(&window_for_keys)
                    .clipboard()
                    .set_texture(&texture);
                let toast = adw::Toast::new("Image copied to clipboard");
                toast.set_timeout(2);
                toast_overlay_for_keys.add_toast(toast);
            }
            return glib::Propagation::Stop;
        }
        if ctrl_pressed && key == gdk::Key::v {
            let Some(folder) = current_folder_for_keys.borrow().as_ref().cloned() else {
                let toast = adw::Toast::new("Open a folder before pasting");
                toast.set_timeout(2);
                toast_overlay_for_keys.add_toast(toast);
                return glib::Propagation::Stop;
            };
            let display = gtk4::prelude::WidgetExt::display(&window_for_keys);
            let clipboard = display.clipboard();
            let toast_overlay = toast_overlay_for_keys.clone();
            let window = window_for_keys.clone();
            let current_folder = current_folder_for_keys.clone();
            let start_scan_for_folder = start_scan_for_folder_keys.clone();
            glib::MainContext::default().spawn_local(async move {
                let Ok(Some(texture)) = clipboard.read_texture_future().await else {
                    let toast = adw::Toast::new("Clipboard does not contain an image");
                    toast.set_timeout(2);
                    toast_overlay.add_toast(toast);
                    return;
                };
                let suggested_name = clipboard
                    .read_text_future()
                    .await
                    .ok()
                    .flatten()
                    .as_ref()
                    .and_then(|text| clipboard_base_name_hint(text.as_str()));
                let uuid_base = glib::uuid_string_random().to_string();
                let target_path = folder.join(format!("{uuid_base}.png"));
                match texture.save_to_png(&target_path) {
                    Ok(()) => {
                        start_scan_for_folder(folder.clone());
                        open_rename_dialog(
                            &window,
                            &toast_overlay,
                            &start_scan_for_folder,
                            &current_folder,
                            target_path,
                            Some(suggested_name.unwrap_or(uuid_base)),
                        );
                    }
                    Err(err) => {
                        let toast = adw::Toast::new(&format!("Paste failed: {}", err));
                        toast.set_timeout(3);
                        toast_overlay.add_toast(toast);
                    }
                }
            });
            return glib::Propagation::Stop;
        }
        if key == gdk::Key::Delete {
            let Some(path) = selected_image_path(&selection_for_keys) else {
                return glib::Propagation::Stop;
            };
            open_delete_dialog(
                &window_for_keys,
                &toast_overlay_for_keys,
                &start_scan_for_folder_keys,
                &current_folder_for_keys,
                path,
            );
            return glib::Propagation::Stop;
        }
        if key == gdk::Key::Escape {
            let in_grid = stack_for_keys.visible_child_name().as_deref() == Some("grid");
            if in_grid {
                if esc_pending.get() {
                    window_for_keys.application().unwrap().quit();
                } else {
                    esc_pending.set(true);
                    let toast = adw::Toast::new("Press Escape again to quit");
                    toast.set_timeout(2);
                    toast_overlay_for_keys.add_toast(toast);
                    let esc_pending_clone = esc_pending.clone();
                    glib::timeout_add_local_once(Duration::from_millis(2000), move || {
                        esc_pending_clone.set(false);
                    });
                }
            } else {
                stack_for_keys.set_visible_child_name("grid");
                left_toggle_for_keys.set_active(pre_fullview_left_keys.get());
                right_toggle_for_keys.set_active(pre_fullview_right_keys.get());
            }
            return glib::Propagation::Stop;
        }
        let in_grid = stack_for_keys.visible_child_name().as_deref() == Some("grid");
        if in_grid
            && (key == gdk::Key::Page_Up
                || key == gdk::Key::Page_Down
                || key == gdk::Key::Home
                || key == gdk::Key::End)
        {
            let count = selection_for_keys.n_items();
            if count == 0 {
                return glib::Propagation::Proceed;
            }
            let has_selection = selection_for_keys.selected_item().is_some();
            let cur = selection_for_keys.selected();
            let thumb_size = (*thumbnail_size_for_keys.borrow()).max(1);
            let cell_width = (thumb_size + 4).max(1);
            let cell_height = (thumb_size + 20).max(1);
            let viewport_width = grid_scroll_for_keys.width().max(cell_width);
            let viewport_height = grid_scroll_for_keys.height().max(cell_height);
            let columns = (viewport_width / cell_width).max(1) as u32;
            let rows = (viewport_height / cell_height).max(1) as u32;
            let page_step = (columns * rows).max(1);

            let next = match key {
                gdk::Key::Home => 0,
                gdk::Key::End => count - 1,
                gdk::Key::Page_Up => {
                    if !has_selection {
                        0
                    } else {
                        cur.saturating_sub(page_step)
                    }
                }
                gdk::Key::Page_Down => {
                    if !has_selection {
                        0
                    } else {
                        cur.saturating_add(page_step).min(count - 1)
                    }
                }
                _ => cur,
            };

            if !has_selection || next != cur {
                selection_for_keys.set_selected(next);
                grid_view_for_keys.scroll_to(
                    next,
                    ListScrollFlags::FOCUS | ListScrollFlags::SELECT,
                    None,
                );
            }
            return glib::Propagation::Stop;
        }
        let in_single = stack_for_keys.visible_child_name().as_deref() == Some("single");
        if in_single && (key == gdk::Key::Left || key == gdk::Key::Right) {
            let count = selection_for_keys.n_items();
            if count == 0 {
                return glib::Propagation::Proceed;
            }
            let cur = selection_for_keys.selected();
            let next = if key == gdk::Key::Left {
                cur.saturating_sub(1)
            } else {
                (cur + 1).min(count - 1)
            };
            if next != cur {
                selection_for_keys.set_selected(next);
                if let Some(item) = selection_for_keys.selected_item().and_downcast::<StringObject>()
                {
                    load_picture_async(&picture_for_keys, &item.string().to_string(), None, None);
                }
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    deps.window.add_controller(key_controller);
}
