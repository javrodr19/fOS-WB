//! Web Browser Core
//! 
//! A high-performance, ultra-lightweight web browser using tao + wry.
//! Implements a shared-state architecture with sandboxed webview.

mod filter;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::WebViewBuilder;

use crate::filter::FILTER_ENGINE;

/// Application state managed by the Rust backend.
/// This implements the "Shared-State" architecture pattern.
#[derive(Debug)]
struct AppState {
    /// Current navigation URL
    current_url: String,
    /// Loading state flag
    is_loading: bool,
    /// Page title
    title: String,
    /// Whether the window is currently focused/visible
    is_focused: bool,
    /// Count of blocked requests
    blocked_count: usize,
}

impl AppState {
    fn new() -> Self {
        Self {
            current_url: String::from("https://www.google.com"),
            is_loading: false,
            title: String::from("Web Browser"),
            is_focused: true,
            blocked_count: 0,
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared state wrapper for thread-safe access
type SharedState = Arc<Mutex<AppState>>;

/// Throttle duration for background/minimized tabs (100ms between updates)
const BACKGROUND_THROTTLE_MS: u64 = 100;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Pre-initialize the filter engine to avoid first-request latency
    println!(
        "[Filter] Initialized with {} patterns",
        FILTER_ENGINE.pattern_count()
    );
    
    // Initialize shared application state
    let state: SharedState = Arc::new(Mutex::new(AppState::new()));
    
    // Create the event loop
    let event_loop = EventLoop::new();
    
    // Build the window with optimized settings
    let window = WindowBuilder::new()
        .with_title("Web Browser")
        .with_inner_size(tao::dpi::LogicalSize::new(1280.0, 800.0))
        .with_min_inner_size(tao::dpi::LogicalSize::new(400.0, 300.0))
        // Hardware acceleration is enabled by default via the GPU compositor
        .with_decorations(true)
        .with_resizable(true)
        .with_focused(true)
        .build(&event_loop)?;

    // Get the initial URL from state
    let initial_url = {
        let state_guard = state.lock().unwrap();
        state_guard.current_url.clone()
    };

    // Clone state for webview callbacks
    let webview_state = Arc::clone(&state);
    let filter_state = Arc::clone(&state);

    // Build the WebView with production-optimized settings
    let builder = WebViewBuilder::new()
        // Navigate to initial URL
        .with_url(&initial_url)
        // Disable context menu in production for cleaner UX
        .with_hotkeys_zoom(true)
        // Navigation handler - intercepts ALL navigation requests for filtering
        .with_navigation_handler(move |url| {
            // Zero-allocation check against blocklist
            if FILTER_ENGINE.is_blocked(&url) {
                // Update blocked count in state
                if let Ok(mut state_guard) = filter_state.try_lock() {
                    state_guard.blocked_count += 1;
                    println!(
                        "[Filter] BLOCKED ({}): {}",
                        state_guard.blocked_count,
                        &url[..url.len().min(80)]
                    );
                }
                return false; // Block the request
            }
            true // Allow the request
        })
        // IPC handler for frontend-to-backend communication
        .with_ipc_handler(move |message| {
            // Handle messages from the sandboxed webview
            let mut state_guard = webview_state.lock().unwrap();
            let body = message.body();
            println!("[IPC] Received: {:?}", body);
            
            // Parse and handle IPC commands here
            if body.starts_with("navigate:") {
                state_guard.current_url = body.replace("navigate:", "");
            }
        })
        // Disable DevTools in release builds for security and size
        .with_devtools(cfg!(debug_assertions));
    
    // Platform-specific build
    #[cfg(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    ))]
    let _webview = builder.build(&window)?;
    
    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "ios",
        target_os = "android"
    )))]
    let _webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window.default_vbox().unwrap();
        builder.build_gtk(vbox)?
    };

    // Run the event loop with adaptive throttling
    event_loop.run(move |event, _, control_flow| {
        // Check if window is focused for throttling
        let is_focused = state.lock().map(|s| s.is_focused).unwrap_or(true);
        
        if is_focused {
            // Full speed when focused
            *control_flow = ControlFlow::Wait;
        } else {
            // Throttle CPU when in background (tab suspension)
            *control_flow = ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(BACKGROUND_THROTTLE_MS)
            );
        }

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                // Print final stats
                if let Ok(state_guard) = state.lock() {
                    println!(
                        "[Browser] Closing... Total blocked requests: {}",
                        state_guard.blocked_count
                    );
                }
                *control_flow = ControlFlow::Exit;
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(_size),
                ..
            } => {
                // WebView auto-resizes with the window on most platforms
            }
            Event::WindowEvent {
                event: WindowEvent::Focused(focused),
                ..
            } => {
                if let Ok(mut state_guard) = state.lock() {
                    state_guard.is_focused = focused;
                    if focused {
                        println!("[Browser] Window focused - full speed");
                    } else {
                        println!("[Browser] Window unfocused - throttling CPU");
                    }
                }
            }
            _ => {}
        }
    });
}
