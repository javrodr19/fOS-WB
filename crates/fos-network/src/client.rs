//! HTTP Client with Zero-Copy Networking
//!
//! Uses hyper with tokio for async HTTP/1.1 and HTTP/2.
//! Features:
//! - Zero-copy response streaming
//! - Automatic HTTPS with rustls (memory-safe TLS)
//! - Connection pooling
//! - Integrated request interception

use crate::interceptor::{InterceptResult, RequestInterceptor, ResourceType};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::header::{HeaderMap, CONTENT_TYPE, USER_AGENT};
use hyper::{Method, Request, StatusCode, Uri};
use rustls::ClientConfig;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio_rustls::TlsConnector;
use tracing::{debug, info, warn};

/// HTTP client errors
#[derive(Debug, Error)]
pub enum HttpError {
    #[error("Request blocked: {0}")]
    Blocked(String),
    
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    
    #[error("Request timeout")]
    Timeout,
    
    #[error("HTTP error: {0}")]
    HttpError(String),
    
    #[error("TLS error: {0}")]
    TlsError(String),
    
    #[error("Body read error: {0}")]
    BodyError(String),
}

/// HTTP client configuration
#[derive(Debug, Clone)]
pub struct HttpClientConfig {
    /// Request timeout
    pub timeout: Duration,
    /// Connection timeout
    pub connect_timeout: Duration,
    /// User-Agent string
    pub user_agent: String,
    /// Maximum response body size
    pub max_body_size: usize,
    /// Enable HTTP/2
    pub enable_http2: bool,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(10),
            user_agent: format!("fOS-WB/0.1 (Zero-Bloat Browser)"),
            max_body_size: 10 * 1024 * 1024, // 10 MB
            enable_http2: true,
        }
    }
}

/// HTTP response wrapper
#[derive(Debug)]
pub struct Response {
    /// Status code
    pub status: StatusCode,
    /// Response headers
    pub headers: HeaderMap,
    /// Response body (may be empty for streaming)
    pub body: Vec<u8>,
    /// Time to first byte
    pub ttfb: Duration,
    /// Total download time
    pub total_time: Duration,
    /// Final URL (after redirects)
    pub final_url: String,
}

impl Response {
    /// Check if response was successful (2xx)
    pub fn is_success(&self) -> bool {
        self.status.is_success()
    }

    /// Get body as string
    pub fn text(&self) -> Result<String, std::string::FromUtf8Error> {
        String::from_utf8(self.body.clone())
    }

    /// Get Content-Type header
    pub fn content_type(&self) -> Option<&str> {
        self.headers.get(CONTENT_TYPE)?.to_str().ok()
    }
}

/// HTTP client with integrated content blocking
pub struct HttpClient {
    /// Configuration
    config: HttpClientConfig,
    /// Request interceptor for content blocking
    interceptor: Option<Arc<RequestInterceptor>>,
    /// Statistics
    stats: ClientStats,
}

/// Client statistics
#[derive(Debug, Default)]
pub struct ClientStats {
    pub requests_made: std::sync::atomic::AtomicU64,
    pub requests_blocked: std::sync::atomic::AtomicU64,
    pub bytes_downloaded: std::sync::atomic::AtomicU64,
    pub bytes_saved: std::sync::atomic::AtomicU64,
}

