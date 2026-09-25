use crate::cli::run::run;
use crate::model::{Account, Provider, UsageSnapshot};
use crate::providers;
use crate::scheduler::{self, ManualClaim};
use crate::state::AppState;
use crate::store::accounts::{can_delete_dir, new_account, normalize_config_dir, proposed_config_dir as propose, set_pinned as pin, validate_new};
use crate::store::detect::{detect_existing as detect, DetectedAccount};
use crate::store::settings::Settings;
use crate::tray;
use chrono::Utc;
use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;

type CmdResult<T> = Result<T, String>;

/// Persists accounts and pushes the change to the scheduler, the tray and the UI.
pub fn accounts_changed(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.save_accounts();
    let accounts = state.accounts.lock().unwrap().clone();
    if let Err(e) = crate::aliases::write_alias_files(&state.paths, &accounts) {
        log::error!("writing alias files failed: {e}");
    }
    tray::refresh(app);
    let _ = app.emit("accounts-changed", ());
}

/// An account as the UI sees it, with what the backend allows for it.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    #[serde(flatten)]
    account: Account,
    /// Whether "also delete the directory" is offered. `remove_account` re-checks it.
    can_delete_dir: bool,
}

#[tauri::command]
pub fn list_accounts(state: State<'_, AppState>) -> Vec<AccountView> {
    let home = &state.paths.home;
    state
        .accounts
        .lock()
        .unwrap()
        .iter()
        .map(|account| AccountView { can_delete_dir: can_delete_dir(account, home), account: account.clone() })
        .collect()
}

#[tauri::command]
pub fn get_snapshots(state: State<'_, AppState>) -> HashMap<String, UsageSnapshot> {
    state.cache.lock().unwrap().snapshots.clone()
}

#[tauri::command]
pub fn refresh_account(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    {
        let mut runtime = state.runtime.lock().unwrap();
        match scheduler::claim_manual(runtime.entry(id.clone()).or_default(), Utc::now()) {
            ManualClaim::Granted => {}
            ManualClaim::InFlight => return Ok(()),
            ManualClaim::TooSoon => return Err("Please wait a minute between manual refreshes.".into()),
        }
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
    let dir = normalize_config_dir(&config_dir, &state.paths.home)?;
    if use_default_dir && dir != state.paths.home.join(provider.default_dir_name()) {
        return Err(format!(
            "Only ~/{} can use the CLI's default location.",
            provider.default_dir_name()
        ));
    }
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

#[tauri::command]
pub fn propose_config_dir(state: State<'_, AppState>, provider: Provider, label: String) -> String {
    propose(provider, &label, &state.paths.home).to_string_lossy().into_owned()
}

#[tauri::command]
pub async fn add_account(app: AppHandle, provider: Provider, label: String, config_dir: String) -> CmdResult<Account> {
    let state = app.state::<AppState>();
    if label.trim().is_empty() {
        return Err("Label is required.".into());
    }
    let dir = normalize_config_dir(&config_dir, &state.paths.home)?;
    let mut account = {
        let accounts = state.accounts.lock().unwrap();
        validate_new(provider, &dir, &accounts)?;
        new_account(provider, &label, dir.clone(), false, &accounts)
    };
    let created = !dir.exists();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    // Only a directory the app made may later be offered for deletion.
    account.created_by_app = created;
    state.accounts.lock().unwrap().push(account.clone());
    accounts_changed(&app);
    if let Err(e) = state.login.start(app.clone(), account.clone(), created.then_some(dir.clone()), true).await {
        state.accounts.lock().unwrap().retain(|a| a.id != account.id);
        if created {
            let _ = std::fs::remove_dir(&dir);
        }
        accounts_changed(&app);
        return Err(e);
    }
    Ok(account)
}

#[tauri::command]
pub async fn reconnect(app: AppHandle, id: String) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let account = state.accounts.lock().unwrap().iter().find(|a| a.id == id).cloned().ok_or("Account not found")?;
    state.login.start(app.clone(), account, None, false).await
}

#[tauri::command]
pub async fn submit_login_code(app: AppHandle, code: String) -> CmdResult<()> {
    app.state::<AppState>().login.submit_code(&code).await
}

#[tauri::command]
pub async fn cancel_login(app: AppHandle) -> CmdResult<()> {
    app.state::<AppState>().login.cancel().await;
    Ok(())
}

#[tauri::command]
pub async fn remove_account(app: AppHandle, id: String, logout: bool, delete_dir: bool) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let account = state.accounts.lock().unwrap().iter().find(|a| a.id == id).cloned().ok_or("Account not found")?;
    if delete_dir && !can_delete_dir(&account, &state.paths.home) {
        return Err("This directory is protected and will not be deleted.".into());
    }
    if logout {
        let ctx = state.cli.lock().unwrap().clone();
        if let Some(cmd) = ctx.command(&account, providers::logout_args(account.provider)) {
            let _ = run(&cmd, Duration::from_secs(30)).await;
        }
    }
    if delete_dir {
        match std::fs::remove_dir_all(&account.config_dir) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!("Could not delete directory: {e}"));
            }
            _ => {} // already gone counts as deleted
        }
    }
    {
        let mut accounts = state.accounts.lock().unwrap();
        accounts.retain(|a| a.id != id);
        if account.pinned {
            if let Some(first) = accounts.first_mut() {
                first.pinned = true;
            }
        }
    }
    state.cache.lock().unwrap().snapshots.remove(&id);
    state.runtime.lock().unwrap().remove(&id);
    state.save_cache();
    accounts_changed(&app);
    Ok(())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AliasStatus {
    installed: bool,
    targets: Vec<String>,
}

