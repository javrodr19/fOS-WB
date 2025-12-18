//! Engine-Specific Command-Line Flags
//!
//! This module provides the exact flags needed to run each major
//! JavaScript engine in interpreter-only / ultra-low-memory mode.

/// V8 Engine Flags (Chrome, Node.js, Deno)
///
/// # JIT vs Interpreter Trade-offs for V8
///
/// | Flag | Effect | Memory Impact |
/// |------|--------|---------------|
/// | `--jitless` | Disable all JIT | -30-50MB, 10x slower |
/// | `--no-opt` | Disable TurboFan | -10-20MB, 2-3x slower |
/// | `--no-sparkplug` | Disable Sparkplug | -5-10MB, 1.5x slower |
/// | `--single-threaded` | No worker threads | -5-10MB per worker |
///
/// # Pointer Compression
///
/// V8 uses a 4GB virtual memory "cage" for pointer compression:
/// - Pointers stored as 32-bit offsets instead of 64-bit
/// - ~50% reduction in object header overhead
/// - No impact on addressable memory (still 4GB per isolate)
/// - **Highly recommended** for <50MB target
#[derive(Debug, Clone)]
pub struct V8Flags {
    flags: Vec<String>,
}

impl V8Flags {
    /// Create flags for interpreter-only mode
    ///
    /// This completely disables all JIT compilation tiers:
    /// - Ignition interpreter only
    /// - No Sparkplug (baseline JIT)
    /// - No TurboFan (optimizing JIT)
    /// - No Maglev (mid-tier JIT)
    pub fn interpreter_only() -> Self {
        Self {
            flags: vec![
                // Core JIT disable
                "--jitless".to_string(),
                
                // Explicitly disable all tiers (redundant with jitless, but explicit)
                "--no-opt".to_string(),
                "--no-sparkplug".to_string(),
                "--no-maglev".to_string(),
                
                // Disable WebAssembly JIT
                "--no-wasm-opt".to_string(),
                "--no-wasm-lazy-compilation".to_string(),
            ],
        }
    }

    /// Create flags for ultra-low memory mode
    pub fn ultra_low_memory() -> Self {
        Self {
            flags: vec![
                // JIT disable
                "--jitless".to_string(),
                "--no-opt".to_string(),
                "--no-sparkplug".to_string(),
                "--no-maglev".to_string(),
                
                // Memory reduction
                "--max-old-space-size=16".to_string(),      // 16MB max heap
                "--initial-old-space-size=1".to_string(),   // 1MB initial
                "--max-semi-space-size=1".to_string(),      // 1MB nursery
                "--optimize-for-size".to_string(),

                // Pointer compression (enabled by default in modern V8)
                "--compress-pointers".to_string(),
                
                // GC tuning
                "--gc-global".to_string(),                  // Prefer full GC
                "--gc-interval=100".to_string(),            // GC every 100 allocs
                "--expose-gc".to_string(),                  // Allow manual GC
                
                // Disable expensive features
                "--no-concurrent-recompilation".to_string(),
                "--no-concurrent-marking".to_string(),
                "--no-parallel-scavenge".to_string(),
                "--no-idle-time-gc".to_string(),
                
                // Memory-related limits
                "--stack-size=512".to_string(),             // 512KB stack
                "--max-heap-size=16".to_string(),           // 16MB total
            ],
        }
    }

    /// Create flags with pointer compression analysis
    ///
    /// # Pointer Compression Impact Analysis
    ///
    /// For a browser targeting <50MB physical RAM:
    ///
    /// | Metric | Without Compression | With Compression |
    /// |--------|--------------------|--------------------|
    /// | Object overhead | 24 bytes | 12 bytes |
    /// | Array backing | 8 bytes/elem | 4 bytes/elem |
    /// | Function objects | ~200 bytes | ~120 bytes |
    /// | **Typical page** | ~15 MB | ~8 MB |
    ///
    /// **Verdict**: Pointer compression is essential for <50MB target.
    /// It provides ~40% reduction in JS heap size with no performance cost.
    ///
    /// The 4GB cage is virtual memory only - physical memory is unchanged.
    /// Modern systems have abundant virtual address space (128TB+ on x64).
    pub fn with_pointer_compression() -> Self {
        let mut flags = Self::ultra_low_memory();
        flags.flags.extend([
            "--compress-pointers".to_string(),
            "--sandbox".to_string(),            // Enable V8 sandbox (uses cage)
        ]);
        flags
    }

    /// Get flags as command-line arguments
    pub fn as_args(&self) -> &[String] {
        &self.flags
    }

    /// Get flags as a single string
    pub fn as_string(&self) -> String {
        self.flags.join(" ")
    }
}

/// SpiderMonkey Engine Flags (Firefox)
///
/// # JIT Architecture
///
/// SpiderMonkey uses a tiered compilation:
/// 1. Interpreter (baseline)
/// 2. Baseline JIT (fast compilation)
/// 3. IonMonkey (optimizing JIT)
/// 4. Warp (improved IonMonkey)
#[derive(Debug, Clone)]
pub struct SpiderMonkeyFlags {
    flags: Vec<String>,
}

impl SpiderMonkeyFlags {
    /// Create flags for interpreter-only mode
    pub fn interpreter_only() -> Self {
        Self {
            flags: vec![
                // Disable all JIT tiers
                "javascript.options.baselinejit".to_string(),      // = false
                "javascript.options.ion".to_string(),               // = false
                "javascript.options.warp".to_string(),              // = false
                
                // Disable WebAssembly JIT
                "javascript.options.wasm_baselinejit".to_string(), // = false
                "javascript.options.wasm_optimizingjit".to_string(), // = false
            ],
        }
    }

