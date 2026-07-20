use crate::models::TokenSet;
use oauth2::{
    basic::BasicClient, AuthType, AuthUrl, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use rand::rngs::OsRng;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;
use thiserror::Error;
use url::Url;

#[derive(Error, Debug)]
pub enum AuthError {
    #[error("OAuth error: {0}")]
    OAuth(#[from] oauth2::RequestTokenError),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("URL parse error: {0}")]
    UrlParse(#[from] url::ParseError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Other error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, AuthError>;

/// OAuth2 configuration for xAI auth
pub struct XaiAuthConfig {
    pub client_id: String,
    pub auth_url: String,
    pub token_url: String,
    pub redirect_uri: String,
}

impl Default for XaiAuthConfig {
    fn default() -> Self {
        Self {
            // These should match the official CLI's configuration
            client_id: "grok-build-cli".to_string(),
            auth_url: "https://auth.x.ai/oauth/authorize".to_string(),
            token_url: "https://auth.x.ai/oauth/token".to_string(),
            redirect_uri: "http://127.0.0.1:0/callback".to_string(), // Port will be assigned dynamically
        }
    }
}

/// PKCE code verifier and challenge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PkceState {
    pub code_verifier: String,
    pub code_challenge: String,
    pub csrf_token: String,
}

impl PkceState {
    pub fn generate() -> Result<Self> {
        let code_verifier = PkceCodeChallenge::new_random_sha256();
        let (challenge, _verifier) = (code_verifier.clone(), code_verifier);
        
        // Generate CSRF token
        let csrf_token = CsrfToken::new_random().secret().to_string();
        
        Ok(Self {
            code_verifier: code_verifier.secret().to_string(),
            code_challenge: challenge.as_str().to_string(),
            csrf_token,
        })
    }
}

/// Auth service for handling OAuth2 flows
pub struct AuthService {
    config: XaiAuthConfig,
    http_client: Client,
}

impl AuthService {
    pub fn new(config: XaiAuthConfig) -> Self {
        Self {
            config,
            http_client: Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Generate authorization URL with PKCE
    pub fn generate_auth_url(&self) -> Result<(String, PkceState)> {
        let pkce = PkceState::generate()?;
        
        let mut auth_url = Url::parse(&self.config.auth_url)?;
        auth_url
            .query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", &self.config.client_id)
            .append_pair("redirect_uri", &self.config.redirect_uri)
            .append_pair("code_challenge", &pkce.code_challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &pkce.csrf_token)
            .append_pair("scope", "openid profile email offline_access");
        
        Ok((auth_url.to_string(), pkce))
    }

    /// Exchange authorization code for tokens
    pub async fn exchange_code(&self, code: String, _pkce_state: PkceState) -> Result<TokenSet> {
        let client = BasicClient::new(ClientId::new(self.config.client_id.clone()))
            .set_auth_type(AuthType::RequestBody)
            .set_auth_url(AuthUrl::new(self.config.auth_url.clone())?)
            .set_token_url(TokenUrl::new(self.config.token_url.clone())?);

        let token_result = client
            .exchange_code(code.into())
            .request_async(&self.http_client)
            .await?;

        let token_response = token_result.ok_or_else(|| {
            AuthError::Other("Failed to get token response".to_string())
        })?;

        Ok(TokenSet {
            access_token: token_response.access_token().secret().to_string(),
            refresh_token: token_response
                .refresh_token()
                .map(|t| t.secret().to_string()),
            expires_at: token_response
                .expires_in()
                .map(|d| chrono::Utc::now() + d),
            token_type: token_response.token_type().to_string(),
        })
    }

    /// Refresh an expired token
    pub async fn refresh_token(&self, refresh_token: String) -> Result<TokenSet> {
        let client = BasicClient::new(ClientId::new(self.config.client_id.clone()))
            .set_auth_type(AuthType::RequestBody)
            .set_auth_url(AuthUrl::new(self.config.auth_url.clone())?)
            .set_token_url(TokenUrl::new(self.config.token_url.clone())?);

        let token_result = client
            .exchange_refresh_token(refresh_token.into())
            .request_async(&self.http_client)
            .await?;

        let token_response = token_result.ok_or_else(|| {
            AuthError::Other("Failed to refresh token".to_string())
        })?;

        Ok(TokenSet {
            access_token: token_response.access_token().secret().to_string(),
            refresh_token: token_response
                .refresh_token()
                .map(|t| t.secret().to_string()),
            expires_at: token_response
                .expires_in()
                .map(|d| chrono::Utc::now() + d),
            token_type: token_response.token_type().to_string(),
        })
    }

    /// Get user info from token
    pub async fn get_user_info(&self, access_token: &str) -> Result<UserInfo> {
        let response = self
            .http_client
            .get("https://auth.x.ai/userinfo")
            .header("Authorization", format!("Bearer {}", access_token))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(AuthError::Other(format!(
                "User info request failed: {}",
                response.status()
            )));
        }

        let user_info: UserInfo = response.json().await?;
        Ok(user_info)
    }
}

/// User information from OAuth2
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub name: Option<String>,
    pub preferred_username: Option<String>,
}

/// Helper function for SHA256 hashing (for PKCE)
pub fn sha256_hash(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    base64::encode_config(result, base64::URL_SAFE_NO_PAD)
}
