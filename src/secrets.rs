use std::str::FromStr;

use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{AeadMut, Payload},
};
use sqlx::{Executor, Postgres};
use thiserror::Error;
use zeroize::Zeroize;

#[derive(Debug)]
pub struct Secret {
    pub id: String,
    pub master_key_id: i32,
    pub secret_version: i32,
    pub encrypted_dek: Vec<u8>,
    pub encrypted_data: Vec<u8>,
    pub dek_nonce: Vec<u8>,
    pub data_nonce: Vec<u8>,
    pub algorithm: String,
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.encrypted_dek.zeroize();
        self.encrypted_data.zeroize();
        self.dek_nonce.zeroize();
        self.data_nonce.zeroize();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub id: i32,
    key: [u8; 32],
}

impl Drop for Key {
    fn drop(&mut self) {
        self.key.zeroize();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRing {
    keys: Vec<Key>,
}

#[derive(Debug, Error)]
pub enum KeyRingError {
    #[error("Invalid entry: {0}")]
    InvalidEntry(String),
    #[error("Invalid ID: {0}")]
    InvalidID(String),
    #[error("Unexpected character: {0}")]
    UnexpectedCharacter(char),
    #[error("Invalid key length: {1} str: {0}")]
    InvalidKeyLength(String, usize),
    #[error("Zero Keys Specified")]
    LessThanOneKey,
}

impl KeyRing {
    pub fn get(&self, id: i32) -> Option<&Key> {
        self.keys.iter().find(|k| k.id == id)
    }

    pub fn max(&self) -> Option<&Key> {
        self.keys.iter().max_by_key(|k| k.id)
    }

    /// Parse a keyring from a comma and colon delimited list:
    /// 1:HEXKEY,2:HEXKEY,...etc
    pub fn parse_from_string(str: &str) -> Result<Self, KeyRingError> {
        let mut keys = Vec::new();

        for entry in str.split(',') {
            let (id_str, key_hex) = entry
                .split_once(':')
                .ok_or(KeyRingError::InvalidEntry(entry.to_string()))?;

            let id: i32 = id_str
                .parse()
                .map_err(|_| KeyRingError::InvalidID(id_str.to_string()))?;

            let decoded = hex::decode(key_hex).map_err(|e| match e {
                hex::FromHexError::InvalidHexCharacter { c, .. } => {
                    KeyRingError::UnexpectedCharacter(c)
                }
                _ => KeyRingError::InvalidEntry(entry.to_string()),
            })?;

            if decoded.len() != 32 {
                return Err(KeyRingError::InvalidKeyLength(
                    entry.to_string(),
                    decoded.len(),
                ));
            }

            let key: [u8; 32] = decoded
                .try_into()
                .expect("Already checked that key length is 32.");

            keys.push(Key { id, key });
        }

        if keys.is_empty() {
            return Err(KeyRingError::LessThanOneKey);
        }

        Ok(KeyRing { keys })
    }

    pub fn dev() -> Self {
        let dev_key = Key {
            id: 1,
            key: [0; 32],
        };

        Self {
            keys: vec![dev_key],
        }
    }
}

impl FromStr for KeyRing {
    type Err = KeyRingError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        KeyRing::parse_from_string(value)
    }
}

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("Key not found")]
    KeyNotFound(Option<i32>),
    #[error("Algorithm mismatch")]
    AlgorithmMismatch,
    #[error("Invalid nonce length")]
    InvalidNonceLength,
    #[error("Invalid DEK length")]
    InvalidDekLength,
    #[error("Invalid UTF-8")]
    InvalidUTF8,
    #[error("Crypto error {0}")]
    CryptoError(aes_gcm::Error),
    #[error("Invalid signing key length {0}")]
    InvalidSigningKeyLength(usize),
}

impl From<aes_gcm::Error> for SecretError {
    fn from(err: aes_gcm::Error) -> Self {
        SecretError::CryptoError(err)
    }
}

