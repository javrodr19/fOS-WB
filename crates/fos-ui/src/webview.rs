//! fOS-WB - Minimal Browser with Session Persistence
//!
//! Features:
//! - Vertical tabs on left
//! - URL bar at bottom
//! - Lazy loading: tabs only load when activated
//! - Session persistence: saves tabs on close, restores on open
//! - Cookie persistence: stay logged in across restarts
//! - Full keyboard control

use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Entry, Label,
    ListBox, ListBoxRow, Orientation, ScrolledWindow, Separator,
    EventControllerKey, gdk::ModifierType, SelectionMode,
};
use webkit6::prelude::*;
use webkit6::{WebView, NetworkSession, CookiePersistentStorage};
use std::cell::RefCell;
use std::rc::Rc;
use std::path::PathBuf;
use std::fs;
use tracing::info;
use serde::{Serialize, Deserialize};

/// Tab data for session persistence
#[derive(Serialize, Deserialize, Clone)]
struct TabData {
    url: String,
    title: String,
}

/// Session data saved to disk
#[derive(Serialize, Deserialize, Default)]
struct SessionData {
    tabs: Vec<TabData>,
    active_tab: usize,
}

/// Get data directory for browser
fn get_data_dir() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("fos-wb");
    fs::create_dir_all(&dir).ok();
    dir
}

/// Load saved session
fn load_session() -> SessionData {
    let path = get_data_dir().join("session.json");
    if let Ok(data) = fs::read_to_string(&path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        SessionData::default()
    }
}

/// Save session to disk
fn save_session(tabs: &[TabData], active_tab: usize) {
    let data = SessionData { 
        tabs: tabs.to_vec(), 
        active_tab 
    };
    let path = get_data_dir().join("session.json");
    if let Ok(json) = serde_json::to_string_pretty(&data) {
        fs::write(path, json).ok();
    }
}

/// Browser state
struct BrowserState {
    tabs: Vec<TabInfo>,
    active_tab: usize,
    session: NetworkSession,
}

struct TabInfo {
    webview: WebView,
    row: ListBoxRow,
    row_label: Label,
    url: String,
    loaded: bool,
}

/// Run the browser
pub fn run_webview() -> anyhow::Result<()> {
    info!("Starting fOS-WB Browser");

    let app = Application::builder()
        .application_id("org.fos.browser")
        .build();

    app.connect_activate(|app| {
        // Initialize adblocker on main GTK thread
        crate::adblocker::init();
        build_ui(app);
    });

    app.run();
    
    Ok(())
}

