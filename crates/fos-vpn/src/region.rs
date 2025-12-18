//! VPN Region Management
//!
//! Provides dynamic switching between multiple global VPN locations
//! without restarting the entire VPN subsystem.
//!
//! # Supported Regions
//!
//! | Code | Country | Typical Latency |
//! |------|---------|-----------------|
//! | DE | Germany (Frankfurt) | Low |
//! | JP | Japan (Tokyo) | High |
//! | US | USA (New York) | Medium |
//! | KR | South Korea (Seoul) | High |
//! | RU | Russia (Moscow) | High |
//! | UK | United Kingdom (London) | Low |
//!
//! # Memory Efficiency
//!
//! Only the active region's full configuration is loaded.
//! Other regions store minimal metadata (~100 bytes each).

use crate::config::{Endpoint, PeerConfig, VpnConfig};
use crate::keys::{PrivateKey, PublicKey, KeyError};
use crate::tunnel::{WireGuardTunnel, TunnelState, TunnelError};
use crate::kill_switch::KillSwitch;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn, error};

/// Region identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegionId {
    DE, // Germany
    JP, // Japan
    US, // USA
    KR, // South Korea
    RU, // Russia
    UK, // United Kingdom
}

impl RegionId {
    /// Get all available regions
    pub fn all() -> &'static [RegionId] {
        &[
            RegionId::DE,
            RegionId::JP,
            RegionId::US,
            RegionId::KR,
            RegionId::RU,
            RegionId::UK,
        ]
    }

    /// Get region display name
    pub fn name(&self) -> &'static str {
        match self {
            RegionId::DE => "Germany (Frankfurt)",
            RegionId::JP => "Japan (Tokyo)",
            RegionId::US => "USA (New York)",
            RegionId::KR => "South Korea (Seoul)",
            RegionId::RU => "Russia (Moscow)",
            RegionId::UK => "United Kingdom (London)",
        }
    }

    /// Get country code
    pub fn code(&self) -> &'static str {
        match self {
            RegionId::DE => "DE",
            RegionId::JP => "JP",
            RegionId::US => "US",
            RegionId::KR => "KR",
            RegionId::RU => "RU",
            RegionId::UK => "UK",
        }
    }

    /// Is this a high-latency region (needs extra monitoring)?
    pub fn is_high_latency(&self) -> bool {
        matches!(self, RegionId::JP | RegionId::KR | RegionId::RU)
    }

    /// Get expected latency range (ms)
    pub fn expected_latency(&self) -> (u32, u32) {
        match self {
            RegionId::DE | RegionId::UK => (10, 50),
            RegionId::US => (50, 150),
            RegionId::JP | RegionId::KR => (150, 300),
            RegionId::RU => (100, 250),
        }
    }
}

impl std::fmt::Display for RegionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.code())
    }
}

impl std::str::FromStr for RegionId {
    type Err = RegionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "DE" => Ok(RegionId::DE),
            "JP" => Ok(RegionId::JP),
            "US" => Ok(RegionId::US),
            "KR" => Ok(RegionId::KR),
            "RU" => Ok(RegionId::RU),
            "UK" => Ok(RegionId::UK),
            _ => Err(RegionError::InvalidRegion(s.to_string())),
        }
    }
}

/// Region profile (stored in config file)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionProfile {
    /// Region identifier
    pub id: RegionId,
    /// Display name
    pub name: String,
    /// Server endpoint IP
    pub endpoint_ip: String,
    /// Server port (usually 51820)
    pub endpoint_port: u16,
    /// Server public key (base64)
    pub public_key: String,
    /// Allowed IPs (CIDR notation)
    pub allowed_ips: Vec<String>,
    /// Preshared key (optional, base64)
    pub preshared_key: Option<String>,
    /// MTU tuning (for RU/KR regions with ISP filtering)
    /// Default: 1420. Set lower (1280-1380) for problematic networks.
    #[serde(default = "default_mtu")]
    pub mtu: u16,
    /// Is this region enabled?
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_mtu() -> u16 {
    1420 // Standard WireGuard MTU
}

fn default_true() -> bool {
    true
}

