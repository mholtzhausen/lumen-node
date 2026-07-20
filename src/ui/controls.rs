use crate::core::app_state::AppState;
use crate::config;
use crate::db::{self, TagFilterMode};
use crate::similarity::{
    find_similar_paths, PromptIndexEntry, SIMILAR_MIN_SCORE,
};
use crate::sort::sort_key_for_index;
use crate::ui::grid::apply_thumbnail_size_change;
use crate::ui::grid_loading::{
    apply_filter_change, apply_filter_change_then, GridLoadingOverlay,
};
use gtk4::prelude::*;
use gtk4::gio::ListStore;
use gtk4::{glib, CustomFilter, CustomSorter, EventControllerMotion, Orientation, MultiSelection};
use libadwaita as adw;
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    time::Duration,
};

const TAG_FILTER_DEBOUNCE_MS: u64 = 200;
const SIMILAR_TOP_N_DEBOUNCE_MS: u64 = 200;
const SIMILAR_SLIDER_LEAVE_MS: u64 = 120;

pub(crate) fn sync_tags_filter_button_style(
    tags_filter_btn: &gtk4::MenuButton,
    active_tag_filters: &Rc<RefCell<HashMap<String, TagFilterMode>>>,
) {
    if active_tag_filters.borrow().is_empty() {
        tags_filter_btn.remove_css_class("tags-filter-active");
    } else {
        tags_filter_btn.add_css_class("tags-filter-active");
    }
}

fn tag_filter_mode_icon(mode: Option<TagFilterMode>) -> &'static str {
    match mode {
        Some(TagFilterMode::Require) => crate::icons::SELECT,
        Some(TagFilterMode::Exclude) => crate::icons::CLOSE,
        None => crate::icons::CHECKBOX,
    }
}

fn tag_filter_mode_tooltip(mode: Option<TagFilterMode>) -> &'static str {
    match mode {
        Some(TagFilterMode::Require) => "Must have this tag (click to exclude)",
        Some(TagFilterMode::Exclude) => "Must not have this tag (click to clear)",
        None => "Ignore this tag (click to require)",
    }
}

fn persist_tag_filters(
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    filters: &HashMap<String, TagFilterMode>,
) {
    if let Some(folder) = current_folder.borrow().as_ref() {
        let _ = db::set_ui_state_value(
            folder.as_path(),
            "active_tags",
            &db::encode_active_tag_filters(filters),
        );
    }
}

/// Schedule a debounced tag-filter apply. Newer schedules cancel pending ones.
fn schedule_tag_filter_apply(
    debounce_gen: &Rc<Cell<u64>>,
    active_tag_filters: &Rc<RefCell<HashMap<String, TagFilterMode>>>,
    filter: &CustomFilter,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    grid_loading: &Rc<RefCell<Option<GridLoadingOverlay>>>,
) {
    let gen = debounce_gen.get().wrapping_add(1);
    debounce_gen.set(gen);
    let debounce_gen = debounce_gen.clone();
    let active_tag_filters = active_tag_filters.clone();
    let filter = filter.clone();
    let current_folder = current_folder.clone();
    let grid_loading = grid_loading.clone();
    glib::timeout_add_local_once(Duration::from_millis(TAG_FILTER_DEBOUNCE_MS), move || {
        if debounce_gen.get() != gen {
            return;
        }
        persist_tag_filters(&current_folder, &active_tag_filters.borrow());
        apply_filter_change(
            &grid_loading,
            &filter,
            gtk4::FilterChange::Different,
            "Updating filters…",
        );
    });
}

pub(crate) fn set_similar_filter_chrome(similar_filter_btn: &gtk4::Button, active: bool) {
    if active {
        similar_filter_btn.add_css_class("similar-filter-active");
    } else {
        similar_filter_btn.remove_css_class("similar-filter-active");
    }
}

