use crate::db;
use crate::dialogs::{open_add_tag_dialog, open_trash_dialog};
use crate::metadata::ImageMetadata;
use crate::metadata_section::{apply_metadata_section_state, metadata_has_content};
use crate::metadata_view::{
    extract_seed_from_parameters, format_generation_command, format_metadata_text,
    has_generation_command_content,
};
use crate::similarity::{
    find_similar_paths, meta_has_similarity_source, upsert_prompt_index, SIMILAR_MIN_SCORE,
    SIMILAR_TOP_N,
};
use crate::ui::controls::refresh_tag_filter_from_folder;
use crate::ui::list_mutation::ListMutationContext;
use crate::ui::grid::{enter_compare_view_mode, refresh_realized_grid_favourite_icons};
use crate::ui::preview::{clear_picture, load_picture_async};
use crate::thumbnails;
use crate::view_helpers::{attach_context_menu_with_prepare, selected_image_path};
use gtk4::glib;
use gtk4::glib::prelude::*;
use gtk4::prelude::*;
use gtk4::{gdk, gio, GridView, Picture, SingleSelection, StringObject};
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;

#[derive(Clone)]
struct CtxMenuActionHandles {
    copy_prompt: gio::SimpleAction,
    copy_negative_prompt: gio::SimpleAction,
    copy_seed: gio::SimpleAction,
    copy_generation_command: gio::SimpleAction,
    copy_image: gio::SimpleAction,
    copy_path: gio::SimpleAction,
    copy_metadata: gio::SimpleAction,
    refresh_thumbnail: gio::SimpleAction,
    refresh_metadata: gio::SimpleAction,
    refresh_folder_thumbnails: gio::SimpleAction,
    refresh_folder_metadata: gio::SimpleAction,
    open_in_file_manager: gio::SimpleAction,
    open_in_external_editor: gio::SimpleAction,
    toggle_favourite: gio::SimpleAction,
    add_tag: gio::SimpleAction,
    remove_tag: gio::SimpleAction,
    move_to_trash: gio::SimpleAction,
    pin_for_compare: gio::SimpleAction,
    exit_compare: gio::SimpleAction,
    show_similar: gio::SimpleAction,
    remove_tags_menu: gio::Menu,
}

fn register_context_menu_accels(window: &adw::ApplicationWindow) {
    let Some(app) = window.application() else {
        return;
    };
    app.set_accels_for_action("ctx.copy-prompt", &["<Primary><Shift>p"]);
    app.set_accels_for_action("ctx.copy-negative-prompt", &["<Primary><Shift>n"]);
    app.set_accels_for_action("ctx.copy-seed", &["<Primary><Shift>s"]);
    app.set_accels_for_action("ctx.copy-path", &["<Primary><Shift>c"]);
    app.set_accels_for_action("ctx.copy-metadata", &["<Primary><Shift>m"]);
    app.set_accels_for_action("ctx.copy-generation-command", &["<Primary><Shift>g"]);
    app.set_accels_for_action("ctx.refresh-folder-thumbnails", &["<Primary><Alt>t"]);
    app.set_accels_for_action("ctx.refresh-folder-metadata", &["<Primary><Alt>m"]);
    // Delete → trash is handled in `keyboard.rs` so `SearchEntry` and other text widgets keep Delete.
}

fn select_path_for_context_menu(selection_model: &SingleSelection, path: &str) {
    for idx in 0..selection_model.n_items() {
        let is_match = selection_model
            .item(idx)
            .and_downcast::<StringObject>()
            .map(|obj| obj.string().as_str() == path)
            .unwrap_or(false);
        if is_match {
            selection_model.set_selected(idx);
            return;
        }
    }
}

fn bound_path_at_widget_point(
    widget: &gtk4::Widget,
    x: f64,
    y: f64,
    bound_paths: &Rc<RefCell<HashMap<usize, String>>>,
) -> Option<String> {
    let root_ptr = widget.as_ptr() as usize;
    let mut current = widget.pick(x, y, gtk4::PickFlags::DEFAULT);
    while let Some(candidate) = current {
        let key = candidate.as_ptr() as usize;
        if let Some(path) = bound_paths.borrow().get(&key).cloned() {
            return Some(path);
        }
        if key == root_ptr {
            break;
        }
        current = candidate.parent();
    }
    None
}

