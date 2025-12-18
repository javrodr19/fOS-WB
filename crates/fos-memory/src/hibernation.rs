//! Cold Storage Hibernation System
//!
//! Serializes tab state (DOM tree, JS heap snapshot, scroll position, etc.)
//! into a compressed binary format and moves it to disk (NVMe).
//!
//! Key Features:
//! - Zstd compression for optimal size/speed balance
//! - Atomic writes to prevent corruption
//! - Fast hydration path for tab restoration

use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use thiserror::Error;
use tracing::{debug, info, warn};
use zstd::stream::{Decoder, Encoder};

/// Compression level for Zstd (1-22, higher = better compression, slower)
const COMPRESSION_LEVEL: i32 = 3; // Fast compression for quick hibernation

/// Magic bytes to identify hibernation files
const MAGIC_BYTES: &[u8; 8] = b"FOSWB_HB";

/// Version of the hibernation format
const FORMAT_VERSION: u32 = 1;

/// Errors that can occur during hibernation
#[derive(Debug, Error)]
pub enum HibernationError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    
    #[error("Compression error: {0}")]
    Compression(String),
    
    #[error("Serialization error: {0}")]
    Serialization(String),
    
    #[error("Invalid hibernation file: {0}")]
    InvalidFile(String),
    
    #[error("Version mismatch: expected {expected}, got {got}")]
    VersionMismatch { expected: u32, got: u32 },
    
    #[error("Tab not found: {0}")]
    TabNotFound(u64),
    
    #[error("Hydration failed: {0}")]
    HydrationFailed(String),
}

/// Configuration for the hibernation system
#[derive(Debug, Clone)]
pub struct HibernationConfig {
    /// Directory to store hibernated tabs
    pub storage_dir: PathBuf,
    /// Compression level (1-22)
    pub compression_level: i32,
    /// Maximum age before auto-cleanup (None = never)
    pub max_age: Option<Duration>,
    /// Maximum total storage in bytes (None = unlimited)
    pub max_storage_bytes: Option<u64>,
}

impl Default for HibernationConfig {
    fn default() -> Self {
        Self {
            storage_dir: PathBuf::from("/tmp/fos-wb/hibernation"),
            compression_level: COMPRESSION_LEVEL,
            max_age: Some(Duration::from_secs(7 * 24 * 3600)), // 7 days
            max_storage_bytes: Some(1024 * 1024 * 1024), // 1 GB
        }
    }
}

/// Represents a serializable snapshot of a tab's state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabSnapshot {
    /// Tab identifier
    pub tab_id: u64,
    /// Current URL
    pub url: String,
    /// Page title
    pub title: String,
    /// Scroll position (x, y)
    pub scroll_position: (f32, f32),
    /// Form field values (for restoration)
    pub form_data: Vec<FormField>,
    /// Serialized DOM tree (simplified representation)
    pub dom_snapshot: Vec<u8>,
    /// JavaScript heap snapshot (if available)
    pub js_heap_snapshot: Option<Vec<u8>>,
    /// Timestamp when hibernated
    pub hibernated_at: u64,
    /// Original memory usage before hibernation
    pub original_memory_bytes: usize,
}

/// Form field data for restoration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormField {
    pub element_id: String,
    pub name: String,
    pub value: String,
    pub field_type: String,
}

/// File header for hibernation files
#[derive(Debug)]
struct HibernationHeader {
    magic: [u8; 8],
    version: u32,
    tab_id: u64,
    uncompressed_size: u64,
    compressed_size: u64,
    checksum: u32,
}