pub(crate) fn clear_similar_filter(
    similar_paths: &Rc<RefCell<Option<HashSet<String>>>>,
    similar_query_path: &Rc<RefCell<Option<String>>>,
    filter: &CustomFilter,
    similar_filter_btn: &gtk4::Button,
    grid_loading: &Rc<RefCell<Option<GridLoadingOverlay>>>,
) {
    *similar_paths.borrow_mut() = None;
    *similar_query_path.borrow_mut() = None;
    set_similar_filter_chrome(similar_filter_btn, false);
    apply_filter_change(
        grid_loading,
        filter,
        gtk4::FilterChange::Different,
        "Updating filters…",
    );
}

/// Apply similar-in-folder filter for `query_path` using the current top-N.
/// Returns the match count (including the query) when successful.
pub(crate) fn apply_similar_filter_for_query(
    index: &HashMap<String, PromptIndexEntry>,
    query_path: &str,
    top_n: usize,
    similar_paths: &Rc<RefCell<Option<HashSet<String>>>>,
    similar_query_path: &Rc<RefCell<Option<String>>>,
    similar_filter_btn: &gtk4::Button,
    filter: &CustomFilter,
    grid_loading: &Rc<RefCell<Option<GridLoadingOverlay>>>,
) -> Option<usize> {
    let matches = find_similar_paths(index, query_path, top_n, SIMILAR_MIN_SCORE)?;
    let count = matches.len();
    *similar_paths.borrow_mut() = Some(matches);
    *similar_query_path.borrow_mut() = Some(query_path.to_string());
    set_similar_filter_chrome(similar_filter_btn, true);
    apply_filter_change(
        grid_loading,
        filter,
        gtk4::FilterChange::Different,
        "Updating filters…",
    );
    Some(count)
}

fn schedule_similar_top_n_apply(
    debounce_gen: &Rc<Cell<u64>>,
    similar_top_n: &Rc<Cell<usize>>,
    prompt_index: &Rc<RefCell<HashMap<String, PromptIndexEntry>>>,
    similar_paths: &Rc<RefCell<Option<HashSet<String>>>>,
    similar_query_path: &Rc<RefCell<Option<String>>>,
    similar_filter_btn: &gtk4::Button,
    filter: &CustomFilter,
    grid_loading: &Rc<RefCell<Option<GridLoadingOverlay>>>,
) {
    let gen = debounce_gen.get().wrapping_add(1);
    debounce_gen.set(gen);
    let debounce_gen = debounce_gen.clone();
    let similar_top_n = similar_top_n.clone();
    let prompt_index = prompt_index.clone();
    let similar_paths = similar_paths.clone();
    let similar_query_path = similar_query_path.clone();
    let similar_filter_btn = similar_filter_btn.clone();
    let filter = filter.clone();
    let grid_loading = grid_loading.clone();
    glib::timeout_add_local_once(Duration::from_millis(SIMILAR_TOP_N_DEBOUNCE_MS), move || {
        if debounce_gen.get() != gen {
            return;
        }
        let n = config::normalize_similar_top_n(similar_top_n.get() as i32);
        similar_top_n.set(n as usize);
        config::save_similar_top_n(n);

        let Some(query) = similar_query_path.borrow().clone() else {
            return;
        };
        if similar_paths.borrow().is_none() {
            return;
        }
        let _ = apply_similar_filter_for_query(
            &prompt_index.borrow(),
            &query,
            n as usize,
            &similar_paths,
            &similar_query_path,
            &similar_filter_btn,
            &filter,
            &grid_loading,
        );
    });
}

