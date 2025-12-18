//! Memory statistics collection.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Global memory statistics for the browser.
#[derive(Debug, Default)]
pub struct MemoryStats {
    /// Total allocated bytes across all heaps
    pub total_allocated: AtomicUsize,
    /// Number of active tab heaps
    pub active_heaps: AtomicUsize,
    /// Peak memory usage
    pub peak_usage: AtomicUsize,
}

impl MemoryStats {
    /// Create new memory stats tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record an allocation.
    pub fn record_alloc(&self, size: usize) {
        let new_total = self.total_allocated.fetch_add(size, Ordering::Relaxed) + size;
        
        // Update peak if needed
        let mut peak = self.peak_usage.load(Ordering::Relaxed);
        while new_total > peak {
            match self.peak_usage.compare_exchange_weak(
                peak,
                new_total,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => peak = current,
            }
        }
    }

    /// Record a deallocation.
    pub fn record_dealloc(&self, size: usize) {
        self.total_allocated.fetch_sub(size, Ordering::Relaxed);
    }

    /// Record a new heap being created.
    pub fn record_heap_created(&self) {
        self.active_heaps.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a heap being destroyed.
    pub fn record_heap_destroyed(&self) {
        self.active_heaps.fetch_sub(1, Ordering::Relaxed);
    }

    /// Get current total allocated bytes.
    pub fn total(&self) -> usize {
        self.total_allocated.load(Ordering::Relaxed)
    }

    /// Get peak memory usage.
    pub fn peak(&self) -> usize {
        self.peak_usage.load(Ordering::Relaxed)
    }

    /// Get number of active heaps.
    pub fn heap_count(&self) -> usize {
        self.active_heaps.load(Ordering::Relaxed)
    }

    /// Format memory size for display.
    pub fn format_bytes(bytes: usize) -> String {
        const KB: usize = 1024;
        const MB: usize = KB * 1024;
        const GB: usize = MB * 1024;

        if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stats_tracking() {
        let stats = MemoryStats::new();
        
        stats.record_alloc(1000);
        assert_eq!(stats.total(), 1000);
        assert_eq!(stats.peak(), 1000);
        
        stats.record_alloc(500);
        assert_eq!(stats.total(), 1500);
        assert_eq!(stats.peak(), 1500);
        
        stats.record_dealloc(1000);
        assert_eq!(stats.total(), 500);
        assert_eq!(stats.peak(), 1500); // Peak should not decrease
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(MemoryStats::format_bytes(500), "500 B");
        assert_eq!(MemoryStats::format_bytes(1024), "1.00 KB");
        assert_eq!(MemoryStats::format_bytes(1024 * 1024), "1.00 MB");
    }
}
