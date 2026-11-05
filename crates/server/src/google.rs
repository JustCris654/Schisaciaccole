//! Google OpenID Connect client used by the login flow.
//!
//! The provider metadata is fetched once at startup through discovery. The
//! signing keys (JWKS) are fetched again at every code exchange, so a key
//! rotation by Google never requires a restart.

use std::fmt;

use openidconnect::core::{
    CoreAuthDisplay, CoreAuthPrompt, CoreErrorResponseType, CoreGenderClaim, CoreJsonWebKey,
    CoreJweContentEncryptionAlgorithm, CoreJwsSigningAlgorithm, CoreProviderMetadata,
    CoreResponseType, CoreRevocableToken, CoreRevocationErrorResponse,
    CoreTokenIntrospectionResponse, CoreTokenType,
};
use openidconnect::reqwest;
use openidconnect::{
    AdditionalClaims, AuthenticationFlow, AuthorizationCode, Client, ClientId, ClientSecret,
    CsrfToken, EmptyExtraTokenFields, EndpointMaybeSet, EndpointNotSet, EndpointSet, IdToken,
    IdTokenFields, IdTokenVerifier, IssuerUrl, JsonWebKeySet, JsonWebKeySetUrl, Nonce,
    OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken,
    RequestTokenError, Scope, StandardErrorResponse, StandardTokenResponse, TokenResponse,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

use crate::config::Config;

/// Error returned by the Google OpenID Connect client.
#[derive(Debug, Error)]
pub enum GoogleError {
    /// The HTTP client could not be built.
    #[error("cannot build the HTTP client: {0}")]
    HttpClient(#[source] reqwest::Error),

    /// The issuer or redirect URL from the configuration is not a valid URL.
    #[error("invalid URL in the configuration: {0}")]
    BadUrl(#[source] url::ParseError),

    /// Discovery or the JWKS download failed.
    #[error("OpenID discovery failed: {0}")]
    Discovery(String),

    /// The provider metadata has no token endpoint.
    #[error("the provider metadata has no token endpoint")]
    NoTokenEndpoint,

    /// Google answered the code exchange with an OAuth error (for example an
    /// invalid or already used code).
    #[error("Google rejected the code exchange: {0}")]
    Rejected(String),

    /// The token endpoint could not be reached or answered with something
    /// that is not a token response.
    #[error("token request failed: {0}")]
    TokenRequest(String),

    /// The token response has no ID token.
    #[error("the token response has no ID token")]
    MissingIdToken,

    /// The ID token failed verification (signature, issuer, audience,
    /// expiry or nonce).
    #[error("invalid ID token: {0}")]
    InvalidIdToken(#[source] openidconnect::ClaimsVerificationError),
}

/// Google-specific claims of the ID token.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct GoogleClaims {
    /// Google Workspace domain of the account; absent for consumer accounts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hd: Option<String>,
}

impl AdditionalClaims for GoogleClaims {}

type GoogleIdTokenFields = IdTokenFields<
    GoogleClaims,
    EmptyExtraTokenFields,
    CoreGenderClaim,
    CoreJweContentEncryptionAlgorithm,
    CoreJwsSigningAlgorithm,
>;

/// Token response of the Google token endpoint, with [`GoogleClaims`].
pub type GoogleTokenResponse = StandardTokenResponse<GoogleIdTokenFields, CoreTokenType>;

/// Google ID token, with [`GoogleClaims`].
pub type GoogleIdToken = IdToken<
    GoogleClaims,
    CoreGenderClaim,
    CoreJweContentEncryptionAlgorithm,
    CoreJwsSigningAlgorithm,
>;

type GoogleClient<HasTokenUrl = EndpointSet> = Client<
    GoogleClaims,
    CoreAuthDisplay,
    CoreGenderClaim,
    CoreJweContentEncryptionAlgorithm,
    CoreJsonWebKey,
    CoreAuthPrompt,
    StandardErrorResponse<CoreErrorResponseType>,
    GoogleTokenResponse,
    CoreTokenIntrospectionResponse,
    CoreRevocableToken,
    CoreRevocationErrorResponse,
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    HasTokenUrl,
    EndpointMaybeSet,
>;

/// An authorization request ready to be sent to Google.
pub struct AuthorizeRequest {
    /// URL of Google's authorization endpoint, with all parameters.
    pub url: Url,
    /// Random `state` included in `url`.
    pub state: CsrfToken,
    /// Random nonce included in `url`, expected back in the ID token.
    pub nonce: Nonce,
    /// PKCE verifier matching the challenge included in `url`.
    pub pkce_verifier: PkceCodeVerifier,
}

/// Identity and credentials obtained from a successful code exchange.
#[derive(Debug)]
pub struct GoogleIdentity {
    /// Stable Google account id (`sub`).
    pub sub: String,
    /// Email address, if Google returned one.
    pub email: Option<String>,
    /// True only when Google states that the email is verified.
    pub email_verified: bool,
    /// Workspace domain (`hd` claim), absent for consumer accounts.
    pub hd: Option<String>,
    /// Full name, if Google returned one.
    pub name: Option<String>,
    /// Google refresh token; Google sends it only on a consent screen.
    pub refresh_token: Option<RefreshToken>,
    /// Scopes granted, when the token response lists them.
    pub scopes: Option<Vec<String>>,
}

/// Google OpenID Connect client: builds authorization URLs and exchanges
/// authorization codes for a verified identity.
pub struct GoogleOidc {
    client: GoogleClient,
    http: reqwest::Client,
    client_id: ClientId,
    client_secret: ClientSecret,
    issuer: IssuerUrl,
    jwks_url: JsonWebKeySetUrl,
    signing_algs: Vec<CoreJwsSigningAlgorithm>,
}

impl GoogleOidc {
    /// Runs OpenID discovery on `config.google_issuer_url` and builds the
    /// client. The redirect URI is `public_base_url` followed by
    /// `redirect_path`.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the HTTP client cannot be built, a URL in the
    /// configuration is invalid, the discovery document or the JWKS cannot
    /// be fetched or parsed, the issuer in the document differs from the
    /// configured one, or the document has no token endpoint.
    pub async fn discover(config: &Config, redirect_path: &str) -> Result<Self, GoogleError> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(GoogleError::HttpClient)?;
        let issuer =
            IssuerUrl::new(config.google_issuer_url.clone()).map_err(GoogleError::BadUrl)?;
        let metadata = CoreProviderMetadata::discover_async(issuer, &http)
            .await
            .map_err(|err| GoogleError::Discovery(error_chain(&err)))?;
        Self::from_metadata(config, redirect_path, metadata, http)
    }

    /// Builds the client from already fetched provider metadata.
    ///
    /// No request is made: the JWKS is fetched at each code exchange.
    ///
    /// # Errors
    ///
    /// Returns `Err` when the redirect URL is invalid or the metadata has no
    /// token endpoint.
    pub fn from_metadata(
        config: &Config,
        redirect_path: &str,
        metadata: CoreProviderMetadata,
        http: reqwest::Client,
    ) -> Result<Self, GoogleError> {
        let client_id = ClientId::new(config.google_client_id.clone());
        let client_secret = ClientSecret::new(config.google_client_secret.clone());
        let redirect_url = RedirectUrl::new(format!("{}{redirect_path}", config.public_base_url))
            .map_err(GoogleError::BadUrl)?;
        let token_url = metadata
            .token_endpoint()
            .cloned()
            .ok_or(GoogleError::NoTokenEndpoint)?;
        let issuer = metadata.issuer().clone();
        let jwks_url = metadata.jwks_uri().clone();
        let signing_algs = metadata.id_token_signing_alg_values_supported().clone();

        let client = GoogleClient::<EndpointMaybeSet>::from_provider_metadata(
            metadata,
            client_id.clone(),
            Some(client_secret.clone()),
        )
        .set_token_uri(token_url)
        .set_redirect_uri(redirect_url);

        Ok(Self {
            client,
            http,
            client_id,
            client_secret,
            issuer,
            jwks_url,
            signing_algs,
        })
    }

    /// Builds an authorization URL with fresh `state`, nonce and PKCE `S256`
    /// challenge.
    ///
    /// Requests `openid` plus `scopes`, offline access and the consent
    /// prompt (Google returns a refresh token only on consent). `hd` is
    /// passed as a hint for the account chooser; it restricts nothing, so
    /// the `hd` claim must still be checked on the ID token.
    pub fn authorize_request(&self, scopes: &[&str], hd: &str) -> AuthorizeRequest {
        let (challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let mut request = self
            .client
            .authorize_url(
                AuthenticationFlow::<CoreResponseType>::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .set_pkce_challenge(challenge)
            .add_prompt(CoreAuthPrompt::Consent)
            .add_extra_param("access_type", "offline")
            .add_extra_param("hd", hd);
        for scope in scopes {
            request = request.add_scope(Scope::new((*scope).to_string()));
        }
        let (url, state, nonce) = request.url();
        AuthorizeRequest {
            url,
            state,
            nonce,
            pkce_verifier,
        }
    }

    /// Exchanges an authorization code and verifies the returned ID token.
    ///
    /// The ID token signature is checked against the JWKS downloaded now,
    /// together with issuer, audience, expiry and `nonce`. Claims such as
    /// `email_verified` and `hd` are returned, not enforced.
    ///
    /// # Errors
    ///
    /// Returns `Err` when:
    /// - Google rejects the exchange (`GoogleError::Rejected`);
    /// - the token endpoint is unreachable or answers garbage
    ///   (`GoogleError::TokenRequest`);
    /// - the response has no ID token (`GoogleError::MissingIdToken`);
    /// - the JWKS cannot be fetched (`GoogleError::Discovery`);
    /// - the ID token fails verification (`GoogleError::InvalidIdToken`).
    pub async fn exchange_code(
        &self,
        code: String,
        pkce_verifier: PkceCodeVerifier,
        nonce: &Nonce,
    ) -> Result<GoogleIdentity, GoogleError> {
        let response = self
            .client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(&self.http)
            .await
            .map_err(token_request_error)?;
        let id_token = response.id_token().ok_or(GoogleError::MissingIdToken)?;

        let jwks = JsonWebKeySet::<CoreJsonWebKey>::fetch_async(&self.jwks_url, &self.http)
            .await
            .map_err(|err| GoogleError::Discovery(error_chain(&err)))?;
        let verifier = IdTokenVerifier::new_confidential_client(
            self.client_id.clone(),
            self.client_secret.clone(),
            self.issuer.clone(),
            jwks,
        )
        .set_allowed_algs(self.signing_algs.clone());
        let claims = id_token
            .claims(&verifier, nonce)
            .map_err(GoogleError::InvalidIdToken)?;

        let name = claims
            .name()
            .and_then(|names| names.get(None))
            .map(|name| name.as_str().to_string());
        let scopes = response
            .scopes()
            .map(|scopes| scopes.iter().map(|scope| scope.to_string()).collect());
        Ok(GoogleIdentity {
            sub: claims.subject().as_str().to_string(),
            email: claims.email().map(|email| email.as_str().to_string()),
            email_verified: claims.email_verified() == Some(true),
            hd: claims.additional_claims().hd.clone(),
            name,
            refresh_token: response.refresh_token().cloned(),
            scopes,
        })
    }
}

impl fmt::Debug for GoogleOidc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GoogleOidc")
            .field("issuer", &self.issuer.as_str())
            .finish_non_exhaustive()
    }
}

/// Maps a token request failure without ever including the response body,
/// which may contain tokens.
fn token_request_error<RE, T>(err: RequestTokenError<RE, T>) -> GoogleError
where
    RE: std::error::Error + 'static,
    T: openidconnect::ErrorResponse + 'static,
{
    match err {
        RequestTokenError::ServerResponse(response) => GoogleError::Rejected(response.to_string()),
        RequestTokenError::Request(source) => GoogleError::TokenRequest(error_chain(&source)),
        RequestTokenError::Parse(source, _body) => {
            GoogleError::TokenRequest(format!("unparsable response: {source}"))
        }
        RequestTokenError::Other(message) => GoogleError::TokenRequest(message),
    }
}

/// Joins an error and its sources into one line.
fn error_chain(err: &dyn std::error::Error) -> String {
    let mut message = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        message = format!("{message}: {cause}");
        source = cause.source();
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config::from_lookup(|name| match name {
            "DATABASE_URL" => Some("postgres://u:p@localhost/x".to_string()),
            "PUBLIC_BASE_URL" => Some("https://api.example.com".to_string()),
            "GOOGLE_CLIENT_ID" => Some("client-id".to_string()),
            "GOOGLE_CLIENT_SECRET" => Some("secret".to_string()),
            "ALLOWED_HD" => Some("example.com".to_string()),
            "TOKEN_ENC_KEY" => Some("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=".to_string()),
            "JWT_SIGNING_KEY" => Some("0123456789abcdef0123456789abcdef".to_string()),
            _ => None,
        })
        .unwrap()
    }

    fn metadata(token_endpoint: Option<&str>) -> CoreProviderMetadata {
        let mut doc = serde_json::json!({
            "issuer": "https://issuer.example.com",
            "authorization_endpoint": "https://issuer.example.com/authorize",
            "jwks_uri": "https://issuer.example.com/jwks",
            "response_types_supported": ["code"],
            "subject_types_supported": ["public"],
            "id_token_signing_alg_values_supported": ["RS256"],
        });
        if let Some(url) = token_endpoint {
            doc["token_endpoint"] = serde_json::json!(url);
        }
        serde_json::from_value(doc).unwrap()
    }

    fn oidc() -> GoogleOidc {
        GoogleOidc::from_metadata(
            &config(),
            "/auth/google/callback",
            metadata(Some("https://issuer.example.com/token")),
            reqwest::Client::new(),
        )
        .unwrap()
    }

    fn query(url: &Url) -> Vec<(String, String)> {
        url.query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    fn param<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
        pairs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn from_metadata_requires_token_endpoint() {
        let result =
            GoogleOidc::from_metadata(&config(), "/cb", metadata(None), reqwest::Client::new());

        assert!(matches!(result, Err(GoogleError::NoTokenEndpoint)));
    }

    #[test]
    fn authorize_url_points_to_authorization_endpoint() {
        let request = oidc().authorize_request(&["email"], "example.com");

        assert_eq!(
            request.url.as_str().split('?').next(),
            Some("https://issuer.example.com/authorize")
        );
    }

    #[test]
    fn authorize_url_carries_offline_consent_and_hd() {
        let request = oidc().authorize_request(&["email", "profile"], "example.com");
        let pairs = query(&request.url);

        assert_eq!(param(&pairs, "access_type"), Some("offline"));
        assert_eq!(param(&pairs, "prompt"), Some("consent"));
        assert_eq!(param(&pairs, "hd"), Some("example.com"));
        assert_eq!(param(&pairs, "scope"), Some("openid email profile"));
        assert_eq!(param(&pairs, "response_type"), Some("code"));
        assert_eq!(param(&pairs, "client_id"), Some("client-id"));
        assert_eq!(
            param(&pairs, "redirect_uri"),
            Some("https://api.example.com/auth/google/callback")
        );
    }

    #[test]
    fn authorize_url_carries_state_nonce_and_pkce() {
        let request = oidc().authorize_request(&[], "example.com");
        let pairs = query(&request.url);

        assert_eq!(
            param(&pairs, "state"),
            Some(request.state.secret().as_str())
        );
        assert_eq!(
            param(&pairs, "nonce"),
            Some(request.nonce.secret().as_str())
        );
        assert_eq!(param(&pairs, "code_challenge_method"), Some("S256"));
        let challenge = param(&pairs, "code_challenge").unwrap();
        assert!(crate::crypto::pkce_s256_matches(
            request.pkce_verifier.secret(),
            challenge
        ));
    }

    #[test]
    fn error_chain_joins_sources() {
        let inner = std::io::Error::other("inner");
        let outer = GoogleError::Discovery(error_chain(&inner));

        assert_eq!(outer.to_string(), "OpenID discovery failed: inner");
    }
}
