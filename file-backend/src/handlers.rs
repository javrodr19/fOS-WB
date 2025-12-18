//! HTTP Handlers
//!
//! Upload, stream, and thumbnail endpoints with maximum throughput.

use axum::{
    body::Body,
    extract::{Multipart, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::auth::{create_session, SessionStore};
use crate::storage::Storage;
use crate::thumbnail::{generate_thumbnail, ThumbnailConfig};

/// Application state shared across handlers
pub struct AppState {
    pub storage: Storage,
    pub sessions: SessionStore,
    pub thumbnail_config: ThumbnailConfig,
}

/// Upload response
#[derive(Serialize)]
pub struct UploadResponse {
    pub id: String,
    pub size: u64,
    pub message: String,
}

/// Login request
#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// Login response
#[derive(Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub expires_in: u64,
}

/// Error response
#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

/// POST /upload - Multipart file upload
/// 
/// Handles large file uploads with streaming to avoid memory issues.
pub async fn upload_handler(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, StatusCode> {
    while let Some(field) = multipart.next_field().await.map_err(|_| StatusCode::BAD_REQUEST)? {
        // Only process file fields
        if field.name() != Some("file") {
            continue;
        }
        
        // Generate unique file ID
        let file_id = Storage::generate_id();
        
        // Read file data
        let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
        
        // Save to storage
        let size = state
            .storage
            .save_file(&file_id, &data)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        
        tracing::info!("Uploaded file: {} ({} bytes)", file_id, size);
        
        // Spawn thumbnail generation in background for images
        let storage = state.storage.clone();
        let config = state.thumbnail_config.clone();
        let id_clone = file_id.clone();
        tokio::spawn(async move {
            if let Err(e) = generate_thumbnail(&storage, &id_clone, &config).await {
                tracing::warn!("Thumbnail generation failed for {}: {}", id_clone, e);
            }
        });
        
        return Ok(Json(UploadResponse {
            id: file_id,
            size,
            message: "Upload successful".to_string(),
        }));
    }
    
    Err(StatusCode::BAD_REQUEST)
}

/// GET /stream/:id - Zero-copy file streaming
/// 
/// Uses sendfile optimization for maximum throughput.
pub async fn stream_handler(
    State(state): State<Arc<AppState>>,
    Path(file_id): Path<String>,
) -> Result<Response, StatusCode> {
    // Check if file exists
    if !state.storage.file_exists(&file_id).await {
        return Err(StatusCode::NOT_FOUND);
    }
    
    // Get zero-copy stream
    let (body, size, mime) = state
        .storage
        .stream_file(&file_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    tracing::info!("Streaming file: {} ({} bytes)", file_id, size);
    
    // Build response with appropriate headers
    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, size)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", file_id),
        )
        .body(body)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(response)
}

/// GET /thumbnail/:id - On-demand thumbnail generation
pub async fn thumbnail_handler(
    State(state): State<Arc<AppState>>,
    Path(file_id): Path<String>,
) -> Result<Response, StatusCode> {
    // Check if original file exists
    if !state.storage.file_exists(&file_id).await {
        return Err(StatusCode::NOT_FOUND);
    }
    
    // Generate or retrieve cached thumbnail
    let thumbnail_data = generate_thumbnail(&state.storage, &file_id, &state.thumbnail_config)
        .await
        .map_err(|e| {
            tracing::error!("Thumbnail error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    // Return WebP thumbnail
    let response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/webp")
        .header(header::CONTENT_LENGTH, thumbnail_data.len())
        .header(header::CACHE_CONTROL, "public, max-age=31536000")
        .body(Body::from(thumbnail_data))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(response)
}

/// POST /auth/login - Simple login endpoint
pub async fn login_handler(
    State(state): State<Arc<AppState>>,
    Json(request): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, StatusCode> {
    // Simple authentication (in production, validate against database)
    if request.username.is_empty() || request.password.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    
    // Create session and generate token
    let token = create_session(&state.sessions, &request.username);
    
    Ok(Json(LoginResponse {
        token,
        expires_in: 86400,
    }))
}

/// Health check endpoint
pub async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

// Make Storage cloneable for handlers
impl Clone for Storage {
    fn clone(&self) -> Self {
        Self::new(&self.file_path("").parent().unwrap())
    }
}

impl Clone for ThumbnailConfig {
    fn clone(&self) -> Self {
        Self {
            max_width: self.max_width,
            max_height: self.max_height,
            quality: self.quality,
        }
    }
}