impl Secret {
    pub async fn get<'a, E>(id: &str, executor: E) -> Result<Self, sqlx::Error>
    where
        E: Executor<'a, Database = Postgres>,
    {
        sqlx::query_as!(
            Secret,
            r#"
        SELECT * FROM secrets
        WHERE id = $1
        "#,
            id
        )
        .fetch_one(executor)
        .await
    }

    pub async fn put<'a, E>(self, executor: E) -> Result<(), sqlx::Error>
    where
        E: Executor<'a, Database = Postgres>,
    {
        sqlx::query!(r#"
        INSERT INTO secrets (id, master_key_id, secret_version, encrypted_dek, encrypted_data, dek_nonce, data_nonce, algorithm)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        ON CONFLICT (id) DO UPDATE SET
            master_key_id = EXCLUDED.master_key_id,
            secret_version = EXCLUDED.secret_version,
            encrypted_dek = EXCLUDED.encrypted_dek,
            encrypted_data = EXCLUDED.encrypted_data,
            dek_nonce = EXCLUDED.dek_nonce,
            data_nonce = EXCLUDED.data_nonce,
            algorithm = EXCLUDED.algorithm
        "#,
        self.id,
        self.master_key_id,
        self.secret_version,
        self.encrypted_dek,
        self.encrypted_data,
        self.dek_nonce,
        self.data_nonce,
        self.algorithm
      )
      .execute(executor)
      .await?;
        Ok(())
    }

    pub fn decrypt(&self, keys: &KeyRing) -> Result<String, SecretError> {
        let master = keys
            .get(self.master_key_id)
            .ok_or(SecretError::KeyNotFound(Some(self.master_key_id)))?;

        if self.algorithm != "AES-256-GCM" {
            return Err(SecretError::AlgorithmMismatch);
        }

        let dek_nonce: [u8; 12] = self
            .dek_nonce
            .clone()
            .try_into()
            .map_err(|_| SecretError::InvalidNonceLength)?;
        let data_nonce: [u8; 12] = self
            .data_nonce
            .clone()
            .try_into()
            .map_err(|_| SecretError::InvalidNonceLength)?;

        let dek: [u8; 32] = Aes256Gcm::new(&master.key.into())
            .decrypt(
                &dek_nonce.into(),
                Payload::from(self.encrypted_dek.as_slice()),
            )?
            .try_into()
            .map_err(|_| SecretError::InvalidDekLength)?;

        let secret = Aes256Gcm::new(&dek.into()).decrypt(
            &data_nonce.into(),
            Payload::from(self.encrypted_data.as_slice()),
        )?;

        String::from_utf8(secret).map_err(|_| SecretError::InvalidUTF8)
    }

    pub fn new(id: String, secret: String, keys: &KeyRing) -> Result<Self, SecretError> {
        let master = keys.max().ok_or(SecretError::KeyNotFound(None))?;

        let dek = rand::random::<[u8; 32]>();
        let dek_nonce = rand::random::<[u8; 12]>();
        let data_nonce = rand::random::<[u8; 12]>();

        let encrypted_data =
            Aes256Gcm::new(&dek.into()).encrypt(&data_nonce.into(), secret.as_bytes())?;
        let encrypted_dek =
            Aes256Gcm::new(&master.key.into()).encrypt(&dek_nonce.into(), dek.as_slice())?;

        Ok(Secret {
            id,
            master_key_id: master.id,
            secret_version: 1,
            encrypted_dek,
            encrypted_data,
            dek_nonce: dek_nonce.to_vec(),
            data_nonce: data_nonce.to_vec(),
            algorithm: "AES-256-GCM".into(),
        })
    }

