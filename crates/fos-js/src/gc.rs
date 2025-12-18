//! Garbage Collection Triggers and Strategies
//!
//! Implements synchronous GC triggering for ultra-low-memory mode,
//! including the critical "GC on tab blur" feature.

use crate::config::MemoryMode;
use crate::heap::GcUrgency;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// GC strategy configuration
#[derive(Debug, Clone)]
pub struct GcStrategy {
    /// Memory mode affecting GC behavior
    pub memory_mode: MemoryMode,

    /// Trigger synchronous GC when tab loses focus
    pub gc_on_blur: bool,

    /// Trigger GC on navigation (page load)
    pub gc_on_navigate: bool,

    /// Minimum time between GC runs (avoid thrashing)
    pub min_gc_interval: Duration,

    /// Force full GC instead of incremental
    pub prefer_full_gc: bool,

    /// Compact heap after GC
    pub compact_after_gc: bool,
}

impl GcStrategy {
    /// Ultra-aggressive GC for minimal memory
    pub fn ultra_aggressive() -> Self {
        Self {
            memory_mode: MemoryMode::Ultra,
            gc_on_blur: true,
            gc_on_navigate: true,
            min_gc_interval: Duration::from_millis(100),
            prefer_full_gc: true,
            compact_after_gc: true,
        }
    }

    /// Balanced GC strategy
    pub fn balanced() -> Self {
        Self {
            memory_mode: MemoryMode::Conservative,
            gc_on_blur: true,
            gc_on_navigate: false,
            min_gc_interval: Duration::from_secs(1),
            prefer_full_gc: false,
            compact_after_gc: false,
        }
    }

    /// Default (standard) GC behavior
    pub fn standard() -> Self {
        Self {
            memory_mode: MemoryMode::Standard,
            gc_on_blur: false,
            gc_on_navigate: false,
            min_gc_interval: Duration::from_secs(5),
            prefer_full_gc: false,
            compact_after_gc: false,
        }
    }
}

impl Default for GcStrategy {
    fn default() -> Self {
        Self::ultra_aggressive()
    }
}

/// GC run statistics
#[derive(Debug, Clone, Default)]
pub struct GcStats {
    /// Total GC runs
    pub total_runs: u64,
    /// Full GC runs
    pub full_gc_runs: u64,
    /// Incremental GC runs
    pub incremental_runs: u64,
    /// Total pause time
    pub total_pause_time: Duration,
    /// Longest pause
    pub max_pause_time: Duration,
    /// Total bytes freed
    pub total_freed: u64,
    /// GCs triggered by blur events
    pub blur_triggered: u64,
    /// GCs triggered by memory pressure
    pub pressure_triggered: u64,
}

impl GcStats {
    /// Average pause time
    pub fn average_pause(&self) -> Duration {
        if self.total_runs == 0 {
            Duration::ZERO
        } else {
            self.total_pause_time / self.total_runs as u32
        }
    }

    /// Format as human-readable string
    pub fn format(&self) -> String {
        format!(
            "GC: {} runs ({} full), {:.2}ms avg pause, {:.2}MB freed",
            self.total_runs,
            self.full_gc_runs,
            self.average_pause().as_secs_f64() * 1000.0,
            self.total_freed as f64 / (1024.0 * 1024.0)
        )
    }
}

/// GC trigger reasons
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GcReason {
    /// Tab lost focus (user switched away)
    TabBlur,
    /// Navigation to new page
    Navigation,
    /// Memory threshold exceeded
    MemoryThreshold,
    /// Critical memory pressure
    MemoryPressure,
    /// Explicit request (from code)
    Explicit,
    /// Idle time available
    Idle,
    /// Timer-based periodic GC
    Periodic,
}

