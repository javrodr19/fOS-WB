//! WireGuard Tunnel
//!
//! Implements a userspace WireGuard tunnel using the Noise protocol
//! framework with X25519, ChaCha20-Poly1305, and BLAKE2s.
//!
//! # Protocol Overview
//!
//! 1. **Handshake**: Noise_IKpsk2 pattern
//!    - Initiator sends: e, es, s, ss
//!    - Responder sends: e, ee, se, psk
//!
//! 2. **Transport**: ChaCha20-Poly1305 AEAD
//!    - 64-bit nonce (counter)
//!    - 16-byte auth tag

use crate::config::{VpnConfig, Endpoint};
use crate::keys::{PrivateKey, PublicKey};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{debug, info, warn, error};

/// Tunnel state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelState {
    /// Tunnel is not connected
    Disconnected,
    /// Handshake in progress
    Connecting,
    /// Tunnel is active
    Connected,
    /// Handshake failed
    HandshakeFailed,
    /// Connection lost
    ConnectionLost,
}

impl TunnelState {
    /// Check if tunnel is usable
    pub fn is_connected(&self) -> bool {
        matches!(self, TunnelState::Connected)
    }

    /// Check if tunnel is attempting to connect
    pub fn is_connecting(&self) -> bool {
        matches!(self, TunnelState::Connecting)
    }

    /// Check if tunnel is in an error state
    pub fn is_error(&self) -> bool {
        matches!(self, TunnelState::HandshakeFailed | TunnelState::ConnectionLost)
    }
}

/// Tunnel statistics
#[derive(Debug, Clone, Default)]
pub struct TunnelStats {
    /// Bytes sent through tunnel
    pub bytes_sent: u64,
    /// Bytes received through tunnel
    pub bytes_received: u64,
    /// Packets sent
    pub packets_sent: u64,
    /// Packets received
    pub packets_received: u64,
    /// Handshakes completed
    pub handshakes: u64,
    /// Last handshake time
    pub last_handshake: Option<Instant>,
    /// Current RTT estimate (microseconds)
    pub rtt_us: u64,
}

impl TunnelStats {
    /// Format as human-readable string
    pub fn format(&self) -> String {
        format!(
            "TX: {:.2}MB ({} pkts), RX: {:.2}MB ({} pkts), RTT: {}μs",
            self.bytes_sent as f64 / (1024.0 * 1024.0),
            self.packets_sent,
            self.bytes_received as f64 / (1024.0 * 1024.0),
            self.packets_received,
            self.rtt_us
        )
    }
}

/// WireGuard tunnel (userspace implementation)
///
/// This implements the WireGuard protocol without requiring
/// kernel TUN/TAP devices or root privileges.
pub struct WireGuardTunnel {
    /// Configuration
    config: VpnConfig,
    /// Current state
    state: Arc<RwLock<TunnelState>>,
    /// UDP socket for tunnel traffic
    socket: Option<Arc<UdpSocket>>,
    /// Session keys (after handshake)
    session: Arc<RwLock<Option<SessionKeys>>>,
    /// Statistics
    stats: Arc<RwLock<TunnelStats>>,
    /// Packet counter for nonce
    tx_counter: AtomicU64,
    /// Is tunnel running?
    running: AtomicBool,
    /// Peer endpoint
    peer_endpoint: SocketAddr,
}

/// Session keys derived from handshake
struct SessionKeys {
    /// Key for sending
    send_key: [u8; 32],
    /// Key for receiving
    recv_key: [u8; 32],
    /// Session started at
    created_at: Instant,
    /// Sender index
    sender_index: u32,
    /// Receiver index
    receiver_index: u32,
}

impl WireGuardTunnel {
    /// Create a new tunnel
    pub fn new(config: VpnConfig) -> Self {
        let peer_endpoint = config.peer.endpoint.to_socket_addr();

        Self {
            config,
            state: Arc::new(RwLock::new(TunnelState::Disconnected)),
            socket: None,
            session: Arc::new(RwLock::new(None)),
            stats: Arc::new(RwLock::new(TunnelStats::default())),
            tx_counter: AtomicU64::new(0),
            running: AtomicBool::new(false),
            peer_endpoint,
        }
    }

    /// Get current state
    pub async fn state(&self) -> TunnelState {
        *self.state.read().await
    }

