mod metadata;
mod scanner;
mod thumbnails;

use metadata::{DefaultMetadataDispatcher, ImageMetadata, MetadataDispatcher, ScanMessage};
use scanner::scan_directory;

use libadwaita as adw;
use adw::prelude::*;
use gtk4::{
    gdk, gio, glib, EventControllerKey, GridView, Image, Label, ListItem, Orientation, Picture,
    ScrolledWindow, Separator, SignalListItemFactory, SingleSelection, Spinner, StringObject,
};

// ---------------------------------------------------------------------------
// UI construction
// ---------------------------------------------------------------------------

fn build_ui(app: &adw::Application) {
    let window = adw::ApplicationWindow::new(app);
    window.set_title(Some("LumenNode"));
    window.set_default_size(1280, 800);

    // Shared model: each item holds the absolute path of one image.
    let list_store = gio::ListStore::new::<StringObject>();

    // Async channel: background scan thread → GTK main thread.
    let (sender, receiver) = async_channel::unbounded::<ScanMessage>();

    // ViewStack is created early so the AdwViewSwitcher (in the header bar)
    // can reference it before the content area is assembled.
    let view_stack = adw::ViewStack::new();

    // Scan-progress indicator (shown in header while a scan is running).
    let spinner = Spinner::new();

    // Toast overlay wraps all main content for non-intrusive notifications.
    let toast_overlay = adw::ToastOverlay::new();

    // -----------------------------------------------------------------------
    // Receiver task: update model, manage spinner, show scan-complete toast
    // -----------------------------------------------------------------------
    let list_store_recv = list_store.clone();
    let spinner_recv = spinner.clone();
    let toast_recv = toast_overlay.clone();
    glib::MainContext::default().spawn_local(async move {
        while let Ok(msg) = receiver.recv().await {
            match msg {
                ScanMessage::ImageFound(path) => {
                    list_store_recv.append(&StringObject::new(&path));
                }
                ScanMessage::MetadataReady { .. } => {}
                ScanMessage::ScanComplete => {
                    spinner_recv.stop();
                    let n = list_store_recv.n_items();
                    let text = format!(
                        "Found {} image{}",
                        n,
                        if n == 1 { "" } else { "s" }
                    );
                    let toast = adw::Toast::new(&text);
                    toast.set_timeout(3);
                    toast_recv.add_toast(toast);
                }
            }
        }
    });

    // -----------------------------------------------------------------------
    // AdwHeaderBar — window chrome
    // -----------------------------------------------------------------------
    let header_bar = adw::HeaderBar::new();

    // "Open Folder" button in the start slot.
    let open_btn = gtk4::Button::from_icon_name("folder-open-symbolic");
    open_btn.set_tooltip_text(Some("Open Folder…"));
    header_bar.pack_start(&open_btn);

    // AdwViewSwitcher as the title widget — shows "Grid" and "Single" tabs.
    let view_switcher = adw::ViewSwitcher::new();
    view_switcher.set_stack(Some(&view_stack));
    header_bar.set_title_widget(Some(&view_switcher));

    // Spinner in the end slot — visible while scanning.
    header_bar.pack_end(&spinner);

    // -----------------------------------------------------------------------
    // Three-pane layout: [left sidebar] | [center] | [right sidebar]
    // -----------------------------------------------------------------------
    let root_box = gtk4::Box::new(Orientation::Horizontal, 0);

    // --- Left sidebar: current folder path ---
    let left_sidebar = gtk4::Box::new(Orientation::Vertical, 8);
    left_sidebar.set_width_request(220);
    left_sidebar.set_margin_top(12);
    left_sidebar.set_margin_bottom(12);
    left_sidebar.set_margin_start(8);
    left_sidebar.set_margin_end(4);

    let folders_label = Label::new(Some("Folders"));
    folders_label.add_css_class("title-4");
    folders_label.set_halign(gtk4::Align::Start);
    left_sidebar.append(&folders_label);

    let current_folder_label = Label::new(Some("No folder selected"));
    current_folder_label.set_wrap(true);
    current_folder_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    current_folder_label.set_halign(gtk4::Align::Start);
    current_folder_label.add_css_class("caption");
    left_sidebar.append(&current_folder_label);

    // --- Center: ViewStack with Grid + Single pages ---
    let selection_model = SingleSelection::new(Some(list_store.clone()));

    let factory = SignalListItemFactory::new();

    factory.connect_setup(|_, obj| {
        let list_item = obj.downcast_ref::<ListItem>().unwrap();
        let cell_box = gtk4::Box::new(Orientation::Vertical, 4);
        cell_box.set_halign(gtk4::Align::Center);
        cell_box.set_size_request(132, 148);
        let thumb_image = Image::new();
        thumb_image.set_pixel_size(128);
        let name_label = Label::new(None);
        name_label.set_max_width_chars(16);
        name_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        name_label.add_css_class("caption");
        cell_box.append(&thumb_image);
        cell_box.append(&name_label);
        list_item.set_child(Some(&cell_box));
    });

    factory.connect_bind(|_, obj| {
        let list_item = obj.downcast_ref::<ListItem>().unwrap();
        let path_str = list_item
            .item()
            .and_downcast::<StringObject>()
            .map(|s| s.string().to_string())
            .unwrap_or_default();

        let cell_box = list_item.child().and_downcast::<gtk4::Box>().unwrap();
        let thumb_image = cell_box.first_child().and_downcast::<Image>().unwrap();
        let name_label = cell_box.last_child().and_downcast::<Label>().unwrap();

        // Set filename label and placeholder icon synchronously (zero I/O).
        let filename = std::path::Path::new(&path_str)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        name_label.set_text(&filename);
        thumb_image.set_icon_name(Some("image-x-generic-symbolic"));

        // Tag the image widget with the path it's currently bound to.
        // The async callback uses this to detect stale (recycled) cells.
        unsafe { thumb_image.set_data("bound-path", path_str.clone()); }

        // Off-thread thumbnail generation via GLib's bounded thread pool.
        let path_for_thread = std::path::PathBuf::from(&path_str);
        let task =
            gio::spawn_blocking(move || thumbnails::ensure_thumbnail(&path_for_thread));

        let image_weak = thumb_image.downgrade();
        glib::MainContext::default().spawn_local(async move {
            let Ok(maybe_cache) = task.await else { return };
            let Some(image) = image_weak.upgrade() else { return };
            // Stale-update guard: skip if this cell was recycled to another item.
            let is_current = unsafe {
                image
                    .data::<String>("bound-path")
                    .map(|p| p.as_ref().as_str() == path_str.as_str())
                    .unwrap_or(false)
            };
            if !is_current {
                return;
            }
            match maybe_cache.and_then(|p| gdk_pixbuf::Pixbuf::from_file(&p).ok()) {
                Some(pb) => image.set_from_pixbuf(Some(&pb)),
                None => image.set_icon_name(Some("image-missing-symbolic")),
            }
        });
    });

    factory.connect_unbind(|_, obj| {
        let list_item = obj.downcast_ref::<ListItem>().unwrap();
        if let Some(cell_box) = list_item.child().and_downcast::<gtk4::Box>() {
            if let Some(image) = cell_box.first_child().and_downcast::<Image>() {
                unsafe { image.steal_data::<String>("bound-path"); }
                image.set_icon_name(Some("image-x-generic-symbolic"));
            }
        }
    });

    let grid_view = GridView::new(Some(selection_model.clone()), Some(factory));
    grid_view.set_max_columns(12);
    grid_view.set_min_columns(2);

    let grid_scroll = ScrolledWindow::new();
    grid_scroll.set_vexpand(true);
    grid_scroll.set_hexpand(true);
    grid_scroll.set_child(Some(&grid_view));

    // add_titled returns ViewStackPage — use it to set the page icon.
    let grid_page = view_stack.add_titled(&grid_scroll, Some("grid"), "Grid");
    grid_page.set_icon_name(Some("view-grid-symbolic"));

    let single_picture = Picture::new();
    single_picture.set_vexpand(true);
    single_picture.set_hexpand(true);
    single_picture.set_can_shrink(true);
    let single_page = view_stack.add_titled(&single_picture, Some("single"), "Single");
    single_page.set_icon_name(Some("view-fullscreen-symbolic"));
    view_stack.set_visible_child_name("grid");

    let center_box = gtk4::Box::new(Orientation::Vertical, 0);
    center_box.set_hexpand(true);
    center_box.append(&view_stack);

    // --- Right sidebar: metadata display ---
    let right_sidebar = gtk4::Box::new(Orientation::Vertical, 6);
    right_sidebar.set_width_request(260);
    right_sidebar.set_margin_top(12);
    right_sidebar.set_margin_bottom(12);
    right_sidebar.set_margin_start(4);
    right_sidebar.set_margin_end(8);

    let meta_title = Label::new(Some("Metadata"));
    meta_title.add_css_class("title-4");
    meta_title.set_halign(gtk4::Align::Start);
    right_sidebar.append(&meta_title);

    let meta_scroll = ScrolledWindow::new();
    meta_scroll.set_vexpand(true);
    let meta_listbox = gtk4::ListBox::new();
    meta_listbox.add_css_class("boxed-list");
    meta_listbox.set_selection_mode(gtk4::SelectionMode::None);
    meta_scroll.set_child(Some(&meta_listbox));
    right_sidebar.append(&meta_scroll);

    // -----------------------------------------------------------------------
    // Wire: grid item activate → switch to single view
    // -----------------------------------------------------------------------
    let stack_for_grid = view_stack.clone();
    let picture_for_grid = single_picture.clone();
    let selection_for_grid = selection_model.clone();
    grid_view.connect_activate(move |_, pos| {
        if let Some(item) = selection_for_grid.item(pos).and_downcast::<StringObject>() {
            let path = std::path::PathBuf::from(item.string().as_str());
            picture_for_grid.set_filename(Some(&path));
        }
        stack_for_grid.set_visible_child_name("single");
    });

    // -----------------------------------------------------------------------
    // Wire: selection change → populate metadata sidebar
    // -----------------------------------------------------------------------
    let meta_listbox_sel = meta_listbox.clone();
    selection_model.connect_selection_changed(move |model, _, _| {
        let Some(item) = model.selected_item().and_downcast::<StringObject>() else {
            return;
        };
        let path = std::path::PathBuf::from(item.string().as_str());
        let (tx, rx) = async_channel::bounded::<ImageMetadata>(1);
        std::thread::spawn(move || {
            let dispatcher = DefaultMetadataDispatcher;
            let meta = dispatcher.extract(&path).unwrap_or_default();
            let _ = tx.send_blocking(meta);
        });
        let listbox = meta_listbox_sel.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Ok(meta) = rx.recv().await {
                populate_metadata_sidebar(&listbox, &meta);
            }
        });
    });

    // -----------------------------------------------------------------------
    // Wire: open_btn → FileDialog → start scan
    // -----------------------------------------------------------------------
    let sender_btn = sender.clone();
    let list_store_btn = list_store.clone();
    let folder_label_btn = current_folder_label.clone();
    let window_ref = window.clone();
    let spinner_btn = spinner.clone();
    open_btn.connect_clicked(move |_| {
        let dialog = gtk4::FileDialog::builder().title("Choose a Folder").build();
        let sender2 = sender_btn.clone();
        let list_store2 = list_store_btn.clone();
        let label2 = folder_label_btn.clone();
        let spinner2 = spinner_btn.clone();
        dialog.select_folder(
            Some(&window_ref),
            None::<&gio::Cancellable>,
            move |result| {
                let Ok(file) = result else { return };
                let Some(path) = file.path() else { return };
                label2.set_text(&path.display().to_string());
                list_store2.remove_all();
                spinner2.start();
                scan_directory(path, sender2.clone());
            },
        );
    });

    // -----------------------------------------------------------------------
    // Assemble three-pane layout with visual separators
    // -----------------------------------------------------------------------
    root_box.append(&left_sidebar);
    root_box.append(&Separator::new(Orientation::Vertical));
    root_box.append(&center_box);
    root_box.append(&Separator::new(Orientation::Vertical));
    root_box.append(&right_sidebar);

    // Wrap content in ToastOverlay → ToolbarView → window
    toast_overlay.set_child(Some(&root_box));

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(&header_bar);
    toolbar_view.set_content(Some(&toast_overlay));

    window.set_content(Some(&toolbar_view));

    // -----------------------------------------------------------------------
    // Keyboard: Escape → back to grid view
    // -----------------------------------------------------------------------
    let key_controller = EventControllerKey::new();
    let stack_for_esc = view_stack.clone();
    key_controller.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            stack_for_esc.set_visible_child_name("grid");
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_controller);

    window.present();
}

