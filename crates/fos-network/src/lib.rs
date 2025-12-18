//! fOS Network Layer
//!
//! Zero-copy networking with integrated content blocking.
//!
//! Architecture:
//! 1. Request comes in → Bloom filter check (< 1μs)
//! 2. If blocked → drop before DNS lookup
//! 3. If allowed → async mio/tokio networking
//! 4. Response streaming with zero-copy buffers

mod bloom_filter;
mod filter_list;
mod interceptor;
mod client;
mod dns;

pub use bloom_filter::{UrlBloomFilter, BloomConfig};
pub use filter_list::{FilterList, FilterRule, FilterAction};
pub use interceptor::{RequestInterceptor, InterceptResult, ResourceType};
pub use client::{HttpClient, HttpClientConfig, Response};
pub use dns::{DnsResolver, DnsConfig};
