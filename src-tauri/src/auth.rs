//! OAuth2 (PKCE) + Grok CLI auth.json import + token refresh.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration as StdDuration;
use thiserror::Error;

pub const XAI_ISSUER: &str = "https://auth.x.ai";
pub const XAI_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
pub const TOKEN_HEADER: &str = "xai-grok-cli";
pub const TOKEN_ENDPOINT: &str = "https://auth.x.ai/oauth2/token";
pub const AUTHORIZE_ENDPOINT: &str = "https://auth.x.ai/oauth2/authorize";
pub const USERINFO_ENDPOINT: &str = "https://auth.x.ai/oauth2/userinfo";

/// Default scopes used by Grok Build CLI (frozen contract).
pub const DEFAULT_SCOPES: &[&str] = &[
    "openid",
    "profile",
    "email",
    "offline_access",
    "grok-cli:access",
    "api:access",
    "conversations:read",
    "conversations:write",
    "workspaces:read",
    "workspaces:write",
];

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("auth failed: {0}")]
    Message(String),
    #[error("timeout waiting for OAuth callback")]
    Timeout,
    #[error("cancelled")]
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub user_id: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub client_id: String,
    pub issuer: String,
    /// Source of this credential (import / oauth).
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub id: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub source: String,
    pub expires_at: Option<DateTime<Utc>>,
}

impl AccountTokens {
    pub fn id(&self) -> String {
        self.user_id.clone()
    }

    pub fn summary(&self) -> AccountSummary {
        AccountSummary {
            id: self.id(),
            email: self.email.clone(),
            display_name: self.display_name.clone(),
            source: self.source.clone(),
            expires_at: self.expires_at,
        }
    }

    pub fn is_expired_or_near(&self, skew_secs: i64) -> bool {
        match self.expires_at {
            Some(exp) => Utc::now() + Duration::seconds(skew_secs) >= exp,
            None => false,
        }
    }
}

/// Shape of a single entry in `~/.grok/auth.json`.
#[derive(Debug, Deserialize)]
struct GrokAuthEntry {
    key: String,
    #[serde(default)]
    user_id: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    first_name: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_at: Option<String>,
    #[serde(default)]
    oidc_issuer: Option<String>,
    #[serde(default)]
    oidc_client_id: Option<String>,
}

fn grok_auth_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".grok")
        .join("auth.json")
}

/// Import accounts from the local Grok Build CLI credential store.
pub fn import_from_grok_cli() -> Result<Vec<AccountTokens>, AuthError> {
    let path = grok_auth_path();
    if !path.exists() {
        return Err(AuthError::Message(format!(
            "Grok CLI auth not found at {}. Run `grok login` first, or use OAuth login.",
            path.display()
        )));
    }

    let raw = std::fs::read_to_string(&path)?;
    let map: HashMap<String, GrokAuthEntry> = serde_json::from_str(&raw)?;
    let mut accounts = Vec::new();

    for (_scope, entry) in map {
        let user_id = match entry.user_id.filter(|s| !s.is_empty()) {
            Some(id) => id,
            None => continue,
        };
        let expires_at = entry
            .expires_at
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        accounts.push(AccountTokens {
            access_token: entry.key,
            refresh_token: entry.refresh_token,
            expires_at,
            user_id,
            email: entry.email,
            display_name: entry.first_name,
            client_id: entry
                .oidc_client_id
                .unwrap_or_else(|| XAI_CLIENT_ID.to_string()),
            issuer: entry
                .oidc_issuer
                .unwrap_or_else(|| XAI_ISSUER.to_string()),
            source: "grok-cli".into(),
        });
    }

    if accounts.is_empty() {
        return Err(AuthError::Message(
            "No usable OAuth sessions found in ~/.grok/auth.json".into(),
        ));
    }

    Ok(accounts)
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserInfo {
    sub: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    given_name: Option<String>,
}

/// Refresh an access token using the refresh_token grant.
pub async fn refresh_tokens(account: &AccountTokens) -> Result<AccountTokens, AuthError> {
    let refresh = account
        .refresh_token
        .as_ref()
        .ok_or_else(|| AuthError::Message("No refresh_token available; re-login required".into()))?;

    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_ENDPOINT)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.as_str()),
            ("client_id", account.client_id.as_str()),
        ])
        .send()
        .await?;

    let status = resp.status();
    let body: TokenResponse = resp.json().await?;
    if !status.is_success() || body.error.is_some() {
        return Err(AuthError::Message(format!(
            "Token refresh failed: {}",
            body.error_description
                .or(body.error)
                .unwrap_or_else(|| status.to_string())
        )));
    }

    let expires_at = body
        .expires_in
        .map(|secs| Utc::now() + Duration::seconds(secs));

    let mut updated = account.clone();
    updated.access_token = body.access_token;
    if let Some(rt) = body.refresh_token {
        updated.refresh_token = Some(rt);
    }
    updated.expires_at = expires_at;
    Ok(updated)
}

