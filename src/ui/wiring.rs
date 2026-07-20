use crate::core::app_state::AppState;
use crate::metadata::ImageMetadata;
use crate::thumbnail_sizing::thumbnail_size_options;
use crate::ui::actions::install_context_menu;
use crate::ui::center::CenterContentBundle;
use crate::ui::controls::{
    install_clear_button_handler, install_favorites_only_handler, install_search_entry_handler,
    install_similar_filter_button_handler, install_sort_dropdown_handler,
    install_tags_filter_popover_handler, install_thumbnail_size_handlers,
    refresh_tag_filter_from_folder,
};
use crate::ui::left_chrome_wiring::LeftChromeWiring;
use crate::ui::list_mutation::ListMutationContext;
use crate::ui::open_folder::{build_open_folder_action, OpenFolderActionDeps};
use crate::ui::right_sidebar::RightSidebarBundle;
use crate::ui::preview::{clear_picture, load_picture_async};
use crate::ui::selection::{handle_selection_change_event, ClickTrace};
use crate::ui::shell::{install_history_popover_handler, install_open_button_handler};
use crate::ui::grid::show_full_view_favourite_hud;
use crate::ui::sidebar::{
    clear_metadata_sidebar, populate_metadata_sidebar, update_preview_favourite_indicator,
};
use gtk4::prelude::*;
use gtk4::{ListScrollFlags, StringObject};
use libadwaita as adw;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

fn favourite_for_selected(app_state: &AppState) -> Option<bool> {
    let path = app_state.selected_path.borrow().clone()?;
    Some(
        app_state
            .favourite_cache
            .borrow()
            .get(&path)
            .copied()
            .unwrap_or(false),
    )
}

pub(crate) struct ContextMenuWiringDeps {
    pub(crate) app_state: AppState,
    pub(crate) window: adw::ApplicationWindow,
    pub(crate) toast_overlay: adw::ToastOverlay,
    pub(crate) selection_model: gtk4::SingleSelection,
    pub(crate) center: CenterContentBundle,
    pub(crate) right: RightSidebarBundle,
    pub(crate) min_meta_split_px: i32,
    pub(crate) start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    pub(crate) external_editor: Option<PathBuf>,
    pub(crate) filter: gtk4::CustomFilter,
    pub(crate) left_toggle: gtk4::ToggleButton,
    pub(crate) right_toggle: gtk4::ToggleButton,
    pub(crate) pre_fullview_left: Rc<Cell<bool>>,
    pub(crate) pre_fullview_right: Rc<Cell<bool>>,
    pub(crate) tags_filter_btn: gtk4::MenuButton,
    pub(crate) tags_filter_list: gtk4::Box,
    pub(crate) similar_filter_btn: gtk4::Button,
}

pub(crate) fn install_context_menu_wiring(deps: ContextMenuWiringDeps) -> Rc<dyn Fn()> {
    let refresh_metadata_sidebar: Rc<dyn Fn(&ImageMetadata)> = Rc::new({
        let meta_listbox = deps.right.meta_listbox.clone();
        move |meta: &ImageMetadata| populate_metadata_sidebar(&meta_listbox, meta)
    });
    let start_scan_for_folder: Rc<dyn Fn(PathBuf)> = deps.start_scan_for_folder.clone();

    let on_favourite_changed: Rc<dyn Fn(bool)> = {
        let preview_favourite = deps.right.preview_favourite.clone();
        let hud = deps.center.full_view_favourite_hud.clone();
        let view_stack = deps.center.view_stack.clone();
        Rc::new(move |is_favourite: bool| {
            update_preview_favourite_indicator(&preview_favourite, Some(is_favourite));
            if view_stack.visible_child_name().as_deref() == Some("single") {
                show_full_view_favourite_hud(&hud, is_favourite);
            }
        })
    };
    *deps.app_state.on_favourite_changed.borrow_mut() = Some(on_favourite_changed.clone());

    {
        let window = deps.window.clone();
        deps.right.preview_favourite.button.connect_clicked(move |_| {
            let _ = gtk4::prelude::WidgetExt::activate_action(
                &window,
                "ctx.toggle-favourite",
                None,
            );
        });
    }
    {
        let window = deps.window.clone();
        deps.center
            .full_view_favourite_hud
            .button
            .connect_clicked(move |_| {
                let _ = gtk4::prelude::WidgetExt::activate_action(
                    &window,
                    "ctx.toggle-favourite",
                    None,
                );
            });
    }

    install_context_menu(
        &deps.window,
        &deps.toast_overlay,
        &deps.selection_model,
        &deps.app_state.meta_cache,
        &deps.app_state.hash_cache,
        &deps.app_state.thumbnail_size,
        &deps.right.meta_expander,
        &deps.right.meta_paned,
        &deps.right.meta_split_before_auto_collapse,
        &deps.right.meta_position_programmatic,
        &deps.right.meta_section_expanded_pref,
        deps.min_meta_split_px,
        &deps.app_state.current_folder,
        &start_scan_for_folder,
        &deps.app_state.list_store,
        &refresh_metadata_sidebar,
        deps.external_editor.as_ref(),
        &deps.center.grid_view,
        &deps.center.single_picture,
        &deps.center.compare_left_picture,
        &deps.center.compare_right_picture,
        &deps.right.meta_preview,
        &deps.center.view_stack,
        &deps.left_toggle,
        &deps.right_toggle,
        &deps.pre_fullview_left,
        &deps.pre_fullview_right,
        &ListMutationContext {
            app_state: deps.app_state.clone(),
            selection_model: deps.selection_model.clone(),
            start_scan_for_folder: deps.start_scan_for_folder.clone(),
        },
        &deps.filter,
        on_favourite_changed,
        &deps.tags_filter_btn,
        &deps.tags_filter_list,
        &deps.similar_filter_btn,
    )
}

