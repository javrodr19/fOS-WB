//! Simple Shape Renderer
//!
//! Minimal wgpu pipeline for drawing colored rectangles.
//! No textures, no fonts - just flat colored shapes.

use wgpu::{
    BindGroup, BindGroupLayout, Buffer, Device, Queue, RenderPipeline,
    ShaderModule, TextureFormat, TextureView,
};
use std::sync::Arc;
use bytemuck::{Pod, Zeroable};

/// A colored vertex
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
}

impl Vertex {
    const ATTRIBS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x4,
    ];

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// Shape renderer for flat colored rectangles
pub struct ShapeRenderer {
    pipeline: RenderPipeline,
    vertex_buffer: Buffer,
    index_buffer: Buffer,
    vertices: Vec<Vertex>,
    indices: Vec<u16>,
    screen_size: (f32, f32),
    max_rects: usize,
}

impl ShapeRenderer {
    const MAX_RECTS: usize = 256;
    const VERTICES_PER_RECT: usize = 4;
    const INDICES_PER_RECT: usize = 6;

    /// Create a new shape renderer
    pub fn new(device: &Device, format: TextureFormat, width: u32, height: u32) -> Self {
        // Create shader
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shape Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/shape.wgsl").into()),
        });

        // Create pipeline
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Shape Pipeline Layout"),
            bind_group_layouts: &[],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Shape Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[Vertex::desc()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Create vertex buffer
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shape Vertex Buffer"),
            size: (Self::MAX_RECTS * Self::VERTICES_PER_RECT * std::mem::size_of::<Vertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create index buffer
        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shape Index Buffer"),
            size: (Self::MAX_RECTS * Self::INDICES_PER_RECT * std::mem::size_of::<u16>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            pipeline,
            vertex_buffer,
            index_buffer,
            vertices: Vec::with_capacity(Self::MAX_RECTS * Self::VERTICES_PER_RECT),
            indices: Vec::with_capacity(Self::MAX_RECTS * Self::INDICES_PER_RECT),
            screen_size: (width as f32, height as f32),
            max_rects: Self::MAX_RECTS,
        }
    }

    /// Resize the renderer
    pub fn resize(&mut self, width: u32, height: u32) {
        self.screen_size = (width as f32, height as f32);
    }

    /// Clear all shapes for new frame
    pub fn begin(&mut self) {
        self.vertices.clear();
        self.indices.clear();
    }

    /// Add a rectangle (screen coordinates)
    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) {
        if self.vertices.len() / Self::VERTICES_PER_RECT >= self.max_rects {
            return; // Buffer full
        }

        // Convert screen coords to NDC (-1 to 1)
        let (sw, sh) = self.screen_size;
        let x1 = (x / sw) * 2.0 - 1.0;
        let y1 = 1.0 - (y / sh) * 2.0;
        let x2 = ((x + w) / sw) * 2.0 - 1.0;
        let y2 = 1.0 - ((y + h) / sh) * 2.0;

        let base = self.vertices.len() as u16;

        // Vertices: top-left, top-right, bottom-right, bottom-left
        self.vertices.push(Vertex { position: [x1, y1], color });
        self.vertices.push(Vertex { position: [x2, y1], color });
        self.vertices.push(Vertex { position: [x2, y2], color });
        self.vertices.push(Vertex { position: [x1, y2], color });

        // Two triangles
        self.indices.extend_from_slice(&[
            base, base + 1, base + 2,
            base, base + 2, base + 3,
        ]);
    }

    /// Add a rectangle with rounded corners (approximated)
    pub fn rounded_rect(&mut self, x: f32, y: f32, w: f32, h: f32, _radius: f32, color: [f32; 4]) {
        // For simplicity, just draw a regular rect
        // True rounded corners would require more vertices
        self.rect(x, y, w, h, color);
    }

    /// Render all shapes
    pub fn render<'a>(
        &'a mut self,
        queue: &Queue,
        render_pass: &mut wgpu::RenderPass<'a>,
    ) {
        if self.vertices.is_empty() {
            return;
        }

        // Upload vertices
        queue.write_buffer(
            &self.vertex_buffer,
            0,
            bytemuck::cast_slice(&self.vertices),
        );

        // Upload indices
        queue.write_buffer(
            &self.index_buffer,
            0,
            bytemuck::cast_slice(&self.indices),
        );

        // Render
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        render_pass.draw_indexed(0..self.indices.len() as u32, 0, 0..1);
    }

    /// Get number of rects queued
    pub fn rect_count(&self) -> usize {
        self.vertices.len() / Self::VERTICES_PER_RECT
    }
}

/// Simple immediate-mode UI builder
pub struct UiBuilder<'a> {
    shapes: &'a mut ShapeRenderer,
}

impl<'a> UiBuilder<'a> {
    pub fn new(shapes: &'a mut ShapeRenderer) -> Self {
        shapes.begin();
        Self { shapes }
    }

    /// Draw a filled rectangle
    pub fn fill_rect(&mut self, x: f32, y: f32, w: f32, h: f32, color: [f32; 4]) -> &mut Self {
        self.shapes.rect(x, y, w, h, color);
        self
    }

    /// Draw a horizontal bar
    pub fn horizontal_bar(&mut self, y: f32, height: f32, color: [f32; 4]) -> &mut Self {
        let (sw, _) = self.shapes.screen_size;
        self.shapes.rect(0.0, y, sw, height, color);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vertex_layout() {
        assert_eq!(std::mem::size_of::<Vertex>(), 24); // 2 + 4 floats * 4 bytes
    }
}
