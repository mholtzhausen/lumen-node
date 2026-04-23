use crate::json_tree::build_json_metadata_widget;
use crate::metadata::ImageMetadata;
use gtk4::prelude::*;
use gtk4::{glib, Align};
use libadwaita as adw;
use libadwaita::prelude::*;

pub fn create_right_sidebar(initial_visible: bool) -> gtk4::Box {
    let right_sidebar = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    right_sidebar.set_width_request(260);
    right_sidebar.set_visible(initial_visible);
    right_sidebar.set_margin_top(0);
    right_sidebar.set_margin_bottom(0);
    right_sidebar.set_margin_start(0);
    right_sidebar.set_margin_end(0);
    right_sidebar
}

pub fn create_meta_preview_picture() -> gtk4::Picture {
    let meta_preview = gtk4::Picture::new();
    meta_preview.set_vexpand(true);
    meta_preview.set_hexpand(true);
    meta_preview.set_can_shrink(true);
    meta_preview
}

pub fn create_meta_content_container() -> gtk4::Box {
    let meta_content = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    meta_content.set_vexpand(true);
    meta_content.set_margin_top(12);
    meta_content.set_margin_bottom(12);
    meta_content.set_margin_start(4);
    meta_content.set_margin_end(8);
    meta_content
}

pub fn create_meta_scroll_list() -> (gtk4::ScrolledWindow, gtk4::ListBox) {
    let meta_scroll = gtk4::ScrolledWindow::new();
    meta_scroll.set_vexpand(true);
    meta_scroll.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);

    let meta_listbox = gtk4::ListBox::new();
    meta_listbox.add_css_class("boxed-list");
    meta_listbox.set_selection_mode(gtk4::SelectionMode::None);
    meta_scroll.set_child(Some(&meta_listbox));

    (meta_scroll, meta_listbox)
}

pub fn create_meta_expander(meta_scroll: &gtk4::ScrolledWindow) -> gtk4::Expander {
    let meta_expander = gtk4::Expander::new(Some("Metadata"));
    meta_expander.set_expanded(true);
    meta_expander.set_child(Some(meta_scroll));
    meta_expander
}

pub fn populate_metadata_sidebar(listbox: &gtk4::ListBox, meta: &ImageMetadata) {
    while let Some(child) = listbox.first_child() {
        listbox.remove(&child);
    }

    let short_rows: &[(&str, Option<&str>)] = &[
        ("Make", meta.camera_make.as_deref()),
        ("Model", meta.camera_model.as_deref()),
        ("Exposure", meta.exposure.as_deref()),
        ("ISO", meta.iso.as_deref()),
    ];

    for (key, maybe_val) in short_rows {
        let Some(val) = maybe_val else { continue };
        let display_val = val.to_string();
        let row = adw::ActionRow::new();
        row.set_title(key);
        row.set_subtitle(&glib::markup_escape_text(&display_val));
        row.set_subtitle_selectable(true);
        let copy_text = display_val.clone();
        let copy_button = gtk4::Button::from_icon_name("edit-copy-symbolic");
        copy_button.add_css_class("flat");
        copy_button.set_tooltip_text(Some("Copy"));
        copy_button.connect_clicked(move |btn| {
            gtk4::prelude::WidgetExt::display(btn).clipboard().set_text(&copy_text);
        });
        row.add_suffix(&copy_button);
        listbox.append(&row);
    }

    let long_rows: &[(&str, Option<&str>)] = &[
        ("Prompt", meta.prompt.as_deref()),
        ("Neg. Prompt", meta.negative_prompt.as_deref()),
        ("Parameters", meta.raw_parameters.as_deref()),
        ("Workflow", meta.workflow_json.as_deref()),
    ];

    for (key, maybe_val) in long_rows {
        let Some(val) = maybe_val else { continue };
        let display_val = val.to_string();

        let row_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
        row_box.set_margin_top(8);
        row_box.set_margin_bottom(4);
        row_box.set_margin_start(12);
        row_box.set_margin_end(8);

        let header_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        header_box.set_hexpand(true);

        let key_label = gtk4::Label::new(Some(key));
        key_label.add_css_class("caption-heading");
        key_label.set_halign(Align::Start);
        key_label.set_hexpand(true);
        header_box.append(&key_label);

        let copy_text = display_val.clone();
        let copy_button = gtk4::Button::from_icon_name("edit-copy-symbolic");
        copy_button.add_css_class("flat");
        copy_button.set_tooltip_text(Some("Copy"));
        copy_button.connect_clicked(move |btn| {
            gtk4::prelude::WidgetExt::display(btn).clipboard().set_text(&copy_text);
        });
        header_box.append(&copy_button);
        row_box.append(&header_box);

        if let Some(json_widget) = build_json_metadata_widget(&display_val) {
            row_box.append(&json_widget);
        } else {
            let text_view = gtk4::TextView::new();
            text_view.set_editable(false);
            text_view.set_cursor_visible(false);
            text_view.set_wrap_mode(gtk4::WrapMode::WordChar);
            text_view.set_hexpand(true);
            text_view.add_css_class("caption");
            text_view.add_css_class("metadata-text-view");
            text_view.buffer().set_text(&display_val);
            row_box.append(&text_view);
        }

        let list_row = gtk4::ListBoxRow::new();
        list_row.set_child(Some(&row_box));
        list_row.set_activatable(false);
        list_row.set_selectable(false);
        listbox.append(&list_row);
    }

    if listbox.first_child().is_none() {
        let empty = adw::ActionRow::new();
        empty.set_title("No metadata found");
        listbox.append(&empty);
    }
}
