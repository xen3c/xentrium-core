use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use xentrium_shared::{Address, Result, XentriumError};
use crate::{KeyPair, encrypt};

pub struct Wallet {
    keypair: Option<KeyPair>,
    address: Address,
    locked: Arc<Mutex<bool>>,
    last_activity: Arc<Mutex<Instant>>,
}

impl Wallet {
    pub fn new(password: &str) -> Result<Self> {
        let keypair = KeyPair::generate();
        let address = Self::derive_address(&keypair);
        
        let wallet = Wallet {
            keypair: Some(keypair),
            address,
            locked: Arc::new(Mutex::new(true)),
            last_activity: Arc::new(Mutex::new(Instant::now())),
        };

        // Store encrypted keypair (simplified for Phase 1)
        let keypair_bytes = wallet.keypair.as_ref().unwrap().to_bytes();
        let _encrypted = encrypt(&keypair_bytes, password)?;

        Ok(wallet)
    }

    pub fn address(&self) -> &Address {
        &self.address
    }

    pub fn is_locked(&self) -> bool {
        *self.locked.lock().unwrap()
    }

    pub fn unlock(&self, password: &str) -> Result<()> {
        // Verify password (simplified for Phase 1)
        if password.is_empty() {
            return Err(XentriumError::InvalidPassword);
        }
        *self.locked.lock().unwrap() = false;
        self.update_activity();
        Ok(())
    }

    pub fn lock(&self) {
        *self.locked.lock().unwrap() = true;
    }

    pub fn sign(&self, message: &[u8]) -> Result<Vec<u8>> {
        if self.is_locked() {
            return Err(XentriumError::WalletLocked);
        }
        self.update_activity();
        let keypair = self.keypair.as_ref().ok_or_else(|| XentriumError::WalletLocked)?;
        Ok(keypair.sign(message))
    }

    pub fn public_key(&self) -> Vec<u8> {
        if let Some(keypair) = &self.keypair {
            keypair.public_key().to_bytes().to_vec()
        } else {
            vec![]
        }
    }

    fn derive_address(keypair: &KeyPair) -> Address {
        let public_key_bytes = keypair.public_key().to_bytes();
        let encoded = bs58::encode(public_key_bytes).into_string();
        format!("{}{}", xentrium_shared::ADDRESS_PREFIX, encoded)
    }

    fn update_activity(&self) {
        *self.last_activity.lock().unwrap() = Instant::now();
    }

    pub fn check_auto_lock(&self) {
        let elapsed = self.last_activity.lock().unwrap().elapsed();
        if elapsed > Duration::from_secs(xentrium_shared::AUTO_LOCK_MINUTES * 60) {
            self.lock();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wallet_creation() {
        let wallet = Wallet::new("test_password").unwrap();
        assert!(!wallet.address().is_empty());
    }

    #[test]
    fn test_lock_unlock() {
        let wallet = Wallet::new("test_password").unwrap();
        wallet.unlock("test_password").unwrap();
        assert!(!wallet.is_locked());
        wallet.lock();
        assert!(wallet.is_locked());
    }

    #[test]
    fn test_signing() {
        let wallet = Wallet::new("test_password").unwrap();
        wallet.unlock("test_password").unwrap();
        let message = b"test message";
        let signature = wallet.sign(message).unwrap();
        assert!(signature.len() == 64);
    }

    #[test]
    fn test_signing_locked() {
        let wallet = Wallet::new("test_password").unwrap();
        let message = b"test message";
        let result = wallet.sign(message);
        assert!(result.is_err());
    }
}
