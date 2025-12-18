//! JavaScript Context Management
//!
//! Provides per-tab JavaScript context configuration with
//! integrated heap and GC management.

use crate::config::JsEngineConfig;
use crate::gc::{GcTrigger, GcRequest, GcReason, GcStrategy};
use crate::heap::{JsHeap, HeapLimits, GcUrgency};
use std::time::Instant;
use tracing::{debug, info};

/// Context configuration
#[derive(Debug, Clone)]
pub struct ContextConfig {
    /// Engine configuration
    pub engine: JsEngineConfig,
    /// GC strategy
    pub gc_strategy: GcStrategy,
    /// Enable strict mode by default
    pub strict_mode: bool,
    /// Enable WeakRefs (can cause memory retention)
    pub enable_weak_refs: bool,
    /// Enable FinalizationRegistry
    pub enable_finalization: bool,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            engine: JsEngineConfig::ultra_low(),
            gc_strategy: GcStrategy::ultra_aggressive(),
            strict_mode: true,
            enable_weak_refs: false,  // Disabled for lower memory
            enable_finalization: false,
        }
    }
}

impl ContextConfig {
    /// Ultra-low memory configuration
    pub fn ultra_low() -> Self {
        Self::default()
    }

    /// Balanced configuration
    pub fn balanced() -> Self {
        Self {
            engine: JsEngineConfig::balanced(),
            gc_strategy: GcStrategy::balanced(),
            strict_mode: true,
            enable_weak_refs: true,
            enable_finalization: true,
        }
    }
}

/// JavaScript context for a single tab
///
/// Manages the JS engine state, heap, and GC for one browser tab.
pub struct JsContext {
    /// Tab identifier
    tab_id: u64,
    /// Configuration
    config: ContextConfig,
    /// JavaScript heap
    heap: JsHeap,
    /// GC trigger
    gc_trigger: GcTrigger,
    /// Context state
    state: ContextState,
    /// Creation time
    created_at: Instant,
}

/// Context state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextState {
    /// Context is active and executing
    Active,
    /// Context is paused (tab not focused)
    Paused,
    /// Context is suspended (ready for hibernation)
    Suspended,
    /// Context has crashed
    Crashed,
}

impl JsContext {
    /// Create a new JavaScript context
    pub fn new(tab_id: u64, config: ContextConfig) -> Self {
        info!("Creating JS context for tab {} (JIT: {:?})", 
              tab_id, config.engine.jit_mode);

        let heap_limits = HeapLimits::from_config(&config.engine);
        let heap = JsHeap::new(tab_id, heap_limits);
        let gc_trigger = GcTrigger::new(tab_id, config.gc_strategy.clone());

        Self {
            tab_id,
            config,
            heap,
            gc_trigger,
            state: ContextState::Active,
            created_at: Instant::now(),
        }
    }

    /// Create with ultra-low memory settings
    pub fn ultra_low(tab_id: u64) -> Self {
        Self::new(tab_id, ContextConfig::ultra_low())
    }

    /// Get tab ID
    pub fn tab_id(&self) -> u64 {
        self.tab_id
    }

    /// Get context state
    pub fn state(&self) -> ContextState {
        self.state
    }

    /// Get heap reference
    pub fn heap(&self) -> &JsHeap {
        &self.heap
    }

    /// Get mutable heap
    pub fn heap_mut(&mut self) -> &mut JsHeap {
        &mut self.heap
    }

    /// Handle tab focus change
    ///
    /// This is the key entry point for "GC on blur":
    ///
    /// ```rust,ignore
    /// // In tab manager:
    /// fn on_tab_blur(&mut self, tab_id: u64) {
    ///     if let Some(ctx) = self.contexts.get_mut(&tab_id) {
    ///         if let Some(gc_request) = ctx.on_focus_change(false) {
    ///             self.execute_gc(tab_id, gc_request);
    ///         }
    ///     }
    /// }
    /// ```
    pub fn on_focus_change(&mut self, focused: bool) -> Option<GcRequest> {
        if focused {
            self.state = ContextState::Active;
        } else {
            self.state = ContextState::Paused;
        }

        self.gc_trigger.on_focus_change(focused)
    }

