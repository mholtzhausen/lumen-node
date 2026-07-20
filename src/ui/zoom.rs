//! Display-only zoom for preview / single-view `GtkPicture` widgets.
//!
//! Scale `1.0` means fit-to-display (no transform). Zoom is applied via a
//! per-picture CSS `transform: scale(...)` so the decoded texture stays
//! unchanged. A fade-out percentage HUD mirrors `FullViewFavouriteHud`.

use gtk4::prelude::*;
use gtk4::{glib, CssProvider, Label, Overlay, Picture, STYLE_PROVIDER_PRIORITY_APPLICATION};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

const FIT_SCALE: f64 = 1.0;
const ZOOM_MIN: f64 = 0.25;
const ZOOM_MAX: f64 = 8.0;
const ZOOM_FACTOR: f64 = 1.25;
const INDICATOR_HOLD_MS: u64 = 1200;
const DATA_STATE: &str = "lumen-zoom-state";

struct ZoomState {
    scale: Cell<f64>,
    css: CssProvider,
    indicator: Label,
    fade_gen: Cell<u64>,
}

/// Attach zoom state + CSS provider to `picture`, and a fade-out level label on `overlay`.
pub fn install_picture_zoom(picture: &Picture, overlay: &Overlay) {
    picture.add_css_class("zoomable-picture");
    overlay.add_css_class("zoom-picture-host");
    // Clip scaled paintables to the host; GTK CSS has no `overflow` property.
    overlay.set_overflow(gtk4::Overflow::Hidden);

    let css = CssProvider::new();
    // Per-widget provider is the practical hook for dynamic transform CSS.
    // StyleContext APIs are deprecated since 4.10 but remain functional here.
    #[allow(deprecated)]
    {
        picture
            .style_context()
            .add_provider(&css, STYLE_PROVIDER_PRIORITY_APPLICATION);
    }

    let indicator = Label::new(None);
    indicator.add_css_class("title-4");
    indicator.add_css_class("zoom-level-hud");
    indicator.set_halign(gtk4::Align::Center);
    indicator.set_valign(gtk4::Align::End);
    indicator.set_margin_bottom(24);
    indicator.set_opacity(0.0);
    indicator.set_visible(false);
    indicator.set_can_target(false);
    overlay.add_overlay(&indicator);

    let state = Rc::new(ZoomState {
        scale: Cell::new(FIT_SCALE),
        css,
        indicator,
        fade_gen: Cell::new(0),
    });
    unsafe {
        picture.set_data(DATA_STATE, state.clone());
    }
    apply_transform(&state, FIT_SCALE);
}

pub fn zoom_in(picture: &Picture) {
    adjust_zoom(picture, 1);
}

pub fn zoom_out(picture: &Picture) {
    adjust_zoom(picture, -1);
}

pub fn zoom_reset(picture: &Picture) {
    set_zoom(picture, FIT_SCALE, true);
}

/// Reset to fit without showing the HUD (image change / clear).
pub fn zoom_reset_silent(picture: &Picture) {
    set_zoom(picture, FIT_SCALE, false);
}

pub fn adjust_zoom(picture: &Picture, steps: i32) {
    if steps == 0 {
        return;
    }
    let Some(state) = zoom_state(picture) else {
        return;
    };
    let mut scale = state.scale.get();
    for _ in 0..steps.abs() {
        if steps > 0 {
            scale = (scale * ZOOM_FACTOR).min(ZOOM_MAX);
        } else {
            scale = (scale / ZOOM_FACTOR).max(ZOOM_MIN);
        }
    }
    set_zoom_state(&state, scale, true);
}

fn set_zoom(picture: &Picture, scale: f64, show_hud: bool) {
    let Some(state) = zoom_state(picture) else {
        return;
    };
    set_zoom_state(&state, scale, show_hud);
}

fn set_zoom_state(state: &Rc<ZoomState>, scale: f64, show_hud: bool) {
    let scale = if (scale - FIT_SCALE).abs() < 0.001 {
        FIT_SCALE
    } else {
        scale.clamp(ZOOM_MIN, ZOOM_MAX)
    };
    state.scale.set(scale);
    apply_transform(state, scale);
    if show_hud {
        show_zoom_indicator(state, scale);
    } else {
        hide_zoom_indicator_immediate(state);
    }
}

fn zoom_state(picture: &Picture) -> Option<Rc<ZoomState>> {
    unsafe {
        picture
            .data::<Rc<ZoomState>>(DATA_STATE)
            .map(|p| p.as_ref().clone())
    }
}

fn apply_transform(state: &ZoomState, scale: f64) {
    if (scale - FIT_SCALE).abs() < 0.001 {
        state
            .css
            .load_from_string(".zoomable-picture { transform: none; }");
    } else {
        state.css.load_from_string(&format!(
            ".zoomable-picture {{ transform: scale({scale}); transform-origin: center center; }}"
        ));
    }
}

fn show_zoom_indicator(state: &Rc<ZoomState>, scale: f64) {
    let text = if (scale - FIT_SCALE).abs() < 0.001 {
        "Fit".to_string()
    } else {
        format!("{}%", (scale * 100.0).round() as i32)
    };
    state.indicator.set_text(&text);
    state.indicator.set_visible(true);
    state.indicator.set_opacity(1.0);

    let gen = state.fade_gen.get().saturating_add(1);
    state.fade_gen.set(gen);
    let state = state.clone();
    glib::timeout_add_local_once(Duration::from_millis(INDICATOR_HOLD_MS), move || {
        if state.fade_gen.get() != gen {
            return;
        }
        fade_zoom_indicator(&state, gen, 1.0);
    });
}

fn hide_zoom_indicator_immediate(state: &ZoomState) {
    state.fade_gen.set(state.fade_gen.get().saturating_add(1));
    state.indicator.set_opacity(0.0);
    state.indicator.set_visible(false);
}

fn fade_zoom_indicator(state: &Rc<ZoomState>, gen: u64, opacity: f64) {
    if state.fade_gen.get() != gen {
        return;
    }
    const STEP: f64 = 0.12;
    const INTERVAL_MS: u64 = 30;
    let next = (opacity - STEP).max(0.0);
    state.indicator.set_opacity(next);
    if next <= 0.0 {
        state.indicator.set_visible(false);
        return;
    }
    let state = state.clone();
    glib::timeout_add_local_once(Duration::from_millis(INTERVAL_MS), move || {
        fade_zoom_indicator(&state, gen, next);
    });
}
