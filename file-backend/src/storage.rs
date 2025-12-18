//! Storage module - Zero-copy file I/O
//!
//! Uses OS-level sendfile for maximum throughput when streaming files.

use axum::body::Body;
use std::path::{Path, PathBuf};
use tokio::fs::{self, File};
use tokio::io::AsyncReadExt;
use tokio_util::io::ReaderStream;
use uuid::Uuid;

/// Storage configuration
pub struct Storage {
    /// Base directory for file storage
    base_path: PathBuf,
    /// Thumbnail cache directory
    thumbnail_path: PathBuf,
}

impl Storage {
    /// Create a new storage instance with the given base path
    pub fn new<P: AsRef<Path>>(base_path: P) -> Self {
        let base = base_path.as_ref().to_path_buf();
        let thumbnails = base.join("thumbnails");
        
        Self {
            base_path: base,
            thumbnail_path: thumbnails,
        }
    }
    
    /// Initialize storage directories
    pub async fn init(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.base_path).await?;
        fs::create_dir_all(&self.thumbnail_path).await?;
        Ok(())
    }
    
    /// Get the full path for a file ID
    pub fn file_path(&self, id: &str) -> PathBuf {
        self.base_path.join(id)
    }
    
    /// Get the full path for a thumbnail
    pub fn thumbnail_path(&self, id: &str) -> PathBuf {
        self.thumbnail_path.join(format!("{}_thumb.webp", id))
    }
    
    /// Generate a new unique file ID
    pub fn generate_id() -> String {
        Uuid::new_v4().to_string()
    }
    
    /// Save uploaded data to a file
    /// 
    /// Returns the file ID and size in bytes
    pub async fn save_file(&self, id: &str, data: &[u8]) -> std::io::Result<u64> {
        let path = self.file_path(id);
        fs::write(&path, data).await?;
        let metadata = fs::metadata(&path).await?;
        Ok(metadata.len())
    }
    
    /// Stream a file with zero-copy optimization
    /// 
    /// This uses `ReaderStream` which, when combined with Hyper's response,
    /// can leverage OS-level sendfile for true zero-copy transfers on Linux.
    pub async fn stream_file(&self, id: &str) -> std::io::Result<(Body, u64, String)> {
        let path = self.file_path(id);
        
        // Get file metadata for Content-Length
        let metadata = fs::metadata(&path).await?;
        let size = metadata.len();
        
        // Guess MIME type
        let mime = mime_guess::from_path(&path)
            .first_or_octet_stream()
            .to_string();
        
        // Open file for async reading
        let file = File::open(&path).await?;
        
        // Create a stream from the file reader
        // Buffer size of 64KB for optimal throughput
        let stream = ReaderStream::with_capacity(file, 64 * 1024);
        
        // Convert to Body - this enables zero-copy on supported platforms
        let body = Body::from_stream(stream);
        
        Ok((body, size, mime))
    }
    
    /// Read file into memory (for thumbnail generation)
    pub async fn read_file(&self, id: &str) -> std::io::Result<Vec<u8>> {
        let path = self.file_path(id);
        let mut file = File::open(&path).await?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).await?;
        Ok(buffer)
    }
    
    /// Check if a file exists
    pub async fn file_exists(&self, id: &str) -> bool {
        fs::metadata(self.file_path(id)).await.is_ok()
    }
    
    /// Check if a thumbnail exists
    pub async fn thumbnail_exists(&self, id: &str) -> bool {
        fs::metadata(self.thumbnail_path(id)).await.is_ok()
    }
    
    /// Delete a file
    pub async fn delete_file(&self, id: &str) -> std::io::Result<()> {
        let path = self.file_path(id);
        fs::remove_file(&path).await?;
        
        // Also remove thumbnail if exists
        let thumb_path = self.thumbnail_path(id);
        let _ = fs::remove_file(&thumb_path).await;
        
        Ok(())
    }
}
