use gtk4::prelude::*;
use gtk4::{gio, glib, ListScrollFlags, ListView, TreeListModel, TreeListRow};
use std::path::{Path, PathBuf};

/// Expands ancestor rows in the `TreeListModel` so `target` is visible, then
/// selects and scrolls to it.
pub fn sync_tree_to_path(tree_model: &TreeListModel, tree_list_view: &ListView, target: &Path) {
    // Find the root item that is either equal to `target` or its deepest
    // ancestor that appears as a root row (depth 0).
    let n = tree_model.n_items();
    let mut best_root: Option<(u32, PathBuf)> = None;
    for pos in 0..n {
        if let Some(row) = tree_model.item(pos).and_downcast::<TreeListRow>() {
            if row.depth() != 0 {
                continue;
            }
            if let Some(file) = row.item().and_downcast::<gio::File>() {
                if let Some(p) = file.path() {
                    if target.starts_with(&p) {
                        let depth = p.components().count();
                        let better = best_root
                            .as_ref()
                            .map_or(true, |(_, b)| depth > b.components().count());
                        if better {
                            best_root = Some((pos, p));
                        }
                    }
                }
            }
        }
    }
    let (_, root_path) = match best_root {
        Some(v) => v,
        None => return,
    };

    // Build the chain: root_path → … → target (each step one component deeper)
    let rel = match target.strip_prefix(&root_path) {
        Ok(r) => r,
        Err(_) => return,
    };
    let mut segments: Vec<PathBuf> = vec![root_path.clone()];
    let mut acc = root_path;
    for component in rel.components() {
        acc.push(component);
        segments.push(acc.clone());
    }

    // Walk segments: find each in the flat model, expand non-last ones.
    for (i, seg) in segments.iter().enumerate() {
        let is_last = i == segments.len() - 1;
        let n = tree_model.n_items();
        for pos in 0..n {
            if let Some(row) = tree_model.item(pos).and_downcast::<TreeListRow>() {
                if let Some(file) = row.item().and_downcast::<gio::File>() {
                    if file.path().as_deref() == Some(seg.as_path()) {
                        if is_last {
                            tree_list_view.scroll_to(
                                pos,
                                ListScrollFlags::SELECT,
                                None::<gtk4::ScrollInfo>,
                            );
                        } else if row.is_expandable() {
                            row.set_expanded(true);
                        }
                        break;
                    }
                }
            }
        }
    }
}

/// Replaces the tree root with exactly one folder entry.
pub fn reset_tree_root(tree_root: &gio::ListStore, root_path: &Path) {
    tree_root.remove_all();
    tree_root.append(&gio::File::for_path(root_path));
}

/// Builds the root `ListStore` for the file tree.
/// Uses last opened folder when present, otherwise falls back to home.
pub fn build_tree_root(last_folder: Option<&PathBuf>) -> gio::ListStore {
    let store = gio::ListStore::new::<gio::File>();
    let root = match last_folder {
        Some(path) if path.is_dir() => path.clone(),
        _ => glib::home_dir(),
    };
    store.append(&gio::File::for_path(root));
    store
}
