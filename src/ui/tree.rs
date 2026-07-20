use crate::core::app_state::AppState;
use crate::db;
use crate::recent_folders::push_recent_folder_entry;
use crate::sort::sort_index_for_key;
use crate::thumbnail_sizing::{normalize_thumbnail_size, thumbnail_size_options};
use crate::tree_sidebar::{reset_tree_root, tree_root_path};
use crate::ui::left_chrome_wiring::LeftChromeWiring;
use gtk4::prelude::*;
use gtk4::{gio, glib, Image, Label, ListItem, ListView, Orientation, ScrolledWindow, TreeListRow};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub(crate) struct TreeWidgets {
    pub(crate) left_sidebar: gtk4::Box,
    pub(crate) tree_root: gio::ListStore,
    pub(crate) tree_model: gtk4::TreeListModel,
    pub(crate) tree_selection: gtk4::SingleSelection,
    pub(crate) tree_list_view: ListView,
}

pub(crate) fn build_tree_widgets(
    last_folder: Option<&PathBuf>,
    initial_left_sidebar_visible: bool,
) -> TreeWidgets {
    let left_sidebar = gtk4::Box::new(Orientation::Vertical, 0);
    left_sidebar.set_width_request(200);
    left_sidebar.set_visible(initial_left_sidebar_visible);

    let tree_root = crate::tree_sidebar::build_tree_root(last_folder);
    let tree_model = gtk4::TreeListModel::new(
        tree_root.clone(),
        false,
        false,
        move |item: &glib::Object| -> Option<gio::ListModel> {
            let file = item.downcast_ref::<gio::File>()?;
            let store = gio::ListStore::new::<gio::File>();
            if let Ok(enumerator) = file.enumerate_children(
                "standard::name,standard::type",
                gio::FileQueryInfoFlags::NONE,
                None::<&gio::Cancellable>,
            ) {
                let mut children: Vec<gio::FileInfo> = enumerator
                    .filter_map(|r| r.ok())
                    .filter(|info| {
                        info.file_type() == gio::FileType::Directory
                            && !info.name().to_string_lossy().starts_with('.')
                    })
                    .collect();
                children
                    .sort_by_key(|info| info.name().to_string_lossy().to_lowercase().to_string());
                for info in children {
                    store.append(&file.child(info.name()));
                }
            }
            if store.n_items() > 0 {
                Some(store.upcast::<gio::ListModel>())
            } else {
                None
            }
        },
    );
    let tree_selection = gtk4::SingleSelection::new(Some(tree_model.clone()));

    let tree_factory = gtk4::SignalListItemFactory::new();
    tree_factory.connect_setup(|_, obj| {
        let Some(list_item) = obj.downcast_ref::<ListItem>() else {
            return;
        };
        let expander = gtk4::TreeExpander::new();
        let row_box = gtk4::Box::new(Orientation::Horizontal, 4);
        row_box.set_margin_top(3);
        row_box.set_margin_bottom(3);
        let icon = Image::from_icon_name("folder-symbolic");
        let label = Label::new(None);
        label.set_halign(gtk4::Align::Start);
        label.set_hexpand(true);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
        row_box.append(&icon);
        row_box.append(&label);
        expander.set_child(Some(&row_box));
        list_item.set_child(Some(&expander));
    });
    tree_factory.connect_bind(|_, obj| {
        let Some(list_item) = obj.downcast_ref::<ListItem>() else {
            return;
        };
        let Some(expander) = list_item.child().and_downcast::<gtk4::TreeExpander>() else {
            return;
        };
        let Some(row) = list_item.item().and_downcast::<TreeListRow>() else {
            expander.set_list_row(None::<&gtk4::TreeListRow>);
            return;
        };
        expander.set_list_row(Some(&row));
        let Some(file) = row.item().and_downcast::<gio::File>() else {
            return;
        };
        let Some(row_box) = expander.child().and_downcast::<gtk4::Box>() else {
            return;
        };
        let Some(label) = row_box.last_child().and_downcast::<Label>() else {
            return;
        };
        let name = if let Some(p) = file.path() {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.to_string_lossy().into_owned())
        } else {
            file.uri().to_string()
        };
        label.set_text(&name);
    });
    tree_factory.connect_unbind(|_, obj| {
        let Some(list_item) = obj.downcast_ref::<ListItem>() else {
            return;
        };
        let Some(expander) = list_item.child().and_downcast::<gtk4::TreeExpander>() else {
            return;
        };
        expander.set_list_row(None::<&gtk4::TreeListRow>);
    });

    let tree_list_view = ListView::new(Some(tree_selection.clone()), Some(tree_factory));
    tree_list_view.add_css_class("navigation-sidebar");
    // Disable natural-width propagation so the ScrolledWindow can clip the
    // ListView and show a horizontal scrollbar for deeply-nested long names.
    tree_list_view.set_hexpand(false);

    let tree_scroll = ScrolledWindow::new();
    tree_scroll.set_vexpand(true);
    tree_scroll.set_hscrollbar_policy(gtk4::PolicyType::Automatic);
    tree_scroll.set_propagate_natural_width(false);
    tree_scroll.set_child(Some(&tree_list_view));
    left_sidebar.append(&tree_scroll);

    TreeWidgets {
        left_sidebar,
        tree_root,
        tree_model,
        tree_selection,
        tree_list_view,
    }
}

