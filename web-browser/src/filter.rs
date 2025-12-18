//! Filter Engine Module
//!
//! High-performance ad and tracker blocking using Aho-Corasick automaton.
//! Designed for zero-allocation during the matching process.

use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use once_cell::sync::Lazy;

/// Default blocklist containing common ad and tracker domains.
/// This is embedded at compile time for zero startup cost.
const DEFAULT_BLOCKLIST: &[&str] = &[
    // Ad networks
    "doubleclick.net",
    "googlesyndication.com",
    "googleadservices.com",
    "google-analytics.com",
    "googletagmanager.com",
    "googletagservices.com",
    "adservice.google.com",
    "pagead2.googlesyndication.com",
    "ads.google.com",
    
    // Facebook trackers
    "facebook.com/tr",
    "connect.facebook.net",
    "pixel.facebook.com",
    
    // Common ad networks
    "adsrvr.org",
    "adnxs.com",
    "rubiconproject.com",
    "pubmatic.com",
    "openx.net",
    "criteo.com",
    "criteo.net",
    "outbrain.com",
    "taboola.com",
    "amazon-adsystem.com",
    
    // Trackers
    "hotjar.com",
    "mixpanel.com",
    "segment.io",
    "segment.com",
    "amplitude.com",
    "fullstory.com",
    "mouseflow.com",
    "crazyegg.com",
    "optimizely.com",
    "quantserve.com",
    "scorecardresearch.com",
    "newrelic.com",
    
    // Ad-related paths
    "/ads/",
    "/adserver/",
    "/tracking/",
    "/tracker/",
    "/pixel/",
    "/beacon/",
    "/analytics/",
    "?utm_",
    "&utm_",
];

/// Global filter engine instance - initialized lazily on first use.
/// Uses Aho-Corasick for O(n) matching regardless of pattern count.
pub static FILTER_ENGINE: Lazy<FilterEngine> = Lazy::new(FilterEngine::new);

/// High-performance filter engine using Aho-Corasick automaton.
/// 
/// The automaton is built once at initialization and provides
/// zero-allocation pattern matching during runtime.
#[derive(Debug)]
pub struct FilterEngine {
    /// The Aho-Corasick automaton for multi-pattern matching
    automaton: AhoCorasick,
    /// Number of patterns loaded
    pattern_count: usize,
    /// Whether filtering is enabled
    enabled: bool,
}

impl FilterEngine {
    /// Create a new filter engine with the default blocklist.
    pub fn new() -> Self {
        Self::with_patterns(DEFAULT_BLOCKLIST)
    }
    
    /// Create a filter engine with custom patterns.
    /// 
    /// # Arguments
    /// * `patterns` - Slice of pattern strings to block
    pub fn with_patterns<I, P>(patterns: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<[u8]>,
    {
        let patterns: Vec<_> = patterns.into_iter().collect();
        let pattern_count = patterns.len();
        
        // Build automaton optimized for leftmost-first matching
        // This is the most efficient for our use case (we only need to know IF there's a match)
        let automaton = AhoCorasickBuilder::new()
            .match_kind(MatchKind::LeftmostFirst)
            // Use DFA for faster matching at cost of more memory
            // For a small blocklist, this is an excellent tradeoff
            .build(&patterns)
            .expect("Failed to build Aho-Corasick automaton");
        
        Self {
            automaton,
            pattern_count,
            enabled: true,
        }
    }
    
    /// Check if a URL should be blocked.
    /// 
    /// This method performs zero heap allocations during matching.
    /// Time complexity: O(n) where n is the length of the URL.
    /// 
    /// # Arguments
    /// * `url` - The URL to check
    /// 
    /// # Returns
    /// `true` if the URL matches any blocked pattern, `false` otherwise.
    #[inline]
    pub fn is_blocked(&self, url: &str) -> bool {
        if !self.enabled {
            return false;
        }
        
        // Zero-allocation search - only checks for existence of match
        self.automaton.is_match(url)
    }
    
    /// Enable or disable the filter engine.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    
    /// Check if the filter engine is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
    
    /// Get the number of patterns loaded.
    pub fn pattern_count(&self) -> usize {
        self.pattern_count
    }
}

impl Default for FilterEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_blocks_ad_domains() {
        let engine = FilterEngine::new();
        
        assert!(engine.is_blocked("https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js"));
        assert!(engine.is_blocked("https://www.google-analytics.com/analytics.js"));
        assert!(engine.is_blocked("https://connect.facebook.net/en_US/fbevents.js"));
    }
    
    #[test]
    fn test_allows_normal_urls() {
        let engine = FilterEngine::new();
        
        assert!(!engine.is_blocked("https://www.google.com"));
        assert!(!engine.is_blocked("https://github.com"));
        assert!(!engine.is_blocked("https://docs.rs"));
    }
    
    #[test]
    fn test_blocks_tracking_paths() {
        let engine = FilterEngine::new();
        
        assert!(engine.is_blocked("https://example.com/ads/banner.js"));
        assert!(engine.is_blocked("https://example.com/tracking/pixel.gif"));
        assert!(engine.is_blocked("https://example.com?utm_source=test"));
    }
    
    #[test]
    fn test_disable_filter() {
        let mut engine = FilterEngine::new();
        
        assert!(engine.is_blocked("https://doubleclick.net/ad.js"));
        
        engine.set_enabled(false);
        assert!(!engine.is_blocked("https://doubleclick.net/ad.js"));
    }
}
