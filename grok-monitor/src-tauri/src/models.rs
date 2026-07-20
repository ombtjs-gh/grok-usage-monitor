use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

/// Token set for OAuth2 authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub token_type: String,
}

impl TokenSet {
    pub fn is_expired(&self) -> bool {
        if let Some(expires) = self.expires_at {
            Utc::now() >= expires
        } else {
            false
        }
    }
    
    pub fn should_refresh(&self) -> bool {
        if let Some(expires) = self.expires_at {
            // Refresh 5 minutes before expiration
            let threshold = Utc::now() + chrono::Duration::minutes(5);
            threshold >= expires
        } else {
            false
        }
    }
}

/// Usage snapshot for a Grok account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSnapshot {
    pub remaining_queries: Option<u32>,
    pub total_queries: Option<u32>,
    pub remaining_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub reset_at: Option<DateTime<Utc>>,
    pub model_breakdown: HashMap<String, ModelUsage>,
    pub raw: serde_json::Value,
    pub last_updated: DateTime<Utc>,
}

impl UsageSnapshot {
    pub fn usage_percentage(&self) -> Option<f64> {
        match (self.remaining_queries, self.total_queries) {
            (Some(remaining), Some(total)) if total > 0 => {
                Some(((total - remaining) as f64 / total as f64) * 100.0)
            }
            _ => None,
        }
    }
    
    pub fn is_low_usage(&self, threshold: f64) -> bool {
        self.usage_percentage().map_or(false, |pct| pct > threshold)
    }
}

/// Model-specific usage breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelUsage {
    pub model_name: String,
    pub queries_used: u32,
    pub tokens_used: Option<u64>,
}

/// Account representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub display_name: String,
    pub email: Option<String>,
    pub auth: TokenSet,
    pub usage: Option<UsageSnapshot>,
    pub last_polled: Option<DateTime<Utc>>,
    pub poll_interval_secs: u64,
    pub is_active: bool,
    pub account_type: AccountType,
}

impl Account {
    pub fn new(display_name: String, email: Option<String>, auth: TokenSet) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            display_name,
            email,
            auth,
            usage: None,
            last_polled: None,
            poll_interval_secs: 30,
            is_active: true,
            account_type: AccountType::Consumer,
        }
    }
}

/// Account type enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccountType {
    Consumer,    // grok.com
    API,         // console.x.ai
    Enterprise,  // SSO/OIDC
}

/// Application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub opacity: f64,
    pub always_on_top: bool,
    pub default_poll_interval: u64,
    pub theme: Theme,
    pub compact_mode: bool,
    pub auto_start: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            opacity: 0.9,
            always_on_top: true,
            default_poll_interval: 30,
            theme: Theme::Dark,
            compact_mode: true,
            auto_start: false,
        }
    }
}

/// UI Theme
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Theme {
    Dark,
    Light,
    System,
}