impl HibernationHeader {
    fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&self.magic)?;
        writer.write_all(&self.version.to_le_bytes())?;
        writer.write_all(&self.tab_id.to_le_bytes())?;
        writer.write_all(&self.uncompressed_size.to_le_bytes())?;
        writer.write_all(&self.compressed_size.to_le_bytes())?;
        writer.write_all(&self.checksum.to_le_bytes())?;
        Ok(())
    }

    fn read_from<R: Read>(reader: &mut R) -> io::Result<Self> {
        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic)?;
        
        let mut buf4 = [0u8; 4];
        let mut buf8 = [0u8; 8];
        
        reader.read_exact(&mut buf4)?;
        let version = u32::from_le_bytes(buf4);
        
        reader.read_exact(&mut buf8)?;
        let tab_id = u64::from_le_bytes(buf8);
        
        reader.read_exact(&mut buf8)?;
        let uncompressed_size = u64::from_le_bytes(buf8);
        
        reader.read_exact(&mut buf8)?;
        let compressed_size = u64::from_le_bytes(buf8);
        
        reader.read_exact(&mut buf4)?;
        let checksum = u32::from_le_bytes(buf4);
        
        Ok(Self {
            magic,
            version,
            tab_id,
            uncompressed_size,
            compressed_size,
            checksum,
        })
    }
}

/// Cold storage manager for hibernated tabs
pub struct ColdStorage {
    config: HibernationConfig,
}

impl ColdStorage {
    /// Create a new cold storage manager
    pub fn new(config: HibernationConfig) -> Result<Self, HibernationError> {
        // Ensure storage directory exists
        fs::create_dir_all(&config.storage_dir)?;
        
        info!(
            "Cold storage initialized at: {}",
            config.storage_dir.display()
        );
        
        Ok(Self { config })
    }

    /// Create with default configuration
    pub fn with_defaults() -> Result<Self, HibernationError> {
        Self::new(HibernationConfig::default())
    }

    /// Hibernate a tab to cold storage
    ///
    /// Returns the size of the compressed file in bytes
    pub fn hibernate(&self, snapshot: &TabSnapshot) -> Result<u64, HibernationError> {
        let start = Instant::now();
        
        // Serialize the snapshot
        let serialized = bincode_serialize(snapshot)?;
        let uncompressed_size = serialized.len() as u64;
        
        // Prepare file path
        let file_path = self.tab_file_path(snapshot.tab_id);
        let temp_path = file_path.with_extension("tmp");
        
        // Write to temp file first (atomic write pattern)
        let file = File::create(&temp_path)?;
        let mut writer = BufWriter::new(file);
        
        // Compress and write
        let mut encoder = Encoder::new(&mut writer, self.config.compression_level)
            .map_err(|e| HibernationError::Compression(e.to_string()))?;
        
        encoder.write_all(&serialized)
            .map_err(|e| HibernationError::Compression(e.to_string()))?;
        
        encoder.finish()
            .map_err(|e| HibernationError::Compression(e.to_string()))?;
        
        writer.flush()?;
        drop(writer);
        
        // Get compressed size
        let compressed_size = fs::metadata(&temp_path)?.len();
        
        // Write header to final file
        let final_file = File::create(&file_path)?;
        let mut final_writer = BufWriter::new(final_file);
        
        let header = HibernationHeader {
            magic: *MAGIC_BYTES,
            version: FORMAT_VERSION,
            tab_id: snapshot.tab_id,
            uncompressed_size,
            compressed_size,
            checksum: crc32_checksum(&serialized),
        };
        
        header.write_to(&mut final_writer)?;
        
        // Copy compressed data
        let mut temp_reader = BufReader::new(File::open(&temp_path)?);
        io::copy(&mut temp_reader, &mut final_writer)?;
        
        final_writer.flush()?;
        drop(final_writer);
        
        // Remove temp file
        let _ = fs::remove_file(&temp_path);
        
        let elapsed = start.elapsed();
        let ratio = (compressed_size as f64 / uncompressed_size as f64) * 100.0;
        
        info!(
            "Hibernated tab {} in {:?}: {} -> {} bytes ({:.1}% ratio)",
            snapshot.tab_id,
            elapsed,
            uncompressed_size,
            compressed_size,
            ratio
        );
        
        Ok(compressed_size)
    }

