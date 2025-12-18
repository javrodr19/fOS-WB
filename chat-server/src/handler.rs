//! WebSocket handler
//!
//! Handles client connections with binary Protobuf frames.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use prost::Message;
use tokio::net::TcpStream;
use tokio_tungstenite::{accept_async, tungstenite::Message as WsMessage};
use uuid::Uuid;

use crate::actor::{RoomCommand, RoomManager};
use crate::proto::{
    client_message, server_message, ClientMessage, Error, Pong, RoomInfo, ServerMessage,
};

/// Handle a WebSocket connection
pub async fn handle_connection(stream: TcpStream, room_manager: Arc<RoomManager>) {
    let ws_stream = match accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            tracing::error!("WebSocket handshake failed: {}", e);
            return;
        }
    };

    let (mut write, mut read) = ws_stream.split();
    let user_id = Uuid::new_v4().to_string();
    let mut current_room: Option<String> = None;
    let mut broadcast_rx: Option<tokio::sync::broadcast::Receiver<ServerMessage>> = None;

    tracing::info!("Client connected: {}", user_id);

    loop {
        tokio::select! {
            // Handle incoming messages from client
            msg = read.next() => {
                match msg {
                    Some(Ok(WsMessage::Binary(data))) => {
                        // Decode Protobuf message
                        match ClientMessage::decode(data.as_ref()) {
                            Ok(client_msg) => {
                                if let Some(payload) = client_msg.payload {
                                    match payload {
                                        client_message::Payload::Join(join) => {
                                            // Leave current room if any
                                            if let Some(room_id) = &current_room {
                                                let handle = room_manager.get_or_create_room(room_id).await;
                                                handle.send(RoomCommand::Leave {
                                                    user_id: user_id.clone(),
                                                }).await;
                                            }
                                            
                                            // Join new room
                                            let handle = room_manager.get_or_create_room(&join.room_id).await;
                                            broadcast_rx = Some(handle.subscribe());
                                            handle.send(RoomCommand::Join {
                                                user_id: user_id.clone(),
                                                username: join.username,
                                            }).await;
                                            current_room = Some(join.room_id.clone());
                                            
                                            // Send room info
                                            let (tx, mut rx) = tokio::sync::mpsc::channel(1);
                                            handle.send(RoomCommand::GetInfo { response_tx: tx }).await;
                                            if let Some(info) = rx.recv().await {
                                                let response = ServerMessage {
                                                    payload: Some(server_message::Payload::RoomInfo(info)),
                                                };
                                                let encoded = response.encode_to_vec();
                                                let _ = write.send(WsMessage::Binary(encoded.into())).await;
                                            }
                                        }
                                        client_message::Payload::Leave(_leave) => {
                                            if let Some(room_id) = &current_room {
                                                let handle = room_manager.get_or_create_room(room_id).await;
                                                handle.send(RoomCommand::Leave {
                                                    user_id: user_id.clone(),
                                                }).await;
                                            }
                                            current_room = None;
                                            broadcast_rx = None;
                                        }
                                        client_message::Payload::Send(send) => {
                                            if let Some(room_id) = &current_room {
                                                if room_id == &send.room_id {
                                                    let handle = room_manager.get_or_create_room(room_id).await;
                                                    handle.send(RoomCommand::Message {
                                                        user_id: user_id.clone(),
                                                        content: send.content,
                                                    }).await;
                                                }
                                            }
                                        }
                                        client_message::Payload::Ping(ping) => {
                                            let response = ServerMessage {
                                                payload: Some(server_message::Payload::Pong(Pong {
                                                    timestamp: ping.timestamp,
                                                })),
                                            };
                                            let encoded = response.encode_to_vec();
                                            let _ = write.send(WsMessage::Binary(encoded.into())).await;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to decode message: {}", e);
                                let error = ServerMessage {
                                    payload: Some(server_message::Payload::Error(Error {
                                        code: 400,
                                        message: "Invalid message format".to_string(),
                                    })),
                                };
                                let encoded = error.encode_to_vec();
                                let _ = write.send(WsMessage::Binary(encoded.into())).await;
                            }
                        }
                    }
                    Some(Ok(WsMessage::Close(_))) | None => {
                        break;
                    }
                    Some(Err(e)) => {
                        tracing::error!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
            
            // Handle broadcasts from room
            broadcast = async {
                if let Some(ref mut rx) = broadcast_rx {
                    rx.recv().await.ok()
                } else {
                    std::future::pending::<Option<ServerMessage>>().await
                }
            } => {
                if let Some(msg) = broadcast {
                    let encoded = msg.encode_to_vec();
                    if write.send(WsMessage::Binary(encoded.into())).await.is_err() {
                        break;
                    }
                }
            }
        }
    }

    // Clean up on disconnect
    if let Some(room_id) = current_room {
        let handle = room_manager.get_or_create_room(&room_id).await;
        handle
            .send(RoomCommand::Leave {
                user_id: user_id.clone(),
            })
            .await;
    }

    tracing::info!("Client disconnected: {}", user_id);
}