// ---------------------------------------------------------------------------
// Metadata sidebar population
// ---------------------------------------------------------------------------

fn populate_metadata_sidebar(listbox: &gtk4::ListBox, meta: &ImageMetadata) {
    // Clear existing rows
    while let Some(child) = listbox.first_child() {
        listbox.remove(&child);
    }

    let rows: &[(&str, Option<&str>)] = &[
        ("Make", meta.camera_make.as_deref()),
        ("Model", meta.camera_model.as_deref()),
        ("Exposure", meta.exposure.as_deref()),
        ("ISO", meta.iso.as_deref()),
        ("Prompt", meta.prompt.as_deref()),
        ("Neg. Prompt", meta.negative_prompt.as_deref()),
        ("Parameters", meta.raw_parameters.as_deref()),
        ("Workflow", meta.workflow_json.as_deref()),
    ];

    for (key, maybe_val) in rows {
        let Some(val) = maybe_val else { continue };
        let row = adw::ActionRow::new();
        row.set_title(key);
        row.set_subtitle(val);
        row.set_subtitle_selectable(true);
        listbox.append(&row);
    }

    if listbox.first_child().is_none() {
        let empty = adw::ActionRow::new();
        empty.set_title("No metadata found");
        listbox.append(&empty);
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

fn main() -> glib::ExitCode {
    let app = adw::Application::builder()
        .application_id("com.lumennode.app")
        .build();
    app.connect_activate(build_ui);
    app.run()
}
