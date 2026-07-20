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

/// Preview picture wrapped in an overlay host for the zoom-level HUD.
pub fn create_meta_preview() -> (gtk4::Overlay, gtk4::Picture) {
    let meta_preview = create_meta_preview_picture();
    let overlay = gtk4::Overlay::new();
    overlay.set_vexpand(true);
    overlay.set_hexpand(true);
    overlay.set_child(Some(&meta_preview));
    crate::ui::zoom::install_picture_zoom(&meta_preview, &overlay);
    (overlay, meta_preview)
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

/// Favourite + Similar controls on the Metadata expander header (right side).
#[derive(Clone)]
pub struct PreviewFavouriteIndicator {
    pub button: gtk4::Button,
    pub similar_button: gtk4::Button,
}

pub fn create_meta_expander(
    meta_scroll: &gtk4::ScrolledWindow,
    initially_expanded: bool,
) -> (gtk4::Expander, PreviewFavouriteIndicator) {
    let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    header.set_hexpand(true);

    let title = gtk4::Label::new(Some("Metadata"));
    title.set_halign(Align::Start);
    title.set_hexpand(true);
    header.append(&title);

    let actions = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    actions.set_halign(Align::End);

    let button = gtk4::Button::from_icon_name(crate::icons::NON_STARRED);
    button.add_css_class("flat");
    button.add_css_class("circular");
    button.add_css_class("thumbnail-favourite-button");
    button.set_tooltip_text(Some("Toggle favourite"));
    button.set_focus_on_click(false);
    button.set_sensitive(false);

    let similar_button = gtk4::Button::from_icon_name(crate::icons::SIMILAR);
    similar_button.add_css_class("flat");
    similar_button.add_css_class("circular");
    similar_button.add_css_class("thumbnail-favourite-button");
    similar_button.set_tooltip_text(Some("Similar in folder"));
    similar_button.set_focus_on_click(false);
    similar_button.set_visible(false);

    actions.append(&button);
    actions.append(&similar_button);
    header.append(&actions);

    let meta_expander = gtk4::Expander::new(None);
    meta_expander.set_label_widget(Some(&header));
    meta_expander.set_expanded(initially_expanded);
    meta_expander.set_child(Some(meta_scroll));

    (
        meta_expander,
        PreviewFavouriteIndicator {
            button,
            similar_button,
        },
    )
}

pub fn update_preview_favourite_indicator(
    indicator: &PreviewFavouriteIndicator,
    is_favourite: Option<bool>,
) {
    match is_favourite {
        Some(true) => {
            indicator.button.set_sensitive(true);
            indicator.button.set_icon_name(crate::icons::STARRED);
            indicator.button.add_css_class("thumbnail-favourite-active");
        }
        Some(false) => {
            indicator.button.set_sensitive(true);
            indicator.button.set_icon_name(crate::icons::NON_STARRED);
            indicator.button.remove_css_class("thumbnail-favourite-active");
        }
        None => {
            indicator.button.set_sensitive(false);
            indicator.button.set_icon_name(crate::icons::NON_STARRED);
            indicator.button.remove_css_class("thumbnail-favourite-active");
            indicator.similar_button.set_visible(false);
        }
    }
}

pub fn update_preview_similar_button(indicator: &PreviewFavouriteIndicator, visible: bool) {
    indicator.similar_button.set_visible(visible);
}

pub fn set_preview_similar_active(indicator: &PreviewFavouriteIndicator, active: bool) {
    if active {
        indicator
            .similar_button
            .add_css_class("similar-filter-active");
        indicator
            .similar_button
            .add_css_class("thumbnail-favourite-active");
        indicator
            .similar_button
            .set_tooltip_text(Some("Clear similar filter"));
    } else {
        indicator
            .similar_button
            .remove_css_class("similar-filter-active");
        indicator
            .similar_button
            .remove_css_class("thumbnail-favourite-active");
        indicator
            .similar_button
            .set_tooltip_text(Some("Similar in folder"));
    }
}

pub fn create_meta_split_before_auto_collapse() -> std::rc::Rc<std::cell::Cell<Option<i32>>> {
    std::rc::Rc::new(std::cell::Cell::new(None))
}

pub fn create_meta_position_programmatic() -> std::rc::Rc<std::cell::Cell<u32>> {
    std::rc::Rc::new(std::cell::Cell::new(0_u32))
}

pub fn create_meta_split_dirty_flag() -> std::rc::Rc<std::cell::Cell<bool>> {
    std::rc::Rc::new(std::cell::Cell::new(false))
}

pub fn create_pane_restore_complete_flag() -> std::rc::Rc<std::cell::Cell<bool>> {
    std::rc::Rc::new(std::cell::Cell::new(false))
}

pub fn create_meta_paned(
    meta_preview: &impl IsA<gtk4::Widget>,
    meta_content: &gtk4::Box,
) -> gtk4::Paned {
    let meta_paned = gtk4::Paned::new(gtk4::Orientation::Vertical);
    meta_paned.set_vexpand(true);
    meta_paned.set_start_child(Some(meta_preview));
    meta_paned.set_end_child(Some(meta_content));
    meta_paned.set_resize_start_child(true);
    meta_paned.set_resize_end_child(true);
    meta_paned.set_shrink_start_child(false);
    meta_paned.set_shrink_end_child(false);
    meta_paned
}

pub fn initialize_meta_paned_position(
    meta_paned: &gtk4::Paned,
    meta_position_programmatic: &std::rc::Rc<std::cell::Cell<u32>>,
    meta_pane_start_px: i32,
) {
    meta_position_programmatic.set(meta_position_programmatic.get().saturating_add(1));
    meta_paned.set_position(meta_pane_start_px);
    meta_position_programmatic.set(meta_position_programmatic.get().saturating_sub(1));
}

pub fn connect_meta_paned_dirty_tracking(
    meta_paned: &gtk4::Paned,
    meta_position_programmatic: &std::rc::Rc<std::cell::Cell<u32>>,
    meta_split_dirty: &std::rc::Rc<std::cell::Cell<bool>>,
    pane_restore_complete: &std::rc::Rc<std::cell::Cell<bool>>,
) {
    let meta_position_programmatic = meta_position_programmatic.clone();
    let meta_split_dirty = meta_split_dirty.clone();
    let pane_restore_complete = pane_restore_complete.clone();
    meta_paned.connect_notify_local(Some("position"), move |_, _| {
        if !pane_restore_complete.get() {
            return;
        }
        if meta_position_programmatic.get() != 0 {
            return;
        }
        meta_split_dirty.set(true);
    });
}

pub fn append_meta_paned_to_sidebar(right_sidebar: &gtk4::Box, meta_paned: &gtk4::Paned) {
    right_sidebar.append(meta_paned);
}

pub fn connect_sidebar_visibility_toggles(
    left_toggle: &gtk4::ToggleButton,
    left_sidebar: &gtk4::Box,
    right_toggle: &gtk4::ToggleButton,
    right_sidebar: &gtk4::Box,
) {
    let left_sidebar_toggle = left_sidebar.clone();
    left_toggle.connect_toggled(move |btn| {
        left_sidebar_toggle.set_visible(btn.is_active());
    });

    let right_sidebar_toggle = right_sidebar.clone();
    right_toggle.connect_toggled(move |btn| {
        right_sidebar_toggle.set_visible(btn.is_active());
    });
}

pub fn clear_metadata_sidebar(listbox: &gtk4::ListBox) {
    while let Some(child) = listbox.first_child() {
        listbox.remove(&child);
    }
}

pub fn populate_metadata_sidebar(
    listbox: &gtk4::ListBox,
    meta: &ImageMetadata,
    preview_favourite: &PreviewFavouriteIndicator,
    similar_filter_active: bool,
) {
    clear_metadata_sidebar(listbox);
    update_preview_similar_button(
        preview_favourite,
        crate::similarity::meta_has_similarity_source(meta),
    );
    set_preview_similar_active(preview_favourite, similar_filter_active);

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
        let copy_button = gtk4::Button::from_icon_name(crate::icons::COPY);
        copy_button.add_css_class("flat");
        copy_button.set_tooltip_text(Some("Copy"));
        copy_button.connect_clicked(move |btn| {
            gtk4::prelude::WidgetExt::display(btn)
                .clipboard()
                .set_text(&copy_text);
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
        let copy_button = gtk4::Button::from_icon_name(crate::icons::COPY);
        copy_button.add_css_class("flat");
        copy_button.set_tooltip_text(Some("Copy"));
        copy_button.connect_clicked(move |btn| {
            gtk4::prelude::WidgetExt::display(btn)
                .clipboard()
                .set_text(&copy_text);
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
