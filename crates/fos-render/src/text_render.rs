//! Text Rendering with fontdue
//!
//! CPU-rasterized text using fontdue, uploaded to GPU textures.

use fontdue::{Font, FontSettings, Metrics};
use std::collections::HashMap;
use wgpu::{
    BindGroup, BindGroupLayout, Buffer, Device, Queue, RenderPipeline,
    Sampler, Texture, TextureView, TextureFormat,
};
use bytemuck::{Pod, Zeroable};

/// Default font (embedded)
const DEFAULT_FONT_DATA: &[u8] = include_bytes!("fonts/Inter-Regular.ttf");

/// A glyph in the atlas
#[derive(Debug, Clone, Copy)]
pub struct GlyphInfo {
    pub uv_x: f32,
    pub uv_y: f32,
    pub uv_w: f32,
    pub uv_h: f32,
    pub width: f32,
    pub height: f32,
    pub x_offset: f32,
    pub y_offset: f32,
    pub advance: f32,
}

/// Text vertex for GPU
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct TextVertex {
    pub position: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}

impl TextVertex {
    const ATTRIBS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x4,
    ];

    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TextVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRIBS,
        }
    }
}

/// Simple text renderer
///
/// Uses a single font at a fixed size for simplicity.
pub struct SimpleTextRenderer {
    font: Font,
    font_size: f32,
    glyph_cache: HashMap<char, GlyphInfo>,
    atlas_texture: Option<Texture>,
    atlas_view: Option<TextureView>,
    atlas_width: u32,
    atlas_height: u32,
    next_x: u32,
    next_y: u32,
    row_height: u32,
    pipeline: Option<RenderPipeline>,
    bind_group: Option<BindGroup>,
    bind_group_layout: Option<BindGroupLayout>,
    sampler: Option<Sampler>,
    vertex_buffer: Option<Buffer>,
    index_buffer: Option<Buffer>,
    vertices: Vec<TextVertex>,
    indices: Vec<u16>,
    screen_size: (f32, f32),
}

impl SimpleTextRenderer {
    const ATLAS_SIZE: u32 = 512;
    const MAX_CHARS: usize = 1024;

    /// Create a new text renderer
    pub fn new(font_size: f32) -> Self {
        let font = Font::from_bytes(DEFAULT_FONT_DATA, FontSettings::default())
            .expect("Failed to load embedded font");

        Self {
            font,
            font_size,
            glyph_cache: HashMap::new(),
            atlas_texture: None,
            atlas_view: None,
            atlas_width: Self::ATLAS_SIZE,
            atlas_height: Self::ATLAS_SIZE,
            next_x: 1,
            next_y: 1,
            row_height: 0,
            pipeline: None,
            bind_group: None,
            bind_group_layout: None,
            sampler: None,
            vertex_buffer: None,
            index_buffer: None,
            vertices: Vec::with_capacity(Self::MAX_CHARS * 4),
            indices: Vec::with_capacity(Self::MAX_CHARS * 6),
            screen_size: (1024.0, 768.0),
        }
    }

    /// Initialize GPU resources
    pub fn init(&mut self, device: &Device, queue: &Queue, format: TextureFormat) {
        // Create atlas texture
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Text Atlas"),
            size: wgpu::Extent3d {
                width: self.atlas_width,
                height: self.atlas_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Text Sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // Create bind group layout
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Text Bind Group Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Text Bind Group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        // Create shader
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Text Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/text.wgsl").into()),
        });

        // Create pipeline
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Text Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Text Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[TextVertex::desc()],
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
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Create buffers
        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Text Vertex Buffer"),
            size: (Self::MAX_CHARS * 4 * std::mem::size_of::<TextVertex>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Text Index Buffer"),
            size: (Self::MAX_CHARS * 6 * std::mem::size_of::<u16>()) as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Pre-cache common characters
        self.precache_glyphs(device, queue, &texture);

        self.atlas_texture = Some(texture);
        self.atlas_view = Some(view);
        self.sampler = Some(sampler);
        self.bind_group_layout = Some(bind_group_layout);
        self.bind_group = Some(bind_group);
        self.pipeline = Some(pipeline);
        self.vertex_buffer = Some(vertex_buffer);
        self.index_buffer = Some(index_buffer);
    }

    /// Pre-cache common glyphs
    fn precache_glyphs(&mut self, _device: &Device, queue: &Queue, texture: &Texture) {
        let chars: Vec<char> = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 .,;:!?'-+=/\\()[]{}@#$%^&*_<>\"~`|".chars().collect();
        
        for c in chars {
            self.cache_glyph(queue, texture, c);
        }
    }

