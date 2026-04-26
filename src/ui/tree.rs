use crate::core::app_state::AppState;
use crate::db;
use crate::recent_folders::push_recent_folder_entry;
use crate::sort::sort_index_for_key;
use crate::thumbnail_sizing::{normalize_thumbnail_size, thumbnail_size_options};
use crate::tree_sidebar::reset_tree_root;
use crate::ui::left_chrome_wiring::LeftChromeWiring;
use gtk4::prelude::*;
use gtk4::{gio, glib, Image, Label, ListItem, ListView, Orientation, ScrolledWindow, TreeListRow};
use std::path::PathBuf;
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
        let Some(row) = list_item.item().and_downcast::<gtk4::TreeListRow>() else {
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
    pub(crate) start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    pub(crate) recent_folders_limit: usize,
}

/// Wire tree folder selection → restore/save UI state, recent list, tree root, and start scan.
pub(crate) fn install_tree_folder_selection(deps: TreeFolderSelectionDeps) {
    let current_folder = deps.app_state.current_folder.clone();
    let recent_folders = deps.app_state.recent_folders.clone();
    let tree_root = deps.chrome.tree_root.clone();
    let sort_key = deps.app_state.sort_key.clone();
    let search_text = deps.app_state.search_text.clone();
    let thumbnail_size = deps.app_state.thumbnail_size.clone();
    let sort_dropdown = deps.chrome.sort_dropdown.clone();
    let search_entry = deps.chrome.search_entry.clone();
    let size_buttons = deps.chrome.size_buttons.clone();
    let progress_state = deps.app_state.progress_state.clone();
    let start_scan = deps.start_scan_for_folder.clone();
    let recent_limit = deps.recent_folders_limit;

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
            if current_folder.borrow().as_deref() == Some(path.as_path()) {
                return;
            }

            if let Some(saved_ui_state) = db::load_ui_state(path.as_path()) {
                let selected_sort = sort_index_for_key(&saved_ui_state.sort_key);
                *sort_key.borrow_mut() = saved_ui_state.sort_key;
                *search_text.borrow_mut() = saved_ui_state.search_text.clone();
                *thumbnail_size.borrow_mut() =
                    normalize_thumbnail_size(saved_ui_state.thumbnail_size);

                if sort_dropdown.selected() != selected_sort {
                    sort_dropdown.set_selected(selected_sort);
                }
                search_entry.set_text(&saved_ui_state.search_text);
                for (i, btn) in size_buttons.iter().enumerate() {
                    btn.set_active(thumbnail_size_options()[i] == *thumbnail_size.borrow());
                }
            } else {
                let seeded_state = db::UiState {
                    sort_key: sort_key.borrow().clone(),
                    search_text: search_text.borrow().clone(),
                    thumbnail_size: *thumbnail_size.borrow(),
                };
                let _ = db::save_ui_state(path.as_path(), &seeded_state);
            }

            *current_folder.borrow_mut() = Some(path.clone());
            progress_state.borrow_mut().current_folder_path = path.display().to_string();
            {
                let mut history = recent_folders.borrow_mut();
                push_recent_folder_entry(&mut history, path.as_path(), recent_limit);
                crate::config::save_recent_state(Some(path.as_path()), &history);
            }
            reset_tree_root(&tree_root, path.as_path());
            start_scan(path);
        });
}
