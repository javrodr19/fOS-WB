//! File Backend Server
//!
//! High-performance file management with zero-copy streaming.

mod auth;
mod handlers;
mod storage;
mod thumbnail;

use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::auth::{auth_middleware, cleanup_expired_sessions, create_session_store};
use crate::handlers::{
    health_handler, login_handler, stream_handler, thumbnail_handler, upload_handler, AppState,
};
use crate::storage::Storage;
use crate::thumbnail::ThumbnailConfig;

/// Server configuration
const BIND_ADDRESS: &str = "0.0.0.0:3000";
const UPLOAD_DIR: &str = "./uploads";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "file_backend=debug,tower_http=debug".into()),
        )
        .init();

    // Initialize storage
    let storage = Storage::new(UPLOAD_DIR);
    storage.init().await?;
    tracing::info!("Storage initialized at: {}", UPLOAD_DIR);

    // Create session store
    let sessions = create_session_store();

    // Create shared application state
    let state = Arc::new(AppState {
        storage,
        sessions: sessions.clone(),
        thumbnail_config: ThumbnailConfig::default(),
    });

    // Spawn session cleanup task
    tokio::spawn(cleanup_expired_sessions(sessions));

    // Build router
    let app = Router::new()
        // Public routes
        .route("/health", get(health_handler))
        .route("/auth/login", post(login_handler))
        // Protected routes (require authentication)
        .route("/upload", post(upload_handler))
        .route("/stream/{id}", get(stream_handler))
        .route("/thumbnail/{id}", get(thumbnail_handler))
        // Apply authentication middleware to protected routes
        .layer(middleware::from_fn(auth_middleware))
        // Apply CORS and tracing to all routes
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    // Start server
    let listener = tokio::net::TcpListener::bind(BIND_ADDRESS).await?;
    tracing::info!("Server listening on http://{}", BIND_ADDRESS);

    axum::serve(listener, app).await?;

    Ok(())
}
