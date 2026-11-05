//! Server configuration, loaded from environment variables.

use std::fmt;
use std::net::{AddrParseError, SocketAddr};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use thiserror::Error;

const DEFAULT_BIND_ADDR: &str = "0.0.0.0:8080";
const DEFAULT_GOOGLE_ISSUER_URL: &str = "https://accounts.google.com";
const DEFAULT_GOOGLE_CALENDAR_BASE_URL: &str = "https://www.googleapis.com/calendar/v3";
const DEFAULT_GOOGLE_CALENDAR_SCOPE: &str =
    "https://www.googleapis.com/auth/calendar.events.owned.readonly";

const TOKEN_ENC_KEY_LEN: usize = 32;
const JWT_SIGNING_KEY_MIN_LEN: usize = 32;

/// Error returned when the configuration is missing or malformed.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// A required variable is unset or empty.
    #[error("{var}: required variable is not set")]
    Missing { var: &'static str },

    /// `BIND_ADDR` is not a valid `host:port` socket address.
    #[error("{var}: not a valid socket address: {source}")]
    BadBindAddr {
        var: &'static str,
        source: AddrParseError,
    },

    /// A key variable is not valid standard base64.
    #[error("{var}: not valid base64: {source}")]
    BadBase64 {
        var: &'static str,
        source: base64::DecodeError,
    },

    /// A key decoded to a different number of bytes than required.
    #[error("{var}: must decode to exactly {expected} bytes, got {actual}")]
    WrongKeyLength {
        var: &'static str,
        expected: usize,
        actual: usize,
    },

    /// A key is shorter than the minimum length.
    #[error("{var}: must be at least {min} bytes, got {actual}")]
    KeyTooShort {
        var: &'static str,
        min: usize,
        actual: usize,
    },
}

/// Runtime configuration of the server.
///
/// `Debug` redacts every secret, so the value can be logged safely.
#[derive(Clone)]
pub struct Config {
    /// Address the HTTP server listens on.
    pub bind_addr: SocketAddr,
    /// PostgreSQL connection URL (contains the password).
    pub database_url: String,
    /// Public URL of the server, without trailing slash.
    pub public_base_url: String,
    /// Google OAuth client id.
    pub google_client_id: String,
    /// Google OAuth client secret.
    pub google_client_secret: String,
    /// Google Workspace domain allowed to log in.
    pub allowed_hd: String,
    /// Key used to encrypt stored Google tokens.
    pub token_enc_key: [u8; 32],
    /// Key used to sign the JWTs issued by the server.
    pub jwt_signing_key: Vec<u8>,
    /// Google OpenID Connect issuer URL.
    pub google_issuer_url: String,
    /// Base URL of the Google Calendar API.
    pub google_calendar_base_url: String,
    /// OAuth scope requested for Google Calendar access.
    pub google_calendar_scope: String,
}

