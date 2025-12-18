//! Clean Tabbed Browser - Vertical Tabs with Lazy Loading
//!
//! Features:
//! - Vertical tabs on left
//! - URL bar at bottom
//! - Lazy loading: tabs only load when activated
//! - Keyboard shortcuts: Ctrl+T/W/R/L

use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, Entry, Label,
    ListBox, ListBoxRow, Orientation, ScrolledWindow, Separator,
    EventControllerKey, gdk::ModifierType, SelectionMode,
};
use webkit6::prelude::*;
use webkit6::WebView;
use std::cell::RefCell;
use std::rc::Rc;
use tracing::info;

/// Browser state
struct BrowserState {
    tabs: Vec<TabInfo>,
    active_tab: usize,
}

struct TabInfo {
    webview: WebView,
    row: ListBoxRow,
    url: String,
    loaded: bool,  // Track if content has been loaded
}

/// Run the browser
pub fn run_webview() -> anyhow::Result<()> {
    info!("Starting fOS-WB Browser");

    let app = Application::builder()
        .application_id("org.fos.browser")
        .build();

    app.connect_activate(|app| {
        build_ui(app);
    });

    app.run();
    
    Ok(())
}

fn build_ui(app: &Application) {
    let state = Rc::new(RefCell::new(BrowserState {
        tabs: Vec::new(),
        active_tab: 0,
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
    sidebar.set_width_request(180);
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
    let bottom_bar = GtkBox::new(Orientation::Horizontal, 4);
    bottom_bar.set_margin_start(8);
    bottom_bar.set_margin_end(8);
    bottom_bar.set_margin_top(4);
    bottom_bar.set_margin_bottom(8);

    let back_btn = Button::with_label("←");
    let forward_btn = Button::with_label("→");

    let address_bar = Entry::new();
    address_bar.set_hexpand(true);
    address_bar.set_placeholder_text(Some("Enter URL or search..."));

    bottom_bar.append(&back_btn);
    bottom_bar.append(&forward_btn);
    bottom_bar.append(&address_bar);

    content_box.append(&bottom_bar);
    main_box.append(&content_box);

    // Create first tab (this one loads immediately)
    create_tab(&state, &tab_list, &webview_container, &address_bar, "https://duckduckgo.com", true);

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
                        
                        // Hide all tabs, show selected
                        for (i, tab) in state.tabs.iter().enumerate() {
                            tab.webview.set_visible(i == idx);
                        }
                        
                        // Lazy load: if tab hasn't loaded yet, load it now
                        if !state.tabs[idx].loaded {
                            let url = state.tabs[idx].url.clone();
                            state.tabs[idx].webview.load_uri(&url);
                            state.tabs[idx].loaded = true;
                            info!("Lazy loading tab: {}", url);
                        }
                        
                        // Update address bar
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

    // === Keyboard shortcuts ===
    let key_controller = EventControllerKey::new();
    {
        let s = state.clone();
        let tl = tab_list.clone();
        let container = webview_container.clone();
        let addr = address_bar.clone();
        key_controller.connect_key_pressed(move |_, key, _, modifiers| {
            if modifiers.contains(ModifierType::CONTROL_MASK) {
                match key.name().as_deref() {
                    Some("t") => {
                        // New tab - DON'T load immediately (lazy)
                        create_tab(&s, &tl, &container, &addr, "https://duckduckgo.com", false);
                        return gtk4::glib::Propagation::Stop;
                    }
                    Some("w") => {
                        let mut state = s.borrow_mut();
                        if state.tabs.len() > 1 && state.active_tab < state.tabs.len() {
                            let idx = state.active_tab;
                            container.remove(&state.tabs[idx].webview);
                            tl.remove(&state.tabs[idx].row);
                            state.tabs.remove(idx);
                            
                            let new_idx = if idx > 0 { idx - 1 } else { 0 };
                            if new_idx < state.tabs.len() {
                                state.active_tab = new_idx;
                                state.tabs[new_idx].webview.set_visible(true);
                                tl.select_row(Some(&state.tabs[new_idx].row));
                            }
                        }
                        return gtk4::glib::Propagation::Stop;
                    }
                    Some("r") => {
                        let state = s.borrow();
                        if state.active_tab < state.tabs.len() {
                            state.tabs[state.active_tab].webview.reload();
                        }
                        return gtk4::glib::Propagation::Stop;
                    }
                    Some("l") => {
                        addr.grab_focus();
                        addr.select_region(0, -1);
                        return gtk4::glib::Propagation::Stop;
                    }
                    _ => {}
                }
            }
            gtk4::glib::Propagation::Proceed
        });
    }
    window.add_controller(key_controller);

    // Navigation buttons
    {
        let s = state.clone();
        back_btn.connect_clicked(move |_| {
            let state = s.borrow();
            if state.active_tab < state.tabs.len() {
                state.tabs[state.active_tab].webview.go_back();
            }
        });
    }
    {
        let s = state.clone();
        forward_btn.connect_clicked(move |_| {
            let state = s.borrow();
            if state.active_tab < state.tabs.len() {
                state.tabs[state.active_tab].webview.go_forward();
            }
        });
    }

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
        .sidebar listbox row { padding: 8px 12px; border-radius: 4px; margin: 2px 4px; }
        .sidebar listbox row:selected { background: alpha(@accent_color, 0.2); }
    "#);
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().unwrap(),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    window.set_child(Some(&main_box));
    window.present();

    info!("Browser ready with lazy loading");
}

fn create_tab(
    state: &Rc<RefCell<BrowserState>>,
    tab_list: &ListBox,
    container: &GtkBox,
    address_bar: &Entry,
    url: &str,
    load_now: bool,  // Whether to load immediately or defer
) {
    let webview = WebView::new();

    // Memory-optimized settings
    if let Some(settings) = webkit6::prelude::WebViewExt::settings(&webview) {
        // Essential features only
        settings.set_enable_javascript(true);
        settings.set_enable_smooth_scrolling(true);
        settings.set_hardware_acceleration_policy(webkit6::HardwareAccelerationPolicy::Always);
        
        // Disable dev tools in release (saves memory)
        #[cfg(debug_assertions)]
        settings.set_enable_developer_extras(true);
        #[cfg(not(debug_assertions))]
        settings.set_enable_developer_extras(false);
        
        // Disable unused features to save memory
        settings.set_enable_media(true);  // Keep media for videos
        settings.set_enable_webgl(false);  // Disable WebGL to save GPU memory
        settings.set_enable_webaudio(false);  // Disable WebAudio if not needed
        
        // Reduce memory usage
        settings.set_enable_page_cache(false);  // Disable back/forward cache
        settings.set_enable_offline_web_application_cache(false);
    }

    // Only load if requested (first tab loads, Ctrl+T tabs don't until selected)
    if load_now {
        webview.load_uri(url);
    }
    
    webview.set_vexpand(true);
    webview.set_hexpand(true);

    let row = ListBoxRow::new();
    let row_label = Label::new(Some(if load_now { "Loading..." } else { "New Tab" }));
    row_label.set_halign(gtk4::Align::Start);
    row_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    row_label.set_max_width_chars(18);
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

    if load_now {
        info!("Tab opened and loaded: {}", url);
    } else {
        info!("Tab created (lazy): {}", url);
    }
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
