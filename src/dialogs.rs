use crate::file_name_ops::{build_renamed_target, split_filename};
use gtk4::gio::prelude::FileExt;
use gtk4::prelude::*;
use gtk4::{gio, Label, Orientation};
use libadwaita as adw;
use std::{cell::RefCell, rc::Rc};

pub fn open_rename_dialog(
    window: &adw::ApplicationWindow,
    toast_overlay: &adw::ToastOverlay,
    start_scan_for_folder: &Rc<dyn Fn(std::path::PathBuf)>,
    current_folder: &Rc<RefCell<Option<std::path::PathBuf>>>,
    source_path: std::path::PathBuf,
    initial_base_name: Option<String>,
) {
    let (current_base, ext) = split_filename(&source_path);
    let dialog = gtk4::Window::builder()
        .transient_for(window)
        .modal(true)
        .title("Rename file")
        .default_width(420)
        .build();

    let content = gtk4::Box::new(Orientation::Vertical, 8);
    content.set_spacing(8);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    dialog.set_child(Some(&content));

    let prompt = Label::new(Some("Enter a new base name:"));
    prompt.set_halign(gtk4::Align::Start);
    content.append(&prompt);

    let entry = gtk4::Entry::new();
    entry.set_text(initial_base_name.as_deref().unwrap_or(&current_base));
    entry.set_hexpand(true);
    entry.select_region(0, -1);
    content.append(&entry);

    let extension_hint = if let Some(ext) = &ext {
        format!("Extension '.{ext}' will be preserved")
    } else {
        "File has no extension".to_string()
    };
    let hint_label = Label::new(Some(&extension_hint));
    hint_label.add_css_class("caption");
    hint_label.set_halign(gtk4::Align::Start);
    content.append(&hint_label);

    let error_label = Label::new(None);
    error_label.add_css_class("caption");
    error_label.add_css_class("error");
    error_label.set_halign(gtk4::Align::Start);
    content.append(&error_label);

    let button_row = gtk4::Box::new(Orientation::Horizontal, 6);
    button_row.set_halign(gtk4::Align::End);
    let cancel_btn = gtk4::Button::with_label("Cancel");
    let rename_btn = gtk4::Button::with_label("Rename");
    rename_btn.set_sensitive(false);
    button_row.append(&cancel_btn);
    button_row.append(&rename_btn);
    content.append(&button_row);

    let candidate_target: Rc<RefCell<Option<std::path::PathBuf>>> = Rc::new(RefCell::new(None));
    let validate_input: Rc<dyn Fn(&str)> = Rc::new({
        let source_path = source_path.clone();
        let candidate_target = candidate_target.clone();
        let rename_btn = rename_btn.clone();
        let error_label = error_label.clone();
        move |value: &str| match build_renamed_target(&source_path, value) {
            Ok(path) => {
                *candidate_target.borrow_mut() = Some(path);
                error_label.set_text("");
                rename_btn.set_sensitive(true);
            }
            Err(message) => {
                *candidate_target.borrow_mut() = None;
                error_label.set_text(&message);
                rename_btn.set_sensitive(false);
            }
        }
    });
    (validate_input.as_ref())(entry.text().as_str());

    let validate_on_change = validate_input.clone();
    entry.connect_changed(move |e| {
        (validate_on_change.as_ref())(e.text().as_str());
    });

    let start_scan_for_folder = start_scan_for_folder.clone();
    let current_folder = current_folder.clone();
    let toast_overlay = toast_overlay.clone();
    let dialog_for_cancel = dialog.clone();
    cancel_btn.connect_clicked(move |_| {
        dialog_for_cancel.close();
    });

    let dialog_for_rename = dialog.clone();
    let rename_btn_activate = rename_btn.clone();
    entry.connect_activate(move |_| {
        rename_btn_activate.emit_clicked();
    });

    rename_btn.connect_clicked(move |_| {
        if let Some(target) = candidate_target.borrow().clone() {
            match std::fs::rename(&source_path, &target) {
                Ok(()) => {
                    if let Some(folder) = current_folder.borrow().as_ref().cloned() {
                        start_scan_for_folder(folder);
                    }
                    toast_overlay.add_toast(adw::Toast::new("File renamed"));
                }
                Err(err) => {
                    toast_overlay.add_toast(adw::Toast::new(&format!("Rename failed: {}", err)));
                }
            }
        }
        dialog_for_rename.close();
    });

    dialog.present();
}

