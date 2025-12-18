//! fOS Browser UI
//!
//! Provides the browser using:
//! - shell: Custom GPU-rendered chrome (tabs, address bar)
//! - webview: System WebView for full web content (HTML/CSS/JS)

mod shell;
mod webview;

pub use shell::run;
pub use webview::{run_webview, WebBrowser};
