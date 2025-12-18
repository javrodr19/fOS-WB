//! Ghost Tab System
//!
//! Maintains a low-resolution bitmap (thumbnail) of hibernated tabs
//! while the actual engine instance is killed. This allows showing
//! tab previews without loading the full page content.
//!
//! The ghost tab holds:
//! - A compressed PNG thumbnail of the page
//! - Basic metadata (title, URL, favicon)
//! - Reference to hibernated state on disk

use image::{DynamicImage, ImageBuffer, Rgba, imageops::FilterType};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;
use tracing::{debug, info};

/// Maximum thumbnail dimensions
const THUMBNAIL_WIDTH: u32 = 320;
const THUMBNAIL_HEIGHT: u32 = 180;

/// JPEG quality for thumbnails (0-100)
const THUMBNAIL_QUALITY: u8 = 75;

/// Errors from the ghost tab system
#[derive(Debug, Error)]
pub enum GhostError {
    #[error("Image encoding error: {0}")]
    ImageEncode(String),
    
    #[error("Image decoding error: {0}")]
    ImageDecode(String),
    
    #[error("No bitmap available")]
    NoBitmap,
}

/// A compressed bitmap representation of a page
#[derive(Debug, Clone)]
pub struct GhostBitmap {
    /// PNG-encoded thumbnail data
    data: Arc<Vec<u8>>,
    /// Original width before scaling
    original_width: u32,
    /// Original height before scaling
    original_height: u32,
    /// Thumbnail width
    thumbnail_width: u32,
    /// Thumbnail height
    thumbnail_height: u32,
}

impl GhostBitmap {
    /// Create a ghost bitmap from raw RGBA pixel data
    pub fn from_rgba(
        pixels: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Self, GhostError> {
        let start = Instant::now();
        
        // Create image from raw pixels
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(
            width,
            height,
            pixels.to_vec()
        ).ok_or_else(|| GhostError::ImageEncode("Invalid pixel buffer".to_string()))?;
        
        let img = DynamicImage::ImageRgba8(img);
        
        // Calculate thumbnail dimensions maintaining aspect ratio
        let (thumb_width, thumb_height) = Self::calculate_thumbnail_size(width, height);
        
        // Resize to thumbnail
        let thumbnail = img.resize_exact(thumb_width, thumb_height, FilterType::Triangle);
        
        // Encode as PNG (lossless, good compression)
        let mut png_data = Vec::new();
        thumbnail.write_to(
            &mut Cursor::new(&mut png_data),
            image::ImageFormat::Png
        ).map_err(|e| GhostError::ImageEncode(e.to_string()))?;
        
        debug!(
            "Created ghost bitmap: {}x{} -> {}x{} ({} bytes) in {:?}",
            width, height,
            thumb_width, thumb_height,
            png_data.len(),
            start.elapsed()
        );
        
        Ok(Self {
            data: Arc::new(png_data),
            original_width: width,
            original_height: height,
            thumbnail_width: thumb_width,
            thumbnail_height: thumb_height,
        })
    }

    /// Create a ghost bitmap from a pre-existing PNG
    pub fn from_png(
        png_data: Vec<u8>,
        original_width: u32,
        original_height: u32,
    ) -> Result<Self, GhostError> {
        // Decode to get dimensions
        let img = image::load_from_memory(&png_data)
            .map_err(|e| GhostError::ImageDecode(e.to_string()))?;
        
        Ok(Self {
            thumbnail_width: img.width(),
            thumbnail_height: img.height(),
            data: Arc::new(png_data),
            original_width,
            original_height,
        })
    }

    /// Get the raw PNG data
    pub fn png_data(&self) -> &[u8] {
        &self.data
    }

    /// Get memory size of the bitmap
    pub fn memory_size(&self) -> usize {
        self.data.len()
    }

    /// Get original dimensions
    pub fn original_dimensions(&self) -> (u32, u32) {
        (self.original_width, self.original_height)
    }

    /// Get thumbnail dimensions
    pub fn thumbnail_dimensions(&self) -> (u32, u32) {
        (self.thumbnail_width, self.thumbnail_height)
    }

    /// Decode the PNG back to pixels (for display)
    pub fn to_rgba(&self) -> Result<Vec<u8>, GhostError> {
        let img = image::load_from_memory(&self.data)
            .map_err(|e| GhostError::ImageDecode(e.to_string()))?;
        
        Ok(img.to_rgba8().into_raw())
    }

    fn calculate_thumbnail_size(width: u32, height: u32) -> (u32, u32) {
        let aspect = width as f32 / height as f32;
        let target_aspect = THUMBNAIL_WIDTH as f32 / THUMBNAIL_HEIGHT as f32;
        
        if aspect > target_aspect {
            // Width-constrained
            let new_width = THUMBNAIL_WIDTH;
            let new_height = (THUMBNAIL_WIDTH as f32 / aspect) as u32;
            (new_width, new_height.max(1))
        } else {
            // Height-constrained
            let new_height = THUMBNAIL_HEIGHT;
            let new_width = (THUMBNAIL_HEIGHT as f32 * aspect) as u32;
            (new_width.max(1), new_height)
        }
    }
}

/// Serializable metadata for a ghost tab
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GhostMetadata {
    /// Tab ID
    pub tab_id: u64,
    /// Page title
    pub title: String,
    /// Current URL
    pub url: String,
    /// Favicon URL (if available)
    pub favicon_url: Option<String>,
    /// Inline favicon data (small PNG)
    pub favicon_data: Option<Vec<u8>>,
    /// When the tab was ghosted
    pub ghosted_at_secs: u64,
}

/// A ghost tab - a lightweight representation of a hibernated tab
pub struct GhostTab {
    /// Metadata about the tab
    pub metadata: GhostMetadata,
    /// Low-resolution bitmap of the page
    bitmap: Option<GhostBitmap>,
    /// Whether the full tab is hibernated on disk
    is_hibernated: bool,
}

impl GhostTab {
    /// Create a new ghost tab
    pub fn new(metadata: GhostMetadata) -> Self {
        info!(
            "Created ghost tab {}: '{}'",
            metadata.tab_id, metadata.title
        );
        
        Self {
            metadata,
            bitmap: None,
            is_hibernated: false,
        }
    }

