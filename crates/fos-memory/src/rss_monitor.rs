//! RSS (Resident Set Size) Monitor
//!
//! Monitors process memory usage and triggers hibernation events
//! when memory pressure is detected. This is the core of the
//! "aggressive tab hibernation" system.
//!
//! Memory Pressure Levels:
//! - Low: < 50% of threshold (normal operation)
//! - Medium: 50-80% of threshold (consider hibernating old tabs)
//! - High: 80-100% of threshold (aggressively hibernate)
//! - Critical: > threshold (force hibernate, may OOM soon)

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, RefreshKind, System};
use thiserror::Error;
use tracing::{debug, info, warn};

/// Default RSS threshold (40 MB for sub-50MB target)
const DEFAULT_RSS_THRESHOLD: usize = 40 * 1024 * 1024;

/// How often to poll RSS
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Errors from RSS monitoring
#[derive(Debug, Error)]
pub enum RssError {
    #[error("Failed to get process info")]
    ProcessNotFound,
    
    #[error("Monitor already running")]
    AlreadyRunning,
}

/// Memory pressure levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MemoryPressureLevel {
    /// Under 50% of threshold, normal operation
    Low,
    /// 50-80% of threshold, consider hibernating
    Medium,
    /// 80-100% of threshold, aggressively hibernate
    High,
    /// Over threshold, critical
    Critical,
}

impl MemoryPressureLevel {
    /// Determine pressure level from RSS and threshold
    pub fn from_usage(current_rss: usize, threshold: usize) -> Self {
        let ratio = current_rss as f64 / threshold as f64;
        
        if ratio >= 1.0 {
            Self::Critical
        } else if ratio >= 0.8 {
            Self::High  
        } else if ratio >= 0.5 {
            Self::Medium
        } else {
            Self::Low
        }
    }

    /// Get human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            Self::Low => "Low (normal operation)",
            Self::Medium => "Medium (consider hibernating)",
            Self::High => "High (aggressively hibernate)",
            Self::Critical => "Critical (force hibernate)",
        }
    }
}

/// Configuration for RSS thresholds
#[derive(Debug, Clone)]
pub struct RssThreshold {
    /// Soft threshold - start considering hibernation
    pub soft: usize,
    /// Hard threshold - aggressive hibernation
    pub hard: usize,
    /// Critical threshold - emergency hibernation
    pub critical: usize,
}

impl Default for RssThreshold {
    fn default() -> Self {
        Self {
            soft: DEFAULT_RSS_THRESHOLD,       // 40 MB
            hard: (DEFAULT_RSS_THRESHOLD as f64 * 1.25) as usize, // 50 MB
            critical: (DEFAULT_RSS_THRESHOLD as f64 * 1.5) as usize, // 60 MB
        }
    }
}

/// Callback for when memory pressure changes
pub type PressureCallback = Arc<dyn Fn(MemoryPressureLevel, usize) + Send + Sync>;

/// RSS Monitor - watches process memory and triggers events
pub struct RssMonitor {
    /// RSS threshold configuration
    thresholds: RssThreshold,
    /// Polling interval
    poll_interval: Duration,
    /// Current RSS (atomic for thread-safe reads)
    current_rss: Arc<AtomicUsize>,
    /// Current pressure level (encoded as usize)
    current_pressure: Arc<AtomicUsize>,
    /// Whether the monitor is running
    running: Arc<AtomicBool>,
    /// Callback when pressure changes
    callback: Option<PressureCallback>,
}

impl RssMonitor {
    /// Create a new RSS monitor
    pub fn new(thresholds: RssThreshold) -> Self {
        Self {
            thresholds,
            poll_interval: DEFAULT_POLL_INTERVAL,
            current_rss: Arc::new(AtomicUsize::new(0)),
            current_pressure: Arc::new(AtomicUsize::new(0)),
            running: Arc::new(AtomicBool::new(false)),
            callback: None,
        }
    }

    /// Create with default thresholds
    pub fn with_defaults() -> Self {
        Self::new(RssThreshold::default())
    }

    /// Set callback for pressure changes
    pub fn on_pressure_change<F>(&mut self, callback: F)
    where
        F: Fn(MemoryPressureLevel, usize) + Send + Sync + 'static,
    {
        self.callback = Some(Arc::new(callback));
    }

    /// Set poll interval
    pub fn set_poll_interval(&mut self, interval: Duration) {
        self.poll_interval = interval;
    }

