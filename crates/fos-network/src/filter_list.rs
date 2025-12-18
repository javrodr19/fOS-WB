//! Filter List Parser (EasyList Format)
//!
//! Parses filter lists in EasyList/AdBlock Plus format.
//! Supports:
//! - Domain blocking: ||example.com^
//! - URL patterns: /ads/*
//! - Exception rules: @@||allowed.com^
//! - Comments: ! or [Adblock Plus...]

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read};
use thiserror::Error;
use tracing::{debug, info, warn};

/// Errors during filter list parsing
#[derive(Debug, Error)]
pub enum FilterListError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Invalid rule format: {0}")]
    InvalidRule(String),
    
    #[error("Empty filter list")]
    EmptyList,
}

/// Action to take when a rule matches
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterAction {
    /// Block the request
    Block,
    /// Allow the request (exception rule)
    Allow,
}

/// Type of filter rule
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleType {
    /// Block exact domain: ||domain.com^
    DomainBlock,
    /// Block URL pattern: /pattern/
    UrlPattern,
    /// Block requests starting with: |https://
    StartsWith,
    /// Block requests containing: substring
    Contains,
    /// Exception (whitelist)
    Exception,
}

/// A single filter rule
#[derive(Debug, Clone)]
pub struct FilterRule {
    /// Original rule text
    pub raw: String,
    /// Parsed pattern
    pub pattern: String,
    /// Type of rule
    pub rule_type: RuleType,
    /// Action to take
    pub action: FilterAction,
    /// Optional domain restrictions
    pub domains: Option<Vec<String>>,
    /// Resource types this applies to
    pub resource_types: Option<Vec<String>>,
}

impl FilterRule {
    /// Check if this rule matches a URL
    pub fn matches(&self, url: &str, domain: &str) -> bool {
        match self.rule_type {
            RuleType::DomainBlock => {
                let normalized = domain.to_lowercase();
                normalized == self.pattern || 
                normalized.ends_with(&format!(".{}", self.pattern))
            }
            RuleType::UrlPattern => {
                url.to_lowercase().contains(&self.pattern)
            }
            RuleType::StartsWith => {
                url.to_lowercase().starts_with(&self.pattern)
            }
            RuleType::Contains => {
                url.to_lowercase().contains(&self.pattern)
            }
            RuleType::Exception => {
                // Exception rules are handled separately
                false
            }
        }
    }
}

/// A complete filter list
pub struct FilterList {
    /// Name of the filter list
    pub name: String,
    /// All blocking rules
    pub block_rules: Vec<FilterRule>,
    /// Exception (whitelist) rules
    pub exception_rules: Vec<FilterRule>,
    /// Quick lookup set for domain blocks
    pub blocked_domains: HashSet<String>,
    /// Quick lookup set for allowed domains
    pub allowed_domains: HashSet<String>,
}

