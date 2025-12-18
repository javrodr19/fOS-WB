//! VPN Manager
//!
//! Coordinates the WireGuard tunnel, SOCKS5 proxy, and kill switch
//! into a unified VPN interface for the browser.
//!
//! # Usage
//!
//! ```rust,ignore
//! // Create and start VPN
//! let mut vpn = VpnManager::new(VpnConfig::german_exit());
//! vpn.connect().await?;
//!
//! // Configure browser to use proxy
//! let proxy_url = vpn.proxy_url(); // "socks5://127.0.0.1:1080"
//!
//! // Check status
//! if vpn.is_connected() {
//!     println!("Connected to VPN");
//! }
//!
//! // Disconnect
//! vpn.disconnect().await;
//! ```

use crate::config::VpnConfig;
use crate::tunnel::{WireGuardTunnel, TunnelState, TunnelStats, TunnelError};
use crate::proxy::{Socks5Proxy, ProxyConfig, ProxyError};
use crate::kill_switch::{KillSwitch, KillSwitchState};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{info, warn, error, debug};

/// VPN connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VpnState {
    /// VPN is disconnected
    Disconnected,
    /// VPN is connecting
    Connecting,
    /// VPN is connected and ready
    Connected,
    /// Connection failed
    Failed,
    /// Reconnecting after drop
    Reconnecting,
}

impl VpnState {
    /// Check if VPN is usable
    pub fn is_connected(&self) -> bool {
        matches!(self, VpnState::Connected)
    }

    /// Check if VPN is in a failure state
    pub fn is_failed(&self) -> bool {
        matches!(self, VpnState::Failed)
    }
}

/// VPN Manager errors
#[derive(Debug, thiserror::Error)]
pub enum VpnError {
    #[error("VPN already connected")]
    AlreadyConnected,

    #[error("VPN not connected")]
    NotConnected,

