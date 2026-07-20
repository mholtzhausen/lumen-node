//! Right-pane batch editor shown when two or more grid items are selected.

use crate::byte_format::human_readable_bytes;
use crate::core::app_state::AppState;
use crate::db;
use crate::file_name_ops::{
    batch_rename_target, default_index_pad_width, expand_batch_rename_stem,
    find_batch_rename_collisions, parse_batch_index_placeholder,
};
use crate::thumbnails;
use crate::ui::grid::refresh_realized_grid_favourite_icons;
use crate::ui::list_mutation::ListMutationContext;
use crate::view_helpers::{
    filename_of, order_batch_paths, selected_count, selected_image_path_strings, BatchListSortKey,
};
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, CheckButton, DropDown, Entry, Image, Label, ListBox, ListBoxRow,
    Orientation, PolicyType, ScrolledWindow, Separator,
};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TriState {
    None,
    Mixed,
    All,
}

#[derive(Clone)]
pub(crate) struct BatchEditorBundle {
    pub(crate) root: GtkBox,
    pub(crate) refresh: Rc<dyn Fn()>,
}

pub(crate) struct BatchEditorDeps {
    pub(crate) app_state: AppState,
    pub(crate) selection_model: gtk4::MultiSelection,
    pub(crate) toast_overlay: adw::ToastOverlay,
    pub(crate) window: adw::ApplicationWindow,
    pub(crate) filter: gtk4::CustomFilter,
    pub(crate) mutation_ctx: ListMutationContext,
}

