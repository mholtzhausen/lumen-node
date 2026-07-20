use crate::core::app_state::AppState;
use crate::image_types::is_supported_image_path;
use crate::similarity::{rekey_prompt_index, upsert_prompt_index};
use crate::sort_flags::compute_sort_fields;
use crate::view_helpers::{
    find_path_index, is_path_selected, primary_selected_index, select_only_index, select_only_path,
};
use gtk4::prelude::*;
use gtk4::StringObject;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[derive(Clone)]
pub struct ListMutationContext {
    pub app_state: AppState,
    pub selection_model: gtk4::MultiSelection,
    pub start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
}

impl ListMutationContext {
    pub fn fallback_rescan(&self) {
        if let Some(folder) = self.app_state.current_folder.borrow().as_ref().cloned() {
            (self.start_scan_for_folder)(folder);
        }
    }

    pub fn remove_path(&self, target_path: &Path) -> bool {
        let target = target_path.to_string_lossy().to_string();
        let remove_idx = self.find_list_store_index(&target);
        let Some(remove_idx) = remove_idx else {
            return false;
        };

        let was_selected = is_path_selected(&self.selection_model, &target);
        let selected_idx_before = find_path_index(&self.selection_model, &target)
            .or_else(|| primary_selected_index(&self.selection_model))
            .unwrap_or(0);

        self.app_state.list_store.remove(remove_idx);
        self.app_state.hash_cache.borrow_mut().remove(&target);
        self.app_state.meta_cache.borrow_mut().remove(&target);
        self.app_state.favourite_cache.borrow_mut().remove(&target);
        self.app_state.tags_cache.borrow_mut().remove(&target);
        self.app_state
            .prompt_similarity_index
            .borrow_mut()
            .remove(&target);
        if let Some(similar) = self.app_state.similar_paths.borrow_mut().as_mut() {
            similar.remove(&target);
        }
        self.app_state.sort_fields_cache.borrow_mut().remove(&target);
        self.app_state.selected_paths.borrow_mut().remove(&target);

        if was_selected {
            let item_count = self.selection_model.n_items();
            if item_count == 0 {
                return true;
            }
            // If other selections remain, leave them; else pick neighbor.
            if self.selection_model.selection().size() == 0 {
                let next_idx = selected_idx_before.min(item_count.saturating_sub(1));
                select_only_index(&self.selection_model, next_idx);
            }
        }
        true
    }

    pub fn insert_path(&self, target_path: &Path, select_new: bool) -> bool {
        if !target_path.is_file() || !is_supported_image_path(target_path) {
            return false;
        }
        let target = target_path.to_string_lossy().to_string();
        if self.find_list_store_index(&target).is_some() {
            if select_new {
                self.select_path(&target);
            }
            self.upsert_similarity_for_path(&target);
            return true;
        }

        self.app_state
            .sort_fields_cache
            .borrow_mut()
            .insert(target.clone(), compute_sort_fields(&target));
        self.app_state.list_store.append(&StringObject::new(&target));
        self.upsert_similarity_for_path(&target);
        if select_new {
            self.select_path(&target);
        }
        true
    }

    pub fn replace_path(&self, old_path: &Path, new_path: &Path, select_new: bool) -> bool {
        let old_key = old_path.to_string_lossy().to_string();
        let new_key = new_path.to_string_lossy().to_string();

        // Preserve meta under the new key before remove_path drops the old entry.
        if old_key != new_key {
            let preserved_meta = self.app_state.meta_cache.borrow().get(&old_key).cloned();
            if let Some(meta) = preserved_meta {
                self.app_state
                    .meta_cache
                    .borrow_mut()
                    .entry(new_key.clone())
                    .or_insert(meta);
            }
            rekey_prompt_index(
                &mut self.app_state.prompt_similarity_index.borrow_mut(),
                &old_key,
                &new_key,
            );
        }

        let removed = self.remove_path(old_path);
        // remove_path clears index for old_key only; rekey already moved the entry.
        let inserted = self.insert_path(new_path, select_new);
        removed && inserted
    }

    fn upsert_similarity_for_path(&self, path: &str) {
        let meta = self.app_state.meta_cache.borrow().get(path).cloned();
        if let Some(meta) = meta {
            upsert_prompt_index(
                &mut self.app_state.prompt_similarity_index.borrow_mut(),
                path,
                &meta,
            );
        }
    }

    fn find_list_store_index(&self, target: &str) -> Option<u32> {
        for idx in 0..self.app_state.list_store.n_items() {
            let is_match = self
                .app_state
                .list_store
                .item(idx)
                .and_downcast::<StringObject>()
                .map(|obj| obj.string().as_str() == target)
                .unwrap_or(false);
            if is_match {
                return Some(idx);
            }
        }
        None
    }

    fn select_path(&self, target: &str) {
        let _ = select_only_path(&self.selection_model, target);
    }
}