/// Hover popover with top-N slider under the similar-clear button.
pub(crate) fn install_similar_top_n_hover_slider(
    similar_filter_btn: &gtk4::Button,
    similar_top_n: &Rc<Cell<usize>>,
    similar_top_n_debounce_gen: &Rc<Cell<u64>>,
    prompt_similarity_index: &Rc<RefCell<HashMap<String, PromptIndexEntry>>>,
    similar_paths: &Rc<RefCell<Option<HashSet<String>>>>,
    similar_query_path: &Rc<RefCell<Option<String>>>,
    filter: &CustomFilter,
    grid_loading: &Rc<RefCell<Option<GridLoadingOverlay>>>,
) {
    let initial = config::normalize_similar_top_n(similar_top_n.get() as i32) as f64;
    let adjustment = gtk4::Adjustment::new(initial, 10.0, 100.0, 1.0, 5.0, 0.0);
    let scale = gtk4::Scale::new(Orientation::Horizontal, Some(&adjustment));
    scale.set_draw_value(true);
    scale.set_value_pos(gtk4::PositionType::Right);
    scale.set_digits(0);
    scale.set_hexpand(true);
    scale.set_width_request(160);
    scale.add_mark(10.0, gtk4::PositionType::Bottom, Some("10"));
    scale.add_mark(100.0, gtk4::PositionType::Bottom, Some("100"));

    let label = gtk4::Label::new(Some("Similar count"));
    label.add_css_class("caption");
    label.set_halign(gtk4::Align::Start);

    let content = gtk4::Box::new(Orientation::Vertical, 6);
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(10);
    content.set_margin_end(10);
    content.append(&label);
    content.append(&scale);

    let popover = gtk4::Popover::new();
    popover.set_parent(similar_filter_btn);
    popover.set_child(Some(&content));
    popover.set_position(gtk4::PositionType::Bottom);
    popover.set_autohide(false);

    let pointer_inside = Rc::new(Cell::new(false));

    {
        let pointer_inside_enter = pointer_inside.clone();
        let popover_enter = popover.clone();
        let motion = EventControllerMotion::new();
        motion.connect_enter(move |_, _, _| {
            pointer_inside_enter.set(true);
            popover_enter.popup();
        });
        let pointer_inside_leave = pointer_inside.clone();
        let popover_leave = popover.clone();
        motion.connect_leave(move |_| {
            pointer_inside_leave.set(false);
            let popover = popover_leave.clone();
            let inside = pointer_inside_leave.clone();
            glib::timeout_add_local_once(Duration::from_millis(SIMILAR_SLIDER_LEAVE_MS), move || {
                if !inside.get() && popover.is_visible() {
                    popover.popdown();
                }
            });
        });
        similar_filter_btn.add_controller(motion);
    }

    {
        let pointer_inside_enter = pointer_inside.clone();
        let popover_leave = popover.clone();
        let motion = EventControllerMotion::new();
        motion.connect_enter(move |_, _, _| {
            pointer_inside_enter.set(true);
        });
        let pointer_inside_leave = pointer_inside.clone();
        motion.connect_leave(move |_| {
            pointer_inside_leave.set(false);
            let popover = popover_leave.clone();
            let inside = pointer_inside_leave.clone();
            glib::timeout_add_local_once(Duration::from_millis(SIMILAR_SLIDER_LEAVE_MS), move || {
                if !inside.get() && popover.is_visible() {
                    popover.popdown();
                }
            });
        });
        content.add_controller(motion);
    }

    let similar_top_n = similar_top_n.clone();
    let debounce_gen = similar_top_n_debounce_gen.clone();
    let prompt_index = prompt_similarity_index.clone();
    let similar_paths = similar_paths.clone();
    let similar_query_path = similar_query_path.clone();
    let similar_filter_btn = similar_filter_btn.clone();
    let filter = filter.clone();
    let grid_loading = grid_loading.clone();
    scale.connect_value_changed(move |s| {
        let n = config::normalize_similar_top_n(s.value().round() as i32) as usize;
        similar_top_n.set(n);
        schedule_similar_top_n_apply(
            &debounce_gen,
            &similar_top_n,
            &prompt_index,
            &similar_paths,
            &similar_query_path,
            &similar_filter_btn,
            &filter,
            &grid_loading,
        );
    });
}

