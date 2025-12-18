//! fOS Memory Management
//!
//! Provides per-tab heap isolation using mimalloc arenas.
//! Each tab gets its own memory heap that can be entirely freed on crash.

mod heap;
mod stats;
mod hibernation;
mod ghost;
mod rss_monitor;

pub use heap::{TabHeap, HeapConfig};
pub use stats::MemoryStats;
pub use hibernation::{ColdStorage, TabSnapshot, HibernationConfig};
pub use ghost::{GhostTab, GhostBitmap};
pub use rss_monitor::{RssMonitor, RssThreshold, MemoryPressureLevel};