    /// Create with a bitmap
    pub fn with_bitmap(metadata: GhostMetadata, bitmap: GhostBitmap) -> Self {
        Self {
            metadata,
            bitmap: Some(bitmap),
            is_hibernated: false,
        }
    }

    /// Set the bitmap for this ghost tab
    pub fn set_bitmap(&mut self, bitmap: GhostBitmap) {
        self.bitmap = Some(bitmap);
    }

    /// Get the bitmap if available
    pub fn bitmap(&self) -> Option<&GhostBitmap> {
        self.bitmap.as_ref()
    }

    /// Take ownership of the bitmap
    pub fn take_bitmap(&mut self) -> Option<GhostBitmap> {
        self.bitmap.take()
    }

    /// Mark as hibernated (full state saved to disk)
    pub fn set_hibernated(&mut self, hibernated: bool) {
        self.is_hibernated = hibernated;
    }

    /// Check if hibernated
    pub fn is_hibernated(&self) -> bool {
        self.is_hibernated
    }

    /// Get tab ID
    pub fn tab_id(&self) -> u64 {
        self.metadata.tab_id
    }

    /// Get memory usage of this ghost tab
    pub fn memory_usage(&self) -> usize {
        let mut total = std::mem::size_of::<Self>();
        total += self.metadata.title.len();
        total += self.metadata.url.len();
        
        if let Some(ref bitmap) = self.bitmap {
            total += bitmap.memory_size();
        }
        
        if let Some(ref favicon) = self.metadata.favicon_data {
            total += favicon.len();
        }
        
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_pixels(width: u32, height: u32) -> Vec<u8> {
        // Create a simple gradient for testing
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        
        for y in 0..height {
            for x in 0..width {
                let r = (x * 255 / width) as u8;
                let g = (y * 255 / height) as u8;
                let b = 128;
                let a = 255;
                pixels.extend_from_slice(&[r, g, b, a]);
            }
        }
        
        pixels
    }

    #[test]
    fn test_bitmap_creation() {
        let width = 1920;
        let height = 1080;
        let pixels = create_test_pixels(width, height);
        
        let bitmap = GhostBitmap::from_rgba(&pixels, width, height).unwrap();
        
        assert_eq!(bitmap.original_dimensions(), (1920, 1080));
        
        // Thumbnail should be smaller
        let (tw, th) = bitmap.thumbnail_dimensions();
        assert!(tw <= THUMBNAIL_WIDTH);
        assert!(th <= THUMBNAIL_HEIGHT);
        
        // PNG should be much smaller than raw pixels
        let raw_size = (width * height * 4) as usize;
        assert!(bitmap.memory_size() < raw_size / 10);
    }

    #[test]
    fn test_bitmap_roundtrip() {
        let width = 640;
        let height = 480;
        let pixels = create_test_pixels(width, height);
        
        let bitmap = GhostBitmap::from_rgba(&pixels, width, height).unwrap();
        
        // Decode back to RGBA
        let decoded = bitmap.to_rgba().unwrap();
        
        // Decoded size matches thumbnail
        let (tw, th) = bitmap.thumbnail_dimensions();
        assert_eq!(decoded.len(), (tw * th * 4) as usize);
    }

    #[test]
    fn test_ghost_tab() {
        let metadata = GhostMetadata {
            tab_id: 1,
            title: "Example".to_string(),
            url: "https://example.com".to_string(),
            favicon_url: None,
            favicon_data: None,
            ghosted_at_secs: 12345,
        };
        
        let mut ghost = GhostTab::new(metadata);
        
        assert_eq!(ghost.tab_id(), 1);
        assert!(!ghost.is_hibernated());
        assert!(ghost.bitmap().is_none());
        
        ghost.set_hibernated(true);
        assert!(ghost.is_hibernated());
        
        // Memory usage should be small
        assert!(ghost.memory_usage() < 1024);
    }

    #[test]
    fn test_aspect_ratio_preservation() {
        // Very wide image
        let (w, h) = GhostBitmap::calculate_thumbnail_size(3840, 1080);
        assert!(w <= THUMBNAIL_WIDTH);
        assert!(h <= THUMBNAIL_HEIGHT);
        
        // Very tall image
        let (w, h) = GhostBitmap::calculate_thumbnail_size(1080, 3840);
        assert!(w <= THUMBNAIL_WIDTH);
        assert!(h <= THUMBNAIL_HEIGHT);
        
        // Square image
        let (w, h) = GhostBitmap::calculate_thumbnail_size(1000, 1000);
        assert!(w <= THUMBNAIL_WIDTH);
        assert!(h <= THUMBNAIL_HEIGHT);
    }
}