/// Rebuilds the tag-filter popover with three-state polarity controls.
/// Clicks update state immediately and schedule a 200ms debounced apply.
pub(crate) fn rebuild_tag_filter_list(
    tags_filter_list: &gtk4::Box,
    tags_filter_btn: &gtk4::MenuButton,
    active_tag_filters: &Rc<RefCell<HashMap<String, TagFilterMode>>>,
    tag_filter_debounce_gen: &Rc<Cell<u64>>,
    filter: &CustomFilter,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    known_tags: &[String],
    grid_loading: &Rc<RefCell<Option<GridLoadingOverlay>>>,
) {
    while let Some(child) = tags_filter_list.first_child() {
        tags_filter_list.remove(&child);
    }

    let heading = gtk4::Label::new(Some("Filter by tags"));
    heading.add_css_class("caption-heading");
    heading.set_halign(gtk4::Align::Start);
    tags_filter_list.append(&heading);

    let legend = gtk4::Label::new(Some(
        "✓ require · empty ignore · ✕ exclude",
    ));
    legend.add_css_class("caption");
    legend.set_halign(gtk4::Align::Start);
    tags_filter_list.append(&legend);

    if known_tags.is_empty() {
        let empty = gtk4::Label::new(Some("No tags in this folder yet."));
        empty.add_css_class("caption");
        empty.set_halign(gtk4::Align::Start);
        tags_filter_list.append(&empty);
        sync_tags_filter_button_style(tags_filter_btn, active_tag_filters);
        return;
    }

    let active_snapshot = active_tag_filters.borrow().clone();
    for tag in known_tags {
        let mode = active_snapshot.get(tag).copied();
        let row = gtk4::Box::new(Orientation::Horizontal, 6);
        row.set_halign(gtk4::Align::Fill);

        let mode_btn = gtk4::Button::from_icon_name(tag_filter_mode_icon(mode));
        mode_btn.add_css_class("flat");
        mode_btn.set_tooltip_text(Some(tag_filter_mode_tooltip(mode)));
        mode_btn.set_focus_on_click(false);

        let label = gtk4::Label::new(Some(tag));
        label.set_halign(gtk4::Align::Start);
        label.set_hexpand(true);

        let tag_owned = tag.clone();
        let filters_cb = active_tag_filters.clone();
        let debounce_cb = tag_filter_debounce_gen.clone();
        let filter_cb = filter.clone();
        let folder_cb = current_folder.clone();
        let btn_cb = tags_filter_btn.clone();
        let grid_loading_cb = grid_loading.clone();
        let mode_btn_ui = mode_btn.clone();
        mode_btn.connect_clicked(move |_| {
            let next = {
                let mut filters = filters_cb.borrow_mut();
                let current = filters.get(&tag_owned).copied();
                let next = TagFilterMode::next_from(current);
                match next {
                    Some(mode) => {
                        filters.insert(tag_owned.clone(), mode);
                    }
                    None => {
                        filters.remove(&tag_owned);
                    }
                }
                next
            };
            mode_btn_ui.set_icon_name(tag_filter_mode_icon(next));
            mode_btn_ui.set_tooltip_text(Some(tag_filter_mode_tooltip(next)));
            sync_tags_filter_button_style(&btn_cb, &filters_cb);
            schedule_tag_filter_apply(
                &debounce_cb,
                &filters_cb,
                &filter_cb,
                &folder_cb,
                &grid_loading_cb,
            );
        });

        row.append(&mode_btn);
        row.append(&label);
        tags_filter_list.append(&row);
    }

    if !active_snapshot.is_empty() {
        let clear = gtk4::Button::with_label("Clear tag filter");
        clear.add_css_class("flat");
        let filters_clear = active_tag_filters.clone();
        let debounce_clear = tag_filter_debounce_gen.clone();
        let filter_clear = filter.clone();
        let folder_clear = current_folder.clone();
        let btn_clear = tags_filter_btn.clone();
        let list_clear = tags_filter_list.clone();
        let known_clear: Vec<String> = known_tags.to_vec();
        let grid_loading_clear = grid_loading.clone();
        clear.connect_clicked(move |_| {
            // Cancel any pending debounced apply.
            debounce_clear.set(debounce_clear.get().wrapping_add(1));
            filters_clear.borrow_mut().clear();
            persist_tag_filters(&folder_clear, &filters_clear.borrow());
            rebuild_tag_filter_list(
                &list_clear,
                &btn_clear,
                &filters_clear,
                &debounce_clear,
                &filter_clear,
                &folder_clear,
                &known_clear,
                &grid_loading_clear,
            );
            apply_filter_change(
                &grid_loading_clear,
                &filter_clear,
                gtk4::FilterChange::LessStrict,
                "Updating filters…",
            );
        });
        tags_filter_list.append(&clear);
    }

    sync_tags_filter_button_style(tags_filter_btn, active_tag_filters);
}

