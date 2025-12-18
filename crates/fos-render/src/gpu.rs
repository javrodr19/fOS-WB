//! GPU Context - wgpu initialization and management
//!
//! Provides a minimal GPU context that can render directly to
//! a window surface without intermediate compositing.

use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, info, warn};
use wgpu::{
    Adapter, Device, DeviceDescriptor, Features, Instance, InstanceDescriptor,
    Limits, PowerPreference, Queue, RequestAdapterOptions,
};

/// GPU context errors
#[derive(Debug, Error)]
pub enum GpuError {
    #[error("No suitable GPU adapter found")]
    NoAdapter,
    
    #[error("Failed to create device: {0}")]
    DeviceCreation(String),
    
    #[error("Surface error: {0}")]
    Surface(String),
}

/// GPU configuration
#[derive(Debug, Clone)]
pub struct GpuConfig {
    /// Prefer low-power GPU (integrated) over high-performance (discrete)
    pub low_power: bool,
    /// Maximum texture dimension
    pub max_texture_dimension: u32,
    /// Enable debug validation
    pub debug: bool,
}

impl Default for GpuConfig {
    fn default() -> Self {
        Self {
            low_power: true, // Prefer integrated GPU for lower power
            max_texture_dimension: 8192,
            debug: cfg!(debug_assertions),
        }
    }
}

/// GPU context holding wgpu device and queue
pub struct GpuContext {
    /// wgpu instance
    pub instance: Instance,
    /// Selected adapter
    pub adapter: Adapter,
    /// Logical device
    pub device: Arc<Device>,
    /// Command queue
    pub queue: Arc<Queue>,
    /// Configuration
    pub config: GpuConfig,
}

impl GpuContext {
    /// Create a new GPU context
    pub async fn new(config: GpuConfig) -> Result<Self, GpuError> {
        info!("Initializing GPU context (low_power: {})", config.low_power);

        // Create instance with preferred backends
        let instance = Instance::new(&InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // Request adapter
        let power_preference = if config.low_power {
            PowerPreference::LowPower
        } else {
            PowerPreference::HighPerformance
        };

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference,
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or(GpuError::NoAdapter)?;

        let adapter_info = adapter.get_info();
        info!(
            "GPU adapter: {} ({:?})",
            adapter_info.name, adapter_info.backend
        );
        debug!(
            "GPU driver: {} (vendor: {})",
            adapter_info.driver, adapter_info.vendor
        );

        // Request device with minimal features
        let limits = Limits {
            max_texture_dimension_2d: config.max_texture_dimension,
            ..Limits::downlevel_webgl2_defaults()
        };

        let (device, queue) = adapter
            .request_device(
                &DeviceDescriptor {
                    label: Some("fOS-WB GPU Device"),
                    required_features: Features::empty(),
                    required_limits: limits,
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
                },
                None,
            )
            .await
            .map_err(|e| GpuError::DeviceCreation(e.to_string()))?;

        // Set up error handling
        device.on_uncaptured_error(Box::new(|error| {
            warn!("wgpu error: {}", error);
        }));

        info!("GPU context initialized successfully");

        Ok(Self {
            instance,
            adapter,
            device: Arc::new(device),
            queue: Arc::new(queue),
            config,
        })
    }

    /// Create with default configuration
    pub async fn with_defaults() -> Result<Self, GpuError> {
        Self::new(GpuConfig::default()).await
    }

    /// Get device memory info (if available)
    pub fn memory_info(&self) -> Option<(u64, u64)> {
        // Note: wgpu doesn't expose memory info directly
        // This would require backend-specific queries
        None
    }

    /// Check if GPU supports required features
    pub fn check_features(&self) -> bool {
        // We use minimal features, so this is always true
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: GPU tests require actual hardware, skip in CI
    #[test]
    #[ignore = "requires GPU"]
    fn test_gpu_context_creation() {
        let ctx = pollster::block_on(GpuContext::with_defaults());
        assert!(ctx.is_ok());
    }
}
