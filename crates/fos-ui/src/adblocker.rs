//! Adblocker Module - Using Brave's adblock-rust engine
//!
//! Supports EasyList, EasyPrivacy, and uBlock Origin filter syntax.

use adblock::Engine;
use adblock::lists::{FilterSet, ParseOptions};
use std::cell::RefCell;
use std::path::PathBuf;
use std::fs;
use tracing::{info, warn};

/// Filter lists to download
const FILTER_LISTS: &[(&str, &str)] = &[
    ("easylist", "https://easylist.to/easylist/easylist.txt"),
    ("easyprivacy", "https://easylist.to/easylist/easyprivacy.txt"),
    ("ublock-ads", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/filters.txt"),
    ("ublock-privacy", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/privacy.txt"),
    ("peter-lowe", "https://pgl.yoyo.org/adservers/serverlist.php?hostformat=adblockplus&showintro=1&mimetype=plaintext"),
];

// Thread-local engine (since we're running single-threaded GTK)
thread_local! {
    static ADBLOCK_ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
}

/// Get the filter cache directory
fn get_filter_dir() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("fos-wb")
        .join("filters");
    fs::create_dir_all(&dir).ok();
    dir
}

/// Create the adblock engine with filter lists
fn create_engine() -> Engine {
    let filter_dir = get_filter_dir();
    let mut filter_set = FilterSet::new(false);
    let mut total_rules = 0;
    
    for (name, url) in FILTER_LISTS {
        let cache_path = filter_dir.join(format!("{}.txt", name));
        
        // Try to load from cache first
        let content = if cache_path.exists() {
            info!("Loading cached filter: {}", name);
            fs::read_to_string(&cache_path).ok()
        } else {
            None
        };
        
        let content = content.unwrap_or_else(|| {
            // Download filter list
            info!("Downloading filter list: {}", name);
            match download_filter_list(url) {
                Ok(data) => {
                    // Cache it
                    fs::write(&cache_path, &data).ok();
                    data
                }
                Err(e) => {
                    warn!("Failed to download {}: {}", name, e);
                    String::new()
                }
            }
        });
        
        if !content.is_empty() {
            let rules: Vec<&str> = content.lines().collect();
            let count = rules.len();
            filter_set.add_filters(&rules, ParseOptions::default());
            total_rules += count;
            info!("Loaded {} rules from {}", count, name);
        }
    }
    
    info!("Adblock engine initialized with {} total rules", total_rules);
    Engine::from_filter_set(filter_set, true)
}

/// Download a filter list
fn download_filter_list(url: &str) -> Result<String, String> {
    reqwest::blocking::get(url)
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())
}

/// Check if a URL should be blocked
pub fn should_block(url: &str, source_url: &str, request_type: &str) -> bool {
    ADBLOCK_ENGINE.with(|engine| {
        let engine = engine.borrow();
        let Some(engine) = engine.as_ref() else {
            return false; // Engine not ready yet
        };
        
        match adblock::request::Request::new(url, source_url, request_type) {
            Ok(request) => engine.check_network_request(&request).matched,
            Err(_) => false,
        }
    })
}

/// Initialize the adblocker (call at startup on main thread)
pub fn init() {
    info!("Initializing adblocker...");
    let engine = create_engine();
    ADBLOCK_ENGINE.with(|e| {
        *e.borrow_mut() = Some(engine);
    });
    info!("Adblocker ready");
}