pub(crate) fn build_batch_editor(deps: BatchEditorDeps) -> BatchEditorBundle {
    let root = GtkBox::new(Orientation::Vertical, 0);
    root.set_hexpand(true);
    root.set_vexpand(true);
    root.add_css_class("batch-editor");

    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Never)
        .vscrollbar_policy(PolicyType::Automatic)
        .vexpand(true)
        .hexpand(true)
        .build();

    let content = GtkBox::new(Orientation::Vertical, 12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_hexpand(true);

    let summary = Label::new(Some("0 selected"));
    summary.add_css_class("title-3");
    summary.set_halign(Align::Start);
    content.append(&summary);

    let actions = GtkBox::new(Orientation::Horizontal, 6);
    actions.set_halign(Align::Start);
    let copy_paths_btn = Button::with_label("Copy paths");
    let copy_names_btn = Button::with_label("Copy filenames");

    let fav_btn = Button::new();
    fav_btn.set_tooltip_text(Some("Toggle favourite for all selected"));
    let fav_row = GtkBox::new(Orientation::Horizontal, 6);
    let fav_icon = Image::from_icon_name("non-starred-symbolic");
    let fav_label = Label::new(Some("Favourite all"));
    fav_row.append(&fav_icon);
    fav_row.append(&fav_label);
    fav_btn.set_child(Some(&fav_row));

    actions.append(&copy_paths_btn);
    actions.append(&copy_names_btn);
    actions.append(&fav_btn);
    content.append(&actions);

    content.append(&Separator::new(Orientation::Horizontal));

    let rename_header = Label::new(Some("Batch rename"));
    rename_header.add_css_class("heading");
    rename_header.set_halign(Align::Start);
    content.append(&rename_header);

    let pattern_entry = Entry::new();
    pattern_entry.set_placeholder_text(Some("name_{index} or name_{index:3}"));
    pattern_entry.set_text("image_{index}");
    content.append(&pattern_entry);

    let rename_apply = Button::with_label("Apply rename");
    rename_apply.add_css_class("suggested-action");
    content.append(&rename_apply);

    content.append(&Separator::new(Orientation::Horizontal));

    let list_header_row = GtkBox::new(Orientation::Horizontal, 8);
    let list_label = Label::new(Some("Selected images"));
    list_label.add_css_class("heading");
    list_label.set_halign(Align::Start);
    list_label.set_hexpand(true);
    let sort_dropdown = DropDown::from_strings(&[
        "Name ↑",
        "Name ↓",
        "Date ↑",
        "Date ↓",
        "Size ↑",
        "Size ↓",
    ]);
    sort_dropdown.set_selected(sort_key_to_index(BatchListSortKey::from_str(
        deps.app_state.batch_list_sort_key.borrow().as_str(),
    )));
    list_header_row.append(&list_label);
    list_header_row.append(&sort_dropdown);
    content.append(&list_header_row);

    let selection_list = ListBox::new();
    selection_list.add_css_class("boxed-list");
    selection_list.set_selection_mode(gtk4::SelectionMode::Browse);
    content.append(&selection_list);

    content.append(&Separator::new(Orientation::Horizontal));

    let tags_header = Label::new(Some("Tags"));
    tags_header.add_css_class("heading");
    tags_header.set_halign(Align::Start);
    content.append(&tags_header);

    let tags_box = GtkBox::new(Orientation::Vertical, 4);
    content.append(&tags_box);

    scroll.set_child(Some(&content));
    root.append(&scroll);

    let ordered_paths: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    let fav_state: Rc<Cell<TriState>> = Rc::new(Cell::new(TriState::None));
    let suppress_tag_toggle: Rc<Cell<bool>> = Rc::new(Cell::new(false));
    let refresh_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));

    let refresh: Rc<dyn Fn()> = {
        let app_state = deps.app_state.clone();
        let selection_model = deps.selection_model.clone();
        let summary = summary.clone();
        let selection_list = selection_list.clone();
        let tags_box = tags_box.clone();
        let fav_icon = fav_icon.clone();
        let fav_label = fav_label.clone();
        let fav_state = fav_state.clone();
        let ordered_paths = ordered_paths.clone();
        let pattern_entry = pattern_entry.clone();
        let rename_apply = rename_apply.clone();
        let sort_dropdown = sort_dropdown.clone();
        let window = deps.window.clone();
        let suppress_tag_toggle = suppress_tag_toggle.clone();
        let refresh_slot = refresh_slot.clone();

        Rc::new(move || {
            let count = selected_count(&selection_model);
            summary.set_text(&format!(
                "{} selected",
                if count == 1 {
                    "1 image".to_string()
                } else {
                    format!("{count} images")
                }
            ));
            if count < 2 {
                return;
            }

            let key = index_to_sort_key(sort_dropdown.selected());
            *app_state.batch_list_sort_key.borrow_mut() = key.as_str().to_string();

            let paths = selected_image_path_strings(&selection_model);
            let ordered = order_batch_paths(&paths, &app_state.sort_fields_cache.borrow(), key);
            *ordered_paths.borrow_mut() = ordered.clone();

            while let Some(child) = selection_list.first_child() {
                selection_list.remove(&child);
            }

            let pad = default_index_pad_width(ordered.len());
            let pattern = pattern_entry.text().to_string();
            let pattern_ok = parse_batch_index_placeholder(&pattern).is_ok();

            let mut sources = Vec::new();
            let mut targets = Vec::new();

            for (i, path) in ordered.iter().enumerate() {
                let idx = i + 1;
                let preview_name = match expand_batch_rename_stem(&pattern, idx, pad) {
                    Ok(stem) => {
                        let src = PathBuf::from(path);
                        let ext = src
                            .extension()
                            .map(|e| format!(".{}", e.to_string_lossy()))
                            .unwrap_or_default();
                        let full = format!("{stem}{ext}");
                        if let Ok(target) = batch_rename_target(&src, &pattern, idx, pad) {
                            sources.push(src);
                            targets.push(target);
                        }
                        full
                    }
                    Err(err) => err,
                };

                let row = ListBoxRow::new();
                row.set_activatable(true);
                row.set_focusable(true);
                let row_box = GtkBox::new(Orientation::Horizontal, 8);
                row_box.set_margin_top(4);
                row_box.set_margin_bottom(4);
                row_box.set_margin_start(4);
                row_box.set_margin_end(4);

                let thumb = Image::from_icon_name("image-x-generic-symbolic");
                thumb.set_pixel_size(40);
                load_batch_thumb(&thumb, &app_state, path);

                let detail = GtkBox::new(Orientation::Vertical, 2);
                detail.set_hexpand(true);
                let name = Label::new(Some(&filename_of(path)));
                name.set_halign(Align::Start);
                name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                let size = app_state
                    .sort_fields_cache
                    .borrow()
                    .get(path)
                    .map(|f| f.size)
                    .unwrap_or(0);
                let meta = Label::new(Some(&format!(
                    "{} → {}",
                    human_readable_bytes(size),
                    preview_name
                )));
                meta.add_css_class("dim-label");
                meta.add_css_class("caption");
                meta.set_halign(Align::Start);
                meta.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                detail.append(&name);
                detail.append(&meta);
                row_box.append(&thumb);
                row_box.append(&detail);
                row.set_child(Some(&row_box));
                selection_list.append(&row);
            }

            let collisions = if pattern_ok {
                find_batch_rename_collisions(&sources, &targets)
            } else {
                vec!["Invalid pattern".to_string()]
            };
            let can_apply = collisions.is_empty() && !ordered.is_empty() && pattern_ok;
            rename_apply.set_sensitive(can_apply);
            if collisions.is_empty() {
                rename_apply.set_tooltip_text(Some("Apply batch rename"));
            } else {
                rename_apply.set_tooltip_text(Some(&collisions.join("\n")));
            }

            let favs: Vec<bool> = ordered
                .iter()
                .map(|p| {
                    app_state
                        .favourite_cache
                        .borrow()
                        .get(p)
                        .copied()
                        .unwrap_or(false)
                })
                .collect();
            let state = tri_from_bools(&favs);
            fav_state.set(state);
            match state {
                TriState::All => {
                    fav_label.set_text("Unfavourite all");
                    fav_icon.set_icon_name(Some("starred-symbolic"));
                }
                TriState::Mixed => {
                    fav_label.set_text("Favourite all…");
                    fav_icon.set_icon_name(Some("semi-starred-symbolic"));
                }
                TriState::None => {
                    fav_label.set_text("Favourite all");
                    fav_icon.set_icon_name(Some("non-starred-symbolic"));
                }
            }

            while let Some(child) = tags_box.first_child() {
                tags_box.remove(&child);
            }
            suppress_tag_toggle.set(true);
            let folder_tags = app_state
                .current_folder
                .borrow()
                .as_ref()
                .and_then(|f| db::open(f).ok())
                .and_then(|conn| db::list_all_tags_in_folder(&conn).ok())
                .unwrap_or_default();
            for tag in folder_tags {
                let counts = ordered
                    .iter()
                    .filter(|p| {
                        app_state
                            .tags_cache
                            .borrow()
                            .get(*p)
                            .map(|t| t.iter().any(|x| x == &tag))
                            .unwrap_or(false)
                    })
                    .count();
                let tag_state = if counts == 0 {
                    TriState::None
                } else if counts == ordered.len() {
                    TriState::All
                } else {
                    TriState::Mixed
                };

                let check = CheckButton::with_label(&tag);
                apply_tri_check(&check, tag_state);

                let tag_name = tag.clone();
                let app_state_t = app_state.clone();
                let ordered_t = ordered_paths.clone();
                let window_t = window.clone();
                let suppress = suppress_tag_toggle.clone();
                let refresh_slot_t = refresh_slot.clone();

                check.connect_toggled(move |btn| {
                    if suppress.get() {
                        return;
                    }
                    let paths = ordered_t.borrow().clone();
                    if paths.is_empty() {
                        return;
                    }

                    let prev_counts = paths
                        .iter()
                        .filter(|p| {
                            app_state_t
                                .tags_cache
                                .borrow()
                                .get(*p)
                                .map(|t| t.iter().any(|x| x == &tag_name))
                                .unwrap_or(false)
                        })
                        .count();
                    let was_mixed = prev_counts > 0 && prev_counts < paths.len();

                    if was_mixed && btn.is_active() {
                        suppress.set(true);
                        btn.set_active(false);
                        btn.set_inconsistent(true);
                        suppress.set(false);

                        let dialog = adw::AlertDialog::new(
                            Some("Apply tag to all?"),
                            Some(&format!(
                                "Add “{tag_name}” to all {} selected images?",
                                paths.len()
                            )),
                        );
                        dialog.add_response("cancel", "Cancel");
                        dialog.add_response("apply", "Apply");
                        dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);
                        let app_state_d = app_state_t.clone();
                        let paths_d = paths.clone();
                        let tag_d = tag_name.clone();
                        let refresh_slot_d = refresh_slot_t.clone();
                        dialog.connect_response(None, move |_, response| {
                            if response != "apply" {
                                return;
                            }
                            apply_tag_to_paths(&app_state_d, &paths_d, &tag_d, true);
                            if let Some(refresh) = refresh_slot_d.borrow().as_ref() {
                                refresh();
                            }
                        });
                        dialog.present(Some(&window_t));
                        return;
                    }

                    let want_on = btn.is_active();
                    apply_tag_to_paths(&app_state_t, &paths, &tag_name, want_on);
                    if let Some(refresh) = refresh_slot_t.borrow().as_ref() {
                        refresh();
                    }
                });
                tags_box.append(&check);
            }
            suppress_tag_toggle.set(false);
        })
    };

    *refresh_slot.borrow_mut() = Some(refresh.clone());

    {
        let refresh_c = refresh.clone();
        sort_dropdown.connect_selected_notify(move |_| refresh_c());
    }
    {
        let refresh_c = refresh.clone();
        pattern_entry.connect_changed(move |_| refresh_c());
    }

    {
        let window = deps.window.clone();
        let ordered_paths = ordered_paths.clone();
        copy_paths_btn.connect_clicked(move |_| {
            let text = ordered_paths.borrow().join("\n");
            gtk4::prelude::WidgetExt::display(&window)
                .clipboard()
                .set_text(&text);
        });
    }
    {
        let window = deps.window.clone();
        let ordered_paths = ordered_paths.clone();
        copy_names_btn.connect_clicked(move |_| {
            let text = ordered_paths
                .borrow()
                .iter()
                .map(|p| filename_of(p))
                .collect::<Vec<_>>()
                .join("\n");
            gtk4::prelude::WidgetExt::display(&window)
                .clipboard()
                .set_text(&text);
        });
    }

    {
        let app_state = deps.app_state.clone();
        let ordered_paths = ordered_paths.clone();
        let fav_state = fav_state.clone();
        let filter = deps.filter.clone();
        let refresh_c = refresh.clone();
        let window = deps.window.clone();
        fav_btn.connect_clicked(move |_| {
            let paths = ordered_paths.borrow().clone();
            if paths.is_empty() {
                return;
            }
            match fav_state.get() {
                TriState::Mixed => {
                    let dialog = adw::AlertDialog::new(
                        Some("Favourite all?"),
                        Some(&format!(
                            "Mark all {} selected images as favourites?",
                            paths.len()
                        )),
                    );
                    dialog.add_response("cancel", "Cancel");
                    dialog.add_response("apply", "Apply");
                    dialog.set_response_appearance("apply", adw::ResponseAppearance::Suggested);
                    let app_state_d = app_state.clone();
                    let paths_d = paths.clone();
                    let filter_d = filter.clone();
                    let refresh_d = refresh_c.clone();
                    dialog.connect_response(None, move |_, response| {
                        if response != "apply" {
                            return;
                        }
                        apply_favourite_to_paths(&app_state_d, &paths_d, true);
                        filter_d.changed(gtk4::FilterChange::Different);
                        refresh_realized_grid_favourite_icons(&app_state_d);
                        refresh_d();
                    });
                    dialog.present(Some(&window));
                }
                TriState::All => {
                    apply_favourite_to_paths(&app_state, &paths, false);
                    filter.changed(gtk4::FilterChange::Different);
                    refresh_realized_grid_favourite_icons(&app_state);
                    refresh_c();
                }
                TriState::None => {
                    apply_favourite_to_paths(&app_state, &paths, true);
                    filter.changed(gtk4::FilterChange::Different);
                    refresh_realized_grid_favourite_icons(&app_state);
                    refresh_c();
                }
            }
        });
    }

    {
        let app_state = deps.app_state.clone();
        let ordered_paths = ordered_paths.clone();
        let pattern_entry = pattern_entry.clone();
        let mutation_ctx = deps.mutation_ctx.clone();
        let toast_overlay = deps.toast_overlay.clone();
        let refresh_c = refresh.clone();
        rename_apply.connect_clicked(move |_| {
            let paths = ordered_paths.borrow().clone();
            let pattern = pattern_entry.text().to_string();
            let pad = default_index_pad_width(paths.len());
            if parse_batch_index_placeholder(&pattern).is_err() {
                return;
            }
            let mut sources = Vec::new();
            let mut targets = Vec::new();
            for (i, path) in paths.iter().enumerate() {
                let src = PathBuf::from(path);
                match batch_rename_target(&src, &pattern, i + 1, pad) {
                    Ok(t) => {
                        sources.push(src);
                        targets.push(t);
                    }
                    Err(_) => return,
                }
            }
            if !find_batch_rename_collisions(&sources, &targets).is_empty() {
                return;
            }
            let folder = app_state.current_folder.borrow().clone();
            let mut ok = 0usize;
            let mut err_msg = None;
            for (src, dst) in sources.iter().zip(targets.iter()) {
                if src == dst {
                    ok += 1;
                    continue;
                }
                match std::fs::rename(src, dst) {
                    Ok(()) => {
                        if let Some(ref folder) = folder {
                            if let Ok(conn) = db::open(folder) {
                                if let Some(row) = db::move_image_row(&conn, src, dst) {
                                    let old_key = src.to_string_lossy().to_string();
                                    let new_key = dst.to_string_lossy().to_string();
                                    app_state
                                        .favourite_cache
                                        .borrow_mut()
                                        .insert(new_key.clone(), row.favourite != 0);
                                    app_state.favourite_cache.borrow_mut().remove(&old_key);
                                    if let Ok(tags) = db::list_tags_for_path(&conn, dst) {
                                        app_state
                                            .tags_cache
                                            .borrow_mut()
                                            .insert(new_key.clone(), tags);
                                    }
                                    app_state.tags_cache.borrow_mut().remove(&old_key);
                                    if !row.hash.is_empty() {
                                        app_state
                                            .hash_cache
                                            .borrow_mut()
                                            .insert(new_key, row.hash);
                                        app_state.hash_cache.borrow_mut().remove(&old_key);
                                    }
                                }
                            }
                        }
                        let _ = mutation_ctx.replace_path(src, dst, false);
                        ok += 1;
                    }
                    Err(e) => {
                        err_msg = Some(e.to_string());
                        break;
                    }
                }
            }
            let toast = if let Some(err) = err_msg {
                adw::Toast::new(&format!("Renamed {ok}; stopped: {err}"))
            } else {
                adw::Toast::new(&format!("Renamed {ok} images"))
            };
            toast.set_timeout(3);
            toast_overlay.add_toast(toast);
            refresh_c();
        });
    }

    {
        let refresh_c = refresh.clone();
        deps.selection_model
            .connect_selection_changed(move |_, _, _| {
                refresh_c();
            });
    }

    refresh();

    BatchEditorBundle { root, refresh }
}

