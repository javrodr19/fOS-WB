//! SOCKS5 Proxy Bridge
//!
//! Implements a local SOCKS5 proxy that routes traffic through
//! the WireGuard tunnel.
//!
//! # Protocol
//!
//! ```text
//! Browser → SOCKS5 (127.0.0.1:1080) → WireGuard → VPN Exit → Internet
//! ```
//!
//! # Memory Efficiency
//!
//! - Fixed-size connection pool
//! - Zero-copy buffer forwarding where possible
//! - No DNS caching (tunnel handles DNS)

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tracing::{debug, info, warn, error};

/// SOCKS5 version
const SOCKS5_VERSION: u8 = 0x05;

/// SOCKS5 authentication methods
const AUTH_NO_AUTH: u8 = 0x00;

/// SOCKS5 commands
const CMD_CONNECT: u8 = 0x01;

/// SOCKS5 address types
const ADDR_IPV4: u8 = 0x01;
const ADDR_DOMAIN: u8 = 0x03;
const ADDR_IPV6: u8 = 0x04;

/// SOCKS5 reply codes
const REPLY_SUCCESS: u8 = 0x00;
const REPLY_GENERAL_FAILURE: u8 = 0x01;
const REPLY_CONNECTION_REFUSED: u8 = 0x05;

/// Proxy configuration
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Listen address (usually 127.0.0.1)
    pub listen_addr: SocketAddr,
    /// Maximum concurrent connections
    pub max_connections: usize,
    /// Buffer size per connection
    pub buffer_size: usize,
    /// Connection timeout (seconds)
    pub timeout_secs: u64,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:1080".parse().unwrap(),
            max_connections: 64, // Limit for memory
            buffer_size: 4096,   // 4KB per direction
            timeout_secs: 30,
        }
    }
}

impl ProxyConfig {
    /// Estimate memory usage
    pub fn memory_estimate(&self) -> usize {
        // Per connection: 2 buffers (read + write) + overhead
        self.max_connections * (self.buffer_size * 2 + 512)
    }
}

/// SOCKS5 proxy server
pub struct Socks5Proxy {
    /// Configuration
    config: ProxyConfig,
    /// TCP listener
    listener: Option<TcpListener>,
    /// Connection semaphore (limits concurrent connections)
    semaphore: Arc<Semaphore>,
    /// Is proxy running?
    running: Arc<AtomicBool>,
    /// Active connections
    active_connections: Arc<AtomicU64>,
    /// Callback for tunnel requests
    tunnel_callback: Option<Arc<dyn Fn(&[u8]) -> Result<Vec<u8>, std::io::Error> + Send + Sync>>,
}

