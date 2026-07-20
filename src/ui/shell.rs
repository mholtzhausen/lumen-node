use crate::config::{AppConfig, ColorSchemePref};
use crate::window_math::monitor_bounds_for_window;
use adw::prelude::*;
use gtk4::prelude::*;
use gtk4::{gio, Orientation, Paned, ProgressBar};
use libadwaita as adw;
use std::{cell::Cell, cell::RefCell, path::PathBuf, rc::Rc};

pub(crate) fn apply_color_scheme_pref(pref: ColorSchemePref) {
    let scheme = match pref {
        ColorSchemePref::System => adw::ColorScheme::Default,
        ColorSchemePref::Light => adw::ColorScheme::ForceLight,
        ColorSchemePref::Dark => adw::ColorScheme::ForceDark,
    };
    adw::StyleManager::default().set_color_scheme(scheme);
}

pub(crate) fn sync_theme_button(btn: &gtk4::Button, pref: ColorSchemePref) {
    btn.set_icon_name(pref.icon_name());
    btn.set_tooltip_text(Some(pref.tooltip()));
}

const THUMBNAIL_CHROME_BASE_PX: f64 = 28.0;
const THUMBNAIL_CHROME_PAD_BASE_PX: f64 = 2.0;
const THUMBNAIL_CHROME_MARGIN_BASE_PX: f64 = 4.0;

/// Pixel size for grid chrome buttons at the given scale.
pub(crate) fn thumbnail_chrome_button_px(scale: f64) -> i32 {
    let scale = crate::config::normalize_thumbnail_chrome_scale(scale);
    ((THUMBNAIL_CHROME_BASE_PX * scale).round() as i32).clamp(12, 28)
}

/// Updates the live CSS metrics for grid thumbnail chrome (favourite + tag buttons).
/// GTK CSS does not support max-width/max-height or logical margin-end — keep buttons
/// square with widget `set_size_request` instead (see `thumbnail_chrome_button_px`).
pub(crate) fn apply_thumbnail_chrome_scale(provider: &gtk4::CssProvider, scale: f64) {
    let scale = crate::config::normalize_thumbnail_chrome_scale(scale);
    let btn_px = thumbnail_chrome_button_px(scale);
    let pad_px = ((THUMBNAIL_CHROME_PAD_BASE_PX * scale).round() as i32).max(1);
    let margin_px = ((THUMBNAIL_CHROME_MARGIN_BASE_PX * scale).round() as i32).max(1);
    provider.load_from_string(&format!(
        "
        .thumbnail-chrome-button {{
            min-width: {btn_px}px;
            min-height: {btn_px}px;
            padding: {pad_px}px;
        }}
        .thumbnail-chrome-pane {{
            margin-top: {margin_px}px;
            margin-right: {margin_px}px;
        }}
        .thumbnail-chrome-button.thumbnail-tag-active {{
            color: @accent_color;
        }}
        "
    ));
}