impl FilterList {
    /// Create an empty filter list
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            block_rules: Vec::new(),
            exception_rules: Vec::new(),
            blocked_domains: HashSet::new(),
            allowed_domains: HashSet::new(),
        }
    }

    /// Parse a filter list from a reader
    pub fn parse<R: Read>(name: &str, reader: R) -> Result<Self, FilterListError> {
        let buf_reader = BufReader::new(reader);
        let mut list = Self::new(name);
        let mut line_count = 0;
        let mut rule_count = 0;

        for line in buf_reader.lines() {
            let line = line?;
            line_count += 1;
            
            // Skip empty lines and comments
            let trimmed = line.trim();
            if trimmed.is_empty() || 
               trimmed.starts_with('!') || 
               trimmed.starts_with('[') {
                continue;
            }

            // Parse the rule
            if let Some(rule) = Self::parse_rule(trimmed) {
                match rule.action {
                    FilterAction::Block => {
                        if let RuleType::DomainBlock = rule.rule_type {
                            list.blocked_domains.insert(rule.pattern.clone());
                        }
                        list.block_rules.push(rule);
                    }
                    FilterAction::Allow => {
                        if let RuleType::DomainBlock = rule.rule_type {
                            list.allowed_domains.insert(rule.pattern.clone());
                        }
                        list.exception_rules.push(rule);
                    }
                }
                rule_count += 1;
            }
        }

        if rule_count == 0 {
            return Err(FilterListError::EmptyList);
        }

        info!(
            "Parsed filter list '{}': {} lines, {} rules ({} block, {} exception), {} domains",
            name,
            line_count,
            rule_count,
            list.block_rules.len(),
            list.exception_rules.len(),
            list.blocked_domains.len()
        );

        Ok(list)
    }

    /// Parse a single rule line
    fn parse_rule(line: &str) -> Option<FilterRule> {
        let trimmed = line.trim();
        
        // Exception rule: @@||domain.com^
        if trimmed.starts_with("@@") {
            let pattern = trimmed.trim_start_matches("@@");
            return Self::parse_pattern(pattern, FilterAction::Allow);
        }

        Self::parse_pattern(trimmed, FilterAction::Block)
    }

    /// Parse the pattern part of a rule
    fn parse_pattern(pattern: &str, action: FilterAction) -> Option<FilterRule> {
        // Domain block: ||domain.com^ or ||domain.com/
        if pattern.starts_with("||") {
            let domain = pattern
                .trim_start_matches("||")
                .trim_end_matches('^')
                .trim_end_matches('/')
                .split('/')
                .next()?
                .split('$')
                .next()?
                .to_lowercase();
            
            if domain.is_empty() {
                return None;
            }

            return Some(FilterRule {
                raw: pattern.to_string(),
                pattern: domain,
                rule_type: if action == FilterAction::Allow {
                    RuleType::Exception
                } else {
                    RuleType::DomainBlock
                },
                action,
                domains: None,
                resource_types: None,
            });
        }

        // Start-of-URL match: |https://
        if pattern.starts_with('|') && !pattern.starts_with("||") {
            let url_start = pattern.trim_start_matches('|');
            return Some(FilterRule {
                raw: pattern.to_string(),
                pattern: url_start.to_lowercase(),
                rule_type: RuleType::StartsWith,
                action,
                domains: None,
                resource_types: None,
            });
        }

        // URL pattern with wildcards or generic
        if pattern.contains('/') || pattern.contains('*') {
            let clean = pattern
                .replace('*', "")
                .trim_end_matches('^')
                .split('$')
                .next()?
                .to_lowercase();
            
            if clean.len() < 3 {
                return None; // Too short, would match too much
            }

            return Some(FilterRule {
                raw: pattern.to_string(),
                pattern: clean,
                rule_type: RuleType::UrlPattern,
                action,
                domains: None,
                resource_types: None,
            });
        }

        // Plain string match (contains)
        if pattern.len() >= 5 && !pattern.contains('#') {
            return Some(FilterRule {
                raw: pattern.to_string(),
                pattern: pattern.to_lowercase(),
                rule_type: RuleType::Contains,
                action,
                domains: None,
                resource_types: None,
            });
        }

        None
    }

    /// Check if a URL should be blocked
    pub fn should_block(&self, url: &str, domain: &str) -> bool {
        let normalized_domain = domain.to_lowercase();
        
        // Check if this exact domain is allowed (exception rule)
        if self.allowed_domains.contains(&normalized_domain) {
            return false;
        }
        
        // Check if domain or any parent is in allowed list
        if self.is_domain_allowed(&normalized_domain) {
            return false;
        }

        // Check exception rules (more specific matches)
        for rule in &self.exception_rules {
            if self.matches_exception(rule, url, &normalized_domain) {
                debug!("URL allowed by exception: {}", url);
                return false;
            }
        }

        // Now check if domain is blocked
        if self.is_domain_blocked(&normalized_domain) {
            return true;
        }

        // Check block rules
        for rule in &self.block_rules {
            if rule.matches(url, domain) {
                debug!("URL blocked by rule '{}': {}", rule.pattern, url);
                return true;
            }
        }

        false
    }

    /// Check if a domain matches an exception rule
    fn matches_exception(&self, rule: &FilterRule, _url: &str, domain: &str) -> bool {
        // Exception matches if domain equals or ends with the exception domain
        domain == rule.pattern || domain.ends_with(&format!(".{}", rule.pattern))
    }

    /// Check if a domain is in the allowed set (including subdomains)
    fn is_domain_allowed(&self, domain: &str) -> bool {
        // Check exact match
        if self.allowed_domains.contains(domain) {
            return true;
        }
        
        // Check if this domain is a subdomain of an allowed domain
        // Note: We do NOT check parent domains here - only exact or subdomain match
        false
    }

    /// Check if a domain is in the blocked set
    fn is_domain_blocked(&self, domain: &str) -> bool {
        if self.blocked_domains.contains(domain) {
            return true;
        }
        
        // Check parent domains
        let parts: Vec<&str> = domain.split('.').collect();
        for i in 1..parts.len() {
            let parent = parts[i..].join(".");
            if self.blocked_domains.contains(&parent) {
                return true;
            }
        }
        
        false
    }

    /// Get all blocked domains
    pub fn blocked_domains(&self) -> &HashSet<String> {
        &self.blocked_domains
    }

    /// Number of rules
    pub fn rule_count(&self) -> usize {
        self.block_rules.len() + self.exception_rules.len()
    }
}

