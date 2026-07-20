//! Per-thumbnail quick-tag popover: filterable checkbox list + “Add `foo`”.

use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, CheckButton, Label, MenuButton, Orientation, Popover, ScrolledWindow,
    SearchEntry,
};
use libadwaita as adw;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;

use crate::core::app_state::AppState;
use crate::db;
use crate::ui::controls::refresh_tag_filter_from_folder;

pub(crate) struct QuickTagAttachDeps {
    pub(crate) app_state: AppState,
    pub(crate) toast_overlay: adw::ToastOverlay,
    pub(crate) filter: gtk4::CustomFilter,
    pub(crate) tags_filter_btn: MenuButton,
    pub(crate) tags_filter_list: gtk4::Box,
    pub(crate) bound_paths: Rc<RefCell<HashMap<usize, String>>>,
}

/// Builds the popover UI, attaches it to `tags_btn`, and wires open/search/toggle handlers.
/// Returns the popover so callers can observe show/hide for chrome visibility.
pub(crate) fn attach_quick_tag_popover(
    tags_btn: &MenuButton,
    deps: QuickTagAttachDeps,
) -> Popover {
    let root = GtkBox::new(Orientation::Vertical, 6);
    root.set_margin_top(8);
    root.set_margin_bottom(8);
    root.set_margin_start(8);
    root.set_margin_end(8);
    root.set_size_request(220, -1);

    let search = SearchEntry::new();
    search.set_placeholder_text(Some("Filter tags…"));
    search.set_hexpand(true);
    root.append(&search);

    let tags_list = GtkBox::new(Orientation::Vertical, 2);
    let scroll = ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_max_content_height(240);
    scroll.set_propagate_natural_height(true);
    scroll.set_child(Some(&tags_list));
    root.append(&scroll);

    let popover = Popover::new();
    popover.set_child(Some(&root));
    tags_btn.set_popover(Some(&popover));

    let rebuild = {
        let tags_list = tags_list.clone();
        let search = search.clone();
        let tags_btn = tags_btn.clone();
        let app_state = deps.app_state.clone();
        let toast_overlay = deps.toast_overlay.clone();
        let filter = deps.filter.clone();
        let tags_filter_btn = deps.tags_filter_btn.clone();
        let tags_filter_list = deps.tags_filter_list.clone();
        let bound_paths = deps.bound_paths.clone();
        Rc::new(move || {
            let key = tags_btn.as_ptr() as usize;
            let Some(path_str) = bound_paths.borrow().get(&key).cloned() else {
                clear_box(&tags_list);
                let hint = Label::new(Some("No image bound."));
                hint.add_css_class("caption");
                hint.set_halign(gtk4::Align::Start);
                tags_list.append(&hint);
                return;
            };
            rebuild_quick_tag_list(
                &tags_list,
                search.text().as_str(),
                &path_str,
                &app_state,
                &toast_overlay,
                &filter,
                &tags_filter_btn,
                &tags_filter_list,
                &search,
            );
        })
    };

    let rebuild_on_show = rebuild.clone();
    popover.connect_show(move |_| {
        rebuild_on_show();
    });

    let rebuild_on_search = rebuild.clone();
    search.connect_search_changed(move |_| {
        rebuild_on_search();
    });

    popover
}

fn clear_box(box_: &GtkBox) {
    while let Some(child) = box_.first_child() {
        box_.remove(&child);
    }
}

fn image_tags_set(app_state: &AppState, path_str: &str) -> HashSet<String> {
    app_state
        .tags_cache
        .borrow()
        .get(path_str)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect()
}

fn sync_path_tags_cache(app_state: &AppState, conn: &rusqlite::Connection, path: &Path) {
    let path_key = path.to_string_lossy().to_string();
    let tags = db::list_tags_for_path(conn, path).unwrap_or_default();
    if tags.is_empty() {
        app_state.tags_cache.borrow_mut().remove(&path_key);
    } else {
        app_state.tags_cache.borrow_mut().insert(path_key, tags);
    }
}

fn notify_tags_changed(
    app_state: &AppState,
    filter: &gtk4::CustomFilter,
    tags_filter_btn: &MenuButton,
    tags_filter_list: &gtk4::Box,
) {
    filter.changed(gtk4::FilterChange::Different);
    refresh_tag_filter_from_folder(
        tags_filter_list,
        tags_filter_btn,
        &app_state.active_tags,
        filter,
        &app_state.current_folder,
    );
}

