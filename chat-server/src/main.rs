//! Chat Server
//!
//! Real-time WebSocket chat with Protobuf and actor-based state.

mod actor;
mod handler;

/// Generated Protobuf types
pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/chat.rs"));
}

use std::sync::Arc;
use tokio::net::TcpListener;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::actor::RoomManager;
use crate::handler::handle_connection;

const BIND_ADDRESS: &str = "0.0.0.0:9000";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "chat_server=debug".into()),
        )
        .init();

    // Create room manager
    let room_manager = RoomManager::new();

    // Start TCP listener
    let listener = TcpListener::bind(BIND_ADDRESS).await?;
    tracing::info!("Chat server listening on ws://{}", BIND_ADDRESS);

    // Accept connections
    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                tracing::debug!("New connection from: {}", addr);
                let manager = Arc::clone(&room_manager);
                tokio::spawn(async move {
                    handle_connection(stream, manager).await;
                });
            }
            Err(e) => {
                tracing::error!("Failed to accept connection: {}", e);
            }
        }
    }
}
