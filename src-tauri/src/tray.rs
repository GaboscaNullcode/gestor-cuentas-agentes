use crate::model::{Account, AccountStatus, Provider, UsageSnapshot, WindowKind};
use crate::notifier::until;
use crate::scheduler;
use crate::state::AppState;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::image::Image;
use tauri::menu::{IconMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Green,
    Yellow,
    Red,
    Gray,
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

fn level_rgb(level: Level) -> [u8; 3] {
    match level {
        Level::Green => [0x2e, 0xa0, 0x43],
        Level::Yellow => [0xd2, 0x99, 0x22],
        Level::Red => [0xda, 0x36, 0x33],
        Level::Gray => [0x8b, 0x94, 0x9e],
    }
}

/// What the tray ring shows for one account.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gauge {
    /// `fraction` of the ring (0.0 to 1.0) is filled, clockwise from 12 o'clock.
    Arc { fraction: f32, level: Level },
    /// Signed out, CLI missing, an error, or nothing fetched yet.
    NoData,
}

/// The tightest window decides the gauge; a stale snapshot still shows its last numbers.
pub fn gauge(snapshot: Option<&UsageSnapshot>) -> Gauge {
    let Some(snapshot) = snapshot else { return Gauge::NoData };
    match snapshot.status {
        AccountStatus::RateLimited => Gauge::Arc { fraction: 1.0, level: Level::Red },
        AccountStatus::Ok | AccountStatus::Stale => snapshot
            .windows
            .iter()
            .map(|w| w.used_pct.clamp(0.0, 100.0))
            .reduce(f32::max)
            .map_or(Gauge::NoData, |pct| Gauge::Arc { fraction: pct / 100.0, level: level_for_pct(pct) }),
        _ => Gauge::NoData,
    }
}

pub fn snapshot_level(snapshot: Option<&UsageSnapshot>) -> Level {
    match gauge(snapshot) {
        Gauge::Arc { level, .. } => level,
        Gauge::NoData => Level::Gray,
    }
}

/// Accounts in the panel's order: pinned first, then oldest first.
pub fn panel_order(accounts: &[Account]) -> Vec<&Account> {
    let mut ordered: Vec<&Account> = accounts.iter().collect();
    ordered.sort_by_key(|a| (!a.pinned, a.created_at));
    ordered
}

pub fn tray_title(snapshot: Option<&UsageSnapshot>) -> String {
    let Some(snapshot) = snapshot else { return "—".to_string() };
    let pct = |kind: WindowKind| {
        snapshot.window(&kind).map(|w| format!("{:.0}%", w.used_pct)).unwrap_or_else(|| "—".to_string())
    };
    format!("{} · {}", pct(WindowKind::FiveHour), pct(WindowKind::Weekly))
}

/// One account's state in a few words, shared by the tooltip and the menu.
pub fn status_text(snapshot: Option<&UsageSnapshot>, now: DateTime<Utc>) -> String {
    let Some(snapshot) = snapshot else { return tray_title(None) };
    match snapshot.status {
        AccountStatus::Ok | AccountStatus::Stale => tray_title(Some(snapshot)),
        AccountStatus::NeedsLogin => "signed out".to_string(),
        AccountStatus::CliMissing => "CLI not found".to_string(),
        AccountStatus::Error(_) => "error".to_string(),
        AccountStatus::RateLimited => [WindowKind::FiveHour, WindowKind::Weekly]
            .iter()
            .filter_map(|kind| snapshot.window(kind)?.resets_at)
            .find(|at| *at > now)
            .map_or_else(|| "limited".to_string(), |at| format!("limited, back in {}", until(at, now))),
    }
}

/// Windows cuts tray tooltips at 127 UTF-16 code units.
const TOOLTIP_MAX_UTF16: usize = 127;
const TOOLTIP_HEADER: &str = "Usage Monitor";

pub fn tooltip(accounts: &[Account], snapshots: &HashMap<String, UsageSnapshot>, now: DateTime<Utc>) -> String {
    let lines: Vec<String> = panel_order(accounts)
        .iter()
        .map(|a| format!("{}: {}", a.label, status_text(snapshots.get(&a.id), now)))
        .collect();
    let full = std::iter::once(TOOLTIP_HEADER.to_string()).chain(lines.iter().cloned()).collect::<Vec<_>>().join("\n");
    if full.encode_utf16().count() <= TOOLTIP_MAX_UTF16 {
        return full;
    }
    // Labels are capped at 40 chars, so the first line plus the count always fits.
    format!("{TOOLTIP_HEADER}\n{}\nand {} more", lines[0], lines.len() - 1)
}

