use gtk4::prelude::*;
use gtk4::{gio, glib, Image, Label, ListItem, ListView, Orientation, ScrolledWindow};
use std::path::PathBuf;

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
                children.sort_by_key(|info| info.name().to_string_lossy().to_lowercase().to_string());
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