/// Ensure a fresh access token (refresh if near expiry).
pub async fn ensure_fresh(account: &AccountTokens) -> Result<AccountTokens, AuthError> {
    if account.is_expired_or_near(120) {
        refresh_tokens(account).await
    } else {
        Ok(account.clone())
    }
}

fn random_url_safe(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

fn pkce_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

/// Run Authorization Code + PKCE via loopback redirect.
pub async fn login_with_pkce() -> Result<AccountTokens, AuthError> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(false)?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    let code_verifier = random_url_safe(64);
    let code_challenge = pkce_challenge(&code_verifier);
    let state = random_url_safe(16);

    let auth_url = format!(
        "{AUTHORIZE_ENDPOINT}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256&referrer=grok-usage-monitor",
        urlencoding::encode(XAI_CLIENT_ID),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(&DEFAULT_SCOPES.join(" ")),
        urlencoding::encode(&state),
        urlencoding::encode(&code_challenge),
    );

    tauri_plugin_opener::open_url(&auth_url, None::<&str>)
        .map_err(|e| AuthError::Message(format!("Failed to open browser: {e}")))?;

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = accept_oauth_callback(listener, &state);
        let _ = tx.send(result);
    });

    let code = tokio::task::spawn_blocking(move || {
        rx.recv_timeout(StdDuration::from_secs(300))
            .map_err(|_| AuthError::Timeout)?
    })
    .await
    .map_err(|e| AuthError::Message(e.to_string()))??;

    exchange_code(&code, &redirect_uri, &code_verifier).await
}

fn accept_oauth_callback(listener: TcpListener, expected_state: &str) -> Result<String, AuthError> {
    let (mut stream, _) = listener.accept()?;
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf)?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let first_line = req.lines().next().unwrap_or("");
    let path = first_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| AuthError::Message("Malformed OAuth callback".into()))?;

    let url = format!("http://127.0.0.1{path}");
    let parsed = url::Url::parse(&url).map_err(|e| AuthError::Message(e.to_string()))?;
    let pairs: HashMap<_, _> = parsed.query_pairs().into_owned().collect();

    if let Some(err) = pairs.get("error") {
        let desc = pairs
            .get("error_description")
            .cloned()
            .unwrap_or_default();
        let body = html_page(
            "Login failed",
            &format!("OAuth error: {err} {desc}. You can close this tab."),
        );
        let _ = stream.write_all(body.as_bytes());
        return Err(AuthError::Message(format!("OAuth error: {err} {desc}")));
    }

    let code = pairs
        .get("code")
        .cloned()
        .ok_or_else(|| AuthError::Message("No authorization code in callback".into()))?;
    let state = pairs.get("state").map(String::as_str).unwrap_or("");
    if state != expected_state {
        let body = html_page("Login failed", "Invalid state parameter. Close this tab.");
        let _ = stream.write_all(body.as_bytes());
        return Err(AuthError::Message("OAuth state mismatch".into()));
    }

    let body = html_page(
        "Login successful",
        "You can close this tab and return to Grok Usage Monitor.",
    );
    let _ = stream.write_all(body.as_bytes());
    Ok(code)
}

fn html_page(title: &str, message: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\n\r\n\
         <!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title>\
         <style>body{{font-family:system-ui;background:#0b0b0f;color:#eee;display:flex;\
         align-items:center;justify-content:center;height:100vh;margin:0}}\
         .card{{background:#16161d;padding:2rem 2.5rem;border-radius:12px;max-width:28rem;\
         text-align:center;border:1px solid #2a2a35}}</style></head>\
         <body><div class=\"card\"><h1>{title}</h1><p>{message}</p></div></body></html>"
    )
}

async fn exchange_code(
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> Result<AccountTokens, AuthError> {
    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_ENDPOINT)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", XAI_CLIENT_ID),
            ("code_verifier", code_verifier),
        ])
        .send()
        .await?;

    let status = resp.status();
    let body: TokenResponse = resp.json().await?;
    if !status.is_success() || body.error.is_some() {
        return Err(AuthError::Message(format!(
            "Token exchange failed: {}",
            body.error_description
                .or(body.error)
                .unwrap_or_else(|| status.to_string())
        )));
    }

    let expires_at = body
        .expires_in
        .map(|secs| Utc::now() + Duration::seconds(secs));

    let userinfo: UserInfo = client
        .get(USERINFO_ENDPOINT)
        .bearer_auth(&body.access_token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(AccountTokens {
        access_token: body.access_token,
        refresh_token: body.refresh_token,
        expires_at,
        user_id: userinfo.sub,
        email: userinfo.email,
        display_name: userinfo.name.or(userinfo.given_name),
        client_id: XAI_CLIENT_ID.to_string(),
        issuer: XAI_ISSUER.to_string(),
        source: "oauth".into(),
    })
}