fn install_thumbnail_chrome_css(
    window: &adw::ApplicationWindow,
    scale: f64,
) -> gtk4::CssProvider {
    let provider = gtk4::CssProvider::new();
    apply_thumbnail_chrome_scale(&provider, scale);
    gtk4::style_context_add_provider_for_display(
        &gtk4::prelude::WidgetExt::display(window),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    provider
}

pub(crate) struct PanedLayout {
    pub(crate) inner_paned: Paned,
    pub(crate) outer_paned: Paned,
    pub(crate) inner_position_programmatic: Rc<Cell<u32>>,
    pub(crate) inner_split_dirty: Rc<Cell<bool>>,
    pub(crate) outer_position_programmatic: Rc<Cell<u32>>,
    pub(crate) outer_split_dirty: Rc<Cell<bool>>,
}

pub(crate) struct HeaderControls {
    pub(crate) header_bar: adw::HeaderBar,
    pub(crate) controls_row: gtk4::Box,
    pub(crate) sort_dropdown: gtk4::DropDown,
    pub(crate) size_buttons: Rc<Vec<gtk4::ToggleButton>>,
    pub(crate) favourites_filter_btn: gtk4::ToggleButton,
    pub(crate) tags_filter_btn: gtk4::MenuButton,
    pub(crate) tags_filter_list: gtk4::Box,
    pub(crate) search_entry: gtk4::SearchEntry,
    pub(crate) clear_btn: gtk4::Button,
    pub(crate) similar_filter_btn: gtk4::Button,
    pub(crate) left_toggle: gtk4::ToggleButton,
    pub(crate) right_toggle: gtk4::ToggleButton,
    pub(crate) open_btn: gtk4::Button,
    pub(crate) history_list: gtk4::Box,
    pub(crate) history_popover: gtk4::Popover,
    pub(crate) initial_left_sidebar_visible: bool,
    pub(crate) initial_right_sidebar_visible: bool,
    pub(crate) color_scheme: Rc<Cell<ColorSchemePref>>,
}

pub(crate) fn build_header_controls(
    app_config: &AppConfig,
    initial_thumbnail_size: i32,
    window: &adw::ApplicationWindow,
    runtime_report: String,
    app_state: crate::core::app_state::AppState,
    thumbnail_chrome_css: gtk4::CssProvider,
) -> HeaderControls {
    let header_bar = adw::HeaderBar::new();

    let menu_model = gio::Menu::new();

    // ── File menu ──────────────────────────────────────────────────────────
    let file_menu = gio::Menu::new();
    file_menu.append(Some("Open Folder..."), Some("win.open-folder"));
    let about_section = gio::Menu::new();
    about_section.append(Some("About LumenNode"), Some("win.about"));
    file_menu.append_section(None, &about_section);
    menu_model.append_submenu(Some("File"), &file_menu);

    // ── Edit menu ──────────────────────────────────────────────────────────
    let edit_menu = gio::Menu::new();
    let copy_section = gio::Menu::new();
    copy_section.append(Some("Copy Prompt"), Some("ctx.copy-prompt"));
    copy_section.append(Some("Copy Negative Prompt"), Some("ctx.copy-negative-prompt"));
    copy_section.append(Some("Copy Seed"), Some("ctx.copy-seed"));
    copy_section.append(Some("Copy Generation Command"), Some("ctx.copy-generation-command"));
    copy_section.append(Some("Similar in folder"), Some("ctx.show-similar"));
    edit_menu.append_section(None, &copy_section);

    let clipboard_section = gio::Menu::new();
    clipboard_section.append(Some("Copy Image"), Some("ctx.copy-image"));
    clipboard_section.append(Some("Copy Path"), Some("ctx.copy-path"));
    clipboard_section.append(Some("Copy Metadata"), Some("ctx.copy-metadata"));
    edit_menu.append_section(None, &clipboard_section);

    let organise_section = gio::Menu::new();
    organise_section.append(Some("Favourite"), Some("ctx.toggle-favourite"));
    organise_section.append(Some("Move to Trash"), Some("ctx.move-to-trash"));
    edit_menu.append_section(None, &organise_section);

    let config_section = gio::Menu::new();
    config_section.append(Some("Preferences…"), Some("win.preferences"));
    config_section.append(Some("Get Config"), Some("win.get-config"));
    edit_menu.append_section(None, &config_section);

    menu_model.append_submenu(Some("Edit"), &edit_menu);

    // ── View menu ──────────────────────────────────────────────────────────
    let view_menu = gio::Menu::new();
    let refresh_submenu = gio::Menu::new();
    refresh_submenu.append(Some("Refresh Thumbnail"), Some("ctx.refresh-thumbnail"));
    refresh_submenu.append(Some("Refresh Metadata"), Some("ctx.refresh-metadata"));
    refresh_submenu.append(Some("Refresh Folder Thumbnails"), Some("ctx.refresh-folder-thumbnails"));
    refresh_submenu.append(Some("Refresh Folder Metadata"), Some("ctx.refresh-folder-metadata"));
    view_menu.append_submenu(Some("Refresh"), &refresh_submenu);

    let open_section = gio::Menu::new();
    open_section.append(Some("Open in File Manager"), Some("ctx.open-in-file-manager"));
    open_section.append(Some("Open in External Editor"), Some("ctx.open-in-external-editor"));
    view_menu.append_section(None, &open_section);

    menu_model.append_submenu(Some("View"), &view_menu);

    // ── Help menu ────────────────────────────────────────────────────────────
    let help_menu = gio::Menu::new();
    help_menu.append(Some("Keyboard Shortcuts"), Some("win.show-shortcuts"));
    menu_model.append_submenu(Some("Help"), &help_menu);

    let menubar = gtk4::PopoverMenuBar::from_model(Some(&menu_model));
    menubar.set_halign(gtk4::Align::Start);
    menubar.set_valign(gtk4::Align::Center);

    let get_config_action = gio::SimpleAction::new("get-config", None);
    let window_for_dialog = window.clone();
    get_config_action.connect_activate(move |_, _| {
        let dialog = gtk4::Window::builder()
            .transient_for(&window_for_dialog)
            .modal(true)
            .title("Runtime Config")
            .default_width(820)
            .default_height(480)
            .build();

        let text_view = gtk4::TextView::new();
        text_view.set_editable(false);
        text_view.set_cursor_visible(true);
        text_view.set_monospace(true);
        text_view.set_wrap_mode(gtk4::WrapMode::WordChar);
        text_view.buffer().set_text(&runtime_report);

        let scroll = gtk4::ScrolledWindow::new();
        scroll.set_hexpand(true);
        scroll.set_vexpand(true);
        scroll.set_margin_top(8);
        scroll.set_margin_bottom(8);
        scroll.set_margin_start(8);
        scroll.set_margin_end(8);
        scroll.set_child(Some(&text_view));

        let close_btn = gtk4::Button::with_label("Close");
        close_btn.set_halign(gtk4::Align::End);
        close_btn.set_margin_start(8);
        close_btn.set_margin_end(4);
        close_btn.set_margin_bottom(8);
        let dialog_for_close = dialog.clone();
        close_btn.connect_clicked(move |_| dialog_for_close.close());

        let copy_btn = gtk4::Button::with_label("Copy to Clipboard");
        copy_btn.set_halign(gtk4::Align::End);
        copy_btn.set_margin_start(4);
        copy_btn.set_margin_end(8);
        copy_btn.set_margin_bottom(8);
        let report_for_copy = runtime_report.clone();
        copy_btn.connect_clicked(move |_| {
            if let Some(display) = gtk4::gdk::Display::default() {
                display.clipboard().set_text(&report_for_copy);
            }
        });

        let button_row = gtk4::Box::new(Orientation::Horizontal, 6);
        button_row.set_halign(gtk4::Align::End);
        button_row.append(&copy_btn);
        button_row.append(&close_btn);

        let content = gtk4::Box::new(Orientation::Vertical, 0);
        content.append(&scroll);
        content.append(&button_row);
        dialog.set_child(Some(&content));
        dialog.present();
    });
    window.add_action(&get_config_action);

    // ── About dialog action ──────────────────────────────────────────────────
    let about_action = gio::SimpleAction::new("about", None);
    let window_for_about = window.clone();
    about_action.connect_activate(move |_, _| {
        let dialog = gtk4::AboutDialog::new();
        dialog.set_transient_for(Some(&window_for_about));
        dialog.set_modal(true);
        dialog.set_program_name(Some("LumenNode"));
        dialog.set_version(Some(env!("CARGO_PKG_VERSION")));
        dialog.set_comments(Some("A desktop image gallery for AI-generated art, featuring per-folder indexing, metadata extraction, and thumbnail caching."));
        dialog.set_website(Some("https://github.com/mholtzhausen/lumen-node"));
        dialog.set_website_label("GitHub Project");
        dialog.set_license_type(gtk4::License::MitX11);
        dialog.set_authors(&["mholtzhausen"]);
        dialog.set_logo_icon_name(Some("com.lumennode.app"));
        dialog.present();
    });
    window.add_action(&about_action);

    let show_shortcuts_action = gio::SimpleAction::new("show-shortcuts", None);
    let window_for_shortcuts = window.clone();
    show_shortcuts_action.connect_activate(move |_, _| {
        crate::ui::shortcuts::present_shortcuts_window(&window_for_shortcuts);
    });
    window.add_action(&show_shortcuts_action);
    if let Some(app) = window.application() {
        app.set_accels_for_action("win.show-shortcuts", &["<Primary>question"]);
    }

    let sort_options =
        gtk4::StringList::new(&["Name ↑", "Name ↓", "Date ↑", "Date ↓", "Size ↑", "Size ↓"]);
    let sort_dropdown = gtk4::DropDown::new(Some(sort_options), gtk4::Expression::NONE);
    sort_dropdown.set_tooltip_text(Some("Sort order"));

    let size_options = crate::thumbnail_sizing::thumbnail_size_options();
    let size_selector = gtk4::Box::new(Orientation::Horizontal, 0);
    size_selector.add_css_class("linked");
    size_selector.set_tooltip_text(Some("Thumbnail size"));
    let size_labels = ["1x", "1.3x", "1.6x", "1.9x"];
    let mut size_buttons_vec = Vec::new();
    for (idx, px) in size_options.iter().enumerate() {
        let btn = gtk4::ToggleButton::with_label(size_labels[idx]);
        btn.set_tooltip_text(Some(&format!("{} px", px)));
        btn.set_active(*px == initial_thumbnail_size);
        size_selector.append(&btn);
        size_buttons_vec.push(btn);
    }
    let size_buttons = Rc::new(size_buttons_vec);

    let search_entry = gtk4::SearchEntry::new();
    search_entry.set_placeholder_text(Some("Search filename, prompt, or tag…"));
    search_entry.set_width_request(220);
    search_entry.set_hexpand(true);

    let favourites_filter_btn = gtk4::ToggleButton::new();
    favourites_filter_btn.set_icon_name(crate::icons::STARRED);
    favourites_filter_btn.set_tooltip_text(Some("Show favourites only"));
    favourites_filter_btn.add_css_class("flat");

    let tags_filter_btn = gtk4::MenuButton::new();
    tags_filter_btn.set_icon_name(crate::icons::TAG_ICON_NAME);
    tags_filter_btn.set_tooltip_text(Some("Filter by tags"));
    tags_filter_btn.add_css_class("flat");
    let tags_filter_popover = gtk4::Popover::new();
    let tags_filter_list = gtk4::Box::new(Orientation::Vertical, 4);
    tags_filter_list.set_margin_top(8);
    tags_filter_list.set_margin_bottom(8);
    tags_filter_list.set_margin_start(8);
    tags_filter_list.set_margin_end(8);
    let tags_scroll = gtk4::ScrolledWindow::new();
    tags_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    tags_scroll.set_max_content_height(240);
    tags_scroll.set_propagate_natural_height(true);
    tags_scroll.set_child(Some(&tags_filter_list));
    tags_filter_popover.set_child(Some(&tags_scroll));
    tags_filter_btn.set_popover(Some(&tags_filter_popover));

    let clear_btn = gtk4::Button::from_icon_name(crate::icons::CLEAR);
    clear_btn.set_tooltip_text(Some("Clear filters"));

    let similar_filter_btn = gtk4::Button::from_icon_name(crate::icons::CLEAR);
    similar_filter_btn.set_tooltip_text(Some("Clear similar filter"));
    similar_filter_btn.add_css_class("flat");

    let controls_row = gtk4::Box::new(Orientation::Horizontal, 6);
    controls_row.set_halign(gtk4::Align::Fill);
    controls_row.set_hexpand(true);
    controls_row.set_margin_start(8);
    controls_row.set_margin_end(8);
    controls_row.set_margin_top(4);
    controls_row.set_margin_bottom(4);
    let left_toggle = gtk4::ToggleButton::new();
    left_toggle.set_icon_name(crate::icons::SIDEBAR_LEFT);
    let initial_left_sidebar_visible = app_config.left_sidebar_visible.unwrap_or(false);
    left_toggle.set_active(initial_left_sidebar_visible);
    left_toggle.set_tooltip_text(Some("Toggle left panel"));

    let open_btn = gtk4::Button::from_icon_name(crate::icons::FOLDER_OPEN);
    open_btn.set_tooltip_text(Some("Open Folder..."));

    let history_btn = gtk4::MenuButton::new();
    history_btn.set_icon_name(crate::icons::RECENT);
    history_btn.set_tooltip_text(Some("Recent folders"));
    let history_popover = gtk4::Popover::new();
    let history_list = gtk4::Box::new(Orientation::Vertical, 0);
    history_list.set_margin_top(6);
    history_list.set_margin_bottom(6);
    history_list.set_margin_start(6);
    history_list.set_margin_end(6);
    history_popover.set_child(Some(&history_list));
    history_btn.set_popover(Some(&history_popover));

    controls_row.append(&left_toggle);
    controls_row.append(&open_btn);
    controls_row.append(&history_btn);
    controls_row.append(&sort_dropdown);
    controls_row.append(&size_selector);
    controls_row.append(&favourites_filter_btn);
    controls_row.append(&tags_filter_btn);
    controls_row.append(&similar_filter_btn);
    controls_row.append(&search_entry);
    controls_row.append(&clear_btn);
    header_bar.pack_start(&menubar);

    let right_toggle = gtk4::ToggleButton::new();
    right_toggle.set_icon_name(crate::icons::SIDEBAR_RIGHT);
    let initial_right_sidebar_visible = app_config.right_sidebar_visible.unwrap_or(true);
    right_toggle.set_active(initial_right_sidebar_visible);
    right_toggle.set_tooltip_text(Some("Toggle right panel"));
    header_bar.pack_end(&right_toggle);

    let initial_color_scheme = app_config.color_scheme.unwrap_or(ColorSchemePref::System);
    apply_color_scheme_pref(initial_color_scheme);
    let color_scheme = Rc::new(Cell::new(initial_color_scheme));
    let theme_btn = gtk4::Button::from_icon_name(initial_color_scheme.icon_name());
    theme_btn.add_css_class("flat");
    sync_theme_button(&theme_btn, initial_color_scheme);
    let color_scheme_click = color_scheme.clone();
    theme_btn.connect_clicked(move |btn| {
        let next = color_scheme_click.get().next();
        color_scheme_click.set(next);
        apply_color_scheme_pref(next);
        sync_theme_button(btn, next);
        crate::config::save_color_scheme(next);
    });
    // Packed after right_toggle so it sits immediately left of the pane toggle.
    header_bar.pack_end(&theme_btn);

    let preferences_action = gio::SimpleAction::new("preferences", None);
    let window_for_prefs = window.clone();
    let color_scheme_prefs = color_scheme.clone();
    let theme_btn_prefs = theme_btn.clone();
    let chrome_scale_prefs = app_state.thumbnail_chrome_scale.clone();
    let chrome_css_prefs = thumbnail_chrome_css.clone();
    let app_state_prefs = app_state.clone();
    preferences_action.connect_activate(move |_, _| {
        crate::ui::preferences::present_preferences_window(
            &window_for_prefs,
            crate::ui::preferences::PreferencesDeps {
                color_scheme: color_scheme_prefs.clone(),
                theme_btn: theme_btn_prefs.clone(),
                thumbnail_chrome_scale: chrome_scale_prefs.clone(),
                thumbnail_chrome_css: chrome_css_prefs.clone(),
                app_state: app_state_prefs.clone(),
            },
        );
    });
    window.add_action(&preferences_action);
    if let Some(app) = window.application() {
        app.set_accels_for_action("win.preferences", &["<Primary>comma"]);
    }

    HeaderControls {
        header_bar,
        controls_row,
        sort_dropdown,
        size_buttons,
        favourites_filter_btn,
        tags_filter_btn,
        tags_filter_list,
        search_entry,
        clear_btn,
        similar_filter_btn,
        left_toggle,
        right_toggle,
        open_btn,
        history_list,
        history_popover,
        initial_left_sidebar_visible,
        initial_right_sidebar_visible,
        color_scheme,
    }
}

pub(crate) fn create_window_with_defaults(
    app: &adw::Application,
    app_config: &AppConfig,
    default_window_width: i32,
    default_window_height: i32,
    min_left_pane_px: i32,
    min_center_pane_px: i32,
    min_right_pane_px: i32,
    min_meta_split_px: i32,
) -> (adw::ApplicationWindow, gtk4::CssProvider) {
    let window = adw::ApplicationWindow::new(app);
    window.set_title(Some("LumenNode"));

    let (monitor_width, monitor_height) = monitor_bounds_for_window(&window);
    let min_window_width = min_left_pane_px + min_center_pane_px + min_right_pane_px;
    let min_window_height = (min_meta_split_px * 2).max(360);
    let initial_window_width = app_config
        .window_width
        .unwrap_or(default_window_width)
        .clamp(min_window_width, monitor_width.max(min_window_width));
    let initial_window_height = app_config
        .window_height
        .unwrap_or(default_window_height)
        .clamp(min_window_height, monitor_height.max(min_window_height));
    window.set_default_size(initial_window_width, initial_window_height);

    let css = gtk4::CssProvider::new();
    css.load_from_string(
        "
        .scroll-flag-bubble {
            background-color: alpha(@theme_bg_color, 0.86);
            border-radius: 8px;
            padding: 6px 12px;
        }
        .scroll-flag-pointer {
            color: alpha(@theme_fg_color, 0.95);
        }
        .zoom-level-hud {
            background-color: alpha(@theme_bg_color, 0.86);
            border-radius: 8px;
            padding: 6px 12px;
        }
        .grid-loading-overlay {
            background-color: @window_bg_color;
        }
        .thumbnail-card {
            background-color: alpha(@theme_fg_color, 0.04);
            border-radius: 8px;
            padding: 4px;
        }
        .thumbnail-favourite-button {
            background-color: alpha(@theme_bg_color, 0.62);
            color: alpha(@theme_fg_color, 0.90);
            min-width: 28px;
            min-height: 28px;
            padding: 2px;
        }
        .thumbnail-favourite-active {
            color: @accent_color;
        }
        .favorites-filter-active,
        .tags-filter-active,
        .similar-filter-active {
            color: @accent_color;
        }
        gridview > child {
            background-color: transparent;
            border-color: transparent;
            box-shadow: none;
        }
        gridview > child:hover {
            background-color: transparent;
        }
        gridview > child:selected {
            background-color: transparent;
        }
        gridview > child:hover .thumbnail-card {
            background-color: alpha(@theme_fg_color, 0.10);
            box-shadow: 0 2px 6px alpha(black, 0.14);
        }
        gridview > child:selected .thumbnail-card {
            background-color: alpha(@accent_bg_color, 0.28);
        }
        ",
    );
    gtk4::style_context_add_provider_for_display(
        &gtk4::prelude::WidgetExt::display(&window),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let chrome_scale = app_config
        .thumbnail_chrome_scale
        .map(crate::config::normalize_thumbnail_chrome_scale)
        .unwrap_or(crate::config::DEFAULT_THUMBNAIL_CHROME_SCALE);
    let chrome_css = install_thumbnail_chrome_css(&window, chrome_scale);

    (window, chrome_css)
}

pub(crate) fn assemble_paned_layout(
    left_sidebar: &gtk4::Box,
    center_box: &gtk4::Box,
    right_sidebar: &gtk4::Box,
    pane_restore_complete: &Rc<Cell<bool>>,
    left_pane_start_px: i32,
    inner_pane_start_px: i32,
) -> PanedLayout {
    let inner_paned = Paned::new(Orientation::Horizontal);
    inner_paned.set_start_child(Some(center_box));
    inner_paned.set_end_child(Some(right_sidebar));
    inner_paned.set_resize_start_child(true);
    inner_paned.set_resize_end_child(false);
    inner_paned.set_shrink_start_child(false);
    inner_paned.set_shrink_end_child(false);
    let inner_position_programmatic = Rc::new(Cell::new(0_u32));
    let inner_split_dirty = Rc::new(Cell::new(false));
    inner_position_programmatic.set(inner_position_programmatic.get().saturating_add(1));
    inner_paned.set_position(inner_pane_start_px);
    inner_position_programmatic.set(inner_position_programmatic.get().saturating_sub(1));
    {
        let inner_position_programmatic = inner_position_programmatic.clone();
        let inner_split_dirty = inner_split_dirty.clone();
        let pane_restore_complete = pane_restore_complete.clone();
        inner_paned.connect_notify_local(Some("position"), move |_, _| {
            if !pane_restore_complete.get() {
                return;
            }
            if inner_position_programmatic.get() != 0 {
                return;
            }
            inner_split_dirty.set(true);
        });
    }

    let outer_paned = Paned::new(Orientation::Horizontal);
    outer_paned.set_start_child(Some(left_sidebar));
    outer_paned.set_end_child(Some(&inner_paned));
    outer_paned.set_resize_start_child(false);
    outer_paned.set_resize_end_child(true);
    outer_paned.set_shrink_start_child(false);
    outer_paned.set_shrink_end_child(false);
    let outer_position_programmatic = Rc::new(Cell::new(0_u32));
    let outer_split_dirty = Rc::new(Cell::new(false));
    outer_position_programmatic.set(outer_position_programmatic.get().saturating_add(1));
    outer_paned.set_position(left_pane_start_px);
    outer_position_programmatic.set(outer_position_programmatic.get().saturating_sub(1));
    {
        let outer_position_programmatic = outer_position_programmatic.clone();
        let outer_split_dirty = outer_split_dirty.clone();
        let pane_restore_complete = pane_restore_complete.clone();
        outer_paned.connect_notify_local(Some("position"), move |_, _| {
            if !pane_restore_complete.get() {
                return;
            }
            if outer_position_programmatic.get() != 0 {
                return;
            }
            outer_split_dirty.set(true);
        });
    }

    PanedLayout {
        inner_paned,
        outer_paned,
        inner_position_programmatic,
        inner_split_dirty,
        outer_position_programmatic,
        outer_split_dirty,
    }
}

pub(crate) fn mount_window_content(
    window: &adw::ApplicationWindow,
    header_bar: &adw::HeaderBar,
    controls_row: &gtk4::Box,
    toast_overlay: &adw::ToastOverlay,
    outer_paned: &Paned,
    progress_box: &gtk4::Box,
) -> (adw::Banner, gtk4::Box) {
    toast_overlay.set_child(Some(outer_paned));
    toast_overlay.set_hexpand(true);
    toast_overlay.set_vexpand(true);

    let status_bar = gtk4::Box::new(Orientation::Horizontal, 0);
    status_bar.set_hexpand(true);
    status_bar.set_halign(gtk4::Align::Fill);
    status_bar.set_margin_start(8);
    status_bar.set_margin_end(8);
    status_bar.set_margin_top(2);
    status_bar.set_margin_bottom(2);
    status_bar.append(progress_box);

    let update_banner = adw::Banner::new("");
    update_banner.set_button_label(Some("Dismiss"));
    update_banner.set_revealed(false);

    let content_with_status = gtk4::Box::new(Orientation::Vertical, 0);
    content_with_status.set_hexpand(true);
    content_with_status.set_vexpand(true);
    content_with_status.append(controls_row);
    content_with_status.append(&update_banner);
    content_with_status.append(toast_overlay);
    content_with_status.append(&status_bar);

    let toolbar_view = adw::ToolbarView::new();
    toolbar_view.add_top_bar(header_bar);
    toolbar_view.set_content(Some(&content_with_status));
    window.set_content(Some(&toolbar_view));

    (update_banner, status_bar)
}

pub(crate) fn create_progress_widgets() -> (gtk4::Box, gtk4::Label, ProgressBar) {
    let progress_box = gtk4::Box::new(Orientation::Horizontal, 6);
    progress_box.set_visible(true);
    progress_box.set_halign(gtk4::Align::Start);
    progress_box.set_valign(gtk4::Align::Center);

    let progress_label = gtk4::Label::new(Some("Scanning folder..."));
    progress_label.add_css_class("caption");
    progress_label.set_halign(gtk4::Align::Start);

    let progress_bar = ProgressBar::new();
    progress_bar.set_hexpand(false);
    progress_bar.set_show_text(true);
    progress_bar.set_width_request(180);
    progress_bar.set_height_request(8);
    progress_bar.set_text(Some("--%"));

    progress_box.append(&progress_label);
    progress_box.append(&progress_bar);
    (progress_box, progress_label, progress_bar)
}

pub(crate) fn install_history_popover_handler(
    history_popover: &gtk4::Popover,
    history_list: &gtk4::Box,
    recent_folders: &Rc<RefCell<Vec<PathBuf>>>,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    open_folder_action: Rc<dyn Fn(PathBuf, bool)>,
    recent_folders_limit: usize,
) {
    let history_list_show = history_list.clone();
    let history_popover_show = history_popover.clone();
    let recent_folders_show = recent_folders.clone();
    let current_folder_history = current_folder.clone();
    let open_folder_from_history = open_folder_action.clone();

    history_popover.connect_show(move |_| {
        while let Some(child) = history_list_show.first_child() {
            history_list_show.remove(&child);
        }

        let folders = recent_folders_show.borrow().clone();
        if folders.is_empty() {
            let empty_label = gtk4::Label::new(Some("No recent folders"));
            empty_label.set_halign(gtk4::Align::Start);
            empty_label.add_css_class("dim-label");
            history_list_show.append(&empty_label);
            return;
        }

        for folder in folders.iter().take(recent_folders_limit) {
            let label = folder.display().to_string();
            let row = gtk4::Box::new(Orientation::Horizontal, 6);
            row.set_halign(gtk4::Align::Fill);
            row.set_hexpand(true);

            let btn = gtk4::Button::new();
            btn.set_halign(gtk4::Align::Fill);
            btn.set_hexpand(true);
            btn.set_tooltip_text(Some(&label));
            btn.add_css_class("flat");
            let btn_label = gtk4::Label::new(Some(&label));
            btn_label.set_xalign(0.0);
            btn.set_child(Some(&btn_label));

            let remove_btn = gtk4::Button::from_icon_name(crate::icons::DELETE);
            remove_btn.add_css_class("flat");
            remove_btn.set_tooltip_text(Some("Remove from history"));
            remove_btn.set_visible(false);

            row.append(&btn);
            row.append(&remove_btn);

            let path = folder.clone();
            let open_folder = open_folder_from_history.clone();
            let popover = history_popover_show.clone();
            btn.connect_clicked(move |_| {
                open_folder(path.clone(), true);
                popover.popdown();
            });

            let motion = gtk4::EventControllerMotion::new();
            let remove_btn_enter = remove_btn.clone();
            motion.connect_enter(move |_, _, _| {
                remove_btn_enter.set_visible(true);
            });
            let remove_btn_leave = remove_btn.clone();
            motion.connect_leave(move |_| {
                remove_btn_leave.set_visible(false);
            });
            row.add_controller(motion);

            let path = folder.clone();
            let recent_folders_remove = recent_folders_show.clone();
            let history_list_remove = history_list_show.clone();
            let row_remove = row.clone();
            let current_folder_remove = current_folder_history.clone();
            remove_btn.connect_clicked(move |_| {
                recent_folders_remove
                    .borrow_mut()
                    .retain(|entry| entry != &path);
                {
                    let history = recent_folders_remove.borrow();
                    crate::config::save_recent_state(
                        current_folder_remove.borrow().as_deref(),
                        &history,
                    );
                }
                history_list_remove.remove(&row_remove);
            });

            history_list_show.append(&row);
        }
    });
}

pub(crate) fn install_open_button_handler(
    open_btn: &gtk4::Button,
    window: &adw::ApplicationWindow,
    current_folder: &Rc<RefCell<Option<PathBuf>>>,
    open_folder_action: Rc<dyn Fn(PathBuf, bool)>,
) {
    let window_ref = window.clone();
    let current_folder_btn = current_folder.clone();
    let open_folder_btn = open_folder_action.clone();
    open_btn.connect_clicked(move |_| {
        let dialog = gtk4::FileDialog::builder().title("Choose a Folder").build();
        if let Some(folder) = current_folder_btn.borrow().as_ref() {
            let file = gio::File::for_path(folder);
            dialog.set_initial_folder(Some(&file));
        }
        let open_folder = open_folder_btn.clone();
        dialog.select_folder(
            Some(&window_ref),
            None::<&gio::Cancellable>,
            move |result| {
                let Ok(file) = result else { return };
                let Some(path) = file.path() else { return };
                open_folder(path, true);
            },
        );
    });
}