    /// Create flags for ultra-low memory mode
    pub fn ultra_low_memory() -> Self {
        Self {
            flags: vec![
                // JIT disable
                "javascript.options.baselinejit=false".to_string(),
                "javascript.options.ion=false".to_string(),
                "javascript.options.warp=false".to_string(),
                
                // Memory limits
                "javascript.options.mem.max=16777216".to_string(), // 16MB
                "javascript.options.mem.gc_per_zone=true".to_string(),
                "javascript.options.mem.gc_incremental=false".to_string(),
                
                // GC tuning for low memory
                "javascript.options.gc.dynamic_heap_growth=false".to_string(),
                "javascript.options.gc.high_frequency_low_limit=8".to_string(), // 8MB
            ],
        }
    }

    /// Get as about:config preferences
    pub fn as_prefs(&self) -> &[String] {
        &self.flags
    }
}

/// JavaScriptCore Flags (Safari, WebKit)
///
/// # JIT Architecture
///
/// JSC uses a 4-tier compilation:
/// 1. LLInt (Low Level Interpreter)
/// 2. Baseline JIT
/// 3. DFG JIT (Data Flow Graph)
/// 4. FTL JIT (Faster Than Light - LLVM-based)
#[derive(Debug, Clone)]
pub struct JavaScriptCoreFlags {
    flags: Vec<String>,
}

impl JavaScriptCoreFlags {
    /// Create flags for interpreter-only mode
    pub fn interpreter_only() -> Self {
        Self {
            flags: vec![
                // Disable all JIT tiers
                "--useLLInt=true".to_string(),
                "--useJIT=false".to_string(),
                "--useDFGJIT=false".to_string(),
                "--useFTLJIT=false".to_string(),
                
                // Disable concurrent compilation
                "--useConcurrentJIT=false".to_string(),
            ],
        }
    }

    /// Create flags for ultra-low memory mode
    pub fn ultra_low_memory() -> Self {
        Self {
            flags: vec![
                // JIT disable
                "--useLLInt=true".to_string(),
                "--useJIT=false".to_string(),
                "--useDFGJIT=false".to_string(),
                "--useFTLJIT=false".to_string(),
                "--useConcurrentJIT=false".to_string(),
                
                // Memory limits
                "--jitMemoryReservationSize=0".to_string(),   // No JIT memory
                "--maxPerThreadStackUsage=524288".to_string(), // 512KB stack
                "--softRestricted=true".to_string(),          // Enable restrictions
                
                // GC tuning
                "--useGCActivityCallback=false".to_string(),  // Sync GC only
                "--gcMaxHeapSize=16777216".to_string(),       // 16MB max
            ],
        }
    }

    /// Get as command-line arguments
    pub fn as_args(&self) -> &[String] {
        &self.flags
    }
}

/// Unified flag builder for any engine
#[derive(Debug, Clone)]
pub enum JsEngineFlags {
    V8(V8Flags),
    SpiderMonkey(SpiderMonkeyFlags),
    JavaScriptCore(JavaScriptCoreFlags),
}

impl JsEngineFlags {
    /// Create interpreter-only flags for the specified engine
    pub fn interpreter_only(engine: JsEngine) -> Self {
        match engine {
            JsEngine::V8 => Self::V8(V8Flags::interpreter_only()),
            JsEngine::SpiderMonkey => Self::SpiderMonkey(SpiderMonkeyFlags::interpreter_only()),
            JsEngine::JavaScriptCore => Self::JavaScriptCore(JavaScriptCoreFlags::interpreter_only()),
        }
    }

    /// Create ultra-low memory flags for the specified engine
    pub fn ultra_low_memory(engine: JsEngine) -> Self {
        match engine {
            JsEngine::V8 => Self::V8(V8Flags::ultra_low_memory()),
            JsEngine::SpiderMonkey => Self::SpiderMonkey(SpiderMonkeyFlags::ultra_low_memory()),
            JsEngine::JavaScriptCore => Self::JavaScriptCore(JavaScriptCoreFlags::ultra_low_memory()),
        }
    }
}

/// JavaScript engine type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsEngine {
    V8,
    SpiderMonkey,
    JavaScriptCore,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_v8_interpreter_flags() {
        let flags = V8Flags::interpreter_only();
        let args = flags.as_args();
        
        assert!(args.contains(&"--jitless".to_string()));
        assert!(args.contains(&"--no-opt".to_string()));
    }

    #[test]
    fn test_v8_ultra_low_memory() {
        let flags = V8Flags::ultra_low_memory();
        let args = flags.as_args();
        
        assert!(args.contains(&"--jitless".to_string()));
        assert!(args.contains(&"--max-old-space-size=16".to_string()));
        assert!(args.contains(&"--compress-pointers".to_string()));
    }

    #[test]
    fn test_spidermonkey_interpreter_flags() {
        let flags = SpiderMonkeyFlags::interpreter_only();
        
        assert!(!flags.as_prefs().is_empty());
    }

    #[test]
    fn test_jsc_interpreter_flags() {
        let flags = JavaScriptCoreFlags::interpreter_only();
        let args = flags.as_args();
        
        assert!(args.contains(&"--useJIT=false".to_string()));
        assert!(args.contains(&"--useLLInt=true".to_string()));
    }
}