/// Load default tracking domains
pub fn default_tracking_domains() -> Vec<&'static str> {
    vec![
        // Google Ads/Analytics
        "doubleclick.net",
        "googlesyndication.com",
        "googleadservices.com",
        "google-analytics.com",
        "googletagmanager.com",
        "googletagservices.com",
        "googlesyndication.com",
        
        // Facebook
        "facebook.com/tr",
        "connect.facebook.net",
        "pixel.facebook.com",
        
        // Twitter/X
        "ads-twitter.com",
        "analytics.twitter.com",
        
        // Amazon
        "amazon-adsystem.com",
        "assoc-amazon.com",
        
        // Generic trackers
        "adnxs.com",
        "adsrvr.org",
        "criteo.com",
        "criteo.net",
        "outbrain.com",
        "taboola.com",
        "quantserve.com",
        "scorecardresearch.com",
        "hotjar.com",
        "mixpanel.com",
        "segment.io",
        "amplitude.com",
        "branch.io",
        "appsflyer.com",
        "adjust.com",
        "moat.com",
        "chartbeat.com",
        "newrelic.com",
        "bugsnag.com",
        "sentry.io",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_parse_domain_block() {
        let rule = FilterList::parse_rule("||example.com^").unwrap();
        assert_eq!(rule.pattern, "example.com");
        assert!(matches!(rule.rule_type, RuleType::DomainBlock));
        assert_eq!(rule.action, FilterAction::Block);
    }

    #[test]
    fn test_parse_exception() {
        let rule = FilterList::parse_rule("@@||allowed.com^").unwrap();
        assert_eq!(rule.pattern, "allowed.com");
        assert!(matches!(rule.rule_type, RuleType::Exception));
        assert_eq!(rule.action, FilterAction::Allow);
    }

    #[test]
    fn test_parse_url_pattern() {
        let rule = FilterList::parse_rule("/ads/*.js").unwrap();
        assert!(rule.pattern.contains("ads"));
        assert!(matches!(rule.rule_type, RuleType::UrlPattern));
    }

    #[test]
    fn test_filter_list_parse() {
        let content = r#"
! This is a comment
[Adblock Plus 2.0]
||tracker.com^
||ads.example.com^
@@||allowed.example.com^
/advertisements/
"#;
        
        let list = FilterList::parse("test", Cursor::new(content)).unwrap();
        
        assert_eq!(list.block_rules.len(), 3);
        assert_eq!(list.exception_rules.len(), 1);
        assert!(list.blocked_domains.contains("tracker.com"));
    }

    #[test]
    fn test_should_block() {
        let content = r#"
||badsite.com^
||ads.example.com^
@@||good.badsite.com^
"#;
        let list = FilterList::parse("test", Cursor::new(content)).unwrap();
        
        // Should block
        assert!(list.should_block("https://badsite.com/page", "badsite.com"));
        assert!(list.should_block("https://ads.example.com/ad.js", "ads.example.com"));
        
        // Should allow (exception)
        assert!(!list.should_block("https://good.badsite.com/", "good.badsite.com"));
        
        // Should allow (not in list)
        assert!(!list.should_block("https://example.com/page", "example.com"));
    }

    #[test]
    fn test_subdomain_blocking() {
        let content = "||tracker.com^";
        let list = FilterList::parse("test", Cursor::new(content)).unwrap();
        
        // Main domain
        assert!(list.should_block("https://tracker.com/", "tracker.com"));
        
        // Subdomain should also be blocked
        assert!(list.should_block("https://sub.tracker.com/", "sub.tracker.com"));
    }
}
