//! Kill Switch
//!
//! Implements a "soft kill switch" that prevents IP leaks
//! when the VPN tunnel is down.
//!
//! # Behavior
//!
//! When kill switch is active and tunnel is down:
//! - All network requests return `ConnectionRefused`
//! - DNS queries are blocked
//! - No traffic leaks to real IP
//!
//! This is a "soft" kill switch because:
//! - It only affects browser traffic (not system-wide)
//! - It can be disabled by user preference
//! - It doesn't modify system routing tables

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{debug, warn};

/// Kill switch state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillSwitchState {
    /// Kill switch is disabled
    Disabled,
    /// Kill switch is enabled and tunnel is up (traffic allowed)
    Enabled,
    /// Kill switch is active (tunnel down, traffic blocked)
    Active,
}

impl KillSwitchState {
    /// Check if traffic is allowed
    pub fn allows_traffic(&self) -> bool {
        matches!(self, KillSwitchState::Disabled | KillSwitchState::Enabled)
    }

    /// Check if traffic is blocked
    pub fn is_blocking(&self) -> bool {
        matches!(self, KillSwitchState::Active)
    }
}

/// Kill switch controller
///
/// # Usage
///
/// ```rust,ignore
/// let kill_switch = KillSwitch::new(true); // enabled
///
/// // When making network request:
/// if !kill_switch.allows_traffic() {
///     return Err(io::Error::new(
///         io::ErrorKind::ConnectionRefused,
///         "VPN tunnel is down (kill switch active)"
///     ));
/// }
///
/// // Tunnel events:
/// kill_switch.on_tunnel_up();   // Allow traffic
/// kill_switch.on_tunnel_down(); // Block traffic
/// ```
#[derive(Clone)]
pub struct KillSwitch {
    /// Is kill switch enabled in config?
    enabled: bool,
    /// Is tunnel currently up?
    tunnel_up: Arc<AtomicBool>,
}

impl KillSwitch {
    /// Create a new kill switch
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            tunnel_up: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create an always-disabled kill switch
    pub fn disabled() -> Self {
        Self::new(false)
    }

    /// Check current state
    pub fn state(&self) -> KillSwitchState {
        if !self.enabled {
            KillSwitchState::Disabled
        } else if self.tunnel_up.load(Ordering::Relaxed) {
            KillSwitchState::Enabled
        } else {
            KillSwitchState::Active
        }
    }

    /// Check if traffic is allowed
    pub fn allows_traffic(&self) -> bool {
        self.state().allows_traffic()
    }

    /// Check if traffic should be blocked
    pub fn is_blocking(&self) -> bool {
        self.state().is_blocking()
    }

    /// Notify that tunnel is up
    pub fn on_tunnel_up(&self) {
        debug!("Kill switch: tunnel up, traffic allowed");
        self.tunnel_up.store(true, Ordering::Relaxed);
    }

    /// Notify that tunnel is down
    pub fn on_tunnel_down(&self) {
        if self.enabled {
            warn!("Kill switch: tunnel down, blocking traffic");
        }
        self.tunnel_up.store(false, Ordering::Relaxed);
    }

    /// Check if kill switch is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Enable kill switch
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable kill switch
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Get error for blocked request
    pub fn blocking_error(&self) -> std::io::Error {
        std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "VPN tunnel is down (kill switch active)"
        )
    }
}

impl Default for KillSwitch {
    fn default() -> Self {
        Self::new(true) // Enabled by default for safety
    }
}

/// Result type for kill-switch-aware operations
pub type KillSwitchResult<T> = Result<T, KillSwitchError>;

/// Kill switch error
#[derive(Debug, Clone, thiserror::Error)]
pub enum KillSwitchError {
    #[error("Traffic blocked: VPN tunnel is down (kill switch active)")]
    TrafficBlocked,

    #[error("DNS blocked: VPN tunnel is down")]
    DnsBlocked,
}

/// Guard that checks kill switch before allowing operations
pub struct KillSwitchGuard<'a> {
    kill_switch: &'a KillSwitch,
}

impl<'a> KillSwitchGuard<'a> {
    /// Create a new guard
    pub fn new(kill_switch: &'a KillSwitch) -> KillSwitchResult<Self> {
        if kill_switch.is_blocking() {
            Err(KillSwitchError::TrafficBlocked)
        } else {
            Ok(Self { kill_switch })
        }
    }

    /// Check if still allowed (tunnel might drop during operation)
    pub fn check(&self) -> KillSwitchResult<()> {
        if self.kill_switch.is_blocking() {
            Err(KillSwitchError::TrafficBlocked)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kill_switch_disabled() {
        let ks = KillSwitch::disabled();
        
        assert_eq!(ks.state(), KillSwitchState::Disabled);
        assert!(ks.allows_traffic());
        assert!(!ks.is_blocking());
    }

    #[test]
    fn test_kill_switch_enabled_tunnel_down() {
        let ks = KillSwitch::new(true);
        
        // Initially tunnel is down, should block
        assert_eq!(ks.state(), KillSwitchState::Active);
        assert!(!ks.allows_traffic());
        assert!(ks.is_blocking());
    }

    #[test]
    fn test_kill_switch_enabled_tunnel_up() {
        let ks = KillSwitch::new(true);
        ks.on_tunnel_up();
        
        assert_eq!(ks.state(), KillSwitchState::Enabled);
        assert!(ks.allows_traffic());
        assert!(!ks.is_blocking());
    }

    #[test]
    fn test_kill_switch_tunnel_drop() {
        let ks = KillSwitch::new(true);
        
        ks.on_tunnel_up();
        assert!(ks.allows_traffic());
        
        ks.on_tunnel_down();
        assert!(!ks.allows_traffic());
        assert!(ks.is_blocking());
    }

    #[test]
    fn test_kill_switch_guard() {
        let ks = KillSwitch::new(true);
        
        // Tunnel down, guard should fail
        let result = KillSwitchGuard::new(&ks);
        assert!(result.is_err());
        
        // Tunnel up, guard should succeed
        ks.on_tunnel_up();
        let guard = KillSwitchGuard::new(&ks).unwrap();
        assert!(guard.check().is_ok());
    }
}