/// Refresh tag filter UI from the current folder DB (known tags).
pub(crate) fn refresh_tag_filter_from_folder(
    tags_filter_list: &gtk4::Box,
    tags_filter_btn: &gtk4::MenuButton,
    active_tag_filters: &Rc<RefCell<HashMap<String, TagFilterMode>>>,
    tag_filter_debounce_gen: &Rc<Cell<u64>>,
    filter: &CustomFilter,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    grid_loading: &Rc<RefCell<Option<GridLoadingOverlay>>>,
) {
    let known = current_folder
        .borrow()
        .as_ref()
        .and_then(|folder| db::open(folder).ok())
        .and_then(|conn| db::list_all_tags_in_folder(&conn).ok())
        .unwrap_or_default();
    rebuild_tag_filter_list(
        tags_filter_list,
        tags_filter_btn,
        active_tag_filters,
        tag_filter_debounce_gen,
        filter,
        current_folder,
        &known,
        grid_loading,
    );
}

/// Retained for wiring symmetry; live apply is scheduled from polarity clicks.
pub(crate) fn install_tags_filter_popover_handler(_tags_filter_btn: &gtk4::MenuButton) {
    // No close-time apply — tag filter updates are debounced while the popover is open.
}

pub(crate) fn install_sort_dropdown_handler(
    sort_dropdown: &gtk4::DropDown,
    sort_key: &Rc<RefCell<String>>,
    sorter: &CustomSorter,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    scan_in_progress: &Rc<Cell<bool>>,
    start_scan_for_folder: &Rc<dyn Fn(PathBuf)>,
) {
    let sort_key_dd = sort_key.clone();
    let sorter_dd = sorter.clone();
    let current_folder_dd = current_folder.clone();
    let scan_in_progress_dd = scan_in_progress.clone();
    let start_scan_dd = start_scan_for_folder.clone();
    sort_dropdown.connect_selected_notify(move |dd| {
        let key = sort_key_for_index(dd.selected());
        let new_key = key.to_string();
        if *sort_key_dd.borrow() == new_key {
            return;
        }
        *sort_key_dd.borrow_mut() = new_key;
        if let Some(folder) = current_folder_dd.borrow().as_ref() {
            let _ = db::set_ui_state_value(folder.as_path(), "sort_key", &sort_key_dd.borrow());
        }

        if scan_in_progress_dd.get() {
            if let Some(folder) = current_folder_dd.borrow().as_ref().cloned() {
                start_scan_dd(folder);
                return;
            }
        }

        sorter_dd.changed(gtk4::SorterChange::Different);
    });
}

pub(crate) fn install_search_entry_handler(
    search_entry: &gtk4::SearchEntry,
    search_text: &Rc<RefCell<String>>,
    filter: &CustomFilter,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    grid_loading: &Rc<RefCell<Option<GridLoadingOverlay>>>,
) {
    let search_text_entry = search_text.clone();
    let filter_entry = filter.clone();
    let current_folder_search = current_folder.clone();
    let grid_loading = grid_loading.clone();
    search_entry.connect_search_changed(move |entry| {
        let prev_empty = search_text_entry.borrow().is_empty();
        let new_text = entry.text().to_lowercase();
        let new_empty = new_text.is_empty();
        *search_text_entry.borrow_mut() = new_text;
        if let Some(folder) = current_folder_search.borrow().as_ref() {
            let _ = db::set_ui_state_value(
                folder.as_path(),
                "search_text",
                &search_text_entry.borrow(),
            );
        }
        let change = if new_empty {
            gtk4::FilterChange::LessStrict
        } else if prev_empty {
            gtk4::FilterChange::MoreStrict
        } else {
            gtk4::FilterChange::Different
        };
        apply_filter_change(&grid_loading, &filter_entry, change, "Updating filters…");
    });
}

