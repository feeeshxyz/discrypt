use crate::crypto;
use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use argon2::Argon2;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::{info, warn};
use zeroize::Zeroize;


#[derive(Serialize, Deserialize)]
struct EncryptedStore {
    salt: String,   // hex-encoded 16-byte salt
    nonce: String,  // hex-encoded 12-byte nonce
    ciphertext: String, // base64-encoded AES-256-GCM ciphertext
}

#[derive(Serialize, Deserialize)]
struct StoreFile {
    secret_key: String,
    public_key: String,
    contacts: HashMap<String, ContactFile>,
}

#[derive(Serialize, Deserialize)]
struct ContactFile {
    public_key: String,
    #[serde(default = "default_handshake_status")]
    handshake_status: String,
}

fn default_handshake_status() -> String {
    "complete".to_string()
}


#[derive(Serialize)]
pub struct ContactInfo {
    pub username: String,
    pub public_key_hex: String,
    pub fingerprint: String,
    pub handshake_status: String,
}

struct ContactMemory {
    public_key: [u8; 32],
    shared_secret: [u8; 32],
    handshake_status: String,
}

struct KeyStore {
    file_path: PathBuf,
    encryption_key: [u8; 32], // derived from password, kept for re-saving
    salt: [u8; 16],
    secret_key: [u8; 32],
    public_key: [u8; 32],
    contacts: HashMap<String, ContactMemory>,
}

// Global state: file path is set on init, KeyStore loaded after unlock
static STORE_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
static KEY_STORE: Mutex<Option<KeyStore>> = Mutex::new(None);


fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn hex_decode(hex: &str) -> Result<Vec<u8>, String> {
    if hex.len() % 2 != 0 {
        return Err("Hex string has odd length".into());
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|e| format!("Invalid hex: {}", e))
        })
        .collect()
}

fn to_32(bytes: &[u8]) -> Result<[u8; 32], String> {
    bytes
        .try_into()
        .map_err(|_| format!("Expected 32 bytes, got {}", bytes.len()))
}


fn derive_key(password: &str, salt: &[u8; 16]) -> Result<[u8; 32], String> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|e| format!("Key derivation failed: {}", e))?;
    Ok(key)
}

fn encrypt_json(json: &str, enc_key: &[u8; 32]) -> Result<(String, String), String> {
    let key = Key::<Aes256Gcm>::from_slice(enc_key);
    let cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, json.as_bytes())
        .map_err(|e| format!("Store encryption failed: {}", e))?;
    Ok((hex_encode(&nonce), BASE64.encode(&ciphertext)))
}

fn decrypt_json(nonce_hex: &str, ciphertext_b64: &str, enc_key: &[u8; 32]) -> Result<String, String> {
    let nonce_bytes = hex_decode(nonce_hex)?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = BASE64.decode(ciphertext_b64)
        .map_err(|e| format!("Base64 decode failed: {}", e))?;
    let key = Key::<Aes256Gcm>::from_slice(enc_key);
    let cipher = Aes256Gcm::new(key);
    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| "Wrong password. Decryption failed.".to_string())?;
    let text = String::from_utf8(plaintext)
        .map_err(|e| format!("Corrupted store: decrypted data is not valid UTF-8: {}", e))?;
    // Validate it's actually JSON before returning
    serde_json::from_str::<serde_json::Value>(&text)
        .map_err(|_| "Corrupted store: decrypted data is not valid JSON.".to_string())?;
    Ok(text)
}


impl KeyStore {
    fn decrypt_and_load(file_path: PathBuf, password: &str) -> Result<Self, String> {
        let data = std::fs::read_to_string(&file_path)
            .map_err(|e| format!("Failed to read key file: {}", e))?;
        let enc: EncryptedStore = serde_json::from_str(&data)
            .map_err(|e| format!("Failed to parse key file: {}", e))?;

        let salt_bytes = hex_decode(&enc.salt)?;
        let salt: [u8; 16] = salt_bytes.try_into()
            .map_err(|_| "Invalid salt length".to_string())?;
        let mut enc_key = derive_key(password, &salt)?;

        let json = decrypt_json(&enc.nonce, &enc.ciphertext, &enc_key)?;
        let sf: StoreFile = serde_json::from_str(&json)
            .map_err(|e| format!("Failed to parse decrypted store: {}", e))?;

        let secret_key = to_32(&hex_decode(&sf.secret_key)?)?;
        let public_key = to_32(&hex_decode(&sf.public_key)?)?;

        let mut contacts = HashMap::new();
        for (username, cf) in sf.contacts {
            if let Ok(pk) = to_32(&hex_decode(&cf.public_key)?) {
                let shared = crypto::derive_shared_secret(&secret_key, &pk);
                contacts.insert(username, ContactMemory {
                    public_key: pk,
                    shared_secret: shared,
                    handshake_status: cf.handshake_status.clone(),
                });
            }
        }

        let store = KeyStore {
            file_path,
            encryption_key: enc_key,
            salt,
            secret_key,
            public_key,
            contacts,
        };
        enc_key.zeroize();
        Ok(store)
    }

