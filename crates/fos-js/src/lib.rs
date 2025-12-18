//! fOS JavaScript Engine - Ultra-Low-Memory Configuration
//!
//! This crate provides configuration and management for JavaScript
//! engines (V8, SpiderMonkey, JavaScriptCore) in ultra-low-memory mode.
//!
//! # Key Features
//!
//! 1. **JIT-less Interpreter Mode**: Disables JIT compilation to reduce
//!    memory overhead at the cost of execution speed.
//!
//! 2. **Aggressive Heap Trimming**: Synchronous GC triggers when tabs
//!    lose focus or memory pressure increases.
//!
//! 3. **Pointer Compression**: Utilizes V8's 4GB heap cage for 50%
//!    reduction in pointer sizes.
//!
//! # Memory Trade-offs
//!
//! | Mode | Baseline RAM | Peak RAM | Startup | Execution |
//! |------|-------------|----------|---------|-----------|
//! | Full JIT | 20-50 MB | 100+ MB | Fast | Very Fast |
//! | Lite JIT | 10-30 MB | 50-80 MB | Medium | Fast |
//! | Interpreter | 5-15 MB | 20-40 MB | Slow | Medium |
//! | **Ultra-Low** | 3-8 MB | 10-20 MB | Slow | Slow |

mod config;
mod heap;
mod context;
mod gc;
mod engine_flags;

pub use config::{JsEngineConfig, JitMode, MemoryMode};
pub use heap::{JsHeap, HeapLimits, HeapStats};
pub use context::{JsContext, ContextConfig};
pub use gc::{GcTrigger, GcStrategy, GcStats};
pub use engine_flags::{V8Flags, SpiderMonkeyFlags, JavaScriptCoreFlags};
