use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use rand::RngCore;

/// Encrypt plaintext with AES-256-GCM.
/// Returns `"<hex_nonce>:<hex_ciphertext>"`.
pub fn encrypt(key: &[u8; 32], plaintext: &str) -> Result<String, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(format!("{}:{}", hex::encode(nonce_bytes), hex::encode(ciphertext)))
}

/// Decrypt a value produced by `encrypt`.
pub fn decrypt(key: &[u8; 32], encoded: &str) -> Result<String, String> {
    let (nonce_hex, ct_hex) = encoded
        .split_once(':')
        .ok_or("Invalid encrypted format: missing ':'".to_string())?;
    let nonce_bytes = hex::decode(nonce_hex).map_err(|e| e.to_string())?;
    let ciphertext = hex::decode(ct_hex).map_err(|e| e.to_string())?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| "Decryption failed — wrong key or corrupted data".to_string())?;
    String::from_utf8(plaintext).map_err(|e| e.to_string())
}