    fn create_new(file_path: PathBuf, password: &str) -> Result<Self, String> {
        let (secret_key, public_key) = crypto::generate_keypair();
        let mut salt = [0u8; 16];
        aes_gcm::aead::OsRng.fill_bytes(&mut salt);
        let enc_key = derive_key(password, &salt)?;

        let store = KeyStore {
            file_path,
            encryption_key: enc_key,
            salt,
            secret_key,
            public_key,
            contacts: HashMap::new(),
        };
        store.save()?;
        Ok(store)
    }

    fn save(&self) -> Result<(), String> {
        let sf = StoreFile {
            secret_key: hex_encode(&self.secret_key),
            public_key: hex_encode(&self.public_key),
            contacts: self
                .contacts
                .iter()
                .map(|(k, v)| {
                    (k.clone(), ContactFile {
                        public_key: hex_encode(&v.public_key),
                        handshake_status: v.handshake_status.clone(),
                    })
                })
                .collect(),
        };

        let json = serde_json::to_string_pretty(&sf)
            .map_err(|e| format!("Failed to serialize key store: {}", e))?;

        let (nonce_hex, ciphertext_b64) = encrypt_json(&json, &self.encryption_key)?;

        let enc = EncryptedStore {
            salt: hex_encode(&self.salt),
            nonce: nonce_hex,
            ciphertext: ciphertext_b64,
        };

        if let Some(parent) = self.file_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create data directory: {}", e))?;
        }

        let out = serde_json::to_string_pretty(&enc)
            .map_err(|e| format!("Failed to serialize encrypted store: {}", e))?;
        std::fs::write(&self.file_path, out)
            .map_err(|e| format!("Failed to write key file: {}", e))?;

        Ok(())
    }
}

use aes_gcm::aead::rand_core::RngCore;

/// Set the store file path. Called once during Tauri setup.
pub fn init(data_dir: PathBuf) -> Result<(), String> {
    let file_path = data_dir.join("discrypt_keys.json");
    let mut path = STORE_PATH.lock().map_err(|e| format!("Lock error: {}", e))?;
    *path = Some(file_path);
    Ok(())
}

pub fn store_exists() -> Result<bool, String> {
    let path = STORE_PATH.lock().map_err(|e| format!("Lock error: {}", e))?;
    let path = path.as_ref().ok_or("Store path not initialized")?;
    Ok(path.exists())
}

/// Returns "none", "legacy" (old plaintext), or "encrypted".
pub fn store_format() -> Result<String, String> {
    let path = STORE_PATH.lock().map_err(|e| format!("Lock error: {}", e))?;
    let path = path.as_ref().ok_or("Store path not initialized")?;
    if !path.exists() {
        return Ok("none".into());
    }
    let data = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read key file: {}", e))?;
    if serde_json::from_str::<EncryptedStore>(&data).is_ok() {
        Ok("encrypted".into())
    } else if serde_json::from_str::<StoreFile>(&data).is_ok() {
        Ok("legacy".into())
    } else {
        Err("Store file exists but is in an unknown format".into())
    }
}

/// Migrate a legacy plaintext store to encrypted format.
pub fn migrate_store(password: &str) -> Result<(), String> {
    let path_guard = STORE_PATH.lock().map_err(|e| format!("Lock error: {}", e))?;
    let file_path = path_guard.as_ref().ok_or("Store path not initialized")?.clone();
    drop(path_guard);

    let data = std::fs::read_to_string(&file_path)
        .map_err(|e| format!("Failed to read key file: {}", e))?;
    let sf: StoreFile = serde_json::from_str(&data)
        .map_err(|e| format!("Failed to parse legacy store: {}", e))?;

    let secret_key = to_32(&hex_decode(&sf.secret_key)?)?;
    let public_key = to_32(&hex_decode(&sf.public_key)?)?;

    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    let enc_key = derive_key(password, &salt)?;

    let mut contacts = HashMap::new();
    for (username, cf) in sf.contacts {
        if let Ok(pk) = to_32(&hex_decode(&cf.public_key)?) {
            let shared = crypto::derive_shared_secret(&secret_key, &pk);
            contacts.insert(username, ContactMemory {
                public_key: pk,
                shared_secret: shared,
                handshake_status: cf.handshake_status.clone(),
            });
        }
    }

    let store = KeyStore {
        file_path,
        encryption_key: enc_key,
        salt,
        secret_key,
        public_key,
        contacts,
    };
    store.save()?;

    let mut global = KEY_STORE.lock().map_err(|e| format!("Lock error: {}", e))?;
    *global = Some(store);
    Ok(())
}

