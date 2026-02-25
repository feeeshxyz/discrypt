use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use x25519_dalek::{PublicKey, StaticSecret};

const TAG: &str = "[DISCRYPT]";

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

pub fn encrypt(plaintext: &str, shared_key: &[u8; 32]) -> Result<String, String> {
    let key = Key::<Aes256Gcm>::from_slice(shared_key);
    let cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|_| "Encryption failed".to_string())?;

    let mut payload = Vec::with_capacity(12 + ciphertext.len());
    payload.extend_from_slice(&nonce);
    payload.extend_from_slice(&ciphertext);

    Ok(format!("{}{}", TAG, BASE64.encode(&payload)))
}

pub fn is_encrypted(text: &str) -> bool {
    text.starts_with(TAG)
}

pub fn decrypt(tagged_text: &str, shared_key: &[u8; 32]) -> Option<String> {
    let b64 = tagged_text.strip_prefix(TAG)?;
    let payload = BASE64.decode(b64).ok()?;
    if payload.len() < 29 {
        return None;
    }

    let (nonce_bytes, ciphertext) = payload.split_at(12);
    let key = Key::<Aes256Gcm>::from_slice(shared_key);
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher.decrypt(nonce, ciphertext).ok()?;
    String::from_utf8(plaintext).ok()
}
