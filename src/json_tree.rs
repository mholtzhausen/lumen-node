use serde_json::Value as JsonValue;
use gtk4::prelude::*;

fn json_copy_text(value: &JsonValue) -> String {
    match value {
        JsonValue::String(v) => v.clone(),
        JsonValue::Bool(v) => v.to_string(),
        JsonValue::Number(v) => v.to_string(),
        JsonValue::Null => "null".to_string(),
        JsonValue::Array(_) | JsonValue::Object(_) => {
            serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
        }
    }
}

fn json_display_value(value: &JsonValue) -> String {
    match value {
        JsonValue::String(v) => format!("\"{}\"", v),
        JsonValue::Bool(v) => v.to_string(),
        JsonValue::Number(v) => v.to_string(),
        JsonValue::Null => "null".to_string(),
        JsonValue::Array(values) => format!("[...] ({} items)", values.len()),
        JsonValue::Object(map) => format!("{{...}} ({} keys)", map.len()),
    }
}

fn add_copy_button_hover(row: &gtk4::Box, copy_button: &gtk4::Button) {
    copy_button.set_opacity(0.0);
    let motion = gtk4::EventControllerMotion::new();
    let copy_button_enter = copy_button.clone();
    motion.connect_enter(move |_, _, _| {
        copy_button_enter.set_opacity(1.0);
    });
    let copy_button_leave = copy_button.clone();
    motion.connect_leave(move |_| {
        copy_button_leave.set_opacity(0.0);
    });
    row.add_controller(motion);
}

fn append_json_node(parent: &gtk4::Box, key: Option<&str>, value: &JsonValue, depth: usize) {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    row.set_margin_start((depth as i32) * 14);
    row.set_hexpand(true);

    match value {
        JsonValue::Object(map) => {
            let title = match key {
                Some(k) => format!("\"{}\": {}", k, json_display_value(value)),
                None => json_display_value(value),
            };
            let expander = gtk4::Expander::new(Some(&title));
            expander.set_expanded(depth == 0);
            expander.set_hexpand(true);

            let children = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
            for (child_key, child_val) in map {
                append_json_node(&children, Some(child_key), child_val, depth + 1);
            }
            expander.set_child(Some(&children));
            row.append(&expander);
        }
        JsonValue::Array(items) => {
            let title = match key {
                Some(k) => format!("\"{}\": {}", k, json_display_value(value)),
                None => json_display_value(value),
            };
            let expander = gtk4::Expander::new(Some(&title));
            expander.set_expanded(depth == 0);
            expander.set_hexpand(true);

            let children = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
            for (idx, child_val) in items.iter().enumerate() {
                let idx_key = format!("[{}]", idx);
                append_json_node(&children, Some(&idx_key), child_val, depth + 1);
            }
            expander.set_child(Some(&children));
            row.append(&expander);
        }
        _ => {
            let text = match key {
                Some(k) => format!("\"{}\": {}", k, json_display_value(value)),
                None => json_display_value(value),
            };
            let label = gtk4::Label::new(Some(&text));
            label.set_halign(gtk4::Align::Start);
            label.set_xalign(0.0);
            label.set_hexpand(true);
            label.set_selectable(true);
            label.add_css_class("monospace");
            row.append(&label);
        }
    }

    let copy_text = json_copy_text(value);
    let copy_button = gtk4::Button::from_icon_name("edit-copy-symbolic");
    copy_button.add_css_class("flat");
    copy_button.add_css_class("circular");
    copy_button.set_tooltip_text(Some("Copy"));
    copy_button.connect_clicked(move |btn| {
        gtk4::prelude::WidgetExt::display(btn)
            .clipboard()
            .set_text(&copy_text);
    });
    add_copy_button_hover(&row, &copy_button);
    row.append(&copy_button);

    parent.append(&row);
}

pub fn build_json_metadata_widget(raw: &str) -> Option<gtk4::ScrolledWindow> {
    let value: JsonValue = serde_json::from_str(raw.trim()).ok()?;
    let tree = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    append_json_node(&tree, None, &value, 0);

    let scroller = gtk4::ScrolledWindow::new();
    scroller.set_policy(gtk4::PolicyType::Automatic, gtk4::PolicyType::Automatic);
    scroller.set_min_content_height(130);
    scroller.set_child(Some(&tree));
    Some(scroller)
}
