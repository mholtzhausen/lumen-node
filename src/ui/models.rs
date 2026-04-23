use crate::sort::{
    normalize_sort_key, SORT_KEY_DATE_DESC, SORT_KEY_NAME_DESC, SORT_KEY_SIZE_DESC,
};
use crate::sort_flags::{compute_sort_fields, SortFields};
use crate::ImageMetadata;
use gtk4::prelude::*;
use gtk4::{gio, CustomFilter, CustomSorter, FilterListModel, SingleSelection, SortListModel, StringObject};
use std::{cell::RefCell, collections::HashMap, path::Path, rc::Rc};

pub(crate) struct ModelAssemblyDeps {
    pub(crate) list_store: gio::ListStore,
    pub(crate) meta_cache: Rc<RefCell<HashMap<String, ImageMetadata>>>,
    pub(crate) search_text: Rc<RefCell<String>>,
    pub(crate) sort_key: Rc<RefCell<String>>,
    pub(crate) sort_fields_cache: Rc<RefCell<HashMap<String, SortFields>>>,
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
    let meta_cache_filter = deps.meta_cache.clone();
    let search_text_filter = deps.search_text.clone();
    let filter = CustomFilter::new(move |obj| {
        let query = search_text_filter.borrow().to_lowercase();
        if query.is_empty() {
            return true;
        }
        let path_str = obj
            .downcast_ref::<StringObject>()
            .map(|s| s.string().to_string())
            .unwrap_or_default();
        // Match against filename.
        let filename = Path::new(&path_str)
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if filename.contains(&query) {
            return true;
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
    let filter_model = FilterListModel::new(Some(deps.list_store.clone()), Some(filter.clone()));

    // Sort model: wraps filter_model, applies selected sort key.
    let sort_key_sorter = deps.sort_key.clone();
    let sort_fields_cache_sorter = deps.sort_fields_cache.clone();
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
    sort_model.connect_items_changed(move |model, _, _, _| {
        if model.n_items() > 0 && selection_for_default.selected_item().is_none() {
            selection_for_default.set_selected(0);
        }
    });

    ModelBundle {
        filter,
        filter_model,
        sorter,
        sort_model,
        selection_model,
    }
}