    /// Get current RSS in bytes
    pub fn current_rss(&self) -> usize {
        self.current_rss.load(Ordering::Relaxed)
    }

    /// Get current pressure level
    pub fn current_pressure(&self) -> MemoryPressureLevel {
        match self.current_pressure.load(Ordering::Relaxed) {
            0 => MemoryPressureLevel::Low,
            1 => MemoryPressureLevel::Medium,
            2 => MemoryPressureLevel::High,
            _ => MemoryPressureLevel::Critical,
        }
    }

    /// Check if threshold is exceeded
    pub fn is_over_threshold(&self) -> bool {
        self.current_rss() >= self.thresholds.hard
    }

    /// Get RSS reading synchronously (for one-off checks)
    pub fn read_rss_sync() -> Result<usize, RssError> {
        let mut system = System::new_with_specifics(
            RefreshKind::everything()
        );
        
        let pid = Pid::from_u32(std::process::id());
        system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&[pid]),
            true,
            ProcessRefreshKind::everything()
        );
        
        system
            .process(pid)
            .map(|p| p.memory() as usize)
            .ok_or(RssError::ProcessNotFound)
    }

    /// Start the background monitoring thread
    pub fn start(&self) -> Result<thread::JoinHandle<()>, RssError> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Err(RssError::AlreadyRunning);
        }

        let running = self.running.clone();
        let current_rss = self.current_rss.clone();
        let current_pressure = self.current_pressure.clone();
        let thresholds = self.thresholds.clone();
        let poll_interval = self.poll_interval;
        let callback = self.callback.clone();

        let handle = thread::Builder::new()
            .name("rss-monitor".to_string())
            .spawn(move || {
                info!("RSS monitor started");
                
                let mut system = System::new_with_specifics(
                    RefreshKind::everything()
                );
                let pid = Pid::from_u32(std::process::id());
                let mut last_pressure = MemoryPressureLevel::Low;

                while running.load(Ordering::Relaxed) {
                    // Refresh process info
                    system.refresh_processes_specifics(
                        sysinfo::ProcessesToUpdate::Some(&[pid]),
                        true,
                        ProcessRefreshKind::everything()
                    );

                    if let Some(process) = system.process(pid) {
                        let rss = process.memory() as usize;
                        current_rss.store(rss, Ordering::Relaxed);

                        let pressure = MemoryPressureLevel::from_usage(
                            rss,
                            thresholds.hard
                        );
                        current_pressure.store(pressure_to_usize(pressure), Ordering::Relaxed);

                        // Trigger callback if pressure changed
                        if pressure != last_pressure {
                            info!(
                                "Memory pressure changed: {:?} -> {:?} (RSS: {} bytes)",
                                last_pressure, pressure, rss
                            );
                            
                            if let Some(ref cb) = callback {
                                cb(pressure, rss);
                            }
                            
                            last_pressure = pressure;
                        }

                        if pressure >= MemoryPressureLevel::High {
                            debug!(
                                "High memory pressure: {} bytes / {} threshold",
                                rss, thresholds.hard
                            );
                        }
                    }

                    thread::sleep(poll_interval);
                }

                info!("RSS monitor stopped");
            })
            .expect("Failed to spawn RSS monitor thread");

        Ok(handle)
    }

    /// Stop the monitor
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Check if running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

impl Drop for RssMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

fn pressure_to_usize(pressure: MemoryPressureLevel) -> usize {
    match pressure {
        MemoryPressureLevel::Low => 0,
        MemoryPressureLevel::Medium => 1,
        MemoryPressureLevel::High => 2,
        MemoryPressureLevel::Critical => 3,
    }
}

/// Tab Manager with RSS-based hibernation
/// 
/// This is the main integration point that monitors RSS and triggers
/// SuspendToDisk events when thresholds are reached.
pub struct TabManager {
    /// RSS monitor
    rss_monitor: Arc<RssMonitor>,
    /// Tabs ordered by last access time (oldest first)
    tab_lru: Vec<TabLruEntry>,
    /// Number of tabs currently hibernated
    hibernated_count: usize,
}

/// LRU entry for a tab
#[derive(Debug, Clone)]
pub struct TabLruEntry {
    pub tab_id: u64,
    pub last_accessed: Instant,
    pub memory_usage: usize,
    pub is_hibernated: bool,
    pub is_active: bool,
}

