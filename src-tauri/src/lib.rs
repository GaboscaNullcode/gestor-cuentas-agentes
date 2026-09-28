pub mod aliases;
pub mod cli;
pub mod commands;
pub mod logging;
pub mod login;
pub mod model;
pub mod notifier;
pub mod parse;
pub mod providers;
pub mod scheduler;
pub mod state;
pub mod store;
pub mod tray;

use tauri::{Manager, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(logging::plugin())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let state = state::AppState::load(app.handle())?;
            app.manage(state);
            {
                let state = app.state::<state::AppState>();
                let accounts = state.accounts.lock().unwrap().clone();
                let _ = aliases::write_alias_files(&state.paths, &accounts);
            }
            tray::create(app.handle())?;
            tray::refresh(app.handle());
            scheduler::schedule_all(app.handle());
            scheduler::spawn(app.handle().clone());
            if let Some(window) = app.get_webview_window("main") {
                let panel = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::Focused(false) = event {
                        let _ = panel.hide();
                        tray::note_blur_hide();
                    }
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::get_snapshots,
            commands::refresh_account,
            commands::set_pinned,
            commands::rename_account,
            commands::detect_existing,
            commands::add_existing,
            commands::propose_config_dir,
            commands::add_account,
            commands::reconnect,
            commands::submit_login_code,
            commands::cancel_login,
            commands::remove_account,
            commands::alias_line,
            commands::aliases_status,
            commands::install_aliases,
            commands::uninstall_aliases,
            commands::get_settings,
            commands::save_settings,
            commands::cli_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
