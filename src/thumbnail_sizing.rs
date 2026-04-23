use crate::thumbnails;

pub fn thumbnail_size_options() -> [i32; 4] {
    let base = thumbnails::THUMB_NORMAL_SIZE;
    [
        base,
        (((base as f64) * 1.3 / 16.0).round() as i32) * 16,
        (((base as f64) * 1.6 / 16.0).round() as i32) * 16,
        (((base as f64) * 1.9 / 16.0).round() as i32) * 16,
    ]
}

pub fn normalize_thumbnail_size(size: i32) -> i32 {
    let options = thumbnail_size_options();
    options
        .iter()
        .copied()
        .min_by_key(|opt| (opt - size).abs())
        .unwrap_or(thumbnails::THUMB_NORMAL_SIZE)
}
