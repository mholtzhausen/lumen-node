use crate::metadata::ImageMetadata;
use gtk4::prelude::*;
use std::{cell::Cell, rc::Rc};

/// End-child height when the metadata expander is collapsed (header + margins).
pub(crate) const META_COLLAPSED_HEADER_PX: i32 = 48;

pub fn metadata_has_content(meta: &ImageMetadata) -> bool {
    [
        meta.camera_make.as_ref(),
        meta.camera_model.as_ref(),
        meta.exposure.as_ref(),
        meta.iso.as_ref(),
        meta.prompt.as_ref(),
        meta.negative_prompt.as_ref(),
        meta.raw_parameters.as_ref(),
        meta.workflow_json.as_ref(),
    ]
    .iter()
    .any(|v| v.is_some())
}

fn meta_upper_bound(meta_total_height: i32, min_meta_split_px: i32) -> i32 {
    meta_total_height.saturating_sub(min_meta_split_px)
}

fn collapsed_paned_position(meta_total_height: i32) -> i32 {
    meta_total_height
        .saturating_sub(META_COLLAPSED_HEADER_PX)
        .max(1)
}

fn restored_paned_position(
    previous_pos: i32,
    meta_total_height: i32,
    min_meta_split_px: i32,
) -> i32 {
    let upper = meta_upper_bound(meta_total_height, min_meta_split_px);
    if upper < min_meta_split_px {
        (meta_total_height / 2).max(1)
    } else {
        previous_pos.clamp(min_meta_split_px, upper)
    }
}

pub fn sync_meta_paned_to_expander_state(
    expanded: bool,
    meta_paned: &gtk4::Paned,
    meta_split_before_auto_collapse: &Rc<Cell<Option<i32>>>,
    min_meta_split_px: i32,
) {
    let meta_total_height = meta_paned.height().max(1);
    if expanded {
        if let Some(previous_pos) = meta_split_before_auto_collapse.get() {
            meta_paned.set_position(restored_paned_position(
                previous_pos,
                meta_total_height,
                min_meta_split_px,
            ));
            meta_split_before_auto_collapse.set(None);
        }
    } else {
        if meta_split_before_auto_collapse.get().is_none() {
            meta_split_before_auto_collapse.set(Some(meta_paned.position()));
        }
        meta_paned.set_position(collapsed_paned_position(meta_total_height));
    }
}

/// Apply expander/paned state for the loaded metadata.
///
/// Empty metadata forces a temporary collapse without changing the user's
/// preferred expanded state. When content exists, the expander follows
/// `meta_section_expanded_pref`.
pub fn apply_metadata_section_state(
    metadata: &ImageMetadata,
    meta_expander: &gtk4::Expander,
    meta_paned: &gtk4::Paned,
    meta_split_before_auto_collapse: &Rc<Cell<Option<i32>>>,
    min_meta_split_px: i32,
    meta_section_expanded_pref: &Rc<Cell<bool>>,
) {
    let has_content = metadata_has_content(metadata);
    let expanded = has_content && meta_section_expanded_pref.get();
    meta_expander.set_expanded(expanded);
    sync_meta_paned_to_expander_state(
        expanded,
        meta_paned,
        meta_split_before_auto_collapse,
        min_meta_split_px,
    );
}

pub fn connect_meta_expander_paned_sync(
    meta_expander: &gtk4::Expander,
    meta_paned: &gtk4::Paned,
    meta_split_before_auto_collapse: &Rc<Cell<Option<i32>>>,
    meta_position_programmatic: &Rc<Cell<u32>>,
    min_meta_split_px: i32,
    meta_section_expanded_pref: &Rc<Cell<bool>>,
) {
    let meta_paned = meta_paned.clone();
    let meta_split_before_auto_collapse = meta_split_before_auto_collapse.clone();
    let meta_position_programmatic = meta_position_programmatic.clone();
    let meta_section_expanded_pref = meta_section_expanded_pref.clone();
    meta_expander.connect_notify_local(Some("expanded"), move |expander, _| {
        // User-driven toggles update the persisted preference; programmatic
        // apply/restore (outer wrap already increments the counter) does not.
        if meta_position_programmatic.get() == 0 {
            meta_section_expanded_pref.set(expander.is_expanded());
        }
        meta_position_programmatic.set(meta_position_programmatic.get().saturating_add(1));
        sync_meta_paned_to_expander_state(
            expander.is_expanded(),
            &meta_paned,
            &meta_split_before_auto_collapse,
            min_meta_split_px,
        );
        meta_position_programmatic.set(meta_position_programmatic.get().saturating_sub(1));
    });
}