/// One account row of the tray menu.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuEntry {
    pub account_id: String,
    pub text: String,
    pub level: Level,
}

fn provider_name(provider: Provider) -> &'static str {
    match provider {
        Provider::Claude => "Claude",
        Provider::Codex => "Codex",
    }
}

pub fn menu_entries(
    accounts: &[Account],
    snapshots: &HashMap<String, UsageSnapshot>,
    now: DateTime<Utc>,
) -> Vec<MenuEntry> {
    panel_order(accounts)
        .into_iter()
        .map(|a| {
            let snapshot = snapshots.get(&a.id);
            let star = if a.pinned { "★ " } else { "" };
            MenuEntry {
                account_id: a.id.clone(),
                text: format!("{star}{} · {} — {}", a.label, provider_name(a.provider), status_text(snapshot, now)),
                level: snapshot_level(snapshot),
            }
        })
        .collect()
}

const RING_SUPERSAMPLE: u32 = 4;
const RING_DASHES: f32 = 8.0;
const RING_DASH_DUTY: f32 = 0.6;
const RING_TRACK_ALPHA: f32 = 0.35;

/// A ring gauge generated at runtime (no icon assets), antialiased by supersampling.
/// The unfilled part of an arc is a faint gray track; `NoData` is a dashed gray ring.
pub fn ring_rgba(gauge: Gauge, size: u32) -> Vec<u8> {
    let (color, fraction) = match gauge {
        Gauge::Arc { fraction, level } => (level_rgb(level), fraction),
        Gauge::NoData => (level_rgb(Level::Gray), 0.0),
    };
    let track = level_rgb(Level::Gray);
    let center = size as f32 / 2.0;
    let outer = center - 1.0;
    let inner = outer - size as f32 * 0.15;
    let step = 1.0 / RING_SUPERSAMPLE as f32;
    let samples = (RING_SUPERSAMPLE * RING_SUPERSAMPLE) as f32;
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (mut arc_hits, mut track_hits) = (0.0, 0.0);
            for sy in 0..RING_SUPERSAMPLE {
                for sx in 0..RING_SUPERSAMPLE {
                    let dx = x as f32 + (sx as f32 + 0.5) * step - center;
                    let dy = y as f32 + (sy as f32 + 0.5) * step - center;
                    let distance = dx.hypot(dy);
                    if distance < inner || distance > outer {
                        continue;
                    }
                    // Fraction of a turn from 12 o'clock, clockwise (image y grows downward).
                    let turn = (dx.atan2(-dy) / std::f32::consts::TAU).rem_euclid(1.0);
                    match gauge {
                        Gauge::Arc { .. } if turn < fraction => arc_hits += 1.0,
                        Gauge::Arc { .. } => track_hits += 1.0,
                        Gauge::NoData if (turn * RING_DASHES).fract() < RING_DASH_DUTY => arc_hits += 1.0,
                        Gauge::NoData => {}
                    }
                }
            }
            let arc_alpha = arc_hits / samples;
            let track_alpha = track_hits / samples * RING_TRACK_ALPHA;
            let alpha = arc_alpha + track_alpha;
            let channel = |i: usize| {
                if alpha == 0.0 {
                    0
                } else {
                    ((color[i] as f32 * arc_alpha + track[i] as f32 * track_alpha) / alpha).round() as u8
                }
            };
            pixels.extend_from_slice(&[channel(0), channel(1), channel(2), (alpha * 255.0).round() as u8]);
        }
    }
    pixels
}

/// A small centered dot in the level's color, for menu rows. Menus scale item icons down
/// to about 16px, so the dot keeps a wide transparent margin to stay small next to the text.
pub fn dot_rgba(level: Level, size: u32) -> Vec<u8> {
    let [r, g, b] = level_rgb(level);
    let center = (size as f32 - 1.0) / 2.0;
    let radius = size as f32 / 4.0;
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

const ICON_SIZE: u32 = 32;

fn ring_icon(gauge: Gauge) -> Image<'static> {
    Image::new_owned(ring_rgba(gauge, ICON_SIZE), ICON_SIZE, ICON_SIZE)
}

fn dot_icon(level: Level) -> Image<'static> {
    Image::new_owned(dot_rgba(level, ICON_SIZE), ICON_SIZE, ICON_SIZE)
}