fn sync_context_menu_action_states(
    selection_model: &SingleSelection,
    meta_cache: &RefCell<HashMap<String, ImageMetadata>>,
    current_folder: &RefCell<Option<PathBuf>>,
    pinned_compare_path: &RefCell<Option<String>>,
    h: &CtxMenuActionHandles,
) {
    let path_opt = selected_image_path(selection_model);
    let key = path_opt.as_ref().map(|p| p.to_string_lossy().to_string());
    let meta = key
        .as_ref()
        .and_then(|k| meta_cache.borrow().get(k).cloned());
    let has_sel = path_opt.is_some();
    let file_on_disk = path_opt.as_ref().is_some_and(|p| p.exists());
    let parent_ok = path_opt
        .as_ref()
        .and_then(|p| p.parent())
        .is_some_and(|d| d.exists());
    let folder_open = current_folder
        .borrow()
        .as_ref()
        .is_some_and(|d| d.is_dir());

    let indexed = match (path_opt.as_ref(), current_folder.borrow().as_ref().cloned()) {
        (Some(path), Some(folder)) => db::open(&folder)
            .map(|conn| db::image_row_exists(&conn, path))
            .unwrap_or(false),
        _ => false,
    };

    let prompt_ok = meta
        .as_ref()
        .and_then(|m| m.prompt.as_ref())
        .is_some_and(|s| !s.trim().is_empty());
    let neg_ok = meta
        .as_ref()
        .and_then(|m| m.negative_prompt.as_ref())
        .is_some_and(|s| !s.trim().is_empty());
    let seed_ok = meta
        .as_ref()
        .and_then(|m| extract_seed_from_parameters(m))
        .is_some();
    let gen_ok = meta
        .as_ref()
        .is_some_and(|m| has_generation_command_content(m));
    let meta_ok = meta.as_ref().is_some_and(metadata_has_content);
    let similar_ok = meta.as_ref().is_some_and(meta_has_similarity_source);

    h.copy_prompt.set_enabled(prompt_ok);
    h.copy_negative_prompt.set_enabled(neg_ok);
    h.copy_seed.set_enabled(seed_ok);
    h.copy_generation_command.set_enabled(gen_ok);
    h.show_similar.set_enabled(similar_ok);
    h.copy_image.set_enabled(has_sel && file_on_disk);
    h.copy_path.set_enabled(has_sel && file_on_disk);
    h.copy_metadata.set_enabled(meta_ok);
    h.refresh_thumbnail.set_enabled(has_sel && file_on_disk);
    h.refresh_metadata.set_enabled(has_sel && file_on_disk);
    h.refresh_folder_thumbnails.set_enabled(folder_open);
    h.refresh_folder_metadata.set_enabled(folder_open);
    h.open_in_file_manager.set_enabled(has_sel && parent_ok);
    h.open_in_external_editor.set_enabled(has_sel && file_on_disk);
    h.toggle_favourite.set_enabled(has_sel && indexed);
    h.add_tag.set_enabled(has_sel && indexed);
    h.move_to_trash.set_enabled(has_sel && file_on_disk);
    h.pin_for_compare.set_enabled(has_sel && file_on_disk);
    h.exit_compare
        .set_enabled(pinned_compare_path.borrow().is_some());

    let fav_state = match (path_opt.as_ref(), current_folder.borrow().as_ref().cloned()) {
        (Some(path), Some(folder)) if indexed => db::open(&folder)
            .ok()
            .and_then(|conn| db::get_favourite(&conn, path).ok())
            .flatten()
            .unwrap_or(false),
        _ => false,
    };
    h.toggle_favourite.set_state(&fav_state.to_variant());

    // Rebuild "Remove tag" submenu from the selected image's tags.
    while h.remove_tags_menu.n_items() > 0 {
        h.remove_tags_menu.remove(0);
    }
    let tags = match (path_opt.as_ref(), key.as_ref()) {
        (Some(_), Some(path_key)) => {
            // Prefer cache; fall back to DB if needed.
            // (Caller may not have tags_cache in scope — use DB when indexed.)
            current_folder
                .borrow()
                .as_ref()
                .and_then(|folder| db::open(folder).ok())
                .and_then(|conn| {
                    db::list_tags_for_path(&conn, std::path::Path::new(path_key)).ok()
                })
                .unwrap_or_default()
        }
        _ => Vec::new(),
    };
    h.remove_tag.set_enabled(has_sel && indexed && !tags.is_empty());
    for tag in &tags {
        let item = gio::MenuItem::new(Some(tag), Some("ctx.remove-tag"));
        item.set_attribute_value("target", Some(&tag.to_variant()));
        h.remove_tags_menu.append_item(&item);
    }
}