fn sort_key_to_index(key: BatchListSortKey) -> u32 {
    match key {
        BatchListSortKey::NameAsc => 0,
        BatchListSortKey::NameDesc => 1,
        BatchListSortKey::DateAsc => 2,
        BatchListSortKey::DateDesc => 3,
        BatchListSortKey::SizeAsc => 4,
        BatchListSortKey::SizeDesc => 5,
    }
}

fn index_to_sort_key(index: u32) -> BatchListSortKey {
    match index {
        1 => BatchListSortKey::NameDesc,
        2 => BatchListSortKey::DateAsc,
        3 => BatchListSortKey::DateDesc,
        4 => BatchListSortKey::SizeAsc,
        5 => BatchListSortKey::SizeDesc,
        _ => BatchListSortKey::NameAsc,
    }
}

fn load_batch_thumb(thumb: &Image, app_state: &AppState, path: &str) {
    if let Some(hash) = app_state.hash_cache.borrow().get(path).cloned() {
        if let Some(pb) = thumbnails::hash_thumb_if_exists_for_size(&hash, 64)
            .or_else(|| {
                thumbnails::hash_thumb_if_exists_for_size(&hash, thumbnails::THUMB_NORMAL_SIZE)
            })
        {
            thumb.set_from_file(Some(&pb));
            return;
        }
    }
    let uri_thumb = thumbnails::thumb_path(Path::new(path));
    if uri_thumb.is_file() {
        thumb.set_from_file(Some(&uri_thumb));
    }
}

