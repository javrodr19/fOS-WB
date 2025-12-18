//! fOS Tabs Runtime
//!
//! Implements the single-process, multi-threaded architecture with:
//! - Tab worker pool with panic isolation
//! - Message routing between UI and tab workers
//! - Watchdog for detecting unresponsive tabs

mod message;
mod runtime;
mod tab;
mod worker;
mod watchdog;

pub use message::{TabMessage, UiMessage, TabId};
pub use runtime::Runtime;
pub use tab::Tab;