pub fn install_context_menu(
    window: &adw::ApplicationWindow,
    toast_overlay: &adw::ToastOverlay,
    selection_model: &SingleSelection,
    meta_cache: &Rc<RefCell<HashMap<String, ImageMetadata>>>,
    hash_cache: &Rc<RefCell<HashMap<String, String>>>,
    thumbnail_size: &Rc<RefCell<i32>>,
    meta_expander: &gtk4::Expander,
    meta_paned: &gtk4::Paned,
    meta_split_before_auto_collapse: &Rc<Cell<Option<i32>>>,
    meta_position_programmatic: &Rc<Cell<u32>>,
    meta_section_expanded_pref: &Rc<Cell<bool>>,
    min_meta_split_px: i32,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    start_scan_for_folder: &Rc<dyn Fn(PathBuf)>,
    list_store: &gio::ListStore,
    refresh_metadata_sidebar: &Rc<dyn Fn(&ImageMetadata)>,
    external_editor: Option<&PathBuf>,
    grid_view: &GridView,
    single_picture: &Picture,
    compare_left_picture: &Picture,
    compare_right_picture: &Picture,
    meta_preview: &Picture,
    view_stack: &adw::ViewStack,
    left_toggle: &gtk4::ToggleButton,
    right_toggle: &gtk4::ToggleButton,
    pre_fullview_left: &Rc<Cell<bool>>,
    pre_fullview_right: &Rc<Cell<bool>>,
    mutation_ctx: &ListMutationContext,
    filter: &gtk4::CustomFilter,
    on_favourite_changed: Rc<dyn Fn(bool)>,
    tags_filter_btn: &gtk4::MenuButton,
    tags_filter_list: &gtk4::Box,
) -> Rc<dyn Fn()> {
    let action_group = gio::SimpleActionGroup::new();
    let sync_context_menu_slot: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));

    let copy_prompt_action = gio::SimpleAction::new("copy-prompt", None);
    let copy_negative_prompt_action = gio::SimpleAction::new("copy-negative-prompt", None);
    let copy_seed_action = gio::SimpleAction::new("copy-seed", None);
    let copy_generation_command_action = gio::SimpleAction::new("copy-generation-command", None);
    let copy_image_action = gio::SimpleAction::new("copy-image", None);
    let copy_path_action = gio::SimpleAction::new("copy-path", None);
    let copy_metadata_action = gio::SimpleAction::new("copy-metadata", None);
    let refresh_thumb_action = gio::SimpleAction::new("refresh-thumbnail", None);
    let refresh_meta_action = gio::SimpleAction::new("refresh-metadata", None);
    let refresh_folder_thumbs_action = gio::SimpleAction::new("refresh-folder-thumbnails", None);
    let refresh_folder_meta_action = gio::SimpleAction::new("refresh-folder-metadata", None);
    let open_in_file_manager_action = gio::SimpleAction::new("open-in-file-manager", None);
    let open_in_external_editor_action = gio::SimpleAction::new("open-in-external-editor", None);
    let toggle_favourite_action =
        gio::SimpleAction::new_stateful("toggle-favourite", None, &false.to_variant());
    let add_tag_action = gio::SimpleAction::new("add-tag", None);
    let remove_tag_action =
        gio::SimpleAction::new("remove-tag", Some(glib::VariantTy::STRING));
    let remove_tags_menu = gio::Menu::new();
    let move_to_trash_action = gio::SimpleAction::new("move-to-trash", None);
    let show_similar_action = gio::SimpleAction::new("show-similar", None);

    let selection_for_actions = selection_model.clone();
    let meta_cache_for_actions = meta_cache.clone();
    let window_for_actions = window.clone();
    copy_prompt_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let path_key = path.to_string_lossy().to_string();
        let text = meta_cache_for_actions
            .borrow()
            .get(&path_key)
            .and_then(|meta| meta.prompt.clone())
            .unwrap_or_else(|| "No prompt found".to_string());
        gtk4::prelude::WidgetExt::display(&window_for_actions)
            .clipboard()
            .set_text(&text);
    });

    let selection_for_actions = selection_model.clone();
    let meta_cache_for_actions = meta_cache.clone();
    let window_for_actions = window.clone();
    copy_negative_prompt_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let path_key = path.to_string_lossy().to_string();
        let text = meta_cache_for_actions
            .borrow()
            .get(&path_key)
            .and_then(|meta| meta.negative_prompt.clone())
            .unwrap_or_else(|| "No negative prompt found".to_string());
        gtk4::prelude::WidgetExt::display(&window_for_actions)
            .clipboard()
            .set_text(&text);
    });

    let selection_for_actions = selection_model.clone();
    let meta_cache_for_actions = meta_cache.clone();
    let window_for_actions = window.clone();
    copy_seed_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let path_key = path.to_string_lossy().to_string();
        let text = meta_cache_for_actions
            .borrow()
            .get(&path_key)
            .and_then(extract_seed_from_parameters)
            .unwrap_or_else(|| "No seed found".to_string());
        gtk4::prelude::WidgetExt::display(&window_for_actions)
            .clipboard()
            .set_text(&text);
    });

    let selection_for_actions = selection_model.clone();
    let meta_cache_for_actions = meta_cache.clone();
    let window_for_actions = window.clone();
    copy_generation_command_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let path_key = path.to_string_lossy().to_string();
        let text = meta_cache_for_actions
            .borrow()
            .get(&path_key)
            .map(format_generation_command)
            .unwrap_or_else(|| "No generation parameters found".to_string());
        gtk4::prelude::WidgetExt::display(&window_for_actions)
            .clipboard()
            .set_text(&text);
    });

    let selection_for_actions = selection_model.clone();
    let window_for_actions = window.clone();
    let toast_overlay_for_actions = toast_overlay.clone();
    copy_image_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let file = gio::File::for_path(&path);
        if let Ok(texture) = gdk::Texture::from_file(&file) {
            gtk4::prelude::WidgetExt::display(&window_for_actions)
                .clipboard()
                .set_texture(&texture);
            let toast = adw::Toast::new("Image copied to clipboard");
            toast.set_timeout(2);
            toast_overlay_for_actions.add_toast(toast);
        }
    });

    let selection_for_actions = selection_model.clone();
    let window_for_actions = window.clone();
    copy_path_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        gtk4::prelude::WidgetExt::display(&window_for_actions)
            .clipboard()
            .set_text(&path.to_string_lossy());
    });

    let selection_for_actions = selection_model.clone();
    let meta_cache_for_actions = meta_cache.clone();
    let window_for_actions = window.clone();
    copy_metadata_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let path_key = path.to_string_lossy().to_string();
        let text = meta_cache_for_actions
            .borrow()
            .get(&path_key)
            .map(format_metadata_text)
            .unwrap_or_else(|| "No metadata found".to_string());
        gtk4::prelude::WidgetExt::display(&window_for_actions)
            .clipboard()
            .set_text(&text);
    });

    let selection_for_actions = selection_model.clone();
    let hash_cache_for_actions = hash_cache.clone();
    let toast_overlay_for_actions = toast_overlay.clone();
    let thumbnail_size_for_actions = thumbnail_size.clone();
    refresh_thumb_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let hash = hash_cache_for_actions
            .borrow()
            .get(&path.to_string_lossy().to_string())
            .cloned()
            .or_else(|| db::hash_file(&path).ok());
        let Some(hash) = hash else { return };

        thumbnails::remove_hash_thumbnail_variants(&hash);
        let _ = thumbnails::generate_hash_thumbnail(&path, &hash);
        let current_size = *thumbnail_size_for_actions.borrow();
        if current_size != thumbnails::THUMB_NORMAL_SIZE {
            let _ = thumbnails::generate_hash_thumbnail_for_size(&path, &hash, current_size);
        }
        hash_cache_for_actions
            .borrow_mut()
            .insert(path.to_string_lossy().to_string(), hash);

        let toast = adw::Toast::new("Thumbnail refreshed");
        toast.set_timeout(2);
        toast_overlay_for_actions.add_toast(toast);
    });

    let selection_for_actions = selection_model.clone();
    let meta_cache_for_actions = meta_cache.clone();
    let hash_cache_for_actions = hash_cache.clone();
    let prompt_index_for_refresh = mutation_ctx.app_state.prompt_similarity_index.clone();
    let refresh_metadata_sidebar_for_actions = refresh_metadata_sidebar.clone();
    let meta_expander_for_actions = meta_expander.clone();
    let meta_paned_for_actions = meta_paned.clone();
    let meta_split_before_auto_collapse_for_actions = meta_split_before_auto_collapse.clone();
    let meta_position_programmatic_for_actions = meta_position_programmatic.clone();
    let meta_section_expanded_pref_for_actions = meta_section_expanded_pref.clone();
    let toast_overlay_for_actions = toast_overlay.clone();
    let sync_context_menu_slot_meta = sync_context_menu_slot.clone();
    refresh_meta_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let Some(folder) = path.parent().map(|p| p.to_path_buf()) else {
            return;
        };

        let Ok(conn) = db::open(&folder) else {
            return;
        };
        if let Some(row) = db::refresh_indexed(&conn, &path) {
            let path_key = path.to_string_lossy().to_string();
            upsert_prompt_index(
                &mut prompt_index_for_refresh.borrow_mut(),
                &path_key,
                &row.meta,
            );
            meta_cache_for_actions
                .borrow_mut()
                .insert(path_key.clone(), row.meta.clone());
            hash_cache_for_actions
                .borrow_mut()
                .insert(path_key, row.hash);
            refresh_metadata_sidebar_for_actions(&row.meta);
            meta_position_programmatic_for_actions.set(
                meta_position_programmatic_for_actions
                    .get()
                    .saturating_add(1),
            );
            apply_metadata_section_state(
                &row.meta,
                &meta_expander_for_actions,
                &meta_paned_for_actions,
                &meta_split_before_auto_collapse_for_actions,
                min_meta_split_px,
                &meta_section_expanded_pref_for_actions,
            );
            meta_position_programmatic_for_actions.set(
                meta_position_programmatic_for_actions
                    .get()
                    .saturating_sub(1),
            );

            let toast = adw::Toast::new("Metadata refreshed");
            toast.set_timeout(2);
            toast_overlay_for_actions.add_toast(toast);
            if let Some(sync) = sync_context_menu_slot_meta.borrow().as_ref() {
                sync();
            }
        }
    });

    let current_folder_for_actions = current_folder.clone();
    let hash_cache_for_actions = hash_cache.clone();
    let start_scan_for_actions = start_scan_for_folder.clone();
    refresh_folder_thumbs_action.connect_activate(move |_, _| {
        let Some(folder) = current_folder_for_actions.borrow().as_ref().cloned() else {
            return;
        };

        let cached_hashes: Vec<String> =
            hash_cache_for_actions.borrow().values().cloned().collect();
        for hash in cached_hashes {
            thumbnails::remove_hash_thumbnail_variants(&hash);
        }
        start_scan_for_actions(folder);
    });

    let current_folder_for_actions = current_folder.clone();
    let list_store_for_actions = list_store.clone();
    let start_scan_for_actions = start_scan_for_folder.clone();
    refresh_folder_meta_action.connect_activate(move |_, _| {
        let Some(folder) = current_folder_for_actions.borrow().as_ref().cloned() else {
            return;
        };

        let mut paths = Vec::new();
        for i in 0..list_store_for_actions.n_items() {
            if let Some(item) = list_store_for_actions
                .item(i)
                .and_downcast::<StringObject>()
            {
                paths.push(PathBuf::from(item.string().as_str()));
            }
        }

        if let Ok(conn) = db::open(&folder) {
            for p in &paths {
                let _ = db::refresh_indexed(&conn, p);
            }
        }
        start_scan_for_actions(folder);
    });

    let selection_for_actions = selection_model.clone();
    let toast_overlay_for_actions = toast_overlay.clone();
    open_in_file_manager_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let Some(parent) = path.parent() else {
            return;
        };
        let uri = gio::File::for_path(parent).uri();
        if gio::AppInfo::launch_default_for_uri(&uri, Option::<&gio::AppLaunchContext>::None).is_err()
        {
            let toast = adw::Toast::new("Could not open file manager");
            toast.set_timeout(3);
            toast_overlay_for_actions.add_toast(toast);
        }
    });

    let selection_for_actions = selection_model.clone();
    let toast_overlay_for_actions = toast_overlay.clone();
    let external_editor_for_actions = external_editor.cloned();
    open_in_external_editor_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let failed = if let Some(editor) = external_editor_for_actions.as_ref() {
            Command::new(editor).arg(&path).spawn().is_err()
        } else {
            let uri = gio::File::for_path(&path).uri();
            gio::AppInfo::launch_default_for_uri(&uri, Option::<&gio::AppLaunchContext>::None)
                .is_err()
        };
        if failed {
            let toast = adw::Toast::new("Could not open in external editor");
            toast.set_timeout(3);
            toast_overlay_for_actions.add_toast(toast);
        }
    });

    let selection_for_actions = selection_model.clone();
    let toast_overlay_for_actions = toast_overlay.clone();
    let current_folder_for_favourite = current_folder.clone();
    let sync_context_menu_slot_fav = sync_context_menu_slot.clone();
    let toggle_favourite_for_state = toggle_favourite_action.clone();
    let app_state_for_favourite = mutation_ctx.app_state.clone();
    let filter_for_favourite = filter.clone();
    let on_favourite_changed_action = on_favourite_changed.clone();
    toggle_favourite_action.connect_change_state(move |_, requested| {
        let Some(requested) = requested else {
            return;
        };
        let Some(want_fav) = requested.get::<bool>() else {
            return;
        };
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        let Some(folder) = current_folder_for_favourite.borrow().as_ref().cloned() else {
            return;
        };
        let Ok(conn) = db::open(&folder) else {
            return;
        };
        let prev = toggle_favourite_for_state
            .state()
            .and_then(|s| s.get::<bool>())
            .unwrap_or(false);
        let (toast_text, applied) = match db::set_favourite(&conn, &path, want_fav) {
            Ok(true) => (
                if want_fav {
                    "Added to favourites"
                } else {
                    "Removed from favourites"
                },
                true,
            ),
            Ok(false) => ("Not indexed yet — try again after scan finishes", false),
            Err(_) => ("Could not update favourite", false),
        };
        let toast = adw::Toast::new(toast_text);
        toast.set_timeout(2);
        toast_overlay_for_actions.add_toast(toast);
        if applied {
            toggle_favourite_for_state.set_state(requested);
            let path_key = path.to_string_lossy().to_string();
            app_state_for_favourite
                .favourite_cache
                .borrow_mut()
                .insert(path_key, want_fav);
            filter_for_favourite.changed(gtk4::FilterChange::Different);
            refresh_realized_grid_favourite_icons(&app_state_for_favourite);
            on_favourite_changed_action(want_fav);
        } else {
            toggle_favourite_for_state.set_state(&prev.to_variant());
        }
        if let Some(sync) = sync_context_menu_slot_fav.borrow().as_ref() {
            sync();
        }
    });

    let selection_for_add_tag = selection_model.clone();
    let window_for_add_tag = window.clone();
    let toast_for_add_tag = toast_overlay.clone();
    let mutation_for_add_tag = mutation_ctx.clone();
    let filter_for_add_tag = filter.clone();
    let tags_filter_btn_for_add = tags_filter_btn.clone();
    let tags_filter_list_for_add = tags_filter_list.clone();
    let sync_slot_for_add = sync_context_menu_slot.clone();
    add_tag_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_add_tag) else {
            return;
        };
        let on_tags_changed: Rc<dyn Fn()> = {
            let filter = filter_for_add_tag.clone();
            let tags_filter_btn = tags_filter_btn_for_add.clone();
            let tags_filter_list = tags_filter_list_for_add.clone();
            let app_state = mutation_for_add_tag.app_state.clone();
            let sync_slot = sync_slot_for_add.clone();
            Rc::new(move || {
                filter.changed(gtk4::FilterChange::Different);
                refresh_tag_filter_from_folder(
                    &tags_filter_list,
                    &tags_filter_btn,
                    &app_state.active_tags,
                    &filter,
                    &app_state.current_folder,
                );
                if let Some(sync) = sync_slot.borrow().as_ref() {
                    sync();
                }
            })
        };
        open_add_tag_dialog(
            &window_for_add_tag,
            &toast_for_add_tag,
            &mutation_for_add_tag,
            path,
            on_tags_changed,
        );
    });

    let selection_for_remove_tag = selection_model.clone();
    let current_folder_for_remove_tag = current_folder.clone();
    let toast_for_remove_tag = toast_overlay.clone();
    let mutation_for_remove_tag = mutation_ctx.clone();
    let filter_for_remove_tag = filter.clone();
    let tags_filter_btn_for_remove = tags_filter_btn.clone();
    let tags_filter_list_for_remove = tags_filter_list.clone();
    let sync_slot_for_remove = sync_context_menu_slot.clone();
    remove_tag_action.connect_activate(move |_, param| {
        let Some(tag) = param.and_then(|v| v.str().map(|s| s.to_string())) else {
            return;
        };
        let Some(path) = selected_image_path(&selection_for_remove_tag) else {
            return;
        };
        let Some(folder) = current_folder_for_remove_tag.borrow().as_ref().cloned() else {
            return;
        };
        let Ok(conn) = db::open(&folder) else {
            toast_for_remove_tag.add_toast(adw::Toast::new("Could not open folder database"));
            return;
        };
        match db::remove_tag(&conn, &path, &tag) {
            Ok(true) => {
                let path_key = path.to_string_lossy().to_string();
                let tags = db::list_tags_for_path(&conn, &path).unwrap_or_default();
                if tags.is_empty() {
                    mutation_for_remove_tag
                        .app_state
                        .tags_cache
                        .borrow_mut()
                        .remove(&path_key);
                } else {
                    mutation_for_remove_tag
                        .app_state
                        .tags_cache
                        .borrow_mut()
                        .insert(path_key, tags);
                }
                filter_for_remove_tag.changed(gtk4::FilterChange::Different);
                refresh_tag_filter_from_folder(
                    &tags_filter_list_for_remove,
                    &tags_filter_btn_for_remove,
                    &mutation_for_remove_tag.app_state.active_tags,
                    &filter_for_remove_tag,
                    &mutation_for_remove_tag.app_state.current_folder,
                );
                if let Some(sync) = sync_slot_for_remove.borrow().as_ref() {
                    sync();
                }
                toast_for_remove_tag.add_toast(adw::Toast::new(&format!("Removed tag “{tag}”")));
            }
            Ok(false) => {
                toast_for_remove_tag.add_toast(adw::Toast::new("Tag not found"));
            }
            Err(_) => {
                toast_for_remove_tag.add_toast(adw::Toast::new("Could not remove tag"));
            }
        }
    });

    let selection_for_actions = selection_model.clone();
    let window_for_trash = window.clone();
    let toast_for_trash = toast_overlay.clone();
    let mutation_ctx_for_trash = mutation_ctx.clone();
    move_to_trash_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        open_trash_dialog(&window_for_trash, &toast_for_trash, &mutation_ctx_for_trash, path);
    });

    let pin_for_compare_action = gio::SimpleAction::new("pin-for-compare", None);
    let exit_compare_action = gio::SimpleAction::new("exit-compare", None);

    {
        let selection_for_pin = selection_model.clone();
        let pinned_for_pin = mutation_ctx.app_state.pinned_compare_path.clone();
        let left_picture = compare_left_picture.clone();
        let right_picture = compare_right_picture.clone();
        let view_stack_pin = view_stack.clone();
        let left_toggle_pin = left_toggle.clone();
        let right_toggle_pin = right_toggle.clone();
        let pre_left_pin = pre_fullview_left.clone();
        let pre_right_pin = pre_fullview_right.clone();
        let sync_slot_pin = sync_context_menu_slot.clone();
        let toast_pin = toast_overlay.clone();
        pin_for_compare_action.connect_activate(move |_, _| {
            let Some(path) = selected_image_path(&selection_for_pin) else {
                return;
            };
            if !path.exists() {
                return;
            }
            let path_str = path.to_string_lossy().to_string();
            *pinned_for_pin.borrow_mut() = Some(path_str.clone());
            load_picture_async(&left_picture, &path_str, None, None);
            load_picture_async(&right_picture, &path_str, None, None);
            enter_compare_view_mode(
                &view_stack_pin,
                &left_toggle_pin,
                &right_toggle_pin,
                &pre_left_pin,
                &pre_right_pin,
            );
            let toast = adw::Toast::new("Pinned for compare — Left/Right advances the right pane");
            toast.set_timeout(2);
            toast_pin.add_toast(toast);
            if let Some(sync) = sync_slot_pin.borrow().as_ref() {
                sync();
            }
        });
    }

    {
        let pinned_for_exit = mutation_ctx.app_state.pinned_compare_path.clone();
        let left_picture = compare_left_picture.clone();
        let right_picture = compare_right_picture.clone();
        let single_picture_exit = single_picture.clone();
        let selection_for_exit = selection_model.clone();
        let view_stack_exit = view_stack.clone();
        let sync_slot_exit = sync_context_menu_slot.clone();
        exit_compare_action.connect_activate(move |_, _| {
            *pinned_for_exit.borrow_mut() = None;
            clear_picture(&left_picture);
            clear_picture(&right_picture);
            if view_stack_exit.visible_child_name().as_deref() == Some("compare") {
                if let Some(item) = selection_for_exit
                    .selected_item()
                    .and_downcast::<StringObject>()
                {
                    load_picture_async(
                        &single_picture_exit,
                        &item.string().to_string(),
                        None,
                        None,
                    );
                } else {
                    clear_picture(&single_picture_exit);
                }
                view_stack_exit.set_visible_child_name("single");
            }
            if let Some(sync) = sync_slot_exit.borrow().as_ref() {
                sync();
            }
        });
    }

    {
        let selection_for_similar = selection_model.clone();
        let index_for_similar = mutation_ctx.app_state.prompt_similarity_index.clone();
        let similar_paths = mutation_ctx.app_state.similar_paths.clone();
        let filter_for_similar = filter.clone();
        let toast_for_similar = toast_overlay.clone();
        show_similar_action.connect_activate(move |_, _| {
            let Some(path) = selected_image_path(&selection_for_similar) else {
                return;
            };
            let path_key = path.to_string_lossy().to_string();
            let Some(matches) = find_similar_paths(
                &index_for_similar.borrow(),
                &path_key,
                SIMILAR_TOP_N,
                SIMILAR_MIN_SCORE,
            ) else {
                return;
            };
            let count = matches.len();
            *similar_paths.borrow_mut() = Some(matches);
            filter_for_similar.changed(gtk4::FilterChange::Different);

            let toast = adw::Toast::new(&format!(
                "Showing {} similar image{}",
                count,
                if count == 1 { "" } else { "s" }
            ));
            toast.set_button_label(Some("Clear"));
            toast.set_timeout(4);
            let similar_paths_clear = similar_paths.clone();
            let filter_clear = filter_for_similar.clone();
            toast.connect_button_clicked(move |_| {
                *similar_paths_clear.borrow_mut() = None;
                filter_clear.changed(gtk4::FilterChange::Different);
            });
            toast_for_similar.add_toast(toast);
        });
    }

    action_group.add_action(&copy_prompt_action);
    action_group.add_action(&copy_negative_prompt_action);
    action_group.add_action(&copy_seed_action);
    action_group.add_action(&copy_generation_command_action);
    action_group.add_action(&copy_image_action);
    action_group.add_action(&copy_path_action);
    action_group.add_action(&copy_metadata_action);
    action_group.add_action(&refresh_thumb_action);
    action_group.add_action(&refresh_meta_action);
    action_group.add_action(&refresh_folder_thumbs_action);
    action_group.add_action(&refresh_folder_meta_action);
    action_group.add_action(&open_in_file_manager_action);
    action_group.add_action(&open_in_external_editor_action);
    action_group.add_action(&toggle_favourite_action);
    action_group.add_action(&add_tag_action);
    action_group.add_action(&remove_tag_action);
    action_group.add_action(&move_to_trash_action);
    action_group.add_action(&pin_for_compare_action);
    action_group.add_action(&exit_compare_action);
    action_group.add_action(&show_similar_action);
    window.insert_action_group("ctx", Some(&action_group));
    register_context_menu_accels(window);

    let menu_model = gio::Menu::new();
    let prompt_section = gio::Menu::new();
    prompt_section.append(Some("Copy Prompt"), Some("ctx.copy-prompt"));
    prompt_section.append(
        Some("Copy Negative Prompt"),
        Some("ctx.copy-negative-prompt"),
    );
    prompt_section.append(Some("Copy Seed"), Some("ctx.copy-seed"));
    prompt_section.append(
        Some("Copy Generation Command"),
        Some("ctx.copy-generation-command"),
    );
    prompt_section.append(Some("Similar in folder"), Some("ctx.show-similar"));
    menu_model.append_section(None, &prompt_section);

    let clipboard_section = gio::Menu::new();
    clipboard_section.append(Some("Copy Image"), Some("ctx.copy-image"));
    clipboard_section.append(Some("Copy Path"), Some("ctx.copy-path"));
    clipboard_section.append(Some("Copy Metadata"), Some("ctx.copy-metadata"));
    menu_model.append_section(None, &clipboard_section);

    let open_section = gio::Menu::new();
    open_section.append(
        Some("Open in File Manager"),
        Some("ctx.open-in-file-manager"),
    );
    open_section.append(
        Some("Open in External Editor"),
        Some("ctx.open-in-external-editor"),
    );
    menu_model.append_section(None, &open_section);

    let organise_section = gio::Menu::new();
    organise_section.append(Some("Favourite"), Some("ctx.toggle-favourite"));
    organise_section.append(Some("Add tag…"), Some("ctx.add-tag"));
    organise_section.append_submenu(Some("Remove tag"), &remove_tags_menu);
    organise_section.append(Some("Pin for compare"), Some("ctx.pin-for-compare"));
    organise_section.append(Some("Exit compare"), Some("ctx.exit-compare"));
    organise_section.append(Some("Move to Trash"), Some("ctx.move-to-trash"));
    menu_model.append_section(None, &organise_section);

    let refresh_submenu = gio::Menu::new();
    refresh_submenu.append(Some("Refresh Thumbnail"), Some("ctx.refresh-thumbnail"));
    refresh_submenu.append(Some("Refresh Metadata"), Some("ctx.refresh-metadata"));
    refresh_submenu.append(
        Some("Refresh Folder Thumbnails"),
        Some("ctx.refresh-folder-thumbnails"),
    );
    refresh_submenu.append(
        Some("Refresh Folder Metadata"),
        Some("ctx.refresh-folder-metadata"),
    );
    menu_model.append_submenu(Some("Refresh"), &refresh_submenu);

    let handles = CtxMenuActionHandles {
        copy_prompt: copy_prompt_action.clone(),
        copy_negative_prompt: copy_negative_prompt_action.clone(),
        copy_seed: copy_seed_action.clone(),
        copy_generation_command: copy_generation_command_action.clone(),
        copy_image: copy_image_action.clone(),
        copy_path: copy_path_action.clone(),
        copy_metadata: copy_metadata_action.clone(),
        refresh_thumbnail: refresh_thumb_action.clone(),
        refresh_metadata: refresh_meta_action.clone(),
        refresh_folder_thumbnails: refresh_folder_thumbs_action.clone(),
        refresh_folder_metadata: refresh_folder_meta_action.clone(),
        open_in_file_manager: open_in_file_manager_action.clone(),
        open_in_external_editor: open_in_external_editor_action.clone(),
        toggle_favourite: toggle_favourite_action.clone(),
        add_tag: add_tag_action.clone(),
        remove_tag: remove_tag_action.clone(),
        move_to_trash: move_to_trash_action.clone(),
        pin_for_compare: pin_for_compare_action.clone(),
        exit_compare: exit_compare_action.clone(),
        show_similar: show_similar_action.clone(),
        remove_tags_menu: remove_tags_menu.clone(),
    };
    let sync_fn = Rc::new({
        let selection_for_sync = selection_model.clone();
        let meta_cache_for_sync = meta_cache.clone();
        let current_folder_for_sync = current_folder.clone();
        let pinned_for_sync = mutation_ctx.app_state.pinned_compare_path.clone();
        let handles_for_sync = handles.clone();
        move || {
            sync_context_menu_action_states(
                &selection_for_sync,
                &meta_cache_for_sync,
                &current_folder_for_sync,
                &pinned_for_sync,
                &handles_for_sync,
            );
        }
    });
    *sync_context_menu_slot.borrow_mut() = Some(sync_fn.clone());
    {
        let selection_for_menu = selection_model.clone();
        let sync_for_menu = sync_fn.clone();
        let bound_paths_for_menu = mutation_ctx.app_state.bound_paths.clone();
        attach_context_menu_with_prepare(grid_view, &menu_model, move |widget, x, y| {
            if let Some(path) = bound_path_at_widget_point(widget, x, y, &bound_paths_for_menu) {
                select_path_for_context_menu(&selection_for_menu, &path);
            }
            sync_for_menu();
        });
    }
    {
        let sync_for_menu = sync_fn.clone();
        attach_context_menu_with_prepare(single_picture, &menu_model, move |_, _, _| {
            sync_for_menu();
        });
    }
    {
        let sync_for_menu = sync_fn.clone();
        attach_context_menu_with_prepare(compare_left_picture, &menu_model, move |_, _, _| {
            sync_for_menu();
        });
    }
    {
        let sync_for_menu = sync_fn.clone();
        attach_context_menu_with_prepare(compare_right_picture, &menu_model, move |_, _, _| {
            sync_for_menu();
        });
    }
    {
        let sync_for_menu = sync_fn.clone();
        attach_context_menu_with_prepare(meta_preview, &menu_model, move |_, _, _| {
            sync_for_menu();
        });
    }
    {
        let sync_on_sel = sync_fn.clone();
        selection_model.connect_selection_changed(move |_, _, _| {
            sync_on_sel();
        });
    }
    sync_fn();
    sync_fn
}