pub(crate) fn apply_clear_filters(
    search_text: &Rc<RefCell<String>>,
    favorites_only: &Rc<Cell<bool>>,
    active_tag_filters: &Rc<RefCell<HashMap<String, TagFilterMode>>>,
    tag_filter_debounce_gen: &Rc<Cell<u64>>,
    similar_paths: &Rc<RefCell<Option<HashSet<String>>>>,
    similar_query_path: &Rc<RefCell<Option<String>>>,
    sort_key: &Rc<RefCell<String>>,
    filter: &CustomFilter,
    _sorter: &CustomSorter,
    favourites_filter_btn: &gtk4::ToggleButton,
    tags_filter_btn: &gtk4::MenuButton,
    tags_filter_list: &gtk4::Box,
    search_entry: &gtk4::SearchEntry,
    _sort_dropdown: &gtk4::DropDown,
    thumbnail_size: &Rc<RefCell<i32>>,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    similar_filter_btn: &gtk4::Button,
    grid_loading: &Rc<RefCell<Option<GridLoadingOverlay>>>,
) {
    *search_text.borrow_mut() = String::new();
    favorites_only.set(false);
    tag_filter_debounce_gen.set(tag_filter_debounce_gen.get().wrapping_add(1));
    active_tag_filters.borrow_mut().clear();
    *similar_paths.borrow_mut() = None;
    *similar_query_path.borrow_mut() = None;
    set_similar_filter_chrome(similar_filter_btn, false);
    favourites_filter_btn.remove_css_class("favorites-filter-active");
    favourites_filter_btn.set_active(false);
    search_entry.set_text("");
    if let Some(folder) = current_folder.borrow().as_ref() {
        let _ = db::save_ui_state(
            folder.as_path(),
            &db::UiState {
                sort_key: sort_key.borrow().clone(),
                search_text: search_text.borrow().clone(),
                favorites_only: favorites_only.get(),
                active_tag_filters: HashMap::new(),
                thumbnail_size: *thumbnail_size.borrow(),
            },
        );
    }
    refresh_tag_filter_from_folder(
        tags_filter_list,
        tags_filter_btn,
        active_tag_filters,
        tag_filter_debounce_gen,
        filter,
        current_folder,
        grid_loading,
    );
    apply_filter_change(
        grid_loading,
        filter,
        gtk4::FilterChange::LessStrict,
        "Updating filters…",
    );
}

pub(crate) fn deactivate_tag_filter(
    active_tag_filters: &Rc<RefCell<HashMap<String, TagFilterMode>>>,
    tag_filter_debounce_gen: &Rc<Cell<u64>>,
    filter: &CustomFilter,
    tags_filter_btn: &gtk4::MenuButton,
    tags_filter_list: &gtk4::Box,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    grid_loading: &Rc<RefCell<Option<GridLoadingOverlay>>>,
) {
    tag_filter_debounce_gen.set(tag_filter_debounce_gen.get().wrapping_add(1));
    active_tag_filters.borrow_mut().clear();
    persist_tag_filters(current_folder, &active_tag_filters.borrow());
    refresh_tag_filter_from_folder(
        tags_filter_list,
        tags_filter_btn,
        active_tag_filters,
        tag_filter_debounce_gen,
        filter,
        current_folder,
        grid_loading,
    );
    apply_filter_change(
        grid_loading,
        filter,
        gtk4::FilterChange::LessStrict,
        "Updating filters…",
    );
}

