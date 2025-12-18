//! Request Interceptor
//!
//! Intercepts requests BEFORE DNS lookup to save resources.
//! This is the key to "kernel-level" filtering - we drop requests
//! at the earliest possible point.
//!
//! Flow:
//! 1. Request comes in with URL
//! 2. Bloom filter check (< 1μs)
//! 3. If blocked → return Blocked immediately (no DNS, no socket)
//! 4. If allowed → proceed to DNS resolution

use crate::bloom_filter::UrlBloomFilter;
use crate::filter_list::FilterList;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, trace, warn};

/// Result of interception check
#[derive(Debug, Clone)]
pub enum InterceptResult {
    /// Request is allowed to proceed
    Allow,
    /// Request is blocked
    Blocked {
        reason: BlockReason,
        /// Time spent in filter check (microseconds)
        check_time_us: u64,
    },
}

/// Reason why a request was blocked
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BlockReason {
    /// Matched a known tracking domain
    TrackingDomain(String),
    /// Matched a known ad domain
    AdDomain(String),
    /// Matched a URL pattern
    UrlPattern(String),
    /// Matched in bloom filter (may be false positive)
    BloomFilterMatch,
    /// Resource type not allowed
    BlockedResourceType(ResourceType),
    /// User-defined rule
    CustomRule(String),
}

impl std::fmt::Display for BlockReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TrackingDomain(d) => write!(f, "Tracking domain: {}", d),
            Self::AdDomain(d) => write!(f, "Ad domain: {}", d),
            Self::UrlPattern(p) => write!(f, "URL pattern: {}", p),
            Self::BloomFilterMatch => write!(f, "Bloom filter match"),
            Self::BlockedResourceType(t) => write!(f, "Blocked resource type: {:?}", t),
            Self::CustomRule(r) => write!(f, "Custom rule: {}", r),
        }
    }
}

/// Type of resource being requested
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceType {
    /// Main document
    Document,
    /// CSS stylesheet
    Stylesheet,
    /// JavaScript
    Script,
    /// Image
    Image,
    /// Font
    Font,
    /// XHR/Fetch request
    XmlHttpRequest,
    /// WebSocket
    WebSocket,
    /// Media (video/audio)
    Media,
    /// Other/Unknown
    Other,
}

impl ResourceType {
    /// Parse from Accept header or file extension
    pub fn from_accept_or_path(accept: Option<&str>, path: &str) -> Self {
        // Check file extension first
        if path.ends_with(".js") { return Self::Script; }
        if path.ends_with(".css") { return Self::Stylesheet; }
        if path.ends_with(".woff") || path.ends_with(".woff2") || path.ends_with(".ttf") {
            return Self::Font;
        }
        if path.ends_with(".png") || path.ends_with(".jpg") || 
           path.ends_with(".jpeg") || path.ends_with(".gif") ||
           path.ends_with(".webp") || path.ends_with(".svg") {
            return Self::Image;
        }
        if path.ends_with(".mp4") || path.ends_with(".webm") || 
           path.ends_with(".mp3") || path.ends_with(".ogg") {
            return Self::Media;
        }

        // Check Accept header
        if let Some(accept) = accept {
            if accept.contains("text/html") { return Self::Document; }
            if accept.contains("text/css") { return Self::Stylesheet; }
            if accept.contains("javascript") { return Self::Script; }
            if accept.contains("image/") { return Self::Image; }
            if accept.contains("font/") { return Self::Font; }
        }

        Self::Other
    }
}

/// Request interceptor that blocks requests before DNS lookup
pub struct RequestInterceptor {
    /// Bloom filter for fast URL checks
    bloom: Arc<UrlBloomFilter>,
    /// Optional detailed filter list
    filter_list: Option<Arc<FilterList>>,
    /// Whether to block scripts
    block_scripts: bool,
    /// Whether to block images (extreme mode)
    block_images: bool,
    /// Statistics
    stats: InterceptorStats,
}

/// Interception statistics
#[derive(Debug, Default)]
pub struct InterceptorStats {
    pub total_requests: std::sync::atomic::AtomicU64,
    pub blocked_requests: std::sync::atomic::AtomicU64,
    pub total_check_time_ns: std::sync::atomic::AtomicU64,
}

impl RequestInterceptor {
    /// Create a new interceptor with a bloom filter
    pub fn new(bloom: Arc<UrlBloomFilter>) -> Self {
        Self {
            bloom,
            filter_list: None,
            block_scripts: false,
            block_images: false,
            stats: InterceptorStats::default(),
        }
    }

