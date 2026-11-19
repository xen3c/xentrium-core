use xentrium_shared::{Transaction, Result, XentriumError, TransactionType, Balance};
use xentrium_crypto::KeyPair;
use chrono::Utc;
use hex;

pub struct TransactionProcessor;

impl TransactionProcessor {
    pub fn create_transfer(
        from: String,
        to: String,
        amount: Balance,
        fee: Option<Balance>,
    ) -> Result<Transaction> {
        let fee = fee.unwrap_or(Transaction::DEFAULT_FEE);
        
        if fee < Transaction::FEE_FLOOR {
            return Err(XentriumError::InvalidTransaction("Fee below floor".to_string()));
        }

        Ok(Transaction {
            id: String::new(), // Will be set after signing
            tx_type: TransactionType::Transfer,
            from_address: from,
            to_address: Some(to),
            amount,
            fee,
            nonce: 0, // Will be set by sender
            timestamp: Utc::now(),
            signature: None,
            validator_data: None,
            governance_data: None,
        })
    }

    pub fn sign_transaction(
        mut tx: Transaction,
        keypair: &KeyPair,
    ) -> Result<Transaction> {
        let hash = tx.hash();
        let signature = keypair.sign(&hash);
        tx.signature = Some(signature);
        tx.id = hex::encode(&hash);
        Ok(tx)
    }

    pub fn validate(tx: &Transaction, sender_balance: Balance, sender_nonce: u64) -> Result<()> {
        if tx.amount < 0 {
            return Err(XentriumError::InvalidTransaction("Negative amount".to_string()));
        }

        if tx.fee < Transaction::FEE_FLOOR {
            return Err(XentriumError::InvalidTransaction("Fee below floor".to_string()));
        }

        let total = tx.calculate_total();
        if sender_balance < total {
            return Err(XentriumError::InsufficientBalance);
        }

        if tx.nonce != sender_nonce {
            return Err(XentriumError::InvalidNonce);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xentrium_crypto::KeyPair;

    #[test]
    fn test_create_transfer() {
        let tx = TransactionProcessor::create_transfer(
            "XNTFrom".to_string(),
            "XNTTo".to_string(),
            100_000_000,
            None,
        ).unwrap();
        assert_eq!(tx.amount, 100_000_000);
        assert_eq!(tx.fee, 10_000);
    }

    #[test]
    fn test_sign_transaction() {
        let keypair = KeyPair::generate();
        let mut tx = TransactionProcessor::create_transfer(
            "XNTFrom".to_string(),
            "XNTTo".to_string(),
            100_000_000,
            None,
        ).unwrap();

        tx = TransactionProcessor::sign_transaction(tx, &keypair).unwrap();
        assert!(tx.signature.is_some());
        assert!(tx.signature.unwrap().len() == 64);
    }

    #[test]
    fn test_validate_success() {
        let tx = TransactionProcessor::create_transfer(
            "XNTFrom".to_string(),
            "XNTTo".to_string(),
            100_000_000,
            None,
        ).unwrap();
        let result = TransactionProcessor::validate(&tx, 200_000_000, 0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_insufficient_balance() {
        let tx = TransactionProcessor::create_transfer(
            "XNTFrom".to_string(),
            "XNTTo".to_string(),
            100_000_000,
            None,
        ).unwrap();
        let result = TransactionProcessor::validate(&tx, 50_000_000, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_invalid_nonce() {
        let tx = TransactionProcessor::create_transfer(
            "XNTFrom".to_string(),
            "XNTTo".to_string(),
            100_000_000,
            None,
        ).unwrap();
        let result = TransactionProcessor::validate(&tx, 200_000_000, 5);
        assert!(result.is_err());
    }
}
