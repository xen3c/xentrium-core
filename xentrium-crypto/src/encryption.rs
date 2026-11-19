use aes_gcm::{Aes256Gcm, aead::{Aead, KeyInit, generic_array::GenericArray}};
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use xentrium_shared::{Result, XentriumError};

pub fn encrypt(data: &[u8], password: &str) -> Result<Vec<u8>> {
    let salt = SaltString::from_b64("SGVsbG8gV29ybGQh").unwrap();
    let argon2 = Argon2::default();
    let password_hash = argon2.hash_password(password.as_bytes(), &salt)
        .map_err(|e| XentriumError::EncryptionError(e.to_string()))?;

    let key_bytes = password_hash.hash.unwrap().as_bytes()[..32].to_vec();
    let key = GenericArray::from_slice(&key_bytes);
    let nonce = GenericArray::from_slice(&[0u8; 12]);
    
    let cipher = Aes256Gcm::new(key);
    let ciphertext = cipher.encrypt(nonce, data)
        .map_err(|e| XentriumError::EncryptionError(e.to_string()))?;

    Ok(ciphertext)
}

pub fn decrypt(ciphertext: &[u8], password: &str) -> Result<Vec<u8>> {
    let salt = SaltString::from_b64("SGVsbG8gV29ybGQh").unwrap();
    let argon2 = Argon2::default();
    let password_hash = argon2.hash_password(password.as_bytes(), &salt)
        .map_err(|e| XentriumError::EncryptionError(e.to_string()))?;

    let key_bytes = password_hash.hash.unwrap().as_bytes()[..32].to_vec();
    let key = GenericArray::from_slice(&key_bytes);
    let nonce = GenericArray::from_slice(&[0u8; 12]);

    let cipher = Aes256Gcm::new(key);
    let plaintext = cipher.decrypt(nonce, ciphertext)
        .map_err(|e| XentriumError::EncryptionError(e.to_string()))?;

    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_decryption() {
        let data = b"secret data";
        let password = "test_password";
        let encrypted = encrypt(data, password).unwrap();
        let decrypted = decrypt(&encrypted, password).unwrap();
        assert_eq!(data, decrypted.as_slice());
    }
}