pub fn is_unlocked() -> bool {
    KEY_STORE.lock().map(|s| s.is_some()).unwrap_or(false)
}

pub fn create_store(password: &str) -> Result<(), String> {
    let path_guard = STORE_PATH.lock().map_err(|e| format!("Lock error: {}", e))?;
    let file_path = path_guard.as_ref().ok_or("Store path not initialized")?.clone();
    drop(path_guard);

    if file_path.exists() {
        return Err("Store already exists. Use unlock instead.".into());
    }

    let store = KeyStore::create_new(file_path, password)?;
    let mut global = KEY_STORE.lock().map_err(|e| format!("Lock error: {}", e))?;
    *global = Some(store);
    info!("New key store created");
    Ok(())
}

pub fn unlock_store(password: &str) -> Result<(), String> {
    let path_guard = STORE_PATH.lock().map_err(|e| format!("Lock error: {}", e))?;
    let file_path = path_guard.as_ref().ok_or("Store path not initialized")?.clone();
    drop(path_guard);

    if !file_path.exists() {
        return Err("No store found. Create one first.".into());
    }

    let store = KeyStore::decrypt_and_load(file_path, password)?;
    let contact_count = store.contacts.len();
    let mut global = KEY_STORE.lock().map_err(|e| format!("Lock error: {}", e))?;
    *global = Some(store);
    info!("Store unlocked ({} contacts)", contact_count);
    Ok(())
}

pub fn change_password(old_password: &str, new_password: &str) -> Result<(), String> {
    let path_guard = STORE_PATH.lock().map_err(|e| format!("Lock error: {}", e))?;
    let file_path = path_guard.as_ref().ok_or("Store path not initialized")?.clone();
    drop(path_guard);

    // Verify old password by decrypting
    let mut store = KeyStore::decrypt_and_load(file_path, old_password)?;

    // Derive new key and re-encrypt
    let mut new_salt = [0u8; 16];
    aes_gcm::aead::rand_core::RngCore::fill_bytes(&mut OsRng, &mut new_salt);
    let new_enc_key = derive_key(new_password, &new_salt)?;
    store.salt = new_salt;
    store.encryption_key = new_enc_key;
    store.save()?;

    // Update global store
    let mut global = KEY_STORE.lock().map_err(|e| format!("Lock error: {}", e))?;
    *global = Some(store);
    Ok(())
}

pub fn export_store() -> Result<String, String> {
    let path_guard = STORE_PATH.lock().map_err(|e| format!("Lock error: {}", e))?;
    let file_path = path_guard.as_ref().ok_or("Store path not initialized")?;
    std::fs::read_to_string(file_path)
        .map_err(|e| format!("Failed to read store file: {}", e))
}

pub fn reset_store() -> Result<(), String> {
    let path_guard = STORE_PATH.lock().map_err(|e| format!("Lock error: {}", e))?;
    let file_path = path_guard.as_ref().ok_or("Store path not initialized")?;
    if file_path.exists() {
        std::fs::remove_file(file_path)
            .map_err(|e| format!("Failed to delete store file: {}", e))?;
    }
    drop(path_guard);
    let mut global = KEY_STORE.lock().map_err(|e| format!("Lock error: {}", e))?;
    *global = None;
    Ok(())
}

pub fn get_public_key() -> Result<String, String> {
    let store = KEY_STORE.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = store.as_ref().ok_or("Key store not initialized")?;
    Ok(hex_encode(&store.public_key))
}

pub fn has_contact(username: &str) -> Result<bool, String> {
    let store = KEY_STORE.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = store.as_ref().ok_or("Key store not initialized")?;
    Ok(store.contacts.contains_key(username))
}

