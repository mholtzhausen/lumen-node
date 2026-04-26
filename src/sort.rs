pub const SORT_KEY_NAME_ASC: &str = "name_asc";
pub const SORT_KEY_NAME_DESC: &str = "name_desc";
pub const SORT_KEY_DATE_ASC: &str = "date_asc";
pub const SORT_KEY_DATE_DESC: &str = "date_desc";
pub const SORT_KEY_SIZE_ASC: &str = "size_asc";
pub const SORT_KEY_SIZE_DESC: &str = "size_desc";

pub fn normalize_sort_key(sort_key: &str) -> &'static str {
    match sort_key {
        SORT_KEY_NAME_DESC => SORT_KEY_NAME_DESC,
        SORT_KEY_DATE_ASC => SORT_KEY_DATE_ASC,
        SORT_KEY_DATE_DESC => SORT_KEY_DATE_DESC,
        SORT_KEY_SIZE_ASC => SORT_KEY_SIZE_ASC,
        SORT_KEY_SIZE_DESC => SORT_KEY_SIZE_DESC,
        _ => SORT_KEY_NAME_ASC,
    }
}

pub fn sort_index_for_key(sort_key: &str) -> u32 {
    match normalize_sort_key(sort_key) {
        SORT_KEY_NAME_DESC => 1,
        SORT_KEY_DATE_ASC => 2,
        SORT_KEY_DATE_DESC => 3,
        SORT_KEY_SIZE_ASC => 4,
        SORT_KEY_SIZE_DESC => 5,
        _ => 0,
    }
}

pub fn sort_key_for_index(index: u32) -> &'static str {
    match index {
        1 => SORT_KEY_NAME_DESC,
        2 => SORT_KEY_DATE_ASC,
        3 => SORT_KEY_DATE_DESC,
        4 => SORT_KEY_SIZE_ASC,
        5 => SORT_KEY_SIZE_DESC,
        _ => SORT_KEY_NAME_ASC,
    }
}
