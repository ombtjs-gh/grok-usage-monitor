//! Local persistence: settings on disk, account tokens in the OS keychain.

use crate::auth::AccountTokens;
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

const KEYRING_SERVICE: &str = "com.grok.usage-monitor";
const KEYRING_USER: &str = "accounts";

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("keychain error: {0}")]
    Keyring(String),
}

impl From<keyring::Error> for StoreError {
    fn from(e: keyring::Error) -> Self {
        StoreError::Keyring(e.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// Window opacity 0.0–1.0
    pub opacity: f64,
    pub always_on_top: bool,
    /// Auto-refresh interval in minutes
    pub refresh_interval_minutes: u64,
    /// Currently selected account id (user_id)
    pub selected_account_id: Option<String>,
    pub theme: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            opacity: 0.92,
            always_on_top: true,
            refresh_interval_minutes: 10,
            selected_account_id: None,
            theme: "dark".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountStore {
    pub accounts: Vec<AccountTokens>,
}

pub fn data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".grok-monitor")
}

fn ensure_dir() -> Result<PathBuf, StoreError> {
    let dir = data_dir();
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn accounts_path() -> PathBuf {
    data_dir().join("accounts.json")
}

fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

fn keyring_entry() -> Result<Entry, StoreError> {
    Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(StoreError::from)
}

/// Load accounts: prefer OS keychain, migrate legacy plaintext file if present.
pub fn load_accounts() -> Result<AccountStore, StoreError> {
    // 1) Keychain
    match keyring_entry() {
        Ok(entry) => match entry.get_password() {
            Ok(raw) if !raw.trim().is_empty() => {
                return Ok(serde_json::from_str(&raw)?);
            }
            Ok(_) | Err(keyring::Error::NoEntry) => {}
            Err(e) => {
                // Fall through to file if keychain is unavailable (e.g. headless Linux).
                eprintln!("keychain read failed, trying file fallback: {e}");
            }
        },
        Err(e) => {
            eprintln!("keychain unavailable, trying file fallback: {e}");
        }
    }

    // 2) Legacy plaintext file → migrate into keychain
    let path = accounts_path();
    if path.exists() {
        let raw = fs::read_to_string(&path)?;
        let store: AccountStore = serde_json::from_str(&raw)?;
        match save_accounts_keyring(&store) {
            Ok(()) => {
                // Remove plaintext copy after successful migration.
                let _ = fs::remove_file(&path);
                let legacy = data_dir().join("accounts.json.migrated");
                let _ = fs::write(
                    legacy,
                    "Accounts were migrated to the OS keychain. Safe to delete this marker.\n",
                );
            }
            Err(e) => {
                eprintln!("keychain migration failed, keeping file store: {e}");
            }
        }
        return Ok(store);
    }

    Ok(AccountStore::default())
}

fn save_accounts_keyring(store: &AccountStore) -> Result<(), StoreError> {
    let entry = keyring_entry()?;
    if store.accounts.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(StoreError::from(e)),
        }
    } else {
        let raw = serde_json::to_string(store)?;
        entry.set_password(&raw)?;
        Ok(())
    }
}

fn save_accounts_file(store: &AccountStore) -> Result<(), StoreError> {
    ensure_dir()?;
    let path = accounts_path();
    if store.accounts.is_empty() {
        let _ = fs::remove_file(&path);
        return Ok(());
    }
    let raw = serde_json::to_string_pretty(store)?;
    fs::write(&path, raw)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Persist accounts to the OS keychain (file fallback if keychain fails).
pub fn save_accounts(store: &AccountStore) -> Result<(), StoreError> {
    match save_accounts_keyring(store) {
        Ok(()) => {
            // Prefer keychain-only storage; remove any leftover plaintext file.
            let _ = fs::remove_file(accounts_path());
            Ok(())
        }
        Err(e) => {
            eprintln!("keychain write failed, using file fallback: {e}");
            save_accounts_file(store)
        }
    }
}

pub fn load_settings() -> Result<AppSettings, StoreError> {
    let path = settings_path();
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let raw = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn save_settings(settings: &AppSettings) -> Result<(), StoreError> {
    ensure_dir()?;
    let raw = serde_json::to_string_pretty(settings)?;
    fs::write(settings_path(), raw)?;
    Ok(())
}

/// Upsert accounts by user_id, return the merged store.
pub fn upsert_accounts(incoming: Vec<AccountTokens>) -> Result<AccountStore, StoreError> {
    let mut store = load_accounts()?;
    for acc in incoming {
        if let Some(existing) = store.accounts.iter_mut().find(|a| a.user_id == acc.user_id) {
            *existing = acc;
        } else {
            store.accounts.push(acc);
        }
    }
    save_accounts(&store)?;
    Ok(store)
}

pub fn remove_account(user_id: &str) -> Result<AccountStore, StoreError> {
    let mut store = load_accounts()?;
    store.accounts.retain(|a| a.user_id != user_id);
    save_accounts(&store)?;

    let mut settings = load_settings()?;
    if settings.selected_account_id.as_deref() == Some(user_id) {
        settings.selected_account_id = store.accounts.first().map(|a| a.user_id.clone());
        save_settings(&settings)?;
    }
    Ok(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_store_roundtrip_json() {
        let store = AccountStore { accounts: vec![] };
        let s = serde_json::to_string(&store).unwrap();
        let back: AccountStore = serde_json::from_str(&s).unwrap();
        assert!(back.accounts.is_empty());
    }
}
