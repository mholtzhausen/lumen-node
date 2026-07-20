//! Centered spinner overlay for the thumbnail grid during scan bootstrap
//! and user-initiated filter updates.

use gtk4::prelude::*;
use gtk4::{glib, Align, CustomFilter, FilterChange, Label, Orientation, Spinner};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

/// Shared handles for the grid loading overlay.
#[derive(Clone)]
pub(crate) struct GridLoadingOverlay {
    pub(crate) root: gtk4::Box,
    spinner: Spinner,
    label: Label,
    apply_gen: Rc<Cell<u64>>,
}

pub(crate) fn create_grid_loading_overlay() -> GridLoadingOverlay {
    // Full-bleed root so CSS background covers the grid underneath.
    let root = gtk4::Box::new(Orientation::Vertical, 0);
    root.set_halign(Align::Fill);
    root.set_valign(Align::Fill);
    root.set_hexpand(true);
    root.set_vexpand(true);
    root.set_visible(false);
    root.add_css_class("grid-loading-overlay");

    let content = gtk4::Box::new(Orientation::Vertical, 12);
    content.set_halign(Align::Center);
    content.set_valign(Align::Center);
    content.set_hexpand(true);
    content.set_vexpand(true);

    let spinner = Spinner::new();
    spinner.set_halign(Align::Center);
    spinner.set_size_request(32, 32);

    let label = Label::new(Some("Loading…"));
    label.add_css_class("caption");
    label.set_halign(Align::Center);

    content.append(&spinner);
    content.append(&label);
    root.append(&content);

    GridLoadingOverlay {
        root,
        spinner,
        label,
        apply_gen: Rc::new(Cell::new(0)),
    }
}

pub(crate) fn add_grid_loading_overlay(grid_overlay: &gtk4::Overlay, loading: &GridLoadingOverlay) {
    grid_overlay.add_overlay(&loading.root);
}

impl GridLoadingOverlay {
    pub(crate) fn is_visible(&self) -> bool {
        self.root.is_visible()
    }

    pub(crate) fn show(&self, caption: &str) {
        self.label.set_text(caption);
        self.spinner.start();
        self.root.set_visible(true);
    }

    pub(crate) fn hide(&self) {
        self.root.set_visible(false);
        self.spinner.stop();
    }

    /// Show spinner, yield to the main loop, then apply `filter.changed` and hide.
    /// Overlapping calls only let the latest generation hide the overlay.
    pub(crate) fn apply_filter(&self, filter: &CustomFilter, change: FilterChange, caption: &str) {
        let gen = self.apply_gen.get().wrapping_add(1);
        self.apply_gen.set(gen);
        self.show(caption);

        let filter = filter.clone();
        let overlay = self.clone();
        glib::idle_add_local_once(move || {
            if overlay.apply_gen.get() != gen {
                return;
            }
            filter.changed(change);
            if overlay.apply_gen.get() == gen {
                overlay.hide();
            }
        });
    }

    /// Like [`Self::apply_filter`], then runs `after` once the filter has been applied
    /// (only if this generation is still current).
    pub(crate) fn apply_filter_then(
        &self,
        filter: &CustomFilter,
        change: FilterChange,
        caption: &str,
        after: impl FnOnce() + 'static,
    ) {
        let gen = self.apply_gen.get().wrapping_add(1);
        self.apply_gen.set(gen);
        self.show(caption);

        let filter = filter.clone();
        let overlay = self.clone();
        glib::idle_add_local_once(move || {
            if overlay.apply_gen.get() != gen {
                return;
            }
            filter.changed(change);
            if overlay.apply_gen.get() == gen {
                overlay.hide();
                after();
            }
        });
    }
}

/// Apply a filter change with loading UI when the overlay is available.
pub(crate) fn apply_filter_change(
    grid_loading: &Rc<RefCell<Option<GridLoadingOverlay>>>,
    filter: &CustomFilter,
    change: FilterChange,
    caption: &str,
) {
    if let Some(loading) = grid_loading.borrow().as_ref() {
        loading.apply_filter(filter, change, caption);
    } else {
        filter.changed(change);
    }
}

/// Apply a filter change with loading UI, then run `after` (e.g. toast).
pub(crate) fn apply_filter_change_then(
    grid_loading: &Rc<RefCell<Option<GridLoadingOverlay>>>,
    filter: &CustomFilter,
    change: FilterChange,
    caption: &str,
    after: impl FnOnce() + 'static,
) {
    if let Some(loading) = grid_loading.borrow().as_ref() {
        loading.apply_filter_then(filter, change, caption, after);
    } else {
        filter.changed(change);
        after();
    }
}

pub(crate) fn show_grid_loading(
    grid_loading: &Rc<RefCell<Option<GridLoadingOverlay>>>,
    caption: &str,
) {
    if let Some(loading) = grid_loading.borrow().as_ref() {
        loading.show(caption);
    }
}

pub(crate) fn hide_grid_loading(grid_loading: &Rc<RefCell<Option<GridLoadingOverlay>>>) {
    if let Some(loading) = grid_loading.borrow().as_ref() {
        loading.hide();
    }
}
