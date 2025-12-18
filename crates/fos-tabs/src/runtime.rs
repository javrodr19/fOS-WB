//! Browser runtime - coordinates all tabs and threads.

use crate::message::{TabId, TabMessage, UiMessage};
use crate::tab::Tab;
use crate::watchdog::{spawn_watchdog, TabHeartbeats, WatchdogConfig};
use crate::worker::spawn_worker;
use anyhow::Result;
use crossbeam_channel::{unbounded, Receiver, Sender};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use tracing::{debug, info};

/// The browser runtime that manages all tabs and their workers.
pub struct Runtime {
    /// All active tabs
    tabs: HashMap<TabId, Tab>,
    /// Channels to send messages to tab workers
    tab_senders: Arc<Mutex<HashMap<TabId, Sender<TabMessage>>>>,
    /// Channel to receive messages from tab workers
    ui_rx: Receiver<UiMessage>,
    /// Channel for tab workers to send messages to UI
    ui_tx: Sender<UiMessage>,
    /// Worker thread handles
    workers: HashMap<TabId, JoinHandle<()>>,
    /// Heartbeat tracking for watchdog
    heartbeats: Arc<TabHeartbeats>,
    /// Watchdog thread handle
    _watchdog: JoinHandle<()>,
}

impl Runtime {
    /// Create a new browser runtime.
    pub fn new() -> Result<Self> {
        info!("Initializing browser runtime");

        let (ui_tx, ui_rx) = unbounded();
        let tab_senders: Arc<Mutex<HashMap<TabId, Sender<TabMessage>>>> = 
            Arc::new(Mutex::new(HashMap::new()));
        let heartbeats = Arc::new(TabHeartbeats::new());

        // Spawn watchdog thread
        let watchdog = spawn_watchdog(
            WatchdogConfig::default(),
            heartbeats.clone(),
            tab_senders.clone(),
            ui_tx.clone(),
        );

        Ok(Self {
            tabs: HashMap::new(),
            tab_senders,
            ui_rx,
            ui_tx,
            workers: HashMap::new(),
            heartbeats,
            _watchdog: watchdog,
        })
    }

    /// Create a new tab and return its ID.
    pub fn create_tab(&mut self) -> TabId {
        // Create channels for the new tab
        let (tab_tx, tab_rx) = unbounded();
        
        // Create the tab
        let tab = Tab::new(tab_tx.clone());
        let tab_id = tab.id();
        
        // Spawn worker thread
        let worker = spawn_worker(tab_id, tab_rx, self.ui_tx.clone());
        
        // Store everything
        self.tabs.insert(tab_id, tab);
        self.workers.insert(tab_id, worker);
        self.tab_senders.lock().unwrap().insert(tab_id, tab_tx);
        self.heartbeats.record(tab_id);

        info!("Created new tab: {}", tab_id);
        tab_id
    }

    /// Close a tab.
    pub fn close_tab(&mut self, tab_id: TabId) -> bool {
        if let Some(tab) = self.tabs.remove(&tab_id) {
            // Tab will send shutdown message on drop
            drop(tab);
            
            // Clean up sender
            self.tab_senders.lock().unwrap().remove(&tab_id);
            self.heartbeats.remove(tab_id);
            
            // Wait for worker to finish
            if let Some(worker) = self.workers.remove(&tab_id) {
                let _ = worker.join();
            }
            
            info!("Closed tab: {}", tab_id);
            true
        } else {
            false
        }
    }

    /// Get a reference to a tab.
    pub fn get_tab(&self, tab_id: TabId) -> Option<&Tab> {
        self.tabs.get(&tab_id)
    }

    /// Get a mutable reference to a tab.
    pub fn get_tab_mut(&mut self, tab_id: TabId) -> Option<&mut Tab> {
        self.tabs.get_mut(&tab_id)
    }

    /// Navigate a tab to a URL.
    pub fn navigate(&mut self, tab_id: TabId, url: &str) -> Result<()> {
        if let Some(tab) = self.tabs.get_mut(&tab_id) {
            tab.navigate(url)?;
        }
        Ok(())
    }

    /// Get all tab IDs.
    pub fn tab_ids(&self) -> Vec<TabId> {
        self.tabs.keys().copied().collect()
    }

    /// Get the number of open tabs.
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    /// Get total memory usage across all tabs.
    pub fn total_memory(&self) -> usize {
        self.tabs.values().map(|t| t.memory_usage()).sum()
    }

    /// Poll for UI messages (non-blocking).
    pub fn poll_messages(&self) -> Vec<UiMessage> {
        let mut messages = Vec::new();
        while let Ok(msg) = self.ui_rx.try_recv() {
            // Handle heartbeat responses
            if let UiMessage::Pong { tab_id } = &msg {
                self.heartbeats.record(*tab_id);
            }
            messages.push(msg);
        }
        messages
    }

    /// Process UI messages and update tab state.
    pub fn process_messages(&mut self) {
        for msg in self.poll_messages() {
            match msg {
                UiMessage::TitleChanged { tab_id, title } => {
                    if let Some(tab) = self.tabs.get_mut(&tab_id) {
                        tab.set_title(title);
                    }
                }
                UiMessage::UrlChanged { tab_id, url } => {
                    if let Some(tab) = self.tabs.get_mut(&tab_id) {
                        tab.set_url(url);
                    }
                }
                UiMessage::LoadStarted { tab_id } => {
                    if let Some(tab) = self.tabs.get_mut(&tab_id) {
                        tab.set_loading(true);
                    }
                }
                UiMessage::LoadFinished { tab_id } => {
                    if let Some(tab) = self.tabs.get_mut(&tab_id) {
                        tab.set_loading(false);
                    }
                }
                UiMessage::TabCrashed { tab_id, error } => {
                    debug!("Tab {} crashed: {}", tab_id, error);
                    if let Some(tab) = self.tabs.get_mut(&tab_id) {
                        tab.reset_after_crash();
                    }
                }
                _ => {}
            }
        }
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        info!("Shutting down browser runtime");
        
        // Close all tabs
        let tab_ids: Vec<_> = self.tabs.keys().copied().collect();
        for tab_id in tab_ids {
            self.close_tab(tab_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_close_tab() {
        let mut runtime = Runtime::new().unwrap();
        
        let tab_id = runtime.create_tab();
        assert_eq!(runtime.tab_count(), 1);
        
        runtime.close_tab(tab_id);
        assert_eq!(runtime.tab_count(), 0);
    }

    #[test]
    fn test_multiple_tabs() {
        let mut runtime = Runtime::new().unwrap();
        
        let tab1 = runtime.create_tab();
        let tab2 = runtime.create_tab();
        let tab3 = runtime.create_tab();
        
        assert_eq!(runtime.tab_count(), 3);
        
        runtime.close_tab(tab2);
        assert_eq!(runtime.tab_count(), 2);
        
        assert!(runtime.get_tab(tab1).is_some());
        assert!(runtime.get_tab(tab2).is_none());
        assert!(runtime.get_tab(tab3).is_some());
    }
}