fn rebuild_quick_tag_list(
    tags_list: &GtkBox,
    search_text: &str,
    path_str: &str,
    app_state: &AppState,
    toast_overlay: &adw::ToastOverlay,
    filter: &gtk4::CustomFilter,
    tags_filter_btn: &MenuButton,
    tags_filter_list: &gtk4::Box,
    search_entry: &SearchEntry,
) {
    clear_box(tags_list);

    let Some(folder) = app_state.current_folder.borrow().as_ref().cloned() else {
        let hint = Label::new(Some("Open a folder to tag images."));
        hint.add_css_class("caption");
        hint.set_halign(gtk4::Align::Start);
        tags_list.append(&hint);
        return;
    };
    let Ok(conn) = db::open(&folder) else {
        let hint = Label::new(Some("Could not open folder database."));
        hint.add_css_class("caption");
        hint.set_halign(gtk4::Align::Start);
        tags_list.append(&hint);
        return;
    };

    let known = db::list_all_tags_in_folder(&conn).unwrap_or_default();
    // Prefer DB for the bound path so checks stay accurate if cache is stale.
    let path = Path::new(path_str);
    let on_image: HashSet<String> = db::list_tags_for_path(&conn, path)
        .ok()
        .map(|v| v.into_iter().collect())
        .unwrap_or_else(|| image_tags_set(app_state, path_str));

    let query = search_text.trim().to_lowercase();
    let filtered: Vec<String> = if query.is_empty() {
        known.clone()
    } else {
        known
            .iter()
            .filter(|t| t.to_lowercase().contains(&query))
            .cloned()
            .collect()
    };

    if known.is_empty() && query.is_empty() {
        let hint = Label::new(Some("No tags yet — type to add."));
        hint.add_css_class("caption");
        hint.set_halign(gtk4::Align::Start);
        tags_list.append(&hint);
    }

    for tag in &filtered {
        let check = CheckButton::with_label(tag);
        check.set_active(on_image.contains(tag));
        let tag_owned = tag.clone();
        let path_owned = path_str.to_string();
        let app_state_cb = app_state.clone();
        let toast_cb = toast_overlay.clone();
        let filter_cb = filter.clone();
        let tags_filter_btn_cb = tags_filter_btn.clone();
        let tags_filter_list_cb = tags_filter_list.clone();
        check.connect_toggled(move |btn| {
            toggle_tag_on_image(
                &app_state_cb,
                &toast_cb,
                &filter_cb,
                &tags_filter_btn_cb,
                &tags_filter_list_cb,
                &path_owned,
                &tag_owned,
                btn.is_active(),
            );
        });
        tags_list.append(&check);
    }

    let add_candidate = db::normalize_tag(search_text);
    let show_add = match add_candidate.as_ref() {
        Some(name) => !known.iter().any(|t| t.eq_ignore_ascii_case(name)),
        None => false,
    };
    if let (true, Some(name)) = (show_add, add_candidate) {
        let add_btn = Button::with_label(&format!("Add `{name}`"));
        add_btn.add_css_class("flat");
        add_btn.set_halign(gtk4::Align::Start);
        let path_owned = path_str.to_string();
        let app_state_cb = app_state.clone();
        let toast_cb = toast_overlay.clone();
        let filter_cb = filter.clone();
        let tags_filter_btn_cb = tags_filter_btn.clone();
        let tags_filter_list_cb = tags_filter_list.clone();
        let tags_list_cb = tags_list.clone();
        let search_cb = search_entry.clone();
        let tags_btn_for_rebuild = tags_filter_btn.clone();
        add_btn.connect_clicked(move |_| {
            if !toggle_tag_on_image(
                &app_state_cb,
                &toast_cb,
                &filter_cb,
                &tags_filter_btn_cb,
                &tags_filter_list_cb,
                &path_owned,
                &name,
                true,
            ) {
                return;
            }
            search_cb.set_text("");
            // Rebuild with empty search so the new tag appears checked in the full list.
            rebuild_quick_tag_list(
                &tags_list_cb,
                "",
                &path_owned,
                &app_state_cb,
                &toast_cb,
                &filter_cb,
                &tags_btn_for_rebuild,
                &tags_filter_list_cb,
                &search_cb,
            );
        });
        tags_list.append(&add_btn);
    }
}

/// Returns `true` when the DB mutation succeeded.
fn toggle_tag_on_image(
    app_state: &AppState,
    toast_overlay: &adw::ToastOverlay,
    filter: &gtk4::CustomFilter,
    tags_filter_btn: &MenuButton,
    tags_filter_list: &gtk4::Box,
    path_str: &str,
    tag: &str,
    want_on: bool,
) -> bool {
    let Some(folder) = app_state.current_folder.borrow().as_ref().cloned() else {
        return false;
    };
    let Ok(conn) = db::open(&folder) else {
        toast_overlay.add_toast(adw::Toast::new("Could not open folder database"));
        return false;
    };
    let path = Path::new(path_str);
    let result = if want_on {
        db::add_tag(&conn, path, tag)
    } else {
        // Treat missing tag as success so the checkbox can stay unchecked.
        db::remove_tag(&conn, path, tag).map(|_| true)
    };
    match result {
        Ok(true) => {
            sync_path_tags_cache(app_state, &conn, path);
            notify_tags_changed(app_state, filter, tags_filter_btn, tags_filter_list);
            true
        }
        Ok(false) if want_on => {
            toast_overlay.add_toast(adw::Toast::new(
                "Image must be indexed before tagging",
            ));
            false
        }
        Ok(false) => false,
        Err(_) => {
            toast_overlay.add_toast(adw::Toast::new(if want_on {
                "Could not add tag"
            } else {
                "Could not remove tag"
            }));
            false
        }
    }
}