pub fn open_delete_dialog(
    window: &adw::ApplicationWindow,
    toast_overlay: &adw::ToastOverlay,
    start_scan_for_folder: &Rc<dyn Fn(std::path::PathBuf)>,
    current_folder: &Rc<RefCell<Option<std::path::PathBuf>>>,
    source_path: std::path::PathBuf,
) {
    let file_name = source_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| source_path.to_string_lossy().into_owned());
    let dialog = gtk4::Window::builder()
        .transient_for(window)
        .modal(true)
        .title("Delete file")
        .default_width(420)
        .build();

    let content = gtk4::Box::new(Orientation::Vertical, 10);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    dialog.set_child(Some(&content));

    let prompt = Label::new(Some(&format!("Delete '{}'?", file_name)));
    prompt.set_halign(gtk4::Align::Start);
    content.append(&prompt);

    let hint = Label::new(Some("This cannot be undone."));
    hint.add_css_class("caption");
    hint.set_halign(gtk4::Align::Start);
    content.append(&hint);

    let button_row = gtk4::Box::new(Orientation::Horizontal, 6);
    button_row.set_halign(gtk4::Align::End);
    let cancel_btn = gtk4::Button::with_label("Cancel");
    let delete_btn = gtk4::Button::with_label("Delete");
    delete_btn.add_css_class("destructive-action");
    button_row.append(&cancel_btn);
    button_row.append(&delete_btn);
    content.append(&button_row);

    let dialog_for_cancel = dialog.clone();
    cancel_btn.connect_clicked(move |_| {
        dialog_for_cancel.close();
    });

    let dialog_for_delete = dialog.clone();
    let toast_overlay = toast_overlay.clone();
    let start_scan_for_folder = start_scan_for_folder.clone();
    let current_folder = current_folder.clone();
    delete_btn.connect_clicked(move |_| {
        match std::fs::remove_file(&source_path) {
            Ok(()) => {
                if let Some(folder) = current_folder.borrow().as_ref().cloned() {
                    start_scan_for_folder(folder);
                }
                let toast = adw::Toast::new("File deleted");
                toast.set_timeout(2);
                toast_overlay.add_toast(toast);
            }
            Err(err) => {
                let toast = adw::Toast::new(&format!("Delete failed: {}", err));
                toast.set_timeout(3);
                toast_overlay.add_toast(toast);
            }
        }
        dialog_for_delete.close();
    });

    dialog.present();
}

pub fn open_trash_dialog(
    window: &adw::ApplicationWindow,
    toast_overlay: &adw::ToastOverlay,
    start_scan_for_folder: &Rc<dyn Fn(std::path::PathBuf)>,
    current_folder: &Rc<RefCell<Option<std::path::PathBuf>>>,
    source_path: std::path::PathBuf,
) {
    let file_name = source_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| source_path.to_string_lossy().into_owned());
    let dialog = gtk4::Window::builder()
        .transient_for(window)
        .modal(true)
        .title("Move to Trash")
        .default_width(420)
        .build();

    let content = gtk4::Box::new(Orientation::Vertical, 10);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    dialog.set_child(Some(&content));

    let prompt = Label::new(Some(&format!("Move '{}' to the trash?", file_name)));
    prompt.set_halign(gtk4::Align::Start);
    content.append(&prompt);

    let hint = Label::new(Some("You can restore it from your trash folder."));
    hint.add_css_class("caption");
    hint.set_halign(gtk4::Align::Start);
    content.append(&hint);

    let button_row = gtk4::Box::new(Orientation::Horizontal, 6);
    button_row.set_halign(gtk4::Align::End);
    let cancel_btn = gtk4::Button::with_label("Cancel");
    let trash_btn = gtk4::Button::with_label("Move to Trash");
    trash_btn.add_css_class("destructive-action");
    button_row.append(&cancel_btn);
    button_row.append(&trash_btn);
    content.append(&button_row);

    let dialog_for_cancel = dialog.clone();
    cancel_btn.connect_clicked(move |_| {
        dialog_for_cancel.close();
    });

    let dialog_for_trash = dialog.clone();
    let toast_overlay = toast_overlay.clone();
    let start_scan_for_folder = start_scan_for_folder.clone();
    let current_folder = current_folder.clone();
    trash_btn.connect_clicked(move |_| {
        let file = gio::File::for_path(&source_path);
        match file.trash(gio::Cancellable::NONE) {
            Ok(()) => {
                if let Some(folder) = current_folder.borrow().as_ref().cloned() {
                    start_scan_for_folder(folder);
                }
                let toast = adw::Toast::new(
                    "Moved to trash — Shift+Delete skips trash and deletes permanently",
                );
                toast.set_timeout(3);
                toast_overlay.add_toast(toast);
            }
            Err(err) => {
                let toast = adw::Toast::new(&format!("Could not move to trash: {}", err));
                toast.set_timeout(3);
                toast_overlay.add_toast(toast);
            }
        }
        dialog_for_trash.close();
    });

    dialog.present();
}