/// GC trigger manager for a tab
///
/// # Synchronous GC on Tab Blur
///
/// When a tab loses focus, we trigger a **synchronous, full GC**:
///
/// ```text
/// Tab Focus Lost
///     │
///     ▼
/// ┌─────────────────┐
/// │ Check Last GC   │ ── Too Recent? ──▶ Skip
/// └────────┬────────┘
///          │
///          ▼
/// ┌─────────────────┐
/// │ Pause JS Engine │ ← Stop all execution
/// └────────┬────────┘
///          │
///          ▼
/// ┌─────────────────┐
/// │ Mark All Roots  │ ← Full mark phase
/// └────────┬────────┘
///          │
///          ▼
/// ┌─────────────────┐
/// │ Sweep Dead Obj  │ ← Full sweep
/// └────────┬────────┘
///          │
///          ▼
/// ┌─────────────────┐
/// │ Compact Heap    │ ← Reduce fragmentation
/// └────────┬────────┘
///          │
///          ▼
/// ┌─────────────────┐
/// │ Release Memory  │ ← Return pages to OS
/// └─────────────────┘
/// ```
///
/// This is acceptable because:
/// 1. Tab is not visible - user won't notice pause
/// 2. Tab may be hibernated - want minimal footprint
/// 3. Background tabs shouldn't consume RAM
pub struct GcTrigger {
    /// Tab ID
    tab_id: u64,
    /// GC strategy
    strategy: GcStrategy,
    /// Last GC time
    last_gc: Instant,
    /// GC in progress
    gc_running: AtomicBool,
    /// Statistics
    stats: GcStats,
    /// Tab is focused
    tab_focused: AtomicBool,
}

impl GcTrigger {
    /// Create a new GC trigger
    pub fn new(tab_id: u64, strategy: GcStrategy) -> Self {
        Self {
            tab_id,
            strategy,
            last_gc: Instant::now(),
            gc_running: AtomicBool::new(false),
            stats: GcStats::default(),
            tab_focused: AtomicBool::new(true),
        }
    }

    /// Create with ultra-aggressive strategy
    pub fn ultra_aggressive(tab_id: u64) -> Self {
        Self::new(tab_id, GcStrategy::ultra_aggressive())
    }

    /// Notify that tab focus changed
    ///
    /// Returns true if GC should be triggered
    pub fn on_focus_change(&self, focused: bool) -> Option<GcRequest> {
        let was_focused = self.tab_focused.swap(focused, Ordering::Relaxed);

        // Tab lost focus (blur event)
        if was_focused && !focused && self.strategy.gc_on_blur {
            debug!("Tab {} lost focus, requesting GC", self.tab_id);
            return Some(GcRequest {
                reason: GcReason::TabBlur,
                full_gc: true,
                compact: self.strategy.compact_after_gc,
                synchronous: true, // Key: synchronous GC on blur
            });
        }

        None
    }

    /// Notify that navigation occurred
    pub fn on_navigate(&self) -> Option<GcRequest> {
        if self.strategy.gc_on_navigate && self.can_gc() {
            debug!("Tab {} navigated, requesting GC", self.tab_id);
            return Some(GcRequest {
                reason: GcReason::Navigation,
                full_gc: true,
                compact: true,
                synchronous: false, // Can be async for navigation
            });
        }
        None
    }

    /// Check if GC should run based on heap state
    pub fn check_heap(&self, urgency: GcUrgency) -> Option<GcRequest> {
        if !self.can_gc() {
            return None;
        }

        match urgency {
            GcUrgency::None => None,
            GcUrgency::Recommended => Some(GcRequest {
                reason: GcReason::MemoryThreshold,
                full_gc: self.strategy.prefer_full_gc,
                compact: false,
                synchronous: false,
            }),
            GcUrgency::Critical => Some(GcRequest {
                reason: GcReason::MemoryPressure,
                full_gc: true,
                compact: true,
                synchronous: true, // Must run now
            }),
        }
    }

    /// Record GC completion
    pub fn record_gc(&mut self, freed: u64, pause_time: Duration, reason: GcReason) {
        self.last_gc = Instant::now();
        self.gc_running.store(false, Ordering::Relaxed);

        self.stats.total_runs += 1;
        self.stats.total_freed += freed;
        self.stats.total_pause_time += pause_time;

        if pause_time > self.stats.max_pause_time {
            self.stats.max_pause_time = pause_time;
        }

        match reason {
            GcReason::TabBlur => self.stats.blur_triggered += 1,
            GcReason::MemoryPressure | GcReason::MemoryThreshold => {
                self.stats.pressure_triggered += 1
            }
            _ => {}
        }

        info!(
            "Tab {} GC complete: freed {:.2}MB in {:.2}ms (reason: {:?})",
            self.tab_id,
            freed as f64 / (1024.0 * 1024.0),
            pause_time.as_secs_f64() * 1000.0,
            reason
        );
    }

