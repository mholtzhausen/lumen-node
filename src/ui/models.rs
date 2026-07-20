use crate::core::app_state::AppState;
use crate::sort::{normalize_sort_key, SORT_KEY_DATE_DESC, SORT_KEY_NAME_DESC, SORT_KEY_SIZE_DESC};
use crate::sort_flags::compute_sort_fields;
use crate::view_helpers::{
    cancel_batch_selection, find_path_index, path_at_index, primary_image_path, selected_count,
    selected_image_path_strings, select_only_index, select_only_path,
};
use gtk4::prelude::*;
use gtk4::{
    CustomFilter, CustomSorter, FilterListModel, MultiSelection, SortListModel, StringObject,
};
use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

pub(crate) struct ModelAssemblyDeps {
    pub(crate) app_state: AppState,
}

pub(crate) struct ModelBundle {
    pub(crate) filter: CustomFilter,
    pub(crate) filter_model: FilterListModel,
    pub(crate) sorter: CustomSorter,
    pub(crate) sort_model: SortListModel,
    pub(crate) selection_model: MultiSelection,
}

pub(crate) fn build_model_bundle(deps: ModelAssemblyDeps) -> ModelBundle {
    // Filter model: wraps list_store, applies favourites / tags / search.
    let meta_cache_filter = deps.app_state.meta_cache.clone();
    let favourite_cache_filter = deps.app_state.favourite_cache.clone();
    let tags_cache_filter = deps.app_state.tags_cache.clone();
    let search_text_filter = deps.app_state.search_text.clone();
    let favorites_only_filter = deps.app_state.favorites_only.clone();
    let active_tags_filter = deps.app_state.active_tag_filters.clone();
    let similar_paths_filter = deps.app_state.similar_paths.clone();
    let filter = CustomFilter::new(move |obj| {
        let path_str = obj
            .downcast_ref::<StringObject>()
            .map(|s| s.string().to_string())
            .unwrap_or_default();
        if let Some(similar) = similar_paths_filter.borrow().as_ref() {
            if !similar.contains(&path_str) {
                return false;
            }
        }
        let matches_favorite = if favorites_only_filter.get() {
            favourite_cache_filter
                .borrow()
                .get(&path_str)
                .copied()
                .unwrap_or(false)
        } else {
            true
        };
        if !matches_favorite {
            return false;
        }
        {
            let active = active_tags_filter.borrow();
            if !active.is_empty() {
                let tags = tags_cache_filter.borrow();
                let image_tags = tags.get(&path_str);
                for (required_tag, mode) in active.iter() {
                    let has_tag = image_tags
                        .map(|t| t.iter().any(|x| x == required_tag))
                        .unwrap_or(false);
                    match mode {
                        crate::db::TagFilterMode::Require => {
                            if !has_tag {
                                return false;
                            }
                        }
                        crate::db::TagFilterMode::Exclude => {
                            if has_tag {
                                return false;
                            }
                        }
                    }
                }
            }
        }
        let query = search_text_filter.borrow().clone();
        if query.is_empty() {
            return true;
        }
        // Match against filename.
        let filename = Path::new(&path_str)
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if filename.contains(&query) {
            return true;
        }
        // Match against tags.
        if let Some(tags) = tags_cache_filter.borrow().get(&path_str) {
            for tag in tags {
                if tag.to_lowercase().contains(&query) {
                    return true;
                }
            }
        }
        // Match against cached metadata fields.
        let cache = meta_cache_filter.borrow();
        if let Some(meta) = cache.get(&path_str) {
            let fields: [Option<&str>; 8] = [
                meta.camera_make.as_deref(),
                meta.camera_model.as_deref(),
                meta.exposure.as_deref(),
                meta.iso.as_deref(),
                meta.prompt.as_deref(),
                meta.negative_prompt.as_deref(),
                meta.raw_parameters.as_deref(),
                meta.workflow_json.as_deref(),
            ];
            for field in fields.iter().flatten() {
                if field.to_lowercase().contains(&query) {
                    return true;
                }
            }
        }
        false
    });
    let filter_model = FilterListModel::new(
        Some(deps.app_state.list_store.clone()),
        Some(filter.clone()),
    );

    // Sort model: wraps filter_model, applies selected sort key.
    let sort_key_sorter = deps.app_state.sort_key.clone();
    let sort_fields_cache_sorter = deps.app_state.sort_fields_cache.clone();
    let sorter = CustomSorter::new(move |a, b| {
        let path_a = a
            .downcast_ref::<StringObject>()
            .map(|s| s.string().to_string())
            .unwrap_or_default();
        let path_b = b
            .downcast_ref::<StringObject>()
            .map(|s| s.string().to_string())
            .unwrap_or_default();
        let key = sort_key_sorter.borrow().clone();
        let cache = sort_fields_cache_sorter.borrow();
        let fallback_a;
        let fallback_b;
        let fields_a = if let Some(fields) = cache.get(&path_a) {
            fields
        } else {
            fallback_a = compute_sort_fields(&path_a);
            &fallback_a
        };
        let fields_b = if let Some(fields) = cache.get(&path_b) {
            fields
        } else {
            fallback_b = compute_sort_fields(&path_b);
            &fallback_b
        };
        let ord = match normalize_sort_key(key.as_str()) {
            "name_asc" | "name_desc" => {
                let cmp = fields_a
                    .filename_lower
                    .cmp(&fields_b.filename_lower)
                    .then_with(|| path_a.cmp(&path_b));
                if key == SORT_KEY_NAME_DESC {
                    cmp.reverse()
                } else {
                    cmp
                }
            }
            "date_asc" | "date_desc" => {
                let cmp = fields_a
                    .modified
                    .cmp(&fields_b.modified)
                    .then_with(|| fields_a.filename_lower.cmp(&fields_b.filename_lower))
                    .then_with(|| path_a.cmp(&path_b));
                if key == SORT_KEY_DATE_DESC {
                    cmp.reverse()
                } else {
                    cmp
                }
            }
            "size_asc" | "size_desc" => {
                let cmp = fields_a
                    .size
                    .cmp(&fields_b.size)
                    .then_with(|| fields_a.filename_lower.cmp(&fields_b.filename_lower))
                    .then_with(|| path_a.cmp(&path_b));
                if key == SORT_KEY_SIZE_DESC {
                    cmp.reverse()
                } else {
                    cmp
                }
            }
            _ => std::cmp::Ordering::Equal,
        };
        match ord {
            std::cmp::Ordering::Less => gtk4::Ordering::Smaller,
            std::cmp::Ordering::Greater => gtk4::Ordering::Larger,
            std::cmp::Ordering::Equal => gtk4::Ordering::Equal,
        }
    });
    let sort_model = SortListModel::new(Some(filter_model.clone()), Some(sorter.clone()));

    let selection_model = MultiSelection::new(Some(sort_model.clone()));
    let selection_for_default = selection_model.clone();
    let selected_path_hint = deps.app_state.selected_path.clone();
    let selected_paths_mirror = deps.app_state.selected_paths.clone();
    // Last valid selection index — kept across clears so filter eviction can
    // reselect a neighbor after FilterChange::Different rebuilds (position=0).
    let last_selected_index = Rc::new(Cell::new(0u32));
    {
        let selected_path_hint = selected_path_hint.clone();
        let selected_paths_mirror = selected_paths_mirror.clone();
        let last_selected_index = last_selected_index.clone();
        selection_model.connect_selection_changed(move |model, _, _| {
            let paths = selected_image_path_strings(model);
            let count = paths.len();
            let primary = if count == 0 {
                None
            } else if count == 1 {
                paths.first().cloned()
            } else {
                // Prefer existing primary if still selected; else last index in set.
                let prev = selected_path_hint.borrow().clone();
                if prev.as_ref().is_some_and(|p| paths.iter().any(|x| x == p)) {
                    prev
                } else {
                    primary_image_path(model).map(|p| p.to_string_lossy().into_owned())
                }
            };
            if let Some(ref p) = primary {
                if let Some(pos) = find_path_index(model, p) {
                    last_selected_index.set(pos);
                }
            }
            *selected_path_hint.borrow_mut() = primary;
            *selected_paths_mirror.borrow_mut() = paths.into_iter().collect();
        });
    }
    {
        let selected_path_hint = selected_path_hint.clone();
        let last_selected_index = last_selected_index.clone();
        sort_model.connect_items_changed(move |model, _position, removed, _added| {
            if model.n_items() == 0 {
                let _ = selection_for_default.unselect_all();
                return;
            }

            let count = selected_count(&selection_for_default);
            let hint = selected_path_hint.borrow().clone();

            // Filter / sort reshuffle with multi selected → cancel batch to primary.
            if count > 1 {
                cancel_batch_selection(&selection_for_default, hint.as_deref());
                return;
            }

            let selected_path = if count == 1 {
                primary_image_path(&selection_for_default).map(|p| p.to_string_lossy().into_owned())
            } else {
                None
            };

            // Fast path: selection already matches the path hint — no O(N) scan.
            if hint.is_some() && hint == selected_path {
                return;
            }

            // Prefer restoring by absolute path after sort/filter reshuffles.
            if let Some(ref wanted_path) = hint {
                if select_only_path(&selection_for_default, wanted_path) {
                    return;
                }
            }

            // Selection filtered out (or hint path gone): keep place like trash —
            // select what slid into the vacated index (next), or previous if last.
            if selected_path.is_none() && removed > 0 {
                let next_idx = last_selected_index
                    .get()
                    .min(model.n_items().saturating_sub(1));
                select_only_index(&selection_for_default, next_idx);
                return;
            }

            // Hint missing or stale (folder change): bounce to force a preview reload.
            match selected_path {
                None => {
                    select_only_index(&selection_for_default, 0);
                }
                Some(path) if hint.as_ref() != Some(&path) => {
                    let pos = find_path_index(&selection_for_default, &path).unwrap_or(0);
                    let pos = if pos < model.n_items() { pos } else { 0 };
                    let _ = selection_for_default.unselect_all();
                    select_only_index(&selection_for_default, pos);
                    // Ensure hint catches up even if path string matches.
                    if let Some(p) = path_at_index(&selection_for_default, pos) {
                        *selected_path_hint.borrow_mut() = Some(p);
                    }
                }
                Some(_) => {}
            }
        });
    }

    ModelBundle {
        filter,
        filter_model,
        sorter,
        sort_model,
        selection_model,
    }
}