/// Menu ids of account rows carry this prefix, so a rebuild can find and drop them.
const ACCOUNT_ITEM_PREFIX: &str = "account:";
/// Account rows sit right below the "Accounts" header.
const FIRST_ACCOUNT_POSITION: usize = 1;

/// The tray menu is kept and mutated in place because on Linux a tray menu, once set,
/// cannot be replaced. `shown` avoids rebuilding an unchanged, possibly open, menu.
struct TrayMenu {
    menu: Menu<Wry>,
    shown: Mutex<Option<Vec<MenuEntry>>>,
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let header = MenuItem::with_id(app, "accounts-header", "Accounts", false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open panel", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh all", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &header,
            &PredefinedMenuItem::separator(app)?,
            &open,
            &refresh,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(ring_icon(Gauge::NoData))
        .tooltip(TOOLTIP_HEADER)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_panel(app, Position::TopRight),
            "refresh" => scheduler::refresh_all(app),
            "quit" => quit_app(app),
            id if id.starts_with(ACCOUNT_ITEM_PREFIX) => show_panel(app, Position::TopRight),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                toggle_panel(tray.app_handle());
            }
        })
        .build(app)?;
    app.manage(TrayMenu { menu, shown: Mutex::new(None) });
    Ok(())
}

/// Stops an active sign-in's process tree (it may hold a local port) before exiting.
fn quit_app(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        app.state::<AppState>().login.cancel_and_wait(std::time::Duration::from_secs(2)).await;
        app.exit(0);
    });
}

/// Replaces the account rows of the tray menu, leaving the fixed items untouched.
fn update_menu(app: &AppHandle, tray_menu: &TrayMenu, entries: Vec<MenuEntry>) -> tauri::Result<()> {
    let mut shown = tray_menu.shown.lock().unwrap();
    if shown.as_ref() == Some(&entries) {
        return Ok(());
    }
    let menu = &tray_menu.menu;
    for item in menu.items()? {
        if item.id().as_ref().starts_with(ACCOUNT_ITEM_PREFIX) {
            menu.remove(&item)?;
        }
    }
    if entries.is_empty() {
        let placeholder =
            MenuItem::with_id(app, format!("{ACCOUNT_ITEM_PREFIX}none"), "No accounts yet", false, None::<&str>)?;
        menu.insert(&placeholder, FIRST_ACCOUNT_POSITION)?;
    }
    for (offset, entry) in entries.iter().enumerate() {
        let item = IconMenuItem::with_id(
            app,
            format!("{ACCOUNT_ITEM_PREFIX}{}", entry.account_id),
            &entry.text,
            true,
            Some(dot_icon(entry.level)),
            None::<&str>,
        )?;
        menu.insert(&item, FIRST_ACCOUNT_POSITION + offset)?;
    }
    *shown = Some(entries);
    Ok(())
}

