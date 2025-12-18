//! fOS Render - Lightweight GPU Rendering
//!
//! A minimal rendering pipeline for the browser that bypasses
//! heavy compositors by rendering directly to wgpu surfaces.

mod gpu;
mod surface;
mod chrome;
mod text;
mod color;
mod vpn_picker;
mod shapes;
mod text_render;

pub use gpu::{GpuContext, GpuConfig};
pub use surface::{RenderSurface, SurfaceConfig};
pub use chrome::{BrowserChrome, ChromeState, TabInfo, ChromeAction, DrawCommand};
pub use text::{TextRenderer, TextStyle};
pub use color::Color;
pub use vpn_picker::{
    LocationPicker, LocationAction, VpnIndicator, VpnUiState,
    RegionInfo, DrawCmd, ZeroLeakSwitch,
};
pub use shapes::{ShapeRenderer, Vertex, UiBuilder};
pub use text_render::SimpleTextRenderer;
