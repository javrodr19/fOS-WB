//! fOS-WB: Zero-Bloat Web Browser
//!
//! Main entry point for the browser. Initializes the global allocator,
//! sets up logging, and launches the browser with system WebView.

use anyhow::Result;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

// Use mimalloc as the global allocator for reduced memory fragmentation
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> Result<()> {
    // Initialize logging
    let _subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .compact()
        .init();

    info!("fOS-WB starting...");
    info!("Using mimalloc allocator");
    info!("Using system WebView for full web compatibility");

    // Run the WebView-based browser
    fos_ui::run_webview()?;

    info!("fOS-WB shutting down");
    Ok(())
}
