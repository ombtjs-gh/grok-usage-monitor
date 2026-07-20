mod auth;
mod store;
mod usage;

use auth::{AccountSummary, AccountTokens, AuthError};
use parking_lot::Mutex;
use serde::Serialize;
use std::collections::HashMap;
use store::{AppSettings, StoreError};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WebviewWindow,
};
use usage::{UsageData, UsageError};

struct AppState {
    accounts: Mutex<Vec<AccountTokens>>,
    settings: Mutex<AppSettings>,
    /// Usage cache keyed by account user_id.
    usage_by_account: Mutex<HashMap<String, UsageData>>,
    /// UI / tray locale: "en" | "ja"
    locale: Mutex<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppSnapshot {
    accounts: Vec<AccountSummary>,
    settings: AppSettings,
    /// Selected account usage (primary card).
    usage: Option<UsageData>,
    /// All cached usage rows (for list badges).
    usage_by_account: HashMap<String, UsageData>,
    error: Option<String>,
}

impl From<AuthError> for String {
    fn from(e: AuthError) -> Self {
        e.to_string()
    }
}

impl From<UsageError> for String {
    fn from(e: UsageError) -> Self {
        e.to_string()
    }
}

impl From<StoreError> for String {
    fn from(e: StoreError) -> Self {
        e.to_string()
    }
}

fn snapshot_from(state: &AppState) -> AppSnapshot {
    let accounts: Vec<AccountSummary> = state.accounts.lock().iter().map(|a| a.summary()).collect();
    let settings = state.settings.lock().clone();
    let usage_map = state.usage_by_account.lock().clone();
    let usage = settings
        .selected_account_id
        .as_ref()
        .and_then(|id| usage_map.get(id).cloned())
        .or_else(|| usage_map.values().next().cloned());
    AppSnapshot {
        accounts,
        settings,
        usage,
        usage_by_account: usage_map,
        error: None,
    }
}

fn selected_account(state: &AppState) -> Result<AccountTokens, String> {
    let accounts = state.accounts.lock();
    let settings = state.settings.lock();
    if accounts.is_empty() {
        return Err("No accounts configured. Import from Grok CLI or log in.".into());
    }
    if let Some(id) = &settings.selected_account_id {
        if let Some(acc) = accounts.iter().find(|a| &a.user_id == id) {
            return Ok(acc.clone());
        }
    }
    Ok(accounts[0].clone())
}

fn replace_account(state: &AppState, updated: AccountTokens) -> Result<(), String> {
    {
        let mut accounts = state.accounts.lock();
        if let Some(slot) = accounts.iter_mut().find(|a| a.user_id == updated.user_id) {
            *slot = updated.clone();
        } else {
            accounts.push(updated.clone());
        }
        let store = store::AccountStore {
            accounts: accounts.clone(),
        };
        store::save_accounts(&store)?;
    }
    Ok(())
}

fn emit_snapshot(app: &AppHandle, snap: &AppSnapshot) {
    let _ = app.emit("usage-updated", snap);
    update_tray_tooltip(app, snap);
}

fn tray_strings(locale: &str) -> (&'static str, &'static str, &'static str, &'static str, &'static str) {
    // (show, refresh, quit, disconnected, waiting)
    if locale == "ja" {
        (
            "表示",
            "使用量を更新",
            "終了",
            "Grok Usage · 未接続",
            "Grok Usage · 更新待ち",
        )
    } else {
        (
            "Show",
            "Refresh usage",
            "Quit",
            "Grok Usage · disconnected",
            "Grok Usage · waiting",
        )
    }
}

fn update_tray_tooltip(app: &AppHandle, snap: &AppSnapshot) {
    let locale = app
        .try_state::<AppState>()
        .map(|s| s.locale.lock().clone())
        .unwrap_or_else(|| "en".into());
    let (_show, _refresh, _quit, disconnected, waiting) = tray_strings(&locale);

    let tooltip = if let Some(u) = &snap.usage {
        let label = u
            .account_email
            .as_deref()
            .or(u.account_name.as_deref())
            .unwrap_or("Grok");
        format!("Grok Usage · {label} · {:.0}%", u.percentage)
    } else if snap.accounts.is_empty() {
        disconnected.to_string()
    } else {
        waiting.to_string()
    };
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(tooltip));
    }
}

