use crate::db;
use crate::dialogs::open_trash_dialog;
use crate::metadata::ImageMetadata;
use crate::metadata_section::{apply_metadata_section_state, metadata_has_content};
use crate::metadata_view::{
    extract_seed_from_parameters, format_generation_command, format_metadata_text,
    has_generation_command_content,
};
use crate::thumbnails;
use crate::view_helpers::{attach_context_menu, selected_image_path};
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
    move_to_trash: gio::SimpleAction,
}

fn register_context_menu_accels(window: &adw::ApplicationWindow) {
    let Some(app) = window.application() else {
        return;
    };
    app.set_accels_for_action("ctx.copy-prompt", &["<Primary><Shift>p"]);
    app.set_accels_for_action("ctx.copy-negative-prompt", &["<Primary><Shift>n"]);
    app.set_accels_for_action("ctx.copy-path", &["<Primary><Shift>c"]);
    app.set_accels_for_action("ctx.move-to-trash", &["Delete"]);
}

fn sync_context_menu_action_states(
    selection_model: &SingleSelection,
    meta_cache: &RefCell<HashMap<String, ImageMetadata>>,
    current_folder: &RefCell<Option<PathBuf>>,
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

    h.copy_prompt.set_enabled(prompt_ok);
    h.copy_negative_prompt.set_enabled(neg_ok);
    h.copy_seed.set_enabled(seed_ok);
    h.copy_generation_command.set_enabled(gen_ok);
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
    h.move_to_trash.set_enabled(has_sel && file_on_disk);

    let fav_state = match (path_opt.as_ref(), current_folder.borrow().as_ref().cloned()) {
        (Some(path), Some(folder)) if indexed => db::open(&folder)
            .ok()
            .and_then(|conn| db::get_favourite(&conn, path).ok())
            .flatten()
            .unwrap_or(false),
        _ => false,
    };
    h.toggle_favourite.set_state(&fav_state.to_variant());
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
    min_meta_split_px: i32,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    start_scan_for_folder: &Rc<dyn Fn(PathBuf)>,
    list_store: &gio::ListStore,
    refresh_metadata_sidebar: &Rc<dyn Fn(&ImageMetadata)>,
    external_editor: Option<&PathBuf>,
    grid_view: &GridView,
    single_picture: &Picture,
    meta_preview: &Picture,
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
    let move_to_trash_action = gio::SimpleAction::new("move-to-trash", None);

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
    let refresh_metadata_sidebar_for_actions = refresh_metadata_sidebar.clone();
    let meta_expander_for_actions = meta_expander.clone();
    let meta_paned_for_actions = meta_paned.clone();
    let meta_split_before_auto_collapse_for_actions = meta_split_before_auto_collapse.clone();
    let meta_position_programmatic_for_actions = meta_position_programmatic.clone();
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
        } else {
            toggle_favourite_for_state.set_state(&prev.to_variant());
        }
        if let Some(sync) = sync_context_menu_slot_fav.borrow().as_ref() {
            sync();
        }
    });

    let selection_for_actions = selection_model.clone();
    let window_for_trash = window.clone();
    let toast_for_trash = toast_overlay.clone();
    let start_scan_for_trash = start_scan_for_folder.clone();
    let current_folder_for_trash = current_folder.clone();
    move_to_trash_action.connect_activate(move |_, _| {
        let Some(path) = selected_image_path(&selection_for_actions) else {
            return;
        };
        open_trash_dialog(
            &window_for_trash,
            &toast_for_trash,
            &start_scan_for_trash,
            &current_folder_for_trash,
            path,
        );
    });

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
    action_group.add_action(&move_to_trash_action);
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

    attach_context_menu(grid_view, &menu_model);
    attach_context_menu(single_picture, &menu_model);
    attach_context_menu(meta_preview, &menu_model);

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
        move_to_trash: move_to_trash_action.clone(),
    };
    let sync_fn = Rc::new({
        let selection_for_sync = selection_model.clone();
        let meta_cache_for_sync = meta_cache.clone();
        let current_folder_for_sync = current_folder.clone();
        let handles_for_sync = handles.clone();
        move || {
            sync_context_menu_action_states(
                &selection_for_sync,
                &meta_cache_for_sync,
                &current_folder_for_sync,
                &handles_for_sync,
            );
        }
    });
    *sync_context_menu_slot.borrow_mut() = Some(sync_fn.clone());
    {
        let sync_on_sel = sync_fn.clone();
        selection_model.connect_selection_changed(move |_, _, _| {
            sync_on_sel();
        });
    }
    sync_fn();
    sync_fn
}
