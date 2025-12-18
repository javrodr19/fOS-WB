//! fOS Render - Lightweight GPU Rendering
//!
//! A minimal rendering pipeline for the browser that bypasses
//! heavy compositors by rendering directly to wgpu surfaces.
//!
//! Architecture:
//! - Direct-to-surface rendering (no intermediate compositing)
//! - Immediate-mode UI for browser chrome
//! - fontdue for CPU-rasterized text (tiny, no dependencies)
//! - wgpu for GPU-accelerated content rendering
//!
//! ## GUI Framework Evaluation
//!
//! | Framework | Binary Size | Heap Usage | Verdict |
//! |-----------|-------------|------------|---------|
//! | Qt        | +30-50 MB   | ~50 MB     | Too heavy |
//! | Electron  | +100 MB     | ~100 MB    | Way too heavy |
//! | FLTK      | +2-5 MB     | ~5 MB      | Good, but C++ |
//! | Slint     | +1-3 MB     | ~3 MB      | Good, declarative |
//! | Dear ImGui| +500 KB     | ~1 MB      | Best for dev tools |
//! | **Custom**| +100 KB     | ~500 KB    | **Chosen: minimal** |
//!
//! We use a custom immediate-mode approach with:
//! - winit for window management (~minimal overhead)
//! - wgpu for GPU rendering (~2-5 MB runtime)
//! - fontdue for text (~50 KB added size)

mod gpu;
mod surface;
mod chrome;
mod text;
mod color;
mod vpn_picker;

pub use gpu::{GpuContext, GpuConfig};
pub use surface::{RenderSurface, SurfaceConfig};
pub use chrome::{BrowserChrome, ChromeState, TabInfo, ChromeAction, DrawCommand};
pub use text::{TextRenderer, TextStyle};
pub use color::Color;
pub use vpn_picker::{
    LocationPicker, LocationAction, VpnIndicator, VpnUiState,
    RegionInfo, DrawCmd, ZeroLeakSwitch,
};