fn build_tray_menu<R: tauri::Runtime>(
    app: &AppHandle<R>,
    locale: &str,
) -> tauri::Result<Menu<R>> {
    let (show, refresh, quit, _, _) = tray_strings(locale);
    let show_i = MenuItem::with_id(app, "show", show, true, None::<&str>)?;
    let refresh_i = MenuItem::with_id(app, "refresh", refresh, true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", quit, true, None::<&str>)?;
    Menu::with_items(app, &[&show_i, &refresh_i, &quit_i])
}

fn apply_tray_locale(app: &AppHandle, locale: &str) -> Result<(), String> {
    let menu = build_tray_menu(app, locale).map_err(|e| e.to_string())?;
    if let Some(tray) = app.tray_by_id("main") {
        tray.set_menu(Some(menu)).map_err(|e| e.to_string())?;
    }
    // Refresh tooltip language for idle states.
    let snap = {
        let state = app.state::<AppState>();
        snapshot_from(&state)
    };
    update_tray_tooltip(app, &snap);
    Ok(())
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

#[tauri::command]
fn get_snapshot(state: State<'_, AppState>) -> AppSnapshot {
    snapshot_from(&state)
}

#[tauri::command]
async fn import_grok_cli(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AppSnapshot, String> {
    let imported = auth::import_from_grok_cli()?;
    let merged = store::upsert_accounts(imported)?;
    {
        let mut accounts = state.accounts.lock();
        *accounts = merged.accounts;
    }
    {
        let mut settings = state.settings.lock();
        if settings.selected_account_id.is_none() {
            if let Some(first) = state.accounts.lock().first() {
                settings.selected_account_id = Some(first.user_id.clone());
                store::save_settings(&settings)?;
            }
        }
    }
    let snap = refresh_usage_inner(&state).await?;
    emit_snapshot(&app, &snap);
    Ok(snap)
}

#[tauri::command]
async fn login_oauth(app: AppHandle, state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let account = auth::login_with_pkce().await?;
    let id = account.user_id.clone();
    store::upsert_accounts(vec![account])?;
    {
        let store = store::load_accounts()?;
        *state.accounts.lock() = store.accounts;
    }
    {
        let mut settings = state.settings.lock();
        settings.selected_account_id = Some(id);
        store::save_settings(&settings)?;
    }
    let snap = refresh_usage_inner(&state).await?;
    emit_snapshot(&app, &snap);
    Ok(snap)
}

#[tauri::command]
fn remove_account(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
) -> Result<AppSnapshot, String> {
    let store = store::remove_account(&account_id)?;
    *state.accounts.lock() = store.accounts;
    *state.settings.lock() = store::load_settings()?;
    state.usage_by_account.lock().remove(&account_id);
    let snap = snapshot_from(&state);
    emit_snapshot(&app, &snap);
    Ok(snap)
}

#[tauri::command]
async fn select_account(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
) -> Result<AppSnapshot, String> {
    {
        let mut settings = state.settings.lock();
        settings.selected_account_id = Some(account_id.clone());
        store::save_settings(&settings)?;
    }
    // Prefer cached usage if fresh enough; still refresh selected.
    let snap = refresh_one_account(&state, &account_id).await?;
    emit_snapshot(&app, &snap);
    Ok(snap)
}

async fn refresh_one_account(state: &AppState, account_id: &str) -> Result<AppSnapshot, String> {
    let account = {
        let accounts = state.accounts.lock();
        accounts
            .iter()
            .find(|a| a.user_id == account_id)
            .cloned()
            .ok_or_else(|| format!("Account not found: {account_id}"))?
    };
    let fresh = auth::ensure_fresh(&account).await?;
    if fresh.access_token != account.access_token
        || fresh.expires_at != account.expires_at
        || fresh.refresh_token != account.refresh_token
    {
        replace_account(state, fresh.clone())?;
    }
    let usage = usage::fetch_usage(&fresh).await?;
    state
        .usage_by_account
        .lock()
        .insert(usage.account_id.clone(), usage);
    Ok(snapshot_from(state))
}

/// Refresh selected account first, then other accounts (best-effort).
async fn refresh_usage_inner(state: &AppState) -> Result<AppSnapshot, String> {
    let selected = selected_account(state)?;
    let selected_id = selected.user_id.clone();
    refresh_one_account(state, &selected_id).await?;

    let others: Vec<String> = state
        .accounts
        .lock()
        .iter()
        .filter(|a| a.user_id != selected_id)
        .map(|a| a.user_id.clone())
        .collect();

    for id in others {
        // Best-effort for non-selected accounts; don't fail the whole refresh.
        let _ = refresh_one_account(state, &id).await;
    }

    Ok(snapshot_from(state))
}

#[tauri::command]
async fn refresh_usage(app: AppHandle, state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let snap = refresh_usage_inner(&state).await?;
    emit_snapshot(&app, &snap);
    Ok(snap)
}

fn apply_window_opacity(window: &WebviewWindow, opacity: f64) -> Result<(), String> {
    let opacity = opacity.clamp(0.15, 1.0);
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongW, SetLayeredWindowAttributes, SetWindowLongW, GWL_EXSTYLE, LWA_ALPHA,
            WS_EX_LAYERED,
        };
        let hwnd: HWND = window.hwnd().map_err(|e| e.to_string())?;
        unsafe {
            let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
            SetWindowLongW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED.0 as i32);
            let alpha = (opacity * 255.0).round() as u8;
            SetLayeredWindowAttributes(
                hwnd,
                windows::Win32::Foundation::COLORREF(0),
                alpha,
                LWA_ALPHA,
            )
            .map_err(|e| e.to_string())?;
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (window, opacity);
    }
    Ok(())
}

#[tauri::command]
fn set_opacity(
    window: WebviewWindow,
    state: State<'_, AppState>,
    opacity: f64,
) -> Result<(), String> {
    let opacity = opacity.clamp(0.15, 1.0);
    apply_window_opacity(&window, opacity)?;
    let mut settings = state.settings.lock();
    settings.opacity = opacity;
    store::save_settings(&settings)?;
    Ok(())
}

#[tauri::command]
fn set_always_on_top(
    window: WebviewWindow,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    window
        .set_always_on_top(enabled)
        .map_err(|e| e.to_string())?;
    let mut settings = state.settings.lock();
    settings.always_on_top = enabled;
    store::save_settings(&settings)?;
    Ok(())
}

#[tauri::command]
fn set_refresh_interval(state: State<'_, AppState>, minutes: u64) -> Result<AppSettings, String> {
    let minutes = minutes.clamp(1, 120);
    let mut settings = state.settings.lock();
    settings.refresh_interval_minutes = minutes;
    store::save_settings(&settings)?;
    Ok(settings.clone())
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> AppSettings {
    state.settings.lock().clone()
}

/// Sync tray menu (and idle tooltips) with the UI language.
#[tauri::command]
fn set_locale(app: AppHandle, state: State<'_, AppState>, locale: String) -> Result<(), String> {
    let locale = match locale.as_str() {
        "ja" | "jp" => "ja".to_string(),
        _ => "en".to_string(),
    };
    *state.locale.lock() = locale.clone();
    apply_tray_locale(&app, &locale)
}

fn bootstrap_state() -> AppState {
    let accounts = store::load_accounts()
        .map(|s| s.accounts)
        .unwrap_or_default();
    let mut settings = store::load_settings().unwrap_or_default();
    if settings.selected_account_id.is_none() {
        settings.selected_account_id = accounts.first().map(|a| a.user_id.clone());
    }
    AppState {
        accounts: Mutex::new(accounts),
        settings: Mutex::new(settings),
        usage_by_account: Mutex::new(HashMap::new()),
        locale: Mutex::new("en".into()),
    }
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let locale = app
        .try_state::<AppState>()
        .map(|s| s.locale.lock().clone())
        .unwrap_or_else(|| "en".into());
    let menu = build_tray_menu(app, &locale)?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::FailedToReceiveMessage)?;

    let _tray = TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("Grok Usage Monitor")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "refresh" => {
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<AppState>();
                    if let Ok(snap) = refresh_usage_inner(&state).await {
                        emit_snapshot(&handle, &snap);
                    }
                });
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(bootstrap_state())
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            import_grok_cli,
            login_oauth,
            remove_account,
            select_account,
            refresh_usage,
            set_opacity,
            set_always_on_top,
            set_refresh_interval,
            get_settings,
            set_locale,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            // State must exist before tray (locale).
            setup_tray(&handle)?;

            let state = app.state::<AppState>();
            let settings = state.settings.lock().clone();
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_always_on_top(settings.always_on_top);
                let _ = apply_window_opacity(&window, settings.opacity);

                // Close button hides to tray instead of quitting.
                let w = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w.hide();
                    }
                });
            }

            // Background refresh loop
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                {
                    let state = handle.state::<AppState>();
                    if !state.accounts.lock().is_empty() {
                        if let Ok(snap) = refresh_usage_inner(&state).await {
                            emit_snapshot(&handle, &snap);
                        }
                    }
                }
                loop {
                    let interval_mins = {
                        let state = handle.state::<AppState>();
                        let mins = state.settings.lock().refresh_interval_minutes.max(1);
                        mins
                    };
                    tokio::time::sleep(std::time::Duration::from_secs(interval_mins * 60)).await;
                    let state = handle.state::<AppState>();
                    if state.accounts.lock().is_empty() {
                        continue;
                    }
                    if let Ok(snap) = refresh_usage_inner(&state).await {
                        emit_snapshot(&handle, &snap);
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