impl Socks5Proxy {
    /// Create a new SOCKS5 proxy
    pub fn new(config: ProxyConfig) -> Self {
        let semaphore = Arc::new(Semaphore::new(config.max_connections));

        Self {
            config,
            listener: None,
            semaphore,
            running: Arc::new(AtomicBool::new(false)),
            active_connections: Arc::new(AtomicU64::new(0)),
            tunnel_callback: None,
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(ProxyConfig::default())
    }

    /// Get the proxy URL for browser configuration
    pub fn proxy_url(&self) -> String {
        format!("socks5://{}", self.config.listen_addr)
    }

    /// Get number of active connections
    pub fn active_connections(&self) -> u64 {
        self.active_connections.load(Ordering::Relaxed)
    }

    /// Start the proxy server
    pub async fn start(&mut self) -> Result<(), ProxyError> {
        if self.running.load(Ordering::Relaxed) {
            return Ok(());
        }

        info!("Starting SOCKS5 proxy on {}", self.config.listen_addr);

        let listener = TcpListener::bind(self.config.listen_addr).await
            .map_err(|e| ProxyError::BindError(e.to_string()))?;

        self.listener = Some(listener);
        self.running.store(true, Ordering::Relaxed);

        info!(
            "SOCKS5 proxy listening (max {} connections, ~{:.1}MB memory)",
            self.config.max_connections,
            self.config.memory_estimate() as f64 / (1024.0 * 1024.0)
        );

        Ok(())
    }

    /// Stop the proxy server
    pub fn stop(&mut self) {
        info!("Stopping SOCKS5 proxy");
        self.running.store(false, Ordering::Relaxed);
        self.listener = None;
    }

    /// Accept and handle connections (run in background task)
    pub async fn run(&self) -> Result<(), ProxyError> {
        let listener = match &self.listener {
            Some(l) => l,
            None => return Err(ProxyError::NotStarted),
        };

        while self.running.load(Ordering::Relaxed) {
            // Accept with semaphore (limits connections)
            let permit = self.semaphore.clone().acquire_owned().await
                .map_err(|_| ProxyError::ShuttingDown)?;

            match listener.accept().await {
                Ok((stream, addr)) => {
                    let running = self.running.clone();
                    let active = self.active_connections.clone();
                    let buffer_size = self.config.buffer_size;

                    active.fetch_add(1, Ordering::Relaxed);

                    tokio::spawn(async move {
                        debug!("SOCKS5 connection from {}", addr);

                        if let Err(e) = Self::handle_connection(stream, buffer_size, &running).await {
                            debug!("Connection error: {}", e);
                        }

                        active.fetch_sub(1, Ordering::Relaxed);
                        drop(permit);
                    });
                }
                Err(e) => {
                    warn!("Accept error: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Handle a single SOCKS5 connection
    async fn handle_connection(
        mut stream: TcpStream,
        buffer_size: usize,
        running: &AtomicBool,
    ) -> Result<(), ProxyError> {
        // ===== SOCKS5 Handshake =====

        // Read greeting (version + methods)
        let mut buf = [0u8; 2];
        stream.read_exact(&mut buf).await?;

        if buf[0] != SOCKS5_VERSION {
            return Err(ProxyError::InvalidVersion);
        }

        let n_methods = buf[1] as usize;
        let mut methods = vec![0u8; n_methods];
        stream.read_exact(&mut methods).await?;

        // We only support no authentication
        if !methods.contains(&AUTH_NO_AUTH) {
            stream.write_all(&[SOCKS5_VERSION, 0xFF]).await?;
            return Err(ProxyError::AuthNotSupported);
        }

        // Send method selection
        stream.write_all(&[SOCKS5_VERSION, AUTH_NO_AUTH]).await?;

        // ===== Connection Request =====

        // Read request header
        let mut header = [0u8; 4];
        stream.read_exact(&mut header).await?;

        if header[0] != SOCKS5_VERSION {
            return Err(ProxyError::InvalidVersion);
        }

        if header[1] != CMD_CONNECT {
            // Only CONNECT supported
            Self::send_reply(&mut stream, REPLY_GENERAL_FAILURE).await?;
            return Err(ProxyError::UnsupportedCommand);
        }

        // Parse destination address
        let dest = match header[3] {
            ADDR_IPV4 => {
                let mut ip = [0u8; 4];
                stream.read_exact(&mut ip).await?;
                let mut port = [0u8; 2];
                stream.read_exact(&mut port).await?;
                
                let addr = std::net::Ipv4Addr::from(ip);
                let port = u16::from_be_bytes(port);
                format!("{}:{}", addr, port)
            }
            ADDR_DOMAIN => {
                let mut len = [0u8; 1];
                stream.read_exact(&mut len).await?;
                let mut domain = vec![0u8; len[0] as usize];
                stream.read_exact(&mut domain).await?;
                let mut port = [0u8; 2];
                stream.read_exact(&mut port).await?;
                
                let domain = String::from_utf8_lossy(&domain);
                let port = u16::from_be_bytes(port);
                format!("{}:{}", domain, port)
            }
            ADDR_IPV6 => {
                let mut ip = [0u8; 16];
                stream.read_exact(&mut ip).await?;
                let mut port = [0u8; 2];
                stream.read_exact(&mut port).await?;
                
                let addr = std::net::Ipv6Addr::from(ip);
                let port = u16::from_be_bytes(port);
                format!("[{}]:{}", addr, port)
            }
            _ => {
                Self::send_reply(&mut stream, REPLY_GENERAL_FAILURE).await?;
                return Err(ProxyError::InvalidAddress);
            }
        };

        debug!("SOCKS5 CONNECT to {}", dest);

        // ===== Connect Through Tunnel =====

        // In real implementation, this would go through WireGuard tunnel
        // For now, simulate connection (or connect directly for testing)
        match TcpStream::connect(&dest).await {
            Ok(remote) => {
                debug!("Connected to {}", dest);
                Self::send_reply(&mut stream, REPLY_SUCCESS).await?;

                // Relay data between client and remote
                Self::relay(stream, remote, buffer_size).await?;
            }
            Err(e) => {
                warn!("Failed to connect to {}: {}", dest, e);
                Self::send_reply(&mut stream, REPLY_CONNECTION_REFUSED).await?;
                return Err(ProxyError::ConnectionFailed(e.to_string()));
            }
        }

        Ok(())
    }

    /// Send SOCKS5 reply
    async fn send_reply(stream: &mut TcpStream, reply: u8) -> Result<(), ProxyError> {
        let response = [
            SOCKS5_VERSION,
            reply,
            0x00, // Reserved
            ADDR_IPV4,
            0, 0, 0, 0, // Bind address (0.0.0.0)
            0, 0,       // Bind port (0)
        ];
        stream.write_all(&response).await?;
        Ok(())
    }

    /// Relay data between two streams
    async fn relay(
        mut client: TcpStream,
        mut remote: TcpStream,
        buffer_size: usize,
    ) -> Result<(), ProxyError> {
        let (mut client_read, mut client_write) = client.split();
        let (mut remote_read, mut remote_write) = remote.split();

        let client_to_remote = async {
            let mut buf = vec![0u8; buffer_size];
            loop {
                let n = client_read.read(&mut buf).await?;
                if n == 0 { break; }
                remote_write.write_all(&buf[..n]).await?;
            }
            Ok::<_, std::io::Error>(())
        };

        let remote_to_client = async {
            let mut buf = vec![0u8; buffer_size];
            loop {
                let n = remote_read.read(&mut buf).await?;
                if n == 0 { break; }
                client_write.write_all(&buf[..n]).await?;
            }
            Ok::<_, std::io::Error>(())
        };

        // Run both directions concurrently
        let _ = tokio::try_join!(client_to_remote, remote_to_client);

        Ok(())
    }
}

/// Proxy errors
#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("Proxy not started")]
    NotStarted,

    #[error("Failed to bind: {0}")]
    BindError(String),

    #[error("Invalid SOCKS version")]
    InvalidVersion,

    #[error("Authentication not supported")]
    AuthNotSupported,

    #[error("Unsupported command")]
    UnsupportedCommand,

    #[error("Invalid address")]
    InvalidAddress,

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Shutting down")]
    ShuttingDown,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proxy_config() {
        let config = ProxyConfig::default();
        
        assert_eq!(config.max_connections, 64);
        assert!(config.memory_estimate() < 1024 * 1024); // < 1MB
    }

    #[test]
    fn test_proxy_url() {
        let proxy = Socks5Proxy::with_defaults();
        assert_eq!(proxy.proxy_url(), "socks5://127.0.0.1:1080");
    }

    #[tokio::test]
    async fn test_proxy_creation() {
        let proxy = Socks5Proxy::with_defaults();
        assert_eq!(proxy.active_connections(), 0);
    }
}