    /// Create with both bloom filter and detailed filter list
    pub fn with_filter_list(bloom: Arc<UrlBloomFilter>, filter_list: Arc<FilterList>) -> Self {
        Self {
            bloom,
            filter_list: Some(filter_list),
            block_scripts: false,
            block_images: false,
            stats: InterceptorStats::default(),
        }
    }

    /// Enable script blocking
    pub fn block_scripts(&mut self, enabled: bool) {
        self.block_scripts = enabled;
    }

    /// Enable image blocking (extreme mode)
    pub fn block_images(&mut self, enabled: bool) {
        self.block_images = enabled;
    }

    /// Check if a request should be intercepted (blocked)
    ///
    /// This is the HOT PATH - must be extremely fast.
    /// Target: < 1μs for bloom filter check
    #[inline]
    pub fn check(&self, url: &str, resource_type: ResourceType) -> InterceptResult {
        use std::sync::atomic::Ordering;
        
        let start = Instant::now();
        self.stats.total_requests.fetch_add(1, Ordering::Relaxed);

        // 1. Check resource type blocking (fastest check)
        if self.block_scripts && resource_type == ResourceType::Script {
            let elapsed = start.elapsed().as_nanos() as u64;
            self.stats.total_check_time_ns.fetch_add(elapsed, Ordering::Relaxed);
            self.stats.blocked_requests.fetch_add(1, Ordering::Relaxed);
            return InterceptResult::Blocked {
                reason: BlockReason::BlockedResourceType(ResourceType::Script),
                check_time_us: elapsed / 1000,
            };
        }

        if self.block_images && resource_type == ResourceType::Image {
            let elapsed = start.elapsed().as_nanos() as u64;
            self.stats.total_check_time_ns.fetch_add(elapsed, Ordering::Relaxed);
            self.stats.blocked_requests.fetch_add(1, Ordering::Relaxed);
            return InterceptResult::Blocked {
                reason: BlockReason::BlockedResourceType(ResourceType::Image),
                check_time_us: elapsed / 1000,
            };
        }

        // 2. Bloom filter check (< 1μs)
        if self.bloom.should_block(url) {
            let elapsed = start.elapsed().as_nanos() as u64;
            self.stats.total_check_time_ns.fetch_add(elapsed, Ordering::Relaxed);
            self.stats.blocked_requests.fetch_add(1, Ordering::Relaxed);
            
            trace!("Bloom filter blocked: {} ({} ns)", url, elapsed);
            
            return InterceptResult::Blocked {
                reason: BlockReason::BloomFilterMatch,
                check_time_us: elapsed / 1000,
            };
        }

        // 3. Detailed filter list check (if available, slower)
        if let Some(ref filter_list) = self.filter_list {
            let domain = extract_domain(url);
            if filter_list.should_block(url, domain) {
                let elapsed = start.elapsed().as_nanos() as u64;
                self.stats.total_check_time_ns.fetch_add(elapsed, Ordering::Relaxed);
                self.stats.blocked_requests.fetch_add(1, Ordering::Relaxed);
                
                debug!("Filter list blocked: {} ({} ns)", url, elapsed);
                
                return InterceptResult::Blocked {
                    reason: BlockReason::UrlPattern(domain.to_string()),
                    check_time_us: elapsed / 1000,
                };
            }
        }

        let elapsed = start.elapsed().as_nanos() as u64;
        self.stats.total_check_time_ns.fetch_add(elapsed, Ordering::Relaxed);
        
        InterceptResult::Allow
    }

    /// Quick domain-only check (even faster, for DNS prefetch blocking)
    #[inline]
    pub fn check_domain(&self, domain: &str) -> bool {
        self.bloom.is_domain_blocked(domain)
    }

    /// Get statistics
    pub fn stats(&self) -> (u64, u64, u64) {
        use std::sync::atomic::Ordering;
        (
            self.stats.total_requests.load(Ordering::Relaxed),
            self.stats.blocked_requests.load(Ordering::Relaxed),
            self.stats.total_check_time_ns.load(Ordering::Relaxed),
        )
    }

