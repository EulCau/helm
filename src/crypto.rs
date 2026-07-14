use argon2::{Algorithm, Argon2, Params, Version};
use chacha20::{
    ChaCha20,
    cipher::{KeyIvInit, StreamCipher},
};
use zeroize::Zeroizing;

pub const SALT_LEN: usize = 16;
pub const NONCE_LEN: usize = 12;

#[derive(Debug)]
pub enum CryptoError {
    Randomness,
    InvalidParameters,
}

pub fn random_salt() -> Result<[u8; SALT_LEN], CryptoError> {
    let mut salt = [0_u8; SALT_LEN];
    getrandom::fill(&mut salt).map_err(|_| CryptoError::Randomness)?;
    Ok(salt)
}

pub fn derive_key(
    master_password: &str,
    salt: &[u8; SALT_LEN],
) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
    // 64 MiB, 3 iterations, and one lane keeps interactive desktop use practical.
    let params =
        Params::new(64 * 1024, 3, 1, Some(32)).map_err(|_| CryptoError::InvalidParameters)?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = Zeroizing::new([0_u8; 32]);
    argon2
        .hash_password_into(master_password.as_bytes(), salt, key.as_mut())
        .map_err(|_| CryptoError::InvalidParameters)?;
    Ok(key)
}

pub fn encrypt(
    key: &[u8; 32],
    plaintext: &[u8],
) -> Result<([u8; NONCE_LEN], Vec<u8>), CryptoError> {
    let mut nonce = [0_u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(|_| CryptoError::Randomness)?;
    let mut ciphertext = plaintext.to_vec();
    let mut cipher = ChaCha20::new(key.into(), (&nonce).into());
    cipher.apply_keystream(&mut ciphertext);
    Ok((nonce, ciphertext))
}

pub fn decrypt(key: &[u8; 32], nonce: &[u8; NONCE_LEN], ciphertext: &[u8]) -> Zeroizing<Vec<u8>> {
    let mut plaintext = Zeroizing::new(ciphertext.to_vec());
    let mut cipher = ChaCha20::new(key.into(), nonce.into());
    cipher.apply_keystream(plaintext.as_mut());
    plaintext
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correct_password_round_trips() {
        let salt = random_salt().unwrap();
        let key = derive_key("master", &salt).unwrap();
        let (nonce, ciphertext) = encrypt(&key, b"secret-123").unwrap();
        let plaintext = decrypt(&key, &nonce, &ciphertext);
        assert_eq!(&*plaintext, b"secret-123");
        assert_ne!(ciphertext, b"secret-123");
    }

    #[test]
    fn wrong_password_still_returns_bytes() {
        let salt = random_salt().unwrap();
        let key = derive_key("master", &salt).unwrap();
        let wrong_key = derive_key("wrong", &salt).unwrap();
        let (nonce, ciphertext) = encrypt(&key, b"secret-123").unwrap();
        let plaintext = decrypt(&wrong_key, &nonce, &ciphertext);
        assert_ne!(&*plaintext, b"secret-123");
        assert_eq!(plaintext.len(), ciphertext.len());
    }
}
