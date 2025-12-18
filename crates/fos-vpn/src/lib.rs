//! fOS VPN - Userspace WireGuard with SOCKS5 Proxy
//!
//! Provides an integrated VPN feature using userspace WireGuard
//! (no root/admin required) with a local SOCKS5 proxy bridge.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                    Browser Process                       │
//! │                                                          │
//! │  ┌──────────┐    ┌──────────────┐    ┌───────────────┐  │
//! │  │  HTTP    │───▶│ SOCKS5 Proxy │───▶│  WireGuard    │  │
//! │  │  Client  │    │ (127.0.0.1)  │    │   Tunnel      │  │
//! │  └──────────┘    └──────────────┘    └───────┬───────┘  │
//! │                                              │          │
//! └──────────────────────────────────────────────│──────────┘
//!                                                │
//!                                                ▼ UDP
//!                                    ┌───────────────────┐
//!                                    │  German VPN Exit  │
//!                                    │  (WireGuard Peer) │
//!                                    └───────────────────┘
//! ```
//!
//! # Features
//!
//! - **Userspace WireGuard**: No TUN device, no root required
//! - **Local SOCKS5 Proxy**: Browser routes through 127.0.0.1:1080
//! - **Kill Switch**: Connection refused if tunnel drops
//! - **Memory Efficient**: <5MB additional RAM
//!
//! # Security
//!
//! - All traffic encrypted with ChaCha20-Poly1305
//! - Perfect forward secrecy via X25519 key exchange
//! - Kill switch prevents IP leaks

mod config;
mod tunnel;
mod proxy;
mod manager;
mod keys;
mod kill_switch;
mod region;

pub use config::{VpnConfig, PeerConfig, Endpoint};
pub use tunnel::{WireGuardTunnel, TunnelState, TunnelStats};
pub use proxy::{Socks5Proxy, ProxyConfig};
pub use manager::{VpnManager, VpnState, VpnError};
pub use keys::{PrivateKey, PublicKey, KeyPair};
pub use kill_switch::{KillSwitch, KillSwitchState};
pub use region::{
    VpnRegionManager, RegionId, RegionProfile, RegionConfig,
    RegionHealth, ConfigFormat, RegionError,
};
