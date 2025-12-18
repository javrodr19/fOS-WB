//! fOS Browser UI
//!
//! Provides the browser using GTK4 + WebKitGTK6.
//! Includes built-in adblocker powered by Brave's adblock-rust engine.

mod webview;
mod adblocker;

pub use webview::{run_webview, WebBrowser};
pub use adblocker::{should_block, init as init_adblocker};
