//! PKCE (RFC 7636) helpers for the desktop login flow.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

/// Failure while generating random bytes.
#[derive(Debug, thiserror::Error)]
pub enum PkceError {
    /// The operating system random number generator failed.
    #[error("random number generator failed: {0}")]
    Random(#[from] getrandom::Error),
}

/// Returns `len` random bytes encoded as base64url without padding.
fn random_base64url(len: usize) -> Result<String, PkceError> {
    let mut bytes = vec![0u8; len];
    getrandom::fill(&mut bytes)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// Generates a PKCE code verifier: 32 random bytes as base64url, 43 characters.
///
/// # Errors
///
/// Returns [`PkceError::Random`] when the OS random generator fails.
pub fn generate_verifier() -> Result<String, PkceError> {
    random_base64url(32)
}

/// Generates the `state` parameter of the login: 24 random bytes as base64url, 32 characters.
///
/// # Errors
///
/// Returns [`PkceError::Random`] when the OS random generator fails.
pub fn generate_state() -> Result<String, PkceError> {
    random_base64url(24)
}

/// Computes the S256 code challenge of a verifier: base64url (no padding) of its SHA-256.
///
/// The verifier is hashed as given; its length and alphabet are not checked.
pub fn challenge_for(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_base64url(text: &str) -> bool {
        text.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    }

    #[test]
    fn challenge_matches_rfc7636_appendix_b() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        assert_eq!(
            challenge_for(verifier),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn verifier_has_43_url_safe_characters() {
        let verifier = generate_verifier().unwrap();
        assert_eq!(verifier.len(), 43);
        assert!(is_base64url(&verifier));
    }

    #[test]
    fn state_has_32_url_safe_characters() {
        let state = generate_state().unwrap();
        assert_eq!(state.len(), 32);
        assert!(is_base64url(&state));
    }

    #[test]
    fn verifiers_differ_between_calls() {
        assert_ne!(generate_verifier().unwrap(), generate_verifier().unwrap());
    }
}