fn build_ui(app: &Application) {
    // Create persistent network session for cookies
    let data_dir = get_data_dir();
    let cache_dir = data_dir.join("cache");
    fs::create_dir_all(&cache_dir).ok();
    
    let session = NetworkSession::new(
        Some(&data_dir.to_string_lossy()),
        Some(&cache_dir.to_string_lossy()),
    );
    
    // Enable persistent cookies
    if let Some(cookie_manager) = session.cookie_manager() {
        let cookies_path = data_dir.join("cookies.sqlite");
        cookie_manager.set_persistent_storage(
            &cookies_path.to_string_lossy(),
            CookiePersistentStorage::Sqlite,
        );
        info!("Cookies will persist to {:?}", cookies_path);
    }
    
    let state = Rc::new(RefCell::new(BrowserState {
        tabs: Vec::new(),
        active_tab: 0,
        session: session.clone(),
    }));

    let window = ApplicationWindow::builder()
        .application(app)
        .title("fOS-WB")
        .default_width(1280)
        .default_height(800)
        .build();

    let main_box = GtkBox::new(Orientation::Horizontal, 0);

    // === LEFT SIDEBAR (Vertical Tabs) ===
    let sidebar = GtkBox::new(Orientation::Vertical, 0);
    sidebar.set_width_request(160);
    sidebar.add_css_class("sidebar");

    let tab_list = ListBox::new();
    tab_list.set_selection_mode(SelectionMode::Single);
    tab_list.set_vexpand(true);

    let tab_scroll = ScrolledWindow::new();
    tab_scroll.set_child(Some(&tab_list));
    tab_scroll.set_vexpand(true);

    sidebar.append(&tab_scroll);
    main_box.append(&sidebar);

    let sep = Separator::new(Orientation::Vertical);
    main_box.append(&sep);

    // === CONTENT AREA ===
    let content_box = GtkBox::new(Orientation::Vertical, 0);
    content_box.set_hexpand(true);

    let webview_container = GtkBox::new(Orientation::Vertical, 0);
    webview_container.set_vexpand(true);
    webview_container.set_hexpand(true);
    content_box.append(&webview_container);

    // === BOTTOM BAR ===
    let bottom_bar = GtkBox::new(Orientation::Horizontal, 0);
    bottom_bar.set_margin_start(8);
    bottom_bar.set_margin_end(8);
    bottom_bar.set_margin_top(4);
    bottom_bar.set_margin_bottom(8);

    let address_bar = Entry::new();
    address_bar.set_hexpand(true);
    address_bar.set_placeholder_text(Some("Enter URL or search..."));

    bottom_bar.append(&address_bar);
    content_box.append(&bottom_bar);
    main_box.append(&content_box);

    // Load saved session or create default tab
    let saved_session = load_session();
    if saved_session.tabs.is_empty() {
        create_tab(&state, &tab_list, &webview_container, &address_bar, "https://duckduckgo.com", "DuckDuckGo", true);
    } else {
        // Restore saved tabs with their titles
        for (i, tab_data) in saved_session.tabs.iter().enumerate() {
            let load_now = i == saved_session.active_tab;
            create_tab(&state, &tab_list, &webview_container, &address_bar, &tab_data.url, &tab_data.title, load_now);
        }
        // Set correct active tab
        let mut s = state.borrow_mut();
        if saved_session.active_tab < s.tabs.len() {
            s.active_tab = saved_session.active_tab;
            for (i, tab) in s.tabs.iter().enumerate() {
                tab.webview.set_visible(i == saved_session.active_tab);
            }
        }
        info!("Restored {} tabs from session", saved_session.tabs.len());
    }

    // === Save session on close ===
    {
        let s = state.clone();
        window.connect_close_request(move |_| {
            let state = s.borrow();
            let tabs: Vec<TabData> = state.tabs.iter().map(|t| {
                // Get title from the row label (always up-to-date)
                let label_title = t.row_label.text().to_string();
                TabData {
                    url: t.webview.uri()
                        .map(|u| u.to_string())
                        .unwrap_or_else(|| t.url.clone()),
                    title: if label_title.is_empty() || label_title == "Loading..." {
                        t.webview.title()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| "New Tab".to_string())
                    } else {
                        label_title
                    },
                }
            }).collect();
            save_session(&tabs, state.active_tab);
            info!("Session saved with {} tabs", tabs.len());
            gtk4::glib::Propagation::Proceed
        });
    }

    // === Tab selection - LAZY LOADING ===
    {
        let s = state.clone();
        let addr = address_bar.clone();
        tab_list.connect_row_selected(move |_, row| {
            if let Some(row) = row {
                let idx = row.index() as usize;
                if let Ok(mut state) = s.try_borrow_mut() {
                    if idx < state.tabs.len() {
                        state.active_tab = idx;
                        
                        for (i, tab) in state.tabs.iter().enumerate() {
                            tab.webview.set_visible(i == idx);
                        }
                        
                        // Lazy load
                        if !state.tabs[idx].loaded {
                            let url = state.tabs[idx].url.clone();
                            state.tabs[idx].webview.load_uri(&url);
                            state.tabs[idx].loaded = true;
                        }
                        
                        if let Some(uri) = state.tabs[idx].webview.uri() {
                            addr.set_text(&uri);
                        } else {
                            addr.set_text(&state.tabs[idx].url);
                        }
                    }
                }
            }
        });
    }

    // === KEYBOARD SHORTCUTS ===
    let key_controller = EventControllerKey::new();
    {
        let s = state.clone();
        let tl = tab_list.clone();
        let container = webview_container.clone();
        let addr = address_bar.clone();
        key_controller.connect_key_pressed(move |_, key, _, modifiers| {
            if modifiers.contains(ModifierType::CONTROL_MASK) {
                match key.name().as_deref() {
                    // Ctrl+T: New tab
                    Some("t") => {
                        create_tab(&s, &tl, &container, &addr, "https://duckduckgo.com", "New Tab", false);
                        return gtk4::glib::Propagation::Stop;
                    }
                    // Ctrl+W: Close tab
                    Some("w") => {
                        let mut state = s.borrow_mut();
                        if state.tabs.len() > 1 {
                            let idx = state.active_tab;
                            if idx < state.tabs.len() {
                                container.remove(&state.tabs[idx].webview);
                                tl.remove(&state.tabs[idx].row);
                                state.tabs.remove(idx);
                                
                                let new_idx = idx.saturating_sub(1).min(state.tabs.len().saturating_sub(1));
                                state.active_tab = new_idx;
                                if new_idx < state.tabs.len() {
                                    state.tabs[new_idx].webview.set_visible(true);
                                    tl.select_row(Some(&state.tabs[new_idx].row));
                                }
                            }
                        }
                        return gtk4::glib::Propagation::Stop;
                    }
                    // Ctrl+R: Reload
                    Some("r") => {
                        let state = s.borrow();
                        if state.active_tab < state.tabs.len() {
                            state.tabs[state.active_tab].webview.reload();
                        }
                        return gtk4::glib::Propagation::Stop;
                    }
                    // Ctrl+I: Focus URL bar
                    Some("i") => {
                        addr.grab_focus();
                        addr.select_region(0, -1);
                        return gtk4::glib::Propagation::Stop;
                    }
                    // Ctrl+O: Tab above
                    Some("o") => {
                        let mut state = s.borrow_mut();
                        if state.tabs.len() > 1 && state.active_tab > 0 {
                            state.tabs[state.active_tab].webview.set_visible(false);
                            let new_idx = state.active_tab - 1;
                            state.active_tab = new_idx;
                            state.tabs[new_idx].webview.set_visible(true);
                            if !state.tabs[new_idx].loaded {
                                let url = state.tabs[new_idx].url.clone();
                                state.tabs[new_idx].webview.load_uri(&url);
                                state.tabs[new_idx].loaded = true;
                            }
                            tl.select_row(Some(&state.tabs[new_idx].row));
                            if let Some(uri) = state.tabs[new_idx].webview.uri() {
                                addr.set_text(&uri);
                            }
                        }
                        return gtk4::glib::Propagation::Stop;
                    }
                    // Ctrl+L: Tab below
                    Some("l") => {
                        let mut state = s.borrow_mut();
                        if state.tabs.len() > 1 && state.active_tab < state.tabs.len() - 1 {
                            state.tabs[state.active_tab].webview.set_visible(false);
                            let new_idx = state.active_tab + 1;
                            state.active_tab = new_idx;
                            state.tabs[new_idx].webview.set_visible(true);
                            if !state.tabs[new_idx].loaded {
                                let url = state.tabs[new_idx].url.clone();
                                state.tabs[new_idx].webview.load_uri(&url);
                                state.tabs[new_idx].loaded = true;
                            }
                            tl.select_row(Some(&state.tabs[new_idx].row));
                            if let Some(uri) = state.tabs[new_idx].webview.uri() {
                                addr.set_text(&uri);
                            }
                        }
                        return gtk4::glib::Propagation::Stop;
                    }
                    // Ctrl+K: Go back
                    Some("k") => {
                        let state = s.borrow();
                        if state.active_tab < state.tabs.len() {
                            state.tabs[state.active_tab].webview.go_back();
                        }
                        return gtk4::glib::Propagation::Stop;
                    }
                    // Ctrl+Ñ: Go forward
                    Some("ntilde") | Some("Ntilde") | Some("ñ") | Some("Ñ") => {
                        let state = s.borrow();
                        if state.active_tab < state.tabs.len() {
                            state.tabs[state.active_tab].webview.go_forward();
                        }
                        return gtk4::glib::Propagation::Stop;
                    }
                    _ => {}
                }
            }
            gtk4::glib::Propagation::Proceed
        });
    }
    window.add_controller(key_controller);

    // Address bar
    {
        let s = state.clone();
        address_bar.connect_activate(move |entry| {
            let text = entry.text().to_string();
            let url = if text.starts_with("http") {
                text
            } else if text.contains('.') {
                format!("https://{}", text)
            } else {
                format!("https://duckduckgo.com/?q={}", text.replace(' ', "+"))
            };

            let mut state = s.borrow_mut();
            let idx = state.active_tab;
            if idx < state.tabs.len() {
                state.tabs[idx].webview.load_uri(&url);
                state.tabs[idx].url = url;
                state.tabs[idx].loaded = true;
            }
        });
    }

    // CSS
    let css = gtk4::CssProvider::new();
    css.load_from_data(r#"
        .sidebar { background: shade(@window_bg_color, 0.95); }
        .sidebar listbox { background: transparent; }
        .sidebar listbox row { padding: 6px 10px; border-radius: 4px; margin: 1px 4px; }
        .sidebar listbox row:selected { background: alpha(@accent_color, 0.2); }
    "#);
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().unwrap(),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    window.set_child(Some(&main_box));
    window.present();

    info!("Browser ready with session persistence");
}

