//! JavaScript Heap Management
//!
//! Provides heap configuration and monitoring for per-tab
//! JavaScript contexts with strict memory limits.

use crate::config::JsEngineConfig;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Heap size limits
#[derive(Debug, Clone, Copy)]
pub struct HeapLimits {
    /// Maximum heap size (bytes)
    pub max_size: usize,
    /// Initial/minimum heap size (bytes)
    pub initial_size: usize,
    /// Size at which to trigger GC (bytes)
    pub gc_threshold: usize,
    /// Size at which to trigger aggressive GC (bytes)
    pub critical_threshold: usize,
}

impl HeapLimits {
    /// Create from engine config
    pub fn from_config(config: &JsEngineConfig) -> Self {
        Self {
            max_size: config.max_heap_size,
            initial_size: config.initial_heap_size,
            gc_threshold: config.gc_threshold,
            critical_threshold: (config.max_heap_size as f64 * 0.9) as usize,
        }
    }

    /// Create ultra-low memory limits
    pub fn ultra_low() -> Self {
        Self {
            max_size: 16 * 1024 * 1024,       // 16 MB
            initial_size: 1 * 1024 * 1024,    // 1 MB
            gc_threshold: 12 * 1024 * 1024,   // 12 MB
            critical_threshold: 14 * 1024 * 1024, // 14 MB
        }
    }

    /// Check if size is within limits
    pub fn is_within_limit(&self, size: usize) -> bool {
        size <= self.max_size
    }

    /// Check if GC should be triggered
    pub fn should_gc(&self, size: usize) -> bool {
        size >= self.gc_threshold
    }

    /// Check if aggressive/emergency GC is needed
    pub fn needs_emergency_gc(&self, size: usize) -> bool {
        size >= self.critical_threshold
    }
}

impl Default for HeapLimits {
    fn default() -> Self {
        Self::ultra_low()
    }
}

/// Heap statistics
#[derive(Debug, Clone, Default)]
pub struct HeapStats {
    /// Current allocated bytes
    pub allocated: usize,
    /// Peak allocated bytes
    pub peak_allocated: usize,
    /// Total bytes ever allocated
    pub total_allocated: usize,
    /// Number of GC runs
    pub gc_count: usize,
    /// Bytes freed by GC
    pub gc_freed: usize,
    /// External memory (ArrayBuffers, etc.)
    pub external_memory: usize,
}

impl HeapStats {
    /// Memory utilization as percentage
    pub fn utilization(&self, limits: &HeapLimits) -> f32 {
        (self.allocated as f32 / limits.max_size as f32) * 100.0
    }

    /// Format as human-readable string
    pub fn format(&self) -> String {
        format!(
            "Heap: {:.2}MB / {:.2}MB peak, {} GCs, {:.2}MB freed",
            self.allocated as f64 / (1024.0 * 1024.0),
            self.peak_allocated as f64 / (1024.0 * 1024.0),
            self.gc_count,
            self.gc_freed as f64 / (1024.0 * 1024.0)
        )
    }
}

/// JavaScript heap for a single context/tab
pub struct JsHeap {
    /// Tab identifier
    tab_id: u64,
    /// Heap limits
    limits: HeapLimits,
    /// Current statistics
    stats: HeapStats,
    /// Allocated bytes (atomic for thread safety)
    allocated: Arc<AtomicUsize>,
    /// Peak bytes
    peak: Arc<AtomicUsize>,
    /// Is heap frozen (hibernated)?
    frozen: bool,
}

impl JsHeap {
    /// Create a new JS heap for a tab
    pub fn new(tab_id: u64, limits: HeapLimits) -> Self {
        info!(
            "Creating JS heap for tab {} (max: {}MB)",
            tab_id,
            limits.max_size / (1024 * 1024)
        );

        Self {
            tab_id,
            limits,
            stats: HeapStats::default(),
            allocated: Arc::new(AtomicUsize::new(0)),
            peak: Arc::new(AtomicUsize::new(0)),
            frozen: false,
        }
    }

    /// Create with ultra-low limits
    pub fn ultra_low(tab_id: u64) -> Self {
        Self::new(tab_id, HeapLimits::ultra_low())
    }

    /// Get current allocated size
    pub fn allocated(&self) -> usize {
        self.allocated.load(Ordering::Relaxed)
    }

    /// Get peak allocated size
    pub fn peak_allocated(&self) -> usize {
        self.peak.load(Ordering::Relaxed)
    }