    /// Check if enough time has passed since last GC
    fn can_gc(&self) -> bool {
        if self.gc_running.load(Ordering::Relaxed) {
            return false;
        }
        self.last_gc.elapsed() >= self.strategy.min_gc_interval
    }

    /// Get statistics
    pub fn stats(&self) -> &GcStats {
        &self.stats
    }
}

/// A request to run GC
#[derive(Debug, Clone)]
pub struct GcRequest {
    /// Reason for the GC
    pub reason: GcReason,
    /// Run full (mark-sweep) GC?
    pub full_gc: bool,
    /// Compact heap after GC?
    pub compact: bool,
    /// Synchronous (blocking) GC?
    pub synchronous: bool,
}

impl GcRequest {
    /// Convert to V8 GC flags
    ///
    /// # V8 GC Implementation
    ///
    /// Call `v8::Isolate::low_memory_notification()` followed by
    /// `v8::Isolate::request_garbage_collection_for_testing()` with
    /// the appropriate mode.
    pub fn to_v8_mode(&self) -> &'static str {
        if self.full_gc {
            "kFullGarbageCollection"
        } else {
            "kMinorGarbageCollection"
        }
    }

    /// Convert to SpiderMonkey GC options
    ///
    /// # SpiderMonkey GC Implementation
    ///
    /// Call `JS_GC(cx)` for full GC or `JS_MaybeGC(cx)` for minor.
    /// For shrinking, use `JS::ShrinkingGC(cx)`.
    pub fn to_sm_function(&self) -> &'static str {
        if self.compact {
            "JS::ShrinkingGC"
        } else if self.full_gc {
            "JS_GC"
        } else {
            "JS_MaybeGC"
        }
    }

    /// Convert to JSC GC options
    ///
    /// # JSC GC Implementation
    ///
    /// Call `JSSynchronousGC(&vm)` for sync GC or
    /// `vm.heap.collectNow(CollectionScope::Full)` for full GC.
    pub fn to_jsc_scope(&self) -> &'static str {
        if self.full_gc {
            "CollectionScope::Full"
        } else {
            "CollectionScope::Eden"
        }
    }
}

/// Memory pressure levels for proactive GC
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressure {
    None,
    Moderate,    // System RAM < 20% free
    Critical,    // System RAM < 10% free
    Emergency,   // System RAM < 5% free (may OOM)
}

impl MemoryPressure {
    /// Convert to GC request if needed
    pub fn to_gc_request(&self) -> Option<GcRequest> {
        match self {
            MemoryPressure::None => None,
            MemoryPressure::Moderate => Some(GcRequest {
                reason: GcReason::MemoryPressure,
                full_gc: false,
                compact: false,
                synchronous: false,
            }),
            MemoryPressure::Critical => Some(GcRequest {
                reason: GcReason::MemoryPressure,
                full_gc: true,
                compact: false,
                synchronous: false,
            }),
            MemoryPressure::Emergency => Some(GcRequest {
                reason: GcReason::MemoryPressure,
                full_gc: true,
                compact: true,
                synchronous: true,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gc_strategy() {
        let strategy = GcStrategy::ultra_aggressive();
        
        assert!(strategy.gc_on_blur);
        assert!(strategy.prefer_full_gc);
    }

    #[test]
    fn test_gc_on_blur() {
        let trigger = GcTrigger::ultra_aggressive(1);
        
        // Focus lost should trigger GC
        let request = trigger.on_focus_change(false);
        assert!(request.is_some());
        
        let request = request.unwrap();
        assert_eq!(request.reason, GcReason::TabBlur);
        assert!(request.synchronous);
        assert!(request.full_gc);
    }

    #[test]
    fn test_gc_request_conversion() {
        let request = GcRequest {
            reason: GcReason::TabBlur,
            full_gc: true,
            compact: true,
            synchronous: true,
        };
        
        assert_eq!(request.to_v8_mode(), "kFullGarbageCollection");
        assert_eq!(request.to_sm_function(), "JS::ShrinkingGC");
        assert_eq!(request.to_jsc_scope(), "CollectionScope::Full");
    }

    #[test]
    fn test_memory_pressure() {
        assert!(MemoryPressure::None.to_gc_request().is_none());
        
        let request = MemoryPressure::Emergency.to_gc_request().unwrap();
        assert!(request.synchronous);
        assert!(request.full_gc);
        assert!(request.compact);
    }
}
