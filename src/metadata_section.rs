use crate::metadata::ImageMetadata;
use gtk4::prelude::*;
use std::{cell::Cell, rc::Rc};

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

pub fn apply_metadata_section_state(
    metadata: &ImageMetadata,
    meta_expander: &gtk4::Expander,
    meta_paned: &gtk4::Paned,
    meta_split_before_auto_collapse: &Rc<Cell<Option<i32>>>,
    min_meta_split_px: i32,
) {
    let has_content = metadata_has_content(metadata);
    let meta_total_height = meta_paned.height().max(1);
    let meta_upper_bound = meta_total_height.saturating_sub(min_meta_split_px);

    if has_content {
        meta_expander.set_expanded(true);
        if let Some(previous_pos) = meta_split_before_auto_collapse.get() {
            let restored_pos = if meta_upper_bound < min_meta_split_px {
                (meta_total_height / 2).max(1)
            } else {
                previous_pos.clamp(min_meta_split_px, meta_upper_bound)
            };
            meta_paned.set_position(restored_pos);
            meta_split_before_auto_collapse.set(None);
        }
    } else {
        if meta_split_before_auto_collapse.get().is_none() {
            meta_split_before_auto_collapse.set(Some(meta_paned.position()));
        }
        meta_expander.set_expanded(false);
        let collapsed_pos = if meta_upper_bound < min_meta_split_px {
            (meta_total_height / 2).max(1)
        } else {
            meta_upper_bound
        };
        meta_paned.set_position(collapsed_pos);
    }
}
