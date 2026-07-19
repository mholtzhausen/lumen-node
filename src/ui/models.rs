use crate::core::app_state::AppState;
use crate::sort::{normalize_sort_key, SORT_KEY_DATE_DESC, SORT_KEY_NAME_DESC, SORT_KEY_SIZE_DESC};
use crate::sort_flags::compute_sort_fields;
use gtk4::prelude::*;
use gtk4::{
    CustomFilter, CustomSorter, FilterListModel, SingleSelection, SortListModel, StringObject,
};
use std::path::Path;

pub(crate) struct ModelAssemblyDeps {
    pub(crate) app_state: AppState,
}

pub(crate) struct ModelBundle {
    pub(crate) filter: CustomFilter,
    pub(crate) filter_model: FilterListModel,
    pub(crate) sorter: CustomSorter,
    pub(crate) sort_model: SortListModel,
    pub(crate) selection_model: SingleSelection,
}

pub(crate) fn build_model_bundle(deps: ModelAssemblyDeps) -> ModelBundle {
    // Filter model: wraps list_store, applies search text.
    let meta_cache_filter = deps.app_state.meta_cache.clone();
    let favourite_cache_filter = deps.app_state.favourite_cache.clone();
    let search_text_filter = deps.app_state.search_text.clone();
    let favorites_only_filter = deps.app_state.favorites_only.clone();
    let filter = CustomFilter::new(move |obj| {
        let path_str = obj
            .downcast_ref::<StringObject>()
            .map(|s| s.string().to_string())
            .unwrap_or_default();
        let matches_favorite = if favorites_only_filter.get() {
            favourite_cache_filter
                .borrow()
                .get(&path_str)
                .copied()
                .unwrap_or(false)
        } else {
            true
        };
        let query = search_text_filter.borrow().clone();
        if query.is_empty() {
            return matches_favorite;
        }
        // Match against filename.
        let filename = Path::new(&path_str)
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if filename.contains(&query) {
            return matches_favorite;
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
                    return matches_favorite;
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

    let selection_model = SingleSelection::new(Some(sort_model.clone()));
    let selection_for_default = selection_model.clone();
    let selected_path_hint: std::rc::Rc<std::cell::RefCell<Option<String>>> =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    {
        let selected_path_hint = selected_path_hint.clone();
        selection_model.connect_selection_changed(move |model, _, _| {
            let path = model
                .selected_item()
                .and_downcast::<StringObject>()
                .map(|obj| obj.string().to_string());
            *selected_path_hint.borrow_mut() = path;
        });
    }
    {
        let selected_path_hint = selected_path_hint.clone();
        sort_model.connect_items_changed(move |model, _, _, _| {
            if model.n_items() == 0 {
                return;
            }

            // Prefer restoring by absolute path after sort/filter reshuffles.
            let target_pos = selected_path_hint.borrow().as_ref().and_then(|wanted_path| {
                (0..model.n_items()).find(|idx| {
                    model
                        .item(*idx)
                        .and_downcast::<StringObject>()
                        .map(|obj| obj.string().as_str() == wanted_path.as_str())
                        .unwrap_or(false)
                })
            });

            if let Some(pos) = target_pos {
                if selection_for_default.selected() != pos {
                    selection_for_default.set_selected(pos);
                }
                return;
            }

            // Hint missing or stale (folder change): index may be unchanged so
            // selection-changed never fired — bounce to force a preview reload.
            let selected_path = selection_for_default
                .selected_item()
                .and_downcast::<StringObject>()
                .map(|obj| obj.string().to_string());

            match selected_path {
                None => {
                    selection_for_default.set_selected(0);
                }
                Some(path) if selected_path_hint.borrow().as_ref() != Some(&path) => {
                    selection_for_default.set_can_unselect(true);
                    let pos = selection_for_default.selected();
                    let pos = if pos < model.n_items() { pos } else { 0 };
                    selection_for_default.set_selected(gtk4::INVALID_LIST_POSITION);
                    selection_for_default.set_selected(pos);
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