fn create_tab(
    state: &Rc<RefCell<BrowserState>>,
    tab_list: &ListBox,
    container: &GtkBox,
    address_bar: &Entry,
    url: &str,
    title: &str,
    load_now: bool,
) {
    // Use shared persistent session for all tabs
    let session = state.borrow().session.clone();
    let webview = WebView::builder()
        .network_session(&session)
        .build();

    // Settings - optimized for speed and video playback
    if let Some(settings) = webkit6::prelude::WebViewExt::settings(&webview) {
        settings.set_enable_javascript(true);
        settings.set_enable_smooth_scrolling(true);
        settings.set_enable_developer_extras(false);
        
        // Performance optimizations (HW accel disabled due to flickering on this GPU)
        settings.set_hardware_acceleration_policy(webkit6::HardwareAccelerationPolicy::Never);
        settings.set_enable_site_specific_quirks(true);    // Browser compatibility
        
        // Video playback - critical for seeking to work
        settings.set_enable_mediasource(true);      // MSE for YouTube/streaming
        settings.set_enable_media(true);
        settings.set_enable_webaudio(true);
        settings.set_enable_webgl(true);            // Required for some video players
        settings.set_enable_encrypted_media(true);  // DRM content
        settings.set_enable_fullscreen(true);       // Fullscreen video
        settings.set_media_playback_requires_user_gesture(false);
        settings.set_media_playback_allows_inline(true);  // Inline video in iframes
        
        // Caching - faster page loads
        settings.set_enable_page_cache(true);
        settings.set_enable_offline_web_application_cache(true);
        settings.set_enable_dns_prefetching(true);
        
        // Iframe permissions for embedded players
        settings.set_allow_file_access_from_file_urls(true);
        settings.set_allow_universal_access_from_file_urls(true);
        settings.set_javascript_can_open_windows_automatically(true);
    }
    
    // Adblocker - intercept resource loads (skip for media)
    webview.connect_decide_policy(|wv, decision, decision_type| {
        use webkit6::PolicyDecisionType;
        
        if decision_type == PolicyDecisionType::NavigationAction 
            || decision_type == PolicyDecisionType::NewWindowAction {
            // Allow navigation
            return false;
        }
        
        // For resource requests, check the adblocker (but skip media)
        if decision_type == PolicyDecisionType::Response {
            if let Some(response_decision) = decision.downcast_ref::<webkit6::ResponsePolicyDecision>() {
                // Skip blocking for media content types
                if let Some(response) = response_decision.response() {
                    if let Some(mime) = response.mime_type() {
                        let mime = mime.to_lowercase();
                        if mime.starts_with("video/") || mime.starts_with("audio/") 
                            || mime.contains("mp4") || mime.contains("webm") 
                            || mime.contains("mpeg") || mime.contains("ogg") {
                            return false; // Never block media
                        }
                    }
                }
                
                if let Some(request) = response_decision.request() {
                    if let Some(uri) = request.uri() {
                        // Don't block common video embed domains
                        let uri_lower = uri.to_lowercase();
                        if uri_lower.contains("youtube.com") || uri_lower.contains("ytimg.com")
                            || uri_lower.contains("vimeo.com") || uri_lower.contains("vimeocdn.com")
                            || uri_lower.contains("twitch.tv") || uri_lower.contains("dailymotion")
                            || uri_lower.contains("jwplatform.com") || uri_lower.contains("jwpcdn.com")
                            || uri_lower.contains("cloudflare") || uri_lower.contains("akamai")
                            || uri_lower.contains(".m3u8") || uri_lower.contains(".mpd") {
                            return false; // Allow video CDN and streaming
                        }
                        
                        let source = wv.uri().map(|s| s.to_string()).unwrap_or_default();
                        if crate::adblocker::should_block(&uri, &source, "other") {
                            decision.ignore();
                            return true;
                        }
                    }
                }
            }
        }
        
        false // Let WebKit handle it
    });

    // Fullscreen handlers - prevent window state corruption
    {
        let win = container.root().and_downcast::<ApplicationWindow>();
        if let Some(window) = win {
            let w = window.clone();
            webview.connect_enter_fullscreen(move |_| {
                w.fullscreen();
                true // Signal handled
            });
            let w = window.clone();
            webview.connect_leave_fullscreen(move |_| {
                w.unfullscreen();
                true
            });
        }
    }

    if load_now {
        webview.load_uri(url);
    }
    
    webview.set_vexpand(true);
    webview.set_hexpand(true);

    let row = ListBoxRow::new();
    // Use saved title, or "Loading..." if actively loading
    let initial_title = if load_now && title == "New Tab" { "Loading..." } else { title };
    let row_label = Label::new(Some(initial_title));
    row_label.set_halign(gtk4::Align::Start);
    row_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    row_label.set_max_width_chars(16);
    row.set_child(Some(&row_label));

    // Update tab title
    {
        let lbl = row_label.clone();
        webview.connect_title_notify(move |wv| {
            if let Some(title) = wv.title() {
                lbl.set_text(&title);
            }
        });
    }

    // Update address bar
    {
        let addr = address_bar.clone();
        let s = state.clone();
        let wv = webview.clone();
        webview.connect_uri_notify(move |webview| {
            if let Ok(state) = s.try_borrow() {
                if state.active_tab < state.tabs.len() {
                    if std::ptr::eq(&state.tabs[state.active_tab].webview, &wv) {
                        if let Some(uri) = webview.uri() {
                            addr.set_text(&uri);
                        }
                    }
                }
            }
        });
    }

    // Inject adblock scripts when page loads
    {
        webview.connect_load_changed(move |wv, event| {
            use webkit6::LoadEvent;
            
            // Inject scripts when DOM is ready
            if event == LoadEvent::Committed || event == LoadEvent::Finished {
                if let Some(uri) = wv.uri() {
                    let uri_str = uri.to_string();
                    
                    // Inject cosmetic filters (element hiding CSS)
                    let cosmetic_css = crate::adblocker::get_cosmetic_filters(&uri_str);
                    if !cosmetic_css.is_empty() {
                        let cosmetic_script = crate::adblocker::get_cosmetic_script(&cosmetic_css);
                        wv.evaluate_javascript(&cosmetic_script, None, None, None::<&gtk4::gio::Cancellable>, |_| {});
                    }
                    
                    // Inject YouTube ad-skip script
                    if uri_str.contains("youtube.com") || uri_str.contains("youtu.be") {
                        let youtube_script = crate::adblocker::get_youtube_adskip_script();
                        wv.evaluate_javascript(youtube_script, None, None, None::<&gtk4::gio::Cancellable>, |_| {});
                    }
                }
            }
        });
    }

    tab_list.append(&row);
    container.append(&webview);

    {
        let mut s = state.borrow_mut();
        for tab in &s.tabs {
            tab.webview.set_visible(false);
        }
        
        s.tabs.push(TabInfo {
            webview: webview.clone(),
            row: row.clone(),
            row_label: row_label.clone(),
            url: url.to_string(),
            loaded: load_now,
        });
        s.active_tab = s.tabs.len() - 1;
    }

    webview.set_visible(true);
    tab_list.select_row(Some(&row));
    address_bar.set_text(url);
}

/// Browser wrapper
pub struct WebBrowser;
impl WebBrowser {
    pub fn new() -> Self { Self }
    pub fn run(self) -> anyhow::Result<()> { run_webview() }
}
impl Default for WebBrowser {
    fn default() -> Self { Self::new() }
}