/// Event emitted when hibernation should occur
#[derive(Debug, Clone)]
pub enum HibernationEvent {
    /// Suspend a tab to disk
    SuspendToDisk {
        tab_id: u64,
        reason: HibernationReason,
    },
    /// Restore a tab from disk
    RestoreFromDisk {
        tab_id: u64,
    },
}

/// Reason for hibernation
#[derive(Debug, Clone, Copy)]
pub enum HibernationReason {
    /// Memory pressure exceeded threshold
    MemoryPressure(MemoryPressureLevel),
    /// Tab has been inactive for too long
    Inactivity { idle_secs: u64 },
    /// User explicitly requested hibernation
    UserRequest,
}

impl TabManager {
    /// Create a new tab manager
    pub fn new(rss_monitor: Arc<RssMonitor>) -> Self {
        Self {
            rss_monitor,
            tab_lru: Vec::new(),
            hibernated_count: 0,
        }
    }

    /// Register a new tab
    pub fn register_tab(&mut self, tab_id: u64, memory_usage: usize) {
        self.tab_lru.push(TabLruEntry {
            tab_id,
            last_accessed: Instant::now(),
            memory_usage,
            is_hibernated: false,
            is_active: true,
        });
    }

    /// Mark a tab as accessed (update LRU)
    pub fn touch_tab(&mut self, tab_id: u64) {
        if let Some(entry) = self.tab_lru.iter_mut().find(|e| e.tab_id == tab_id) {
            entry.last_accessed = Instant::now();
        }
        self.sort_by_lru();
    }

    /// Update a tab's memory usage
    pub fn update_memory(&mut self, tab_id: u64, memory_usage: usize) {
        if let Some(entry) = self.tab_lru.iter_mut().find(|e| e.tab_id == tab_id) {
            entry.memory_usage = memory_usage;
        }
    }

    /// Mark a tab as hibernated
    pub fn mark_hibernated(&mut self, tab_id: u64) {
        if let Some(entry) = self.tab_lru.iter_mut().find(|e| e.tab_id == tab_id) {
            if !entry.is_hibernated {
                entry.is_hibernated = true;
                entry.memory_usage = 0; // Hibernated tabs use minimal RAM
                self.hibernated_count += 1;
            }
        }
    }

    /// Mark a tab as restored
    pub fn mark_restored(&mut self, tab_id: u64, memory_usage: usize) {
        if let Some(entry) = self.tab_lru.iter_mut().find(|e| e.tab_id == tab_id) {
            if entry.is_hibernated {
                entry.is_hibernated = false;
                entry.memory_usage = memory_usage;
                entry.last_accessed = Instant::now();
                self.hibernated_count = self.hibernated_count.saturating_sub(1);
            }
        }
    }

    /// Remove a tab
    pub fn remove_tab(&mut self, tab_id: u64) {
        if let Some(pos) = self.tab_lru.iter().position(|e| e.tab_id == tab_id) {
            if self.tab_lru[pos].is_hibernated {
                self.hibernated_count = self.hibernated_count.saturating_sub(1);
            }
            self.tab_lru.remove(pos);
        }
    }

    /// Check memory pressure and return tabs that should be hibernated
    pub fn check_pressure(&mut self) -> Vec<HibernationEvent> {
        let mut events = Vec::new();
        let pressure = self.rss_monitor.current_pressure();
        
        if pressure < MemoryPressureLevel::Medium {
            return events;
        }

        // Sort by LRU to hibernate oldest first
        self.sort_by_lru();

        // Calculate how many tabs to hibernate based on pressure
        let target_hibernate = match pressure {
            MemoryPressureLevel::Low => 0,
            MemoryPressureLevel::Medium => 1,
            MemoryPressureLevel::High => 2,
            MemoryPressureLevel::Critical => {
                // Hibernate all but the active tab
                self.tab_lru.iter().filter(|e| !e.is_active && !e.is_hibernated).count()
            }
        };

        let mut hibernated = 0;
        for entry in &self.tab_lru {
            if hibernated >= target_hibernate {
                break;
            }
            
            // Don't hibernate active tab or already hibernated
            if entry.is_active || entry.is_hibernated {
                continue;
            }

            events.push(HibernationEvent::SuspendToDisk {
                tab_id: entry.tab_id,
                reason: HibernationReason::MemoryPressure(pressure),
            });
            
            hibernated += 1;
        }

        if !events.is_empty() {
            warn!(
                "Memory pressure {:?}: scheduling {} tabs for hibernation",
                pressure, events.len()
            );
        }

        events
    }

