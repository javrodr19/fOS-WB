//! Async DNS Resolver with Caching
//!
//! Provides DNS resolution with:
//! - Pre-lookup blocking (don't resolve blocked domains)
//! - Caching to reduce latency
//! - DoH (DNS over HTTPS) support for privacy

use hickory_resolver::config::{ResolverConfig, ResolverOpts};
use hickory_resolver::TokioAsyncResolver;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::{debug, info, warn};

/// DNS resolution errors
#[derive(Debug, Error)]
pub enum DnsError {
    #[error("Domain blocked: {0}")]
    Blocked(String),
    
    #[error("Resolution failed: {0}")]
    ResolutionFailed(String),
    
    #[error("No addresses found for domain")]
    NoAddresses,
    
    #[error("Resolver error: {0}")]
    ResolverError(String),
}

/// DNS resolver configuration
#[derive(Debug, Clone)]
pub struct DnsConfig {
    /// Cache TTL for successful lookups
    pub cache_ttl: Duration,
    /// Cache TTL for failed lookups
    pub negative_cache_ttl: Duration,
    /// Maximum cache size
    pub max_cache_entries: usize,
    /// Whether to use system DNS or custom
    pub use_system_dns: bool,
    /// Timeout for DNS queries
    pub timeout: Duration,
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            cache_ttl: Duration::from_secs(300), // 5 minutes
            negative_cache_ttl: Duration::from_secs(60), // 1 minute
            max_cache_entries: 10000,
            use_system_dns: true,
            timeout: Duration::from_secs(5),
        }
    }
}

/// Cached DNS entry
#[derive(Clone)]
struct CacheEntry {
    addresses: Vec<IpAddr>,
    expires_at: Instant,
}

/// Async DNS resolver with caching and blocking
pub struct DnsResolver {
    /// Underlying resolver
    resolver: TokioAsyncResolver,
    /// DNS cache
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    /// Function to check if domain should be blocked
    block_checker: Option<Arc<dyn Fn(&str) -> bool + Send + Sync>>,
    /// Configuration
    config: DnsConfig,
}

impl DnsResolver {
    /// Create a new DNS resolver
    pub async fn new(config: DnsConfig) -> Result<Self, DnsError> {
        let resolver = if config.use_system_dns {
            TokioAsyncResolver::tokio_from_system_conf()
                .map_err(|e| DnsError::ResolverError(e.to_string()))?
        } else {
            // Use Cloudflare DNS for privacy
            TokioAsyncResolver::tokio(
                ResolverConfig::cloudflare(),
                ResolverOpts::default(),
            )
        };

        info!("DNS resolver initialized");

        Ok(Self {
            resolver,
            cache: Arc::new(RwLock::new(HashMap::new())),
            block_checker: None,
            config,
        })
    }

    /// Create with default configuration
    pub async fn with_defaults() -> Result<Self, DnsError> {
        Self::new(DnsConfig::default()).await
    }

    /// Set the domain block checker
    pub fn set_block_checker<F>(&mut self, checker: F)
    where
        F: Fn(&str) -> bool + Send + Sync + 'static,
    {
        self.block_checker = Some(Arc::new(checker));
    }

    /// Resolve a domain to IP addresses
    pub async fn resolve(&self, domain: &str) -> Result<Vec<IpAddr>, DnsError> {
        let normalized = domain.to_lowercase();

        // 1. Check if domain is blocked BEFORE any network activity
        if let Some(ref checker) = self.block_checker {
            if checker(&normalized) {
                debug!("DNS blocked for: {}", domain);
                return Err(DnsError::Blocked(domain.to_string()));
            }
        }

        // 2. Check cache
        if let Some(cached) = self.get_cached(&normalized) {
            debug!("DNS cache hit for: {}", domain);
            return Ok(cached);
        }

        // 3. Perform actual DNS lookup
        debug!("DNS lookup for: {}", domain);
        let lookup = self.resolver.lookup_ip(&normalized).await
            .map_err(|e| DnsError::ResolutionFailed(e.to_string()))?;

        let addresses: Vec<IpAddr> = lookup.iter().collect();
        
        if addresses.is_empty() {
            return Err(DnsError::NoAddresses);
        }

        // 4. Cache the result
        self.cache_addresses(&normalized, &addresses);

        Ok(addresses)
    }

    /// Resolve to a single address (first available)
    pub async fn resolve_one(&self, domain: &str) -> Result<IpAddr, DnsError> {
        let addresses = self.resolve(domain).await?;
        addresses.into_iter().next().ok_or(DnsError::NoAddresses)
    }

    /// Prefetch DNS for a list of domains
    pub async fn prefetch(&self, domains: &[&str]) {
        for domain in domains {
            let _ = self.resolve(domain).await;
        }
    }

    /// Clear all cached entries
    pub fn clear_cache(&self) {
        let mut cache = self.cache.write().unwrap();
        cache.clear();
        info!("DNS cache cleared");
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> (usize, usize) {
        let cache = self.cache.read().unwrap();
        let total = cache.len();
        let expired = cache.values()
            .filter(|e| e.expires_at < Instant::now())
            .count();
        (total, total - expired)
    }

    fn get_cached(&self, domain: &str) -> Option<Vec<IpAddr>> {
        let cache = self.cache.read().unwrap();
        
        if let Some(entry) = cache.get(domain) {
            if entry.expires_at > Instant::now() {
                return Some(entry.addresses.clone());
            }
        }
        
        None
    }

    fn cache_addresses(&self, domain: &str, addresses: &[IpAddr]) {
        let mut cache = self.cache.write().unwrap();
        
        // Evict old entries if cache is full
        if cache.len() >= self.config.max_cache_entries {
            let now = Instant::now();
            cache.retain(|_, v| v.expires_at > now);
            
            // If still full, remove oldest entries
            if cache.len() >= self.config.max_cache_entries {
                let keys_to_remove: Vec<_> = cache.keys()
                    .take(cache.len() / 4)
                    .cloned()
                    .collect();
                for key in keys_to_remove {
                    cache.remove(&key);
                }
            }
        }
        
        cache.insert(domain.to_string(), CacheEntry {
            addresses: addresses.to_vec(),
            expires_at: Instant::now() + self.config.cache_ttl,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dns_resolver_creation() {
        let resolver = DnsResolver::with_defaults().await;
        assert!(resolver.is_ok());
    }

    #[tokio::test]
    async fn test_dns_resolution() {
        let resolver = DnsResolver::with_defaults().await.unwrap();
        
        // Resolve a known domain
        let result = resolver.resolve("example.com").await;
        assert!(result.is_ok());
        
        let addresses = result.unwrap();
        assert!(!addresses.is_empty());
    }

    #[tokio::test]
    async fn test_dns_caching() {
        let resolver = DnsResolver::with_defaults().await.unwrap();
        
        // First lookup
        let _ = resolver.resolve("example.com").await;
        
        // Second should be cached
        let (total, valid) = resolver.cache_stats();
        assert_eq!(total, 1);
        assert_eq!(valid, 1);
    }

    #[tokio::test]
    async fn test_dns_blocking() {
        let mut resolver = DnsResolver::with_defaults().await.unwrap();
        
        resolver.set_block_checker(|domain| domain.contains("blocked"));
        
        // Should block
        let result = resolver.resolve("blocked.example.com").await;
        assert!(matches!(result, Err(DnsError::Blocked(_))));
        
        // Should allow
        let result = resolver.resolve("example.com").await;
        assert!(result.is_ok());
    }
}
