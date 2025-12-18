//! JavaScript Engine Configuration
//!
//! Provides configuration options for running JS engines in
//! ultra-low-memory mode.

use serde::{Deserialize, Serialize};

/// JIT compilation mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JitMode {
    /// Full JIT with all tiers (baseline + optimizing)
    /// Memory: High (20-50MB baseline)
    /// Speed: Very fast
    Full,

    /// Lite JIT with only baseline compiler
    /// Memory: Medium (10-30MB baseline)
    /// Speed: Fast
    Lite,

    /// Interpreter only, no JIT compilation
    /// Memory: Low (5-15MB baseline)
    /// Speed: 5-10x slower than JIT
    Interpreter,

    /// Ultra-low memory mode: interpreter + aggressive GC
    /// Memory: Very low (3-8MB baseline)
    /// Speed: 10-20x slower than JIT
    UltraLow,
}

impl Default for JitMode {
    fn default() -> Self {
        JitMode::Interpreter // Default to low-memory mode
    }
}

/// Memory management mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryMode {
    /// Standard memory management
    Standard,

    /// Conservative: smaller initial heaps, more frequent GC
    Conservative,

    /// Aggressive: very small heaps, synchronous GC on blur
    Aggressive,

    /// Ultra: hibernation-ready, minimal footprint
    Ultra,
}

impl Default for MemoryMode {
    fn default() -> Self {
        MemoryMode::Aggressive
    }
}

/// Complete JS engine configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsEngineConfig {
    /// JIT compilation mode
    pub jit_mode: JitMode,

    /// Memory management mode
    pub memory_mode: MemoryMode,

    /// Maximum heap size per context (bytes)
    /// Default: 16 MB for ultra-low mode
    pub max_heap_size: usize,

    /// Initial heap size (bytes)
    /// Default: 1 MB
    pub initial_heap_size: usize,

    /// Heap size at which to trigger GC (bytes)
    /// Default: 75% of max
    pub gc_threshold: usize,

    /// Enable pointer compression (V8 only)
    /// Reduces pointer size from 8 to 4 bytes
    pub pointer_compression: bool,

    /// Enable synchronous GC on tab blur
    pub gc_on_blur: bool,

    /// Enable heap snapshot on hibernation
    pub snapshot_on_hibernate: bool,

    /// Maximum string length (prevents memory bombs)
    pub max_string_length: usize,

    /// Maximum array length
    pub max_array_length: usize,

    /// Stack size limit (bytes)
    pub stack_size: usize,
}

impl Default for JsEngineConfig {
    fn default() -> Self {
        Self::ultra_low()
    }
}

impl JsEngineConfig {
    /// Create configuration for ultra-low memory usage
    pub fn ultra_low() -> Self {
        Self {
            jit_mode: JitMode::UltraLow,
            memory_mode: MemoryMode::Ultra,
            max_heap_size: 16 * 1024 * 1024,       // 16 MB max
            initial_heap_size: 1 * 1024 * 1024,    // 1 MB initial
            gc_threshold: 12 * 1024 * 1024,        // 12 MB (75%)
            pointer_compression: true,
            gc_on_blur: true,
            snapshot_on_hibernate: true,
            max_string_length: 1024 * 1024,        // 1 MB max string
            max_array_length: 1024 * 1024,         // 1M elements
            stack_size: 512 * 1024,                // 512 KB stack
        }
    }

    /// Create configuration for balanced performance/memory
    pub fn balanced() -> Self {
        Self {
            jit_mode: JitMode::Lite,
            memory_mode: MemoryMode::Conservative,
            max_heap_size: 64 * 1024 * 1024,       // 64 MB max
            initial_heap_size: 4 * 1024 * 1024,    // 4 MB initial
            gc_threshold: 48 * 1024 * 1024,        // 48 MB
            pointer_compression: true,
            gc_on_blur: true,
            snapshot_on_hibernate: false,
            max_string_length: 16 * 1024 * 1024,   // 16 MB max string
            max_array_length: 16 * 1024 * 1024,    // 16M elements
            stack_size: 1024 * 1024,               // 1 MB stack
        }
    }

    /// Create configuration for maximum performance
    pub fn performance() -> Self {
        Self {
            jit_mode: JitMode::Full,
            memory_mode: MemoryMode::Standard,
            max_heap_size: 512 * 1024 * 1024,      // 512 MB max
            initial_heap_size: 16 * 1024 * 1024,   // 16 MB initial
            gc_threshold: 384 * 1024 * 1024,       // 384 MB
            pointer_compression: true,
            gc_on_blur: false,
            snapshot_on_hibernate: false,
            max_string_length: 256 * 1024 * 1024,  // 256 MB max string
            max_array_length: 256 * 1024 * 1024,   // 256M elements
            stack_size: 2 * 1024 * 1024,           // 2 MB stack
        }
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.initial_heap_size > self.max_heap_size {
            return Err(ConfigError::InvalidHeapSize);
        }
        if self.gc_threshold > self.max_heap_size {
            return Err(ConfigError::InvalidGcThreshold);
        }
        if self.stack_size < 64 * 1024 {
            return Err(ConfigError::StackTooSmall);
        }
        Ok(())
    }
}

/// Configuration errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigError {
    #[error("Initial heap size cannot exceed max heap size")]
    InvalidHeapSize,

    #[error("GC threshold cannot exceed max heap size")]
    InvalidGcThreshold,

    #[error("Stack size too small (minimum 64KB)")]
    StackTooSmall,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ultra_low_config() {
        let config = JsEngineConfig::ultra_low();
        
        assert_eq!(config.jit_mode, JitMode::UltraLow);
        assert!(config.gc_on_blur);
        assert!(config.pointer_compression);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation() {
        let mut config = JsEngineConfig::ultra_low();
        config.initial_heap_size = config.max_heap_size + 1;
        
        assert!(config.validate().is_err());
    }
}
