//! Tab state and management.

use crate::message::{TabId, TabMessage};
use crossbeam_channel::Sender;
use fos_memory::TabHeap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Global tab ID counter
static NEXT_TAB_ID: AtomicU64 = AtomicU64::new(1);

/// Generate a new unique tab ID.
fn next_tab_id() -> TabId {
    TabId::new(NEXT_TAB_ID.fetch_add(1, Ordering::Relaxed))
}

/// Represents a browser tab's state.
pub struct Tab {
    /// Unique identifier
    id: TabId,
    /// Current URL
    url: String,
    /// Page title
    title: String,
    /// Loading state
    loading: bool,
    /// Memory heap for this tab
    heap: TabHeap,
    /// Channel to send messages to this tab's worker
    sender: Sender<TabMessage>,
}

impl Tab {
    /// Create a new tab with the given worker channel.
    pub fn new(sender: Sender<TabMessage>) -> Self {
        let id = next_tab_id();
        Self {
            id,
            url: String::from("about:blank"),
            title: String::from("New Tab"),
            loading: false,
            heap: TabHeap::with_defaults(id.0),
            sender,
        }
    }

    /// Get the tab ID.
    pub fn id(&self) -> TabId {
        self.id
    }

    /// Get the current URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Set the current URL.
    pub fn set_url(&mut self, url: String) {
        self.url = url;
    }

    /// Get the page title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Set the page title.
    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    /// Check if the tab is currently loading.
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// Set loading state.
    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
    }

    /// Get the tab's memory heap.
    pub fn heap(&self) -> &TabHeap {
        &self.heap
    }

    /// Get current memory usage.
    pub fn memory_usage(&self) -> usize {
        self.heap.allocated()
    }

    /// Send a message to this tab's worker.
    pub fn send(&self, msg: TabMessage) -> Result<(), crossbeam_channel::SendError<TabMessage>> {
        self.sender.send(msg)
    }

    /// Navigate to a URL.
    pub fn navigate(&mut self, url: &str) -> Result<(), crossbeam_channel::SendError<TabMessage>> {
        self.url = url.to_string();
        self.loading = true;
        self.send(TabMessage::Navigate { url: url.to_string() })
    }

    /// Stop loading.
    pub fn stop(&mut self) -> Result<(), crossbeam_channel::SendError<TabMessage>> {
        self.loading = false;
        self.send(TabMessage::Stop)
    }

    /// Reload the page.
    pub fn reload(&self) -> Result<(), crossbeam_channel::SendError<TabMessage>> {
        self.send(TabMessage::Reload)
    }

    /// Reset the tab after a crash.
    pub fn reset_after_crash(&mut self) {
        self.heap.reset();
        self.loading = false;
        self.url = String::from("about:crash");
        self.title = String::from("Tab Crashed");
    }
}

impl Drop for Tab {
    fn drop(&mut self) {
        // Send shutdown message (ignore errors if receiver is gone)
        let _ = self.send(TabMessage::Shutdown);
    }
}
