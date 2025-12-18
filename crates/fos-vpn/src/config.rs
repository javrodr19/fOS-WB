//! VPN Configuration
//!
//! Provides configuration structures for WireGuard peers and endpoints.

use crate::keys::{PrivateKey, PublicKey};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// Network endpoint (IP + port)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    /// IP address
    pub addr: IpAddr,
    /// UDP port
    pub port: u16,
}

impl Endpoint {
    /// Create a new endpoint
    pub fn new(addr: IpAddr, port: u16) -> Self {
        Self { addr, port }
    }

    /// Create from IPv4 address
    pub fn ipv4(a: u8, b: u8, c: u8, d: u8, port: u16) -> Self {
        Self {
            addr: IpAddr::V4(Ipv4Addr::new(a, b, c, d)),
            port,
        }
    }

    /// Convert to SocketAddr
    pub fn to_socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.addr, self.port)
    }

    /// **German VPN Exit Node (Placeholder)**
    ///
    /// This is a placeholder configuration for a German WireGuard server.
    /// Replace with actual VPN provider endpoint.
    pub fn german_exit() -> Self {
        // Example: Frankfurt, Germany datacenter
        // IP: 185.186.78.xxx (placeholder)
        // Port: 51820 (standard WireGuard)
        Self::ipv4(185, 186, 78, 1, 51820)
    }
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.addr, self.port)
    }
}

/// WireGuard peer configuration
#[derive(Debug, Clone)]
pub struct PeerConfig {
    /// Peer's public key
    pub public_key: PublicKey,
    /// Peer's endpoint
    pub endpoint: Endpoint,
    /// Allowed IPs (what traffic to route through this peer)
    pub allowed_ips: Vec<(IpAddr, u8)>,
    /// Persistent keepalive interval (seconds)
    pub keepalive: Option<u16>,
    /// Preshared key (optional, for post-quantum resistance)
    pub preshared_key: Option<[u8; 32]>,
}

impl PeerConfig {
    /// Create a new peer configuration
    pub fn new(public_key: PublicKey, endpoint: Endpoint) -> Self {
        Self {
            public_key,
            endpoint,
            allowed_ips: vec![
                (IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0), // 0.0.0.0/0 = all traffic
            ],
            keepalive: Some(25), // 25 seconds
            preshared_key: None,
        }
    }

    /// **German VPN Peer (Placeholder)**
    ///
    /// Creates a peer configuration for the German exit node.
    /// The public key is a placeholder - replace with actual VPN provider key.
    pub fn german_exit() -> Self {
        // Placeholder public key (replace with actual VPN provider key)
        // This is a valid base64-encoded 32-byte key for testing
        let public_key = PublicKey::from_base64(
            "WDvCRKv9hVAx1P3L7dKxiNxI3CxbK9Ue1tL8x2ZqRVk="
        ).expect("Hardcoded key should be valid");

        Self::new(public_key, Endpoint::german_exit())
    }
}

/// Complete VPN configuration
#[derive(Debug, Clone)]
pub struct VpnConfig {
    /// Our private key
    pub private_key: PrivateKey,
    /// Our internal VPN IP address
    pub address: IpAddr,
    /// DNS server to use inside the tunnel
    pub dns: Vec<IpAddr>,
    /// The peer we connect to
    pub peer: PeerConfig,
    /// Local SOCKS5 proxy port
    pub proxy_port: u16,
    /// Enable kill switch
    pub kill_switch: bool,
    /// Maximum memory usage (bytes)
    pub max_memory: usize,
}

impl VpnConfig {
    /// Create a new configuration
    pub fn new(private_key: PrivateKey, peer: PeerConfig) -> Self {
        Self {
            private_key,
            address: IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)),
            dns: vec![
                IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),      // Cloudflare
                IpAddr::V4(Ipv4Addr::new(9, 9, 9, 9)),      // Quad9
            ],
            peer,
            proxy_port: 1080, // Standard SOCKS5 port
            kill_switch: true,
            max_memory: 5 * 1024 * 1024, // 5 MB limit
        }
    }

    /// Create a configuration for connecting to German exit node
    pub fn german_exit() -> Self {
        let private_key = PrivateKey::generate();
        let peer = PeerConfig::german_exit();
        Self::new(private_key, peer)
    }

    /// Get the local proxy URL
    pub fn proxy_url(&self) -> String {
        format!("socks5://127.0.0.1:{}", self.proxy_port)
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.proxy_port == 0 {
            return Err(ConfigError::InvalidPort);
        }
        if self.max_memory < 1024 * 1024 {
            return Err(ConfigError::MemoryTooLow);
        }
        Ok(())
    }
}

impl Default for VpnConfig {
    fn default() -> Self {
        Self::german_exit()
    }
}

/// Configuration errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigError {
    #[error("Invalid proxy port")]
    InvalidPort,

    #[error("Memory limit too low (minimum 1MB)")]
    MemoryTooLow,

    #[error("Invalid peer configuration")]
    InvalidPeer,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint() {
        let ep = Endpoint::ipv4(192, 168, 1, 1, 51820);
        assert_eq!(ep.port, 51820);
    }

    #[test]
    fn test_german_exit() {
        let peer = PeerConfig::german_exit();
        assert_eq!(peer.endpoint.port, 51820);
        assert!(peer.keepalive.is_some());
    }

    #[test]
    fn test_config_default() {
        let config = VpnConfig::default();
        
        assert!(config.kill_switch);
        assert_eq!(config.proxy_port, 1080);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_proxy_url() {
        let config = VpnConfig::default();
        assert_eq!(config.proxy_url(), "socks5://127.0.0.1:1080");
    }
}
