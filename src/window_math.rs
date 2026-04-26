use gtk4::gdk;
use gtk4::prelude::*;
use libadwaita as adw;

const DEFAULT_WINDOW_WIDTH: i32 = 1280;
const DEFAULT_WINDOW_HEIGHT: i32 = 800;

pub fn clamp_f64(value: f64, min: f64, max: f64) -> f64 {
    value.max(min).min(max)
}

pub fn pct_to_px(total: i32, pct: f64) -> i32 {
    let total = total.max(1) as f64;
    ((clamp_f64(pct, 0.0, 100.0) / 100.0) * total).round() as i32
}

pub fn px_to_pct(px: i32, total: i32) -> f64 {
    let total = total.max(1) as f64;
    clamp_f64(((px.max(0) as f64) / total) * 100.0, 0.0, 100.0)
}

pub fn monitor_bounds_for_window(window: &adw::ApplicationWindow) -> (i32, i32) {
    let display = gtk4::prelude::WidgetExt::display(window);
    if let Some(surface) = window.surface() {
        if let Some(monitor) = display.monitor_at_surface(&surface) {
            let geometry = monitor.geometry();
            return (geometry.width().max(1), geometry.height().max(1));
        }
    }

    let monitors = display.monitors();
    for i in 0..monitors.n_items() {
        if let Some(monitor) = monitors.item(i).and_downcast::<gdk::Monitor>() {
            let geometry = monitor.geometry();
            return (geometry.width().max(1), geometry.height().max(1));
        }
    }

    (DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT)
}