#[tauri::command]
pub fn alias_line(state: State<'_, AppState>, id: String) -> CmdResult<String> {
    let accounts = state.accounts.lock().unwrap();
    let account = accounts.iter().find(|a| a.id == id).ok_or("Account not found")?;
    Ok(crate::aliases::alias_line(account))
}

// The alias commands touch shell profiles and, on Windows, spawn PowerShell to find them,
// so they run on a blocking thread instead of the main thread.

#[tauri::command]
pub async fn aliases_status(app: AppHandle) -> CmdResult<AliasStatus> {
    blocking(app, |state| {
        let targets = crate::aliases::installed_targets(&state.paths, &state.shell);
        Ok(AliasStatus { installed: !targets.is_empty(), targets: targets.iter().map(|p| p.display().to_string()).collect() })
    })
    .await
}

#[tauri::command]
pub async fn install_aliases(app: AppHandle) -> CmdResult<AliasStatus> {
    blocking(app, |state| {
        let targets = crate::aliases::install(&state.paths, &state.shell).map_err(|e| e.to_string())?;
        Ok(AliasStatus { installed: !targets.is_empty(), targets: targets.iter().map(|p| p.display().to_string()).collect() })
    })
    .await
}

#[tauri::command]
pub async fn uninstall_aliases(app: AppHandle) -> CmdResult<AliasStatus> {
    blocking(app, |state| {
        crate::aliases::uninstall(&state.paths, &state.shell).map_err(|e| e.to_string())?;
        Ok(AliasStatus { installed: false, targets: Vec::new() })
    })
    .await
}

async fn blocking<T: Send + 'static>(
    app: AppHandle,
    f: impl FnOnce(&AppState) -> CmdResult<T> + Send + 'static,
) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(move || f(&app.state::<AppState>()))
        .await
        .map_err(|e| e.to_string())?
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliStatus {
    claude: Option<String>,
    codex: Option<String>,
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub fn cli_status(state: State<'_, AppState>) -> CliStatus {
    let cli = state.cli.lock().unwrap();
    CliStatus {
        claude: cli.claude.as_ref().map(|p| p.display().to_string()),
        codex: cli.codex.as_ref().map(|p| p.display().to_string()),
    }
}

#[tauri::command]
pub fn save_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> CmdResult<Settings> {
    let next = settings.normalized();
    let prev = state.settings.lock().unwrap().clone();
    if next.launch_at_login != prev.launch_at_login {
        let autolaunch = app.autolaunch();
        let result = if next.launch_at_login { autolaunch.enable() } else { autolaunch.disable() };
        result.map_err(|e| format!("Could not change launch at login: {e}"))?;
    }
    *state.settings.lock().unwrap() = next.clone();
    state.save_settings();
    let paths_changed = next.claude_path != prev.claude_path || next.codex_path != prev.codex_path;
    if paths_changed {
        let resolved = providers::CliContext::resolve(&next, &state.shell, &state.paths.home, state.paths.work_dir());
        log::info!("claude CLI: {:?}, codex CLI: {:?}", resolved.claude, resolved.codex);
        *state.cli.lock().unwrap() = resolved;
    }
    if paths_changed || next.interval_minutes != prev.interval_minutes {
        scheduler::schedule_all(&app);
    }
    Ok(next)
}
