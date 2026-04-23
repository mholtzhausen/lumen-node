use gtk4::prelude::*;
use gtk4::{gdk, gio, GestureClick, PopoverMenu, SingleSelection, StringObject};
use std::path::PathBuf;

pub fn attach_context_menu<W: IsA<gtk4::Widget>>(widget: &W, menu_model: &gio::Menu) {
    let widget_obj = widget.as_ref().clone();
    let menu_model = menu_model.clone();
    let click = GestureClick::new();
    click.set_button(3);
    click.connect_pressed(move |_, _, x, y| {
        let pop = PopoverMenu::from_model(Some(&menu_model));
        pop.set_parent(&widget_obj);
        pop.set_has_arrow(true);
        pop.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        pop.popup();
    });
    widget.add_controller(click);
}

pub fn selected_image_path(selection: &SingleSelection) -> Option<PathBuf> {
    selection
        .selected_item()
        .and_downcast::<StringObject>()
        .map(|s| PathBuf::from(s.string().as_str()))
}