    /// Get average check time in nanoseconds
    pub fn avg_check_time_ns(&self) -> u64 {
        use std::sync::atomic::Ordering;
        let total = self.stats.total_requests.load(Ordering::Relaxed);
        if total == 0 {
            return 0;
        }
        self.stats.total_check_time_ns.load(Ordering::Relaxed) / total
    }
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

/// Pre-DNS interceptor that blocks requests before any network activity
pub struct PreDnsInterceptor {
    interceptor: Arc<RequestInterceptor>,
}

impl PreDnsInterceptor {
    pub fn new(interceptor: Arc<RequestInterceptor>) -> Self {
        Self { interceptor }
    }

    /// Process a request before DNS lookup
    ///
    /// Returns None if blocked, Some(url) if allowed
    pub fn process(&self, url: &str, resource_type: ResourceType) -> Option<String> {
        match self.interceptor.check(url, resource_type) {
            InterceptResult::Allow => Some(url.to_string()),
            InterceptResult::Blocked { reason, check_time_us } => {
                debug!(
                    "Pre-DNS block: {} ({}μs) - {}",
                    url, check_time_us, reason
                );
                None
            }
        }
    }

    /// Check if domain should be blocked (for DNS prefetch)
    pub fn should_block_domain(&self, domain: &str) -> bool {
        self.interceptor.check_domain(domain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_interceptor() -> RequestInterceptor {
        let bloom = Arc::new(UrlBloomFilter::new());
        bloom.add_blocked_domain("tracker.com");
        bloom.add_blocked_domain("ads.example.com");
        bloom.add_blocked_domain("doubleclick.net");
        
        RequestInterceptor::new(bloom)
    }

    #[test]
    fn test_block_tracking_domain() {
        let interceptor = create_test_interceptor();
        
        let result = interceptor.check(
            "https://tracker.com/pixel.gif",
            ResourceType::Image
        );
        
        assert!(matches!(result, InterceptResult::Blocked { .. }));
    }

    #[test]
    fn test_allow_normal_domain() {
        let interceptor = create_test_interceptor();
        
        let result = interceptor.check(
            "https://example.com/page.html",
            ResourceType::Document
        );
        
        assert!(matches!(result, InterceptResult::Allow));
    }

    #[test]
    fn test_block_scripts_mode() {
        let bloom = Arc::new(UrlBloomFilter::new());
        let mut interceptor = RequestInterceptor::new(bloom);
        interceptor.block_scripts(true);
        
        let result = interceptor.check(
            "https://example.com/app.js",
            ResourceType::Script
        );
        
        assert!(matches!(
            result,
            InterceptResult::Blocked { reason: BlockReason::BlockedResourceType(ResourceType::Script), .. }
        ));
    }

    #[test]
    fn test_check_performance() {
        let interceptor = create_test_interceptor();
        
        // Warm up
        for _ in 0..1000 {
            interceptor.check("https://example.com/page", ResourceType::Document);
        }
        
        let start = Instant::now();
        const ITERATIONS: usize = 10000;
        
        for _ in 0..ITERATIONS {
            interceptor.check("https://example.com/page", ResourceType::Document);
        }
        
        let elapsed = start.elapsed();
        let per_check_ns = elapsed.as_nanos() / ITERATIONS as u128;
        
        println!("Per-check time: {} ns", per_check_ns);
        
        // Should be under 10μs (10,000 ns) per check
        assert!(per_check_ns < 10_000, "Check too slow: {} ns", per_check_ns);
    }

    #[test]
    fn test_resource_type_detection() {
        assert_eq!(
            ResourceType::from_accept_or_path(None, "/script.js"),
            ResourceType::Script
        );
        assert_eq!(
            ResourceType::from_accept_or_path(None, "/style.css"),
            ResourceType::Stylesheet
        );
        assert_eq!(
            ResourceType::from_accept_or_path(None, "/image.png"),
            ResourceType::Image
        );
        assert_eq!(
            ResourceType::from_accept_or_path(Some("text/html"), "/page"),
            ResourceType::Document
        );
    }

    #[test]
    fn test_pre_dns_interceptor() {
        let bloom = Arc::new(UrlBloomFilter::new());
        bloom.add_blocked_domain("blocked.com");
        
        let interceptor = Arc::new(RequestInterceptor::new(bloom));
        let pre_dns = PreDnsInterceptor::new(interceptor);
        
        // Should block
        assert!(pre_dns.process("https://blocked.com/track", ResourceType::Script).is_none());
        assert!(pre_dns.should_block_domain("blocked.com"));
        
        // Should allow
        assert!(pre_dns.process("https://allowed.com/page", ResourceType::Document).is_some());
        assert!(!pre_dns.should_block_domain("allowed.com"));
    }
}
