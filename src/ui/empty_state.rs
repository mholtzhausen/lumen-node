use crate::core::app_state::AppState;
use crate::ui::controls::{
    apply_clear_filters, deactivate_favorites_filter, deactivate_tag_filter,
};
use gtk4::prelude::*;
use gtk4::{SortListModel, MultiSelection};
use libadwaita as adw;
use std::{cell::Cell, rc::Rc};

pub(crate) struct EmptyStatusPage {
    pub(crate) page: adw::StatusPage,
    pub(crate) action_btn: gtk4::Button,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EmptyAction {
    None,
    OpenFolder,
    ShowAllImages,
    ClearTagFilter,
    ClearFilters,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EmptyStateVariant {
    Hidden,
    OpenFolder,
    NoImages,
    NoFavourites,
    NoTags,
    NoMatches,
}

pub(crate) fn create_empty_status_page() -> EmptyStatusPage {
    let page = adw::StatusPage::new();
    page.set_halign(gtk4::Align::Center);
    page.set_valign(gtk4::Align::Center);
    page.set_hexpand(true);
    page.set_vexpand(true);
    page.set_visible(false);

    let action_btn = gtk4::Button::new();
    action_btn.add_css_class("suggested-action");
    action_btn.add_css_class("pill");
    page.set_child(Some(&action_btn));

    EmptyStatusPage { page, action_btn }
}

pub(crate) fn add_empty_status_overlay(grid_overlay: &gtk4::Overlay, page: &adw::StatusPage) {
    grid_overlay.add_overlay(page);
}

pub(crate) struct EmptyStateWiringDeps {
    pub(crate) app_state: AppState,
    pub(crate) selection_model: MultiSelection,
    pub(crate) sort_model: SortListModel,
    pub(crate) status_page: adw::StatusPage,
    pub(crate) action_btn: gtk4::Button,
    pub(crate) window: adw::ApplicationWindow,
    pub(crate) favourites_filter_btn: gtk4::ToggleButton,
    pub(crate) tags_filter_btn: gtk4::MenuButton,
    pub(crate) tags_filter_list: gtk4::Box,
    pub(crate) search_entry: gtk4::SearchEntry,
    pub(crate) sort_dropdown: gtk4::DropDown,
    pub(crate) similar_filter_btn: gtk4::Button,
    pub(crate) filter: gtk4::CustomFilter,
    pub(crate) sorter: gtk4::CustomSorter,
}

pub(crate) fn install_empty_state_wiring(deps: EmptyStateWiringDeps) -> Rc<dyn Fn()> {
    let current_action: Rc<Cell<EmptyAction>> = Rc::new(Cell::new(EmptyAction::None));

    let refresh = {
        let app_state = deps.app_state.clone();
        let selection_model = deps.selection_model.clone();
        let status_page = deps.status_page.clone();
        let action_btn = deps.action_btn.clone();
        let current_action = current_action.clone();
        Rc::new(move || {
            let variant = compute_variant(&app_state, &selection_model);
            apply_variant(
                variant,
                &status_page,
                &action_btn,
                &current_action,
            );
        })
    };

    {
        let window = deps.window.clone();
        let app_state = deps.app_state.clone();
        let favourites_filter_btn = deps.favourites_filter_btn.clone();
        let tags_filter_btn = deps.tags_filter_btn.clone();
        let tags_filter_list = deps.tags_filter_list.clone();
        let search_entry = deps.search_entry.clone();
        let sort_dropdown = deps.sort_dropdown.clone();
        let similar_filter_btn = deps.similar_filter_btn.clone();
        let filter = deps.filter.clone();
        let sorter = deps.sorter.clone();
        let current_action = current_action.clone();
        deps.action_btn.connect_clicked(move |_| {
            match current_action.get() {
                EmptyAction::OpenFolder => {
                    let _ = gtk4::prelude::WidgetExt::activate_action(
                        &window,
                        "win.open-folder",
                        None,
                    );
                }
                EmptyAction::ShowAllImages => {
                    deactivate_favorites_filter(
                        &app_state.favorites_only,
                        &filter,
                        &favourites_filter_btn,
                        &app_state.current_folder,
                        &app_state.grid_loading,
                    );
                }
                EmptyAction::ClearTagFilter => {
                    deactivate_tag_filter(
                        &app_state.active_tag_filters,
                        &app_state.tag_filter_debounce_gen,
                        &filter,
                        &tags_filter_btn,
                        &tags_filter_list,
                        &app_state.current_folder,
                        &app_state.grid_loading,
                    );
                }
                EmptyAction::ClearFilters => {
                    apply_clear_filters(
                        &app_state.search_text,
                        &app_state.favorites_only,
                        &app_state.active_tag_filters,
                        &app_state.tag_filter_debounce_gen,
                        &app_state.similar_paths,
                        &app_state.sort_key,
                        &filter,
                        &sorter,
                        &favourites_filter_btn,
                        &tags_filter_btn,
                        &tags_filter_list,
                        &search_entry,
                        &sort_dropdown,
                        &app_state.thumbnail_size,
                        &app_state.current_folder,
                        &similar_filter_btn,
                        &app_state.grid_loading,
                    );
                }
                EmptyAction::None => {}
            }
        });
    }

    {
        let refresh = refresh.clone();
        deps.app_state
            .list_store
            .connect_items_changed(move |_, _, _, _| refresh());
    }
    {
        let refresh = refresh.clone();
        deps.sort_model
            .connect_items_changed(move |_, _, _, _| refresh());
    }
    {
        let refresh = refresh.clone();
        deps.favourites_filter_btn.connect_toggled(move |_| refresh());
    }
    {
        let refresh = refresh.clone();
        deps.search_entry.connect_search_changed(move |_| refresh());
    }

    refresh();
    refresh
}

fn compute_variant(app_state: &AppState, selection_model: &MultiSelection) -> EmptyStateVariant {
    if app_state.current_folder.borrow().is_none() {
        return EmptyStateVariant::OpenFolder;
    }

    let total = app_state.list_store.n_items();
    let visible = selection_model.n_items();

    let loading_visible = app_state
        .grid_loading
        .borrow()
        .as_ref()
        .is_some_and(|loading| loading.is_visible());
    if loading_visible || (app_state.scan_in_progress.get() && total == 0) {
        return EmptyStateVariant::Hidden;
    }
    if total == 0 {
        return EmptyStateVariant::NoImages;
    }
    if visible > 0 {
        return EmptyStateVariant::Hidden;
    }
    if app_state.favorites_only.get() && app_state.active_tag_filters.borrow().is_empty() {
        return EmptyStateVariant::NoFavourites;
    }
    if !app_state.active_tag_filters.borrow().is_empty()
        && app_state.search_text.borrow().is_empty()
        && !app_state.favorites_only.get()
    {
        return EmptyStateVariant::NoTags;
    }
    EmptyStateVariant::NoMatches
}

fn apply_variant(
    variant: EmptyStateVariant,
    status_page: &adw::StatusPage,
    action_btn: &gtk4::Button,
    current_action: &Rc<Cell<EmptyAction>>,
) {
    match variant {
        EmptyStateVariant::Hidden => {
            status_page.set_visible(false);
            current_action.set(EmptyAction::None);
        }
        EmptyStateVariant::OpenFolder => {
            status_page.set_icon_name(Some("folder-symbolic"));
            status_page.set_title("Open a folder");
            status_page.set_description(Some(
                "Choose a folder to browse your images in the grid.",
            ));
            action_btn.set_label("Open Folder");
            action_btn.set_visible(true);
            current_action.set(EmptyAction::OpenFolder);
            status_page.set_visible(true);
        }
        EmptyStateVariant::NoImages => {
            status_page.set_icon_name(Some("image-x-generic-symbolic"));
            status_page.set_title("No images in this folder");
            status_page.set_description(Some(
                "This folder does not contain any supported image files.",
            ));
            action_btn.set_visible(false);
            current_action.set(EmptyAction::None);
            status_page.set_visible(true);
        }
        EmptyStateVariant::NoFavourites => {
            status_page.set_icon_name(Some("starred-symbolic"));
            status_page.set_title("No favourites");
            status_page.set_description(Some(
                "None of the images in this folder are marked as favourites.",
            ));
            action_btn.set_label("Show all images");
            action_btn.set_visible(true);
            current_action.set(EmptyAction::ShowAllImages);
            status_page.set_visible(true);
        }
        EmptyStateVariant::NoTags => {
            status_page.set_icon_name(Some(crate::icons::TAG_ICON_NAME));
            status_page.set_title("No matching tags");
            status_page.set_description(Some(
                "No images have all of the selected tags. Clear the tag filter to see more.",
            ));
            action_btn.set_label("Clear tag filter");
            action_btn.set_visible(true);
            current_action.set(EmptyAction::ClearTagFilter);
            status_page.set_visible(true);
        }
        EmptyStateVariant::NoMatches => {
            status_page.set_icon_name(Some("system-search-symbolic"));
            status_page.set_title("No matches");
            status_page.set_description(Some(
                "No images match the current search or filter settings.",
            ));
            action_btn.set_label("Clear filters");
            action_btn.set_visible(true);
            current_action.set(EmptyAction::ClearFilters);
            status_page.set_visible(true);
        }
    }
}