    /// Handle navigation to new page
    pub fn on_navigate(&mut self, _url: &str) -> Option<GcRequest> {
        // Reset heap for new page
        self.heap.reset();
        self.state = ContextState::Active;

        self.gc_trigger.on_navigate()
    }

    /// Check if GC should run based on current heap state
    pub fn check_gc(&self) -> Option<GcRequest> {
        let urgency = self.heap.needs_gc();
        self.gc_trigger.check_heap(urgency)
    }

    /// Prepare for hibernation
    ///
    /// 1. Trigger full GC
    /// 2. Freeze heap
    /// 3. Return serializable state
    pub fn prepare_hibernate(&mut self) -> HibernationState {
        debug!("Preparing tab {} for hibernation", self.tab_id);

        self.state = ContextState::Suspended;
        self.heap.freeze();

        HibernationState {
            tab_id: self.tab_id,
            heap_size: self.heap.allocated(),
            // In real implementation: serialize JS heap snapshot
        }
    }

    /// Restore from hibernation
    pub fn restore_from_hibernation(&mut self, _state: HibernationState) {
        debug!("Restoring tab {} from hibernation", self.tab_id);

        self.heap.unfreeze();
        self.state = ContextState::Active;
        // In real implementation: deserialize JS heap snapshot
    }

    /// Handle crash
    pub fn on_crash(&mut self, error: &str) {
        info!("Tab {} JS crash: {}", self.tab_id, error);
        
        self.state = ContextState::Crashed;
        self.heap.reset();
    }

    /// Record GC completion
    pub fn record_gc(&mut self, freed: usize, pause_ms: u64) {
        use std::time::Duration;
        
        self.heap.record_gc(freed);
        self.gc_trigger.record_gc(
            freed as u64,
            Duration::from_millis(pause_ms),
            GcReason::Explicit,
        );
    }

    /// Get uptime
    pub fn uptime(&self) -> std::time::Duration {
        self.created_at.elapsed()
    }

    /// Get memory stats summary
    pub fn memory_summary(&self) -> String {
        format!(
            "Tab {}: {} | {}",
            self.tab_id,
            self.heap.stats().format(),
            self.gc_trigger.stats().format()
        )
    }
}

/// State saved for hibernation
#[derive(Debug, Clone)]
pub struct HibernationState {
    /// Tab ID
    pub tab_id: u64,
    /// Heap size at hibernation
    pub heap_size: usize,
    // In real implementation: serialized heap snapshot bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_creation() {
        let ctx = JsContext::ultra_low(1);
        
        assert_eq!(ctx.tab_id(), 1);
        assert_eq!(ctx.state(), ContextState::Active);
    }

    #[test]
    fn test_focus_change_triggers_gc() {
        let mut ctx = JsContext::ultra_low(1);
        
        // Losing focus should trigger GC request
        let request = ctx.on_focus_change(false);
        assert!(request.is_some());
        
        let request = request.unwrap();
        assert!(request.synchronous);
        assert!(request.full_gc);
    }

    #[test]
    fn test_navigation_resets_heap() {
        let mut ctx = JsContext::ultra_low(1);
        
        // Simulate some allocations
        ctx.heap_mut().record_allocation(1024).unwrap();
        assert!(ctx.heap().allocated() > 0);
        
        // Navigation should reset
        ctx.on_navigate("https://example.com");
        assert_eq!(ctx.heap().allocated(), 0);
    }

    #[test]
    fn test_hibernation() {
        let mut ctx = JsContext::ultra_low(1);
        
        let state = ctx.prepare_hibernate();
        assert_eq!(ctx.state(), ContextState::Suspended);
        
        ctx.restore_from_hibernation(state);
        assert_eq!(ctx.state(), ContextState::Active);
    }
}
