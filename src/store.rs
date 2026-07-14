use std::{
    fs, io,
    path::{Path, PathBuf},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

use crate::crypto::{NONCE_LEN, SALT_LEN};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VaultEntry {
    pub name: String,
    pub nonce: String,
    pub ciphertext: String,
}

impl VaultEntry {
    pub fn from_bytes(name: String, nonce: [u8; NONCE_LEN], ciphertext: Vec<u8>) -> Self {
        Self {
            name,
            nonce: STANDARD.encode(nonce),
            ciphertext: STANDARD.encode(ciphertext),
        }
    }

    pub fn decode(&self) -> Option<([u8; NONCE_LEN], Vec<u8>)> {
        let nonce: [u8; NONCE_LEN] = STANDARD.decode(&self.nonce).ok()?.try_into().ok()?;
        let ciphertext = STANDARD.decode(&self.ciphertext).ok()?;
        Some((nonce, ciphertext))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VaultData {
    version: u8,
    pub kdf_salt: String,
    pub entries: Vec<VaultEntry>,
}

impl VaultData {
    pub fn new(salt: [u8; SALT_LEN]) -> Self {
        Self {
            version: 1,
            kdf_salt: STANDARD.encode(salt),
            entries: Vec::new(),
        }
    }

    pub fn decode_salt(&self) -> Option<[u8; SALT_LEN]> {
        STANDARD.decode(&self.kdf_salt).ok()?.try_into().ok()
    }
}

pub struct VaultStore {
    path: PathBuf,
}

impl VaultStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn load(&self) -> io::Result<Option<VaultData>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&self.path)?;
        let data = serde_json::from_slice(&bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(Some(data))
    }

    pub fn save(&self, data: &VaultData) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = self.path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(data).map_err(io::Error::other)?,
        )?;
        replace_file(&temporary, &self.path)
    }
}

fn replace_file(temporary: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(windows)]
    if destination.exists() {
        fs::remove_file(destination)?;
    }
    fs::rename(temporary, destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_only_encoded_crypto_material() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("vault.json");
        let store = VaultStore::new(path.clone());
        let mut data = VaultData::new([1; SALT_LEN]);
        data.entries.push(VaultEntry::from_bytes(
            "mail".into(),
            [2; NONCE_LEN],
            vec![3, 4],
        ));
        store.save(&data).unwrap();
        let text = fs::read_to_string(path).unwrap();
        assert!(text.contains("mail"));
        assert!(!text.contains("master"));
        assert_eq!(store.load().unwrap().unwrap().entries.len(), 1);
    }
}