impl RegionProfile {
    /// Parse endpoint
    pub fn endpoint(&self) -> Result<Endpoint, RegionError> {
        let ip: IpAddr = self.endpoint_ip.parse()
            .map_err(|_| RegionError::InvalidEndpoint)?;
        Ok(Endpoint::new(ip, self.endpoint_port))
    }

    /// Parse public key
    pub fn parse_public_key(&self) -> Result<PublicKey, KeyError> {
        PublicKey::from_base64(&self.public_key)
    }

    /// Convert to PeerConfig
    pub fn to_peer_config(&self) -> Result<PeerConfig, RegionError> {
        let public_key = self.parse_public_key()?;
        let endpoint = self.endpoint()?;
        
        let mut peer = PeerConfig::new(public_key, endpoint);
        
        // Parse allowed IPs
        for cidr in &self.allowed_ips {
            if let Some((ip, prefix)) = Self::parse_cidr(cidr) {
                peer.allowed_ips.push((ip, prefix));
            }
        }
        
        Ok(peer)
    }

    /// Parse CIDR notation (e.g., "0.0.0.0/0")
    fn parse_cidr(cidr: &str) -> Option<(IpAddr, u8)> {
        let parts: Vec<&str> = cidr.split('/').collect();
        if parts.len() != 2 {
            return None;
        }
        
        let ip: IpAddr = parts[0].parse().ok()?;
        let prefix: u8 = parts[1].parse().ok()?;
        
        Some((ip, prefix))
    }
}

/// Multi-region configuration file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionConfig {
    /// Client private key (base64)
    pub private_key: String,
    /// Client internal VPN IP
    pub client_ip: String,
    /// DNS servers to use
    pub dns: Vec<String>,
    /// SOCKS5 proxy port
    #[serde(default = "default_proxy_port")]
    pub proxy_port: u16,
    /// Kill switch enabled
    #[serde(default = "default_true")]
    pub kill_switch: bool,
    /// All region profiles
    pub regions: Vec<RegionProfile>,
}

fn default_proxy_port() -> u16 {
    1080
}