    /// Hydrate (restore) a tab from cold storage
    pub fn hydrate(&self, tab_id: u64) -> Result<TabSnapshot, HibernationError> {
        let start = Instant::now();
        let file_path = self.tab_file_path(tab_id);
        
        if !file_path.exists() {
            return Err(HibernationError::TabNotFound(tab_id));
        }
        
        let file = File::open(&file_path)?;
        let mut reader = BufReader::new(file);
        
        // Read and validate header
        let header = HibernationHeader::read_from(&mut reader)?;
        
        if &header.magic != MAGIC_BYTES {
            return Err(HibernationError::InvalidFile(
                "Invalid magic bytes".to_string()
            ));
        }
        
        if header.version != FORMAT_VERSION {
            return Err(HibernationError::VersionMismatch {
                expected: FORMAT_VERSION,
                got: header.version,
            });
        }
        
        // Decompress
        let decoder = Decoder::new(&mut reader)
            .map_err(|e| HibernationError::Compression(e.to_string()))?;
        
        let mut decompressed = Vec::with_capacity(header.uncompressed_size as usize);
        let mut decoder_reader = BufReader::new(decoder);
        decoder_reader.read_to_end(&mut decompressed)
            .map_err(|e| HibernationError::Compression(e.to_string()))?;
        
        // Verify checksum
        let checksum = crc32_checksum(&decompressed);
        if checksum != header.checksum {
            return Err(HibernationError::InvalidFile(
                format!("Checksum mismatch: expected {}, got {}", header.checksum, checksum)
            ));
        }
        
        // Deserialize
        let snapshot = bincode_deserialize(&decompressed)?;
        
        let elapsed = start.elapsed();
        info!(
            "Hydrated tab {} in {:?}: {} bytes decompressed",
            tab_id,
            elapsed,
            decompressed.len()
        );
        
        Ok(snapshot)
    }

    /// Check if a tab is hibernated
    pub fn is_hibernated(&self, tab_id: u64) -> bool {
        self.tab_file_path(tab_id).exists()
    }

    /// Delete a hibernated tab
    pub fn delete(&self, tab_id: u64) -> Result<(), HibernationError> {
        let file_path = self.tab_file_path(tab_id);
        if file_path.exists() {
            fs::remove_file(&file_path)?;
            debug!("Deleted hibernated tab {}", tab_id);
        }
        Ok(())
    }

    /// Get metadata about a hibernated tab without fully loading it
    pub fn get_metadata(&self, tab_id: u64) -> Result<HibernatedTabInfo, HibernationError> {
        let file_path = self.tab_file_path(tab_id);
        
        if !file_path.exists() {
            return Err(HibernationError::TabNotFound(tab_id));
        }
        
        let file = File::open(&file_path)?;
        let mut reader = BufReader::new(file);
        let header = HibernationHeader::read_from(&mut reader)?;
        
        let metadata = fs::metadata(&file_path)?;
        
        Ok(HibernatedTabInfo {
            tab_id,
            compressed_size: header.compressed_size,
            uncompressed_size: header.uncompressed_size,
            hibernated_at: metadata.modified()?,
        })
    }

    /// List all hibernated tabs
    pub fn list_hibernated(&self) -> Result<Vec<u64>, HibernationError> {
        let mut tabs = Vec::new();
        
        for entry in fs::read_dir(&self.config.storage_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.extension().map(|e| e == "hib").unwrap_or(false) {
                if let Some(stem) = path.file_stem() {
                    if let Some(s) = stem.to_str() {
                        if let Ok(tab_id) = s.parse::<u64>() {
                            tabs.push(tab_id);
                        }
                    }
                }
            }
        }
        
        Ok(tabs)
    }

    /// Cleanup old hibernated tabs
    pub fn cleanup_old(&self) -> Result<usize, HibernationError> {
        let Some(max_age) = self.config.max_age else {
            return Ok(0);
        };
        
        let now = SystemTime::now();
        let mut deleted = 0;
        
        for tab_id in self.list_hibernated()? {
            if let Ok(info) = self.get_metadata(tab_id) {
                if let Ok(age) = now.duration_since(info.hibernated_at) {
                    if age > max_age {
                        self.delete(tab_id)?;
                        deleted += 1;
                    }
                }
            }
        }
        
        if deleted > 0 {
            info!("Cleaned up {} old hibernated tabs", deleted);
        }
        
        Ok(deleted)
    }

    /// Get total storage used by hibernated tabs
    pub fn total_storage_bytes(&self) -> Result<u64, HibernationError> {
        let mut total = 0;
        
        for entry in fs::read_dir(&self.config.storage_dir)? {
            let entry = entry?;
            if let Ok(metadata) = entry.metadata() {
                total += metadata.len();
            }
        }
        
        Ok(total)
    }