    /// Record an allocation
    pub fn record_allocation(&mut self, size: usize) -> Result<(), HeapError> {
        if self.frozen {
            return Err(HeapError::Frozen);
        }

        let new_size = self.allocated.fetch_add(size, Ordering::Relaxed) + size;
        
        // Update peak
        let mut current_peak = self.peak.load(Ordering::Relaxed);
        while new_size > current_peak {
            match self.peak.compare_exchange_weak(
                current_peak,
                new_size,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(p) => current_peak = p,
            }
        }

        // Update stats
        self.stats.allocated = new_size;
        self.stats.peak_allocated = self.peak.load(Ordering::Relaxed);
        self.stats.total_allocated += size;

        // Check limits
        if new_size > self.limits.max_size {
            warn!("Tab {} exceeded heap limit", self.tab_id);
            return Err(HeapError::OutOfMemory);
        }

        if self.limits.should_gc(new_size) {
            debug!("Tab {} heap at {}MB, GC recommended", 
                   self.tab_id, new_size / (1024 * 1024));
        }

        Ok(())
    }

    /// Record a deallocation
    pub fn record_deallocation(&mut self, size: usize) {
        let old = self.allocated.fetch_sub(size, Ordering::Relaxed);
        self.stats.allocated = old.saturating_sub(size);
    }

    /// Record GC completion
    pub fn record_gc(&mut self, freed: usize) {
        self.stats.gc_count += 1;
        self.stats.gc_freed += freed;
        self.stats.allocated = self.allocated.load(Ordering::Relaxed);

        debug!(
            "Tab {} GC completed: freed {}KB, now {}KB",
            self.tab_id,
            freed / 1024,
            self.stats.allocated / 1024
        );
    }

    /// Check if GC should run
    pub fn needs_gc(&self) -> GcUrgency {
        let allocated = self.allocated();

        if self.limits.needs_emergency_gc(allocated) {
            GcUrgency::Critical
        } else if self.limits.should_gc(allocated) {
            GcUrgency::Recommended
        } else {
            GcUrgency::None
        }
    }

    /// Freeze heap (for hibernation)
    pub fn freeze(&mut self) {
        self.frozen = true;
        debug!("Tab {} heap frozen", self.tab_id);
    }

    /// Unfreeze heap
    pub fn unfreeze(&mut self) {
        self.frozen = false;
        debug!("Tab {} heap unfrozen", self.tab_id);
    }

    /// Reset heap (after crash or navigation)
    pub fn reset(&mut self) {
        self.allocated.store(0, Ordering::Relaxed);
        self.stats = HeapStats::default();
        self.frozen = false;

        debug!("Tab {} heap reset", self.tab_id);
    }

    /// Get current stats
    pub fn stats(&self) -> &HeapStats {
        &self.stats
    }

    /// Get limits
    pub fn limits(&self) -> &HeapLimits {
        &self.limits
    }
}

/// GC urgency level
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcUrgency {
    /// No GC needed
    None,
    /// GC recommended but not urgent
    Recommended,
    /// Critical: GC must run immediately
    Critical,
}

/// Heap errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum HeapError {
    #[error("Out of memory")]
    OutOfMemory,

    #[error("Heap is frozen (hibernated)")]
    Frozen,

    #[error("Allocation too large")]
    AllocationTooLarge,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heap_limits() {
        let limits = HeapLimits::ultra_low();
        
        assert_eq!(limits.max_size, 16 * 1024 * 1024);
        assert!(limits.should_gc(13 * 1024 * 1024));
        assert!(!limits.should_gc(10 * 1024 * 1024));
    }

    #[test]
    fn test_js_heap() {
        let mut heap = JsHeap::ultra_low(1);
        
        // Record allocation
        heap.record_allocation(1024 * 1024).unwrap();
        assert_eq!(heap.allocated(), 1024 * 1024);
        
        // Record deallocation
        heap.record_deallocation(512 * 1024);
        assert_eq!(heap.allocated(), 512 * 1024);
    }

    #[test]
    fn test_heap_limits_exceeded() {
        let limits = HeapLimits {
            max_size: 1024,
            initial_size: 256,
            gc_threshold: 768,
            critical_threshold: 900,
        };
        let mut heap = JsHeap::new(1, limits);
        
        // This should fail
        let result = heap.record_allocation(2048);
        assert!(result.is_err());
    }

    #[test]
    fn test_gc_urgency() {
        let limits = HeapLimits {
            max_size: 1000,
            initial_size: 100,
            gc_threshold: 750,
            critical_threshold: 900,
        };
        let mut heap = JsHeap::new(1, limits);
        
        heap.record_allocation(500).unwrap();
        assert_eq!(heap.needs_gc(), GcUrgency::None);
        
        heap.record_allocation(300).unwrap();
        assert_eq!(heap.needs_gc(), GcUrgency::Recommended);
        
        heap.record_allocation(150).unwrap();
        assert_eq!(heap.needs_gc(), GcUrgency::Critical);
    }
}
