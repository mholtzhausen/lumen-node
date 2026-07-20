use gtk4::prelude::*;
use gtk4::{
    gdk, gio, BitsetIter, GestureClick, MultiSelection, PopoverMenu, SelectionModel, StringObject,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::sort_flags::SortFields;

pub fn attach_context_menu_with_prepare<W, F>(widget: &W, menu_model: &gio::Menu, prepare: F)
where
    W: IsA<gtk4::Widget>,
    F: Fn(&gtk4::Widget, f64, f64) + 'static,
{
    let widget_obj = widget.as_ref().clone();
    let menu_model = menu_model.clone();
    let click = GestureClick::new();
    click.set_button(3);
    click.connect_pressed(move |_, _, x, y| {
        prepare(&widget_obj, x, y);
        let pop = PopoverMenu::from_model(Some(&menu_model));
        pop.set_parent(&widget_obj);
        pop.set_has_arrow(true);
        pop.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        pop.popup();
    });
    widget.add_controller(click);
}

/// Like [`attach_context_menu_with_prepare`], but `prepare` chooses which menu to show.
pub fn attach_context_menu_dynamic<W, F>(widget: &W, prepare: F)
where
    W: IsA<gtk4::Widget>,
    F: Fn(&gtk4::Widget, f64, f64) -> gio::Menu + 'static,
{
    let widget_obj = widget.as_ref().clone();
    let click = GestureClick::new();
    click.set_button(3);
    click.connect_pressed(move |_, _, x, y| {
        let menu_model = prepare(&widget_obj, x, y);
        let pop = PopoverMenu::from_model(Some(&menu_model));
        pop.set_parent(&widget_obj);
        pop.set_has_arrow(true);
        pop.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        pop.popup();
    });
    widget.add_controller(click);
}

/// Selected indices in ascending position order.
pub fn selected_indices(selection: &impl IsA<SelectionModel>) -> Vec<u32> {
    let bitset = selection.selection();
    let mut out = Vec::new();
    if let Some((mut iter, first)) = BitsetIter::init_first(&bitset) {
        out.push(first);
        while let Some(next) = iter.next() {
            out.push(next);
        }
    }
    out
}

pub fn selected_count(selection: &impl IsA<SelectionModel>) -> u32 {
    selection.selection().size() as u32
}

/// Selected paths as `PathBuf`s (position order).
#[allow(dead_code)]
pub fn selected_image_paths(selection: &MultiSelection) -> Vec<PathBuf> {
    selected_indices(selection)
        .into_iter()
        .filter_map(|idx| {
            selection
                .item(idx)
                .and_downcast::<StringObject>()
                .map(|s| PathBuf::from(s.string().as_str()))
        })
        .collect()
}

pub fn selected_image_path_strings(selection: &MultiSelection) -> Vec<String> {
    selected_indices(selection)
        .into_iter()
        .filter_map(|idx| {
            selection
                .item(idx)
                .and_downcast::<StringObject>()
                .map(|s| s.string().to_string())
        })
        .collect()
}

/// Primary path for single-image actions: sole selection, else `None` when empty/multi.
/// Prefer [`primary_image_path`] / app_state mirror when a multi primary is needed.
pub fn selected_image_path(selection: &MultiSelection) -> Option<PathBuf> {
    let indices = selected_indices(selection);
    if indices.len() != 1 {
        return None;
    }
    selection
        .item(indices[0])
        .and_downcast::<StringObject>()
        .map(|s| PathBuf::from(s.string().as_str()))
}

/// Highest selected index (primary for nav), or `None` if empty.
pub fn primary_selected_index(selection: &MultiSelection) -> Option<u32> {
    selected_indices(selection).into_iter().next_back()
}

/// Path at the primary selected index, or `None` if empty.
pub fn primary_image_path(selection: &MultiSelection) -> Option<PathBuf> {
    let idx = primary_selected_index(selection)?;
    selection
        .item(idx)
        .and_downcast::<StringObject>()
        .map(|s| PathBuf::from(s.string().as_str()))
}

pub fn path_at_index(selection: &MultiSelection, idx: u32) -> Option<String> {
    selection
        .item(idx)
        .and_downcast::<StringObject>()
        .map(|s| s.string().to_string())
}

pub fn find_path_index(selection: &MultiSelection, path: &str) -> Option<u32> {
    for idx in 0..selection.n_items() {
        let is_match = selection
            .item(idx)
            .and_downcast::<StringObject>()
            .map(|obj| obj.string().as_str() == path)
            .unwrap_or(false);
        if is_match {
            return Some(idx);
        }
    }
    None
}

pub fn is_path_selected(selection: &MultiSelection, path: &str) -> bool {
    find_path_index(selection, path).is_some_and(|idx| selection.is_selected(idx))
}

/// Replace selection with a single index.
pub fn select_only_index(selection: &MultiSelection, position: u32) {
    let _ = selection.select_item(position, true);
}

/// Replace selection with the given path if present.
pub fn select_only_path(selection: &MultiSelection, path: &str) -> bool {
    let Some(idx) = find_path_index(selection, path) else {
        return false;
    };
    select_only_index(selection, idx);
    true
}

pub fn select_all_visible(selection: &MultiSelection) {
    let _ = selection.select_all();
}

pub fn unselect_all(selection: &MultiSelection) {
    let _ = selection.unselect_all();
}

/// Cancel batch: keep at most one path (preferred if still visible), else clear.
pub fn cancel_batch_selection(selection: &MultiSelection, preferred: Option<&str>) {
    if let Some(path) = preferred {
        if select_only_path(selection, path) {
            return;
        }
    }
    unselect_all(selection);
}

/// Collapse multi-selection to the last path in `ordered_paths` that is still selected
/// (Esc behavior). Falls back to last selected index if none match.
pub fn collapse_selection_to_last_ordered(selection: &MultiSelection, ordered_paths: &[String]) {
    if selected_count(selection) <= 1 {
        return;
    }
    let selected: std::collections::HashSet<String> =
        selected_image_path_strings(selection).into_iter().collect();
    if let Some(last) = ordered_paths.iter().rev().find(|p| selected.contains(p.as_str())) {
        let _ = select_only_path(selection, last);
        return;
    }
    if let Some(path) = primary_image_path(selection) {
        let _ = select_only_path(selection, &path.to_string_lossy());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchListSortKey {
    NameAsc,
    NameDesc,
    DateAsc,
    DateDesc,
    SizeAsc,
    SizeDesc,
}

impl BatchListSortKey {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NameAsc => "name_asc",
            Self::NameDesc => "name_desc",
            Self::DateAsc => "date_asc",
            Self::DateDesc => "date_desc",
            Self::SizeAsc => "size_asc",
            Self::SizeDesc => "size_desc",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "name_desc" => Self::NameDesc,
            "date_asc" => Self::DateAsc,
            "date_desc" => Self::DateDesc,
            "size_asc" => Self::SizeAsc,
            "size_desc" => Self::SizeDesc,
            _ => Self::NameAsc,
        }
    }
}

/// Canonical batch-list order for Esc target and rename indices.
pub fn order_batch_paths(
    paths: &[String],
    sort_fields: &HashMap<String, SortFields>,
    key: BatchListSortKey,
) -> Vec<String> {
    let mut ordered = paths.to_vec();
    ordered.sort_by(|a, b| {
        let fallback_a;
        let fallback_b;
        let fields_a = if let Some(f) = sort_fields.get(a) {
            f
        } else {
            fallback_a = crate::sort_flags::compute_sort_fields(a);
            &fallback_a
        };
        let fields_b = if let Some(f) = sort_fields.get(b) {
            f
        } else {
            fallback_b = crate::sort_flags::compute_sort_fields(b);
            &fallback_b
        };
        let ord = match key {
            BatchListSortKey::NameAsc | BatchListSortKey::NameDesc => fields_a
                .filename_lower
                .cmp(&fields_b.filename_lower)
                .then_with(|| a.cmp(b)),
            BatchListSortKey::DateAsc | BatchListSortKey::DateDesc => fields_a
                .modified
                .cmp(&fields_b.modified)
                .then_with(|| fields_a.filename_lower.cmp(&fields_b.filename_lower))
                .then_with(|| a.cmp(b)),
            BatchListSortKey::SizeAsc | BatchListSortKey::SizeDesc => fields_a
                .size
                .cmp(&fields_b.size)
                .then_with(|| fields_a.filename_lower.cmp(&fields_b.filename_lower))
                .then_with(|| a.cmp(b)),
        };
        match key {
            BatchListSortKey::NameDesc | BatchListSortKey::DateDesc | BatchListSortKey::SizeDesc => {
                ord.reverse()
            }
            _ => ord,
        }
    });
    ordered
}

pub fn filename_of(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}
