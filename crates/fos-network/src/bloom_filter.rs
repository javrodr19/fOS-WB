//! Bloom Filter for URL Matching
//!
//! Uses a probabilistic data structure to check if URLs match known
//! tracking/ad domains in O(1) time with minimal memory overhead.
//!
//! Key properties:
//! - False positives possible (blocked when shouldn't be)
//! - False negatives impossible (never misses a blocked domain)
//! - ~1.44 bytes per item at 1% false positive rate
//!
//! For 100,000 blocked domains: ~144 KB RAM

use bloomfilter::Bloom;
use std::collections::HashSet;
use std::sync::RwLock;
use tracing::{debug, info};
use xxhash_rust::xxh3::xxh3_64;

/// Default false positive rate (0.1% = very aggressive blocking)
const DEFAULT_FALSE_POSITIVE_RATE: f64 = 0.001;

/// Default expected number of items (EasyList has ~80,000 rules)
const DEFAULT_EXPECTED_ITEMS: usize = 100_000;

/// Configuration for the Bloom filter
#[derive(Debug, Clone)]
pub struct BloomConfig {
    /// Expected number of blocked domains/patterns
    pub expected_items: usize,
    /// Acceptable false positive rate (0.0 - 1.0)
    pub false_positive_rate: f64,
}

impl Default for BloomConfig {
    fn default() -> Self {
        Self {
            expected_items: DEFAULT_EXPECTED_ITEMS,
            false_positive_rate: DEFAULT_FALSE_POSITIVE_RATE,
        }
    }
}

/// High-performance URL Bloom filter for content blocking
pub struct UrlBloomFilter {
    /// Primary bloom filter for full URLs
    url_filter: RwLock<Bloom<[u8]>>,
    /// Bloom filter for domain-only matching
    domain_filter: RwLock<Bloom<[u8]>>,
    /// Exact match set for critical blocks (no false positives)
    exact_blocks: RwLock<HashSet<u64>>,
    /// Statistics
    stats: FilterStats,
    /// Configuration
    config: BloomConfig,
}

/// Filter statistics
#[derive(Debug, Default)]
pub struct FilterStats {
    /// Number of URL checks performed
    pub checks: std::sync::atomic::AtomicU64,
    /// Number of blocked requests
    pub blocked: std::sync::atomic::AtomicU64,
    /// Number of allowed requests
    pub allowed: std::sync::atomic::AtomicU64,
}

impl UrlBloomFilter {
    /// Create a new URL bloom filter with default configuration
    pub fn new() -> Self {
        Self::with_config(BloomConfig::default())
    }

    /// Create with custom configuration
    pub fn with_config(config: BloomConfig) -> Self {
        let url_filter = Bloom::new_for_fp_rate(
            config.expected_items,
            config.false_positive_rate,
        );
        
        let domain_filter = Bloom::new_for_fp_rate(
            config.expected_items / 10, // Fewer unique domains
            config.false_positive_rate,
        );

        info!(
            "Bloom filter initialized: {} expected items, {:.2}% FP rate, ~{} KB",
            config.expected_items,
            config.false_positive_rate * 100.0,
            url_filter.bitmap().len() / 8 / 1024
        );

        Self {
            url_filter: RwLock::new(url_filter),
            domain_filter: RwLock::new(domain_filter),
            exact_blocks: RwLock::new(HashSet::new()),
            stats: FilterStats::default(),
            config,
        }
    }

    /// Add a domain to the block list
    pub fn add_blocked_domain(&self, domain: &str) {
        let normalized = normalize_domain(domain);
        let hash = hash_string(&normalized);
        
        // Add to domain filter
        {
            let mut filter = self.domain_filter.write().unwrap();
            filter.set(normalized.as_bytes());
        }
        
        // Add hash to exact blocks for zero false positive on critical domains
        {
            let mut exact = self.exact_blocks.write().unwrap();
            exact.insert(hash);
        }
        
        debug!("Added blocked domain: {}", domain);
    }

    /// Add a URL pattern to the block list
    pub fn add_blocked_url(&self, url_pattern: &str) {
        let normalized = normalize_url(url_pattern);
        
        let mut filter = self.url_filter.write().unwrap();
        filter.set(normalized.as_bytes());
        
        debug!("Added blocked URL pattern: {}", url_pattern);
    }

    /// Add multiple domains efficiently
    pub fn add_blocked_domains(&self, domains: &[&str]) {
        let mut domain_filter = self.domain_filter.write().unwrap();
        let mut exact = self.exact_blocks.write().unwrap();
        
        for domain in domains {
            let normalized = normalize_domain(domain);
            let hash = hash_string(&normalized);
            domain_filter.set(normalized.as_bytes());
            exact.insert(hash);
        }
        
        info!("Added {} blocked domains", domains.len());
    }

    /// Check if a URL should be blocked
    ///
    /// Returns `true` if the URL matches a known tracking/ad pattern.
    /// This is the hot path - must be extremely fast (< 1μs).
    #[inline]
    pub fn should_block(&self, url: &str) -> bool {
        use std::sync::atomic::Ordering;
        
        self.stats.checks.fetch_add(1, Ordering::Relaxed);
        
        // Extract domain from URL
        let domain = extract_domain(url);
        
        // 1. Check exact domain block (zero false positives)
        let hash = hash_string(&normalize_domain(domain));
        {
            let exact = self.exact_blocks.read().unwrap();
            if exact.contains(&hash) {
                self.stats.blocked.fetch_add(1, Ordering::Relaxed);
                return true;
            }
        }
        
        // 2. Check domain bloom filter
        {
            let domain_filter = self.domain_filter.read().unwrap();
            if domain_filter.check(normalize_domain(domain).as_bytes()) {
                self.stats.blocked.fetch_add(1, Ordering::Relaxed);
                return true;
            }
        }
        
        // 3. Check full URL bloom filter
        {
            let url_filter = self.url_filter.read().unwrap();
            if url_filter.check(normalize_url(url).as_bytes()) {
                self.stats.blocked.fetch_add(1, Ordering::Relaxed);
                return true;
            }
        }
        
        self.stats.allowed.fetch_add(1, Ordering::Relaxed);
        false
    }

