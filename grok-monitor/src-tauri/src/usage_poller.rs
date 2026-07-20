use crate::account_manager::AccountManager;
use crate::models::{ModelUsage, UsageSnapshot};
use chrono::Utc;
use log::{debug, error, info, warn};
use reqwest::Client;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time;

/// Usage poller for fetching Grok account usage data
pub struct UsagePoller {
    http_client: Client,
    account_manager: std::sync::Arc<AccountManager>,
}

impl UsagePoller {
    pub fn new(account_manager: std::sync::Arc<AccountManager>) -> Self {
        Self {
            http_client: Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
            account_manager,
        }
    }

    /// Start polling for all active accounts
    pub async fn start_polling(&self) {
        let accounts = self.account_manager.get_accounts().await;
        
        for account in accounts {
            if account.is_active {
                let account_id = account.id.clone();
                let poll_interval = account.poll_interval_secs;
                
                // Spawn a task for each account
                tokio::spawn(async move {
                    // Initial fetch
                    debug!("Starting poll for account {}", account_id);
                    
                    // Create interval timer
                    let mut interval = time::interval(Duration::from_secs(poll_interval));
                    
                    loop {
                        interval.tick().await;
                        
                        // Fetch usage (implementation depends on available endpoints)
                        // This is a placeholder - actual implementation will depend on discovered APIs
                        debug!("Polling usage for account {}", account_id);
                    }
                });
            }
        }
    }

    /// Fetch usage from grok.com internal API (consumer accounts)
    pub async fn fetch_grok_com_usage(&self, access_token: &str) -> Result<UsageSnapshot, Box<dyn std::error::Error>> {
        // Note: This is based on reverse-engineered endpoints and may change
        // Actual endpoint should be verified via browser DevTools
        
        let response = self
            .http_client
            .get("https://grok.com/api/usage")
            .header("Authorization", format!("Bearer {}", access_token))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(format!("Usage request failed: {}", response.status()).into());
        }

        // Parse response - structure will depend on actual API
        let data: serde_json::Value = response.json().await?;
        
        // Extract usage information (adjust based on actual API response)
        let remaining_queries = data["remaining"]
            .as_u64()
            .map(|v| v as u32);
        let total_queries = data["total"]
            .as_u64()
            .map(|v| v as u32);
        
        let reset_at = data["reset_at"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        Ok(UsageSnapshot {
            remaining_queries,
            total_queries,
            remaining_tokens: None,
            total_tokens: None,
            reset_at,
            model_breakdown: HashMap::new(),
            raw: data,
            last_updated: Utc::now(),
        })
    }

    /// Fetch usage from x.ai console API (API accounts)
    pub async fn fetch_console_usage(&self, access_token: &str) -> Result<UsageSnapshot, Box<dyn std::error::Error>> {
        // For API team accounts using console.x.ai
        let response = self
            .http_client
            .get("https://console.x.ai/api/v1/usage")
            .header("Authorization", format!("Bearer {}", access_token))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(format!("Console usage request failed: {}", response.status()).into());
        }

        let data: serde_json::Value = response.json().await?;
        
        // Parse based on console API structure
        Ok(UsageSnapshot {
            remaining_queries: None,
            total_queries: None,
            remaining_tokens: data["remaining_tokens"].as_u64(),
            total_tokens: data["total_tokens"].as_u64(),
            reset_at: None,
            model_breakdown: HashMap::new(),
            raw: data,
            last_updated: Utc::now(),
        })
    }

    /// Fallback: Use rate limit headers from a lightweight request
    pub async fn fetch_usage_from_headers(&self, access_token: &str) -> Result<UsageSnapshot, Box<dyn std::error::Error>> {
        // Make a lightweight request to get rate limit headers
        let response = self
            .http_client
            .get("https://api.x.ai/v1/status")
            .header("Authorization", format!("Bearer {}", access_token))
            .send()
            .await?;

        let headers = response.headers();
        
        // Extract rate limit information from headers
        let remaining = headers
            .get("x-ratelimit-remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok());
        
        let limit = headers
            .get("x-ratelimit-limit")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u32>().ok());
        
        let reset_timestamp = headers
            .get("x-ratelimit-reset")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<i64>().ok())
            .map(|ts| chrono::DateTime::from_timestamp(ts, 0).unwrap_or_else(|| Utc::now()));

        Ok(UsageSnapshot {
            remaining_queries: remaining,
            total_queries: limit,
            remaining_tokens: None,
            total_tokens: None,
            reset_at: reset_timestamp,
            model_breakdown: HashMap::new(),
            raw: serde_json::json!({}),
            last_updated: Utc::now(),
        })
    }

    /// Update usage for a specific account
    pub async fn update_account_usage(&self, account_id: &str) -> Result<(), Box<dyn std::error::Error>> {
        let account = self.account_manager
            .get_account(account_id)
            .await
            .ok_or("Account not found")?;

        let access_token = &account.auth.access_token;
        
        // Try different methods based on account type
        let usage = match account.account_type {
            crate::models::AccountType::Consumer => {
                match self.fetch_grok_com_usage(access_token).await {
                    Ok(u) => u,
                    Err(_) => self.fetch_usage_from_headers(access_token).await?,
                }
            }
            crate::models::AccountType::API => {
                match self.fetch_console_usage(access_token).await {
                    Ok(u) => u,
                    Err(_) => self.fetch_usage_from_headers(access_token).await?,
                }
            }
            crate::models::AccountType::Enterprise => {
                self.fetch_usage_from_headers(access_token).await?
            }
        };

        self.account_manager.update_account_usage(account_id, usage).await?;
        info!("Updated usage for account {}", account_id);
        
        Ok(())
    }
}
