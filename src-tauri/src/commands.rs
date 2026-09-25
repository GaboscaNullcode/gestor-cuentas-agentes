use crate::model::{Account, Provider, UsageSnapshot};
use crate::scheduler;
use crate::state::AppState;
use crate::store::accounts::{new_account, set_pinned as pin, validate_new};
use crate::store::detect::{detect_existing as detect, DetectedAccount};
use crate::tray;
use chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State};

type CmdResult<T> = Result<T, String>;

const MANUAL_REFRESH_GAP_SECS: i64 = 60;

/// Persists accounts and pushes the change to the scheduler, the tray and the UI.
pub fn accounts_changed(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.save_accounts();
    tray::refresh(app);
    let _ = app.emit("accounts-changed", ());
}

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> Vec<Account> {
    state.accounts.lock().unwrap().clone()
}

#[tauri::command]
pub fn get_snapshots(state: State<'_, AppState>) -> HashMap<String, UsageSnapshot> {
    state.cache.lock().unwrap().snapshots.clone()
}

#[tauri::command]
pub fn refresh_account(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let now = Utc::now();
    {
        let mut runtime = state.runtime.lock().unwrap();
        let entry = runtime.entry(id.clone()).or_default();
        if entry.in_flight {
            return Ok(());
        }
        if entry.last_manual.is_some_and(|last| (now - last).num_seconds() < MANUAL_REFRESH_GAP_SECS) {
            return Err("Please wait a minute between manual refreshes.".into());
        }
        entry.last_manual = Some(now);
    }
    let account = state
        .accounts
        .lock()
        .unwrap()
        .iter()
        .find(|a| a.id == id)
        .cloned()
        .ok_or("Account not found")?;
    scheduler::start_fetch(app, account);
    Ok(())
}

#[tauri::command]
pub fn set_pinned(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    pin(&mut state.accounts.lock().unwrap(), &id);
    accounts_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn detect_existing(state: State<'_, AppState>) -> Vec<DetectedAccount> {
    let accounts = state.accounts.lock().unwrap().clone();
    detect(&state.paths.home, &state.shell, &accounts)
}

#[tauri::command]
pub fn add_existing(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: Provider,
    label: String,
    config_dir: String,
    use_default_dir: bool,
) -> CmdResult<Account> {
    if label.trim().is_empty() {
        return Err("Label is required.".into());
    }
    let dir = PathBuf::from(config_dir.trim());
    if !dir.is_dir() {
        return Err(format!("{} does not exist.", dir.display()));
    }
    let account = {
        let mut accounts = state.accounts.lock().unwrap();
        validate_new(provider, &dir, &accounts)?;
        let account = new_account(provider, &label, dir, use_default_dir, &accounts);
        accounts.push(account.clone());
        account
    };
    accounts_changed(&app);
    scheduler::schedule_now(&app, &account.id);
    scheduler::run_due(&app);
    Ok(account)
}
