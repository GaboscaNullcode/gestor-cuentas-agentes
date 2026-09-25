use crate::model::{Account, AccountStatus, UsageSnapshot, WindowKind};
use crate::scheduler;
use crate::state::AppState;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_positioner::{Position, WindowExt};

pub const TRAY_ID: &str = "main";

/// A tray click first blurs the open panel (which hides it) and only then reaches the
/// click handler; a hide this recent means the click was meant to close the panel.
const BLUR_CLICK_WINDOW_MS: u64 = 300;

static LAST_BLUR_HIDE_MS: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

pub fn recently_hidden(now_ms: u64, hidden_ms: u64) -> bool {
    hidden_ms != 0 && now_ms.saturating_sub(hidden_ms) < BLUR_CLICK_WINDOW_MS
}

/// Called when the panel hides because it lost focus.
pub fn note_blur_hide() {
    LAST_BLUR_HIDE_MS.store(now_ms(), Ordering::Relaxed);
}

/// Variant order is severity order: Red beats an error (Gray), which beats Yellow-free Green.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Green,
    Gray,
    Yellow,
    Red,
}

pub fn level_for_pct(pct: f32) -> Level {
    if pct >= 80.0 {
        Level::Red
    } else if pct >= 50.0 {
        Level::Yellow
    } else {
        Level::Green
    }
}

pub fn snapshot_level(snapshot: &UsageSnapshot) -> Level {
    match snapshot.status {
        AccountStatus::Ok | AccountStatus::Stale | AccountStatus::RateLimited => snapshot
            .windows
            .iter()
            .map(|w| level_for_pct(w.used_pct))
            .max()
            .unwrap_or(Level::Green),
        _ => Level::Gray,
    }
}

pub fn worst_level<'a>(snapshots: impl Iterator<Item = &'a UsageSnapshot>) -> Level {
    snapshots.map(snapshot_level).max().unwrap_or(Level::Gray)
}

pub fn tray_title(snapshot: Option<&UsageSnapshot>) -> String {
    let Some(snapshot) = snapshot else { return "—".to_string() };
    let pct = |kind: WindowKind| {
        snapshot.window(&kind).map(|w| format!("{:.0}%", w.used_pct)).unwrap_or_else(|| "—".to_string())
    };
    format!("{} · {}", pct(WindowKind::FiveHour), pct(WindowKind::Weekly))
}

pub fn tooltip(accounts: &[Account], snapshots: &HashMap<String, UsageSnapshot>) -> String {
    let mut ordered: Vec<&Account> = accounts.iter().collect();
    ordered.sort_by_key(|a| !a.pinned);
    let mut lines = vec!["Usage Monitor".to_string()];
    lines.extend(ordered.iter().map(|a| format!("{}: {}", a.label, tray_title(snapshots.get(&a.id)))));
    lines.join("\n")
}

/// A filled circle in the level's color, generated at runtime so no icon assets are needed.
pub fn icon_rgba(level: Level, size: u32) -> Vec<u8> {
    let [r, g, b] = match level {
        Level::Green => [0x2e, 0xa0, 0x43],
        Level::Yellow => [0xd2, 0x99, 0x22],
        Level::Red => [0xda, 0x36, 0x33],
        Level::Gray => [0x8b, 0x94, 0x9e],
    };
    let center = (size as f32 - 1.0) / 2.0;
    let radius = size as f32 / 2.0 - 1.0;
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let distance = ((x as f32 - center).powi(2) + (y as f32 - center).powi(2)).sqrt();
            let alpha = ((radius - distance + 0.5).clamp(0.0, 1.0) * 255.0) as u8;
            pixels.extend_from_slice(&[r, g, b, alpha]);
        }
    }
    pixels
}

