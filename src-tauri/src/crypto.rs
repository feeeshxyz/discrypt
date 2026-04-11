use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use thiserror::Error;
use x25519_dalek::{PublicKey, StaticSecret};

const TAG: &str = "[DISCRYPT]";

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("Encryption failed: AES-GCM error")]
    EncryptionFailed,
    #[error("Missing [DISCRYPT] tag")]
    MissingTag,
    #[error("Base64 decode failed: {0}")]
    Base64Decode(#[from] base64::DecodeError),
    #[error("Payload too short ({0} bytes, need at least 29)")]
    PayloadTooShort(usize),
    #[error("Decryption failed: wrong key or corrupted data")]
    DecryptionFailed,
    #[error("Decrypted data is not valid UTF-8: {0}")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),
}

impl From<CryptoError> for String {
    fn from(e: CryptoError) -> String {
        e.to_string()
    }
}

pub fn generate_keypair() -> ([u8; 32], [u8; 32]) {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    (secret.to_bytes(), *public.as_bytes())
}

pub fn derive_shared_secret(my_secret: &[u8; 32], their_public: &[u8; 32]) -> [u8; 32] {
    let secret = StaticSecret::from(*my_secret);
    let public = PublicKey::from(*their_public);
    *secret.diffie_hellman(&public).as_bytes()
}

pub fn encrypt(plaintext: &str, shared_key: &[u8; 32]) -> Result<String, CryptoError> {
    let key = Key::<Aes256Gcm>::from_slice(shared_key);
    let cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|_| CryptoError::EncryptionFailed)?;

    let mut payload = Vec::with_capacity(12 + ciphertext.len());
    payload.extend_from_slice(&nonce);
    payload.extend_from_slice(&ciphertext);

    Ok(format!("{}{}", TAG, BASE64.encode(&payload)))
}

pub fn is_encrypted(text: &str) -> bool {
    text.starts_with(TAG)
}

pub fn decrypt(tagged_text: &str, shared_key: &[u8; 32]) -> Result<String, CryptoError> {
    let b64 = tagged_text.strip_prefix(TAG).ok_or(CryptoError::MissingTag)?;
    let payload = BASE64.decode(b64)?;
    if payload.len() < 29 {
        return Err(CryptoError::PayloadTooShort(payload.len()));
    }

    let (nonce_bytes, ciphertext) = payload.split_at(12);
    let key = Key::<Aes256Gcm>::from_slice(shared_key);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| CryptoError::DecryptionFailed)?;
    Ok(String::from_utf8(plaintext)?)
}
