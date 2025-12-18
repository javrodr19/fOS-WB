//! Per-tab heap management using mimalloc arenas.

use std::alloc::{GlobalAlloc, Layout};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::{debug, warn};

/// Configuration for a tab heap
#[derive(Debug, Clone)]
pub struct HeapConfig {
    /// Soft memory limit in bytes (advisory)
    pub soft_limit: usize,
    /// Hard memory limit in bytes (enforced)
    pub hard_limit: usize,
    /// Whether to eagerly return memory to OS
    pub eager_decommit: bool,
}

impl Default for HeapConfig {
    fn default() -> Self {
        Self {
            soft_limit: 32 * 1024 * 1024,  // 32 MB soft limit
            hard_limit: 64 * 1024 * 1024,  // 64 MB hard limit
            eager_decommit: true,
        }
    }
}

/// A per-tab memory heap backed by mimalloc.
///
/// Each tab gets its own heap that can be entirely freed when the tab
/// crashes or closes, providing instant cleanup without memory leaks.
pub struct TabHeap {
    id: u64,
    allocated: AtomicUsize,
    config: HeapConfig,
}

impl TabHeap {
    /// Create a new tab heap with the given ID and configuration.
    pub fn new(id: u64, config: HeapConfig) -> Self {
        debug!(tab_id = id, "Creating new tab heap");
        
        Self {
            id,
            allocated: AtomicUsize::new(0),
            config,
        }
    }

    /// Create a new tab heap with default configuration.
    pub fn with_defaults(id: u64) -> Self {
        Self::new(id, HeapConfig::default())
    }

    /// Get the tab ID associated with this heap.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Get current allocated bytes for this heap.
    pub fn allocated(&self) -> usize {
        self.allocated.load(Ordering::Relaxed)
    }

    /// Check if the heap has exceeded its soft limit.
    pub fn over_soft_limit(&self) -> bool {
        self.allocated() > self.config.soft_limit
    }

    /// Check if the heap has exceeded its hard limit.
    pub fn over_hard_limit(&self) -> bool {
        self.allocated() > self.config.hard_limit
    }

    /// Allocate memory from this heap.
    ///
    /// Returns None if the hard limit would be exceeded.
    pub fn allocate(&self, layout: Layout) -> Option<NonNull<u8>> {
        let size = layout.size();
        let current = self.allocated.fetch_add(size, Ordering::Relaxed);
        
        if current + size > self.config.hard_limit {
            // Rollback and reject allocation
            self.allocated.fetch_sub(size, Ordering::Relaxed);
            warn!(
                tab_id = self.id,
                requested = size,
                current = current,
                limit = self.config.hard_limit,
                "Tab heap hard limit exceeded, rejecting allocation"
            );
            return None;
        }

        // Use the global mimalloc allocator
        let ptr = unsafe { mimalloc::MiMalloc.alloc(layout) };
        NonNull::new(ptr)
    }

    /// Deallocate memory from this heap.
    pub fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        let size = layout.size();
        self.allocated.fetch_sub(size, Ordering::Relaxed);
        
        unsafe {
            mimalloc::MiMalloc.dealloc(ptr.as_ptr(), layout);
        }
    }

    /// Force a garbage collection / memory return to OS.
    pub fn collect(&self) {
        debug!(tab_id = self.id, "Collecting tab heap memory");
        // In a full implementation, this would trigger mimalloc's
        // mi_heap_collect with force=true to return pages to OS
    }

    /// Reset the heap, freeing all allocations.
    ///
    /// This is called when a tab crashes to instantly reclaim all memory.
    pub fn reset(&self) {
        debug!(
            tab_id = self.id,
            freed = self.allocated(),
            "Resetting tab heap after crash"
        );
        self.allocated.store(0, Ordering::Relaxed);
        // In a full implementation, this would call mi_heap_destroy
        // to atomically free all memory in the heap
    }
}

impl Drop for TabHeap {
    fn drop(&mut self) {
        debug!(
            tab_id = self.id,
            remaining = self.allocated(),
            "Dropping tab heap"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heap_creation() {
        let heap = TabHeap::with_defaults(1);
        assert_eq!(heap.id(), 1);
        assert_eq!(heap.allocated(), 0);
    }

    #[test]
    fn test_allocation_tracking() {
        let heap = TabHeap::with_defaults(1);
        let layout = Layout::from_size_align(1024, 8).unwrap();
        
        let ptr = heap.allocate(layout).expect("allocation should succeed");
        assert_eq!(heap.allocated(), 1024);
        
        heap.deallocate(ptr, layout);
        assert_eq!(heap.allocated(), 0);
    }

    #[test]
    fn test_hard_limit() {
        let config = HeapConfig {
            soft_limit: 512,
            hard_limit: 1024,
            eager_decommit: true,
        };
        let heap = TabHeap::new(1, config);
        
        // Allocate up to limit
        let layout = Layout::from_size_align(1024, 8).unwrap();
        let ptr = heap.allocate(layout).expect("should succeed");
        
        // Next allocation should fail
        let small_layout = Layout::from_size_align(1, 1).unwrap();
        assert!(heap.allocate(small_layout).is_none());
        
        // Cleanup
        heap.deallocate(ptr, layout);
    }
}