    /// Check if a domain should be blocked (domain-only check)
    #[inline]
    pub fn is_domain_blocked(&self, domain: &str) -> bool {
        let normalized = normalize_domain(domain);
        let hash = hash_string(&normalized);
        
        // Check exact first
        {
            let exact = self.exact_blocks.read().unwrap();
            if exact.contains(&hash) {
                return true;
            }
        }
        
        // Check bloom
        let domain_filter = self.domain_filter.read().unwrap();
        domain_filter.check(normalized.as_bytes())
    }

    /// Get current statistics
    pub fn stats(&self) -> (u64, u64, u64) {
        use std::sync::atomic::Ordering;
        (
            self.stats.checks.load(Ordering::Relaxed),
            self.stats.blocked.load(Ordering::Relaxed),
            self.stats.allowed.load(Ordering::Relaxed),
        )
    }

    /// Get memory usage in bytes
    pub fn memory_usage(&self) -> usize {
        let url_filter = self.url_filter.read().unwrap();
        let domain_filter = self.domain_filter.read().unwrap();
        let exact = self.exact_blocks.read().unwrap();
        
        url_filter.bitmap().len() / 8 +
        domain_filter.bitmap().len() / 8 +
        exact.len() * 8 // u64 hash per entry
    }

    /// Clear all filters
    pub fn clear(&self) {
        *self.url_filter.write().unwrap() = Bloom::new_for_fp_rate(
            self.config.expected_items,
            self.config.false_positive_rate,
        );
        *self.domain_filter.write().unwrap() = Bloom::new_for_fp_rate(
            self.config.expected_items / 10,
            self.config.false_positive_rate,
        );
        self.exact_blocks.write().unwrap().clear();
        
        info!("Bloom filters cleared");
    }
}

impl Default for UrlBloomFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Normalize a domain for consistent matching
#[inline]
fn normalize_domain(domain: &str) -> String {
    domain
        .trim()
        .to_lowercase()
        .trim_start_matches("www.")
        .to_string()
}

/// Normalize a URL for consistent matching
#[inline]
fn normalize_url(url: &str) -> String {
    url.trim()
        .to_lowercase()
        .trim_start_matches("http://")
        .trim_start_matches("https://")
        .trim_start_matches("www.")
        .trim_end_matches('/')
        .to_string()
}

/// Extract domain from URL
#[inline]
fn extract_domain(url: &str) -> &str {
    let url = url
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    
    url.split('/').next().unwrap_or(url)
        .split(':').next().unwrap_or(url)
}

/// Hash a string using xxHash3 (very fast, good distribution)
#[inline]
fn hash_string(s: &str) -> u64 {
    xxh3_64(s.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bloom_filter_basic() {
        let filter = UrlBloomFilter::new();
        
        // Add some blocked domains
        filter.add_blocked_domain("doubleclick.net");
        filter.add_blocked_domain("googlesyndication.com");
        filter.add_blocked_domain("facebook.com/tr");
        
        // Should block these
        assert!(filter.should_block("https://doubleclick.net/ads/something"));
        assert!(filter.is_domain_blocked("doubleclick.net"));
        assert!(filter.is_domain_blocked("www.doubleclick.net"));
        
        // Should NOT block these (different domains)
        assert!(!filter.should_block("https://google.com/search"));
        assert!(!filter.is_domain_blocked("example.com"));
    }

    #[test]
    fn test_batch_add() {
        let filter = UrlBloomFilter::new();
        
        let domains = vec![
            "tracker1.com",
            "tracker2.com", 
            "ads.example.com",
            "analytics.bad.com",
        ];
        
        filter.add_blocked_domains(&domains);
        
        for domain in &domains {
            assert!(filter.is_domain_blocked(domain), "Should block {}", domain);
        }
    }

    #[test]
    fn test_normalization() {
        assert_eq!(normalize_domain("WWW.EXAMPLE.COM"), "example.com");
        assert_eq!(normalize_domain("  example.com  "), "example.com");
        
        assert_eq!(
            normalize_url("https://www.Example.COM/path/"),
            "example.com/path"
        );
    }

    #[test]
    fn test_domain_extraction() {
        assert_eq!(extract_domain("https://example.com/path"), "example.com");
        assert_eq!(extract_domain("http://sub.domain.com:8080/x"), "sub.domain.com");
        assert_eq!(extract_domain("example.com"), "example.com");
    }

    #[test]
    fn test_memory_usage() {
        let filter = UrlBloomFilter::new();
        let mem = filter.memory_usage();
        
        // Should be reasonable (~150-200 KB for 100k items)
        assert!(mem < 500 * 1024, "Memory usage too high: {} bytes", mem);
        println!("Bloom filter memory: {} KB", mem / 1024);
    }

    #[test]
    fn test_stats() {
        let filter = UrlBloomFilter::new();
        filter.add_blocked_domain("blocked.com");
        
        filter.should_block("https://blocked.com/ad");
        filter.should_block("https://allowed.com/page");
        filter.should_block("https://blocked.com/another");
        
        let (checks, blocked, allowed) = filter.stats();
        assert_eq!(checks, 3);
        assert_eq!(blocked, 2);
        assert_eq!(allowed, 1);
    }
}