pub(crate) struct TreeFolderSelectionDeps {
    pub(crate) app_state: AppState,
    pub(crate) chrome: LeftChromeWiring,
    pub(crate) filter: gtk4::CustomFilter,
    pub(crate) start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    pub(crate) recent_folders_limit: usize,
}

struct BrowseFolderCtx {
    current_folder: Rc<RefCell<Option<PathBuf>>>,
    recent_folders: Rc<RefCell<Vec<PathBuf>>>,
    tree_root: gio::ListStore,
    sort_key: Rc<RefCell<String>>,
    search_text: Rc<RefCell<String>>,
    favorites_only: Rc<Cell<bool>>,
    active_tag_filters: Rc<RefCell<std::collections::HashMap<String, crate::db::TagFilterMode>>>,
    tag_filter_debounce_gen: Rc<Cell<u64>>,
    thumbnail_size: Rc<RefCell<i32>>,
    sort_dropdown: gtk4::DropDown,
    favourites_filter_btn: gtk4::ToggleButton,
    tags_filter_btn: gtk4::MenuButton,
    tags_filter_list: gtk4::Box,
    search_entry: gtk4::SearchEntry,
    filter: gtk4::CustomFilter,
    size_buttons: Rc<Vec<gtk4::ToggleButton>>,
    progress_state: Rc<RefCell<crate::ScanProgressState>>,
    start_scan: Rc<dyn Fn(PathBuf)>,
    recent_limit: usize,
    grid_loading: Rc<RefCell<Option<crate::ui::grid_loading::GridLoadingOverlay>>>,
}

/// Restore/seed per-folder UI state, set current folder, update recent list, and scan.
/// When `persist_as_root` is true, `last_folder` is written as `path`; otherwise the
/// existing tree root path is preserved in config.
fn browse_folder(ctx: &BrowseFolderCtx, path: &Path, persist_as_root: bool) {
    if let Some(saved_ui_state) = db::load_ui_state(path) {
        let selected_sort = sort_index_for_key(&saved_ui_state.sort_key);
        *ctx.sort_key.borrow_mut() = saved_ui_state.sort_key;
        *ctx.search_text.borrow_mut() = saved_ui_state.search_text.clone();
        ctx.favorites_only.set(saved_ui_state.favorites_only);
        *ctx.active_tag_filters.borrow_mut() = saved_ui_state.active_tag_filters.clone();
        ctx.tag_filter_debounce_gen.set(0);
        *ctx.thumbnail_size.borrow_mut() = normalize_thumbnail_size(saved_ui_state.thumbnail_size);

        if ctx.sort_dropdown.selected() != selected_sort {
            ctx.sort_dropdown.set_selected(selected_sort);
        }
        ctx.favourites_filter_btn
            .set_active(saved_ui_state.favorites_only);
        if saved_ui_state.favorites_only {
            ctx.favourites_filter_btn
                .add_css_class("favorites-filter-active");
        } else {
            ctx.favourites_filter_btn
                .remove_css_class("favorites-filter-active");
        }
        ctx.search_entry.set_text(&saved_ui_state.search_text);
        for (i, btn) in ctx.size_buttons.iter().enumerate() {
            btn.set_active(thumbnail_size_options()[i] == *ctx.thumbnail_size.borrow());
        }
    } else {
        ctx.active_tag_filters.borrow_mut().clear();
        ctx.tag_filter_debounce_gen.set(0);
        let seeded_state = db::UiState {
            sort_key: ctx.sort_key.borrow().clone(),
            search_text: ctx.search_text.borrow().clone(),
            favorites_only: ctx.favorites_only.get(),
            active_tag_filters: std::collections::HashMap::new(),
            thumbnail_size: *ctx.thumbnail_size.borrow(),
        };
        let _ = db::save_ui_state(path, &seeded_state);
    }

    *ctx.current_folder.borrow_mut() = Some(path.to_path_buf());
    ctx.progress_state.borrow_mut().current_folder_path = path.display().to_string();
    {
        let mut history = ctx.recent_folders.borrow_mut();
        push_recent_folder_entry(&mut history, path, ctx.recent_limit);
        let root_owned = tree_root_path(&ctx.tree_root);
        let last_folder = if persist_as_root {
            Some(path)
        } else {
            root_owned.as_deref().or(Some(path))
        };
        crate::config::save_recent_state(last_folder, &history);
    }
    crate::ui::controls::refresh_tag_filter_from_folder(
        &ctx.tags_filter_list,
        &ctx.tags_filter_btn,
        &ctx.active_tag_filters,
        &ctx.tag_filter_debounce_gen,
        &ctx.filter,
        &ctx.current_folder,
        &ctx.grid_loading,
    );
    (ctx.start_scan)(path.to_path_buf());
}