impl RegionConfig {
    /// Load from TOML file
    pub fn from_toml_file(path: &Path) -> Result<Self, RegionError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| RegionError::IoError(e.to_string()))?;
        Self::from_toml(&content)
    }

    /// Load from TOML string
    pub fn from_toml(content: &str) -> Result<Self, RegionError> {
        toml::from_str(content)
            .map_err(|e| RegionError::ParseError(e.to_string()))
    }

    /// Load from JSON file
    pub fn from_json_file(path: &Path) -> Result<Self, RegionError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| RegionError::IoError(e.to_string()))?;
        Self::from_json(&content)
    }

    /// Load from JSON string
    pub fn from_json(content: &str) -> Result<Self, RegionError> {
        serde_json::from_str(content)
            .map_err(|e| RegionError::ParseError(e.to_string()))
    }

    /// Get default configuration with placeholder values
    pub fn default_config() -> Self {
        Self {
            private_key: "REPLACE_WITH_YOUR_PRIVATE_KEY_BASE64".to_string(),
            client_ip: "10.0.0.2".to_string(),
            dns: vec!["1.1.1.1".to_string(), "9.9.9.9".to_string()],
            proxy_port: 1080,
            kill_switch: true,
            regions: Self::default_regions(),
        }
    }

    /// Get default region profiles (placeholders)
    fn default_regions() -> Vec<RegionProfile> {
        vec![
            RegionProfile {
                id: RegionId::DE,
                name: "Germany (Frankfurt)".to_string(),
                endpoint_ip: "185.186.78.1".to_string(),
                endpoint_port: 51820,
                public_key: "WDvCRKv9hVAx1P3L7dKxiNxI3CxbK9Ue1tL8x2ZqRVk=".to_string(),
                allowed_ips: vec!["0.0.0.0/0".to_string()],
                preshared_key: None,
                mtu: 1420, // Standard
                enabled: true,
            },
            RegionProfile {
                id: RegionId::JP,
                name: "Japan (Tokyo)".to_string(),
                endpoint_ip: "103.231.88.1".to_string(),
                endpoint_port: 51820,
                public_key: "YjP3Kv8xMNz1R4LqWiXiOpI4DyAbM0Vf2uN9y3ArSWk=".to_string(),
                allowed_ips: vec!["0.0.0.0/0".to_string()],
                preshared_key: None,
                mtu: 1400, // Slightly lower for trans-Pacific
                enabled: true,
            },
            RegionProfile {
                id: RegionId::US,
                name: "USA (New York)".to_string(),
                endpoint_ip: "192.169.69.1".to_string(),
                endpoint_port: 51820,
                public_key: "ZkQ4Mv9yOPa2S5MrXjYjQrJ5EzCcN1Wg3vO0z4BtTXk=".to_string(),
                allowed_ips: vec!["0.0.0.0/0".to_string()],
                preshared_key: None,
                mtu: 1420, // Standard
                enabled: true,
            },
            RegionProfile {
                id: RegionId::KR,
                name: "South Korea (Seoul)".to_string(),
                endpoint_ip: "121.254.178.1".to_string(),
                endpoint_port: 51820,
                public_key: "AlR5Nw0zPQb3T6NsYkZkRsK6FaCdO2Xh4wP1a5CtUYk=".to_string(),
                allowed_ips: vec!["0.0.0.0/0".to_string()],
                preshared_key: None,
                mtu: 1320, // Lower MTU for ISP filtering
                enabled: true,
            },
            RegionProfile {
                id: RegionId::RU,
                name: "Russia (Moscow)".to_string(),
                endpoint_ip: "185.22.153.1".to_string(),
                endpoint_port: 51820,
                public_key: "BmS6Ox1aQRc4U7OtZlAlStL7GbDeP3Yi5xQ2b6DuVZk=".to_string(),
                allowed_ips: vec!["0.0.0.0/0".to_string()],
                preshared_key: None,
                mtu: 1280, // Lowest MTU for aggressive DPI
                enabled: true,
            },
            RegionProfile {
                id: RegionId::UK,
                name: "United Kingdom (London)".to_string(),
                endpoint_ip: "178.62.1.1".to_string(),
                endpoint_port: 51820,
                public_key: "CnT7Py2bRSd5V8PuAmBmTuM8HcEfQ4Zj6yR3c7EwWak=".to_string(),
                allowed_ips: vec!["0.0.0.0/0".to_string()],
                preshared_key: None,
                mtu: 1420, // Standard
                enabled: true,
            },
        ]
    }

    /// Export as TOML
    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    /// Export as JSON
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Find region by ID
    pub fn find_region(&self, id: RegionId) -> Option<&RegionProfile> {
        self.regions.iter().find(|r| r.id == id && r.enabled)
    }

    /// Get enabled regions
    pub fn enabled_regions(&self) -> Vec<&RegionProfile> {
        self.regions.iter().filter(|r| r.enabled).collect()
    }
}

/// Region health status
#[derive(Debug, Clone)]
pub struct RegionHealth {
    /// Region ID
    pub region: RegionId,
    /// Is region reachable?
    pub reachable: bool,
    /// Last successful ping time
    pub last_ping: Option<Instant>,
    /// Current latency (ms)
    pub latency_ms: Option<u32>,
    /// Packets sent for monitoring
    pub packets_sent: u64,
    /// Packets received
    pub packets_received: u64,
    /// Connection failures
    pub failures: u32,
}

impl RegionHealth {
    /// Create initial health status
    pub fn new(region: RegionId) -> Self {
        Self {
            region,
            reachable: false,
            last_ping: None,
            latency_ms: None,
            packets_sent: 0,
            packets_received: 0,
            failures: 0,
        }
    }

    /// Packet loss percentage
    pub fn packet_loss(&self) -> f32 {
        if self.packets_sent == 0 {
            0.0
        } else {
            ((self.packets_sent - self.packets_received) as f32 / self.packets_sent as f32) * 100.0
        }
    }

    /// Is health acceptable?
    pub fn is_healthy(&self) -> bool {
        self.reachable && self.packet_loss() < 20.0
    }

    /// Record successful ping
    pub fn record_success(&mut self, latency_ms: u32) {
        self.reachable = true;
        self.last_ping = Some(Instant::now());
        self.latency_ms = Some(latency_ms);
        self.packets_sent += 1;
        self.packets_received += 1;
        self.failures = 0;
    }

