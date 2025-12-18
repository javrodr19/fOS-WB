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

/// Session data saved to disk
#[derive(Serialize, Deserialize, Default)]
struct SessionData {
    tabs: Vec<String>,
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
fn save_session(tabs: &[String], active_tab: usize) {
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
        create_tab(&state, &tab_list, &webview_container, &address_bar, "https://duckduckgo.com", true);
    } else {
        // Restore saved tabs
        for (i, url) in saved_session.tabs.iter().enumerate() {
            let load_now = i == saved_session.active_tab;
            create_tab(&state, &tab_list, &webview_container, &address_bar, url, load_now);
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
            let urls: Vec<String> = state.tabs.iter().map(|t| {
                t.webview.uri()
                    .map(|u| u.to_string())
                    .unwrap_or_else(|| t.url.clone())
            }).collect();
            save_session(&urls, state.active_tab);
            info!("Session saved with {} tabs", urls.len());
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
                        create_tab(&s, &tl, &container, &addr, "https://duckduckgo.com", false);
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
    load_now: bool,
) {
    // Use shared persistent session for all tabs
    let session = state.borrow().session.clone();
    let webview = WebView::builder()
        .network_session(&session)
        .build();

    // Settings
    if let Some(settings) = webkit6::prelude::WebViewExt::settings(&webview) {
        settings.set_enable_javascript(true);
        settings.set_enable_smooth_scrolling(true);
        settings.set_hardware_acceleration_policy(webkit6::HardwareAccelerationPolicy::Always);
        settings.set_enable_developer_extras(false);
        settings.set_enable_webgl(false);
        settings.set_enable_webaudio(true);
        settings.set_enable_media(true);
        settings.set_enable_page_cache(false);
        settings.set_enable_offline_web_application_cache(false);
        settings.set_enable_dns_prefetching(true);
    }
    
    // Adblocker - intercept resource loads
    webview.connect_decide_policy(|wv, decision, decision_type| {
        use webkit6::PolicyDecisionType;
        
        if decision_type == PolicyDecisionType::NavigationAction 
            || decision_type == PolicyDecisionType::NewWindowAction {
            // Allow navigation
            return false;
        }
        
        // For resource requests, check the adblocker
        if decision_type == PolicyDecisionType::Response {
            if let Some(response_decision) = decision.downcast_ref::<webkit6::ResponsePolicyDecision>() {
                if let Some(request) = response_decision.request() {
                    if let Some(uri) = request.uri() {
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

    if load_now {
        webview.load_uri(url);
    }
    
    webview.set_vexpand(true);
    webview.set_hexpand(true);

    let row = ListBoxRow::new();
    let row_label = Label::new(Some(if load_now { "Loading..." } else { "New Tab" }));
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
