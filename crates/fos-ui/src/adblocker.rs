//! Adblocker Module - Enhanced with Cosmetic Filtering & Scriptlets
//!
//! Features:
//! - Network-level blocking via Brave's adblock-rust engine
//! - Cosmetic filtering (element hiding via CSS)
//! - YouTube ad-skip scriptlet injection
//! - Multiple filter lists including YouTube-specific blockers

use adblock::Engine;
use adblock::lists::{FilterSet, ParseOptions};
use std::cell::RefCell;
use std::path::PathBuf;
use std::fs;
use tracing::{info, warn};

/// Filter lists to download - expanded for better coverage
const FILTER_LISTS: &[(&str, &str)] = &[
    // Core lists
    ("easylist", "https://easylist.to/easylist/easylist.txt"),
    ("easyprivacy", "https://easylist.to/easylist/easyprivacy.txt"),
    
    // uBlock Origin lists
    ("ublock-ads", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/filters.txt"),
    ("ublock-privacy", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/privacy.txt"),
    ("ublock-quick", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/quick-fixes.txt"),
    ("ublock-unbreak", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/unbreak.txt"),
    
    // YouTube/Google specific
    ("ublock-badware", "https://raw.githubusercontent.com/uBlockOrigin/uAssets/master/filters/badware.txt"),
    
    // Peter Lowe's list
    ("peter-lowe", "https://pgl.yoyo.org/adservers/serverlist.php?hostformat=adblockplus&showintro=1&mimetype=plaintext"),
    
    // Annoyances
    ("fanboy-annoyance", "https://secure.fanboy.co.nz/fanboy-annoyance.txt"),
    ("fanboy-social", "https://easylist.to/easylist/fanboy-social.txt"),
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

/// Get cosmetic filters (CSS rules to hide elements) for a URL
pub fn get_cosmetic_filters(url: &str) -> String {
    ADBLOCK_ENGINE.with(|engine| {
        let engine = engine.borrow();
        let Some(engine) = engine.as_ref() else {
            return String::new();
        };
        
        let resources = engine.url_cosmetic_resources(url);
        
        // Build CSS to hide elements
        let mut css = String::new();
        
        // Hide matched selectors
        for selector in &resources.hide_selectors {
            if !css.is_empty() {
                css.push(',');
            }
            css.push_str(selector);
        }
        
        if !css.is_empty() {
            css.push_str(" { display: none !important; visibility: hidden !important; }");
        }
        
        // Add injected CSS
        if !resources.injected_script.is_empty() {
            css.push_str("\n");
            css.push_str(&resources.injected_script);
        }
        
        css
    })
}

/// Get YouTube ad-skip script
/// This script auto-skips YouTube ads and removes ad overlays
pub fn get_youtube_adskip_script() -> &'static str {
    r#"
    (function() {
        'use strict';
        
        // Skip video ads
        function skipAd() {
            // Click skip button if available
            const skipBtn = document.querySelector('.ytp-skip-ad-button, .ytp-ad-skip-button, .ytp-ad-skip-button-modern');
            if (skipBtn) {
                skipBtn.click();
                return true;
            }
            
            // Skip unskippable ads by jumping to end
            const video = document.querySelector('video');
            const adContainer = document.querySelector('.ad-showing, .ytp-ad-player-overlay');
            if (video && adContainer && video.duration && video.duration < 120) {
                video.currentTime = video.duration;
                return true;
            }
            
            return false;
        }
        
        // Remove ad overlays
        function removeOverlays() {
            const selectors = [
                '.ytp-ad-overlay-container',
                '.ytp-ad-text-overlay',
                '.ytp-ad-overlay-slot',
                'ytd-promoted-sparkles-web-renderer',
                'ytd-display-ad-renderer',
                'ytd-promoted-video-renderer',
                'ytd-compact-promoted-video-renderer',
                '.ytd-banner-promo-renderer',
                'ytd-in-feed-ad-layout-renderer',
                'ytd-ad-slot-renderer',
                '.ytd-mealbar-promo-renderer',
                'tp-yt-paper-dialog.ytd-popup-container',  // Premium popup
                '#masthead-ad'
            ];
            
            selectors.forEach(sel => {
                document.querySelectorAll(sel).forEach(el => {
                    el.remove();
                });
            });
        }
        
        // Mute ads
        function muteAd() {
            const video = document.querySelector('video');
            const adContainer = document.querySelector('.ad-showing');
            if (video && adContainer) {
                video.muted = true;
            }
        }
        
        // Run periodically
        setInterval(() => {
            skipAd();
            removeOverlays();
            muteAd();
        }, 500);
        
        // Also observe DOM changes
        const observer = new MutationObserver(() => {
            skipAd();
            removeOverlays();
        });
        
        observer.observe(document.body, {
            childList: true,
            subtree: true
        });
        
        console.log('[fOS-WB] YouTube ad blocker active');
    })();
    "#
}

/// Get generic cosmetic filter script (hides elements based on CSS selectors)
pub fn get_cosmetic_script(css: &str) -> String {
    if css.is_empty() {
        return String::new();
    }
    
    format!(r#"
    (function() {{
        'use strict';
        const style = document.createElement('style');
        style.textContent = `{}`;
        document.head.appendChild(style);
    }})();
    "#, css.replace('`', "\\`").replace("${", "\\${"))
}

/// Initialize the adblocker (call at startup on main thread)
pub fn init() {
    info!("Initializing enhanced adblocker...");
    let engine = create_engine();
    ADBLOCK_ENGINE.with(|e| {
        *e.borrow_mut() = Some(engine);
    });
    info!("Enhanced adblocker ready");
}

/// Force refresh all filter lists (delete cache and re-download)
pub fn refresh_filters() {
    info!("Refreshing filter lists...");
    let filter_dir = get_filter_dir();
    
    // Delete cached filters
    for (name, _) in FILTER_LISTS {
        let cache_path = filter_dir.join(format!("{}.txt", name));
        fs::remove_file(&cache_path).ok();
    }
    
    // Recreate engine
    let engine = create_engine();
    ADBLOCK_ENGINE.with(|e| {
        *e.borrow_mut() = Some(engine);
    });
    info!("Filter lists refreshed");
}