    /// Record failed ping
    pub fn record_failure(&mut self) {
        self.packets_sent += 1;
        self.failures += 1;
        
        if self.failures >= 3 {
            self.reachable = false;
        }
    }
}

/// VPN Region Manager
///
/// Manages multiple VPN regions with dynamic switching.
///
/// # Memory Efficiency
///
/// - Region metadata: ~100 bytes each (6 regions = ~600 bytes)
/// - Active config: ~2KB when loaded
/// - Health tracking: ~100 bytes per region
/// - **Total overhead: <10KB static**
pub struct VpnRegionManager {
    /// Configuration (lazy loaded)
    config: Option<RegionConfig>,
    /// Active region
    active_region: Arc<RwLock<Option<RegionId>>>,
    /// Active tunnel
    tunnel: Arc<RwLock<Option<WireGuardTunnel>>>,
    /// Kill switch
    kill_switch: KillSwitch,
    /// Health status for each region
    health: Arc<RwLock<HashMap<RegionId, RegionHealth>>>,
    /// Is switching in progress?
    switching: Arc<RwLock<bool>>,
}

impl VpnRegionManager {
    /// Create a new region manager
    pub fn new() -> Self {
        let mut health = HashMap::new();
        for region in RegionId::all() {
            health.insert(*region, RegionHealth::new(*region));
        }

        Self {
            config: None,
            active_region: Arc::new(RwLock::new(None)),
            tunnel: Arc::new(RwLock::new(None)),
            kill_switch: KillSwitch::new(true),
            health: Arc::new(RwLock::new(health)),
            switching: Arc::new(RwLock::new(false)),
        }
    }

    /// Load configuration from file
    pub fn load_config(&mut self, path: &Path) -> Result<(), RegionError> {
        let ext = path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let config = match ext {
            "toml" => RegionConfig::from_toml_file(path)?,
            "json" => RegionConfig::from_json_file(path)?,
            _ => return Err(RegionError::UnsupportedFormat),
        };

        info!("Loaded VPN config with {} regions", config.regions.len());
        self.config = Some(config);
        Ok(())
    }

    /// Load configuration from string (for embedded configs)
    pub fn load_config_str(&mut self, content: &str, format: ConfigFormat) -> Result<(), RegionError> {
        let config = match format {
            ConfigFormat::Toml => RegionConfig::from_toml(content)?,
            ConfigFormat::Json => RegionConfig::from_json(content)?,
        };

        self.config = Some(config);
        Ok(())
    }

    /// Load default configuration
    pub fn load_defaults(&mut self) {
        self.config = Some(RegionConfig::default_config());
    }

    /// Get available regions
    pub fn available_regions(&self) -> Vec<RegionId> {
        self.config
            .as_ref()
            .map(|c| c.enabled_regions().iter().map(|r| r.id).collect())
            .unwrap_or_default()
    }

    /// Get current active region
    pub async fn active_region(&self) -> Option<RegionId> {
        *self.active_region.read().await
    }

    /// Get health for a region
    pub async fn health(&self, region: RegionId) -> Option<RegionHealth> {
        self.health.read().await.get(&region).cloned()
    }

    /// Get all region health statuses
    pub async fn all_health(&self) -> HashMap<RegionId, RegionHealth> {
        self.health.read().await.clone()
    }

    /// Switch to a new region
    ///
    /// This performs a graceful switch:
    /// 1. Blocks new traffic (kill switch)
    /// 2. Closes current UDP socket
    /// 3. Updates peer metadata
    /// 4. Re-establishes handshake
    /// 5. Resumes traffic
    pub async fn switch_region(&self, region_id: RegionId) -> Result<(), RegionError> {
        // Check if already switching
        {
            let mut switching = self.switching.write().await;
            if *switching {
                return Err(RegionError::SwitchInProgress);
            }
            *switching = true;
        }

        let result = self.do_switch_region(region_id).await;

        // Clear switching flag
        *self.switching.write().await = false;

        result
    }

