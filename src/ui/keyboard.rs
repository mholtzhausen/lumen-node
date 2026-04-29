use crate::dialogs::{open_delete_dialog, open_rename_dialog};
use crate::file_name_ops::clipboard_base_name_hint;
use crate::ui::center::CenterContentBundle;
use crate::ui::list_mutation::ListMutationContext;
use crate::ui::preview::load_picture_async;
use crate::view_helpers::selected_image_path;
use crate::core::app_state::AppState;
use crate::db;
use gtk4::prelude::*;
use gtk4::{
    gdk, gio, glib, EventControllerKey, EventControllerScroll, EventControllerScrollFlags,
    ListScrollFlags, StringObject, Widget,
};
use libadwaita as adw;
use std::{cell::Cell, cell::RefCell, path::PathBuf, rc::Rc, time::Duration};

pub(crate) struct KeyboardDeps {
    pub(crate) app_state: AppState,
    pub(crate) window: adw::ApplicationWindow,
    pub(crate) center: CenterContentBundle,
    pub(crate) selection_model: gtk4::SingleSelection,
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
    let cut_source_path: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
    let key_controller = EventControllerKey::new();
    key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let stack_for_keys = deps.center.view_stack.clone();
    let selection_for_keys = deps.selection_model.clone();
    let picture_for_keys = deps.center.single_picture.clone();
    let grid_view_for_keys = deps.center.grid_view.clone();
    let grid_scroll_for_keys = deps.center.grid_scroll.clone();
    let thumbnail_size_for_keys = deps.thumbnail_size.clone();
    let toast_overlay_for_keys = deps.toast_overlay.clone();
    let window_for_keys = deps.window.clone();
    let current_folder_for_keys = deps.current_folder.clone();
    let left_toggle_for_keys = deps.left_toggle.clone();
    let right_toggle_for_keys = deps.right_toggle.clone();
    let pre_fullview_left_keys = deps.pre_fullview_left.clone();
    let pre_fullview_right_keys = deps.pre_fullview_right.clone();
    let mutation_ctx_keys = ListMutationContext {
        app_state: deps.app_state.clone(),
        selection_model: deps.selection_model.clone(),
        start_scan_for_folder: deps.start_scan_for_folder.clone(),
    };
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
        if ctrl_pressed && key == gdk::Key::x {
            let Some(path) = selected_image_path(&selection_for_keys) else {
                return glib::Propagation::Stop;
            };
            *cut_source_path.borrow_mut() = Some(path.clone());
            gtk4::prelude::WidgetExt::display(&window_for_keys)
                .clipboard()
                .set_text(&path.to_string_lossy());
            let toast = adw::Toast::new("Cut image: press Ctrl+V to move into the open folder");
            toast.set_timeout(2);
            toast_overlay_for_keys.add_toast(toast);
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
            let cut_source_path = cut_source_path.clone();
            let mutation_ctx = mutation_ctx_keys.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Some(source) = cut_source_path.borrow_mut().take() {
                    if source.exists() {
                        let file_name = source
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "moved-image.png".to_string());
                        let destination = unique_destination_path(&folder, &file_name);
                        if std::fs::rename(&source, &destination).is_ok() {
                            if let Ok(conn) = db::open(&folder) {
                                if source.parent() == Some(folder.as_path()) {
                                    let _ = db::remove_image_row(&conn, &source);
                                }
                            }
                            if source.parent() == Some(folder.as_path()) {
                                let _ = mutation_ctx.replace_path(&source, &destination, true);
                            } else if !mutation_ctx.insert_path(&destination, true) {
                                mutation_ctx.fallback_rescan();
                            }
                            let toast = adw::Toast::new("Image moved");
                            toast.set_timeout(2);
                            toast_overlay.add_toast(toast);
                            return;
                        }
                    }
                }
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
                        if !mutation_ctx.insert_path(&target_path, true) {
                            mutation_ctx.fallback_rescan();
                        }
                        open_rename_dialog(
                            &window,
                            &toast_overlay,
                            &mutation_ctx,
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
            if state.contains(gdk::ModifierType::SHIFT_MASK) {
                let Some(path) = selected_image_path(&selection_for_keys) else {
                    return glib::Propagation::Proceed;
                };
                open_delete_dialog(
                    &window_for_keys,
                    &toast_overlay_for_keys,
                    &mutation_ctx_keys,
                    path,
                );
                return glib::Propagation::Stop;
            }
            if focus_in_text_input(&window_for_keys) {
                return glib::Propagation::Proceed;
            }
            if selected_image_path(&selection_for_keys).is_none() {
                return glib::Propagation::Proceed;
            }
            if gtk4::prelude::WidgetExt::activate_action(&window_for_keys, "ctx.move-to-trash", None)
                .is_ok()
            {
                return glib::Propagation::Stop;
            }
            return glib::Propagation::Proceed;
        }
        if key == gdk::Key::Escape {
            let in_grid = stack_for_keys.visible_child_name().as_deref() == Some("grid");
            if in_grid {
                if esc_pending.get() {
                    if let Some(app) = window_for_keys.application() {
                        app.quit();
                    }
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
                if window_for_keys.is_fullscreen() {
                    window_for_keys.unfullscreen();
                }
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
                if let Some(item) = selection_for_keys
                    .selected_item()
                    .and_downcast::<StringObject>()
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

pub(crate) fn install_scroll_navigation_handlers(
    selection_model: &gtk4::SingleSelection,
    single_picture: &gtk4::Picture,
    meta_preview: &gtk4::Picture,
) {
    {
        let selection = selection_model.clone();
        let picture = single_picture.clone();
        let accum: Rc<Cell<f64>> = Rc::new(Cell::new(0.0));
        let scroll = EventControllerScroll::new(EventControllerScrollFlags::VERTICAL);
        scroll.connect_scroll(move |_, _dx, dy| {
            let count = selection.n_items();
            if count == 0 {
                return glib::Propagation::Proceed;
            }
            accum.set(accum.get() + dy);
            let steps = accum.get().trunc() as i32;
            if steps == 0 {
                return glib::Propagation::Stop;
            }
            accum.set(accum.get().fract());
            let cur = selection.selected() as i32;
            let next = (cur + steps).clamp(0, count as i32 - 1) as u32;
            if next != cur as u32 {
                selection.set_selected(next);
                if let Some(item) = selection.selected_item().and_downcast::<StringObject>() {
                    load_picture_async(&picture, &item.string().to_string(), None, None);
                }
            }
            glib::Propagation::Stop
        });
        single_picture.add_controller(scroll);
    }
    {
        let selection = selection_model.clone();
        let accum: Rc<Cell<f64>> = Rc::new(Cell::new(0.0));
        let scroll = EventControllerScroll::new(EventControllerScrollFlags::VERTICAL);
        scroll.connect_scroll(move |_, _dx, dy| {
            let count = selection.n_items();
            if count == 0 {
                return glib::Propagation::Proceed;
            }
            accum.set(accum.get() + dy);
            let steps = accum.get().trunc() as i32;
            if steps == 0 {
                return glib::Propagation::Stop;
            }
            accum.set(accum.get().fract());
            let cur = selection.selected() as i32;
            let next = (cur + steps).clamp(0, count as i32 - 1) as u32;
            if next != cur as u32 {
                selection.set_selected(next);
            }
            glib::Propagation::Stop
        });
        meta_preview.add_controller(scroll);
    }
}

fn focus_in_text_input(window: &adw::ApplicationWindow) -> bool {
    let Some(focus) = gtk4::prelude::RootExt::focus(window.upcast_ref::<gtk4::Root>()) else {
        return false;
    };
    widget_is_or_inside_text_input(&focus)
}

fn widget_is_or_inside_text_input(widget: &Widget) -> bool {
    let mut current: Option<Widget> = Some(widget.clone());
    while let Some(w) = current {
        if w.is::<gtk4::SearchEntry>()
            || w.is::<gtk4::Entry>()
            || w.is::<gtk4::TextView>()
            || w.is::<gtk4::SpinButton>()
        {
            return true;
        }
        current = w.parent();
    }
    false
}

fn unique_destination_path(folder: &std::path::Path, file_name: &str) -> PathBuf {
    let candidate = folder.join(file_name);
    if !candidate.exists() {
        return candidate;
    }
    let stem = std::path::Path::new(file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "image".to_string());
    let ext = std::path::Path::new(file_name)
        .extension()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    for i in 1..10_000 {
        let name = if ext.is_empty() {
            format!("{stem}-{i}")
        } else {
            format!("{stem}-{i}.{ext}")
        };
        let p = folder.join(name);
        if !p.exists() {
            return p;
        }
    }
    folder.join(format!("{}-{}.png", stem, glib::uuid_string_random()))
}
