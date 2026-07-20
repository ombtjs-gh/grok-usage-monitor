use crate::auth::{AuthService, XaiAuthConfig};
use crate::models::{Account, AppSettings, TokenSet};
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AccountManagerError {
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Account not found: {0}")]
    NotFound(String),
    #[error("Auth error: {0}")]
    Auth(#[from] crate::auth::AuthError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, AccountManagerError>;

/// Account manager for handling multiple Grok accounts
pub struct AccountManager {
    accounts: Arc<RwLock<Vec<Account>>>,
    settings: Arc<RwLock<AppSettings>>,
    auth_service: Arc<AuthService>,
    storage_path: String,
}

impl AccountManager {
    pub fn new(storage_path: String) -> Self {
        let config = XaiAuthConfig::default();
        let auth_service = Arc::new(AuthService::new(config));
        
        Self {
            accounts: Arc::new(RwLock::new(Vec::new())),
            settings: Arc::new(RwLock::new(AppSettings::default())),
            auth_service,
            storage_path,
        }
    }

    /// Load accounts from storage
    pub async fn load(&self) -> Result<()> {
        // Try to load from keyring or encrypted file
        // For now, we'll use a simple JSON file approach
        let path = std::path::Path::new(&self.storage_path).join("accounts.json");
        
        if path.exists() {
            let data = tokio::fs::read_to_string(&path).await.map_err(|e| {
                AccountManagerError::Storage(format!("Failed to read accounts file: {}", e))
            })?;
            
            let accounts: Vec<Account> = serde_json::from_str(&data)?;
            *self.accounts.write().await = accounts;
            info!("Loaded {} accounts", self.accounts.read().await.len());
        }
        
        // Load settings
        let settings_path = std::path::Path::new(&self.storage_path).join("settings.json");
        if settings_path.exists() {
            let data = tokio::fs::read_to_string(&settings_path).await.ok();
            if let Some(data) = data {
                if let Ok(settings) = serde_json::from_str(&data) {
                    *self.settings.write().await = settings;
                }
            }
        }
        
        Ok(())
    }

    /// Save accounts to storage
    pub async fn save(&self) -> Result<()> {
        let path = std::path::Path::new(&self.storage_path).join("accounts.json");
        
        // Ensure directory exists
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                AccountManagerError::Storage(format!("Failed to create storage directory: {}", e))
            })?;
        }
        
        let data = serde_json::to_string_pretty(&*self.accounts.read().await)?;
        tokio::fs::write(&path, data).await.map_err(|e| {
            AccountManagerError::Storage(format!("Failed to write accounts file: {}", e))
        })?;
        
        // Set file permissions to 0600 (Unix only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = std::fs::metadata(&path) {
                let mut permissions = metadata.permissions();
                permissions.set_mode(0o600);
                let _ = std::fs::set_permissions(&path, permissions);
            }
        }
        
        info!("Saved {} accounts", self.accounts.read().await.len());
        Ok(())
    }

    /// Save settings
    pub async fn save_settings(&self) -> Result<()> {
        let path = std::path::Path::new(&self.storage_path).join("settings.json");
        let data = serde_json::to_string_pretty(&*self.settings.read().await)?;
        tokio::fs::write(&path, data).await.map_err(|e| {
            AccountManagerError::Storage(format!("Failed to write settings file: {}", e))
        })?;
        Ok(())
    }

    /// Add a new account
    pub async fn add_account(&self, display_name: String, email: Option<String>, auth: TokenSet) -> Result<Account> {
        let mut account = Account::new(display_name, email, auth);
        let accounts = &mut *self.accounts.write().await;
        accounts.push(account.clone());
        self.save().await?;
        info!("Added account: {}", account.id);
        Ok(account)
    }

    /// Remove an account
    pub async fn remove_account(&self, account_id: &str) -> Result<()> {
        let mut accounts = self.accounts.write().await;
        let initial_len = accounts.len();
        accounts.retain(|a| a.id != account_id);
        
        if accounts.len() < initial_len {
            self.save().await?;
            info!("Removed account: {}", account_id);
            Ok(())
        } else {
            Err(AccountManagerError::NotFound(account_id.to_string()))
        }
    }

    /// Get all accounts
    pub async fn get_accounts(&self) -> Vec<Account> {
        self.accounts.read().await.clone()
    }

    /// Get account by ID
    pub async fn get_account(&self, account_id: &str) -> Option<Account> {
        self.accounts
            .read()
            .await
            .iter()
            .find(|a| a.id == account_id)
            .cloned()
    }

    /// Update account usage data
    pub async fn update_account_usage(&self, account_id: &str, usage: crate::models::UsageSnapshot) -> Result<()> {
        let mut accounts = self.accounts.write().await;
        if let Some(account) = accounts.iter_mut().find(|a| a.id == account_id) {
            account.usage = Some(usage);
            account.last_polled = Some(chrono::Utc::now());
            self.save().await?;
            Ok(())
        } else {
            Err(AccountManagerError::NotFound(account_id.to_string()))
        }
    }

    /// Get settings
    pub async fn get_settings(&self) -> AppSettings {
        self.settings.read().await.clone()
    }

    /// Update settings
    pub async fn update_settings(&self, settings: AppSettings) -> Result<()> {
        *self.settings.write().await = settings;
        self.save_settings().await?;
        Ok(())
    }

    /// Get auth service reference
    pub fn get_auth_service(&self) -> Arc<AuthService> {
        self.auth_service.clone()
    }

    /// Refresh tokens for all accounts that need it
    pub async fn refresh_expired_tokens(&self) -> Result<()> {
        let accounts = self.accounts.read().await.clone();
        
        for account in accounts {
            if account.auth.should_refresh() {
                if let Some(refresh_token) = &account.auth.refresh_token {
                    info!("Refreshing token for account: {}", account.id);
                    
                    match self.auth_service.refresh_token(refresh_token.clone()).await {
                        Ok(new_token_set) => {
                            self.update_account_auth(account.id.as_str(), new_token_set).await?;
                        }
                        Err(e) => {
                            error!("Failed to refresh token for {}: {}", account.id, e);
                        }
                    }
                }
            }
        }
        
        Ok(())
    }

    /// Update account authentication
    async fn update_account_auth(&self, account_id: &str, auth: TokenSet) -> Result<()> {
        let mut accounts = self.accounts.write().await;
        if let Some(account) = accounts.iter_mut().find(|a| a.id == account_id) {
            account.auth = auth;
            self.save().await?;
            Ok(())
        } else {
            Err(AccountManagerError::NotFound(account_id.to_string()))
        }
    }
}
