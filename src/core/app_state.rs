use crate::config::AppConfig;
use crate::sort_flags::SortFields;
use crate::thumbnail_sizing::normalize_thumbnail_size;
use crate::ImageMetadata;
use crate::ScanProgressState;
use gtk4::{gio, glib, Image, StringObject};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::PathBuf,
    rc::Rc,
    time::Instant,
};

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) current_folder: Rc<RefCell<Option<PathBuf>>>,
    pub(crate) recent_folders: Rc<RefCell<Vec<PathBuf>>>,
    pub(crate) list_store: gio::ListStore,
    pub(crate) progress_state: Rc<RefCell<ScanProgressState>>,
    pub(crate) hash_cache: Rc<RefCell<HashMap<String, String>>>,
    pub(crate) meta_cache: Rc<RefCell<HashMap<String, ImageMetadata>>>,
    pub(crate) sort_fields_cache: Rc<RefCell<HashMap<String, SortFields>>>,
    pub(crate) active_scan_generation: Rc<Cell<u64>>,
    pub(crate) scan_in_progress: Rc<Cell<bool>>,
    pub(crate) sort_key: Rc<RefCell<String>>,
    pub(crate) search_text: Rc<RefCell<String>>,
    pub(crate) thumbnail_size: Rc<RefCell<i32>>,
    pub(crate) realized_thumb_images: Rc<RefCell<Vec<glib::WeakRef<Image>>>>,
    pub(crate) realized_cell_boxes: Rc<RefCell<Vec<glib::WeakRef<gtk4::Box>>>>,
    pub(crate) fast_scroll_active: Rc<Cell<bool>>,
    pub(crate) scroll_last_pos: Rc<Cell<f64>>,
    pub(crate) scroll_last_time: Rc<Cell<Option<Instant>>>,
    pub(crate) scroll_debounce_gen: Rc<Cell<u64>>,
    pub(crate) initial_thumbnail_size: i32,
}

pub(crate) fn build_app_state(
    app_config: &AppConfig,
    recent_folders_limit: usize,
    default_sort_key: &str,
    default_thumbnail_size: i32,
) -> AppState {
    let current_folder: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));
    let recent_folders: Rc<RefCell<Vec<PathBuf>>> =
        Rc::new(RefCell::new(app_config.recent_folders.clone()));
    {
        let mut history = recent_folders.borrow_mut();
        let mut sanitized = Vec::new();
        for folder in history.iter() {
            if folder.is_dir() && !sanitized.iter().any(|entry| entry == folder) {
                sanitized.push(folder.clone());
            }
        }
        *history = sanitized;
        history.truncate(recent_folders_limit);
    }

    let sort_key: Rc<RefCell<String>> = Rc::new(RefCell::new(
        app_config
            .sort_key
            .as_deref()
            .map(crate::sort::normalize_sort_key)
            .unwrap_or(default_sort_key)
            .to_string(),
    ));
    let search_text: Rc<RefCell<String>> = Rc::new(RefCell::new(
        app_config.search_text.clone().unwrap_or_default(),
    ));
    let initial_thumbnail_size =
        normalize_thumbnail_size(app_config.thumbnail_size.unwrap_or(default_thumbnail_size));
    let thumbnail_size: Rc<RefCell<i32>> = Rc::new(RefCell::new(initial_thumbnail_size));

    AppState {
        current_folder,
        recent_folders,
        list_store: gio::ListStore::new::<StringObject>(),
        progress_state: Rc::new(RefCell::new(ScanProgressState::default())),
        hash_cache: Rc::new(RefCell::new(HashMap::new())),
        meta_cache: Rc::new(RefCell::new(HashMap::new())),
        sort_fields_cache: Rc::new(RefCell::new(HashMap::new())),
        active_scan_generation: Rc::new(Cell::new(0_u64)),
        scan_in_progress: Rc::new(Cell::new(false)),
        sort_key,
        search_text,
        thumbnail_size,
        realized_thumb_images: Rc::new(RefCell::new(Vec::new())),
        realized_cell_boxes: Rc::new(RefCell::new(Vec::new())),
        fast_scroll_active: Rc::new(Cell::new(false)),
        scroll_last_pos: Rc::new(Cell::new(0.0)),
        scroll_last_time: Rc::new(Cell::new(None)),
        scroll_debounce_gen: Rc::new(Cell::new(0)),
        initial_thumbnail_size,
    }
}