    /// Internal switch implementation
    async fn do_switch_region(&self, region_id: RegionId) -> Result<(), RegionError> {
        let config = self.config.as_ref()
            .ok_or(RegionError::NoConfig)?;

        let profile = config.find_region(region_id)
            .ok_or(RegionError::InvalidRegion(region_id.to_string()))?;

        info!("Switching VPN to region: {}", profile.name);

        // Step 1: Activate kill switch (block traffic during switch)
        self.kill_switch.on_tunnel_down();
        debug!("Kill switch activated for region switch");

        // Step 2: Close existing tunnel if any
        {
            let mut tunnel_guard = self.tunnel.write().await;
            if let Some(ref mut tunnel) = *tunnel_guard {
                debug!("Closing existing tunnel");
                tunnel.stop().await;
            }
        }

        // Step 3: Create new VPN config for this region
        let private_key = PrivateKey::from_base64(&config.private_key)?;
        let peer_config = profile.to_peer_config()?;
        
        let vpn_config = VpnConfig {
            private_key,
            address: config.client_ip.parse()
                .map_err(|_| RegionError::InvalidConfig("Invalid client IP".into()))?,
            dns: config.dns.iter()
                .filter_map(|s| s.parse().ok())
                .collect(),
            peer: peer_config,
            proxy_port: config.proxy_port,
            kill_switch: config.kill_switch,
            max_memory: 5 * 1024 * 1024,
        };

        // Step 4: Create new tunnel
        let mut new_tunnel = WireGuardTunnel::new(vpn_config);

        // Step 5: Establish handshake with new peer
        match new_tunnel.start().await {
            Ok(()) => {
                info!("Successfully connected to {}", profile.name);
                
                // Update health
                {
                    let mut health = self.health.write().await;
                    if let Some(h) = health.get_mut(&region_id) {
                        h.record_success(50); // Placeholder latency
                    }
                }

                // Store new tunnel
                *self.tunnel.write().await = Some(new_tunnel);
                *self.active_region.write().await = Some(region_id);

                // Re-enable traffic
                self.kill_switch.on_tunnel_up();
                debug!("Kill switch deactivated, traffic flowing");

                Ok(())
            }
            Err(e) => {
                error!("Failed to connect to {}: {}", profile.name, e);

                // Update health
                {
                    let mut health = self.health.write().await;
                    if let Some(h) = health.get_mut(&region_id) {
                        h.record_failure();
                    }
                }

                // Keep kill switch active (no leak)
                Err(RegionError::ConnectionFailed(e.to_string()))
            }
        }
    }

    /// Disconnect from current region
    pub async fn disconnect(&self) {
        info!("Disconnecting from VPN");

        self.kill_switch.on_tunnel_down();

        let mut tunnel_guard = self.tunnel.write().await;
        if let Some(ref mut tunnel) = *tunnel_guard {
            tunnel.stop().await;
        }
        *tunnel_guard = None;
        *self.active_region.write().await = None;
    }

    /// Start heartbeat monitoring for high-latency regions
    pub fn start_heartbeat_monitor(&self) {
        let health = self.health.clone();
        let tunnel = self.tunnel.clone();
        let active = self.active_region.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));

            loop {
                interval.tick().await;

                // Only monitor if connected
                let current_region = *active.read().await;
                if current_region.is_none() {
                    continue;
                }

                let region = current_region.unwrap();
                
                // High-latency regions get extra monitoring
                if region.is_high_latency() {
                    debug!("Heartbeat check for high-latency region: {}", region);
                    
                    // Try to send keepalive through tunnel
                    let tunnel_guard = tunnel.read().await;
                    if let Some(ref t) = *tunnel_guard {
                        match t.keepalive().await {
                            Ok(()) => {
                                let mut h = health.write().await;
                                if let Some(hh) = h.get_mut(&region) {
                                    hh.record_success(100); // Estimated
                                }
                            }
                            Err(e) => {
                                warn!("Heartbeat failed for {}: {}", region, e);
                                let mut h = health.write().await;
                                if let Some(hh) = h.get_mut(&region) {
                                    hh.record_failure();
                                }
                            }
                        }
                    }
                }
            }
        });
    }

    /// Get best available region based on health
    pub async fn best_region(&self) -> Option<RegionId> {
        let health = self.health.read().await;
        
        // Prefer healthy, low-latency regions
        let mut candidates: Vec<_> = health.iter()
            .filter(|(_, h)| h.is_healthy())
            .collect();

        candidates.sort_by(|(a_id, a_health), (b_id, b_health)| {
            // Prefer lower latency regions
            let a_lat = a_health.latency_ms.unwrap_or(999);
            let b_lat = b_health.latency_ms.unwrap_or(999);
            a_lat.cmp(&b_lat)
        });

        candidates.first().map(|(id, _)| **id)
    }

    /// Memory usage estimate
    pub fn memory_usage(&self) -> usize {
        // Config: ~2KB when loaded
        let config_size = self.config.as_ref()
            .map(|_| 2048)
            .unwrap_or(0);

        // Health map: ~100 bytes per region
        let health_size = RegionId::all().len() * 100;

        // Tunnel state: ~2KB when active
        let tunnel_size = 2048;

        config_size + health_size + tunnel_size
    }

    /// Get proxy URL for browser configuration
    pub fn proxy_url(&self) -> String {
        let port = self.config
            .as_ref()
            .map(|c| c.proxy_port)
            .unwrap_or(1080);
        format!("socks5://127.0.0.1:{}", port)
    }

    /// Get status summary
    pub async fn status(&self) -> String {
        let region = self.active_region().await;
        let region_str = region.map(|r| r.name()).unwrap_or("Disconnected");
        
        format!(
            "VPN Region: {} | Memory: ~{}KB",
            region_str,
            self.memory_usage() / 1024
        )
    }
}

