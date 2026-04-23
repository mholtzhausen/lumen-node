use crate::db;
use crate::metadata::ImageMetadata;
use crate::metadata_section::apply_metadata_section_state;
use crate::metadata_view::{
    extract_seed_from_parameters, format_generation_command, format_metadata_text,
};
use crate::thumbnails;
use crate::view_helpers::{attach_context_menu, selected_image_path};
use gtk4::prelude::*;
use gtk4::{gdk, gio, GridView, Picture, SingleSelection, StringObject};
use libadwaita as adw;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

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
    min_meta_split_px: i32,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    start_scan_for_folder: &Rc<dyn Fn(PathBuf)>,
    list_store: &gio::ListStore,
    refresh_metadata_sidebar: &Rc<dyn Fn(&ImageMetadata)>,
    grid_view: &GridView,
    single_picture: &Picture,
    meta_preview: &Picture,
) {
    let action_group = gio::SimpleActionGroup::new();

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
    let toast_overlay_for_actions = toast_overlay.clone();
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
            apply_metadata_section_state(
                &row.meta,
                &meta_expander_for_actions,
                &meta_paned_for_actions,
                &meta_split_before_auto_collapse_for_actions,
                min_meta_split_px,
            );

            let toast = adw::Toast::new("Metadata refreshed");
            toast.set_timeout(2);
            toast_overlay_for_actions.add_toast(toast);
        }
    });

    let current_folder_for_actions = current_folder.clone();
    let hash_cache_for_actions = hash_cache.clone();
    let start_scan_for_actions = start_scan_for_folder.clone();
    refresh_folder_thumbs_action.connect_activate(move |_, _| {
        let Some(folder) = current_folder_for_actions.borrow().as_ref().cloned() else {
            return;
        };

        let cached_hashes: Vec<String> = hash_cache_for_actions.borrow().values().cloned().collect();
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
            if let Some(item) = list_store_for_actions.item(i).and_downcast::<StringObject>() {
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
    window.insert_action_group("ctx", Some(&action_group));

    let menu_model = gio::Menu::new();
    let prompt_section = gio::Menu::new();
    prompt_section.append(Some("Copy Prompt"), Some("ctx.copy-prompt"));
    prompt_section.append(Some("Copy Negative Prompt"), Some("ctx.copy-negative-prompt"));
    prompt_section.append(Some("Copy Seed"), Some("ctx.copy-seed"));
    prompt_section.append(Some("Copy Generation Command"), Some("ctx.copy-generation-command"));
    menu_model.append_section(None, &prompt_section);

    let clipboard_section = gio::Menu::new();
    clipboard_section.append(Some("Copy Image"), Some("ctx.copy-image"));
    clipboard_section.append(Some("Copy Path"), Some("ctx.copy-path"));
    clipboard_section.append(Some("Copy Metadata"), Some("ctx.copy-metadata"));
    menu_model.append_section(None, &clipboard_section);

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
}
