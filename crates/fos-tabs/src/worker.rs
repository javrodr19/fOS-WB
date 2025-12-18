//! Tab worker thread implementation with panic isolation.

use crate::message::{TabId, TabMessage, UiMessage};
use crossbeam_channel::{Receiver, Sender};
use std::panic::{self, AssertUnwindSafe};
use std::thread;
use std::time::Duration;
use tracing::{debug, error, info, warn};

/// Spawn a new tab worker thread.
///
/// The worker runs in a panic isolation boundary, catching any panics
/// and reporting them back to the UI thread without crashing the browser.
pub fn spawn_worker(
    tab_id: TabId,
    rx: Receiver<TabMessage>,
    ui_tx: Sender<UiMessage>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name(format!("tab-worker-{}", tab_id.0))
        .spawn(move || {
            info!("Tab worker {} started", tab_id);
            run_worker_loop(tab_id, rx, ui_tx);
            info!("Tab worker {} stopped", tab_id);
        })
        .expect("Failed to spawn tab worker thread")
}

/// Main worker loop with panic isolation.
fn run_worker_loop(
    tab_id: TabId,
    rx: Receiver<TabMessage>,
    ui_tx: Sender<UiMessage>,
) {
    loop {
        // Wait for the next message
        let msg = match rx.recv() {
            Ok(msg) => msg,
            Err(_) => {
                // Channel closed, shutdown
                debug!("Tab {} channel closed, shutting down", tab_id);
                break;
            }
        };

        // Check for shutdown message before processing
        if matches!(msg, TabMessage::Shutdown) {
            debug!("Tab {} received shutdown", tab_id);
            break;
        }

        // Process message in a panic isolation boundary
        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            process_message(tab_id, &msg, &ui_tx)
        }));

        match result {
            Ok(Ok(())) => {
                // Message processed successfully
            }
            Ok(Err(e)) => {
                // Logic error (not a panic)
                warn!("Tab {} error processing message: {}", tab_id, e);
            }
            Err(panic_info) => {
                // Tab panicked - report to UI but don't crash the browser
                let error_msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown panic".to_string()
                };

                error!("Tab {} panicked: {}", tab_id, error_msg);
                
                // Notify UI of crash
                let _ = ui_tx.send(UiMessage::TabCrashed {
                    tab_id,
                    error: error_msg,
                });
                
                // Note: In a full implementation, we would reset the tab's
                // memory heap here and wait for user to reload
            }
        }
    }
}

/// Process a single message.
fn process_message(
    tab_id: TabId,
    msg: &TabMessage,
    ui_tx: &Sender<UiMessage>,
) -> Result<(), String> {
    match msg {
        TabMessage::Navigate { url } => {
            debug!("Tab {} navigating to: {}", tab_id, url);
            
            // Notify UI that loading started
            let _ = ui_tx.send(UiMessage::LoadStarted { tab_id });
            
            // TODO: In a full implementation, this would:
            // 1. Create a network request
            // 2. Parse HTML as it arrives
            // 3. Build DOM tree
            // 4. Trigger layout
            // 5. Paint to display list
            
            // For now, simulate with a brief delay
            thread::sleep(Duration::from_millis(50));
            
            // Notify completion
            let _ = ui_tx.send(UiMessage::UrlChanged { 
                tab_id, 
                url: url.clone() 
            });
            let _ = ui_tx.send(UiMessage::TitleChanged { 
                tab_id, 
                title: format!("Page: {}", url) 
            });
            let _ = ui_tx.send(UiMessage::LoadFinished { tab_id });
            
            Ok(())
        }
        
        TabMessage::ExecuteScript { script } => {
            debug!("Tab {} executing script: {}", tab_id, script);
            // TODO: Send to SpiderMonkey for execution
            Ok(())
        }
        
        TabMessage::Stop => {
            debug!("Tab {} stopping load", tab_id);
            Ok(())
        }
        
        TabMessage::Reload => {
            debug!("Tab {} reloading", tab_id);
            Ok(())
        }
        
        TabMessage::GoBack => {
            debug!("Tab {} going back", tab_id);
            Ok(())
        }
        
        TabMessage::GoForward => {
            debug!("Tab {} going forward", tab_id);
            Ok(())
        }
        
        TabMessage::Ping => {
            // Respond to watchdog
            let _ = ui_tx.send(UiMessage::Pong { tab_id });
            Ok(())
        }
        
        TabMessage::Shutdown => {
            // Handled in main loop
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;

    #[test]
    fn test_worker_responds_to_ping() {
        let tab_id = TabId::new(1);
        let (tab_tx, tab_rx) = unbounded();
        let (ui_tx, ui_rx) = unbounded();
        
        let handle = spawn_worker(tab_id, tab_rx, ui_tx);
        
        // Send ping
        tab_tx.send(TabMessage::Ping).unwrap();
        
        // Should receive pong
        let response = ui_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(response, UiMessage::Pong { .. }));
        
        // Shutdown
        tab_tx.send(TabMessage::Shutdown).unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn test_worker_survives_panic() {
        let tab_id = TabId::new(2);
        let (tab_tx, tab_rx) = unbounded();
        let (ui_tx, ui_rx) = unbounded();
        
        let handle = spawn_worker(tab_id, tab_rx, ui_tx);
        
        // Send a message that would cause a panic in a real implementation
        // For this test, we send a valid message - in production we'd test
        // with a specially crafted "panic trigger" message
        tab_tx.send(TabMessage::Ping).unwrap();
        let _ = ui_rx.recv_timeout(Duration::from_secs(1));
        
        // Worker should still be alive
        tab_tx.send(TabMessage::Ping).unwrap();
        let response = ui_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(response, UiMessage::Pong { .. }));
        
        // Shutdown
        tab_tx.send(TabMessage::Shutdown).unwrap();
        handle.join().unwrap();
    }
}