pub(crate) struct SelectionWiringDeps {
    pub(crate) app_state: AppState,
    pub(crate) selection_model: gtk4::SingleSelection,
    pub(crate) center: CenterContentBundle,
    pub(crate) right: RightSidebarBundle,
}

pub(crate) fn install_selection_wiring(deps: SelectionWiringDeps) {
    let click_trace_state: Rc<RefCell<Option<ClickTrace>>> = Rc::new(RefCell::new(None));
    let click_trace_state_sel = click_trace_state.clone();
    let meta_listbox_sel = deps.right.meta_listbox.clone();
    let meta_expander_sel = deps.right.meta_expander.clone();
    let meta_paned_sel = deps.right.meta_paned.clone();
    let meta_split_before_auto_collapse_sel = deps.right.meta_split_before_auto_collapse.clone();
    let meta_position_programmatic_sel = deps.right.meta_position_programmatic.clone();
    let meta_section_expanded_pref_sel = deps.right.meta_section_expanded_pref.clone();
    let meta_preview_sel = deps.right.meta_preview.clone();
    let meta_cache_sel = deps.app_state.meta_cache.clone();
    let app_state_sel = deps.app_state.clone();
    let grid_view_sel = deps.center.grid_view.clone();
    let preview_favourite_sel = deps.right.preview_favourite.clone();
    let hud_sel = deps.center.full_view_favourite_hud.clone();
    let view_stack_sel = deps.center.view_stack.clone();
    let compare_right_sel = deps.center.compare_right_picture.clone();
    deps.selection_model
        .connect_selection_changed(move |model, _, _| {
            let Some(item) = model.selected_item().and_downcast::<StringObject>() else {
                clear_picture(&meta_preview_sel);
                clear_metadata_sidebar(&meta_listbox_sel);
                update_preview_favourite_indicator(&preview_favourite_sel, None);
                if view_stack_sel.visible_child_name().as_deref() == Some("compare") {
                    clear_picture(&compare_right_sel);
                }
                return;
            };
            let selected = model.selected();
            if selected < model.n_items() {
                grid_view_sel.scroll_to(
                    selected,
                    ListScrollFlags::SELECT | ListScrollFlags::FOCUS,
                    None,
                );
            }
            handle_selection_change_event(
                &item,
                &click_trace_state_sel,
                &meta_cache_sel,
                &meta_listbox_sel,
                &meta_expander_sel,
                &meta_paned_sel,
                &meta_split_before_auto_collapse_sel,
                &meta_position_programmatic_sel,
                &meta_section_expanded_pref_sel,
                &meta_preview_sel,
                &app_state_sel,
            );
            let is_favourite = favourite_for_selected(&app_state_sel);
            update_preview_favourite_indicator(&preview_favourite_sel, is_favourite);
            let page = view_stack_sel.visible_child_name();
            if page.as_deref() == Some("single") {
                show_full_view_favourite_hud(&hud_sel, is_favourite.unwrap_or(false));
            }
            if page.as_deref() == Some("compare") {
                load_picture_async(&compare_right_sel, &item.string().to_string(), None, None);
            }
        });
}

pub(crate) struct OpenFolderWiringDeps {
    pub(crate) app_state: AppState,
    pub(crate) start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    pub(crate) chrome: LeftChromeWiring,
    pub(crate) filter: gtk4::CustomFilter,
    pub(crate) sorter: gtk4::CustomSorter,
    pub(crate) recent_folders_limit: usize,
    pub(crate) window: adw::ApplicationWindow,
}

