use ed25519_dalek::{SigningKey, VerifyingKey, SecretKey, Signer, Verifier, Signature};
use rand::rngs::OsRng;
use xentrium_shared::{Result, XentriumError};

#[derive(Clone)]
pub struct KeyPair {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
}

impl KeyPair {
    pub fn generate() -> Self {
        let mut rng = OsRng;
        let signing_key = SigningKey::generate(&mut rng);
        let verifying_key = signing_key.verifying_key();
        Self { signing_key, verifying_key }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 64 {
            return Err(XentriumError::EncryptionError("Invalid keypair bytes: expected 64 bytes".to_string()));
        }
        let mut bytes_array = [0u8; 64];
        bytes_array.copy_from_slice(bytes);
        let signing_key = SigningKey::from_keypair_bytes(&bytes_array)
            .map_err(|_| XentriumError::EncryptionError("Invalid keypair bytes".to_string()))?;
        let verifying_key = signing_key.verifying_key();
        Ok(Self { signing_key, verifying_key })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.signing_key.to_keypair_bytes().to_vec()
    }

    pub fn public_key(&self) -> VerifyingKey {
        self.verifying_key
    }

    pub fn secret_key(&self) -> SecretKey {
        self.signing_key.to_bytes()
    }

    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.signing_key.sign(message).to_bytes().to_vec()
    }

    pub fn keypair(&self) -> &SigningKey {
        &self.signing_key
    }
}

/// Verify a signature
pub fn verify_signature(
    public_key: &VerifyingKey,
    message: &[u8],
    signature_bytes: &[u8],
) -> Result<()> {
    if signature_bytes.len() != 64 {
        return Err(XentriumError::InvalidSignature);
    }
    let mut sig_array = [0u8; 64];
    sig_array.copy_from_slice(signature_bytes);
    let signature = Signature::from_bytes(&sig_array);
    public_key
        .verify(message, &signature)
        .map_err(|_| XentriumError::InvalidSignature)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let keypair = KeyPair::generate();
        assert_eq!(keypair.to_bytes().len(), 64);
    }

    #[test]
    fn test_signing() {
        let keypair = KeyPair::generate();
        let message = b"test message";
        let signature = keypair.sign(message);
        assert_eq!(signature.len(), 64);
    }

    #[test]
    fn test_verification() {
        let keypair = KeyPair::generate();
        let message = b"test message";
        let signature = keypair.sign(message);
        let public_key = keypair.public_key();
        assert!(verify_signature(&public_key, message, &signature).is_ok());
    }
}
