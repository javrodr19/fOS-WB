//! Text Renderer - CPU-rasterized text with fontdue
//!
//! Uses fontdue for minimal-overhead text rendering.
//! Text is rasterized on CPU and uploaded to GPU textures.

use crate::Color;
use std::collections::HashMap;
use tracing::debug;

/// Text style configuration
#[derive(Debug, Clone)]
pub struct TextStyle {
    /// Font size in pixels
    pub size: f32,
    /// Text color
    pub color: Color,
    /// Bold weight
    pub bold: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            size: 14.0,
            color: Color::CHROME_TEXT,
            bold: false,
        }
    }
}

impl TextStyle {
    pub fn new(size: f32) -> Self {
        Self {
            size,
            ..Default::default()
        }
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    pub fn with_bold(mut self, bold: bool) -> Self {
        self.bold = bold;
        self
    }
}

/// Text renderer using fontdue
/// 
/// For now, this is a placeholder that will be expanded when fonts are loaded.
pub struct TextRenderer {
    /// Glyph cache (size -> char -> bitmap)
    #[allow(dead_code)]
    cache: HashMap<(u32, char), Vec<u8>>,
}

impl TextRenderer {
    /// Create a new text renderer
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Measure text dimensions (placeholder)
    pub fn measure(&self, text: &str, style: &TextStyle) -> (f32, f32) {
        // Approximate: 0.6 * size per character width, size for height
        let width = text.len() as f32 * style.size * 0.6;
        let height = style.size;
        (width, height)
    }

    /// Rasterize text to an RGBA buffer (placeholder)
    pub fn rasterize(&mut self, text: &str, style: &TextStyle) -> TextBitmap {
        let (width, height) = self.measure(text, style);
        let w = (width.ceil() as u32).max(1);
        let h = (height.ceil() as u32).max(1);

        // For now, just return an empty bitmap
        // Real implementation would use fontdue to rasterize
        TextBitmap {
            data: vec![0u8; (w * h * 4) as usize],
            width: w,
            height: h,
        }
    }

    /// Clear the glyph cache
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }
}

impl Default for TextRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Rasterized text as RGBA bitmap
#[derive(Debug)]
pub struct TextBitmap {
    /// RGBA pixel data
    pub data: Vec<u8>,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_style() {
        let style = TextStyle::new(16.0)
            .with_color(Color::WHITE)
            .with_bold(true);
        
        assert_eq!(style.size, 16.0);
        assert!(style.bold);
    }

    #[test]
    fn test_text_measure() {
        let renderer = TextRenderer::new();
        let style = TextStyle::new(14.0);
        
        let (w, h) = renderer.measure("Hello", &style);
        assert!(w > 0.0);
        assert!(h > 0.0);
    }
}