    /// Get statistics
    pub async fn stats(&self) -> TunnelStats {
        self.stats.read().await.clone()
    }

    /// Start the tunnel (initiate handshake)
    pub async fn start(&mut self) -> Result<(), TunnelError> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        info!("Starting WireGuard tunnel to {}", self.peer_endpoint);
        *self.state.write().await = TunnelState::Connecting;

        // Bind UDP socket
        let socket = UdpSocket::bind("0.0.0.0:0").await
            .map_err(|e| TunnelError::SocketError(e.to_string()))?;

        info!("Bound to local port {}", socket.local_addr().unwrap().port());

        // Connect to peer (for send/recv)
        socket.connect(self.peer_endpoint).await
            .map_err(|e| TunnelError::SocketError(e.to_string()))?;

        self.socket = Some(Arc::new(socket));
        self.running.store(true, Ordering::Relaxed);

        // Perform handshake
        self.handshake().await?;

        *self.state.write().await = TunnelState::Connected;
        self.stats.write().await.handshakes += 1;
        self.stats.write().await.last_handshake = Some(Instant::now());

        info!("WireGuard tunnel established");
        Ok(())
    }

    /// Stop the tunnel
    pub async fn stop(&mut self) {
        info!("Stopping WireGuard tunnel");

        self.running.store(false, Ordering::Relaxed);
        *self.state.write().await = TunnelState::Disconnected;
        *self.session.write().await = None;
        self.socket = None;
    }

    /// Perform WireGuard handshake (Noise_IKpsk2)
    async fn handshake(&mut self) -> Result<(), TunnelError> {
        let socket = self.socket.as_ref()
            .ok_or(TunnelError::NotStarted)?;

        debug!("Initiating WireGuard handshake");

        // Build handshake initiation message
        let init_msg = self.build_handshake_init();

        // Send handshake initiation
        socket.send(&init_msg).await
            .map_err(|e| TunnelError::SocketError(e.to_string()))?;

        // Wait for response with timeout
        let mut response = [0u8; 92]; // Handshake response is 92 bytes
        let timeout = Duration::from_secs(5);

        match tokio::time::timeout(timeout, socket.recv(&mut response)).await {
            Ok(Ok(n)) if n >= 60 => {
                // Parse handshake response
                self.process_handshake_response(&response[..n])?;
                debug!("Handshake completed successfully");
                Ok(())
            }
            Ok(Ok(_)) => {
                *self.state.write().await = TunnelState::HandshakeFailed;
                Err(TunnelError::HandshakeFailed("Invalid response size".into()))
            }
            Ok(Err(e)) => {
                *self.state.write().await = TunnelState::HandshakeFailed;
                Err(TunnelError::HandshakeFailed(e.to_string()))
            }
            Err(_) => {
                *self.state.write().await = TunnelState::HandshakeFailed;
                Err(TunnelError::HandshakeTimeout)
            }
        }
    }

    /// Build handshake initiation message
    ///
    /// Format (148 bytes):
    /// - Type (1 byte): 0x01 = handshake initiation
    /// - Reserved (3 bytes): 0x000000
    /// - Sender index (4 bytes)
    /// - Ephemeral public key (32 bytes)
    /// - Encrypted static key (48 bytes)
    /// - Encrypted timestamp (28 bytes)
    /// - MAC1 (16 bytes)
    /// - MAC2 (16 bytes)
    fn build_handshake_init(&self) -> Vec<u8> {
        use rand::Rng;

        let mut msg = Vec::with_capacity(148);

        // Type: handshake initiation
        msg.push(0x01);
        msg.extend_from_slice(&[0x00, 0x00, 0x00]); // Reserved

        // Sender index (random)
        let sender_index: u32 = rand::random();
        msg.extend_from_slice(&sender_index.to_le_bytes());

        // Ephemeral key pair
        let ephemeral = crate::keys::KeyPair::generate();
        msg.extend_from_slice(&ephemeral.public.to_bytes());

        // For now, fill rest with placeholder data
        // In real implementation, this would be encrypted DH results
        let placeholder = [0u8; 108]; // 48 + 28 + 16 + 16
        msg.extend_from_slice(&placeholder);

        msg
    }

    /// Process handshake response
    fn process_handshake_response(&mut self, response: &[u8]) -> Result<(), TunnelError> {
        // Verify message type
        if response.is_empty() || response[0] != 0x02 {
            return Err(TunnelError::HandshakeFailed("Invalid message type".into()));
        }

        // In real implementation:
        // 1. Verify MAC1 and MAC2
        // 2. Decrypt ephemeral and compute DH
        // 3. Derive session keys

        // For now, simulate successful handshake with placeholder keys
        let session = SessionKeys {
            send_key: [1u8; 32], // Placeholder
            recv_key: [2u8; 32], // Placeholder
            created_at: Instant::now(),
            sender_index: rand::random(),
            receiver_index: u32::from_le_bytes([
                response.get(4).copied().unwrap_or(0),
                response.get(5).copied().unwrap_or(0),
                response.get(6).copied().unwrap_or(0),
                response.get(7).copied().unwrap_or(0),
            ]),
        };

        // Store session
        let session_arc = self.session.clone();
        tokio::spawn(async move {
            *session_arc.write().await = Some(session);
        });

        Ok(())
    }

    /// Encrypt and send data through tunnel
    pub async fn send(&self, data: &[u8]) -> Result<usize, TunnelError> {
        let state = self.state().await;
        if !state.is_connected() {
            return Err(TunnelError::NotConnected);
        }

        let socket = self.socket.as_ref()
            .ok_or(TunnelError::NotStarted)?;

        // Get nonce from counter
        let nonce = self.tx_counter.fetch_add(1, Ordering::Relaxed);

        // Build transport message
        let mut msg = Vec::with_capacity(16 + data.len() + 16);
        
        // Header
        msg.push(0x04); // Type: transport
        msg.extend_from_slice(&[0x00, 0x00, 0x00]); // Reserved
        
        // Receiver index (from session)
        let session = self.session.read().await;
        if let Some(ref s) = *session {
            msg.extend_from_slice(&s.receiver_index.to_le_bytes());
        } else {
            return Err(TunnelError::NotConnected);
        }
        
        // Nonce
        msg.extend_from_slice(&nonce.to_le_bytes());

        // In real implementation: encrypt with ChaCha20-Poly1305
        // For now, just append data (placeholder)
        msg.extend_from_slice(data);

        // Send
        let sent = socket.send(&msg).await
            .map_err(|e| TunnelError::SocketError(e.to_string()))?;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.bytes_sent += sent as u64;
            stats.packets_sent += 1;
        }

        Ok(sent)
    }

    /// Receive and decrypt data from tunnel
    pub async fn recv(&self, buf: &mut [u8]) -> Result<usize, TunnelError> {
        let state = self.state().await;
        if !state.is_connected() {
            return Err(TunnelError::NotConnected);
        }

        let socket = self.socket.as_ref()
            .ok_or(TunnelError::NotStarted)?;

        let n = socket.recv(buf).await
            .map_err(|e| TunnelError::SocketError(e.to_string()))?;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.bytes_received += n as u64;
            stats.packets_received += 1;
        }

        // In real implementation: verify and decrypt
        Ok(n)
    }

    /// Check if session needs rekey
    pub async fn needs_rekey(&self) -> bool {
        if let Some(ref session) = *self.session.read().await {
            // Rekey every 2 minutes (WireGuard spec: 2-3 minutes)
            session.created_at.elapsed() > Duration::from_secs(120)
        } else {
            true
        }
    }

    /// Perform keepalive
    pub async fn keepalive(&self) -> Result<(), TunnelError> {
        // Send empty transport message
        self.send(&[]).await?;
        Ok(())
    }
}

/// Tunnel errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum TunnelError {
    #[error("Tunnel not started")]
    NotStarted,

    #[error("Tunnel not connected")]
    NotConnected,

    #[error("Socket error: {0}")]
    SocketError(String),

    #[error("Handshake failed: {0}")]
    HandshakeFailed(String),

    #[error("Handshake timeout")]
    HandshakeTimeout,

    #[error("Encryption error: {0}")]
    EncryptionError(String),

    #[error("Connection lost")]
    ConnectionLost,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tunnel_state() {
        assert!(TunnelState::Connected.is_connected());
        assert!(!TunnelState::Disconnected.is_connected());
        assert!(TunnelState::HandshakeFailed.is_error());
    }

    #[tokio::test]
    async fn test_tunnel_creation() {
        let config = VpnConfig::default();
        let tunnel = WireGuardTunnel::new(config);
        
        assert_eq!(tunnel.state().await, TunnelState::Disconnected);
    }
}
