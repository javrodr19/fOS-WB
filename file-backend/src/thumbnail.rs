//! Thumbnail generation module
//!
//! Concurrent image processing with SIMD-optimized resizing.

use image::{DynamicImage, ImageFormat, imageops::FilterType};
use std::io::Cursor;
use tokio::fs;
use tokio::task;

use crate::storage::Storage;

/// Thumbnail configuration
pub struct ThumbnailConfig {
    /// Maximum width for thumbnails
    pub max_width: u32,
    /// Maximum height for thumbnails
    pub max_height: u32,
    /// WebP quality (0-100)
    pub quality: u8,
}

impl Default for ThumbnailConfig {
    fn default() -> Self {
        Self {
            max_width: 256,
            max_height: 256,
            quality: 80,
        }
    }
}

/// Generate a thumbnail for an image file
/// 
/// This runs on a blocking thread to avoid blocking the async runtime.
/// Uses SIMD-optimized image processing internally.
pub async fn generate_thumbnail(
    storage: &Storage,
    file_id: &str,
    config: &ThumbnailConfig,
) -> Result<Vec<u8>, ThumbnailError> {
    // Check if thumbnail already exists
    if storage.thumbnail_exists(file_id).await {
        let thumb_path = storage.thumbnail_path(file_id);
        return fs::read(&thumb_path)
            .await
            .map_err(ThumbnailError::IoError);
    }
    
    // Read the original file
    let data = storage
        .read_file(file_id)
        .await
        .map_err(ThumbnailError::IoError)?;
    
    let max_width = config.max_width;
    let max_height = config.max_height;
    let quality = config.quality;
    let thumb_path = storage.thumbnail_path(file_id);
    
    // Spawn blocking task for CPU-intensive image processing
    let thumbnail_data = task::spawn_blocking(move || {
        // Load image from bytes
        let img = image::load_from_memory(&data)
            .map_err(ThumbnailError::ImageError)?;
        
        // Calculate new dimensions maintaining aspect ratio
        let (new_width, new_height) = calculate_dimensions(
            img.width(),
            img.height(),
            max_width,
            max_height,
        );
        
        // Resize using Lanczos3 filter (high quality, SIMD optimized)
        let thumbnail = img.resize_exact(
            new_width,
            new_height,
            FilterType::Lanczos3,
        );
        
        // Encode to WebP for optimal size/quality
        let mut buffer = Cursor::new(Vec::new());
        thumbnail
            .write_to(&mut buffer, ImageFormat::WebP)
            .map_err(ThumbnailError::ImageError)?;
        
        Ok::<Vec<u8>, ThumbnailError>(buffer.into_inner())
    })
    .await
    .map_err(|e| ThumbnailError::TaskError(e.to_string()))??;
    
    // Cache the thumbnail to disk
    fs::write(&thumb_path, &thumbnail_data)
        .await
        .map_err(ThumbnailError::IoError)?;
    
    Ok(thumbnail_data)
}

/// Calculate dimensions maintaining aspect ratio
fn calculate_dimensions(
    width: u32,
    height: u32,
    max_width: u32,
    max_height: u32,
) -> (u32, u32) {
    let ratio = (width as f64 / max_width as f64)
        .max(height as f64 / max_height as f64);
    
    if ratio <= 1.0 {
        // Image is smaller than max dimensions
        (width, height)
    } else {
        (
            (width as f64 / ratio) as u32,
            (height as f64 / ratio) as u32,
        )
    }
}

/// Thumbnail generation errors
#[derive(Debug)]
pub enum ThumbnailError {
    IoError(std::io::Error),
    ImageError(image::ImageError),
    TaskError(String),
}

impl std::fmt::Display for ThumbnailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoError(e) => write!(f, "IO error: {}", e),
            Self::ImageError(e) => write!(f, "Image error: {}", e),
            Self::TaskError(e) => write!(f, "Task error: {}", e),
        }
    }
}

impl std::error::Error for ThumbnailError {}
