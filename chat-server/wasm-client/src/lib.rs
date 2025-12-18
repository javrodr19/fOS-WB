//! WASM Client for Chat Protocol
//!
//! Decodes Protobuf messages off the main JS thread.

use prost::Message;
use wasm_bindgen::prelude::*;

/// Generated Protobuf types
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/chat.rs"));
}

use proto::{client_message, server_message, ClientMessage, ServerMessage};

/// Decode a server message from binary data
/// Returns JSON string for JS consumption
#[wasm_bindgen]
pub fn decode_server_message(data: &[u8]) -> Result<JsValue, JsValue> {
    let msg = ServerMessage::decode(data)
        .map_err(|e| JsValue::from_str(&format!("Decode error: {}", e)))?;
    
    // Convert to JS-friendly format
    let result = match msg.payload {
        Some(server_message::Payload::Chat(chat)) => {
            js_sys::Object::new().tap(|obj| {
                js_sys::Reflect::set(obj, &"type".into(), &"chat".into()).unwrap();
                js_sys::Reflect::set(obj, &"messageId".into(), &chat.message_id.into()).unwrap();
                js_sys::Reflect::set(obj, &"roomId".into(), &chat.room_id.into()).unwrap();
                js_sys::Reflect::set(obj, &"sender".into(), &chat.sender.into()).unwrap();
                js_sys::Reflect::set(obj, &"content".into(), &chat.content.into()).unwrap();
                js_sys::Reflect::set(obj, &"timestamp".into(), &(chat.timestamp as f64).into()).unwrap();
            })
        }
        Some(server_message::Payload::UserJoined(event)) => {
            js_sys::Object::new().tap(|obj| {
                js_sys::Reflect::set(obj, &"type".into(), &"userJoined".into()).unwrap();
                js_sys::Reflect::set(obj, &"roomId".into(), &event.room_id.into()).unwrap();
                js_sys::Reflect::set(obj, &"username".into(), &event.username.into()).unwrap();
                js_sys::Reflect::set(obj, &"timestamp".into(), &(event.timestamp as f64).into()).unwrap();
            })
        }
        Some(server_message::Payload::UserLeft(event)) => {
            js_sys::Object::new().tap(|obj| {
                js_sys::Reflect::set(obj, &"type".into(), &"userLeft".into()).unwrap();
                js_sys::Reflect::set(obj, &"roomId".into(), &event.room_id.into()).unwrap();
                js_sys::Reflect::set(obj, &"username".into(), &event.username.into()).unwrap();
                js_sys::Reflect::set(obj, &"timestamp".into(), &(event.timestamp as f64).into()).unwrap();
            })
        }
        Some(server_message::Payload::RoomInfo(info)) => {
            let users = js_sys::Array::new();
            for user in info.users {
                users.push(&user.into());
            }
            js_sys::Object::new().tap(|obj| {
                js_sys::Reflect::set(obj, &"type".into(), &"roomInfo".into()).unwrap();
                js_sys::Reflect::set(obj, &"roomId".into(), &info.room_id.into()).unwrap();
                js_sys::Reflect::set(obj, &"users".into(), &users).unwrap();
                js_sys::Reflect::set(obj, &"userCount".into(), &(info.user_count as f64).into()).unwrap();
            })
        }
        Some(server_message::Payload::Error(err)) => {
            js_sys::Object::new().tap(|obj| {
                js_sys::Reflect::set(obj, &"type".into(), &"error".into()).unwrap();
                js_sys::Reflect::set(obj, &"code".into(), &(err.code as f64).into()).unwrap();
                js_sys::Reflect::set(obj, &"message".into(), &err.message.into()).unwrap();
            })
        }
        Some(server_message::Payload::Pong(pong)) => {
            js_sys::Object::new().tap(|obj| {
                js_sys::Reflect::set(obj, &"type".into(), &"pong".into()).unwrap();
                js_sys::Reflect::set(obj, &"timestamp".into(), &(pong.timestamp as f64).into()).unwrap();
            })
        }
        None => {
            return Err(JsValue::from_str("Empty message"));
        }
    };
    
    Ok(result.into())
}

/// Encode a join room message
#[wasm_bindgen]
pub fn encode_join(room_id: &str, username: &str) -> Vec<u8> {
    let msg = ClientMessage {
        payload: Some(client_message::Payload::Join(proto::JoinRoom {
            room_id: room_id.to_string(),
            username: username.to_string(),
        })),
    };
    msg.encode_to_vec()
}

/// Encode a send message
#[wasm_bindgen]
pub fn encode_send(room_id: &str, content: &str) -> Vec<u8> {
    let msg = ClientMessage {
        payload: Some(client_message::Payload::Send(proto::SendMessage {
            room_id: room_id.to_string(),
            content: content.to_string(),
        })),
    };
    msg.encode_to_vec()
}

/// Encode a leave room message
#[wasm_bindgen]
pub fn encode_leave(room_id: &str) -> Vec<u8> {
    let msg = ClientMessage {
        payload: Some(client_message::Payload::Leave(proto::LeaveRoom {
            room_id: room_id.to_string(),
        })),
    };
    msg.encode_to_vec()
}

/// Encode a ping message
#[wasm_bindgen]
pub fn encode_ping(timestamp: f64) -> Vec<u8> {
    let msg = ClientMessage {
        payload: Some(client_message::Payload::Ping(proto::Ping {
            timestamp: timestamp as u64,
        })),
    };
    msg.encode_to_vec()
}

/// Helper trait for fluent object building
trait Tap: Sized {
    fn tap<F: FnOnce(&Self)>(self, f: F) -> Self {
        f(&self);
        self
    }
}

impl Tap for js_sys::Object {}