/// Wire tree folder selection → browse thumbnails; activate (double-click/Enter) → re-root.
pub(crate) fn install_tree_folder_selection(deps: TreeFolderSelectionDeps) {
    let ctx = Rc::new(BrowseFolderCtx {
        current_folder: deps.app_state.current_folder.clone(),
        recent_folders: deps.app_state.recent_folders.clone(),
        tree_root: deps.chrome.tree_root.clone(),
        sort_key: deps.app_state.sort_key.clone(),
        search_text: deps.app_state.search_text.clone(),
        favorites_only: deps.app_state.favorites_only.clone(),
        active_tag_filters: deps.app_state.active_tag_filters.clone(),
        tag_filter_debounce_gen: deps.app_state.tag_filter_debounce_gen.clone(),
        thumbnail_size: deps.app_state.thumbnail_size.clone(),
        sort_dropdown: deps.chrome.sort_dropdown.clone(),
        favourites_filter_btn: deps.chrome.favourites_filter_btn.clone(),
        tags_filter_btn: deps.chrome.tags_filter_btn.clone(),
        tags_filter_list: deps.chrome.tags_filter_list.clone(),
        search_entry: deps.chrome.search_entry.clone(),
        filter: deps.filter.clone(),
        size_buttons: deps.chrome.size_buttons.clone(),
        progress_state: deps.app_state.progress_state.clone(),
        start_scan: deps.start_scan_for_folder.clone(),
        recent_limit: deps.recent_folders_limit,
        grid_loading: deps.app_state.grid_loading.clone(),
    });

    let browse_ctx = ctx.clone();
    deps.chrome
        .tree_selection
        .connect_selection_changed(move |model, _, _| {
            let Some(row) = model.selected_item().and_downcast::<TreeListRow>() else {
                return;
            };
            let Some(file) = row.item().and_downcast::<gio::File>() else {
                return;
            };
            let Some(path) = file.path() else { return };
            if browse_ctx.current_folder.borrow().as_deref() == Some(path.as_path()) {
                return;
            }
            browse_folder(&browse_ctx, &path, false);
        });

    let activate_ctx = ctx;
    let tree_model = deps.chrome.tree_model.clone();
    deps.chrome.tree_list_view.connect_activate(move |_, pos| {
        let Some(row) = tree_model.item(pos).and_downcast::<TreeListRow>() else {
            return;
        };
        let Some(file) = row.item().and_downcast::<gio::File>() else {
            return;
        };
        let Some(path) = file.path() else { return };

        let root_owned = tree_root_path(&activate_ctx.tree_root);
        let already_root = root_owned.as_deref() == Some(path.as_path());
        if already_root {
            // Already the tree root; ensure thumbnails match if needed.
            if activate_ctx.current_folder.borrow().as_deref() != Some(path.as_path()) {
                browse_folder(&activate_ctx, &path, true);
            }
            return;
        }

        if activate_ctx.current_folder.borrow().as_deref() != Some(path.as_path()) {
            browse_folder(&activate_ctx, &path, true);
        } else {
            // Already browsing this folder — only re-root and persist.
            let mut history = activate_ctx.recent_folders.borrow_mut();
            push_recent_folder_entry(&mut history, path.as_path(), activate_ctx.recent_limit);
            crate::config::save_recent_state(Some(path.as_path()), &history);
        }
        reset_tree_root(&activate_ctx.tree_root, path.as_path());
    });
}
