//! Cryptographic helpers: token encryption at rest, hashing, random tokens
//! and the PKCE `S256` check.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};
use thiserror::Error;

const NONCE_LEN: usize = 12;
const RANDOM_TOKEN_LEN: usize = 32;

/// Error returned by the cryptographic helpers.
#[derive(Debug, Error)]
pub enum CryptoError {
    /// The operating system random generator failed.
    #[error("random generator failure: {0}")]
    Random(getrandom::Error),

    /// AES-GCM encryption failed (plaintext too long).
    #[error("encryption failed")]
    Encrypt,

    /// The ciphertext is too short, was tampered with, or the key is wrong.
    #[error("decryption failed")]
    Decrypt,
}

/// Encrypts `plaintext` with AES-256-GCM under `key`.
///
/// A fresh random 96-bit nonce is generated for every call and prepended to
/// the output, so encrypting the same plaintext twice gives different
/// results. The output is `nonce || ciphertext || tag`, 28 bytes longer than
/// the input. An empty plaintext is allowed.
///
/// # Errors
///
/// Returns `Err` when the random generator fails or the plaintext exceeds
/// the AES-GCM size limit (about 64 GiB).
pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(CryptoError::Random)?;
    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(*key));
    let ciphertext = cipher
        .encrypt(&nonce.into(), plaintext)
        .map_err(|_| CryptoError::Encrypt)?;

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypts data produced by [`encrypt`] with the same `key`.
///
/// # Errors
///
/// Returns `Err(CryptoError::Decrypt)` when `data` is shorter than the
/// nonce, when any byte was modified, or when `key` is not the one used to
/// encrypt.
pub fn decrypt(key: &[u8; 32], data: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let Some((nonce, ciphertext)) = data.split_first_chunk::<NONCE_LEN>() else {
        return Err(CryptoError::Decrypt);
    };
    let cipher = Aes256Gcm::new(&Key::<Aes256Gcm>::from(*key));
    cipher
        .decrypt(&(*nonce).into(), ciphertext)
        .map_err(|_| CryptoError::Decrypt)
}

/// Returns the SHA-256 digest of `data`.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// Generates a token of 32 random bytes, encoded as base64url without
/// padding (43 characters).
///
/// # Errors
///
/// Returns `Err` when the operating system random generator fails.
pub fn random_token() -> Result<String, CryptoError> {
    let mut bytes = [0u8; RANDOM_TOKEN_LEN];
    getrandom::fill(&mut bytes).map_err(CryptoError::Random)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Checks a PKCE `S256` pair: true when `base64url(sha256(verifier))`,
/// without padding, equals `challenge`.
///
/// The verifier format (length, alphabet) is not checked: any string is
/// hashed as is. The comparison is case sensitive.
pub fn pkce_s256_matches(verifier: &str, challenge: &str) -> bool {
    URL_SAFE_NO_PAD.encode(sha256(verifier.as_bytes())) == challenge
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [7; 32];

    #[test]
    fn decrypt_returns_encrypted_plaintext() {
        let data = encrypt(&KEY, b"refresh-token").unwrap();

        assert_eq!(decrypt(&KEY, &data).unwrap(), b"refresh-token");
    }

    #[test]
    fn encrypt_handles_empty_plaintext() {
        let data = encrypt(&KEY, b"").unwrap();

        assert_eq!(data.len(), 28);
        assert_eq!(decrypt(&KEY, &data).unwrap(), b"");
    }

    #[test]
    fn encrypt_uses_a_fresh_nonce_each_time() {
        let first = encrypt(&KEY, b"same").unwrap();
        let second = encrypt(&KEY, b"same").unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn decrypt_detects_tampered_ciphertext() {
        let mut data = encrypt(&KEY, b"refresh-token").unwrap();
        if let Some(byte) = data.last_mut() {
            *byte ^= 1;
        }

        assert!(matches!(decrypt(&KEY, &data), Err(CryptoError::Decrypt)));
    }

    #[test]
    fn decrypt_detects_tampered_nonce() {
        let mut data = encrypt(&KEY, b"refresh-token").unwrap();
        if let Some(byte) = data.first_mut() {
            *byte ^= 1;
        }

        assert!(matches!(decrypt(&KEY, &data), Err(CryptoError::Decrypt)));
    }

    #[test]
    fn decrypt_rejects_wrong_key() {
        let data = encrypt(&KEY, b"refresh-token").unwrap();

        assert!(matches!(
            decrypt(&[8; 32], &data),
            Err(CryptoError::Decrypt)
        ));
    }

    #[test]
    fn decrypt_rejects_data_shorter_than_nonce() {
        assert!(matches!(decrypt(&KEY, &[0; 11]), Err(CryptoError::Decrypt)));
    }

    #[test]
    fn sha256_matches_known_vector() {
        let digest = sha256(b"abc");

        assert_eq!(
            URL_SAFE_NO_PAD.encode(digest),
            "ungWv48Bz-pBQUDeXa4iI7ADYaOWF3qctBD_YfIAFa0"
        );
    }

    #[test]
    fn random_token_is_43_url_safe_chars() {
        let token = random_token().unwrap();

        assert_eq!(token.len(), 43);
        assert!(
            token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        );
    }

    #[test]
    fn random_tokens_differ() {
        assert_ne!(random_token().unwrap(), random_token().unwrap());
    }

    // Test vector from RFC 7636, appendix B.
    #[test]
    fn pkce_accepts_rfc_7636_vector() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

        assert!(pkce_s256_matches(verifier, challenge));
    }

    #[test]
    fn pkce_rejects_wrong_verifier() {
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

        assert!(!pkce_s256_matches("another-verifier", challenge));
    }
}
