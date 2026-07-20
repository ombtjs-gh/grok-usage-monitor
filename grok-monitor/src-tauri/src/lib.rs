use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub mod auth;
pub mod account_manager;
pub mod usage_poller;
pub mod models;

use crate::models::{Account, UsageSnapshot};
use crate::account_manager::AccountManager;
use crate::auth::{AuthService, XaiAuthConfig, UserInfo};
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

// App state to hold account manager
pub struct AppState {
    pub account_manager: Arc<Mutex<AccountManager>>,
}

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// Get all accounts
#[tauri::command]
async fn get_accounts(state: State<'_, Arc<AppState>>) -> Vec<Account> {
    let manager = state.account_manager.lock().await;
    manager.get_accounts().await
}

/// Add a new account (starts OAuth flow)
#[tauri::command]
async fn start_oauth_login(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let auth_service = AuthService::new(XaiAuthConfig::default());
    match auth_service.generate_auth_url() {
        Ok((url, pkce_state)) => {
            // Store PKCE state temporarily (in real app, use a proper session management)
            let manager = state.account_manager.lock().await;
            manager.store_pkce_state(pkce_state).map_err(|e| e.to_string())?;
            Ok(url)
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Complete OAuth login with authorization code
#[tauri::command]
async fn complete_oauth_login(
    state: State<'_, Arc<AppState>>,
    code: String,
) -> Result<Account, String> {
    let manager = state.account_manager.lock().await;
    
    // Retrieve PKCE state
    let pkce_state = manager.retrieve_pkce_state().map_err(|e| e.to_string())?;
    
    // Exchange code for tokens
    let auth_service = AuthService::new(XaiAuthConfig::default());
    let token_set = auth_service.exchange_code(code, pkce_state).await.map_err(|e| e.to_string())?;
    
    // Get user info
    let user_info = auth_service.get_user_info(&token_set.access_token).await.map_err(|e| e.to_string())?;
    
    // Create and save account
    let display_name = user_info.name.unwrap_or_else(|| user_info.email.clone().unwrap_or_default());
    let email = user_info.email.unwrap_or_default();
    
    let account = manager.add_account(display_name, email, token_set).await.map_err(|e| e.to_string())?;
    
    Ok(account)
}

/// Remove an account
#[tauri::command]
async fn remove_account(state: State<'_, Arc<AppState>>, account_id: String) -> Result<(), String> {
    let manager = state.account_manager.lock().await;
    manager.remove_account(&account_id).await.map_err(|e| e.to_string())
}

/// Manually refresh account usage
#[tauri::command]
async fn refresh_usage(state: State<'_, Arc<AppState>>, account_id: String) -> Result<(), String> {
    let manager = state.account_manager.lock().await;
    let poller = usage_poller::UsagePoller::new(Arc::clone(&manager));
    poller.update_account_usage(&account_id).await.map_err(|e| e.to_string())
}

/// Set opacity for the window
#[tauri::command]
fn set_opacity(window: tauri::Window, opacity: f64) -> Result<(), String> {
    window.set_opacity(opacity).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();
    
    let account_manager = Arc::new(Mutex::new(AccountManager::new()));
    let app_state = Arc::new(AppState {
        account_manager: Arc::clone(&account_manager),
    });
    
    // Start background polling
    let poller = usage_poller::UsagePoller::new(Arc::clone(&account_manager));
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            // Start polling in background
            // Note: This is simplified - in production you'd want better lifecycle management
        });
    });
    
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            greet,
            get_accounts,
            start_oauth_login,
            complete_oauth_login,
            remove_account,
            refresh_usage,
            set_opacity
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