pub(crate) fn deactivate_favorites_filter(
    favorites_only: &Rc<Cell<bool>>,
    filter: &CustomFilter,
    favourites_filter_btn: &gtk4::ToggleButton,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    grid_loading: &Rc<RefCell<Option<GridLoadingOverlay>>>,
) {
    favorites_only.set(false);
    favourites_filter_btn.remove_css_class("favorites-filter-active");
    favourites_filter_btn.set_active(false);
    if let Some(folder) = current_folder.borrow().as_ref() {
        let _ = db::set_ui_state_value(folder.as_path(), "favorites_only", "0");
    }
    apply_filter_change(
        grid_loading,
        filter,
        gtk4::FilterChange::LessStrict,
        "Updating filters…",
    );
}

pub(crate) fn install_clear_button_handler(
    clear_btn: &gtk4::Button,
    search_text: &Rc<RefCell<String>>,
    favorites_only: &Rc<Cell<bool>>,
    active_tag_filters: &Rc<RefCell<HashMap<String, TagFilterMode>>>,
    tag_filter_debounce_gen: &Rc<Cell<u64>>,
    similar_paths: &Rc<RefCell<Option<HashSet<String>>>>,
    similar_query_path: &Rc<RefCell<Option<String>>>,
    sort_key: &Rc<RefCell<String>>,
    filter: &CustomFilter,
    sorter: &CustomSorter,
    favourites_filter_btn: &gtk4::ToggleButton,
    tags_filter_btn: &gtk4::MenuButton,
    tags_filter_list: &gtk4::Box,
    search_entry: &gtk4::SearchEntry,
    sort_dropdown: &gtk4::DropDown,
    thumbnail_size: &Rc<RefCell<i32>>,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    similar_filter_btn: &gtk4::Button,
    grid_loading: &Rc<RefCell<Option<GridLoadingOverlay>>>,
) {
    let search_text_clear = search_text.clone();
    let favorites_only_clear = favorites_only.clone();
    let active_tag_filters_clear = active_tag_filters.clone();
    let tag_filter_debounce_gen_clear = tag_filter_debounce_gen.clone();
    let similar_paths_clear = similar_paths.clone();
    let similar_query_path_clear = similar_query_path.clone();
    let sort_key_clear = sort_key.clone();
    let filter_clear = filter.clone();
    let sorter_clear = sorter.clone();
    let favourites_filter_btn_clear = favourites_filter_btn.clone();
    let tags_filter_btn_clear = tags_filter_btn.clone();
    let tags_filter_list_clear = tags_filter_list.clone();
    let search_entry_clear = search_entry.clone();
    let sort_dropdown_clear = sort_dropdown.clone();
    let thumbnail_size_clear = thumbnail_size.clone();
    let current_folder_clear = current_folder.clone();
    let similar_filter_btn_clear = similar_filter_btn.clone();
    let grid_loading_clear = grid_loading.clone();
    clear_btn.connect_clicked(move |_| {
        apply_clear_filters(
            &search_text_clear,
            &favorites_only_clear,
            &active_tag_filters_clear,
            &tag_filter_debounce_gen_clear,
            &similar_paths_clear,
            &similar_query_path_clear,
            &sort_key_clear,
            &filter_clear,
            &sorter_clear,
            &favourites_filter_btn_clear,
            &tags_filter_btn_clear,
            &tags_filter_list_clear,
            &search_entry_clear,
            &sort_dropdown_clear,
            &thumbnail_size_clear,
            &current_folder_clear,
            &similar_filter_btn_clear,
            &grid_loading_clear,
        );
    });
}

pub(crate) fn install_similar_filter_button_handler(
    similar_filter_btn: &gtk4::Button,
    similar_paths: &Rc<RefCell<Option<HashSet<String>>>>,
    similar_query_path: &Rc<RefCell<Option<String>>>,
    filter: &CustomFilter,
    grid_loading: &Rc<RefCell<Option<GridLoadingOverlay>>>,
) {
    let similar_paths = similar_paths.clone();
    let similar_query_path = similar_query_path.clone();
    let filter = filter.clone();
    let btn = similar_filter_btn.clone();
    let grid_loading = grid_loading.clone();
    similar_filter_btn.connect_clicked(move |_| {
        if similar_paths.borrow().is_none() {
            return;
        }
        clear_similar_filter(
            &similar_paths,
            &similar_query_path,
            &filter,
            &btn,
            &grid_loading,
        );
    });
}