fn tri_from_bools(vals: &[bool]) -> TriState {
    if vals.is_empty() {
        return TriState::None;
    }
    let all = vals.iter().all(|v| *v);
    let none = vals.iter().all(|v| !*v);
    if all {
        TriState::All
    } else if none {
        TriState::None
    } else {
        TriState::Mixed
    }
}

fn apply_tri_check(check: &CheckButton, state: TriState) {
    match state {
        TriState::All => {
            check.set_inconsistent(false);
            check.set_active(true);
        }
        TriState::None => {
            check.set_inconsistent(false);
            check.set_active(false);
        }
        TriState::Mixed => {
            check.set_active(false);
            check.set_inconsistent(true);
        }
    }
}

fn apply_tag_to_paths(app_state: &AppState, paths: &[String], tag: &str, want_on: bool) {
    let Some(folder) = app_state.current_folder.borrow().clone() else {
        return;
    };
    let Ok(conn) = db::open(&folder) else {
        return;
    };
    for path in paths {
        let path_buf = PathBuf::from(path);
        if want_on {
            let _ = db::add_tag(&conn, &path_buf, tag);
        } else {
            let _ = db::remove_tag(&conn, &path_buf, tag);
        }
        if let Ok(tags) = db::list_tags_for_path(&conn, Path::new(path)) {
            app_state.tags_cache.borrow_mut().insert(path.clone(), tags);
        }
    }
}

fn apply_favourite_to_paths(app_state: &AppState, paths: &[String], want: bool) {
    let Some(folder) = app_state.current_folder.borrow().clone() else {
        return;
    };
    let Ok(conn) = db::open(&folder) else {
        return;
    };
    for path in paths {
        let _ = db::set_favourite(&conn, Path::new(path), want);
        app_state
            .favourite_cache
            .borrow_mut()
            .insert(path.clone(), want);
    }
    if let Some(cb) = app_state.on_favourite_changed.borrow().as_ref() {
        if let Some(primary) = app_state.selected_path.borrow().as_ref() {
            let is_fav = app_state
                .favourite_cache
                .borrow()
                .get(primary)
                .copied()
                .unwrap_or(false);
            cb(is_fav);
        }
    }
}

/// Show/hide batch vs single sidebar content.
pub(crate) fn set_batch_mode_visible(sidebar_stack: &gtk4::Stack, batch: bool) {
    sidebar_stack.set_visible_child_name(if batch { "batch" } else { "single" });
}

#[allow(dead_code)]
pub(crate) fn is_batch_mode(selection: &gtk4::MultiSelection) -> bool {
    selected_count(selection) > 1
}