    /// Check for inactive tabs that should be hibernated
    pub fn check_inactivity(&self, max_idle: Duration) -> Vec<HibernationEvent> {
        let now = Instant::now();
        let mut events = Vec::new();

        for entry in &self.tab_lru {
            if entry.is_hibernated || entry.is_active {
                continue;
            }

            let idle_duration = now.duration_since(entry.last_accessed);
            if idle_duration > max_idle {
                events.push(HibernationEvent::SuspendToDisk {
                    tab_id: entry.tab_id,
                    reason: HibernationReason::Inactivity {
                        idle_secs: idle_duration.as_secs(),
                    },
                });
            }
        }

        events
    }

    /// Get statistics
    pub fn stats(&self) -> TabManagerStats {
        let total_memory: usize = self.tab_lru.iter()
            .filter(|e| !e.is_hibernated)
            .map(|e| e.memory_usage)
            .sum();

        TabManagerStats {
            total_tabs: self.tab_lru.len(),
            active_tabs: self.tab_lru.iter().filter(|e| !e.is_hibernated).count(),
            hibernated_tabs: self.hibernated_count,
            total_memory,
            current_rss: self.rss_monitor.current_rss(),
            pressure_level: self.rss_monitor.current_pressure(),
        }
    }

    fn sort_by_lru(&mut self) {
        self.tab_lru.sort_by(|a, b| a.last_accessed.cmp(&b.last_accessed));
    }
}

/// Statistics from the tab manager
#[derive(Debug)]
pub struct TabManagerStats {
    pub total_tabs: usize,
    pub active_tabs: usize,
    pub hibernated_tabs: usize,
    pub total_memory: usize,
    pub current_rss: usize,
    pub pressure_level: MemoryPressureLevel,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pressure_levels() {
        let threshold = 1000;
        
        assert_eq!(
            MemoryPressureLevel::from_usage(400, threshold),
            MemoryPressureLevel::Low
        );
        assert_eq!(
            MemoryPressureLevel::from_usage(600, threshold),
            MemoryPressureLevel::Medium
        );
        assert_eq!(
            MemoryPressureLevel::from_usage(900, threshold),
            MemoryPressureLevel::High
        );
        assert_eq!(
            MemoryPressureLevel::from_usage(1100, threshold),
            MemoryPressureLevel::Critical
        );
    }

    #[test]
    fn test_read_rss() {
        // This should succeed on any system
        let rss = RssMonitor::read_rss_sync().unwrap();
        assert!(rss > 0);
        println!("Current RSS: {} bytes ({:.2} MB)", rss, rss as f64 / 1024.0 / 1024.0);
    }

    #[test]
    fn test_tab_manager_lru() {
        let monitor = Arc::new(RssMonitor::with_defaults());
        let mut manager = TabManager::new(monitor);
        
        // Register tabs with different access times
        manager.register_tab(1, 1000);
        thread::sleep(Duration::from_millis(10));
        manager.register_tab(2, 2000);
        thread::sleep(Duration::from_millis(10));
        manager.register_tab(3, 3000);
        
        // Tab 1 should be first (oldest) after sorting
        manager.sort_by_lru();
        assert_eq!(manager.tab_lru[0].tab_id, 1);
        
        // Touch tab 1, now it should be last
        manager.touch_tab(1);
        assert_eq!(manager.tab_lru[2].tab_id, 1);
    }

    #[test]
    fn test_hibernation_events() {
        let monitor = Arc::new(RssMonitor::with_defaults());
        let mut manager = TabManager::new(monitor);
        
        manager.register_tab(1, 1024 * 1024);
        manager.register_tab(2, 2 * 1024 * 1024);
        
        // Mark tabs as not active (not the focused tab)
        for entry in &mut manager.tab_lru {
            entry.is_active = false;
        }
        
        // Simulate inactivity
        thread::sleep(Duration::from_millis(100));
        
        let events = manager.check_inactivity(Duration::from_millis(50));
        assert_eq!(events.len(), 2); // Both tabs should be candidates
        
        // Mark one as hibernated
        manager.mark_hibernated(1);
        let stats = manager.stats();
        assert_eq!(stats.hibernated_tabs, 1);
        assert_eq!(stats.active_tabs, 1);
    }
}
