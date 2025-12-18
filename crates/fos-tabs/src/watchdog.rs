//! Watchdog thread for detecting unresponsive tabs.

use crate::message::{TabId, TabMessage, UiMessage};
use crossbeam_channel::Sender;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

/// Configuration for the watchdog.
#[derive(Debug, Clone)]
pub struct WatchdogConfig {
    /// How often to ping tabs
    pub ping_interval: Duration,
    /// How long to wait before declaring a tab unresponsive
    pub timeout: Duration,
}

impl Default for WatchdogConfig {
    fn default() -> Self {
        Self {
            ping_interval: Duration::from_millis(500),
            timeout: Duration::from_secs(5),
        }
    }
}

/// Tracks the last heartbeat from each tab.
pub struct TabHeartbeats {
    inner: Mutex<HashMap<TabId, Instant>>,
}

impl TabHeartbeats {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Record a heartbeat from a tab.
    pub fn record(&self, tab_id: TabId) {
        let mut map = self.inner.lock().unwrap();
        map.insert(tab_id, Instant::now());
    }

    /// Remove a tab from tracking.
    pub fn remove(&self, tab_id: TabId) {
        let mut map = self.inner.lock().unwrap();
        map.remove(&tab_id);
    }

    /// Check which tabs are unresponsive.
    pub fn check_unresponsive(&self, timeout: Duration) -> Vec<TabId> {
        let map = self.inner.lock().unwrap();
        let now = Instant::now();
        
        map.iter()
            .filter(|(_, last_heartbeat)| now.duration_since(**last_heartbeat) > timeout)
            .map(|(tab_id, _)| *tab_id)
            .collect()
    }

    /// Get all tracked tab IDs.
    #[allow(dead_code)]
    pub fn tab_ids(&self) -> Vec<TabId> {
        let map = self.inner.lock().unwrap();
        map.keys().copied().collect()
    }
}

impl Default for TabHeartbeats {
    fn default() -> Self {
        Self::new()
    }
}

/// Spawn the watchdog thread.
pub fn spawn_watchdog(
    config: WatchdogConfig,
    heartbeats: Arc<TabHeartbeats>,
    tab_senders: Arc<Mutex<HashMap<TabId, Sender<TabMessage>>>>,
    ui_tx: Sender<UiMessage>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("watchdog".to_string())
        .spawn(move || {
            debug!("Watchdog started");
            run_watchdog_loop(config, heartbeats, tab_senders, ui_tx);
            debug!("Watchdog stopped");
        })
        .expect("Failed to spawn watchdog thread")
}

fn run_watchdog_loop(
    config: WatchdogConfig,
    heartbeats: Arc<TabHeartbeats>,
    tab_senders: Arc<Mutex<HashMap<TabId, Sender<TabMessage>>>>,
    ui_tx: Sender<UiMessage>,
) {
    loop {
        thread::sleep(config.ping_interval);

        // Send ping to all tabs
        {
            let senders = tab_senders.lock().unwrap();
            for (tab_id, sender) in senders.iter() {
                if sender.send(TabMessage::Ping).is_err() {
                    debug!("Tab {} channel closed", tab_id);
                }
            }
        }

        // Wait a bit for responses
        thread::sleep(Duration::from_millis(100));

        // Check for unresponsive tabs
        let unresponsive = heartbeats.check_unresponsive(config.timeout);
        for tab_id in unresponsive {
            warn!("Tab {} is unresponsive", tab_id);
            let _ = ui_tx.send(UiMessage::TabUnresponsive { tab_id });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heartbeat_tracking() {
        let heartbeats = TabHeartbeats::new();
        let tab_id = TabId::new(1);
        
        // Initially no tabs should be unresponsive
        assert!(heartbeats.check_unresponsive(Duration::from_secs(1)).is_empty());
        
        // Record a heartbeat
        heartbeats.record(tab_id);
        
        // Should not be unresponsive immediately
        assert!(heartbeats.check_unresponsive(Duration::from_secs(1)).is_empty());
        
        // Wait and check again
        thread::sleep(Duration::from_millis(50));
        let unresponsive = heartbeats.check_unresponsive(Duration::from_millis(10));
        assert_eq!(unresponsive.len(), 1);
        assert_eq!(unresponsive[0], tab_id);
    }
}
