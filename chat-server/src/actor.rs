//! Actor-based state management
//!
//! Uses message passing via mpsc channels to avoid mutex contention.
//! Each chat room is managed by its own actor task.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, mpsc, RwLock};
use uuid::Uuid;

use crate::proto::{ChatMessage, RoomInfo, ServerMessage, UserJoined, UserLeft};

/// Actor command types
#[derive(Debug, Clone)]
pub enum RoomCommand {
    /// User joins the room
    Join {
        user_id: String,
        username: String,
    },
    /// User leaves the room
    Leave {
        user_id: String,
    },
    /// User sends a message
    Message {
        user_id: String,
        content: String,
    },
    /// Get room info
    GetInfo {
        response_tx: mpsc::Sender<RoomInfo>,
    },
}

/// Room actor state
struct RoomActor {
    room_id: String,
    users: HashMap<String, String>, // user_id -> username
    broadcast_tx: broadcast::Sender<ServerMessage>,
}

impl RoomActor {
    fn new(room_id: String) -> Self {
        let (broadcast_tx, _) = broadcast::channel(1024);
        Self {
            room_id,
            users: HashMap::new(),
            broadcast_tx,
        }
    }
    
    /// Get current timestamp
    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }
    
    /// Handle join command
    fn handle_join(&mut self, user_id: String, username: String) {
        self.users.insert(user_id, username.clone());
        
        // Broadcast user joined event
        let event = ServerMessage {
            payload: Some(crate::proto::server_message::Payload::UserJoined(
                UserJoined {
                    room_id: self.room_id.clone(),
                    username,
                    timestamp: Self::now(),
                },
            )),
        };
        let _ = self.broadcast_tx.send(event);
    }
    
    /// Handle leave command
    fn handle_leave(&mut self, user_id: &str) {
        if let Some(username) = self.users.remove(user_id) {
            let event = ServerMessage {
                payload: Some(crate::proto::server_message::Payload::UserLeft(
                    UserLeft {
                        room_id: self.room_id.clone(),
                        username,
                        timestamp: Self::now(),
                    },
                )),
            };
            let _ = self.broadcast_tx.send(event);
        }
    }
    
    /// Handle message command
    fn handle_message(&mut self, user_id: &str, content: String) {
        if let Some(username) = self.users.get(user_id) {
            let message = ServerMessage {
                payload: Some(crate::proto::server_message::Payload::Chat(
                    ChatMessage {
                        message_id: Uuid::new_v4().to_string(),
                        room_id: self.room_id.clone(),
                        sender: username.clone(),
                        content,
                        timestamp: Self::now(),
                    },
                )),
            };
            let _ = self.broadcast_tx.send(message);
        }
    }
    
    /// Get room info
    fn get_info(&self) -> RoomInfo {
        RoomInfo {
            room_id: self.room_id.clone(),
            users: self.users.values().cloned().collect(),
            user_count: self.users.len() as u32,
        }
    }
    
    /// Subscribe to room broadcasts
    fn subscribe(&self) -> broadcast::Receiver<ServerMessage> {
        self.broadcast_tx.subscribe()
    }
}

/// Room manager - manages all room actors
pub struct RoomManager {
    rooms: RwLock<HashMap<String, RoomHandle>>,
}

/// Handle to communicate with a room actor
#[derive(Clone)]
pub struct RoomHandle {
    command_tx: mpsc::Sender<RoomCommand>,
    broadcast_tx: broadcast::Sender<ServerMessage>,
}

impl RoomHandle {
    /// Send a command to the room actor
    pub async fn send(&self, cmd: RoomCommand) {
        let _ = self.command_tx.send(cmd).await;
    }
    
    /// Subscribe to room broadcasts
    pub fn subscribe(&self) -> broadcast::Receiver<ServerMessage> {
        self.broadcast_tx.subscribe()
    }
}

impl RoomManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            rooms: RwLock::new(HashMap::new()),
        })
    }
    
    /// Get or create a room
    pub async fn get_or_create_room(&self, room_id: &str) -> RoomHandle {
        // Check if room exists
        {
            let rooms = self.rooms.read().await;
            if let Some(handle) = rooms.get(room_id) {
                return handle.clone();
            }
        }
        
        // Create new room
        let (command_tx, mut command_rx) = mpsc::channel::<RoomCommand>(256);
        let mut actor = RoomActor::new(room_id.to_string());
        let broadcast_tx = actor.broadcast_tx.clone();
        
        // Spawn actor task
        tokio::spawn(async move {
            while let Some(cmd) = command_rx.recv().await {
                match cmd {
                    RoomCommand::Join { user_id, username } => {
                        actor.handle_join(user_id, username);
                    }
                    RoomCommand::Leave { user_id } => {
                        actor.handle_leave(&user_id);
                    }
                    RoomCommand::Message { user_id, content } => {
                        actor.handle_message(&user_id, content);
                    }
                    RoomCommand::GetInfo { response_tx } => {
                        let _ = response_tx.send(actor.get_info()).await;
                    }
                }
            }
        });
        
        let handle = RoomHandle {
            command_tx,
            broadcast_tx,
        };
        
        // Store handle
        {
            let mut rooms = self.rooms.write().await;
            rooms.insert(room_id.to_string(), handle.clone());
        }
        
        handle
    }
}

impl Default for RoomManager {
    fn default() -> Self {
        Self {
            rooms: RwLock::new(HashMap::new()),
        }
    }
}
