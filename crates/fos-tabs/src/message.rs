//! Message types for communication between UI thread and tab workers.

use std::fmt;

/// Unique identifier for a tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub u64);

impl TabId {
    /// Create a new tab ID.
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

impl fmt::Display for TabId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tab({})", self.0)
    }
}

/// Messages sent from UI thread to tab workers.
#[derive(Debug, Clone)]
pub enum TabMessage {
    /// Navigate to a URL
    Navigate { url: String },
    /// Execute JavaScript
    ExecuteScript { script: String },
    /// Stop loading the current page
    Stop,
    /// Reload the current page
    Reload,
    /// Go back in history
    GoBack,
    /// Go forward in history
    GoForward,
    /// Request the tab to send a heartbeat (from watchdog)
    Ping,
    /// Graceful shutdown request
    Shutdown,
}

/// Messages sent from tab workers to UI thread.
#[derive(Debug, Clone)]
pub enum UiMessage {
    /// Tab has started loading
    LoadStarted { tab_id: TabId },
    /// Tab loading progress (0.0 - 1.0)
    LoadProgress { tab_id: TabId, progress: f32 },
    /// Tab has finished loading
    LoadFinished { tab_id: TabId },
    /// Tab title changed
    TitleChanged { tab_id: TabId, title: String },
    /// Tab URL changed
    UrlChanged { tab_id: TabId, url: String },
    /// Tab crashed with error
    TabCrashed { tab_id: TabId, error: String },
    /// Tab is responding to ping (heartbeat)
    Pong { tab_id: TabId },
    /// Tab is unresponsive (from watchdog)
    TabUnresponsive { tab_id: TabId },
    /// Memory usage report
    MemoryReport { tab_id: TabId, bytes: usize },
    /// Request to create UI for a new popup window
    PopupRequested { tab_id: TabId, url: String },
    /// Console message from JavaScript
    ConsoleMessage { 
        tab_id: TabId, 
        level: ConsoleLevel, 
        message: String 
    },
}

/// Console log level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleLevel {
    Log,
    Info,
    Warn,
    Error,
}

impl fmt::Display for ConsoleLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Log => write!(f, "LOG"),
            Self::Info => write!(f, "INFO"),
            Self::Warn => write!(f, "WARN"),
            Self::Error => write!(f, "ERROR"),
        }
    }
}
