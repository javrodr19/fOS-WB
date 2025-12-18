//! WireGuard Key Management
//!
//! Provides X25519 key generation and management for WireGuard.

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use rand::rngs::OsRng;
use x25519_dalek::{PublicKey as X25519Public, StaticSecret};
use std::fmt;

/// WireGuard private key (Curve25519)
#[derive(Clone)]
pub struct PrivateKey {
    secret: StaticSecret,
}

impl PrivateKey {
    /// Generate a new random private key
    pub fn generate() -> Self {
        Self {
            secret: StaticSecret::random_from_rng(OsRng),
        }
    }

    /// Create from raw bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self {
            secret: StaticSecret::from(bytes),
        }
    }

    /// Create from base64 string
    pub fn from_base64(s: &str) -> Result<Self, KeyError> {
        let bytes = BASE64.decode(s)
            .map_err(|_| KeyError::InvalidBase64)?;
        
        if bytes.len() != 32 {
            return Err(KeyError::InvalidLength);
        }

        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self::from_bytes(arr))
    }

    /// Get the corresponding public key
    pub fn public_key(&self) -> PublicKey {
        PublicKey {
            key: X25519Public::from(&self.secret),
        }
    }

    /// Get raw bytes
    pub fn to_bytes(&self) -> [u8; 32] {
        self.secret.to_bytes()
    }

    /// Encode as base64
    pub fn to_base64(&self) -> String {
        BASE64.encode(self.to_bytes())
    }

    /// Get the underlying secret for crypto operations
    pub(crate) fn as_secret(&self) -> &StaticSecret {
        &self.secret
    }
}

impl fmt::Debug for PrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PrivateKey([redacted])")
    }
}

/// WireGuard public key (Curve25519)
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PublicKey {
    key: X25519Public,
}

impl PublicKey {
    /// Create from raw bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self {
            key: X25519Public::from(bytes),
        }
    }

    /// Create from base64 string
    pub fn from_base64(s: &str) -> Result<Self, KeyError> {
        let bytes = BASE64.decode(s)
            .map_err(|_| KeyError::InvalidBase64)?;
        
        if bytes.len() != 32 {
            return Err(KeyError::InvalidLength);
        }

        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self::from_bytes(arr))
    }

    /// Get raw bytes
    pub fn to_bytes(&self) -> [u8; 32] {
        self.key.to_bytes()
    }

    /// Encode as base64
    pub fn to_base64(&self) -> String {
        BASE64.encode(self.to_bytes())
    }

    /// Get the underlying key for crypto operations
    pub(crate) fn as_key(&self) -> &X25519Public {
        &self.key
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicKey({}...)", &self.to_base64()[..8])
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_base64())
    }
}

/// A key pair (private + public)
#[derive(Clone)]
pub struct KeyPair {
    pub private: PrivateKey,
    pub public: PublicKey,
}

impl KeyPair {
    /// Generate a new random key pair
    pub fn generate() -> Self {
        let private = PrivateKey::generate();
        let public = private.public_key();
        Self { private, public }
    }

    /// Create from a private key
    pub fn from_private(private: PrivateKey) -> Self {
        let public = private.public_key();
        Self { private, public }
    }
}

impl fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyPair")
            .field("public", &self.public)
            .finish()
    }
}

/// Key parsing errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum KeyError {
    #[error("Invalid base64 encoding")]
    InvalidBase64,

    #[error("Invalid key length (expected 32 bytes)")]
    InvalidLength,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_generation() {
        let keypair = KeyPair::generate();
        
        assert_eq!(keypair.private.to_bytes().len(), 32);
        assert_eq!(keypair.public.to_bytes().len(), 32);
    }

    #[test]
    fn test_key_base64_roundtrip() {
        let keypair = KeyPair::generate();
        
        let b64 = keypair.private.to_base64();
        let restored = PrivateKey::from_base64(&b64).unwrap();
        
        assert_eq!(keypair.private.to_bytes(), restored.to_bytes());
    }

    #[test]
    fn test_public_from_private() {
        let private = PrivateKey::generate();
        let public1 = private.public_key();
        let public2 = private.public_key();
        
        assert_eq!(public1.to_bytes(), public2.to_bytes());
    }

    #[test]
    fn test_invalid_base64() {
        let result = PublicKey::from_base64("not-valid-base64!!!");
        assert!(result.is_err());
    }
}