pub(crate) fn install_favorites_only_handler(
    favourites_filter_btn: &gtk4::ToggleButton,
    favorites_only: &Rc<Cell<bool>>,
    filter: &CustomFilter,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    toast_overlay: &adw::ToastOverlay,
    selection_model: &MultiSelection,
    list_store: &ListStore,
    grid_loading: &Rc<RefCell<Option<GridLoadingOverlay>>>,
) {
    let favourites_filter_btn_toggle = favourites_filter_btn.clone();
    let favorites_only_toggle = favorites_only.clone();
    let filter_toggle = filter.clone();
    let current_folder_toggle = current_folder.clone();
    let toast_overlay_toggle = toast_overlay.clone();
    let selection_model_toggle = selection_model.clone();
    let list_store_toggle = list_store.clone();
    let grid_loading = grid_loading.clone();
    favourites_filter_btn.connect_toggled(move |btn| {
        let active = btn.is_active();
        favorites_only_toggle.set(active);
        if active {
            favourites_filter_btn_toggle.add_css_class("favorites-filter-active");
        } else {
            favourites_filter_btn_toggle.remove_css_class("favorites-filter-active");
        }
        if let Some(folder) = current_folder_toggle.borrow().as_ref() {
            let _ = db::set_ui_state_value(
                folder.as_path(),
                "favorites_only",
                if active { "1" } else { "0" },
            );
        }
        let change = if active {
            gtk4::FilterChange::MoreStrict
        } else {
            gtk4::FilterChange::LessStrict
        };
        let toast_overlay_toggle = toast_overlay_toggle.clone();
        let selection_model_toggle = selection_model_toggle.clone();
        let list_store_toggle = list_store_toggle.clone();
        apply_filter_change_then(
            &grid_loading,
            &filter_toggle,
            change,
            "Updating filters…",
            move || {
                if active
                    && list_store_toggle.n_items() > 0
                    && selection_model_toggle.n_items() == 0
                {
                    let toast = adw::Toast::new(
                        "No favourites match — turn off the favourites filter to see all images.",
                    );
                    toast.set_timeout(2);
                    toast_overlay_toggle.add_toast(toast);
                }
            },
        );
    });
}

pub(crate) fn install_thumbnail_size_handlers(
    size_buttons: &Rc<Vec<gtk4::ToggleButton>>,
    size_options: [i32; 4],
    app_state: &AppState,
    grid_view: &gtk4::GridView,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
) {
    let app_state_toggle = app_state.clone();
    let grid_view_toggle = grid_view.clone();
    for (idx, button) in size_buttons.iter().enumerate() {
        let options = size_options;
        let buttons = size_buttons.clone();
        let app_state_toggle = app_state_toggle.clone();
        let grid_view_toggle = grid_view_toggle.clone();
        let current_folder_toggle = current_folder.clone();
        button.connect_clicked(move |_| {
            let selected_size = options[idx];
            let was_selected = *app_state_toggle.thumbnail_size.borrow() == selected_size;
            *app_state_toggle.thumbnail_size.borrow_mut() = selected_size;

            for (i, btn) in buttons.iter().enumerate() {
                btn.set_active(i == idx);
            }

            if was_selected {
                return;
            }
            if let Some(folder) = current_folder_toggle.borrow().as_ref() {
                let _ = db::set_ui_state_value(
                    folder.as_path(),
                    "thumbnail_size",
                    &selected_size.to_string(),
                );
            }

            apply_thumbnail_size_change(
                selected_size,
                &app_state_toggle,
                &grid_view_toggle,
            );
        });
    }
}