    /// Cache a single glyph
    fn cache_glyph(&mut self, queue: &Queue, texture: &Texture, c: char) {
        if self.glyph_cache.contains_key(&c) {
            return;
        }

        let (metrics, bitmap) = self.font.rasterize(c, self.font_size);
        
        if metrics.width == 0 || metrics.height == 0 {
            // Whitespace
            self.glyph_cache.insert(c, GlyphInfo {
                uv_x: 0.0,
                uv_y: 0.0,
                uv_w: 0.0,
                uv_h: 0.0,
                width: 0.0,
                height: 0.0,
                x_offset: metrics.xmin as f32,
                y_offset: metrics.ymin as f32,
                advance: metrics.advance_width,
            });
            return;
        }

        // Check if we need to wrap to next row
        if self.next_x + metrics.width as u32 + 1 > self.atlas_width {
            self.next_x = 1;
            self.next_y += self.row_height + 1;
            self.row_height = 0;
        }

        if self.next_y + metrics.height as u32 > self.atlas_height {
            // Atlas full - would need to resize
            return;
        }

        // Upload glyph to atlas
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: self.next_x,
                    y: self.next_y,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &bitmap,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(metrics.width as u32),
                rows_per_image: Some(metrics.height as u32),
            },
            wgpu::Extent3d {
                width: metrics.width as u32,
                height: metrics.height as u32,
                depth_or_array_layers: 1,
            },
        );

        // Store glyph info
        self.glyph_cache.insert(c, GlyphInfo {
            uv_x: self.next_x as f32 / self.atlas_width as f32,
            uv_y: self.next_y as f32 / self.atlas_height as f32,
            uv_w: metrics.width as f32 / self.atlas_width as f32,
            uv_h: metrics.height as f32 / self.atlas_height as f32,
            width: metrics.width as f32,
            height: metrics.height as f32,
            x_offset: metrics.xmin as f32,
            y_offset: metrics.ymin as f32,
            advance: metrics.advance_width,
        });

        self.next_x += metrics.width as u32 + 1;
        self.row_height = self.row_height.max(metrics.height as u32);
    }

    /// Set screen size for coordinate conversion
    pub fn set_screen_size(&mut self, width: f32, height: f32) {
        self.screen_size = (width, height);
    }

    /// Begin a new text frame
    pub fn begin(&mut self) {
        self.vertices.clear();
        self.indices.clear();
    }

    /// Draw text at position
    pub fn draw(&mut self, text: &str, x: f32, y: f32, color: [f32; 4]) {
        let mut cursor_x = x;
        let (sw, sh) = self.screen_size;

        for c in text.chars() {
            if let Some(glyph) = self.glyph_cache.get(&c) {
                if glyph.width > 0.0 && glyph.height > 0.0 {
                    let base = self.vertices.len() as u16;

                    let gx = cursor_x + glyph.x_offset;
                    let gy = y - glyph.y_offset - glyph.height;

                    // Convert to NDC
                    let x1 = (gx / sw) * 2.0 - 1.0;
                    let y1 = 1.0 - (gy / sh) * 2.0;
                    let x2 = ((gx + glyph.width) / sw) * 2.0 - 1.0;
                    let y2 = 1.0 - ((gy + glyph.height) / sh) * 2.0;

                    // UV coords
                    let u1 = glyph.uv_x;
                    let v1 = glyph.uv_y;
                    let u2 = glyph.uv_x + glyph.uv_w;
                    let v2 = glyph.uv_y + glyph.uv_h;

                    self.vertices.push(TextVertex { position: [x1, y1], uv: [u1, v1], color });
                    self.vertices.push(TextVertex { position: [x2, y1], uv: [u2, v1], color });
                    self.vertices.push(TextVertex { position: [x2, y2], uv: [u2, v2], color });
                    self.vertices.push(TextVertex { position: [x1, y2], uv: [u1, v2], color });

                    self.indices.extend_from_slice(&[
                        base, base + 1, base + 2,
                        base, base + 2, base + 3,
                    ]);
                }

                cursor_x += glyph.advance;
            }
        }
    }

    /// Render all queued text
    pub fn render<'a>(&'a self, queue: &Queue, render_pass: &mut wgpu::RenderPass<'a>) {
        if self.vertices.is_empty() {
            return;
        }

        let vertex_buffer = match &self.vertex_buffer {
            Some(b) => b,
            None => return,
        };
        let index_buffer = match &self.index_buffer {
            Some(b) => b,
            None => return,
        };
        let pipeline = match &self.pipeline {
            Some(p) => p,
            None => return,
        };
        let bind_group = match &self.bind_group {
            Some(g) => g,
            None => return,
        };

        queue.write_buffer(vertex_buffer, 0, bytemuck::cast_slice(&self.vertices));
        queue.write_buffer(index_buffer, 0, bytemuck::cast_slice(&self.indices));

        render_pass.set_pipeline(pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.set_vertex_buffer(0, vertex_buffer.slice(..));
        render_pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint16);
        render_pass.draw_indexed(0..self.indices.len() as u32, 0, 0..1);
    }

    /// Measure text width
    pub fn measure(&self, text: &str) -> f32 {
        let mut width = 0.0;
        for c in text.chars() {
            if let Some(glyph) = self.glyph_cache.get(&c) {
                width += glyph.advance;
            }
        }
        width
    }
}

impl Default for SimpleTextRenderer {
    fn default() -> Self {
        Self::new(14.0)
    }
}