pub(crate) fn install_open_folder_wiring(deps: OpenFolderWiringDeps) -> Rc<dyn Fn(PathBuf, bool)> {
    let open_folder_action = build_open_folder_action(OpenFolderActionDeps {
        current_folder: deps.app_state.current_folder.clone(),
        start_scan_for_folder: deps.start_scan_for_folder,
        tree_root: deps.chrome.tree_root,
        tree_model: deps.chrome.tree_model,
        tree_list_view: deps.chrome.tree_list_view,
        recent_folders: deps.app_state.recent_folders.clone(),
        sort_key: deps.app_state.sort_key.clone(),
        search_text: deps.app_state.search_text.clone(),
        favorites_only: deps.app_state.favorites_only.clone(),
        active_tag_filters: deps.app_state.active_tag_filters.clone(),
        tag_filter_debounce_gen: deps.app_state.tag_filter_debounce_gen.clone(),
        thumbnail_size: deps.app_state.thumbnail_size.clone(),
        sort_dropdown: deps.chrome.sort_dropdown,
        favourites_filter_btn: deps.chrome.favourites_filter_btn,
        tags_filter_btn: deps.chrome.tags_filter_btn,
        tags_filter_list: deps.chrome.tags_filter_list,
        search_entry: deps.chrome.search_entry,
        filter: deps.filter,
        sorter: deps.sorter,
        size_buttons: deps.chrome.size_buttons,
        progress_state: deps.app_state.progress_state.clone(),
        recent_folders_limit: deps.recent_folders_limit,
        grid_loading: deps.app_state.grid_loading.clone(),
    });

    install_history_popover_handler(
        &deps.chrome.history_popover,
        &deps.chrome.history_list,
        &deps.app_state.recent_folders,
        &deps.app_state.current_folder,
        open_folder_action.clone(),
        deps.recent_folders_limit,
    );

    install_open_button_handler(
        &deps.chrome.open_btn,
        &deps.window,
        &deps.app_state.current_folder,
        open_folder_action.clone(),
    );

    open_folder_action
}

pub(crate) struct ControlsWiringDeps {
    pub(crate) app_state: AppState,
    pub(crate) chrome: LeftChromeWiring,
    pub(crate) center: CenterContentBundle,
    pub(crate) sorter: gtk4::CustomSorter,
    pub(crate) start_scan_for_folder: Rc<dyn Fn(PathBuf)>,
    pub(crate) filter: gtk4::CustomFilter,
    pub(crate) toast_overlay: adw::ToastOverlay,
    pub(crate) selection_model: gtk4::SingleSelection,
}

pub(crate) fn install_controls_wiring(deps: ControlsWiringDeps) {
    {
        let tags_filter_list = deps.chrome.tags_filter_list.clone();
        let tags_filter_btn = deps.chrome.tags_filter_btn.clone();
        let active_tag_filters = deps.app_state.active_tag_filters.clone();
        let tag_filter_debounce_gen = deps.app_state.tag_filter_debounce_gen.clone();
        let filter = deps.filter.clone();
        let current_folder = deps.app_state.current_folder.clone();
        let grid_loading = deps.app_state.grid_loading.clone();
        *deps.app_state.on_folder_tags_changed.borrow_mut() = Some(Rc::new(move || {
            refresh_tag_filter_from_folder(
                &tags_filter_list,
                &tags_filter_btn,
                &active_tag_filters,
                &tag_filter_debounce_gen,
                &filter,
                &current_folder,
                &grid_loading,
            );
            filter.changed(gtk4::FilterChange::Different);
        }));
    }

    install_sort_dropdown_handler(
        &deps.chrome.sort_dropdown,
        &deps.app_state.sort_key,
        &deps.sorter,
        &deps.app_state.current_folder,
        &deps.app_state.scan_in_progress,
        &deps.start_scan_for_folder,
    );
    install_search_entry_handler(
        &deps.chrome.search_entry,
        &deps.app_state.search_text,
        &deps.filter,
        &deps.app_state.current_folder,
        &deps.app_state.grid_loading,
    );
    install_tags_filter_popover_handler(&deps.chrome.tags_filter_btn);
    install_clear_button_handler(
        &deps.chrome.clear_btn,
        &deps.app_state.search_text,
        &deps.app_state.favorites_only,
        &deps.app_state.active_tag_filters,
        &deps.app_state.tag_filter_debounce_gen,
        &deps.app_state.similar_paths,
        &deps.app_state.sort_key,
        &deps.filter,
        &deps.sorter,
        &deps.chrome.favourites_filter_btn,
        &deps.chrome.tags_filter_btn,
        &deps.chrome.tags_filter_list,
        &deps.chrome.search_entry,
        &deps.chrome.sort_dropdown,
        &deps.app_state.thumbnail_size,
        &deps.app_state.current_folder,
        &deps.chrome.similar_filter_btn,
        &deps.app_state.grid_loading,
    );
    install_similar_filter_button_handler(
        &deps.chrome.similar_filter_btn,
        &deps.app_state.similar_paths,
        &deps.filter,
        &deps.app_state.grid_loading,
    );
    install_favorites_only_handler(
        &deps.chrome.favourites_filter_btn,
        &deps.app_state.favorites_only,
        &deps.filter,
        &deps.app_state.current_folder,
        &deps.toast_overlay,
        &deps.selection_model,
        &deps.app_state.list_store,
        &deps.app_state.grid_loading,
    );
    install_thumbnail_size_handlers(
        &deps.chrome.size_buttons,
        thumbnail_size_options(),
        &deps.app_state,
        &deps.center.grid_view,
        &deps.app_state.current_folder,
    );
}
