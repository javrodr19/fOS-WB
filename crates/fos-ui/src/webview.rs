//! WebView Browser - Full Web Engine via System WebView
//!
//! Uses wry to embed WebKitGTK on Linux for full HTML/CSS/JS support.

use tao::{
    event::{Event, StartCause, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};
use wry::WebViewBuilder;
use tracing::{info, error};

/// Run the WebView-based browser
pub fn run_webview() -> anyhow::Result<()> {
    info!("Starting fOS-WB with WebKitGTK");

    // Force X11 on Linux - wry/WebKitGTK doesn't support Wayland window handles yet
    // This must be set BEFORE any GTK/GDK initialization
    #[cfg(target_os = "linux")]
    {
        // SAFETY: Setting env var before any threads are spawned
        unsafe {
            std::env::set_var("GDK_BACKEND", "x11");
        }
        info!("Forced X11 backend for WebKitGTK");
    }

    let event_loop = EventLoop::new();
    
    let window = WindowBuilder::new()
        .with_title("fOS-WB Browser")
        .with_inner_size(tao::dpi::LogicalSize::new(1280, 800))
        .with_visible(true)
        .build(&event_loop)
        .map_err(|e| {
            error!("Failed to create window: {}", e);
            anyhow::anyhow!("Window creation failed: {}", e)
        })?;

    info!("Window created: {:?}", window.inner_size());

    // Create WebView - this embeds WebKitGTK
    let webview = WebViewBuilder::new()
        .with_url("https://duckduckgo.com")
        .with_devtools(true)
        .with_transparent(false)
        .with_navigation_handler(|uri| {
            info!("Navigating to: {}", uri);
            true
        })
        .build(&window)
        .map_err(|e| {
            error!("Failed to create WebView: {}", e);
            anyhow::anyhow!("WebView creation failed: {}", e)
        })?;

    info!("WebView created successfully!");
    info!("Browse any website - full HTML/CSS/JS support!");

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(StartCause::Init) => {
                info!("Browser ready");
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                info!("Closing browser");
                *control_flow = ControlFlow::Exit;
            }
            Event::MainEventsCleared => {
                let _ = &webview;
            }
            _ => {}
        }
    });
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