fn level_icon(level: Level) -> Image<'static> {
    Image::new_owned(icon_rgba(level, 32), 32, 32)
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open panel", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh all", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &refresh, &quit])?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(level_icon(Level::Gray))
        .tooltip("Usage Monitor")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_panel(app, Position::TopRight),
            "refresh" => scheduler::refresh_all(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                toggle_panel(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Updates icon color, tooltip and (macOS/Linux) title text from the current cache.
pub fn refresh(app: &AppHandle) {
    let state = app.state::<AppState>();
    let accounts = state.accounts.lock().unwrap().clone();
    let snapshots = state.cache.lock().unwrap().snapshots.clone();
    let pinned = accounts.iter().find(|a| a.pinned).and_then(|a| snapshots.get(&a.id));
    let level = worst_level(accounts.iter().filter_map(|a| snapshots.get(&a.id)));
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_icon(Some(level_icon(level)));
        let _ = tray.set_tooltip(Some(tooltip(&accounts, &snapshots)));
        // Unsupported on Windows (no-op); there the tooltip carries the numbers.
        let _ = tray.set_title(Some(tray_title(pinned)));
    }
}

pub fn toggle_panel(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else if recently_hidden(now_ms(), LAST_BLUR_HIDE_MS.load(Ordering::Relaxed)) {
            // The blur caused by this very click already closed the panel.
        } else {
            show_panel(app, Position::TrayCenter);
        }
    }
}

pub fn show_panel(app: &AppHandle, position: Position) {
    if let Some(window) = app.get_webview_window("main") {
        // Linux trays emit no click position; fall back to the top-right corner.
        if window.move_window(position).is_err() {
            let _ = window.move_window(Position::TopRight);
        }
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AccountStatus, UsageSnapshot, Window, WindowKind};
    use chrono::Utc;

    fn snap(status: AccountStatus, windows: &[(WindowKind, f32)]) -> UsageSnapshot {
        UsageSnapshot {
            plan: None,
            windows: windows.iter().map(|(k, p)| Window { kind: k.clone(), used_pct: *p, resets_at: None }).collect(),
            fetched_at: Utc::now(),
            status,
            last_error: None,
        }
    }

    #[test]
    fn title_shows_five_hour_and_weekly() {
        let s = snap(AccountStatus::Ok, &[(WindowKind::FiveHour, 34.4), (WindowKind::Weekly, 71.0)]);
        assert_eq!(tray_title(Some(&s)), "34% · 71%");
    }

    #[test]
    fn title_marks_missing_five_hour_window() {
        let s = snap(AccountStatus::Ok, &[(WindowKind::Weekly, 18.0)]);
        assert_eq!(tray_title(Some(&s)), "— · 18%");
        assert_eq!(tray_title(None), "—");
    }

    #[test]
    fn levels_follow_thresholds() {
        assert_eq!(level_for_pct(49.9), Level::Green);
        assert_eq!(level_for_pct(50.0), Level::Yellow);
        assert_eq!(level_for_pct(80.0), Level::Red);
    }

    #[test]
    fn worst_level_prefers_red_over_errors_and_errors_over_green() {
        let green = snap(AccountStatus::Ok, &[(WindowKind::Weekly, 10.0)]);
        let red = snap(AccountStatus::Ok, &[(WindowKind::FiveHour, 90.0)]);
        let broken = snap(AccountStatus::NeedsLogin, &[]);
        assert_eq!(worst_level([&green, &broken].into_iter()), Level::Gray);
        assert_eq!(worst_level([&green, &broken, &red].into_iter()), Level::Red);
        assert_eq!(worst_level(std::iter::empty()), Level::Gray);
    }

    #[test]
    fn click_right_after_blur_hide_counts_as_closing() {
        assert!(recently_hidden(10_250, 10_000));
        assert!(!recently_hidden(10_300, 10_000));
        assert!(!recently_hidden(10_000, 0), "no blur-hide recorded yet");
    }

    #[test]
    fn icon_is_rgba_of_expected_size() {
        assert_eq!(icon_rgba(Level::Red, 32).len(), 32 * 32 * 4);
    }
}