pub fn add_contact(username: &str, public_key_hex: &str) -> Result<(), String> {
    let pk = to_32(&hex_decode(public_key_hex)?)?;

    let mut guard = KEY_STORE.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_mut().ok_or("Key store not initialized")?;

    let shared = crypto::derive_shared_secret(&store.secret_key, &pk);
    let status = if store.contacts.contains_key(username) {
        store.contacts[username].handshake_status.clone()
    } else {
        "complete".to_string()
    };
    store.contacts.insert(
        username.to_string(),
        ContactMemory {
            public_key: pk,
            shared_secret: shared,
            handshake_status: status,
        },
    );
    store.save()
}

pub fn remove_contact(username: &str) -> Result<(), String> {
    let mut guard = KEY_STORE.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_mut().ok_or("Key store not initialized")?;
    store.contacts.remove(username);
    store.save()
}

pub fn list_contacts() -> Result<Vec<ContactInfo>, String> {
    let store = KEY_STORE.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = store.as_ref().ok_or("Key store not initialized")?;

    Ok(store
        .contacts
        .iter()
        .map(|(username, data)| {
            let hex = hex_encode(&data.public_key);
            let fingerprint = hex[..8].to_string();
            ContactInfo {
                username: username.clone(),
                public_key_hex: hex,
                fingerprint,
                handshake_status: data.handshake_status.clone(),
            }
        })
        .collect())
}

pub fn encrypt_for(username: &str, plaintext: &str) -> Result<String, String> {
    let store = KEY_STORE.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = store.as_ref().ok_or("Key store not initialized")?;
    let contact = store
        .contacts
        .get(username)
        .ok_or_else(|| format!("Contact '{}' not found", username))?;
    crypto::encrypt(plaintext, &contact.shared_secret).map_err(|e| e.to_string())
}

pub fn set_handshake_status(username: &str, status: &str) -> Result<(), String> {
    let mut guard = KEY_STORE.lock().map_err(|e| format!("Lock error: {}", e))?;
    let store = guard.as_mut().ok_or("Key store not initialized")?;
    let contact = store
        .contacts
        .get_mut(username)
        .ok_or_else(|| format!("Contact '{}' not found", username))?;
    contact.handshake_status = status.to_string();
    store.save()
}

pub fn try_decrypt(ciphertext: &str) -> Option<String> {
    if !crypto::is_encrypted(ciphertext) {
        return None;
    }
    let store = KEY_STORE.lock().ok()?;
    let store = store.as_ref()?;
    for (username, contact) in &store.contacts {
        match crypto::decrypt(ciphertext, &contact.shared_secret) {
            Ok(plaintext) => return Some(plaintext),
            Err(crypto::CryptoError::DecryptionFailed) => {
                // Wrong key for this contact, try next
                continue;
            }
            Err(e) => {
                warn!("Decrypt error for contact '{}': {}", username, e);
                continue;
            }
        }
    }
    None
}

pub fn import_store(json_data: &str, password: &str) -> Result<(), String> {
    // Validate it's a valid EncryptedStore
    let enc: EncryptedStore = serde_json::from_str(json_data)
        .map_err(|e| format!("Invalid backup format: {}", e))?;

    // Validate salt, nonce, ciphertext are valid hex/base64
    let salt_bytes = hex_decode(&enc.salt)?;
    let salt: [u8; 16] = salt_bytes.try_into()
        .map_err(|_| "Invalid salt length in backup".to_string())?;

    // Try to decrypt with provided password to validate
    let enc_key = derive_key(password, &salt)?;
    let json = decrypt_json(&enc.nonce, &enc.ciphertext, &enc_key)?;

    // Validate inner structure
    let sf: StoreFile = serde_json::from_str(&json)
        .map_err(|e| format!("Backup contains invalid key data: {}", e))?;
    let _ = to_32(&hex_decode(&sf.secret_key)?)?;
    let _ = to_32(&hex_decode(&sf.public_key)?)?;

    // All valid — write to disk
    let path_guard = STORE_PATH.lock().map_err(|e| format!("Lock error: {}", e))?;
    let file_path = path_guard.as_ref().ok_or("Store path not initialized")?.clone();
    drop(path_guard);

    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;
    }
    std::fs::write(&file_path, json_data)
        .map_err(|e| format!("Failed to write backup to store: {}", e))?;

    // Load it into memory
    let store = KeyStore::decrypt_and_load(file_path, password)?;
    let mut global = KEY_STORE.lock().map_err(|e| format!("Lock error: {}", e))?;
    *global = Some(store);

    info!("Store imported successfully from backup");
    Ok(())
}