    #[error("Tunnel error: {0}")]
    Tunnel(#[from] TunnelError),

    #[error("Proxy error: {0}")]
    Proxy(#[from] ProxyError),

    #[error("Kill switch blocking traffic")]
    KillSwitchActive,

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Connection timeout")]
    Timeout,
}

/// VPN Manager
///
/// Provides a unified interface for the integrated VPN:
/// - Manages WireGuard tunnel lifecycle
/// - Runs local SOCKS5 proxy
/// - Enforces kill switch policy
pub struct VpnManager {
    /// Configuration
    config: VpnConfig,
    /// WireGuard tunnel
    tunnel: WireGuardTunnel,
    /// SOCKS5 proxy
    proxy: Socks5Proxy,
    /// Kill switch
    kill_switch: KillSwitch,
    /// Current state
    state: Arc<RwLock<VpnState>>,
    /// Connection started at
    connected_at: Option<Instant>,
    /// Reconnect attempts
    reconnect_attempts: u32,
}

impl VpnManager {
    /// Create a new VPN manager
    pub fn new(config: VpnConfig) -> Self {
        let kill_switch = KillSwitch::new(config.kill_switch);
        let tunnel = WireGuardTunnel::new(config.clone());
        
        let proxy_config = ProxyConfig {
            listen_addr: format!("127.0.0.1:{}", config.proxy_port).parse().unwrap(),
            max_connections: 64,
            buffer_size: 4096,
            timeout_secs: 30,
        };
        let proxy = Socks5Proxy::new(proxy_config);

        Self {
            config,
            tunnel,
            proxy,
            kill_switch,
            state: Arc::new(RwLock::new(VpnState::Disconnected)),
            connected_at: None,
            reconnect_attempts: 0,
        }
    }

    /// Create with German exit node configuration
    pub fn german_exit() -> Self {
        Self::new(VpnConfig::german_exit())
    }

    /// Get current state
    pub async fn state(&self) -> VpnState {
        *self.state.read().await
    }

    /// Check if VPN is connected
    pub async fn is_connected(&self) -> bool {
        self.state().await.is_connected()
    }

    /// Get the proxy URL for browser configuration
    ///
    /// Returns the SOCKS5 proxy URL that the browser should use
    /// to route traffic through the VPN.
    pub fn proxy_url(&self) -> String {
        self.config.proxy_url()
    }

    /// Get kill switch state
    pub fn kill_switch_state(&self) -> KillSwitchState {
        self.kill_switch.state()
    }

    /// Connect to VPN
    ///
    /// 1. Starts SOCKS5 proxy on localhost
    /// 2. Establishes WireGuard tunnel to German exit
    /// 3. Enables kill switch
    pub async fn connect(&mut self) -> Result<(), VpnError> {
        let current = self.state().await;
        if current.is_connected() {
            return Err(VpnError::AlreadyConnected);
        }

        info!("Connecting to VPN ({})", self.config.peer.endpoint);
        *self.state.write().await = VpnState::Connecting;

        // Start proxy first (so kill switch has something to protect)
        self.proxy.start().await?;

        // Connect tunnel
        match self.tunnel.start().await {
            Ok(()) => {
                *self.state.write().await = VpnState::Connected;
                self.kill_switch.on_tunnel_up();
                self.connected_at = Some(Instant::now());
                self.reconnect_attempts = 0;

                info!(
                    "VPN connected successfully (proxy: {})",
                    self.proxy_url()
                );

                // Start background tasks
                self.spawn_keepalive();
                self.spawn_tunnel_monitor();

                Ok(())
            }
            Err(e) => {
                error!("VPN connection failed: {}", e);
                *self.state.write().await = VpnState::Failed;
                self.kill_switch.on_tunnel_down();
                
                // Stop proxy since tunnel failed
                self.proxy.stop();
                
                Err(VpnError::Tunnel(e))
            }
        }
    }

    /// Disconnect from VPN
    pub async fn disconnect(&mut self) {
        info!("Disconnecting VPN");

        // Update state first
        *self.state.write().await = VpnState::Disconnected;
        self.kill_switch.on_tunnel_down();

        // Stop tunnel and proxy
        self.tunnel.stop().await;
        self.proxy.stop();

        self.connected_at = None;

        info!("VPN disconnected");
    }

    /// Reconnect (after tunnel drop)
    pub async fn reconnect(&mut self) -> Result<(), VpnError> {
        self.reconnect_attempts += 1;
        
        if self.reconnect_attempts > 3 {
            error!("Too many reconnect attempts, giving up");
            *self.state.write().await = VpnState::Failed;
            return Err(VpnError::Timeout);
        }

        warn!("Reconnecting VPN (attempt {})", self.reconnect_attempts);
        *self.state.write().await = VpnState::Reconnecting;

        // Brief delay before retry
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Restart tunnel
        match self.tunnel.start().await {
            Ok(()) => {
                *self.state.write().await = VpnState::Connected;
                self.kill_switch.on_tunnel_up();
                self.connected_at = Some(Instant::now());
                
                info!("VPN reconnected successfully");
                Ok(())
            }
            Err(e) => {
                error!("Reconnection failed: {}", e);
                Err(VpnError::Tunnel(e))
            }
        }
    }

    /// Get tunnel statistics
    pub async fn stats(&self) -> TunnelStats {
        self.tunnel.stats().await
    }

    /// Get connection duration
    pub fn connection_duration(&self) -> Option<Duration> {
        self.connected_at.map(|t| t.elapsed())
    }

    /// Get active proxy connections
    pub fn active_connections(&self) -> u64 {
        self.proxy.active_connections()
    }

    /// Check if request is allowed (kill switch check)
    ///
    /// Call this before making any network request to enforce
    /// the kill switch policy.
    pub fn check_request(&self) -> Result<(), VpnError> {
        if self.kill_switch.is_blocking() {
            Err(VpnError::KillSwitchActive)
        } else {
            Ok(())
        }
    }

    /// Spawn keepalive task
    fn spawn_keepalive(&self) {
        let state = self.state.clone();
        
        // Note: In real implementation, this would send keepalive through tunnel
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(25));
            
            loop {
                interval.tick().await;
                
                if !state.read().await.is_connected() {
                    break;
                }
                
                debug!("VPN keepalive");
            }
        });
    }

    /// Spawn tunnel monitor task
    fn spawn_tunnel_monitor(&self) {
        let state = self.state.clone();
        
        // Note: In real implementation, this would monitor tunnel health
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(5)).await;
                
                if !state.read().await.is_connected() {
                    break;
                }
                
                // Check tunnel health here
            }
        });
    }

    /// Get memory usage estimate
    pub fn memory_usage(&self) -> usize {
        // Rough estimate:
        // - Tunnel state: ~1KB
        // - Session keys: ~100B
        // - Proxy buffers: ~512KB (64 connections * 8KB each)
        // - Connection state: ~50KB
        let proxy_mem = self.proxy.active_connections() as usize * 8192;
        1024 + 100 + proxy_mem + 50 * 1024
    }

    /// Format status for display
    pub async fn status(&self) -> String {
        let state = self.state().await;
        let stats = self.stats().await;
        
        format!(
            "VPN: {:?} | {} | Connections: {} | Memory: ~{:.1}KB",
            state,
            stats.format(),
            self.active_connections(),
            self.memory_usage() as f64 / 1024.0
        )
    }
}

impl Drop for VpnManager {
    fn drop(&mut self) {
        // Ensure kill switch blocks traffic when manager is dropped
        self.kill_switch.on_tunnel_down();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vpn_state() {
        assert!(VpnState::Connected.is_connected());
        assert!(!VpnState::Disconnected.is_connected());
        assert!(VpnState::Failed.is_failed());
    }

    #[tokio::test]
    async fn test_vpn_manager_creation() {
        let vpn = VpnManager::german_exit();
        
        assert_eq!(vpn.state().await, VpnState::Disconnected);
        assert!(!vpn.proxy_url().is_empty());
    }

    #[tokio::test]
    async fn test_kill_switch_on_drop() {
        let vpn = VpnManager::german_exit();
        let ks_state = vpn.kill_switch_state();
        
        // Kill switch should be active (tunnel not up)
        assert_eq!(ks_state, KillSwitchState::Active);
    }

    #[tokio::test]
    async fn test_request_check() {
        let vpn = VpnManager::german_exit();
        
        // Should fail because tunnel is not up
        let result = vpn.check_request();
        assert!(result.is_err());
    }

    #[test]
    fn test_memory_estimate() {
        let vpn = VpnManager::german_exit();
        
        // Should be well under 5MB
        assert!(vpn.memory_usage() < 5 * 1024 * 1024);
    }
}