impl Config {
    /// Loads the configuration from the process environment.
    ///
    /// # Errors
    ///
    /// Returns `Err` in the same cases as [`Config::from_lookup`]. Variables
    /// whose value is not valid Unicode are treated as unset.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Builds the configuration reading variables through `lookup`.
    ///
    /// Empty values count as unset: a required variable that is empty is
    /// missing, an optional one falls back to its default. `PUBLIC_BASE_URL`
    /// has its trailing slashes removed. `JWT_SIGNING_KEY` is used as raw
    /// UTF-8 bytes; `TOKEN_ENC_KEY` is decoded from standard base64. URLs
    /// are not checked beyond being non-empty.
    ///
    /// # Errors
    ///
    /// Returns `Err` when:
    /// - a required variable is unset or empty (`ConfigError::Missing`);
    /// - `BIND_ADDR` is not a socket address (`ConfigError::BadBindAddr`);
    /// - `TOKEN_ENC_KEY` is not base64 (`ConfigError::BadBase64`);
    /// - `TOKEN_ENC_KEY` does not decode to 32 bytes (`ConfigError::WrongKeyLength`);
    /// - `JWT_SIGNING_KEY` is shorter than 32 bytes (`ConfigError::KeyTooShort`).
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let optional = |var: &str, default: &str| -> String {
            match lookup(var) {
                Some(value) if !value.is_empty() => value,
                _ => default.to_string(),
            }
        };
        let required = |var: &'static str| -> Result<String, ConfigError> {
            match lookup(var) {
                Some(value) if !value.is_empty() => Ok(value),
                _ => Err(ConfigError::Missing { var }),
            }
        };

        let bind_addr = optional("BIND_ADDR", DEFAULT_BIND_ADDR)
            .parse()
            .map_err(|source| ConfigError::BadBindAddr {
                var: "BIND_ADDR",
                source,
            })?;
        let public_base_url = required("PUBLIC_BASE_URL")?
            .trim_end_matches('/')
            .to_string();

        Ok(Self {
            bind_addr,
            database_url: required("DATABASE_URL")?,
            public_base_url,
            google_client_id: required("GOOGLE_CLIENT_ID")?,
            google_client_secret: required("GOOGLE_CLIENT_SECRET")?,
            allowed_hd: required("ALLOWED_HD")?,
            token_enc_key: decode_token_enc_key(&required("TOKEN_ENC_KEY")?)?,
            jwt_signing_key: parse_jwt_signing_key(&required("JWT_SIGNING_KEY")?)?,
            google_issuer_url: optional("GOOGLE_ISSUER_URL", DEFAULT_GOOGLE_ISSUER_URL),
            google_calendar_base_url: optional(
                "GOOGLE_CALENDAR_BASE_URL",
                DEFAULT_GOOGLE_CALENDAR_BASE_URL,
            ),
            google_calendar_scope: optional("GOOGLE_CALENDAR_SCOPE", DEFAULT_GOOGLE_CALENDAR_SCOPE),
        })
    }
}

/// Decodes `TOKEN_ENC_KEY` from standard base64 into a 32-byte key.
///
/// # Errors
///
/// Returns `Err` when `value` is not valid base64 or does not decode to
/// exactly 32 bytes. Surrounding whitespace is not trimmed.
fn decode_token_enc_key(value: &str) -> Result<[u8; TOKEN_ENC_KEY_LEN], ConfigError> {
    let var = "TOKEN_ENC_KEY";
    let bytes = STANDARD
        .decode(value)
        .map_err(|source| ConfigError::BadBase64 { var, source })?;
    let actual = bytes.len();
    bytes.try_into().map_err(|_| ConfigError::WrongKeyLength {
        var,
        expected: TOKEN_ENC_KEY_LEN,
        actual,
    })
}

