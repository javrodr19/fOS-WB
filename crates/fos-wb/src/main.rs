//! fOS-WB: Zero-Bloat Web Browser
//!
//! Main entry point for the browser. Initializes the global allocator,
//! sets up logging, and launches the browser runtime.

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

    // Initialize the browser runtime
    let runtime = fos_tabs::Runtime::new()?;
    
    // Create the UI and run the event loop
    fos_ui::run(runtime)?;

    info!("fOS-WB shutting down");
    Ok(())
}