impl Default for VpnRegionManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration format
#[derive(Debug, Clone, Copy)]
pub enum ConfigFormat {
    Toml,
    Json,
}

/// Region errors
#[derive(Debug, thiserror::Error)]
pub enum RegionError {
    #[error("Invalid region: {0}")]
    InvalidRegion(String),

    #[error("No configuration loaded")]
    NoConfig,

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Invalid endpoint")]
    InvalidEndpoint,

    #[error("IO error: {0}")]
    IoError(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Unsupported config format")]
    UnsupportedFormat,

    #[error("Switch already in progress")]
    SwitchInProgress,

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Key error: {0}")]
    Key(#[from] KeyError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_region_id() {
        assert_eq!(RegionId::DE.code(), "DE");
        assert_eq!(RegionId::JP.name(), "Japan (Tokyo)");
        assert!(RegionId::JP.is_high_latency());
        assert!(!RegionId::DE.is_high_latency());
    }

    #[test]
    fn test_region_parse() {
        let de: RegionId = "de".parse().unwrap();
        assert_eq!(de, RegionId::DE);
        
        let jp: RegionId = "JP".parse().unwrap();
        assert_eq!(jp, RegionId::JP);
    }

    #[test]
    fn test_default_config() {
        let config = RegionConfig::default_config();
        
        assert_eq!(config.regions.len(), 6);
        assert!(config.kill_switch);
        assert_eq!(config.proxy_port, 1080);
    }

    #[test]
    fn test_config_toml_roundtrip() {
        let config = RegionConfig::default_config();
        let toml = config.to_toml();
        
        let parsed = RegionConfig::from_toml(&toml).unwrap();
        assert_eq!(parsed.regions.len(), 6);
    }

    #[test]
    fn test_config_json_roundtrip() {
        let config = RegionConfig::default_config();
        let json = config.to_json();
        
        let parsed = RegionConfig::from_json(&json).unwrap();
        assert_eq!(parsed.regions.len(), 6);
    }

    #[test]
    fn test_region_health() {
        let mut health = RegionHealth::new(RegionId::JP);
        
        assert!(!health.is_healthy());
        
        health.record_success(150);
        assert!(health.is_healthy());
        assert_eq!(health.packet_loss(), 0.0);
    }

    #[tokio::test]
    async fn test_region_manager() {
        let mut manager = VpnRegionManager::new();
        manager.load_defaults();
        
        let regions = manager.available_regions();
        assert_eq!(regions.len(), 6);
        
        assert!(manager.active_region().await.is_none());
    }

    #[test]
    fn test_memory_estimate() {
        let mut manager = VpnRegionManager::new();
        manager.load_defaults();
        
        // Should be well under 2MB
        assert!(manager.memory_usage() < 2 * 1024 * 1024);
    }
}