    pub fn rotate(self, keys: &KeyRing) -> Result<Self, SecretError> {
        let existing_master = keys
            .get(self.master_key_id)
            .ok_or(SecretError::KeyNotFound(Some(self.master_key_id)))?;
        let master = keys.max().ok_or(SecretError::KeyNotFound(None))?;

        let dek_nonce: [u8; 12] = self
            .dek_nonce
            .clone()
            .try_into()
            .map_err(|_| SecretError::InvalidDekLength)?;

        let dek: [u8; 32] = Aes256Gcm::new(&existing_master.key.into())
            .decrypt(
                &dek_nonce.into(),
                Payload::from(self.encrypted_dek.as_slice()),
            )?
            .try_into()
            .map_err(|_| SecretError::InvalidDekLength)?;

        let re_encrypted_dek =
            Aes256Gcm::new(&master.key.into()).encrypt(&dek_nonce.into(), dek.as_slice())?;

        Ok(Secret {
            id: self.id.clone(),
            master_key_id: master.id,
            secret_version: self.secret_version + 1,
            encrypted_dek: re_encrypted_dek,
            encrypted_data: self.encrypted_data.clone(),
            dek_nonce: self.dek_nonce.clone(),
            data_nonce: self.data_nonce.clone(),
            algorithm: self.algorithm.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_key_ring(keys_data: Vec<(i32, [u8; 32])>) -> KeyRing {
        KeyRing {
            keys: keys_data
                .into_iter()
                .map(|(id, key)| Key { id, key })
                .collect(),
        }
    }

    #[test]
    fn test_secret_lifecycle() {
        let key_data = [1u8; 32];
        let keyring = create_key_ring(vec![(1, key_data)]);

        let secret_value = "my_super_secret_value";
        let secret = Secret::new("secret_id".to_string(), secret_value.to_string(), &keyring)
            .expect("Failed to create secret");

        assert_eq!(secret.id, "secret_id");
        assert_eq!(secret.secret_version, 1);
        assert_eq!(secret.master_key_id, 1);
        assert_eq!(secret.algorithm, "AES-256-GCM");

        let decrypted = secret.decrypt(&keyring).expect("Failed to decrypt");
        assert_eq!(decrypted, secret_value);
    }

    #[test]
    fn test_secret_rotation() {
        let key1_data = [1u8; 32];
        let key2_data = [2u8; 32];

        let keyring1 = create_key_ring(vec![(1, key1_data)]);

        let secret_value = "rotation_test";
        let secret = Secret::new("id".to_string(), secret_value.to_string(), &keyring1)
            .expect("Failed to create secret");

        assert_eq!(secret.master_key_id, 1);

        let keyring2 = create_key_ring(vec![(1, key1_data), (2, key2_data)]);

        let old_dek_nonce = secret.dek_nonce.clone();
        let rotated_secret = secret.rotate(&keyring2).expect("Failed to rotate secret");

        assert_eq!(rotated_secret.master_key_id, 2);
        assert_eq!(rotated_secret.secret_version, 2);
        assert_eq!(rotated_secret.id, "id");
        assert_eq!(rotated_secret.dek_nonce, old_dek_nonce);
        let decrypted = rotated_secret
            .decrypt(&keyring2)
            .expect("Failed to decrypt rotated secret");
        assert_eq!(decrypted, secret_value);
    }

    #[test]
    fn test_missing_key() {
        let key_data = [1u8; 32];
        let keyring = create_key_ring(vec![(1, key_data)]);

        let secret = Secret::new("id".to_string(), "val".to_string(), &keyring).expect("Success");

        let empty_keyring = create_key_ring(vec![]);
        match secret.decrypt(&empty_keyring) {
            Err(SecretError::KeyNotFound(Some(id))) => assert_eq!(id, 1),
            _ => panic!("Expected KeyNotFound(Some(1))"),
        }
    }

    #[test]
    fn test_create_no_keys() {
        let empty_keyring = create_key_ring(vec![]);
        match Secret::new("id".to_string(), "val".to_string(), &empty_keyring) {
            Err(SecretError::KeyNotFound(None)) => {}
            _ => panic!("Expected KeyNotFound(None)"),
        }
    }

    #[test]
    fn test_data_tampering() {
        let key_data = [1u8; 32];
        let keyring = create_key_ring(vec![(1, key_data)]);
        let mut secret = Secret::new("id".into(), "sensitive".into(), &keyring).unwrap();

        // Tamper with encrypted data
        if let Some(byte) = secret.encrypted_data.first_mut() {
            *byte ^= 0xFF;
        }

        match secret.decrypt(&keyring) {
            Err(SecretError::CryptoError(_)) => {}
            res => panic!("Expected CryptoError, got {:?}", res),
        }
    }

    #[test]
    fn test_wrong_key_material() {
        let key_data_a = [1u8; 32];
        let key_data_b = [2u8; 32]; // Same length, different bytes

        let keyring_a = create_key_ring(vec![(1, key_data_a)]);
        let keyring_b_fake = create_key_ring(vec![(1, key_data_b)]); // ID 1, but wrong key

        let secret = Secret::new("id".into(), "val".into(), &keyring_a).unwrap();

        match secret.decrypt(&keyring_b_fake) {
            Err(SecretError::CryptoError(_)) => {} // MAC check failure on DEK decryption
            res => panic!("Expected CryptoError (MAC failure), got {:?}", res),
        }
    }

    #[test]
    fn test_algorithm_mismatch() {
        let key_data = [1u8; 32];
        let keyring = create_key_ring(vec![(1, key_data)]);
        let mut secret = Secret::new("id".into(), "val".into(), &keyring).unwrap();

        secret.algorithm = "AES-128-CBC".into();

        match secret.decrypt(&keyring) {
            Err(SecretError::AlgorithmMismatch) => {}
            res => panic!("Expected AlgorithmMismatch, got {:?}", res),
        }
    }

    #[test]
    fn test_internal_state_corruption_nonce() {
        let key_data = [1u8; 32];
        let keyring = create_key_ring(vec![(1, key_data)]);
        let mut secret = Secret::new("id".into(), "val".into(), &keyring).unwrap();

        secret.dek_nonce.pop(); // Invalid length

        match secret.decrypt(&keyring) {
            Err(SecretError::InvalidNonceLength) => {}
            res => panic!("Expected InvalidNonceLength, got {:?}", res),
        }
    }

    #[test]
    fn test_rotation_failure_missing_old_key() {
        let key1 = [1u8; 32];
        let key2 = [2u8; 32];
        let keyring1 = create_key_ring(vec![(1, key1)]);
        let keyring2_only = create_key_ring(vec![(2, key2)]); // Missing key 1

        let secret = Secret::new("id".into(), "val".into(), &keyring1).unwrap();

        match secret.rotate(&keyring2_only) {
            Err(SecretError::KeyNotFound(Some(1))) => {}
            res => panic!("Expected KeyNotFound(Some(1)), got {:?}", res),
        }
    }

    #[test]
    fn test_invalid_utf8_payload() {
        let key_data = [1u8; 32];
        let keyring = create_key_ring(vec![(1, key_data)]);
        let mut secret = Secret::new("id".into(), "val".into(), &keyring).unwrap();

        // 1. Manually decrypt DEK
        let master_key = keyring.get(1).unwrap();
        let dek_nonce = &secret.dek_nonce;
        let dek_bytes = Aes256Gcm::new(&master_key.key.into())
            .decrypt(
                aes_gcm::Nonce::from_slice(dek_nonce),
                Payload::from(secret.encrypted_dek.as_slice()),
            )
            .expect("Failed to decrypt DEK in test setup");

        let dek = aes_gcm::Key::<Aes256Gcm>::from_slice(&dek_bytes);

        // 2. Encrypt invalid UTF-8 (0xFF is invalid start byte)
        let invalid_utf8 = vec![0xFF, 0xFF, 0xFF];
        let data_nonce = &secret.data_nonce;

        let new_encrypted_data = Aes256Gcm::new(dek)
            .encrypt(
                aes_gcm::Nonce::from_slice(data_nonce),
                invalid_utf8.as_slice(),
            )
            .expect("Failed to encrypt invalid payload");

        secret.encrypted_data = new_encrypted_data;

        // 3. Try to decrypt via struct method
        match secret.decrypt(&keyring) {
            Err(SecretError::InvalidUTF8) => {}
            res => panic!("Expected InvalidUTF8, got {:?}", res),
        }
    }
}
