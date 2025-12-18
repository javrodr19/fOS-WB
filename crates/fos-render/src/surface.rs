//! Render Surface - Direct-to-window rendering
//!
//! Manages the wgpu surface for direct rendering to a window
//! without intermediate compositing layers.

use crate::gpu::{GpuContext, GpuError};
use crate::Color;
use std::sync::Arc;
use tracing::{debug, info};
use wgpu::{
    CommandEncoder, Device, Queue, Surface, SurfaceConfiguration,
    SurfaceTexture, TextureFormat, TextureUsages, TextureView,
};
use winit::window::Window;

/// Surface configuration
#[derive(Debug, Clone)]
pub struct SurfaceConfig {
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// VSync enabled
    pub vsync: bool,
}

impl SurfaceConfig {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            vsync: true,
        }
    }
}

/// Render surface for a window
pub struct RenderSurface<'window> {
    /// wgpu surface
    surface: Surface<'window>,
    /// Surface configuration
    config: SurfaceConfiguration,
    /// Preferred texture format
    format: TextureFormat,
    /// Device reference
    device: Arc<Device>,
    /// Queue reference
    queue: Arc<Queue>,
    /// Current dimensions
    width: u32,
    height: u32,
}

impl<'window> RenderSurface<'window> {
    /// Create a new render surface for a window
    pub fn new(
        gpu: &GpuContext,
        window: Arc<Window>,
        config: SurfaceConfig,
    ) -> Result<Self, GpuError> {
        info!("Creating render surface ({}x{})", config.width, config.height);

        // Create surface
        let surface = gpu.instance
            .create_surface(window)
            .map_err(|e| GpuError::Surface(e.to_string()))?;

        // Get surface capabilities
        let caps = surface.get_capabilities(&gpu.adapter);
        
        // Prefer sRGB format for correct color
        let format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);

        debug!("Surface format: {:?}", format);

        // Configure surface
        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format,
            width: config.width.max(1),
            height: config.height.max(1),
            present_mode: if config.vsync {
                wgpu::PresentMode::AutoVsync
            } else {
                wgpu::PresentMode::AutoNoVsync
            },
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };

        surface.configure(&gpu.device, &surface_config);

        Ok(Self {
            surface,
            config: surface_config,
            format,
            device: gpu.device.clone(),
            queue: gpu.queue.clone(),
            width: config.width,
            height: config.height,
        })
    }

    /// Resize the surface
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == self.width && height == self.height {
            return;
        }

        self.width = width.max(1);
        self.height = height.max(1);
        self.config.width = self.width;
        self.config.height = self.height;
        
        self.surface.configure(&self.device, &self.config);
        
        debug!("Surface resized to {}x{}", self.width, self.height);
    }

    /// Get current dimensions
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Get texture format
    pub fn format(&self) -> TextureFormat {
        self.format
    }

    /// Begin a new frame
    pub fn begin_frame(&self) -> Result<Frame, GpuError> {
        let output = self.surface
            .get_current_texture()
            .map_err(|e| GpuError::Surface(e.to_string()))?;

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let encoder = self.device.create_command_encoder(
            &wgpu::CommandEncoderDescriptor {
                label: Some("Frame Encoder"),
            },
        );

        Ok(Frame {
            output,
            view,
            encoder,
            queue: self.queue.clone(),
        })
    }
}

/// A frame being rendered
pub struct Frame {
    /// Surface texture output
    output: SurfaceTexture,
    /// Texture view for rendering
    pub view: TextureView,
    /// Command encoder
    pub encoder: CommandEncoder,
    /// Queue for submission
    queue: Arc<Queue>,
}

impl Frame {
    /// Clear the frame with a color
    pub fn clear(&mut self, color: Color) {
        let _render_pass = self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Clear Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(color.to_wgpu()),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        // Render pass ends when dropped
    }

    /// Submit the frame and present
    pub fn present(self) {
        self.queue.submit(std::iter::once(self.encoder.finish()));
        self.output.present();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_surface_config() {
        let config = SurfaceConfig::new(800, 600);
        assert_eq!(config.width, 800);
        assert_eq!(config.height, 600);
        assert!(config.vsync);
    }
}