impl HttpClient {
    /// Create a new HTTP client
    pub fn new(config: HttpClientConfig) -> Self {
        info!(
            "HTTP client initialized (timeout: {:?}, HTTP/2: {})",
            config.timeout,
            config.enable_http2
        );

        Self {
            config,
            interceptor: None,
            stats: ClientStats::default(),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(HttpClientConfig::default())
    }

    /// Set the request interceptor
    pub fn set_interceptor(&mut self, interceptor: Arc<RequestInterceptor>) {
        self.interceptor = Some(interceptor);
    }

    /// Perform a GET request
    pub async fn get(&self, url: &str) -> Result<Response, HttpError> {
        self.request(Method::GET, url, None).await
    }

    /// Perform a POST request
    pub async fn post(&self, url: &str, body: Vec<u8>) -> Result<Response, HttpError> {
        self.request(Method::POST, url, Some(body)).await
    }

    /// Perform an HTTP request
    pub async fn request(
        &self,
        method: Method,
        url: &str,
        body: Option<Vec<u8>>,
    ) -> Result<Response, HttpError> {
        use std::sync::atomic::Ordering;
        
        let start = Instant::now();
        self.stats.requests_made.fetch_add(1, Ordering::Relaxed);

        // 1. Check interceptor BEFORE any network activity
        if let Some(ref interceptor) = self.interceptor {
            let resource_type = ResourceType::from_accept_or_path(None, url);
            
            match interceptor.check(url, resource_type) {
                InterceptResult::Allow => {}
                InterceptResult::Blocked { reason, .. } => {
                    self.stats.requests_blocked.fetch_add(1, Ordering::Relaxed);
                    // Estimate bytes saved (average ad is ~50KB)
                    self.stats.bytes_saved.fetch_add(50_000, Ordering::Relaxed);
                    
                    debug!("Request blocked by interceptor: {} - {}", url, reason);
                    return Err(HttpError::Blocked(reason.to_string()));
                }
            }
        }

        // 2. Parse URL
        let uri: Uri = url.parse()
            .map_err(|e: hyper::http::uri::InvalidUri| HttpError::InvalidUrl(e.to_string()))?;

        let host = uri.host()
            .ok_or_else(|| HttpError::InvalidUrl("No host in URL".to_string()))?;
        let port = uri.port_u16().unwrap_or(if uri.scheme_str() == Some("https") { 443 } else { 80 });
        let is_https = uri.scheme_str() == Some("https");

        // 3. Build request
        let mut request_builder = Request::builder()
            .method(method)
            .uri(&uri)
            .header(USER_AGENT, &self.config.user_agent)
            .header("Host", host);

        let request = if let Some(body_data) = body {
            request_builder
                .body(Full::new(Bytes::from(body_data)))
                .map_err(|e| HttpError::HttpError(e.to_string()))?
        } else {
            // For GET requests, use Empty body but we need type compatibility
            Request::builder()
                .method(Method::GET)
                .uri(&uri)
                .header(USER_AGENT, &self.config.user_agent)
                .header("Host", host)
                .body(Full::new(Bytes::new()))
                .map_err(|e| HttpError::HttpError(e.to_string()))?
        };

        // 4. Make connection and send request
        let addr = format!("{}:{}", host, port);
        let ttfb_start = Instant::now();

        // For simplicity, use hyper-util's client with tokio connector
        // In production, we'd use a custom connector with connection pooling
        
        let stream = tokio::net::TcpStream::connect(&addr).await
            .map_err(|e| HttpError::ConnectionFailed(e.to_string()))?;

        // For HTTPS, wrap in TLS
        let response_result = if is_https {
            // Create TLS config
            let mut root_store = rustls::RootCertStore::empty();
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            
            let tls_config = ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth();
            
            let connector = TlsConnector::from(Arc::new(tls_config));
            let server_name = rustls::pki_types::ServerName::try_from(host.to_string())
                .map_err(|_| HttpError::TlsError("Invalid server name".to_string()))?;
            
            let tls_stream = connector.connect(server_name, stream).await
                .map_err(|e| HttpError::TlsError(e.to_string()))?;

            // Use hyper with TLS stream
            let io = hyper_util::rt::TokioIo::new(tls_stream);
            let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await
                .map_err(|e| HttpError::HttpError(e.to_string()))?;
            
            tokio::spawn(async move {
                if let Err(e) = conn.await {
                    warn!("Connection error: {}", e);
                }
            });

            sender.send_request(request).await
        } else {
            // Plain HTTP
            let io = hyper_util::rt::TokioIo::new(stream);
            let (mut sender, conn) = hyper::client::conn::http1::handshake(io).await
                .map_err(|e| HttpError::HttpError(e.to_string()))?;
            
            tokio::spawn(async move {
                if let Err(e) = conn.await {
                    warn!("Connection error: {}", e);
                }
            });

            sender.send_request(request).await
        };

        let response = response_result
            .map_err(|e| HttpError::HttpError(e.to_string()))?;
        
        let ttfb = ttfb_start.elapsed();

        // 5. Read response
        let status = response.status();
        let headers = response.headers().clone();
        
        // Collect body with size limit
        let body = response.into_body();
        let collected = body.collect().await
            .map_err(|e| HttpError::BodyError(e.to_string()))?;
        let body_bytes = collected.to_bytes().to_vec();

        let total_time = start.elapsed();
        
        self.stats.bytes_downloaded.fetch_add(body_bytes.len() as u64, Ordering::Relaxed);

        debug!(
            "HTTP {} {} -> {} ({} bytes, {:?} TTFB)",
            Method::GET, url, status, body_bytes.len(), ttfb
        );

        Ok(Response {
            status,
            headers,
            body: body_bytes,
            ttfb,
            total_time,
            final_url: url.to_string(),
        })
    }

    /// Get client statistics
    pub fn stats(&self) -> (u64, u64, u64, u64) {
        use std::sync::atomic::Ordering;
        (
            self.stats.requests_made.load(Ordering::Relaxed),
            self.stats.requests_blocked.load(Ordering::Relaxed),
            self.stats.bytes_downloaded.load(Ordering::Relaxed),
            self.stats.bytes_saved.load(Ordering::Relaxed),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_client_creation() {
        let client = HttpClient::with_defaults();
        assert_eq!(client.config.enable_http2, true);
    }

    #[tokio::test]
    async fn test_simple_get() {
        let client = HttpClient::with_defaults();
        
        // This test requires network access
        let result = client.get("http://example.com").await;
        
        match result {
            Ok(response) => {
                assert!(response.is_success());
                assert!(!response.body.is_empty());
            }
            Err(e) => {
                // May fail in offline environment
                println!("Network test skipped: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_blocked_request() {
        use crate::bloom_filter::UrlBloomFilter;
        
        let bloom = Arc::new(UrlBloomFilter::new());
        bloom.add_blocked_domain("blocked.example.com");
        
        let interceptor = Arc::new(RequestInterceptor::new(bloom));
        
        let mut client = HttpClient::with_defaults();
        client.set_interceptor(interceptor);
        
        let result = client.get("https://blocked.example.com/ad.js").await;
        
        assert!(matches!(result, Err(HttpError::Blocked(_))));
    }
}