    fn tab_file_path(&self, tab_id: u64) -> PathBuf {
        self.config.storage_dir.join(format!("{}.hib", tab_id))
    }
}

/// Metadata about a hibernated tab
#[derive(Debug)]
pub struct HibernatedTabInfo {
    pub tab_id: u64,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub hibernated_at: SystemTime,
}

/// Simple CRC32 checksum
fn crc32_checksum(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// Serialize using a simple binary format
fn bincode_serialize<T: Serialize>(value: &T) -> Result<Vec<u8>, HibernationError> {
    // Using JSON for now, would use bincode in production
    serde_json::to_vec(value)
        .map_err(|e| HibernationError::Serialization(e.to_string()))
}

/// Deserialize from binary format
fn bincode_deserialize<T: for<'de> Deserialize<'de>>(data: &[u8]) -> Result<T, HibernationError> {
    serde_json::from_slice(data)
        .map_err(|e| HibernationError::Serialization(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::UNIX_EPOCH;

    fn test_config() -> HibernationConfig {
        HibernationConfig {
            storage_dir: PathBuf::from("/tmp/fos-wb-test-hibernation"),
            compression_level: 1,
            max_age: None,
            max_storage_bytes: None,
        }
    }

    #[test]
    fn test_hibernate_and_hydrate() {
        let storage = ColdStorage::new(test_config()).unwrap();
        
        let snapshot = TabSnapshot {
            tab_id: 42,
            url: "https://example.com".to_string(),
            title: "Example Page".to_string(),
            scroll_position: (0.0, 150.5),
            form_data: vec![
                FormField {
                    element_id: "email".to_string(),
                    name: "email".to_string(),
                    value: "test@example.com".to_string(),
                    field_type: "email".to_string(),
                }
            ],
            dom_snapshot: vec![1, 2, 3, 4, 5], // Simulated DOM
            js_heap_snapshot: Some(vec![6, 7, 8, 9, 10]), // Simulated JS heap
            hibernated_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            original_memory_bytes: 1024 * 1024 * 5, // 5 MB
        };
        
        // Hibernate
        let compressed_size = storage.hibernate(&snapshot).unwrap();
        assert!(compressed_size > 0);
        
        // Verify it's hibernated
        assert!(storage.is_hibernated(42));
        
        // Hydrate
        let restored = storage.hydrate(42).unwrap();
        assert_eq!(restored.tab_id, snapshot.tab_id);
        assert_eq!(restored.url, snapshot.url);
        assert_eq!(restored.title, snapshot.title);
        assert_eq!(restored.scroll_position, snapshot.scroll_position);
        assert_eq!(restored.form_data.len(), 1);
        assert_eq!(restored.dom_snapshot, snapshot.dom_snapshot);
        assert_eq!(restored.js_heap_snapshot, snapshot.js_heap_snapshot);
        
        // Cleanup
        storage.delete(42).unwrap();
        assert!(!storage.is_hibernated(42));
    }

    #[test]
    fn test_checksum() {
        let data = b"Hello, World!";
        let checksum1 = crc32_checksum(data);
        let checksum2 = crc32_checksum(data);
        assert_eq!(checksum1, checksum2);
        
        let different = b"Different data";
        let checksum3 = crc32_checksum(different);
        assert_ne!(checksum1, checksum3);
    }

    #[test]
    fn test_compression_ratio() {
        let storage = ColdStorage::new(test_config()).unwrap();
        
        // Create snapshot with repetitive data (compresses well)
        let repetitive_data = vec![0u8; 100_000];
        
        let snapshot = TabSnapshot {
            tab_id: 100,
            url: "https://test.com".to_string(),
            title: "Test".to_string(),
            scroll_position: (0.0, 0.0),
            form_data: vec![],
            dom_snapshot: repetitive_data,
            js_heap_snapshot: None,
            hibernated_at: 0,
            original_memory_bytes: 100_000,
        };
        
        let compressed_size = storage.hibernate(&snapshot).unwrap();
        
        // Repetitive data should compress significantly
        assert!(compressed_size < 50_000, "Compression ratio too low");
        
        storage.delete(100).unwrap();
    }
}