/// Turns `JWT_SIGNING_KEY` into raw key bytes.
///
/// # Errors
///
/// Returns `Err` when `value` is shorter than 32 bytes (UTF-8 length).
fn parse_jwt_signing_key(value: &str) -> Result<Vec<u8>, ConfigError> {
    let actual = value.len();
    if actual < JWT_SIGNING_KEY_MIN_LEN {
        return Err(ConfigError::KeyTooShort {
            var: "JWT_SIGNING_KEY",
            min: JWT_SIGNING_KEY_MIN_LEN,
            actual,
        });
    }
    Ok(value.as_bytes().to_vec())
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const REDACTED: &str = "<redacted>";
        f.debug_struct("Config")
            .field("bind_addr", &self.bind_addr)
            .field("database_url", &REDACTED)
            .field("public_base_url", &self.public_base_url)
            .field("google_client_id", &self.google_client_id)
            .field("google_client_secret", &REDACTED)
            .field("allowed_hd", &self.allowed_hd)
            .field("token_enc_key", &REDACTED)
            .field("jwt_signing_key", &REDACTED)
            .field("google_issuer_url", &self.google_issuer_url)
            .field("google_calendar_base_url", &self.google_calendar_base_url)
            .field("google_calendar_scope", &self.google_calendar_scope)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const KEY_B64: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
    const JWT_KEY: &str = "0123456789abcdef0123456789abcdef";

    fn base_vars() -> HashMap<&'static str, String> {
        HashMap::from([
            ("DATABASE_URL", "postgres://u:p@db/x".to_string()),
            ("PUBLIC_BASE_URL", "https://api.example.com".to_string()),
            ("GOOGLE_CLIENT_ID", "client-id".to_string()),
            ("GOOGLE_CLIENT_SECRET", "client-secret".to_string()),
            ("ALLOWED_HD", "example.com".to_string()),
            ("TOKEN_ENC_KEY", KEY_B64.to_string()),
            ("JWT_SIGNING_KEY", JWT_KEY.to_string()),
        ])
    }

    fn load(vars: &HashMap<&'static str, String>) -> Result<Config, ConfigError> {
        Config::from_lookup(|name| vars.get(name).cloned())
    }

    #[test]
    fn applies_defaults_for_optional_vars() {
        let config = load(&base_vars()).unwrap();

        assert_eq!(config.bind_addr, "0.0.0.0:8080".parse().unwrap());
        assert_eq!(config.google_issuer_url, "https://accounts.google.com");
        assert_eq!(
            config.google_calendar_base_url,
            "https://www.googleapis.com/calendar/v3"
        );
        assert_eq!(
            config.google_calendar_scope,
            "https://www.googleapis.com/auth/calendar.events.owned.readonly"
        );
        assert_eq!(config.token_enc_key[1], 1);
        assert_eq!(config.jwt_signing_key, JWT_KEY.as_bytes());
    }

    #[test]
    fn overrides_defaults_when_set() {
        let mut vars = base_vars();
        vars.insert("BIND_ADDR", "127.0.0.1:9000".to_string());
        vars.insert("GOOGLE_ISSUER_URL", "http://localhost:1".to_string());

        let config = load(&vars).unwrap();

        assert_eq!(config.bind_addr, "127.0.0.1:9000".parse().unwrap());
        assert_eq!(config.google_issuer_url, "http://localhost:1");
    }

    #[test]
    fn rejects_missing_required_var() {
        let mut vars = base_vars();
        vars.remove("DATABASE_URL");

        let err = load(&vars).unwrap_err();

        assert!(matches!(
            err,
            ConfigError::Missing {
                var: "DATABASE_URL"
            }
        ));
        assert!(err.to_string().contains("DATABASE_URL"));
    }

    #[test]
    fn treats_empty_required_var_as_missing() {
        let mut vars = base_vars();
        vars.insert("ALLOWED_HD", String::new());

        let err = load(&vars).unwrap_err();

        assert!(matches!(err, ConfigError::Missing { var: "ALLOWED_HD" }));
    }

    #[test]
    fn rejects_invalid_bind_addr() {
        let mut vars = base_vars();
        vars.insert("BIND_ADDR", "not-an-addr".to_string());

        let err = load(&vars).unwrap_err();

        assert!(matches!(err, ConfigError::BadBindAddr { .. }));
    }

    #[test]
    fn rejects_bad_base64_token_key() {
        let mut vars = base_vars();
        vars.insert("TOKEN_ENC_KEY", "!!!not base64!!!".to_string());

        let err = load(&vars).unwrap_err();

        assert!(matches!(err, ConfigError::BadBase64 { .. }));
        assert!(err.to_string().contains("TOKEN_ENC_KEY"));
    }

    #[test]
    fn rejects_token_key_with_wrong_length() {
        let mut vars = base_vars();
        // 16 bytes of zeros
        vars.insert("TOKEN_ENC_KEY", "AAAAAAAAAAAAAAAAAAAAAA==".to_string());

        let err = load(&vars).unwrap_err();

        assert!(matches!(
            err,
            ConfigError::WrongKeyLength {
                expected: 32,
                actual: 16,
                ..
            }
        ));
    }

    #[test]
    fn rejects_short_jwt_key() {
        let mut vars = base_vars();
        vars.insert("JWT_SIGNING_KEY", "too-short".to_string());

        let err = load(&vars).unwrap_err();

        assert!(matches!(
            err,
            ConfigError::KeyTooShort {
                min: 32,
                actual: 9,
                ..
            }
        ));
    }

    #[test]
    fn trims_trailing_slash_from_public_base_url() {
        let mut vars = base_vars();
        vars.insert("PUBLIC_BASE_URL", "https://api.example.com//".to_string());

        let config = load(&vars).unwrap();

        assert_eq!(config.public_base_url, "https://api.example.com");
    }

    #[test]
    fn debug_output_redacts_secrets() {
        let config = load(&base_vars()).unwrap();

        let debug = format!("{config:?}");

        assert!(!debug.contains("client-secret"));
        assert!(!debug.contains(JWT_KEY));
        assert!(!debug.contains("postgres://"));
        assert!(debug.contains("client-id"));
    }
}