/// Updates the ring icon, tooltip, (macOS/Linux) title text and the menu's account rows
/// from the current cache. The icon and title follow the pinned account, or the first one.
pub fn refresh(app: &AppHandle) {
    let state = app.state::<AppState>();
    let accounts = state.accounts.lock().unwrap().clone();
    let snapshots = state.cache.lock().unwrap().snapshots.clone();
    let now = Utc::now();
    let featured = panel_order(&accounts).first().and_then(|a| snapshots.get(&a.id));
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Err(e) = tray.set_icon(Some(ring_icon(gauge(featured)))) {
            log::error!("setting the tray icon failed: {e}");
        }
        if let Err(e) = tray.set_tooltip(Some(tooltip(&accounts, &snapshots, now))) {
            log::error!("setting the tray tooltip failed: {e}");
        }
        // Unsupported on Windows (no-op); there the ring and the tooltip carry the numbers.
        if let Err(e) = tray.set_title(Some(tray_title(featured))) {
            log::error!("setting the tray title failed: {e}");
        }
    }
    if let Some(tray_menu) = app.try_state::<TrayMenu>() {
        if let Err(e) = update_menu(app, &tray_menu, menu_entries(&accounts, &snapshots, now)) {
            log::error!("updating the tray menu failed: {e}");
        }
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
    use chrono::{Duration, TimeZone};
    use std::path::PathBuf;

    fn snap(status: AccountStatus, windows: &[(WindowKind, f32)]) -> UsageSnapshot {
        UsageSnapshot {
            plan: None,
            windows: windows.iter().map(|(k, p)| Window { kind: k.clone(), used_pct: *p, resets_at: None }).collect(),
            fetched_at: Utc::now(),
            status,
            last_error: None,
        }
    }

    fn account(id: &str, label: &str, provider: Provider, pinned: bool, created_day: u32) -> Account {
        Account {
            id: id.to_string(),
            provider,
            label: label.to_string(),
            config_dir: PathBuf::from(format!("/tmp/{id}")),
            use_default_dir: false,
            pinned,
            alias_name: id.to_string(),
            created_at: Utc.with_ymd_and_hms(2026, 1, created_day, 0, 0, 0).unwrap(),
            created_by_app: false,
        }
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 28, 14, 32, 0).unwrap()
    }

    fn pixel(rgba: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
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
    fn gauge_follows_the_tightest_window() {
        let s = snap(
            AccountStatus::Ok,
            &[(WindowKind::FiveHour, 23.0), (WindowKind::WeeklyScoped("Sonnet".into()), 64.0)],
        );
        assert_eq!(gauge(Some(&s)), Gauge::Arc { fraction: 0.64, level: Level::Yellow });
        let over = snap(AccountStatus::Stale, &[(WindowKind::Weekly, 130.0)]);
        assert_eq!(gauge(Some(&over)), Gauge::Arc { fraction: 1.0, level: Level::Red });
    }

    #[test]
    fn gauge_is_full_red_when_limited_and_empty_without_data() {
        let limited = snap(AccountStatus::RateLimited, &[(WindowKind::Weekly, 40.0)]);
        assert_eq!(gauge(Some(&limited)), Gauge::Arc { fraction: 1.0, level: Level::Red });
        assert_eq!(gauge(Some(&snap(AccountStatus::NeedsLogin, &[(WindowKind::Weekly, 40.0)]))), Gauge::NoData);
        assert_eq!(gauge(Some(&snap(AccountStatus::CliMissing, &[]))), Gauge::NoData);
        assert_eq!(gauge(Some(&snap(AccountStatus::Error("boom".into()), &[]))), Gauge::NoData);
        assert_eq!(gauge(Some(&snap(AccountStatus::Ok, &[]))), Gauge::NoData);
        assert_eq!(gauge(None), Gauge::NoData);
        assert_eq!(snapshot_level(None), Level::Gray);
    }

    #[test]
    fn panel_order_puts_pinned_first_then_oldest() {
        let accounts = vec![
            account("c", "C", Provider::Codex, false, 3),
            account("b", "B", Provider::Claude, true, 2),
            account("a", "A", Provider::Claude, false, 1),
        ];
        let ids: Vec<&str> = panel_order(&accounts).iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["b", "a", "c"]);
    }

    #[test]
    fn status_text_covers_every_status() {
        let now = now();
        assert_eq!(status_text(None, now), "—");
        let ok = snap(AccountStatus::Stale, &[(WindowKind::FiveHour, 82.0), (WindowKind::Weekly, 46.0)]);
        assert_eq!(status_text(Some(&ok), now), "82% · 46%");
        assert_eq!(status_text(Some(&snap(AccountStatus::NeedsLogin, &[])), now), "signed out");
        assert_eq!(status_text(Some(&snap(AccountStatus::CliMissing, &[])), now), "CLI not found");
        assert_eq!(status_text(Some(&snap(AccountStatus::Error("x".into()), &[])), now), "error");
        assert_eq!(status_text(Some(&snap(AccountStatus::RateLimited, &[])), now), "limited");
    }

    #[test]
    fn limited_counts_down_to_the_five_hour_reset_else_the_weekly_one() {
        let now = now();
        let mut s = snap(AccountStatus::RateLimited, &[(WindowKind::FiveHour, 100.0), (WindowKind::Weekly, 64.0)]);
        s.windows[0].resets_at = Some(now + Duration::minutes(38));
        s.windows[1].resets_at = Some(now + Duration::days(3));
        assert_eq!(status_text(Some(&s), now), "limited, back in 38m");
        s.windows[0].resets_at = Some(now - Duration::minutes(1));
        assert_eq!(status_text(Some(&s), now), "limited, back in 3d 0h");
    }

    #[test]
    fn tooltip_lists_every_account_in_panel_order() {
        let accounts = vec![
            account("p", "Personal", Provider::Codex, false, 2),
            account("w", "Work", Provider::Claude, true, 1),
        ];
        let snapshots = HashMap::from([
            ("w".to_string(), snap(AccountStatus::Ok, &[(WindowKind::FiveHour, 82.0), (WindowKind::Weekly, 46.0)])),
            ("p".to_string(), snap(AccountStatus::NeedsLogin, &[])),
        ]);
        assert_eq!(tooltip(&accounts, &snapshots, now()), "Usage Monitor\nWork: 82% · 46%\nPersonal: signed out");
        assert_eq!(tooltip(&[], &snapshots, now()), "Usage Monitor");
    }

    #[test]
    fn long_tooltip_falls_back_to_the_first_account_and_a_count() {
        let mut accounts: Vec<Account> = (1..=8)
            .map(|day| account(&format!("id{day}"), &format!("Side project {day}"), Provider::Claude, false, day))
            .collect();
        accounts[4].pinned = true;
        let text = tooltip(&accounts, &HashMap::new(), now());
        assert_eq!(text, "Usage Monitor\nSide project 5: —\nand 7 more");
        assert!(text.encode_utf16().count() <= TOOLTIP_MAX_UTF16);
    }

    #[test]
    fn menu_entries_star_the_pinned_account_and_carry_its_level() {
        let accounts = vec![
            account("t", "Team", Provider::Codex, false, 2),
            account("w", "Work", Provider::Claude, true, 1),
        ];
        let snapshots = HashMap::from([
            ("w".to_string(), snap(AccountStatus::Ok, &[(WindowKind::FiveHour, 82.0), (WindowKind::Weekly, 46.0)])),
            ("t".to_string(), snap(AccountStatus::RateLimited, &[])),
        ]);
        assert_eq!(
            menu_entries(&accounts, &snapshots, now()),
            vec![
                MenuEntry { account_id: "w".into(), text: "★ Work · Claude — 82% · 46%".into(), level: Level::Red },
                MenuEntry { account_id: "t".into(), text: "Team · Codex — limited".into(), level: Level::Red },
            ]
        );
    }

    #[test]
    fn ring_fills_clockwise_from_twelve_over_a_faint_track() {
        let size = 32;
        let rgba = ring_rgba(Gauge::Arc { fraction: 0.25, level: Level::Green }, size);
        assert_eq!(rgba.len(), (size * size * 4) as usize);
        let [r, g, b] = level_rgb(Level::Green);
        assert_eq!(pixel(&rgba, size, 16, 3), [r, g, b, 255], "12 o'clock is on the arc");
        assert_eq!(pixel(&rgba, size, 28, 15), [r, g, b, 255], "just before 3 o'clock is still arc");
        assert_eq!(pixel(&rgba, size, 28, 16)[3], (RING_TRACK_ALPHA * 255.0).round() as u8, "past 3 o'clock is track");
        let [tr, tg, tb] = level_rgb(Level::Gray);
        let track = (RING_TRACK_ALPHA * 255.0).round() as u8;
        assert_eq!(pixel(&rgba, size, 3, 16), [tr, tg, tb, track], "9 o'clock is unfilled track");
        assert_eq!(pixel(&rgba, size, 16, 16)[3], 0, "center is transparent");
        assert_eq!(pixel(&rgba, size, 0, 0)[3], 0, "corner is transparent");
    }

    #[test]
    fn empty_arc_shows_only_the_track() {
        let rgba = ring_rgba(Gauge::Arc { fraction: 0.0, level: Level::Green }, 32);
        assert!(rgba.chunks(4).all(|p| p[3] <= (RING_TRACK_ALPHA * 255.0).round() as u8));
    }

    #[test]
    fn no_data_ring_is_dashed() {
        let size = 32;
        let rgba = ring_rgba(Gauge::NoData, size);
        let ring_alphas: Vec<u8> = (0..size).map(|x| pixel(&rgba, size, x, 3)[3]).collect();
        assert!(ring_alphas.contains(&255), "dashes are drawn");
        // The first gap starts at 0.6 of the first eighth of a turn, just right of 12 o'clock.
        let gap_angle = std::f32::consts::TAU * (RING_DASH_DUTY + 1.0) / 2.0 / RING_DASHES;
        let radius = 13.0;
        let (x, y) = (16.0 + radius * gap_angle.sin(), 16.0 - radius * gap_angle.cos());
        assert_eq!(pixel(&rgba, size, x as u32, y as u32)[3], 0, "gaps are transparent");
    }

    #[test]
    fn dot_is_rgba_of_expected_size() {
        assert_eq!(dot_rgba(Level::Red, 32).len(), 32 * 32 * 4);
    }

    #[test]
    fn click_right_after_blur_hide_counts_as_closing() {
        assert!(recently_hidden(10_250, 10_000));
        assert!(!recently_hidden(10_300, 10_000));
        assert!(!recently_hidden(10_000, 0), "no blur-hide recorded yet");
    }
}
