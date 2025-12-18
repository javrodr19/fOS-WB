//! Authentication module
//!
//! JWT validation and DashMap-based session store for O(1) lookups.

use axum::{
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use dashmap::DashMap;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// JWT secret (in production, load from environment)
const JWT_SECRET: &[u8] = b"super-secret-key-change-in-production";

/// Session duration in seconds (24 hours)
const SESSION_DURATION_SECS: u64 = 86400;

/// JWT claims
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    /// Subject (user ID)
    pub sub: String,
    /// Expiration time (Unix timestamp)
    pub exp: u64,
    /// Issued at (Unix timestamp)
    pub iat: u64,
}

/// Session data stored in DashMap
#[derive(Debug, Clone)]
pub struct Session {
    pub user_id: String,
    pub created_at: u64,
    pub expires_at: u64,
}

/// Thread-safe session store using DashMap for O(1) concurrent access
pub type SessionStore = Arc<DashMap<String, Session>>;

/// Create a new session store
pub fn create_session_store() -> SessionStore {
    Arc::new(DashMap::new())
}

/// Generate a JWT token for a user
pub fn generate_token(user_id: &str) -> Result<String, AuthError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let claims = Claims {
        sub: user_id.to_string(),
        iat: now,
        exp: now + SESSION_DURATION_SECS,
    };
    
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(JWT_SECRET),
    )
    .map_err(|e| AuthError::TokenCreation(e.to_string()))
}

/// Validate a JWT token and return claims
pub fn validate_token(token: &str) -> Result<Claims, AuthError> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(JWT_SECRET),
        &Validation::default(),
    )
    .map_err(|e| AuthError::InvalidToken(e.to_string()))?;
    
    Ok(token_data.claims)
}

/// Create or refresh a session in the store
pub fn create_session(store: &SessionStore, user_id: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let token = generate_token(user_id).unwrap();
    
    let session = Session {
        user_id: user_id.to_string(),
        created_at: now,
        expires_at: now + SESSION_DURATION_SECS,
    };
    
    store.insert(token.clone(), session);
    token
}

/// Validate a session from the store (O(1) lookup)
pub fn validate_session(store: &SessionStore, token: &str) -> Option<Session> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    store.get(token).and_then(|session| {
        if session.expires_at > now {
            Some(session.clone())
        } else {
            // Session expired, remove it
            drop(session);
            store.remove(token);
            None
        }
    })
}

/// Authentication middleware
pub async fn auth_middleware(
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Extract token from Authorization header
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    
    let token = match auth_header {
        Some(header) if header.starts_with("Bearer ") => {
            &header[7..]
        }
        _ => return Err(StatusCode::UNAUTHORIZED),
    };
    
    // Validate JWT token
    match validate_token(token) {
        Ok(_claims) => Ok(next.run(request).await),
        Err(_) => Err(StatusCode::UNAUTHORIZED),
    }
}

/// Authentication errors
#[derive(Debug)]
pub enum AuthError {
    TokenCreation(String),
    InvalidToken(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TokenCreation(e) => write!(f, "Token creation error: {}", e),
            Self::InvalidToken(e) => write!(f, "Invalid token: {}", e),
        }
    }
}

impl std::error::Error for AuthError {}

/// Clean up expired sessions periodically
pub async fn cleanup_expired_sessions(store: SessionStore) {
    loop {
        tokio::time::sleep(Duration::from_secs(300)).await; // Every 5 minutes
        
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        store.retain(|_, session| session.expires_at > now);
        
        tracing::debug!("Session cleanup complete, {} active sessions", store.len());
    }
}
