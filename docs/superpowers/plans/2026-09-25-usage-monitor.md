# Usage Monitor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A cross-platform tray app that shows the 5-hour and weekly usage (and reset times) of several Claude and Codex accounts, and lets the user add accounts by signing in through the official CLIs.

**Architecture:** The app is built on Tauri 2. The Rust core owns all logic: the account store, a scheduler that runs the official `claude` and `codex` CLIs with a different `CLAUDE_CONFIG_DIR` or `CODEX_HOME` per account, tolerant parsers that turn their output into one shared `UsageSnapshot` model, notifications and the tray. The React frontend only renders the panel and forms, and talks to Rust through commands and events.

**Tech Stack:** Tauri 2, Rust (tokio, serde, chrono), React 18 + TypeScript + Vite, pnpm. Plugins: opener, positioner, notification, autostart, log.

**Spec:** `docs/superpowers/specs/2026-09-25-usage-monitor-design.md`

## Spike findings

The spike ran on 2026-09-25 with Claude Code 2.1.282 and Codex CLI 0.153.4. The code and data below are verified, not assumed.

- **Claude usage:** `claude -p "/usage" --no-session-persistence --output-format stream-json --verbose` prints JSON lines.
  - The line with `"type":"assistant"` has a top-level `usage_report.rate_limits.limits[]` array. The full shape is in fixture `claude_usage_ok.jsonl`.
  - It takes about 5.5 s, mostly because the user's session hooks run. `--bare` does not return rate limits, so it cannot be used.
  - When the account is logged out, the command still exits 0, but no line carries `usage_report`.
- **Claude auth status:** `claude auth status` prints JSON (`loggedIn`, `email`, `subscriptionType`). It exits 1 when logged out.
- **Claude login:** `claude auth login` works without a TTY. It opens the browser, prints the URL wrapped in OSC-8 escape codes, and then prints `Paste code here if prompted >` without a trailing newline. The app must read stdout in chunks, not lines, and must offer a field to paste the code.
- **Codex usage:** `codex app-server` speaks newline-delimited JSON-RPC over stdio. The sequence is:
  1. The client sends `{"method":"initialize","id":1,"params":{"clientInfo":{"name":...,"title":...,"version":...}}}`.
  2. The server replies with id 1. Unrelated notifications may arrive first, for example `remoteControl/status/changed`.
  3. The client sends `{"method":"initialized"}`, then `{"method":"account/rateLimits/read","id":2}`.
  4. The server replies with id 2. The full exchange takes 0.8 s. The result shape is in fixture `codex_rate_limits_pro.json`.
  - When logged out, the reply to id 2 is `{"error":{"code":-32600,"message":"codex account authentication required to read rate limits"}}`.
- **Codex login status:** `codex login status` exits 0 when logged in and 1 when not.
- **Codex Pro accounts:** they currently have only a weekly `primary` window (10080 min) and `secondary: null`.

### Deviations from the spec, all justified by the spike

- **No rollout-file fallback for Codex.** The app-server works, so the fallback is YAGNI.
- **Enum dispatch instead of a `Provider` trait.** The providers are functions dispatched by a `match` on `Provider`. There are only two providers, and a trait would need `async_trait`.
- **New `use_default_dir` field on `Account`.** On macOS, Claude derives the Keychain entry name from `CLAUDE_CONFIG_DIR` (`Claude Code-credentials-<sha256(dir)[:8]>`). Without the variable, the entry has no suffix. So the "main" account imported from the default location must run with the variable unset, not set to `~/.claude`.

## Global Constraints

- **Toolchain:** Rust stable (1.96+), Node 22+, pnpm. Tauri 2.x crates and `@tauri-apps/*` 2.x packages.
- **Language:** all UI text is in English. Identifiers, comments, commits and docs are in English.
- **Credentials:** the app never reads or writes OAuth tokens, `auth.json`, `.credentials.json` or the Keychain. It only runs the official CLIs.
- **Exact CLI invocations:**
  - Claude usage: `claude -p /usage --no-session-persistence --output-format stream-json --verbose`
  - Claude auth status: `claude auth status`
  - Claude login and logout: `claude auth login`, `claude auth logout`
  - Codex usage: `codex app-server` (JSON-RPC)
  - Codex login, status and logout: `codex login`, `codex login status`, `codex logout`
- **Per-account environment:**
  - When `use_default_dir` is false, set `CLAUDE_CONFIG_DIR` (Claude) or `CODEX_HOME` (Codex) to `config_dir`.
  - When `use_default_dir` is true, remove that variable from the child environment.
- **Working directory and PATH:** CLIs run with cwd `<app data dir>/work`, so no project hooks or project settings apply. They get `PATH` from the user's login shell.
- **Window classification:** by duration or kind, never by the primary/secondary position.
  - Claude: `session` → FiveHour, `weekly_all` → Weekly, `weekly_scoped` → WeeklyScoped(model name).
  - Codex: 300 min → FiveHour and 10080 min → Weekly for limit id `codex`. Other limit ids map to WeeklyScoped(name) at 10080 min and to Other otherwise.
- **Polling:**
  - Default interval 5 min, allowed range 2–30 min, staggered by `interval / N`.
  - 45 s timeout per fetch. Manual refresh at most once every 60 s per account.
  - Backoff doubles the interval up to 60 min.
  - `NeedsLogin` accounts are not polled.
  - A window reset that passes brings the next fetch forward to `resets_at + 30 s`.
- **Notifications:** thresholds default to 80 and 95. Each threshold notifies once per window. Also notify on window reset (when the previous usage was at least 50 %) and on disconnect (once per transition).
- **Colors:** green below 50 %, yellow from 50 %, red from 80 %, gray on error.
- **Safety:** nothing is deleted by default. Directories are protected when `use_default_dir` is true, when they are `~/.claude` or `~/.codex`, and when they are outside the home directory. Protected directories are never deleted.
- **Linux build dependencies:** `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev`.
- **Commits:** one commit per task, in conventional commit format, with no AI attribution lines.
- **Verification per task:** run the task's `cargo test <module>` and `pnpm exec tsc --noEmit`. Do not run the full test suite or a full build unless the task says so.

## Review Focus

1. **The app is launched from Finder on macOS, so it does not inherit the shell PATH.** The expected result is that `claude` and `codex` are still found (Task 3 test `locate_uses_well_known_dirs_when_path_misses`) or the account shows `CliMissing`, never a crash.
2. **The user's main Claude account uses an explicit `CLAUDE_CONFIG_DIR`, set only in `.zshrc`.** The expected result is that detection picks up the login-shell value and imports the account with `use_default_dir = false`, so the Keychain entry name matches (Task 5 test `detect_prefers_shell_env_dir`).
3. **A Codex Pro account has no 5h window.** The expected result is a snapshot with only Weekly, plus "No 5h limit" in the UI, never a weekly value shown as 5h (Task 2 test `pro_plan_has_only_weekly`).
4. **A window reset passes while the app is waiting for the next poll.** The expected result is that the next fetch is brought forward to shortly after `resets_at` (Task 6 test `next_due_brought_forward_by_reset`), and the UI shows "resetting…" in the meantime (Task 7 `formatReset`).
5. **The user removes the imported main account and checks "delete directory".** The expected result is that `~/.claude` and `~/.codex` are never deleted (Task 5 test `default_and_home_dirs_are_protected`).

---

## File Structure

```
package.json, vite.config.ts, tsconfig*.json, index.html       (scaffold)
src/
  main.tsx                 React entry (no StrictMode: login views must start once)
  App.tsx                  view router + data loading + event listeners
  types.ts                 TS mirrors of Rust serde types
  api.ts                   typed invoke() wrappers
  format.ts                pure display helpers
  styles.css
  components/
    AccountCard.tsx        one account: header, bars, actions, inline remove confirm
    WindowBar.tsx          one usage window bar
    AddAccountView.tsx     import existing / add new / add existing dir
    LoginView.tsx          sign-in progress, URL, code paste, cancel
    SettingsView.tsx       settings form, CLI paths, aliases install
src-tauri/
  Cargo.toml, tauri.conf.json, capabilities/default.json, build.rs
  src/
    main.rs                calls usage_monitor_lib::run()
    lib.rs                 module tree, plugins, setup, command registration
    model.rs               Provider, Account, Window, WindowKind, AccountStatus, UsageSnapshot
    parse/mod.rs           re-exports
    parse/claude.rs        parse_claude_usage, parse_claude_auth_status
    parse/codex.rs         parse_codex_rate_limits, read_rate_limits (JSON-RPC), CodexRpcError
    cli/mod.rs             re-exports
    cli/locator.rs         ShellEnv, read_login_shell_env, locate
    cli/run.rs             CliCommand, CliOutput, RunError, run
    providers/mod.rs       CliContext, account_env, fetch_usage, is_logged_in, login/logout args, failure
    providers/claude.rs    Claude fetch + classify_missing_report
    providers/codex.rs     Codex fetch over app-server
    logging.rs             redact, log plugin builder
    store/mod.rs           read_json, write_json_atomic
    store/accounts.rs      slugify, unique_alias, proposed_config_dir, new_account, validate_new, set_pinned, can_delete_dir
    store/settings.rs      Settings
    store/cache.rs         UsageCache
    store/detect.rs        DetectedAccount, detect_existing
    state.rs               AppPaths, AppState, Runtime
    scheduler.rs           backoff_delay, next_due, stagger_offsets, merge_result + runtime loop
    tray.rs                Level, tray_title, tooltip, icon_rgba + tray runtime
    commands.rs            #[tauri::command] functions
    login.rs               LoginProgress, scan_login_output, LoginManager
    aliases.rs             render_sh, render_ps1, install_block, remove_block + install runtime
    notifier.rs            Notification, diff
  tests/fixtures/          real CLI outputs captured in the spike
```

---

### Task 1: Scaffold, domain model and Claude parser

**Files:**
- Create: the whole Tauri + React scaffold at the repo root
- Create: `src-tauri/src/model.rs`, `src-tauri/src/parse/mod.rs`, `src-tauri/src/parse/claude.rs`
- Create: `src-tauri/tests/fixtures/claude_usage_ok.jsonl`, `claude_usage_logged_out.jsonl`, `claude_auth_status_logged_in.json`, `claude_auth_status_logged_out.json`
- Modify: `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`

**Interfaces:**
- Produces: `model::{Provider, Account, WindowKind, Window, AccountStatus, UsageSnapshot}`, `parse::claude::{parse_claude_usage(&str, DateTime<Utc>) -> Option<UsageSnapshot>, parse_claude_auth_status(&str) -> Option<ClaudeAuthStatus>, ClaudeAuthStatus { logged_in, email, plan }, parse_iso(Option<&Value>) -> Option<DateTime<Utc>>}`

- [ ] **Step 1: Scaffold the app into a temp dir and copy it into the repo**

```bash
REPO="$(git rev-parse --show-toplevel)"
TMP="$(mktemp -d)"
cd "$TMP" && pnpm create tauri-app@latest usage-monitor --template react-ts --manager pnpm --identifier com.gabosca.usagemonitor --yes
rsync -a --exclude .git "$TMP/usage-monitor/" "$REPO/"
cd "$REPO" && pnpm install
```

Expected: `package.json`, `src/`, `src-tauri/` exist at the repo root. `src-tauri/Cargo.toml` has `[lib] name = "usage_monitor_lib"`. If the template's lib name differs, use the template's name everywhere this plan says `usage_monitor_lib`.

- [ ] **Step 2: Add all Rust dependencies once**

Replace the `[dependencies]` section of `src-tauri/Cargo.toml` with the block below (keep the template's `[build-dependencies]` and `[lib]` sections), and add `[dev-dependencies]`:

```toml
[dependencies]
tauri = { version = "2", features = ["tray-icon"] }
tauri-plugin-opener = "2"
tauri-plugin-positioner = { version = "2", features = ["tray-icon"] }
tauri-plugin-notification = "2"
tauri-plugin-autostart = "2"
tauri-plugin-log = "2"
log = "0.4"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
tokio = { version = "1", features = ["process", "time", "sync", "io-util", "macros", "rt-multi-thread"] }
uuid = { version = "1", features = ["v4"] }

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Write the domain model**

Create `src-tauri/src/model.rs`:

```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
}

impl Provider {
    pub fn cli_name(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
        }
    }

    /// Environment variable that points the CLI at an account's config directory.
    pub fn config_env_var(self) -> &'static str {
        match self {
            Provider::Claude => "CLAUDE_CONFIG_DIR",
            Provider::Codex => "CODEX_HOME",
        }
    }

    /// Directory name the CLI uses under the home directory when the env var is unset.
    pub fn default_dir_name(self) -> &'static str {
        match self {
            Provider::Claude => ".claude",
            Provider::Codex => ".codex",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub provider: Provider,
    pub label: String,
    pub config_dir: PathBuf,
    /// Run the CLI with the config env var removed, so it uses its built-in default location.
    /// Needed because Claude derives the macOS Keychain entry name from CLAUDE_CONFIG_DIR.
    pub use_default_dir: bool,
    pub pinned: bool,
    pub alias_name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "name", rename_all = "camelCase")]
pub enum WindowKind {
    FiveHour,
    Weekly,
    WeeklyScoped(String),
    Other(String),
}

impl WindowKind {
    /// Stable identifier used in notification dedupe keys.
    pub fn key(&self) -> String {
        match self {
            WindowKind::FiveHour => "5h".to_string(),
            WindowKind::Weekly => "weekly".to_string(),
            WindowKind::WeeklyScoped(name) => format!("weekly:{name}"),
            WindowKind::Other(name) => format!("other:{name}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub kind: WindowKind,
    pub used_pct: f32,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "message", rename_all = "camelCase")]
pub enum AccountStatus {
    Ok,
    Stale,
    NeedsLogin,
    RateLimited,
    CliMissing,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub plan: Option<String>,
    pub windows: Vec<Window>,
    /// Time of the last successful fetch whose windows are shown.
    pub fetched_at: DateTime<Utc>,
    pub status: AccountStatus,
    pub last_error: Option<String>,
}

impl UsageSnapshot {
    pub fn window(&self, kind: &WindowKind) -> Option<&Window> {
        self.windows.iter().find(|w| &w.kind == kind)
    }
}
```

- [ ] **Step 4: Save the real fixtures captured in the spike**

Create `src-tauri/tests/fixtures/claude_usage_ok.jsonl` (three lines, each a single JSON object):

```
{"type":"system","subtype":"init","cwd":"/tmp/work","session_id":"00000000-0000-0000-0000-000000000000","model":"claude-fable-5-1"}
{"type":"assistant","message":{"role":"assistant","model":"<synthetic>","content":[{"type":"text","text":"Current session: 7% used"}]},"parent_tool_use_id":null,"session_id":"00000000-0000-0000-0000-000000000000","uuid":"00000000-0000-0000-0000-000000000001","timestamp":"2026-09-25T13:28:58.832Z","usage_report":{"session":{"total_cost_usd":0,"total_api_duration_ms":0,"total_duration_ms":4799,"total_lines_added":0,"total_lines_removed":0,"model_usage":{}},"rate_limits":{"limits":[{"kind":"session","group":"session","percent":7,"resets_at":"2026-09-25T17:10:00.026555+00:00","scope":null,"severity":"normal","is_active":false},{"kind":"weekly_all","group":"weekly","percent":25,"resets_at":"2026-09-29T13:00:00.026577+00:00","scope":null,"severity":"normal","is_active":true},{"kind":"weekly_scoped","group":"weekly","percent":0,"resets_at":"2026-09-29T13:00:00+00:00","scope":{"model":{"display_name":"Fable"},"surface":null},"severity":"normal","is_active":false}],"extra_usage":{"is_enabled":false,"monthly_limit":null,"used_credits":null,"utilization":null,"currency":null}}}}
{"type":"result","subtype":"success","is_error":false,"num_turns":0,"total_cost_usd":0,"session_id":"00000000-0000-0000-0000-000000000000"}
```

Create `src-tauri/tests/fixtures/claude_usage_logged_out.jsonl`:

```
{"type":"system","subtype":"init","cwd":"/tmp/work","session_id":"00000000-0000-0000-0000-000000000000","model":"claude-fable-5-1"}
{"type":"assistant","message":{"role":"assistant","model":"<synthetic>","content":[{"type":"text","text":"Not logged in"}]},"parent_tool_use_id":null,"session_id":"00000000-0000-0000-0000-000000000000","uuid":"00000000-0000-0000-0000-000000000001"}
{"type":"result","subtype":"success","is_error":false,"num_turns":0,"total_cost_usd":0,"session_id":"00000000-0000-0000-0000-000000000000"}
```

Create `src-tauri/tests/fixtures/claude_auth_status_logged_in.json`:

```json
{
  "loggedIn": true,
  "authMethod": "claude.ai",
  "apiProvider": "firstParty",
  "configDirectory": "/Users/me/.claude",
  "email": "me@example.com",
  "orgName": "me@example.com's Organization",
  "subscriptionType": "max"
}
```

Create `src-tauri/tests/fixtures/claude_auth_status_logged_out.json`:

```json
{
  "loggedIn": false,
  "authMethod": "none",
  "apiProvider": "firstParty",
  "configDirectory": "/tmp/empty-claude"
}
```

- [ ] **Step 5: Write the failing Claude parser tests**

Create `src-tauri/src/parse/mod.rs`:

```rust
pub mod claude;
```

Create `src-tauri/src/parse/claude.rs` with only the tests for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AccountStatus, WindowKind};
    use chrono::{TimeZone, Utc};

    const OK: &str = include_str!("../../tests/fixtures/claude_usage_ok.jsonl");
    const LOGGED_OUT: &str = include_str!("../../tests/fixtures/claude_usage_logged_out.jsonl");
    const AUTH_IN: &str = include_str!("../../tests/fixtures/claude_auth_status_logged_in.json");
    const AUTH_OUT: &str = include_str!("../../tests/fixtures/claude_auth_status_logged_out.json");

    fn now() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 25, 13, 30, 0).unwrap()
    }

    #[test]
    fn parses_current_limits_fixture() {
        let snap = parse_claude_usage(OK, now()).expect("usage report");
        assert_eq!(snap.status, AccountStatus::Ok);
        assert_eq!(snap.windows.len(), 3);
        assert_eq!(snap.windows[0].kind, WindowKind::FiveHour);
        assert_eq!(snap.windows[0].used_pct, 7.0);
        assert_eq!(
            snap.windows[0].resets_at.unwrap().timestamp(),
            Utc.with_ymd_and_hms(2026, 9, 25, 17, 10, 0).unwrap().timestamp()
        );
        assert_eq!(snap.windows[1].kind, WindowKind::Weekly);
        assert_eq!(snap.windows[1].used_pct, 25.0);
        assert_eq!(snap.windows[2].kind, WindowKind::WeeklyScoped("Fable".into()));
        assert_eq!(snap.fetched_at, now());
    }

    #[test]
    fn returns_none_when_logged_out() {
        assert!(parse_claude_usage(LOGGED_OUT, now()).is_none());
    }

    #[test]
    fn ignores_non_json_lines() {
        let noisy = format!("warning: something\n{OK}\n\n");
        assert!(parse_claude_usage(&noisy, now()).is_some());
    }

    #[test]
    fn unknown_kind_becomes_other_and_malformed_limits_are_skipped() {
        let line = r#"{"type":"assistant","usage_report":{"rate_limits":{"limits":[{"kind":"monthly_cowork","percent":12,"resets_at":null},{"kind":"session"}]}}}"#;
        let snap = parse_claude_usage(line, now()).unwrap();
        assert_eq!(snap.windows.len(), 1);
        assert_eq!(snap.windows[0].kind, WindowKind::Other("monthly_cowork".into()));
        assert_eq!(snap.windows[0].resets_at, None);
    }

    #[test]
    fn parses_legacy_shape() {
        let line = r#"{"type":"assistant","usage_report":{"rate_limits":{"five_hour":{"utilization":9.0,"resets_at":"2026-09-14T02:10:00Z"},"seven_day":{"utilization":68.0,"resets_at":"2026-09-19T09:00:00Z"},"seven_day_opus":null,"seven_day_sonnet":{"utilization":3.0,"resets_at":null}}}}"#;
        let snap = parse_claude_usage(line, now()).unwrap();
        let kinds: Vec<_> = snap.windows.iter().map(|w| w.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![WindowKind::FiveHour, WindowKind::Weekly, WindowKind::WeeklyScoped("Sonnet".into())]
        );
        assert_eq!(snap.windows[1].used_pct, 68.0);
    }

    #[test]
    fn parses_auth_status() {
        let status = parse_claude_auth_status(AUTH_IN).unwrap();
        assert!(status.logged_in);
        assert_eq!(status.email.as_deref(), Some("me@example.com"));
        assert_eq!(status.plan.as_deref(), Some("max"));
        let status = parse_claude_auth_status(AUTH_OUT).unwrap();
        assert!(!status.logged_in);
        assert_eq!(status.plan, None);
        assert!(parse_claude_auth_status("not json").is_none());
    }
}
```

Register the modules at the top of `src-tauri/src/lib.rs` (keep the template's `run()` for now):

```rust
pub mod model;
pub mod parse;
```

- [ ] **Step 6: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test parse::claude`
Expected: compile errors `cannot find function parse_claude_usage` / `parse_claude_auth_status`.

- [ ] **Step 7: Implement the parser**

Add above the tests in `src-tauri/src/parse/claude.rs`:

```rust
use crate::model::{AccountStatus, UsageSnapshot, Window, WindowKind};
use chrono::{DateTime, Utc};
use serde_json::Value;

/// Parses the stream-json output of `claude -p /usage`.
/// Returns None when no line carries a usage report (for example, when logged out).
pub fn parse_claude_usage(stdout: &str, fetched_at: DateTime<Utc>) -> Option<UsageSnapshot> {
    let report = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line.trim()).ok())
        .find_map(|value| value.get("usage_report").cloned())?;
    let rate_limits = report.get("rate_limits")?;
    let windows = match rate_limits.get("limits").and_then(Value::as_array) {
        Some(limits) => limits.iter().filter_map(parse_limit).collect(),
        None => parse_legacy(rate_limits),
    };
    Some(UsageSnapshot {
        plan: None,
        windows,
        fetched_at,
        status: AccountStatus::Ok,
        last_error: None,
    })
}

fn parse_limit(limit: &Value) -> Option<Window> {
    let raw_kind = limit.get("kind")?.as_str()?;
    let used_pct = limit.get("percent")?.as_f64()? as f32;
    let kind = match raw_kind {
        "session" => WindowKind::FiveHour,
        "weekly_all" => WindowKind::Weekly,
        "weekly_scoped" => WindowKind::WeeklyScoped(
            limit
                .pointer("/scope/model/display_name")
                .and_then(Value::as_str)
                .unwrap_or("Scoped")
                .to_string(),
        ),
        other => WindowKind::Other(other.to_string()),
    };
    Some(Window { kind, used_pct, resets_at: parse_iso(limit.get("resets_at")) })
}

/// Shape used before `limits[]` existed; kept so an older CLI still works.
fn parse_legacy(rate_limits: &Value) -> Vec<Window> {
    let fields = [
        ("five_hour", WindowKind::FiveHour),
        ("seven_day", WindowKind::Weekly),
        ("seven_day_opus", WindowKind::WeeklyScoped("Opus".into())),
        ("seven_day_sonnet", WindowKind::WeeklyScoped("Sonnet".into())),
    ];
    fields
        .into_iter()
        .filter_map(|(key, kind)| {
            let window = rate_limits.get(key)?;
            let used_pct = window.get("utilization")?.as_f64()? as f32;
            Some(Window { kind, used_pct, resets_at: parse_iso(window.get("resets_at")) })
        })
        .collect()
}

pub(crate) fn parse_iso(value: Option<&Value>) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value?.as_str()?)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeAuthStatus {
    pub logged_in: bool,
    pub email: Option<String>,
    pub plan: Option<String>,
}

/// Parses the JSON printed by `claude auth status`.
pub fn parse_claude_auth_status(stdout: &str) -> Option<ClaudeAuthStatus> {
    let value: Value = serde_json::from_str(stdout.trim()).ok()?;
    Some(ClaudeAuthStatus {
        logged_in: value.get("loggedIn")?.as_bool()?,
        email: value.get("email").and_then(Value::as_str).map(str::to_string),
        plan: value.get("subscriptionType").and_then(Value::as_str).map(str::to_string),
    })
}
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test parse::claude`
Expected: 6 passed.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "feat(core): scaffold tauri app and parse claude usage report"
```

---

### Task 2: Codex parser and app-server JSON-RPC client

**Files:**
- Create: `src-tauri/src/parse/codex.rs`
- Create: `src-tauri/tests/fixtures/codex_rate_limits_pro.json`, `codex_rate_limits_plus.json`
- Modify: `src-tauri/src/parse/mod.rs`

**Interfaces:**
- Consumes: `model::*` from Task 1.
- Produces: `parse::codex::{parse_codex_rate_limits(&Value, DateTime<Utc>) -> UsageSnapshot, read_rate_limits<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(R, W) -> Result<Value, CodexRpcError>, CodexRpcError { Auth(String), RateLimited(String), Other(String) }, classify_rpc_error(&str) -> CodexRpcError, MAIN_LIMIT_ID}`

- [ ] **Step 1: Save the fixtures**

Create `src-tauri/tests/fixtures/codex_rate_limits_pro.json` (the real response from the spike, with IDs redacted):

```json
{
  "rateLimits": {
    "limitId": "codex",
    "limitName": null,
    "primary": { "usedPercent": 33, "windowDurationMins": 10080, "resetsAt": 1790428839 },
    "secondary": null,
    "credits": { "hasCredits": false, "unlimited": false, "balance": "0" },
    "individualLimit": null,
    "spendControlReached": false,
    "planType": "pro",
    "rateLimitReachedType": null
  },
  "rateLimitsByLimitId": {
    "codex": {
      "limitId": "codex",
      "limitName": null,
      "primary": { "usedPercent": 33, "windowDurationMins": 10080, "resetsAt": 1790428839 },
      "secondary": null,
      "credits": { "hasCredits": false, "unlimited": false, "balance": "0" },
      "individualLimit": null,
      "spendControlReached": false,
      "planType": "pro",
      "rateLimitReachedType": null
    }
  },
  "rateLimitResetCredits": { "availableCount": 1, "credits": [] },
  "accountId": "<redacted>",
  "rateLimitUpsell": null
}
```

Create `src-tauri/tests/fixtures/codex_rate_limits_plus.json` (hand-made: a plan with a 5h window plus a per-model limit):

```json
{
  "rateLimits": {
    "limitId": "codex",
    "limitName": null,
    "primary": { "usedPercent": 55, "windowDurationMins": 300, "resetsAt": 1790410000 },
    "secondary": { "usedPercent": 20, "windowDurationMins": 10080, "resetsAt": 1790950000 },
    "planType": "plus"
  },
  "rateLimitsByLimitId": {
    "codex_bengalfox": {
      "limitId": "codex_bengalfox",
      "limitName": "GPT-5.5-Codex-Max",
      "primary": { "usedPercent": 12, "windowDurationMins": 300, "resetsAt": 1790400000 },
      "secondary": { "usedPercent": 40, "windowDurationMins": 10080, "resetsAt": 1790900000 },
      "planType": "plus"
    },
    "codex": {
      "limitId": "codex",
      "limitName": null,
      "primary": { "usedPercent": 55, "windowDurationMins": 300, "resetsAt": 1790410000 },
      "secondary": { "usedPercent": 20, "windowDurationMins": 10080, "resetsAt": 1790950000 },
      "planType": "plus"
    }
  }
}
```

- [ ] **Step 2: Write the failing tests**

Add `pub mod codex;` to `src-tauri/src/parse/mod.rs`, and create `src-tauri/src/parse/codex.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::WindowKind;
    use chrono::{DateTime, TimeZone, Utc};
    use serde_json::Value;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    const PRO: &str = include_str!("../../tests/fixtures/codex_rate_limits_pro.json");
    const PLUS: &str = include_str!("../../tests/fixtures/codex_rate_limits_plus.json");

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 25, 13, 30, 0).unwrap()
    }

    #[test]
    fn pro_plan_has_only_weekly() {
        let value: Value = serde_json::from_str(PRO).unwrap();
        let snap = parse_codex_rate_limits(&value, now());
        assert_eq!(snap.plan.as_deref(), Some("pro"));
        assert_eq!(snap.windows.len(), 1);
        assert_eq!(snap.windows[0].kind, WindowKind::Weekly);
        assert_eq!(snap.windows[0].used_pct, 33.0);
        assert_eq!(snap.windows[0].resets_at, DateTime::from_timestamp(1790428839, 0));
        assert!(snap.window(&WindowKind::FiveHour).is_none());
    }

    #[test]
    fn plus_plan_classifies_by_duration_and_puts_main_limit_first() {
        let value: Value = serde_json::from_str(PLUS).unwrap();
        let snap = parse_codex_rate_limits(&value, now());
        let kinds: Vec<_> = snap.windows.iter().map(|w| (w.kind.clone(), w.used_pct)).collect();
        assert_eq!(
            kinds,
            vec![
                (WindowKind::FiveHour, 55.0),
                (WindowKind::Weekly, 20.0),
                (WindowKind::Other("GPT-5.5-Codex-Max 5h".into()), 12.0),
                (WindowKind::WeeklyScoped("GPT-5.5-Codex-Max".into()), 40.0),
            ]
        );
    }

    #[test]
    fn falls_back_to_rate_limits_when_by_id_is_missing() {
        let value: Value = serde_json::json!({
            "rateLimits": { "limitId": "codex", "primary": { "usedPercent": 5, "windowDurationMins": 300, "resetsAt": null }, "planType": "plus" }
        });
        let snap = parse_codex_rate_limits(&value, now());
        assert_eq!(snap.windows.len(), 1);
        assert_eq!(snap.windows[0].kind, WindowKind::FiveHour);
        assert_eq!(snap.windows[0].resets_at, None);
    }

    #[test]
    fn unusual_duration_becomes_other() {
        assert_eq!(classify(true, "codex", Some(1440)), WindowKind::Other("1440m".into()));
        assert_eq!(classify(false, "x", None), WindowKind::Other("x".into()));
    }

    #[test]
    fn classifies_rpc_errors() {
        assert!(matches!(
            classify_rpc_error("codex account authentication required to read rate limits"),
            CodexRpcError::Auth(_)
        ));
        assert!(matches!(classify_rpc_error("HTTP 429 Too Many Requests"), CodexRpcError::RateLimited(_)));
        assert!(matches!(classify_rpc_error("boom"), CodexRpcError::Other(_)));
    }

    #[tokio::test]
    async fn exchanges_initialize_and_reads_limits() {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (client_read, client_write) = tokio::io::split(client);
        let server_task = tokio::spawn(async move {
            let (server_read, mut server_write) = tokio::io::split(server);
            let mut lines = BufReader::new(server_read).lines();
            let init: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(init["method"], "initialize");
            assert_eq!(init["params"]["clientInfo"]["name"], "usage_monitor");
            server_write
                .write_all(b"{\"method\":\"remoteControl/status/changed\",\"params\":{}}\n{\"id\":1,\"result\":{}}\n")
                .await
                .unwrap();
            let initialized: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(initialized["method"], "initialized");
            let request: Value = serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
            assert_eq!(request["method"], "account/rateLimits/read");
            let result: Value = serde_json::from_str(PRO).unwrap();
            let reply = serde_json::json!({ "id": 2, "result": result });
            server_write.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
        });
        let result = read_rate_limits(BufReader::new(client_read), client_write).await.unwrap();
        assert_eq!(result["rateLimits"]["planType"], "pro");
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn maps_auth_error_reply() {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (client_read, client_write) = tokio::io::split(client);
        tokio::spawn(async move {
            let (server_read, mut server_write) = tokio::io::split(server);
            let mut lines = BufReader::new(server_read).lines();
            lines.next_line().await.unwrap();
            server_write.write_all(b"{\"id\":1,\"result\":{}}\n").await.unwrap();
            lines.next_line().await.unwrap();
            lines.next_line().await.unwrap();
            server_write
                .write_all(b"{\"error\":{\"code\":-32600,\"message\":\"codex account authentication required to read rate limits\"},\"id\":2}\n")
                .await
                .unwrap();
        });
        let err = read_rate_limits(BufReader::new(client_read), client_write).await.unwrap_err();
        assert!(matches!(err, CodexRpcError::Auth(_)));
    }

    #[tokio::test]
    async fn reports_closed_connection() {
        let (client, server) = tokio::io::duplex(1024);
        drop(server);
        let (client_read, client_write) = tokio::io::split(client);
        let err = read_rate_limits(BufReader::new(client_read), client_write).await.unwrap_err();
        assert!(matches!(err, CodexRpcError::Other(_)));
    }
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test parse::codex`
Expected: compile errors for the missing `parse_codex_rate_limits`, `classify`, `read_rate_limits`, `CodexRpcError`, `classify_rpc_error`.

- [ ] **Step 4: Implement**

Add above the tests in `src-tauri/src/parse/codex.rs`:

```rust
use crate::model::{AccountStatus, UsageSnapshot, Window, WindowKind};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, Lines};

pub const MAIN_LIMIT_ID: &str = "codex";
const FIVE_HOUR_MINS: i64 = 300;
const WEEK_MINS: i64 = 10080;

/// Converts the `account/rateLimits/read` result into a snapshot.
/// The main `codex` limit comes first; other limit ids (per-model limits) follow.
pub fn parse_codex_rate_limits(result: &Value, fetched_at: DateTime<Utc>) -> UsageSnapshot {
    let mut limits: Vec<&Value> = Vec::new();
    if let Some(by_id) = result.get("rateLimitsByLimitId").and_then(Value::as_object) {
        if let Some(main) = by_id.get(MAIN_LIMIT_ID) {
            limits.push(main);
        }
        limits.extend(by_id.iter().filter(|(id, _)| id.as_str() != MAIN_LIMIT_ID).map(|(_, v)| v));
    } else if let Some(main) = result.get("rateLimits") {
        limits.push(main);
    }
    let plan = limits
        .iter()
        .find_map(|l| l.get("planType").and_then(Value::as_str))
        .map(str::to_string);
    let windows = limits.iter().flat_map(|l| windows_of(l)).collect();
    UsageSnapshot { plan, windows, fetched_at, status: AccountStatus::Ok, last_error: None }
}

fn windows_of(limit: &Value) -> Vec<Window> {
    let id = limit.get("limitId").and_then(Value::as_str).unwrap_or(MAIN_LIMIT_ID);
    let name = limit.get("limitName").and_then(Value::as_str).unwrap_or(id);
    ["primary", "secondary"]
        .iter()
        .filter_map(|slot| {
            let window = limit.get(*slot)?;
            let used_pct = window.get("usedPercent")?.as_f64()? as f32;
            let mins = window.get("windowDurationMins").and_then(Value::as_i64);
            let resets_at = window
                .get("resetsAt")
                .and_then(Value::as_i64)
                .and_then(|secs| DateTime::from_timestamp(secs, 0));
            Some(Window { kind: classify(id == MAIN_LIMIT_ID, name, mins), used_pct, resets_at })
        })
        .collect()
}

/// Classifies a window by its duration. Primary/secondary position carries no meaning:
/// Pro plans currently report the weekly window as primary.
pub fn classify(is_main: bool, name: &str, mins: Option<i64>) -> WindowKind {
    match (is_main, mins) {
        (true, Some(FIVE_HOUR_MINS)) => WindowKind::FiveHour,
        (true, Some(WEEK_MINS)) => WindowKind::Weekly,
        (true, Some(m)) => WindowKind::Other(format!("{m}m")),
        (true, None) => WindowKind::Other("unknown".to_string()),
        (false, Some(WEEK_MINS)) => WindowKind::WeeklyScoped(name.to_string()),
        (false, Some(FIVE_HOUR_MINS)) => WindowKind::Other(format!("{name} 5h")),
        (false, Some(m)) => WindowKind::Other(format!("{name} {m}m")),
        (false, None) => WindowKind::Other(name.to_string()),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CodexRpcError {
    Auth(String),
    RateLimited(String),
    Other(String),
}

pub fn classify_rpc_error(message: &str) -> CodexRpcError {
    let lower = message.to_lowercase();
    // Auth first: the auth error message itself contains "rate limits".
    if lower.contains("authentication required") || lower.contains("not logged in") || lower.contains("unauthorized") {
        CodexRpcError::Auth(message.to_string())
    } else if lower.contains("429") || lower.contains("rate limit") || lower.contains("too many requests") {
        CodexRpcError::RateLimited(message.to_string())
    } else {
        CodexRpcError::Other(message.to_string())
    }
}

/// Runs the app-server handshake and returns the `account/rateLimits/read` result.
pub async fn read_rate_limits<R, W>(reader: R, mut writer: W) -> Result<Value, CodexRpcError>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut lines = reader.lines();
    send(
        &mut writer,
        json!({
            "method": "initialize",
            "id": 1,
            "params": { "clientInfo": { "name": "usage_monitor", "title": "Usage Monitor", "version": env!("CARGO_PKG_VERSION") } }
        }),
    )
    .await?;
    wait_for(&mut lines, 1).await?;
    send(&mut writer, json!({ "method": "initialized" })).await?;
    send(&mut writer, json!({ "method": "account/rateLimits/read", "id": 2 })).await?;
    wait_for(&mut lines, 2).await
}

async fn send<W: AsyncWrite + Unpin>(writer: &mut W, message: Value) -> Result<(), CodexRpcError> {
    let line = format!("{message}\n");
    writer.write_all(line.as_bytes()).await.map_err(|e| CodexRpcError::Other(e.to_string()))?;
    writer.flush().await.map_err(|e| CodexRpcError::Other(e.to_string()))
}

async fn wait_for<R: AsyncBufRead + Unpin>(lines: &mut Lines<R>, id: i64) -> Result<Value, CodexRpcError> {
    while let Some(line) = lines.next_line().await.map_err(|e| CodexRpcError::Other(e.to_string()))? {
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        if message.get("id").and_then(Value::as_i64) != Some(id) {
            continue;
        }
        if let Some(error) = message.get("error") {
            let text = error.get("message").and_then(Value::as_str).unwrap_or("unknown app-server error");
            return Err(classify_rpc_error(text));
        }
        return Ok(message.get("result").cloned().unwrap_or(Value::Null));
    }
    Err(CodexRpcError::Other("codex app-server closed the connection".to_string()))
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test parse::codex`
Expected: 8 passed.

- [ ] **Step 6: Commit**

```bash
git add src-tauri
git commit -m "feat(core): parse codex rate limits via app-server json-rpc"
```

---

### Task 3: CLI locator and process runner

**Files:**
- Create: `src-tauri/src/cli/mod.rs`, `src-tauri/src/cli/locator.rs`, `src-tauri/src/cli/run.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod cli;`)

**Interfaces:**
- Produces:
  - `cli::locator::{ShellEnv { vars: HashMap<String,String> }, ShellEnv::get(&self, &str) -> Option<&str>, read_login_shell_env() -> ShellEnv, parse_marked_env(&str) -> Vec<(String,String)>, locate(name: &str, configured: Option<&Path>, search_path: Option<&str>, home: &Path) -> Option<PathBuf>, well_known_dirs(&Path) -> Vec<PathBuf>}`
  - `cli::run::{CliCommand { program, args, env_set, env_remove, cwd, path_env }, CliCommand::to_command(&self) -> tokio::process::Command, CliOutput { code, stdout, stderr }, RunError { Timeout, Spawn(String) }, run(&CliCommand, Duration) -> Result<CliOutput, RunError>}`

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/cli/mod.rs`:

```rust
pub mod locator;
pub mod run;
```

Add `pub mod cli;` to `src-tauri/src/lib.rs`.

Create `src-tauri/src/cli/locator.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn exe_name(name: &str) -> String {
        if cfg!(windows) { format!("{name}.cmd") } else { name.to_string() }
    }

    #[test]
    fn parses_marked_env_and_ignores_rc_noise() {
        let out = "Welcome!\n__USAGE_MONITOR_ENV__PATH=/usr/bin:/opt/homebrew/bin\n__USAGE_MONITOR_ENV__CLAUDE_CONFIG_DIR=/Users/me/.claude\n__USAGE_MONITOR_ENV__CODEX_HOME=\n";
        let vars = parse_marked_env(out);
        assert_eq!(vars[0], ("PATH".to_string(), "/usr/bin:/opt/homebrew/bin".to_string()));
        assert_eq!(vars[1], ("CLAUDE_CONFIG_DIR".to_string(), "/Users/me/.claude".to_string()));
        assert_eq!(vars[2], ("CODEX_HOME".to_string(), String::new()));
    }

    #[test]
    fn locate_finds_binary_on_search_path() {
        let bin = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        fs::write(bin.path().join(exe_name("claude")), "").unwrap();
        let search = std::env::join_paths([bin.path()]).unwrap();
        let found = locate("claude", None, search.to_str(), home.path()).unwrap();
        assert_eq!(found, bin.path().join(exe_name("claude")));
    }

    #[test]
    fn locate_prefers_configured_path_and_rejects_missing_configured_path() {
        let bin = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let configured = bin.path().join("my-claude");
        fs::write(&configured, "").unwrap();
        assert_eq!(locate("claude", Some(&configured), None, home.path()), Some(configured.clone()));
        let missing = bin.path().join("nope");
        assert_eq!(locate("claude", Some(&missing), None, home.path()), None);
    }

    #[test]
    fn locate_uses_well_known_dirs_when_path_misses() {
        let home = tempfile::tempdir().unwrap();
        let local_bin = home.path().join(".local").join("bin");
        fs::create_dir_all(&local_bin).unwrap();
        fs::write(local_bin.join(exe_name("codex")), "").unwrap();
        let empty = tempfile::tempdir().unwrap();
        let search = std::env::join_paths([empty.path()]).unwrap();
        assert_eq!(
            locate("codex", None, search.to_str(), home.path()),
            Some(local_bin.join(exe_name("codex")))
        );
    }

    #[test]
    fn locate_returns_none_when_absent() {
        let home = tempfile::tempdir().unwrap();
        let empty = tempfile::tempdir().unwrap();
        let search = std::env::join_paths([empty.path()]).unwrap();
        assert_eq!(locate("definitely-not-a-cli", None, search.to_str(), home.path()), None);
    }
}
```

Create `src-tauri/src/cli/run.rs` with tests (Unix-only process tests; Windows is covered by manual verification):

```rust
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    fn sh(script: &str) -> CliCommand {
        CliCommand {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".into(), script.into()],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn captures_stdout_stderr_and_exit_code() {
        let out = run(&sh("echo hi; echo err >&2; exit 3"), Duration::from_secs(5)).await.unwrap();
        assert_eq!(out.stdout.trim(), "hi");
        assert_eq!(out.stderr.trim(), "err");
        assert_eq!(out.code, Some(3));
    }

    #[tokio::test]
    async fn sets_and_removes_env_vars() {
        let mut cmd = sh("printf '%s|%s' \"$FOO\" \"$HOME\"");
        cmd.env_set = vec![("FOO".into(), "bar".into())];
        cmd.env_remove = vec!["HOME".into()];
        let out = run(&cmd, Duration::from_secs(5)).await.unwrap();
        assert_eq!(out.stdout, "bar|");
    }

    #[tokio::test]
    async fn uses_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let mut cmd = sh("pwd");
        cmd.cwd = Some(dir.path().to_path_buf());
        let out = run(&cmd, Duration::from_secs(5)).await.unwrap();
        assert_eq!(
            std::fs::canonicalize(out.stdout.trim()).unwrap(),
            std::fs::canonicalize(dir.path()).unwrap()
        );
    }

    #[tokio::test]
    async fn times_out() {
        let err = run(&sh("sleep 5"), Duration::from_millis(200)).await.unwrap_err();
        assert_eq!(err, RunError::Timeout);
    }

    #[tokio::test]
    async fn reports_spawn_errors() {
        let cmd = CliCommand { program: PathBuf::from("/nonexistent/bin"), ..Default::default() };
        assert!(matches!(run(&cmd, Duration::from_secs(1)).await, Err(RunError::Spawn(_))));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test cli::`
Expected: compile errors for the missing items.

- [ ] **Step 3: Implement the locator**

Add above the tests in `src-tauri/src/cli/locator.rs`:

```rust
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const MARKER: &str = "__USAGE_MONITOR_ENV__";
const CAPTURED: [&str; 4] = ["PATH", "CLAUDE_CONFIG_DIR", "CODEX_HOME", "SHELL"];
const SHELL_TIMEOUT: Duration = Duration::from_secs(5);

/// Environment variables read from the user's login shell.
#[derive(Debug, Clone, Default)]
pub struct ShellEnv {
    pub vars: HashMap<String, String>,
}

impl ShellEnv {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.vars.get(key).map(String::as_str).filter(|v| !v.is_empty())
    }
}

/// GUI apps on macOS (and some Linux launchers) do not inherit the shell's PATH or exports,
/// so the values are read from an interactive login shell. Falls back to the process env.
pub fn read_login_shell_env() -> ShellEnv {
    let mut vars: HashMap<String, String> = CAPTURED
        .iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_string(), value)))
        .collect();
    #[cfg(unix)]
    for (key, value) in capture_from_shell() {
        if !value.is_empty() {
            vars.insert(key, value);
        }
    }
    ShellEnv { vars }
}

#[cfg(unix)]
fn capture_from_shell() -> Vec<(String, String)> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let script = CAPTURED
        .iter()
        .map(|key| format!("printf '{MARKER}{key}=%s\\n' \"${key}\""))
        .collect::<Vec<_>>()
        .join("; ");
    let Ok(mut child) = Command::new(&shell)
        .args(["-ilc", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Vec::new();
    };
    let stdout = child.stdout.take();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut out) = stdout {
            let _ = out.read_to_string(&mut text);
        }
        let _ = tx.send(text);
    });
    match rx.recv_timeout(SHELL_TIMEOUT) {
        Ok(text) => {
            let _ = child.wait();
            parse_marked_env(&text)
        }
        Err(_) => {
            let _ = child.kill();
            Vec::new()
        }
    }
}

pub fn parse_marked_env(output: &str) -> Vec<(String, String)> {
    output
        .lines()
        .filter_map(|line| line.split_once(MARKER))
        .filter_map(|(_, rest)| rest.split_once('='))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

/// Finds a CLI binary: the configured path, then the search PATH, then well-known dirs.
/// A configured path that does not exist yields None so the UI can report it.
pub fn locate(name: &str, configured: Option<&Path>, search_path: Option<&str>, home: &Path) -> Option<PathBuf> {
    if let Some(path) = configured {
        return path.is_file().then(|| path.to_path_buf());
    }
    let names = executable_names(name);
    let from_path = search_path.into_iter().flat_map(std::env::split_paths);
    from_path
        .chain(well_known_dirs(home))
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)).collect::<Vec<_>>())
        .find(|candidate| candidate.is_file())
}

fn executable_names(name: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![format!("{name}.exe"), format!("{name}.cmd"), format!("{name}.bat")]
    } else {
        vec![name.to_string()]
    }
}

pub fn well_known_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![
        home.join(".local").join("bin"),
        home.join(".claude").join("local"),
        home.join(".npm-global").join("bin"),
        home.join(".bun").join("bin"),
        home.join(".cargo").join("bin"),
    ];
    if cfg!(target_os = "macos") {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    }
    if cfg!(target_os = "linux") {
        dirs.extend(["/usr/local/bin", "/usr/bin", "/home/linuxbrew/.linuxbrew/bin"].map(PathBuf::from));
    }
    if cfg!(windows) {
        if let Ok(appdata) = std::env::var("APPDATA") {
            dirs.push(PathBuf::from(appdata).join("npm"));
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Programs").join("claude"));
        }
    }
    dirs
}
```

- [ ] **Step 4: Implement the runner**

Add above the tests in `src-tauri/src/cli/run.rs`:

```rust
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

#[derive(Debug, Clone, Default)]
pub struct CliCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env_set: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    pub cwd: Option<PathBuf>,
    /// PATH for the child, so hooks and shims the CLI spawns resolve like in a terminal.
    pub path_env: Option<String>,
}

impl CliCommand {
    /// Builds a command with piped stdout/stderr, null stdin and kill-on-drop.
    /// Windows `.cmd`/`.bat` shims (npm installs) are run through `cmd /C` without a console window.
    pub fn to_command(&self) -> Command {
        let is_script = cfg!(windows)
            && self
                .program
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"));
        let mut cmd = if is_script {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(&self.program);
            c
        } else {
            Command::new(&self.program)
        };
        cmd.args(&self.args);
        if let Some(path) = &self.path_env {
            cmd.env("PATH", path);
        }
        for key in &self.env_remove {
            cmd.env_remove(key);
        }
        for (key, value) in &self.env_set {
            cmd.env(key, value);
        }
        if let Some(dir) = &self.cwd {
            cmd.current_dir(dir);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        cmd
    }
}

#[derive(Debug, Clone)]
pub struct CliOutput {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunError {
    Timeout,
    Spawn(String),
}

/// Runs the command to completion; on timeout the child is killed (kill_on_drop).
pub async fn run(command: &CliCommand, timeout: Duration) -> Result<CliOutput, RunError> {
    let child = command.to_command().spawn().map_err(|e| RunError::Spawn(e.to_string()))?;
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Err(_) => Err(RunError::Timeout),
        Ok(Err(e)) => Err(RunError::Spawn(e.to_string())),
        Ok(Ok(output)) => Ok(CliOutput {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }),
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test cli::`
Expected: 10 passed on macOS/Linux (5 locator + 5 run).

- [ ] **Step 6: Commit**

```bash
git add src-tauri
git commit -m "feat(core): locate cli binaries and run them with per-account env"
```

---

### Task 4: Providers and log redaction

**Files:**
- Create: `src-tauri/src/providers/mod.rs`, `src-tauri/src/providers/claude.rs`, `src-tauri/src/providers/codex.rs`, `src-tauri/src/logging.rs`, `src-tauri/src/store/mod.rs`, `src-tauri/src/store/settings.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod providers; pub mod logging; pub mod store;`)

**Interfaces:**
- Consumes: the parsers (Tasks 1–2) and `cli::*` (Task 3).
- Produces:
  - `store::settings::Settings { interval_minutes: u32, thresholds: Vec<u8>, claude_path: Option<String>, codex_path: Option<String>, launch_at_login: bool }`, with `Settings::normalized(self) -> Self` and `Settings::interval(&self) -> Duration`. It is created here because `CliContext::resolve` needs it; Task 5 adds persistence.
  - `providers::{CliContext { claude, codex, path_env, work_dir }, CliContext::resolve(&Settings, &ShellEnv, &Path, PathBuf) -> CliContext, CliContext::command(&self, &Account, &[&str]) -> Option<CliCommand>, account_env(&Account) -> (Vec<(String,String)>, Vec<String>), fetch_usage(&CliContext, &Account) -> UsageSnapshot (async), is_logged_in(&CliContext, &Account) -> Result<bool,String> (async), login_args(Provider) -> &'static [&'static str], logout_args(Provider) -> &'static [&'static str], failure(AccountStatus, Option<String>) -> UsageSnapshot}`
  - `providers::claude::classify_missing_report(Option<&ClaudeAuthStatus>, &CliOutput) -> UsageSnapshot`
  - `logging::{redact(&str) -> String, plugin<R: Runtime>() -> TauriPlugin<R>}`

- [ ] **Step 1: Create the Settings type (needed by `CliContext::resolve`)**

Create `src-tauri/src/store/mod.rs`:

```rust
pub mod settings;
```

Create `src-tauri/src/store/settings.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub interval_minutes: u32,
    pub thresholds: Vec<u8>,
    pub claude_path: Option<String>,
    pub codex_path: Option<String>,
    pub launch_at_login: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { interval_minutes: 5, thresholds: vec![80, 95], claude_path: None, codex_path: None, launch_at_login: false }
    }
}

impl Settings {
    /// Clamps values into their allowed ranges and drops blank paths.
    pub fn normalized(mut self) -> Self {
        self.interval_minutes = self.interval_minutes.clamp(2, 30);
        self.thresholds.retain(|t| (1..=100).contains(t));
        self.thresholds.sort_unstable();
        self.thresholds.dedup();
        self.claude_path = self.claude_path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
        self.codex_path = self.codex_path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
        self
    }

    pub fn interval(&self) -> Duration {
        Duration::from_secs(u64::from(self.interval_minutes) * 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_ranges_and_blank_paths() {
        let s = Settings {
            interval_minutes: 1,
            thresholds: vec![95, 0, 80, 80, 120],
            claude_path: Some("  ".into()),
            codex_path: Some(" /bin/codex ".into()),
            launch_at_login: true,
        }
        .normalized();
        assert_eq!(s.interval_minutes, 2);
        assert_eq!(s.thresholds, vec![80, 95]);
        assert_eq!(s.claude_path, None);
        assert_eq!(s.codex_path.as_deref(), Some("/bin/codex"));
        assert_eq!(Settings { interval_minutes: 99, ..Default::default() }.normalized().interval_minutes, 30);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let s: Settings = serde_json::from_str(r#"{"intervalMinutes": 10}"#).unwrap();
        assert_eq!(s.interval_minutes, 10);
        assert_eq!(s.thresholds, vec![80, 95]);
    }
}
```

- [ ] **Step 2: Write the failing tests for redaction, env and Claude classification**

Create `src-tauri/src/logging.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secret_keys_in_json_lines() {
        let input = r#"{"tokens":{"access_token":"abc","refresh_token":"def","account_id":"x"},"Authorization":"Bearer z","plan":"pro"}"#;
        let out = redact(input);
        assert!(!out.contains("abc") && !out.contains("def") && !out.contains("Bearer z"));
        assert!(out.contains("\"plan\":\"pro\""));
        assert!(out.contains("\"account_id\":\"x\""));
    }

    #[test]
    fn keeps_plain_text_and_truncates() {
        assert_eq!(redact("hello"), "hello");
        let long = "a".repeat(5000);
        let out = redact(&long);
        assert!(out.ends_with("[truncated]"));
        assert!(out.chars().count() < 2100);
    }
}
```

Create `src-tauri/src/providers/mod.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Account, Provider};
    use chrono::Utc;
    use std::path::PathBuf;

    fn account(provider: Provider, use_default_dir: bool) -> Account {
        Account {
            id: "a1".into(),
            provider,
            label: "Work".into(),
            config_dir: PathBuf::from("/home/me/.claude-work"),
            use_default_dir,
            pinned: false,
            alias_name: "claude-work".into(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn explicit_dir_sets_env_var() {
        let (set, remove) = account_env(&account(Provider::Claude, false));
        assert_eq!(set, vec![("CLAUDE_CONFIG_DIR".to_string(), "/home/me/.claude-work".to_string())]);
        assert!(remove.is_empty());
    }

    #[test]
    fn default_dir_removes_env_var() {
        let (set, remove) = account_env(&account(Provider::Codex, true));
        assert!(set.is_empty());
        assert_eq!(remove, vec!["CODEX_HOME".to_string()]);
    }

    #[test]
    fn command_is_none_without_binary_and_carries_cwd_and_path() {
        let ctx = CliContext {
            claude: Some(PathBuf::from("/bin/claude")),
            codex: None,
            path_env: Some("/usr/bin".into()),
            work_dir: PathBuf::from("/data/work"),
        };
        assert!(ctx.command(&account(Provider::Codex, false), &["app-server"]).is_none());
        let cmd = ctx.command(&account(Provider::Claude, false), &["auth", "status"]).unwrap();
        assert_eq!(cmd.args, vec!["auth", "status"]);
        assert_eq!(cmd.cwd, Some(PathBuf::from("/data/work")));
        assert_eq!(cmd.path_env.as_deref(), Some("/usr/bin"));
    }
}
```

Create `src-tauri/src/providers/claude.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::run::CliOutput;
    use crate::model::AccountStatus;
    use crate::parse::claude::ClaudeAuthStatus;

    fn output(stderr: &str) -> CliOutput {
        CliOutput { code: Some(0), stdout: String::new(), stderr: stderr.into() }
    }

    #[test]
    fn logged_out_means_needs_login() {
        let status = ClaudeAuthStatus { logged_in: false, email: None, plan: None };
        let snap = classify_missing_report(Some(&status), &output(""));
        assert_eq!(snap.status, AccountStatus::NeedsLogin);
    }

    #[test]
    fn stderr_429_means_rate_limited() {
        let status = ClaudeAuthStatus { logged_in: true, email: None, plan: None };
        let snap = classify_missing_report(Some(&status), &output("API Error: 429 rate_limit_error"));
        assert_eq!(snap.status, AccountStatus::RateLimited);
    }

    #[test]
    fn anything_else_is_an_error() {
        let snap = classify_missing_report(None, &output("weird"));
        assert!(matches!(snap.status, AccountStatus::Error(_)));
    }
}
```

Add to `src-tauri/src/lib.rs`: `pub mod logging; pub mod providers; pub mod store;`.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test -- providers:: logging:: store::settings`
Expected: compile errors for the missing items.

- [ ] **Step 4: Implement redaction and the log plugin**

Add above the tests in `src-tauri/src/logging.rs`:

```rust
use serde_json::Value;

const SECRET_HINTS: [&str; 7] = ["token", "access", "refresh", "authorization", "cookie", "secret", "password"];
const MAX_CHARS: usize = 2000;

/// Masks values under secret-looking keys in JSON lines and truncates the text.
/// Applied to every CLI output before it reaches the log file.
pub fn redact(text: &str) -> String {
    let joined = text
        .lines()
        .map(|line| match serde_json::from_str::<Value>(line) {
            Ok(mut value) if value.is_object() || value.is_array() => {
                redact_value(&mut value);
                value.to_string()
            }
            _ => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    truncate(joined)
}

fn redact_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map.iter_mut() {
                if is_secret_key(key) && !inner.is_object() && !inner.is_array() {
                    *inner = Value::String("<redacted>".to_string());
                } else {
                    redact_value(inner);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(redact_value),
        _ => {}
    }
}

fn is_secret_key(key: &str) -> bool {
    let lower = key.to_lowercase();
    SECRET_HINTS.iter().any(|hint| lower.contains(hint))
}

fn truncate(text: String) -> String {
    if text.chars().count() <= MAX_CHARS {
        return text;
    }
    let cut: String = text.chars().take(MAX_CHARS).collect();
    format!("{cut}… [truncated]")
}

/// Rotating file log in the OS log dir plus stdout.
pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    use tauri_plugin_log::{RotationStrategy, Target, TargetKind};
    tauri_plugin_log::Builder::new()
        .targets([
            Target::new(TargetKind::LogDir { file_name: Some("usage-monitor".into()) }),
            Target::new(TargetKind::Stdout),
        ])
        .max_file_size(1_000_000)
        .rotation_strategy(RotationStrategy::KeepOne)
        .level(log::LevelFilter::Info)
        .level_for("usage_monitor_lib", log::LevelFilter::Debug)
        .build()
}
```

- [ ] **Step 5: Implement the provider dispatch**

Add above the tests in `src-tauri/src/providers/mod.rs`:

```rust
pub mod claude;
pub mod codex;

use crate::cli::locator::{locate, ShellEnv};
use crate::cli::run::{run, CliCommand};
use crate::model::{Account, AccountStatus, Provider, UsageSnapshot};
use crate::parse::claude::parse_claude_auth_status;
use crate::store::settings::Settings;
use chrono::Utc;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const STATUS_TIMEOUT: Duration = Duration::from_secs(15);

/// Resolved binaries plus the environment every CLI invocation shares.
#[derive(Debug, Clone, Default)]
pub struct CliContext {
    pub claude: Option<PathBuf>,
    pub codex: Option<PathBuf>,
    pub path_env: Option<String>,
    pub work_dir: PathBuf,
}

impl CliContext {
    pub fn resolve(settings: &Settings, shell: &ShellEnv, home: &Path, work_dir: PathBuf) -> Self {
        let search = shell.get("PATH").map(str::to_string).or_else(|| std::env::var("PATH").ok());
        let find = |name: &str, configured: &Option<String>| {
            locate(name, configured.as_deref().map(Path::new), search.as_deref(), home)
        };
        Self {
            claude: find("claude", &settings.claude_path),
            codex: find("codex", &settings.codex_path),
            path_env: search.clone(),
            work_dir,
        }
    }

    pub fn binary(&self, provider: Provider) -> Option<&PathBuf> {
        match provider {
            Provider::Claude => self.claude.as_ref(),
            Provider::Codex => self.codex.as_ref(),
        }
    }

    pub fn command(&self, account: &Account, args: &[&str]) -> Option<CliCommand> {
        let program = self.binary(account.provider)?.clone();
        let (env_set, env_remove) = account_env(account);
        Some(CliCommand {
            program,
            args: args.iter().map(|a| a.to_string()).collect(),
            env_set,
            env_remove,
            cwd: Some(self.work_dir.clone()),
            path_env: self.path_env.clone(),
        })
    }
}

pub fn account_env(account: &Account) -> (Vec<(String, String)>, Vec<String>) {
    let var = account.provider.config_env_var().to_string();
    if account.use_default_dir {
        (Vec::new(), vec![var])
    } else {
        (vec![(var, account.config_dir.to_string_lossy().into_owned())], Vec::new())
    }
}

pub fn failure(status: AccountStatus, message: Option<String>) -> UsageSnapshot {
    UsageSnapshot { plan: None, windows: Vec::new(), fetched_at: Utc::now(), status, last_error: message }
}

pub async fn fetch_usage(ctx: &CliContext, account: &Account) -> UsageSnapshot {
    match account.provider {
        Provider::Claude => claude::fetch_usage(ctx, account).await,
        Provider::Codex => codex::fetch_usage(ctx, account).await,
    }
}

pub async fn is_logged_in(ctx: &CliContext, account: &Account) -> Result<bool, String> {
    let args: &[&str] = match account.provider {
        Provider::Claude => &["auth", "status"],
        Provider::Codex => &["login", "status"],
    };
    let cmd = ctx
        .command(account, args)
        .ok_or_else(|| format!("{} CLI not found", account.provider.cli_name()))?;
    let out = run(&cmd, STATUS_TIMEOUT).await.map_err(|e| format!("{e:?}"))?;
    Ok(match account.provider {
        Provider::Claude => parse_claude_auth_status(&out.stdout).is_some_and(|s| s.logged_in),
        Provider::Codex => out.code == Some(0),
    })
}

pub fn login_args(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Claude => &["auth", "login"],
        Provider::Codex => &["login"],
    }
}

pub fn logout_args(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::Claude => &["auth", "logout"],
        Provider::Codex => &["logout"],
    }
}
```

- [ ] **Step 6: Implement the Claude provider**

Add above the tests in `src-tauri/src/providers/claude.rs`:

```rust
use super::{failure, CliContext, STATUS_TIMEOUT};
use crate::cli::run::{run, CliOutput, RunError};
use crate::logging::redact;
use crate::model::{Account, AccountStatus, UsageSnapshot};
use crate::parse::claude::{parse_claude_auth_status, parse_claude_usage, ClaudeAuthStatus};
use chrono::Utc;
use std::time::Duration;

const USAGE_ARGS: [&str; 6] = ["-p", "/usage", "--no-session-persistence", "--output-format", "stream-json", "--verbose"];
const USAGE_TIMEOUT: Duration = Duration::from_secs(45);

pub async fn fetch_usage(ctx: &CliContext, account: &Account) -> UsageSnapshot {
    let Some(cmd) = ctx.command(account, &USAGE_ARGS) else {
        return failure(AccountStatus::CliMissing, None);
    };
    let out = match run(&cmd, USAGE_TIMEOUT).await {
        Ok(out) => out,
        Err(RunError::Timeout) => return failure(AccountStatus::Error("claude /usage timed out".into()), None),
        Err(RunError::Spawn(e)) => return failure(AccountStatus::CliMissing, Some(e)),
    };
    let interesting: Vec<&str> = out
        .stdout
        .lines()
        .filter(|l| l.contains("usage_report") || l.contains("\"type\":\"result\""))
        .collect();
    log::debug!("claude usage [{}]: {}", account.label, redact(&interesting.join("\n")));
    let status = auth_status(ctx, account).await;
    match parse_claude_usage(&out.stdout, Utc::now()) {
        Some(mut snapshot) => {
            snapshot.plan = status.and_then(|s| s.plan);
            snapshot
        }
        None => classify_missing_report(status.as_ref(), &out),
    }
}

async fn auth_status(ctx: &CliContext, account: &Account) -> Option<ClaudeAuthStatus> {
    let cmd = ctx.command(account, &["auth", "status"])?;
    let out = run(&cmd, STATUS_TIMEOUT).await.ok()?;
    parse_claude_auth_status(&out.stdout)
}

/// Explains why `/usage` returned no report. Only stderr is inspected for rate limits,
/// because stdout also carries the user's hook output.
pub fn classify_missing_report(status: Option<&ClaudeAuthStatus>, out: &CliOutput) -> UsageSnapshot {
    if status.is_some_and(|s| !s.logged_in) {
        return failure(AccountStatus::NeedsLogin, None);
    }
    let stderr = out.stderr.to_lowercase();
    if stderr.contains("429") || stderr.contains("rate limit") || stderr.contains("rate_limit") {
        return failure(AccountStatus::RateLimited, None);
    }
    let tail: String = out.stderr.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
    failure(
        AccountStatus::Error("Claude did not return a usage report".into()),
        (!tail.trim().is_empty()).then_some(tail),
    )
}
```

- [ ] **Step 7: Implement the Codex provider**

Create `src-tauri/src/providers/codex.rs`:

```rust
use super::{failure, CliContext};
use crate::logging::redact;
use crate::model::{Account, AccountStatus, UsageSnapshot};
use crate::parse::codex::{parse_codex_rate_limits, read_rate_limits, CodexRpcError};
use chrono::Utc;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::BufReader;

const TIMEOUT: Duration = Duration::from_secs(45);

/// Starts `codex app-server` for this account, reads the rate limits and stops it.
/// Codex refreshes its own tokens while serving the request, so the app never touches them.
pub async fn fetch_usage(ctx: &CliContext, account: &Account) -> UsageSnapshot {
    let Some(cmd) = ctx.command(account, &["app-server"]) else {
        return failure(AccountStatus::CliMissing, None);
    };
    let mut command = cmd.to_command();
    command.stdin(Stdio::piped()).stderr(Stdio::null());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => return failure(AccountStatus::CliMissing, Some(e.to_string())),
    };
    let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return failure(AccountStatus::Error("could not open codex app-server pipes".into()), None);
    };
    let result = tokio::time::timeout(TIMEOUT, read_rate_limits(BufReader::new(stdout), stdin)).await;
    let _ = child.kill().await;
    match result {
        Err(_) => failure(AccountStatus::Error("codex app-server timed out".into()), None),
        Ok(Err(CodexRpcError::Auth(_))) => failure(AccountStatus::NeedsLogin, None),
        Ok(Err(CodexRpcError::RateLimited(message))) => failure(AccountStatus::RateLimited, Some(message)),
        Ok(Err(CodexRpcError::Other(message))) => failure(AccountStatus::Error(message.clone()), Some(message)),
        Ok(Ok(value)) => {
            log::debug!("codex usage [{}]: {}", account.label, redact(&value.to_string()));
            parse_codex_rate_limits(&value, Utc::now())
        }
    }
}
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test -- providers:: logging:: store::settings`
Expected: 10 passed (3 providers::tests, 3 providers::claude, 2 logging, 2 store::settings).

- [ ] **Step 9: Live smoke check against the real CLIs (manual, one run)**

Add this temporary test to `src-tauri/src/providers/mod.rs` tests, run it once, then delete it before committing:

```rust
#[tokio::test]
#[ignore]
async fn live_smoke() {
    let shell = crate::cli::locator::read_login_shell_env();
    let home = std::env::var("HOME").map(PathBuf::from).unwrap();
    let work = tempfile::tempdir().unwrap();
    let ctx = CliContext::resolve(&Settings::default(), &shell, &home, work.path().to_path_buf());
    for (provider, dir) in [(Provider::Claude, ".claude"), (Provider::Codex, ".codex")] {
        let mut a = account(provider, true);
        a.config_dir = home.join(dir);
        if let Some(v) = shell.get(provider.config_env_var()) { a.config_dir = PathBuf::from(v); a.use_default_dir = false; }
        println!("{provider:?}: {:?}", fetch_usage(&ctx, &a).await);
    }
}
```

Run: `cd src-tauri && cargo test live_smoke -- --ignored --nocapture`
Expected: both snapshots have `status: Ok` and non-empty `windows`. Then delete the test.

- [ ] **Step 10: Commit**

```bash
git add src-tauri
git commit -m "feat(core): fetch usage through claude and codex clis"
```

---

### Task 5: Persistence and detection of existing accounts

**Files:**
- Modify: `src-tauri/src/store/mod.rs`
- Create: `src-tauri/src/store/accounts.rs`, `src-tauri/src/store/cache.rs`, `src-tauri/src/store/detect.rs`

**Interfaces:**
- Consumes: `model::*`, `cli::locator::ShellEnv`, `store::settings::Settings`.
- Produces:
  - `store::{read_json<T: DeserializeOwned + Default>(&Path) -> T, write_json_atomic<T: Serialize>(&Path, &T) -> io::Result<()>}`
  - `store::accounts::{slugify, unique_alias, proposed_config_dir(Provider, &str, &Path) -> PathBuf, new_account(Provider, &str, PathBuf, bool, &[Account]) -> Account, validate_new(Provider, &Path, &[Account]) -> Result<(), String>, set_pinned(&mut [Account], &str), can_delete_dir(&Account, &Path) -> bool}`
  - `store::cache::UsageCache { snapshots: HashMap<String, UsageSnapshot>, sent: HashMap<String, i64> }`, with `prune_sent(&mut self, DateTime<Utc>)`
  - `store::detect::{DetectedAccount { provider, config_dir, use_default_dir }, detect_existing(&Path, &ShellEnv, &[Account]) -> Vec<DetectedAccount>}`

- [ ] **Step 1: Write the failing tests**

Replace `src-tauri/src/store/mod.rs` with:

```rust
pub mod accounts;
pub mod cache;
pub mod detect;
pub mod settings;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::settings::Settings;

    #[test]
    fn missing_file_yields_default_and_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("settings.json");
        let loaded: Settings = read_json(&path);
        assert_eq!(loaded, Settings::default());
        let custom = Settings { interval_minutes: 7, ..Default::default() };
        write_json_atomic(&path, &custom).unwrap();
        assert_eq!(read_json::<Settings>(&path), custom);
    }

    #[test]
    fn corrupt_file_is_moved_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("accounts.json");
        std::fs::write(&path, "{not json").unwrap();
        let loaded: Vec<crate::model::Account> = read_json(&path);
        assert!(loaded.is_empty());
        assert!(dir.path().join("accounts.json.corrupt").exists());
    }
}
```

Create `src-tauri/src/store/accounts.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Provider;
    use std::path::PathBuf;

    #[test]
    fn slugifies_labels() {
        assert_eq!(slugify("  Mi Cuenta Personal! "), "mi-cuenta-personal");
        assert_eq!(slugify("***"), "account");
    }

    #[test]
    fn aliases_are_unique_and_first_account_is_pinned() {
        let home = PathBuf::from("/home/me");
        let first = new_account(Provider::Claude, "Work", home.join(".claude-work"), false, &[]);
        assert_eq!(first.alias_name, "claude-work");
        assert!(first.pinned);
        let second = new_account(Provider::Claude, "work", home.join(".claude-work2"), false, &[first.clone()]);
        assert_eq!(second.alias_name, "claude-work-2");
        assert!(!second.pinned);
    }

    #[test]
    fn proposes_dir_under_home() {
        assert_eq!(
            proposed_config_dir(Provider::Codex, "Side Project", &PathBuf::from("/home/me")),
            PathBuf::from("/home/me/.codex-side-project")
        );
    }

    #[test]
    fn rejects_duplicate_dir_for_same_provider() {
        let dir = PathBuf::from("/home/me/.claude-work");
        let existing = vec![new_account(Provider::Claude, "Work", dir.clone(), false, &[])];
        assert!(validate_new(Provider::Claude, &dir, &existing).is_err());
        assert!(validate_new(Provider::Codex, &dir, &existing).is_ok());
    }

    #[test]
    fn pins_exactly_one() {
        let mut accounts = vec![
            new_account(Provider::Claude, "A", PathBuf::from("/h/.claude-a"), false, &[]),
            new_account(Provider::Codex, "B", PathBuf::from("/h/.codex-b"), false, &[]),
        ];
        let id = accounts[1].id.clone();
        set_pinned(&mut accounts, &id);
        assert!(!accounts[0].pinned && accounts[1].pinned);
    }

    #[test]
    fn default_and_home_dirs_are_protected() {
        let home = PathBuf::from("/home/me");
        let mut a = new_account(Provider::Claude, "Main", home.join(".claude"), false, &[]);
        assert!(!can_delete_dir(&a, &home));
        a.config_dir = home.join(".codex");
        assert!(!can_delete_dir(&a, &home));
        a.config_dir = home.clone();
        assert!(!can_delete_dir(&a, &home));
        a.config_dir = PathBuf::from("/etc/claude");
        assert!(!can_delete_dir(&a, &home));
        a.config_dir = home.join(".claude-work");
        assert!(can_delete_dir(&a, &home));
        a.use_default_dir = true;
        assert!(!can_delete_dir(&a, &home));
    }
}
```

Create `src-tauri/src/store/cache.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone, Utc};

    #[test]
    fn prunes_sent_keys_older_than_eight_days() {
        let now = Utc.with_ymd_and_hms(2026, 9, 25, 0, 0, 0).unwrap();
        let mut cache = UsageCache::default();
        cache.sent.insert("old".into(), (now - Duration::days(9)).timestamp());
        cache.sent.insert("new".into(), (now - Duration::days(1)).timestamp());
        cache.prune_sent(now);
        assert!(cache.sent.contains_key("new") && !cache.sent.contains_key("old"));
    }
}
```

Create `src-tauri/src/store/detect.rs` with tests:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::locator::ShellEnv;
    use crate::model::Provider;
    use std::fs;

    #[test]
    fn detects_default_dirs() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".claude")).unwrap();
        fs::create_dir_all(home.path().join(".codex")).unwrap();
        fs::write(home.path().join(".codex").join("auth.json"), "{}").unwrap();
        let found = detect_existing(home.path(), &ShellEnv::default(), &[]);
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|d| d.use_default_dir));
    }

    #[test]
    fn codex_without_auth_file_is_not_detected() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".codex")).unwrap();
        assert!(detect_existing(home.path(), &ShellEnv::default(), &[]).is_empty());
    }

    #[test]
    fn detect_prefers_shell_env_dir() {
        let home = tempfile::tempdir().unwrap();
        let custom = home.path().join(".claude");
        fs::create_dir_all(&custom).unwrap();
        let mut shell = ShellEnv::default();
        shell.vars.insert("CLAUDE_CONFIG_DIR".into(), custom.to_string_lossy().into_owned());
        let found = detect_existing(home.path(), &shell, &[]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].provider, Provider::Claude);
        assert_eq!(found[0].config_dir, custom);
        assert!(!found[0].use_default_dir);
    }

    #[test]
    fn skips_already_registered_dirs() {
        let home = tempfile::tempdir().unwrap();
        fs::create_dir_all(home.path().join(".claude")).unwrap();
        let existing = vec![crate::store::accounts::new_account(Provider::Claude, "Main", home.path().join(".claude"), true, &[])];
        assert!(detect_existing(home.path(), &ShellEnv::default(), &existing).is_empty());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test store::`
Expected: compile errors for the missing items.

- [ ] **Step 3: Implement the JSON helpers**

Insert at the top of `src-tauri/src/store/mod.rs`, after the `pub mod` lines:

```rust
use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::Path;

/// Reads a JSON file; a missing file gives the default, a corrupt one is moved to `*.corrupt`
/// so the next save does not silently destroy the user's data.
pub fn read_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    let Ok(text) = fs::read_to_string(path) else { return T::default() };
    match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(e) => {
            log::warn!("{} is unreadable ({e}); moving it aside", path.display());
            let _ = fs::rename(path, path.with_extension("json.corrupt"));
            T::default()
        }
    }
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?;
    fs::write(&tmp, bytes)?;
    fs::rename(tmp, path)
}
```

- [ ] **Step 4: Implement accounts, cache and detection**

Add above the tests in `src-tauri/src/store/accounts.rs`:

```rust
use crate::model::{Account, Provider};
use chrono::Utc;
use std::path::{Path, PathBuf};

pub fn slugify(label: &str) -> String {
    let dashed: String = label
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = dashed.split('-').filter(|part| !part.is_empty()).collect::<Vec<_>>().join("-");
    if slug.is_empty() { "account".to_string() } else { slug }
}

pub fn unique_alias(provider: Provider, label: &str, existing: &[Account]) -> String {
    let base = format!("{}-{}", provider.cli_name(), slugify(label));
    let mut candidate = base.clone();
    let mut n = 2;
    while existing.iter().any(|a| a.alias_name == candidate) {
        candidate = format!("{base}-{n}");
        n += 1;
    }
    candidate
}

pub fn proposed_config_dir(provider: Provider, label: &str, home: &Path) -> PathBuf {
    home.join(format!(".{}-{}", provider.cli_name(), slugify(label)))
}

pub fn new_account(provider: Provider, label: &str, config_dir: PathBuf, use_default_dir: bool, existing: &[Account]) -> Account {
    Account {
        id: uuid::Uuid::new_v4().to_string(),
        provider,
        label: label.trim().to_string(),
        alias_name: unique_alias(provider, label, existing),
        config_dir,
        use_default_dir,
        pinned: existing.is_empty(),
        created_at: Utc::now(),
    }
}

pub fn validate_new(provider: Provider, config_dir: &Path, existing: &[Account]) -> Result<(), String> {
    if existing.iter().any(|a| a.provider == provider && a.config_dir == config_dir) {
        return Err(format!("{} is already registered", config_dir.display()));
    }
    Ok(())
}

pub fn set_pinned(accounts: &mut [Account], id: &str) {
    for account in accounts.iter_mut() {
        account.pinned = account.id == id;
    }
}

/// Directories the app may delete: inside home, not home itself, not a CLI default dir.
pub fn can_delete_dir(account: &Account, home: &Path) -> bool {
    let dir = &account.config_dir;
    !account.use_default_dir
        && dir.starts_with(home)
        && dir != home
        && [Provider::Claude, Provider::Codex]
            .iter()
            .all(|p| *dir != home.join(p.default_dir_name()))
}
```

Add above the tests in `src-tauri/src/store/cache.rs`:

```rust
use crate::model::UsageSnapshot;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UsageCache {
    /// Latest snapshot per account id.
    pub snapshots: HashMap<String, UsageSnapshot>,
    /// Notification dedupe keys mapped to the epoch seconds they relate to.
    pub sent: HashMap<String, i64>,
}

impl UsageCache {
    pub fn prune_sent(&mut self, now: DateTime<Utc>) {
        let cutoff = (now - chrono::Duration::days(8)).timestamp();
        self.sent.retain(|_, at| *at >= cutoff);
    }
}
```

Add above the tests in `src-tauri/src/store/detect.rs`:

```rust
use crate::cli::locator::ShellEnv;
use crate::model::{Account, Provider};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedAccount {
    pub provider: Provider,
    pub config_dir: PathBuf,
    pub use_default_dir: bool,
}

/// Finds CLI config dirs that already hold a session and are not registered yet.
/// A config env var exported in the login shell wins over the default location, because
/// Claude's macOS Keychain entry name depends on whether that variable is set.
pub fn detect_existing(home: &Path, shell: &ShellEnv, existing: &[Account]) -> Vec<DetectedAccount> {
    [Provider::Claude, Provider::Codex]
        .into_iter()
        .filter_map(|provider| {
            let (config_dir, use_default_dir) = match shell.get(provider.config_env_var()) {
                Some(dir) => (PathBuf::from(dir), false),
                None => (home.join(provider.default_dir_name()), true),
            };
            let has_session = match provider {
                Provider::Claude => config_dir.is_dir(),
                Provider::Codex => config_dir.join("auth.json").is_file(),
            };
            let registered = existing.iter().any(|a| a.provider == provider && a.config_dir == config_dir);
            (has_session && !registered).then_some(DetectedAccount { provider, config_dir, use_default_dir })
        })
        .collect()
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test store::`
Expected: 15 passed (2 mod, 6 accounts, 1 cache, 4 detect, 2 settings).

- [ ] **Step 6: Commit**

```bash
git add src-tauri
git commit -m "feat(core): persist accounts, settings and usage cache"
```

---

### Task 6: Scheduling logic and app state

**Files:**
- Create: `src-tauri/src/scheduler.rs`, `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs` (add `pub mod scheduler; pub mod state;`)

**Interfaces:**
- Consumes: everything above.
- Produces:
  - `scheduler::{backoff_delay(Duration, u32) -> Duration, next_due(DateTime<Utc>, &UsageSnapshot, Duration, u32) -> Option<DateTime<Utc>>, stagger_offsets(usize, Duration) -> Vec<Duration>, merge_result(Option<&UsageSnapshot>, UsageSnapshot) -> UsageSnapshot, spawn(AppHandle), schedule_all(&AppHandle), schedule_now(&AppHandle, &str), refresh_all(&AppHandle), run_due(&AppHandle), start_fetch(AppHandle, Account), UsageUpdated { account_id, snapshot }}`
  - `state::{AppPaths { data_dir, config_dir, home }` with `accounts_file() settings_file() cache_file() work_dir() aliases_sh() aliases_ps1()`, `Runtime { next_due, errors, in_flight, last_manual }`, `AppState { paths, shell, accounts, settings, cache, runtime, cli }` with `load(&AppHandle)`, `save_accounts()`, `save_cache()`, `save_settings()`}`
  - Event `"usage-updated"` with payload `{ accountId, snapshot }`.

- [ ] **Step 1: Write the failing tests for the pure scheduling logic**

Create `src-tauri/src/scheduler.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AccountStatus, UsageSnapshot, Window, WindowKind};
    use chrono::{Duration as ChronoDuration, TimeZone, Utc};
    use std::time::Duration;

    const FIVE_MIN: Duration = Duration::from_secs(300);

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap()
    }

    fn snap(status: AccountStatus, resets_in_min: Option<i64>) -> UsageSnapshot {
        UsageSnapshot {
            plan: Some("max".into()),
            windows: vec![Window {
                kind: WindowKind::FiveHour,
                used_pct: 40.0,
                resets_at: resets_in_min.map(|m| now() + ChronoDuration::minutes(m)),
            }],
            fetched_at: now(),
            status,
            last_error: None,
        }
    }

    #[test]
    fn backoff_doubles_and_caps_at_an_hour() {
        assert_eq!(backoff_delay(FIVE_MIN, 0), FIVE_MIN);
        assert_eq!(backoff_delay(FIVE_MIN, 1), Duration::from_secs(600));
        assert_eq!(backoff_delay(FIVE_MIN, 3), Duration::from_secs(2400));
        assert_eq!(backoff_delay(FIVE_MIN, 10), Duration::from_secs(3600));
    }

    #[test]
    fn next_due_uses_interval() {
        let due = next_due(now(), &snap(AccountStatus::Ok, Some(120)), FIVE_MIN, 0).unwrap();
        assert_eq!(due, now() + ChronoDuration::minutes(5));
    }

    #[test]
    fn next_due_brought_forward_by_reset() {
        let due = next_due(now(), &snap(AccountStatus::Ok, Some(2)), FIVE_MIN, 0).unwrap();
        assert_eq!(due, now() + ChronoDuration::minutes(2) + ChronoDuration::seconds(30));
    }

    #[test]
    fn needs_login_is_not_polled() {
        assert_eq!(next_due(now(), &snap(AccountStatus::NeedsLogin, None), FIVE_MIN, 0), None);
    }

    #[test]
    fn staggers_accounts_across_the_interval() {
        assert_eq!(
            stagger_offsets(3, FIVE_MIN),
            vec![Duration::ZERO, Duration::from_secs(100), Duration::from_secs(200)]
        );
        assert!(stagger_offsets(0, FIVE_MIN).is_empty());
    }

    #[test]
    fn merge_keeps_fresh_ok_snapshot() {
        let prev = snap(AccountStatus::Ok, Some(10));
        let mut fresh = snap(AccountStatus::Ok, Some(20));
        fresh.windows[0].used_pct = 50.0;
        assert_eq!(merge_result(Some(&prev), fresh.clone()), fresh);
    }

    #[test]
    fn merge_turns_error_into_stale_with_previous_windows() {
        let prev = snap(AccountStatus::Ok, Some(10));
        let fresh = crate::providers::failure(AccountStatus::Error("boom".into()), None);
        let merged = merge_result(Some(&prev), fresh);
        assert_eq!(merged.status, AccountStatus::Stale);
        assert_eq!(merged.windows, prev.windows);
        assert_eq!(merged.fetched_at, prev.fetched_at);
        assert_eq!(merged.last_error.as_deref(), Some("boom"));
    }

    #[test]
    fn merge_keeps_status_but_previous_windows_for_needs_login() {
        let prev = snap(AccountStatus::Ok, Some(10));
        let merged = merge_result(Some(&prev), crate::providers::failure(AccountStatus::NeedsLogin, None));
        assert_eq!(merged.status, AccountStatus::NeedsLogin);
        assert_eq!(merged.windows, prev.windows);
    }

    #[test]
    fn merge_without_previous_data_returns_fresh() {
        let fresh = crate::providers::failure(AccountStatus::CliMissing, None);
        assert_eq!(merge_result(None, fresh.clone()), fresh);
    }
}
```

Add `pub mod scheduler; pub mod state;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test scheduler::`
Expected: compile errors for the missing items. `state` is also missing; create an empty `src-tauri/src/state.rs` now so only the scheduler items fail.

- [ ] **Step 3: Implement the pure scheduling logic**

Add at the top of `src-tauri/src/scheduler.rs` (the runtime imports come in Step 6, once `AppState` exists):

```rust
use crate::model::{AccountStatus, UsageSnapshot};
use chrono::{DateTime, Utc};
use std::time::Duration;

pub const MAX_BACKOFF: Duration = Duration::from_secs(3600);
const TICK: Duration = Duration::from_secs(10);
const RESET_GRACE_SECS: i64 = 30;

pub fn backoff_delay(interval: Duration, consecutive_errors: u32) -> Duration {
    let factor = 2u32.saturating_pow(consecutive_errors.min(16));
    interval.saturating_mul(factor).min(MAX_BACKOFF.max(interval))
}

/// When the account should be fetched next. None means "wait for the user" (NeedsLogin).
/// A window reset that falls before the normal slot pulls the fetch forward.
pub fn next_due(now: DateTime<Utc>, snapshot: &UsageSnapshot, interval: Duration, errors: u32) -> Option<DateTime<Utc>> {
    if snapshot.status == AccountStatus::NeedsLogin {
        return None;
    }
    let delay = chrono::Duration::from_std(backoff_delay(interval, errors)).unwrap_or(chrono::Duration::hours(1));
    let base = now + delay;
    let reset = snapshot
        .windows
        .iter()
        .filter_map(|w| w.resets_at)
        .filter(|r| *r > now)
        .map(|r| r + chrono::Duration::seconds(RESET_GRACE_SECS))
        .min();
    Some(match reset {
        Some(r) if r < base => r,
        _ => base,
    })
}

pub fn stagger_offsets(count: usize, interval: Duration) -> Vec<Duration> {
    if count == 0 {
        return Vec::new();
    }
    let step = interval / count as u32;
    (0..count).map(|i| step * i as u32).collect()
}

/// Combines a fresh fetch with the previous snapshot so a failed fetch keeps showing old data.
pub fn merge_result(prev: Option<&UsageSnapshot>, fresh: UsageSnapshot) -> UsageSnapshot {
    let Some(prev) = prev.filter(|p| !p.windows.is_empty()) else { return fresh };
    match &fresh.status {
        AccountStatus::Ok => fresh,
        AccountStatus::Error(message) => UsageSnapshot {
            status: AccountStatus::Stale,
            last_error: Some(message.clone()),
            ..prev.clone()
        },
        _ => UsageSnapshot { status: fresh.status.clone(), last_error: fresh.last_error.clone(), ..prev.clone() },
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test scheduler::`
Expected: 9 passed. (`TICK` is unused until Step 6; ignore that warning.)

- [ ] **Step 5: Implement the app state**

Replace `src-tauri/src/state.rs` with:

```rust
use crate::cli::locator::{read_login_shell_env, ShellEnv};
use crate::model::Account;
use crate::providers::CliContext;
use crate::store::cache::UsageCache;
use crate::store::settings::Settings;
use crate::store::{read_json, write_json_atomic};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub home: PathBuf,
}

impl AppPaths {
    pub fn accounts_file(&self) -> PathBuf { self.data_dir.join("accounts.json") }
    pub fn settings_file(&self) -> PathBuf { self.data_dir.join("settings.json") }
    pub fn cache_file(&self) -> PathBuf { self.data_dir.join("usage-cache.json") }
    /// Neutral cwd for CLI runs, so no project hooks or settings apply.
    pub fn work_dir(&self) -> PathBuf { self.data_dir.join("work") }
    pub fn aliases_sh(&self) -> PathBuf { self.config_dir.join("aliases.sh") }
    pub fn aliases_ps1(&self) -> PathBuf { self.config_dir.join("aliases.ps1") }
}

#[derive(Debug, Clone, Default)]
pub struct Runtime {
    pub next_due: Option<DateTime<Utc>>,
    pub errors: u32,
    pub in_flight: bool,
    pub last_manual: Option<DateTime<Utc>>,
}

/// Shared state. std Mutexes are held only for short, non-async sections.
pub struct AppState {
    pub paths: AppPaths,
    pub shell: ShellEnv,
    pub accounts: Mutex<Vec<Account>>,
    pub settings: Mutex<Settings>,
    pub cache: Mutex<UsageCache>,
    pub runtime: Mutex<HashMap<String, Runtime>>,
    pub cli: Mutex<CliContext>,
}

impl AppState {
    pub fn load(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let paths = AppPaths {
            data_dir: app.path().app_data_dir()?,
            config_dir: app.path().app_config_dir()?,
            home: app.path().home_dir()?,
        };
        std::fs::create_dir_all(paths.work_dir())?;
        let shell = read_login_shell_env();
        let settings = read_json::<Settings>(&paths.settings_file()).normalized();
        let accounts: Vec<Account> = read_json(&paths.accounts_file());
        let mut cache: UsageCache = read_json(&paths.cache_file());
        cache.prune_sent(Utc::now());
        let cli = CliContext::resolve(&settings, &shell, &paths.home, paths.work_dir());
        log::info!("claude CLI: {:?}, codex CLI: {:?}", cli.claude, cli.codex);
        Ok(Self {
            paths,
            shell,
            accounts: Mutex::new(accounts),
            settings: Mutex::new(settings),
            cache: Mutex::new(cache),
            runtime: Mutex::new(HashMap::new()),
            cli: Mutex::new(cli),
        })
    }

    pub fn save_accounts(&self) {
        let accounts = self.accounts.lock().unwrap().clone();
        if let Err(e) = write_json_atomic(&self.paths.accounts_file(), &accounts) {
            log::error!("saving accounts failed: {e}");
        }
    }

    pub fn save_cache(&self) {
        let cache = self.cache.lock().unwrap().clone();
        if let Err(e) = write_json_atomic(&self.paths.cache_file(), &cache) {
            log::error!("saving cache failed: {e}");
        }
    }

    pub fn save_settings(&self) {
        let settings = self.settings.lock().unwrap().clone();
        if let Err(e) = write_json_atomic(&self.paths.settings_file(), &settings) {
            log::error!("saving settings failed: {e}");
        }
    }
}
```

- [ ] **Step 6: Implement the scheduler runtime**

Extend the imports at the top of `src-tauri/src/scheduler.rs`:

```rust
use crate::model::Account;
use crate::providers;
use crate::state::AppState;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
```

Append to `src-tauri/src/scheduler.rs`, above the tests:

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageUpdated {
    pub account_id: String,
    pub snapshot: UsageSnapshot,
}

/// Background loop: every tick, start fetches for accounts whose slot has come.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            run_due(&app);
            tokio::time::sleep(TICK).await;
        }
    });
}

/// Spreads first fetches across one interval; the first account is fetched immediately.
pub fn schedule_all(app: &AppHandle) {
    let state = app.state::<AppState>();
    let now = Utc::now();
    let accounts = state.accounts.lock().unwrap().clone();
    let interval = state.settings.lock().unwrap().interval();
    let mut runtime = state.runtime.lock().unwrap();
    runtime.retain(|id, _| accounts.iter().any(|a| &a.id == id));
    for (account, offset) in accounts.iter().zip(stagger_offsets(accounts.len(), interval)) {
        let entry = runtime.entry(account.id.clone()).or_default();
        entry.next_due = Some(now + chrono::Duration::from_std(offset).unwrap_or_default());
        entry.errors = 0;
    }
}

pub fn schedule_now(app: &AppHandle, account_id: &str) {
    let state = app.state::<AppState>();
    let mut runtime = state.runtime.lock().unwrap();
    let entry = runtime.entry(account_id.to_string()).or_default();
    entry.next_due = Some(Utc::now());
    entry.errors = 0;
}

pub fn refresh_all(app: &AppHandle) {
    let ids: Vec<String> = app.state::<AppState>().accounts.lock().unwrap().iter().map(|a| a.id.clone()).collect();
    for id in ids {
        schedule_now(app, &id);
    }
    run_due(app);
}

pub fn run_due(app: &AppHandle) {
    let state = app.state::<AppState>();
    let now = Utc::now();
    let due: Vec<Account> = {
        let accounts = state.accounts.lock().unwrap();
        let mut runtime = state.runtime.lock().unwrap();
        accounts
            .iter()
            .filter(|account| {
                let entry = runtime.entry(account.id.clone()).or_default();
                !entry.in_flight && entry.next_due.is_some_and(|due| due <= now)
            })
            .cloned()
            .collect()
    };
    for account in due {
        start_fetch(app.clone(), account);
    }
}

pub fn start_fetch(app: AppHandle, account: Account) {
    {
        let state = app.state::<AppState>();
        let mut runtime = state.runtime.lock().unwrap();
        let entry = runtime.entry(account.id.clone()).or_default();
        if entry.in_flight {
            return;
        }
        entry.in_flight = true;
    }
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let ctx = state.cli.lock().unwrap().clone();
        let fresh = providers::fetch_usage(&ctx, &account).await;
        let interval = state.settings.lock().unwrap().interval();
        let merged = {
            let mut cache = state.cache.lock().unwrap();
            let prev = cache.snapshots.get(&account.id).cloned();
            let merged = merge_result(prev.as_ref(), fresh.clone());
            cache.snapshots.insert(account.id.clone(), merged.clone());
            merged
        };
        {
            let mut runtime = state.runtime.lock().unwrap();
            let entry = runtime.entry(account.id.clone()).or_default();
            entry.in_flight = false;
            entry.errors = if fresh.status == AccountStatus::Ok { 0 } else { entry.errors + 1 };
            entry.next_due = next_due(Utc::now(), &merged, interval, entry.errors);
        }
        log::info!("fetched {} ({:?}): {:?}", account.label, account.provider, merged.status);
        state.save_cache();
        let _ = app.emit("usage-updated", UsageUpdated { account_id: account.id.clone(), snapshot: merged });
    });
}
```

- [ ] **Step 7: Verify the crate still compiles and the tests pass**

Run: `cd src-tauri && cargo test scheduler::`
Expected: 9 passed, no compile errors.

- [ ] **Step 8: Commit**

```bash
git add src-tauri
git commit -m "feat(core): schedule usage fetches with backoff and stale handling"
```

---

### Task 7: Tray, panel window and the read-only panel UI (includes importing existing accounts)

**Files:**
- Create: `src-tauri/src/tray.rs`, `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs` (full `run()`), `src-tauri/src/scheduler.rs` (call `tray::refresh`), `src-tauri/tauri.conf.json`
- Create: `src/types.ts`, `src/api.ts`, `src/format.ts`, `src/styles.css`, `src/components/WindowBar.tsx`, `src/components/AccountCard.tsx`, `src/components/AddAccountView.tsx` (import section only)
- Modify: `src/App.tsx`, `src/main.tsx`; delete `src/App.css` and `src/assets/`

**Interfaces:**
- Consumes: `AppState`, `scheduler::*`, `store::*`.
- Produces:
  - Rust: `tray::{Level, level_for_pct, snapshot_level, worst_level, tray_title, tooltip, icon_rgba, create, refresh, toggle_panel, show_panel, TRAY_ID}`
  - Commands: `list_accounts`, `get_snapshots`, `refresh_account(id)`, `set_pinned(id)`, `detect_existing`, `add_existing(provider, label, configDir, useDefaultDir)`
  - Helper `commands::accounts_changed(&AppHandle)`: saves, reschedules, emits `accounts-changed`, refreshes the tray. Later tasks extend it.
  - TS: `types.ts` and `api.ts` as shown.

- [ ] **Step 1: Write the failing tests for the tray logic**

Create `src-tauri/src/tray.rs` with:

```rust
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
    fn icon_is_rgba_of_expected_size() {
        assert_eq!(icon_rgba(Level::Red, 32).len(), 32 * 32 * 4);
    }
}
```

Add `pub mod tray; pub mod commands;` to `src-tauri/src/lib.rs`, and create an empty `src-tauri/src/commands.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test tray::`
Expected: compile errors for the missing items.

- [ ] **Step 3: Implement the tray**

Add above the tests in `src-tauri/src/tray.rs`:

```rust
use crate::model::{Account, AccountStatus, UsageSnapshot, WindowKind};
use crate::scheduler;
use crate::state::AppState;
use std::collections::HashMap;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_positioner::{Position, WindowExt};

pub const TRAY_ID: &str = "main";

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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test tray::`
Expected: 5 passed.

- [ ] **Step 5: Implement the commands**

Replace `src-tauri/src/commands.rs` with:

```rust
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
```

- [ ] **Step 6: Make every fetch refresh the tray**

In `src-tauri/src/scheduler.rs`, `start_fetch`, add `crate::tray::refresh(&app);` right after `state.save_cache();`.

- [ ] **Step 7: Wire `run()`**

Replace the template's `run()` in `src-tauri/src/lib.rs` (keep the `pub mod` lines) with:

```rust
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
            tray::create(app.handle())?;
            tray::refresh(app.handle());
            scheduler::schedule_all(app.handle());
            scheduler::spawn(app.handle().clone());
            if let Some(window) = app.get_webview_window("main") {
                let panel = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::Focused(false) = event {
                        let _ = panel.hide();
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
            commands::detect_existing,
            commands::add_existing,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

Delete the template's `greet` command if it is still present.

- [ ] **Step 8: Configure the panel window**

In `src-tauri/tauri.conf.json`, set `productName` to `"Usage Monitor"` and replace `app.windows` with:

```json
"windows": [
  {
    "label": "main",
    "title": "Usage Monitor",
    "width": 380,
    "height": 620,
    "visible": false,
    "decorations": false,
    "resizable": false,
    "alwaysOnTop": true,
    "skipTaskbar": true
  }
]
```

- [ ] **Step 9: Write the TS types, API and formatting helpers**

Create `src/types.ts`:

```ts
export type Provider = "claude" | "codex";

export interface Account {
  id: string;
  provider: Provider;
  label: string;
  configDir: string;
  useDefaultDir: boolean;
  pinned: boolean;
  aliasName: string;
  createdAt: string;
}

export type WindowKind =
  | { type: "fiveHour" }
  | { type: "weekly" }
  | { type: "weeklyScoped"; name: string }
  | { type: "other"; name: string };

export interface UsageWindow {
  kind: WindowKind;
  usedPct: number;
  resetsAt: string | null;
}

export type AccountStatus =
  | { type: "ok" }
  | { type: "stale" }
  | { type: "needsLogin" }
  | { type: "rateLimited" }
  | { type: "cliMissing" }
  | { type: "error"; message: string };

export interface UsageSnapshot {
  plan: string | null;
  windows: UsageWindow[];
  fetchedAt: string;
  status: AccountStatus;
  lastError: string | null;
}

export interface UsageUpdated {
  accountId: string;
  snapshot: UsageSnapshot;
}

export interface DetectedAccount {
  provider: Provider;
  configDir: string;
  useDefaultDir: boolean;
}
```

Create `src/api.ts`:

```ts
import { invoke } from "@tauri-apps/api/core";
import type { Account, DetectedAccount, Provider, UsageSnapshot } from "./types";

export const api = {
  listAccounts: () => invoke<Account[]>("list_accounts"),
  getSnapshots: () => invoke<Record<string, UsageSnapshot>>("get_snapshots"),
  refreshAccount: (id: string) => invoke<void>("refresh_account", { id }),
  setPinned: (id: string) => invoke<void>("set_pinned", { id }),
  detectExisting: () => invoke<DetectedAccount[]>("detect_existing"),
  addExisting: (provider: Provider, label: string, configDir: string, useDefaultDir: boolean) =>
    invoke<Account>("add_existing", { provider, label, configDir, useDefaultDir }),
};
```

Create `src/format.ts`:

```ts
import type { AccountStatus, Provider, WindowKind } from "./types";

export type Level = "green" | "yellow" | "red";

export function level(pct: number): Level {
  if (pct >= 80) return "red";
  if (pct >= 50) return "yellow";
  return "green";
}

export function windowLabel(kind: WindowKind): string {
  switch (kind.type) {
    case "fiveHour":
      return "5h";
    case "weekly":
      return "Week";
    case "weeklyScoped":
      return `Week · ${kind.name}`;
    case "other":
      return kind.name;
  }
}

/** "in 2h 10m" under a day, "Mon 09:00" beyond, "resetting…" once passed. */
export function formatReset(iso: string | null, now: Date): string {
  if (!iso) return "";
  const at = new Date(iso);
  const minutes = Math.round((at.getTime() - now.getTime()) / 60000);
  if (minutes <= 0) return "resetting…";
  if (minutes < 60 * 24) {
    const h = Math.floor(minutes / 60);
    const m = minutes % 60;
    return h > 0 ? `in ${h}h ${m}m` : `in ${m}m`;
  }
  return at.toLocaleString("en-US", { weekday: "short", hour: "2-digit", minute: "2-digit", hour12: false });
}

export function formatAgo(iso: string, now: Date): string {
  const minutes = Math.floor((now.getTime() - new Date(iso).getTime()) / 60000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  return `${Math.floor(minutes / 60)}h ago`;
}

export function statusLabel(status: AccountStatus): string | null {
  switch (status.type) {
    case "ok":
      return null;
    case "stale":
      return "Stale";
    case "needsLogin":
      return "Signed out";
    case "rateLimited":
      return "Rate limited";
    case "cliMissing":
      return "CLI not found";
    case "error":
      return "Error";
  }
}

export function providerName(provider: Provider): string {
  return provider === "claude" ? "Claude" : "Codex";
}

export function planLabel(plan: string | null): string {
  if (!plan) return "";
  return plan.charAt(0).toUpperCase() + plan.slice(1);
}
```

- [ ] **Step 10: Write the components and the app shell**

Create `src/components/WindowBar.tsx`:

```tsx
import { formatReset, level, windowLabel } from "../format";
import type { UsageWindow } from "../types";

interface Props {
  window: UsageWindow;
  now: Date;
  dimmed: boolean;
}

export function WindowBar({ window, now, dimmed }: Props) {
  const passed = window.resetsAt !== null && new Date(window.resetsAt).getTime() <= now.getTime();
  const pct = passed ? 0 : window.usedPct;
  return (
    <div className={`bar-row ${dimmed ? "dimmed" : ""}`}>
      <span className="bar-label">{windowLabel(window.kind)}</span>
      <div className="bar-track">
        <div className={`bar-fill ${level(pct)}`} style={{ width: `${Math.min(pct, 100)}%` }} />
      </div>
      <span className="bar-pct">{Math.round(pct)}%</span>
      <span className="bar-reset">{formatReset(window.resetsAt, now)}</span>
    </div>
  );
}
```

Create `src/components/AccountCard.tsx`:

```tsx
import { formatAgo, planLabel, providerName, statusLabel } from "../format";
import type { Account, UsageSnapshot } from "../types";
import { WindowBar } from "./WindowBar";

interface Props {
  account: Account;
  snapshot: UsageSnapshot | undefined;
  now: Date;
  onPin: () => void;
  onRefresh: () => void;
}

export function AccountCard({ account, snapshot, now, onPin, onRefresh }: Props) {
  const status = snapshot ? statusLabel(snapshot.status) : "Waiting for first update";
  const dimmed = snapshot !== undefined && snapshot.status.type !== "ok";
  const hasFiveHour = snapshot?.windows.some((w) => w.kind.type === "fiveHour") ?? false;
  return (
    <section className="card">
      <header className="card-header">
        <div>
          <span className={`provider ${account.provider}`}>{providerName(account.provider)}</span>
          <strong>{account.label}</strong>
          {snapshot?.plan && <span className="plan">{planLabel(snapshot.plan)}</span>}
        </div>
        <div className="card-actions">
          <button title={account.pinned ? "Shown in tray" : "Show in tray"} onClick={onPin}>
            {account.pinned ? "★" : "☆"}
          </button>
          <button title="Refresh" onClick={onRefresh}>
            ↻
          </button>
        </div>
      </header>
      {status && <p className={`status ${snapshot?.status.type ?? "pending"}`}>{status}</p>}
      {snapshot && snapshot.windows.length > 0 && (
        <>
          {!hasFiveHour && (
            <div className="bar-row muted">
              <span className="bar-label">5h</span>
              <span>No 5h limit</span>
            </div>
          )}
          {snapshot.windows.map((w, i) => (
            <WindowBar key={i} window={w} now={now} dimmed={dimmed} />
          ))}
          <p className="updated">Updated {formatAgo(snapshot.fetchedAt, now)}</p>
        </>
      )}
    </section>
  );
}
```

Create `src/components/AddAccountView.tsx` (the import section only; Task 8 adds the rest):

```tsx
import { useEffect, useState } from "react";
import { api } from "../api";
import { providerName } from "../format";
import type { DetectedAccount } from "../types";

interface Props {
  onDone: () => void;
}

export function AddAccountView({ onDone }: Props) {
  const [detected, setDetected] = useState<DetectedAccount[]>([]);
  const [labels, setLabels] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.detectExisting().then(setDetected).catch((e) => setError(String(e)));
  }, []);

  async function importAccount(d: DetectedAccount) {
    try {
      await api.addExisting(d.provider, labels[d.configDir] ?? "Main", d.configDir, d.useDefaultDir);
      onDone();
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="view">
      <header className="view-header">
        <button onClick={onDone}>← Back</button>
        <h2>Add account</h2>
      </header>
      {error && <p className="error">{error}</p>}
      <h3>Already signed in on this computer</h3>
      {detected.length === 0 && <p className="muted">No unregistered sessions found.</p>}
      {detected.map((d) => (
        <div key={d.configDir} className="detected">
          <div>
            <strong>{providerName(d.provider)}</strong>
            <code>{d.configDir}</code>
          </div>
          <input
            value={labels[d.configDir] ?? "Main"}
            onChange={(e) => setLabels({ ...labels, [d.configDir]: e.target.value })}
          />
          <button onClick={() => importAccount(d)}>Import</button>
        </div>
      ))}
    </div>
  );
}
```

Replace `src/App.tsx`:

```tsx
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { api } from "./api";
import { AccountCard } from "./components/AccountCard";
import { AddAccountView } from "./components/AddAccountView";
import type { Account, UsageSnapshot, UsageUpdated } from "./types";

type View = { name: "panel" } | { name: "add" };

export default function App() {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [snapshots, setSnapshots] = useState<Record<string, UsageSnapshot>>({});
  const [view, setView] = useState<View>({ name: "panel" });
  const [now, setNow] = useState(new Date());
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    setAccounts(await api.listAccounts());
    setSnapshots(await api.getSnapshots());
  }, []);

  useEffect(() => {
    reload();
    const timer = setInterval(() => setNow(new Date()), 30_000);
    const offUsage = listen<UsageUpdated>("usage-updated", (e) =>
      setSnapshots((s) => ({ ...s, [e.payload.accountId]: e.payload.snapshot })),
    );
    const offAccounts = listen("accounts-changed", () => reload());
    return () => {
      clearInterval(timer);
      offUsage.then((off) => off());
      offAccounts.then((off) => off());
    };
  }, [reload]);

  const run = (action: Promise<unknown>) => action.catch((e) => setError(String(e)));
  const ordered = [...accounts].sort(
    (a, b) => Number(b.pinned) - Number(a.pinned) || a.createdAt.localeCompare(b.createdAt),
  );

  if (view.name === "add") return <AddAccountView onDone={() => setView({ name: "panel" })} />;

  return (
    <main className="panel">
      <header className="panel-header">
        <h1>Usage</h1>
        <button onClick={() => setView({ name: "add" })}>+ Add account</button>
      </header>
      {error && (
        <p className="error" onClick={() => setError(null)}>
          {error}
        </p>
      )}
      {ordered.length === 0 && <p className="muted">No accounts yet. Add one to start tracking usage.</p>}
      {ordered.map((account) => (
        <AccountCard
          key={account.id}
          account={account}
          snapshot={snapshots[account.id]}
          now={now}
          onPin={() => run(api.setPinned(account.id))}
          onRefresh={() => run(api.refreshAccount(account.id))}
        />
      ))}
    </main>
  );
}
```

Replace `src/main.tsx` (no StrictMode: login views must start exactly once):

```tsx
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<App />);
```

Create `src/styles.css`:

```css
:root {
  --bg: #ffffff; --fg: #1f2328; --muted: #656d76; --card: #f6f8fa; --border: #d0d7de;
  --green: #2ea043; --yellow: #d29922; --red: #da3633; --accent: #0969da;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Ubuntu, sans-serif;
  font-size: 13px; color: var(--fg); background: var(--bg);
}
@media (prefers-color-scheme: dark) {
  :root { --bg: #0d1117; --fg: #e6edf3; --muted: #8d96a0; --card: #161b22; --border: #30363d; --accent: #4493f8; }
}
* { box-sizing: border-box; }
body { margin: 0; background: var(--bg); }
button { font: inherit; color: var(--fg); background: var(--card); border: 1px solid var(--border); border-radius: 6px; padding: 4px 8px; cursor: pointer; }
button:hover { border-color: var(--accent); }
input, select { font: inherit; color: var(--fg); background: var(--bg); border: 1px solid var(--border); border-radius: 6px; padding: 4px 6px; }
code { font-size: 11px; color: var(--muted); word-break: break-all; }
.panel, .view { padding: 12px; display: flex; flex-direction: column; gap: 10px; }
.panel-header, .view-header { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
.panel-header h1, .view-header h2 { margin: 0; font-size: 15px; }
.card { background: var(--card); border: 1px solid var(--border); border-radius: 8px; padding: 10px; display: flex; flex-direction: column; gap: 6px; }
.card-header { display: flex; justify-content: space-between; align-items: center; }
.card-header > div:first-child { display: flex; gap: 6px; align-items: center; }
.card-actions { display: flex; gap: 4px; }
.provider { font-size: 11px; padding: 1px 6px; border-radius: 10px; border: 1px solid var(--border); }
.plan { font-size: 11px; color: var(--muted); }
.status { margin: 0; font-size: 12px; color: var(--yellow); }
.status.needsLogin, .status.error, .status.cliMissing { color: var(--red); }
.bar-row { display: grid; grid-template-columns: 70px 1fr 38px 86px; align-items: center; gap: 6px; }
.bar-row.muted { grid-template-columns: 70px 1fr; color: var(--muted); }
.bar-row.dimmed { opacity: 0.5; }
.bar-label { color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.bar-track { height: 8px; background: var(--border); border-radius: 4px; overflow: hidden; }
.bar-fill { height: 100%; }
.bar-fill.green { background: var(--green); } .bar-fill.yellow { background: var(--yellow); } .bar-fill.red { background: var(--red); }
.bar-pct { text-align: right; font-variant-numeric: tabular-nums; }
.bar-reset { color: var(--muted); font-size: 11px; text-align: right; }
.updated, .muted { margin: 0; color: var(--muted); font-size: 11px; }
.error { margin: 0; color: var(--red); cursor: pointer; }
.detected { display: grid; grid-template-columns: 1fr 90px auto; gap: 6px; align-items: center; }
.detected > div { display: flex; flex-direction: column; }
h3 { margin: 6px 0 0; font-size: 12px; color: var(--muted); font-weight: 600; }
```

Delete `src/App.css` and `src/assets/`, and remove any imports of them.

- [ ] **Step 11: Typecheck and compile**

Run: `pnpm exec tsc --noEmit && cd src-tauri && cargo build`
Expected: no errors.

- [ ] **Step 12: Manual check (the milestone demo)**

Run: `pnpm tauri dev` (in the background) and check:
- The tray icon appears (gray at first).
- Clicking it opens the panel near the icon. On Linux, use the "Open panel" menu item.
- "+ Add account" lists `~/.claude` and `~/.codex` (on this machine `~/.claude` is listed via `CLAUDE_CONFIG_DIR`, with `useDefaultDir: false`).
- After importing both, their cards fill in with bars within about 10 s.
- The macOS menu bar shows `N% · M%` for the pinned account.

Stop the dev server when done.

- [ ] **Step 13: Commit**

```bash
git add -A
git commit -m "feat(app): tray panel showing usage of imported accounts"
```

---

### Task 8: Sign-in flow (add new, reconnect) and account removal

**Files:**
- Create: `src-tauri/src/login.rs`, `src/components/LoginView.tsx`
- Modify: `src-tauri/src/state.rs` (add `login` field), `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs` (module + handlers), `src/api.ts`, `src/types.ts`, `src/App.tsx`, `src/components/AddAccountView.tsx`, `src/components/AccountCard.tsx`, `src/styles.css`

**Interfaces:**
- Consumes: `providers::{login_args, logout_args, is_logged_in}`, `store::accounts::{proposed_config_dir, can_delete_dir}`, `scheduler::{schedule_now, run_due}`, `commands::accounts_changed`.
- Produces:
  - `login::{LoginProgress { url, needs_code }, scan_login_output(&str) -> LoginProgress, LoginManager` with async methods `start(&self, AppHandle, Account, Option<PathBuf>, bool) -> Result<(), String>`, `submit_code(&self, &str)`, `cancel(&self)`}`
  - Events: `"login-progress"` with payload `{accountId, url, needsCode}`, and `"login-finished"` with payload `{accountId, ok, error}`.
  - Commands: `propose_config_dir(provider, label)`, `add_account(provider, label, configDir)`, `reconnect(id)`, `submit_login_code(code)`, `cancel_login()`, `remove_account(id, logout, deleteDir)`.

- [ ] **Step 1: Write the failing tests for output scanning**

Create `src-tauri/src/login.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_url_from_osc8_claude_output_and_detects_code_prompt() {
        let text = "Opening browser to sign in…\nIf the browser didn't open, visit: \u{1b}]8;;https://claude.com/cai/oauth/authorize?code=true&state=abc\u{1b}\\https://claude.com/cai/oauth/authorize?code=true&state=abc\u{1b}]8;;\u{1b}\\\nPaste code here if prompted > ";
        let progress = scan_login_output(text);
        assert_eq!(progress.url.as_deref(), Some("https://claude.com/cai/oauth/authorize?code=true&state=abc"));
        assert!(progress.needs_code);
    }

    #[test]
    fn prefers_auth_url_over_localhost_for_codex() {
        let text = "Starting local login server on http://localhost:1455.\nIf your browser did not open, navigate to this URL to authenticate:\n\nhttps://auth.openai.com/oauth/authorize?client_id=x\n";
        let progress = scan_login_output(text);
        assert_eq!(progress.url.as_deref(), Some("https://auth.openai.com/oauth/authorize?client_id=x"));
        assert!(!progress.needs_code);
    }

    #[test]
    fn nothing_found_yet() {
        assert_eq!(scan_login_output("Opening browser"), LoginProgress::default());
    }
}
```

Add `pub mod login;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test login::`
Expected: compile errors for the missing `scan_login_output` / `LoginProgress`.

- [ ] **Step 3: Implement the scanner and the login manager**

Add above the tests in `src-tauri/src/login.rs`:

```rust
use crate::commands::accounts_changed;
use crate::model::Account;
use crate::providers;
use crate::scheduler;
use crate::state::AppState;
use serde::Serialize;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::ChildStdin;
use tokio::sync::{oneshot, Mutex};

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginProgress {
    pub url: Option<String>,
    pub needs_code: bool,
}

/// Finds the first https URL (skipping OSC-8 escape codes) and whether the CLI asks for a code.
pub fn scan_login_output(text: &str) -> LoginProgress {
    let url = text
        .split(|c: char| c.is_whitespace() || c == '\u{1b}' || c == '\u{7}')
        .find_map(|token| token.find("https://").map(|i| token[i..].to_string()));
    LoginProgress { url, needs_code: text.to_lowercase().contains("paste code") }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LoginProgressEvent {
    account_id: String,
    url: Option<String>,
    needs_code: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginFinished {
    pub account_id: String,
    pub ok: bool,
    pub error: Option<String>,
}

struct Session {
    stdin: Option<ChildStdin>,
    cancel: Option<oneshot::Sender<()>>,
}

/// One sign-in at a time: `codex login` binds a fixed local port.
#[derive(Default)]
pub struct LoginManager {
    session: Mutex<Option<Session>>,
}

impl LoginManager {
    /// Spawns the CLI login for `account`. When `is_new` and sign-in fails, the account is
    /// removed and `cleanup_dir` (a dir the app created) is deleted if still empty.
    pub async fn start(&self, app: AppHandle, account: Account, cleanup_dir: Option<PathBuf>, is_new: bool) -> Result<(), String> {
        let mut guard = self.session.lock().await;
        if guard.is_some() {
            return Err("Another sign-in is already in progress.".into());
        }
        let ctx = app.state::<AppState>().cli.lock().unwrap().clone();
        let cmd = ctx
            .command(&account, providers::login_args(account.provider))
            .ok_or_else(|| format!("{} CLI not found.", account.provider.cli_name()))?;
        let mut command = cmd.to_command();
        command.stdin(Stdio::piped()).kill_on_drop(true);
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        let (cancel_tx, cancel_rx) = oneshot::channel();
        *guard = Some(Session { stdin: child.stdin.take(), cancel: Some(cancel_tx) });
        drop(guard);

        let output = Arc::new(StdMutex::new(String::new()));
        if let Some(stdout) = child.stdout.take() {
            pump(app.clone(), account.id.clone(), stdout, output.clone());
        }
        if let Some(stderr) = child.stderr.take() {
            pump(app.clone(), account.id.clone(), stderr, output.clone());
        }

        tauri::async_runtime::spawn(async move {
            let exited = tokio::select! {
                _ = child.wait() => true,
                _ = cancel_rx => false,
            };
            if !exited {
                let _ = child.kill().await;
            }
            let state = app.state::<AppState>();
            state.login.clear().await;
            let ok = exited && providers::is_logged_in(&ctx, &account).await.unwrap_or(false);
            if ok {
                scheduler::schedule_now(&app, &account.id);
                scheduler::run_due(&app);
            } else if is_new {
                state.accounts.lock().unwrap().retain(|a| a.id != account.id);
                if let Some(dir) = cleanup_dir {
                    let _ = std::fs::remove_dir(dir); // only succeeds when empty
                }
                accounts_changed(&app);
            }
            let error = match (ok, exited) {
                (true, _) => None,
                (false, false) => Some("Sign-in cancelled.".to_string()),
                (false, true) => Some("Sign-in did not complete.".to_string()),
            };
            let _ = app.emit("login-finished", LoginFinished { account_id: account.id.clone(), ok, error });
        });
        Ok(())
    }

    pub async fn submit_code(&self, code: &str) -> Result<(), String> {
        let mut guard = self.session.lock().await;
        let session = guard.as_mut().ok_or("No sign-in in progress.")?;
        let stdin = session.stdin.as_mut().ok_or("This sign-in does not accept a code.")?;
        stdin.write_all(format!("{}\n", code.trim()).as_bytes()).await.map_err(|e| e.to_string())?;
        stdin.flush().await.map_err(|e| e.to_string())
    }

    pub async fn cancel(&self) {
        if let Some(session) = self.session.lock().await.as_mut() {
            if let Some(tx) = session.cancel.take() {
                let _ = tx.send(());
            }
        }
    }

    async fn clear(&self) {
        *self.session.lock().await = None;
    }
}

/// Reads in chunks, not lines: Claude's code prompt has no trailing newline.
fn pump<R: AsyncRead + Unpin + Send + 'static>(app: AppHandle, account_id: String, mut reader: R, output: Arc<StdMutex<String>>) {
    tauri::async_runtime::spawn(async move {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf).await {
            if n == 0 {
                break;
            }
            let progress = {
                let mut text = output.lock().unwrap();
                text.push_str(&String::from_utf8_lossy(&buf[..n]));
                scan_login_output(&text)
            };
            let _ = app.emit(
                "login-progress",
                LoginProgressEvent { account_id: account_id.clone(), url: progress.url, needs_code: progress.needs_code },
            );
        }
    });
}
```

Add the manager to `AppState` in `src-tauri/src/state.rs`: add the field `pub login: crate::login::LoginManager,` and initialize it with `login: crate::login::LoginManager::default(),` in `load`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test login::`
Expected: 3 passed.

- [ ] **Step 5: Add the commands**

Append to `src-tauri/src/commands.rs`:

```rust
use crate::cli::run::run;
use crate::providers;
use crate::store::accounts::{can_delete_dir, proposed_config_dir as propose};
use std::time::Duration;

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
    let dir = PathBuf::from(config_dir.trim());
    let account = {
        let accounts = state.accounts.lock().unwrap();
        validate_new(provider, &dir, &accounts)?;
        new_account(provider, &label, dir.clone(), false, &accounts)
    };
    let created = !dir.exists();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
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
        std::fs::remove_dir_all(&account.config_dir).map_err(|e| format!("Could not delete directory: {e}"))?;
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
```

Register the new commands in `generate_handler!` in `src-tauri/src/lib.rs`: `commands::propose_config_dir, commands::add_account, commands::reconnect, commands::submit_login_code, commands::cancel_login, commands::remove_account`.

- [ ] **Step 6: Extend the TS types and API**

Append to `src/types.ts`:

```ts
export interface LoginProgress {
  accountId: string;
  url: string | null;
  needsCode: boolean;
}

export interface LoginFinished {
  accountId: string;
  ok: boolean;
  error: string | null;
}
```

Add to the `api` object in `src/api.ts`:

```ts
  proposeConfigDir: (provider: Provider, label: string) => invoke<string>("propose_config_dir", { provider, label }),
  addAccount: (provider: Provider, label: string, configDir: string) =>
    invoke<Account>("add_account", { provider, label, configDir }),
  reconnect: (id: string) => invoke<void>("reconnect", { id }),
  submitLoginCode: (code: string) => invoke<void>("submit_login_code", { code }),
  cancelLogin: () => invoke<void>("cancel_login"),
  removeAccount: (id: string, logout: boolean, deleteDir: boolean) =>
    invoke<void>("remove_account", { id, logout, deleteDir }),
```

- [ ] **Step 7: Write the login view**

Create `src/components/LoginView.tsx`:

```tsx
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { LoginFinished, LoginProgress } from "../types";

interface Props {
  title: string;
  /** Starts the CLI sign-in; called once, after event listeners are attached. */
  start: () => Promise<unknown>;
  onDone: () => void;
}

export function LoginView({ title, start, onDone }: Props) {
  const [progress, setProgress] = useState<LoginProgress | null>(null);
  const [code, setCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [finished, setFinished] = useState(false);
  const started = useRef(false);

  useEffect(() => {
    const offProgress = listen<LoginProgress>("login-progress", (e) => setProgress(e.payload));
    const offFinished = listen<LoginFinished>("login-finished", (e) => {
      setFinished(true);
      if (e.payload.ok) onDone();
      else setError(e.payload.error ?? "Sign-in failed.");
    });
    Promise.all([offProgress, offFinished]).then(() => {
      if (started.current) return;
      started.current = true;
      start().catch((e) => {
        setFinished(true);
        setError(String(e));
      });
    });
    return () => {
      offProgress.then((off) => off());
      offFinished.then((off) => off());
    };
  }, [start, onDone]);

  async function submit() {
    try {
      await api.submitLoginCode(code);
      setCode("");
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="view">
      <header className="view-header">
        <h2>{title}</h2>
      </header>
      {!finished && <p>Waiting for authorization in your browser…</p>}
      {progress?.url && !finished && (
        <p className="muted">
          Browser did not open?{" "}
          <a href="#" onClick={() => openUrl(progress.url!)}>
            Open the sign-in page
          </a>
        </p>
      )}
      {progress?.needsCode && !finished && (
        <div className="code-row">
          <input placeholder="Paste the code shown in the browser" value={code} onChange={(e) => setCode(e.target.value)} />
          <button disabled={!code.trim()} onClick={submit}>
            Submit
          </button>
        </div>
      )}
      {error && <p className="error">{error}</p>}
      {finished ? <button onClick={onDone}>Back</button> : <button onClick={() => api.cancelLogin()}>Cancel</button>}
    </div>
  );
}
```

- [ ] **Step 8: Add "Add new" and "existing directory" to the add view**

In `src/components/AddAccountView.tsx`:

1. Change the props to `interface Props { onDone: () => void; onStartLogin: (title: string, start: () => Promise<unknown>) => void; }`.
2. Add `import type { DetectedAccount, Provider } from "../types";` (replacing the old type import).
3. Add this state and logic inside the component, after the existing state:

```tsx
  const [provider, setProvider] = useState<Provider>("claude");
  const [label, setLabel] = useState("");
  const [dir, setDir] = useState("");
  const [dirEdited, setDirEdited] = useState(false);
  const [alreadySignedIn, setAlreadySignedIn] = useState(false);

  useEffect(() => {
    if (dirEdited || !label.trim()) return;
    api.proposeConfigDir(provider, label).then(setDir);
  }, [provider, label, dirEdited]);

  async function submitNew() {
    if (alreadySignedIn) {
      try {
        await api.addExisting(provider, label, dir, false);
        onDone();
      } catch (e) {
        setError(String(e));
      }
      return;
    }
    onStartLogin(`Sign in to ${providerName(provider)} · ${label}`, () => api.addAccount(provider, label, dir));
  }
```

4. Append this JSX before the closing `</div>` of the view:

```tsx
      <h3>New account</h3>
      <div className="form">
        <label>
          Provider
          <select value={provider} onChange={(e) => setProvider(e.target.value as Provider)}>
            <option value="claude">Claude</option>
            <option value="codex">Codex</option>
          </select>
        </label>
        <label>
          Label
          <input placeholder="Personal" value={label} onChange={(e) => setLabel(e.target.value)} />
        </label>
        <label>
          Config directory
          <input
            value={dir}
            onChange={(e) => {
              setDir(e.target.value);
              setDirEdited(true);
            }}
          />
        </label>
        <label className="checkbox">
          <input type="checkbox" checked={alreadySignedIn} onChange={(e) => setAlreadySignedIn(e.target.checked)} />
          This directory is already signed in
        </label>
        <button disabled={!label.trim() || !dir.trim()} onClick={submitNew}>
          {alreadySignedIn ? "Add" : "Sign in"}
        </button>
      </div>
```

- [ ] **Step 9: Add reconnect and remove to the card**

In `src/components/AccountCard.tsx`:

1. Extend `Props` with `onReconnect: () => void; onRemove: (logout: boolean, deleteDir: boolean) => void;`.
2. Add `import { useState } from "react";`, and inside the component add `const [removing, setRemoving] = useState(false); const [logout, setLogout] = useState(false); const [deleteDir, setDeleteDir] = useState(false);`.
3. Add a `✕` button to `.card-actions`: `<button title="Remove" onClick={() => setRemoving(true)}>✕</button>`.
4. Right after the status paragraph, add:

```tsx
      {snapshot?.status.type === "needsLogin" && <button onClick={onReconnect}>Reconnect</button>}
      {removing && (
        <div className="confirm">
          <p>Remove {account.label} from Usage Monitor?</p>
          <label className="checkbox">
            <input type="checkbox" checked={logout} onChange={(e) => setLogout(e.target.checked)} />
            Also sign out of the CLI
          </label>
          {!account.useDefaultDir && (
            <label className="checkbox">
              <input type="checkbox" checked={deleteDir} onChange={(e) => setDeleteDir(e.target.checked)} />
              Also delete <code>{account.configDir}</code>
            </label>
          )}
          <div className="card-actions">
            <button onClick={() => onRemove(logout, deleteDir)}>Remove</button>
            <button onClick={() => setRemoving(false)}>Cancel</button>
          </div>
        </div>
      )}
```

- [ ] **Step 10: Route the new views in the app**

In `src/App.tsx`:

1. Change the view type to `type View = { name: "panel" } | { name: "add" } | { name: "login"; title: string; start: () => Promise<unknown> };`.
2. Import `LoginView`, and define `const toPanel = useCallback(() => setView({ name: "panel" }), []);`.
3. Replace the `if (view.name === "add")` line with:

```tsx
  if (view.name === "add")
    return <AddAccountView onDone={toPanel} onStartLogin={(title, start) => setView({ name: "login", title, start })} />;
  if (view.name === "login") return <LoginView title={view.title} start={view.start} onDone={toPanel} />;
```

4. Pass these props to `AccountCard`:

```tsx
          onReconnect={() => setView({ name: "login", title: `Reconnect ${account.label}`, start: () => api.reconnect(account.id) })}
          onRemove={(logout, deleteDir) => run(api.removeAccount(account.id, logout, deleteDir))}
```

Append to `src/styles.css`:

```css
.form { display: flex; flex-direction: column; gap: 8px; }
.form label { display: flex; flex-direction: column; gap: 3px; color: var(--muted); font-size: 12px; }
.form label.checkbox, .checkbox { flex-direction: row; align-items: center; gap: 6px; display: flex; font-size: 12px; }
.confirm { border-top: 1px solid var(--border); padding-top: 6px; display: flex; flex-direction: column; gap: 6px; }
.confirm p { margin: 0; }
.code-row { display: flex; gap: 6px; }
.code-row input { flex: 1; }
a { color: var(--accent); }
```

- [ ] **Step 11: Typecheck and compile**

Run: `pnpm exec tsc --noEmit && cd src-tauri && cargo build`
Expected: no errors.

- [ ] **Step 12: Manual check**

Run: `pnpm tauri dev` and check:
- Adding a new Claude account ("Test", default dir) opens the browser.
  - Cancelling returns to "Sign-in cancelled", the account disappears, and `~/.claude-test` is removed if it is empty.
  - Signing in completes and the card fills in.
- Removing that account with "Also sign out" and "Also delete" removes `~/.claude-test`.
- On the imported main account, the "delete" checkbox is not offered when `useDefaultDir` is true. When `useDefaultDir` is false and the directory is `~/.claude`, the command refuses with "protected".

Stop the dev server.

- [ ] **Step 13: Commit**

```bash
git add -A
git commit -m "feat(app): add, reconnect and remove accounts via cli sign-in"
```

---

### Task 9: Shell aliases

**Files:**
- Create: `src-tauri/src/aliases.rs`
- Modify: `src-tauri/src/commands.rs` (`accounts_changed` writes alias files, plus new commands), `src-tauri/src/lib.rs`, `src/api.ts`, `src/types.ts`, `src/components/AccountCard.tsx`, `src/App.tsx`

**Interfaces:**
- Consumes: `model::Account`, `state::AppPaths`, `cli::locator::ShellEnv`.
- Produces:
  - `aliases::{render_sh(&[Account]) -> String, render_ps1(&[Account]) -> String, alias_line(&Account) -> String, sh_source_block(&Path) -> String, ps1_source_block(&Path) -> String, install_block(&str, &str) -> String, remove_block(&str) -> String, write_alias_files(&AppPaths, &[Account]) -> io::Result<()>, install(&AppPaths, &ShellEnv) -> io::Result<Vec<PathBuf>>, uninstall(&AppPaths, &ShellEnv) -> io::Result<()>, installed_targets(&AppPaths, &ShellEnv) -> Vec<PathBuf>}`
  - Commands: `alias_line(id) -> String`, `aliases_status() -> {installed, targets}`, `install_aliases()`, `uninstall_aliases()`.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/aliases.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Account, Provider};
    use chrono::Utc;
    use std::path::PathBuf;

    fn account(provider: Provider, alias: &str, dir: &str, use_default_dir: bool) -> Account {
        Account {
            id: alias.into(),
            provider,
            label: alias.into(),
            config_dir: PathBuf::from(dir),
            use_default_dir,
            pinned: false,
            alias_name: alias.into(),
            created_at: Utc::now(),
        }
    }

    #[test]
    fn renders_sh_aliases() {
        let out = render_sh(&[
            account(Provider::Claude, "claude-personal", "/Users/me/.claude-personal", false),
            account(Provider::Codex, "codex-main", "/Users/me/.codex", true),
        ]);
        assert!(out.starts_with(HEADER));
        assert!(out.contains("alias claude-personal='CLAUDE_CONFIG_DIR=\"/Users/me/.claude-personal\" claude'\n"));
        assert!(out.contains("alias codex-main='env -u CODEX_HOME codex'\n"));
    }

    #[test]
    fn escapes_quotes_in_sh_paths() {
        let out = render_sh(&[account(Provider::Claude, "claude-x", "/Users/o'neil/.claude-x", false)]);
        assert!(out.contains(r#"alias claude-x='CLAUDE_CONFIG_DIR="/Users/o'\''neil/.claude-x" claude'"#));
    }

    #[test]
    fn renders_powershell_functions_that_restore_env() {
        let out = render_ps1(&[account(Provider::Claude, "claude-work", r"C:\Users\me\.claude-work", false)]);
        assert!(out.contains("function claude-work {"));
        assert!(out.contains(r"$env:CLAUDE_CONFIG_DIR = 'C:\Users\me\.claude-work'"));
        assert!(out.contains("try { claude @args } finally {"));
        let default = render_ps1(&[account(Provider::Codex, "codex-main", r"C:\Users\me\.codex", true)]);
        assert!(default.contains("Remove-Item Env:CODEX_HOME -ErrorAction SilentlyContinue\n  try { codex @args }"));
    }

    #[test]
    fn install_block_is_idempotent_and_removable() {
        let block = sh_source_block(Path::new("/cfg/aliases.sh"));
        let original = "export FOO=1\n";
        let once = install_block(original, &block);
        let twice = install_block(&once, &block);
        assert_eq!(once, twice);
        assert!(once.contains(BLOCK_START) && once.contains("/cfg/aliases.sh"));
        assert_eq!(remove_block(&once), original);
        assert_eq!(install_block("", &block), block);
    }

    #[test]
    fn remove_block_leaves_unmarked_content_alone() {
        assert_eq!(remove_block("a\nb\n"), "a\nb\n");
    }
}
```

Add `pub mod aliases;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test aliases::`
Expected: compile errors for the missing items.

- [ ] **Step 3: Implement**

Add above the tests in `src-tauri/src/aliases.rs`:

```rust
use crate::cli::locator::ShellEnv;
use crate::model::Account;
use crate::state::AppPaths;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const HEADER: &str = "# Generated by Usage Monitor. Changes are overwritten.";
pub const BLOCK_START: &str = "# >>> usage-monitor aliases >>>";
pub const BLOCK_END: &str = "# <<< usage-monitor aliases <<<";

fn sh_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn ps_single_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn sh_double_quote_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$").replace('`', "\\`")
}

fn sh_line(account: &Account) -> String {
    let var = account.provider.config_env_var();
    let cli = account.provider.cli_name();
    let body = if account.use_default_dir {
        format!("env -u {var} {cli}")
    } else {
        format!("{var}=\"{}\" {cli}", sh_double_quote_escape(&account.config_dir.to_string_lossy()))
    };
    format!("alias {}={}", account.alias_name, sh_single_quote(&body))
}

fn ps1_function(account: &Account) -> String {
    let var = account.provider.config_env_var();
    let cli = account.provider.cli_name();
    let apply = if account.use_default_dir {
        format!("Remove-Item Env:{var} -ErrorAction SilentlyContinue")
    } else {
        format!("$env:{var} = {}", ps_single_quote(&account.config_dir.to_string_lossy()))
    };
    format!(
        "function {name} {{\n  $prev = $env:{var}\n  {apply}\n  try {{ {cli} @args }} finally {{ if ($null -ne $prev) {{ $env:{var} = $prev }} else {{ Remove-Item Env:{var} -ErrorAction SilentlyContinue }} }}\n}}",
        name = account.alias_name
    )
}

pub fn render_sh(accounts: &[Account]) -> String {
    let mut out = format!("{HEADER}\n");
    for account in accounts {
        out.push_str(&sh_line(account));
        out.push('\n');
    }
    out
}

pub fn render_ps1(accounts: &[Account]) -> String {
    let mut out = format!("{HEADER}\n");
    for account in accounts {
        out.push_str(&ps1_function(account));
        out.push('\n');
    }
    out
}

/// The single alias for this OS, for the "copy alias" button.
pub fn alias_line(account: &Account) -> String {
    if cfg!(windows) { ps1_function(account) } else { sh_line(account) }
}

pub fn sh_source_block(aliases_sh: &Path) -> String {
    let p = sh_single_quote(&aliases_sh.to_string_lossy());
    format!("{BLOCK_START}\n[ -f {p} ] && . {p}\n{BLOCK_END}\n")
}

pub fn ps1_source_block(aliases_ps1: &Path) -> String {
    let p = ps_single_quote(&aliases_ps1.to_string_lossy());
    format!("{BLOCK_START}\nif (Test-Path {p}) {{ . {p} }}\n{BLOCK_END}\n")
}

pub fn install_block(content: &str, block: &str) -> String {
    let cleaned = remove_block(content);
    let mut out = cleaned.trim_end_matches('\n').to_string();
    if !out.is_empty() {
        out.push_str("\n\n");
    }
    out.push_str(block);
    out
}

pub fn remove_block(content: &str) -> String {
    let (Some(start), Some(end_at)) = (content.find(BLOCK_START), content.find(BLOCK_END)) else {
        return content.to_string();
    };
    if end_at < start {
        return content.to_string();
    }
    let mut end = end_at + BLOCK_END.len();
    if content[end..].starts_with('\n') {
        end += 1;
    }
    let before = content[..start].trim_end_matches('\n');
    let after = &content[end..];
    match (before.is_empty(), after.is_empty()) {
        (true, _) => after.to_string(),
        (false, true) => format!("{before}\n"),
        (false, false) => format!("{before}\n{after}"),
    }
}

pub fn write_alias_files(paths: &AppPaths, accounts: &[Account]) -> io::Result<()> {
    fs::create_dir_all(&paths.config_dir)?;
    fs::write(paths.aliases_sh(), render_sh(accounts))?;
    fs::write(paths.aliases_ps1(), render_ps1(accounts))
}

enum ProfileKind {
    Sh,
    Ps1,
}

fn profile_targets(paths: &AppPaths, shell: &ShellEnv) -> Vec<(PathBuf, ProfileKind)> {
    if cfg!(windows) {
        return powershell_profiles().into_iter().map(|p| (p, ProfileKind::Ps1)).collect();
    }
    let existing: Vec<(PathBuf, ProfileKind)> = [".zshrc", ".bashrc"]
        .iter()
        .map(|f| paths.home.join(f))
        .filter(|p| p.exists())
        .map(|p| (p, ProfileKind::Sh))
        .collect();
    if !existing.is_empty() {
        return existing;
    }
    let name = if shell.get("SHELL").is_some_and(|s| s.ends_with("zsh")) { ".zshrc" } else { ".bashrc" };
    vec![(paths.home.join(name), ProfileKind::Sh)]
}

#[cfg(windows)]
fn powershell_profiles() -> Vec<PathBuf> {
    use std::os::windows::process::CommandExt;
    ["pwsh", "powershell"]
        .iter()
        .filter_map(|exe| {
            std::process::Command::new(exe)
                .args(["-NoProfile", "-Command", "$PROFILE"])
                .creation_flags(0x0800_0000)
                .output()
                .ok()
        })
        .filter(|o| o.status.success())
        .map(|o| PathBuf::from(String::from_utf8_lossy(&o.stdout).trim()))
        .filter(|p| !p.as_os_str().is_empty())
        .collect()
}

#[cfg(not(windows))]
fn powershell_profiles() -> Vec<PathBuf> {
    Vec::new()
}

pub fn install(paths: &AppPaths, shell: &ShellEnv) -> io::Result<Vec<PathBuf>> {
    let mut done = Vec::new();
    for (target, kind) in profile_targets(paths, shell) {
        let block = match kind {
            ProfileKind::Sh => sh_source_block(&paths.aliases_sh()),
            ProfileKind::Ps1 => ps1_source_block(&paths.aliases_ps1()),
        };
        let current = fs::read_to_string(&target).unwrap_or_default();
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&target, install_block(&current, &block))?;
        done.push(target);
    }
    Ok(done)
}

pub fn uninstall(paths: &AppPaths, shell: &ShellEnv) -> io::Result<()> {
    for target in installed_targets(paths, shell) {
        let current = fs::read_to_string(&target)?;
        fs::write(&target, remove_block(&current))?;
    }
    Ok(())
}

pub fn installed_targets(paths: &AppPaths, shell: &ShellEnv) -> Vec<PathBuf> {
    profile_targets(paths, shell)
        .into_iter()
        .map(|(p, _)| p)
        .filter(|p| fs::read_to_string(p).is_ok_and(|c| c.contains(BLOCK_START)))
        .collect()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test aliases::`
Expected: 5 passed.

- [ ] **Step 5: Regenerate alias files on every account change, and add the commands**

In `src-tauri/src/commands.rs`, `accounts_changed`, add after `state.save_accounts();`:

```rust
    let accounts = state.accounts.lock().unwrap().clone();
    if let Err(e) = crate::aliases::write_alias_files(&state.paths, &accounts) {
        log::error!("writing alias files failed: {e}");
    }
```

In `src-tauri/src/lib.rs` setup, after `app.manage(state);`, add a first write so the files exist from the start:

```rust
            {
                let state = app.state::<state::AppState>();
                let accounts = state.accounts.lock().unwrap().clone();
                let _ = aliases::write_alias_files(&state.paths, &accounts);
            }
```

Append to `src-tauri/src/commands.rs`:

```rust
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

#[tauri::command]
pub fn aliases_status(state: State<'_, AppState>) -> AliasStatus {
    let targets = crate::aliases::installed_targets(&state.paths, &state.shell);
    AliasStatus { installed: !targets.is_empty(), targets: targets.iter().map(|p| p.display().to_string()).collect() }
}

#[tauri::command]
pub fn install_aliases(state: State<'_, AppState>) -> CmdResult<AliasStatus> {
    let targets = crate::aliases::install(&state.paths, &state.shell).map_err(|e| e.to_string())?;
    Ok(AliasStatus { installed: !targets.is_empty(), targets: targets.iter().map(|p| p.display().to_string()).collect() })
}

#[tauri::command]
pub fn uninstall_aliases(state: State<'_, AppState>) -> CmdResult<AliasStatus> {
    crate::aliases::uninstall(&state.paths, &state.shell).map_err(|e| e.to_string())?;
    Ok(AliasStatus { installed: false, targets: Vec::new() })
}
```

Register `commands::alias_line, commands::aliases_status, commands::install_aliases, commands::uninstall_aliases`.

- [ ] **Step 6: Frontend: copy alias**

Append to `src/types.ts`:

```ts
export interface AliasStatus {
  installed: boolean;
  targets: string[];
}
```

Add to `api` in `src/api.ts` (and import `AliasStatus`):

```ts
  aliasLine: (id: string) => invoke<string>("alias_line", { id }),
  aliasesStatus: () => invoke<AliasStatus>("aliases_status"),
  installAliases: () => invoke<AliasStatus>("install_aliases"),
  uninstallAliases: () => invoke<AliasStatus>("uninstall_aliases"),
```

In `AccountCard.tsx`, add an `onCopyAlias: () => void` prop and a button in `.card-actions`: `<button title={`Copy alias ${account.aliasName}`} onClick={onCopyAlias}>⌘</button>`. In `App.tsx`, pass:

```tsx
          onCopyAlias={() => run(api.aliasLine(account.id).then((line) => navigator.clipboard.writeText(line)))}
```

(Install/uninstall buttons are added to the settings view in Task 11.)

- [ ] **Step 7: Typecheck and compile**

Run: `pnpm exec tsc --noEmit && cd src-tauri && cargo build`
Expected: no errors.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat(app): generate per-account shell aliases"
```

---

### Task 10: Notifications

**Files:**
- Create: `src-tauri/src/notifier.rs`
- Modify: `src-tauri/src/scheduler.rs` (`start_fetch`), `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `model::*`.
- Produces: `notifier::{Notification { title, body }, diff(label: &str, account_id: &str, prev: Option<&UsageSnapshot>, next: &UsageSnapshot, thresholds: &[u8], sent: &mut HashMap<String, i64>, now: DateTime<Utc>) -> Vec<Notification>, window_name(&WindowKind) -> String}`

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/notifier.rs` with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AccountStatus, UsageSnapshot, Window, WindowKind};
    use chrono::{Duration, TimeZone, Utc};
    use std::collections::HashMap;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap()
    }

    fn snap(status: AccountStatus, pct: f32, resets_in_h: i64) -> UsageSnapshot {
        UsageSnapshot {
            plan: None,
            windows: vec![Window { kind: WindowKind::FiveHour, used_pct: pct, resets_at: Some(now() + Duration::hours(resets_in_h)) }],
            fetched_at: now(),
            status,
            last_error: None,
        }
    }

    const T: [u8; 2] = [80, 95];

    #[test]
    fn crossing_a_threshold_notifies_once() {
        let mut sent = HashMap::new();
        let first = diff("Work", "a", Some(&snap(AccountStatus::Ok, 70.0, 2)), &snap(AccountStatus::Ok, 82.0, 2), &T, &mut sent, now());
        assert_eq!(first.len(), 1);
        assert!(first[0].title.contains("82%"));
        let again = diff("Work", "a", Some(&snap(AccountStatus::Ok, 82.0, 2)), &snap(AccountStatus::Ok, 84.0, 2), &T, &mut sent, now());
        assert!(again.is_empty());
    }

    #[test]
    fn higher_threshold_notifies_and_lower_is_not_repeated() {
        let mut sent = HashMap::new();
        diff("Work", "a", None, &snap(AccountStatus::Ok, 82.0, 2), &T, &mut sent, now());
        let high = diff("Work", "a", None, &snap(AccountStatus::Ok, 96.0, 2), &T, &mut sent, now());
        assert_eq!(high.len(), 1);
        assert!(high[0].title.contains("96%"));
    }

    #[test]
    fn jumping_straight_to_the_top_sends_one_notification() {
        let mut sent = HashMap::new();
        let notes = diff("Work", "a", None, &snap(AccountStatus::Ok, 97.0, 2), &T, &mut sent, now());
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn a_new_window_can_notify_again() {
        let mut sent = HashMap::new();
        diff("Work", "a", None, &snap(AccountStatus::Ok, 85.0, 2), &T, &mut sent, now());
        let next_window = diff("Work", "a", None, &snap(AccountStatus::Ok, 85.0, 7), &T, &mut sent, now());
        assert_eq!(next_window.iter().filter(|n| n.title.contains("85%")).count(), 1);
    }

    #[test]
    fn reset_is_announced_when_previous_usage_was_high() {
        let mut sent = HashMap::new();
        let notes = diff("Work", "a", Some(&snap(AccountStatus::Ok, 90.0, 0)), &snap(AccountStatus::Ok, 3.0, 5), &T, &mut sent, now());
        assert!(notes.iter().any(|n| n.title.contains("has reset")));
        let low = diff("Work", "b", Some(&snap(AccountStatus::Ok, 20.0, 0)), &snap(AccountStatus::Ok, 3.0, 5), &T, &mut sent, now());
        assert!(low.is_empty());
    }

    #[test]
    fn disconnect_notifies_once_per_transition() {
        let mut sent = HashMap::new();
        let ok = snap(AccountStatus::Ok, 10.0, 2);
        let out = snap(AccountStatus::NeedsLogin, 10.0, 2);
        assert_eq!(diff("Work", "a", Some(&ok), &out, &T, &mut sent, now()).len(), 1);
        assert!(diff("Work", "a", Some(&out), &out, &T, &mut sent, now()).is_empty());
    }

    #[test]
    fn stale_data_does_not_trigger_threshold_notifications() {
        let mut sent = HashMap::new();
        assert!(diff("Work", "a", None, &snap(AccountStatus::Stale, 99.0, 2), &T, &mut sent, now()).is_empty());
    }
}
```

Add `pub mod notifier;` to `src-tauri/src/lib.rs`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test notifier::`
Expected: compile errors for the missing `diff` / `Notification`.

- [ ] **Step 3: Implement**

Add above the tests in `src-tauri/src/notifier.rs`:

```rust
use crate::model::{AccountStatus, UsageSnapshot, WindowKind};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

const RESET_MIN_PREVIOUS_PCT: f32 = 50.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Notification {
    pub title: String,
    pub body: String,
}

pub fn window_name(kind: &WindowKind) -> String {
    match kind {
        WindowKind::FiveHour => "5-hour window".to_string(),
        WindowKind::Weekly => "Weekly limit".to_string(),
        WindowKind::WeeklyScoped(name) => format!("Weekly {name} limit"),
        WindowKind::Other(name) => name.clone(),
    }
}

fn until(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let minutes = (at - now).num_minutes().max(0);
    if minutes < 60 {
        format!("{minutes}m")
    } else if minutes < 1440 {
        format!("{}h {}m", minutes / 60, minutes % 60)
    } else {
        format!("{}d {}h", minutes / 1440, (minutes % 1440) / 60)
    }
}

/// Decides which notifications a new snapshot triggers. `sent` holds dedupe keys
/// (persisted in the cache) so each threshold fires once per window instance.
pub fn diff(
    label: &str,
    account_id: &str,
    prev: Option<&UsageSnapshot>,
    next: &UsageSnapshot,
    thresholds: &[u8],
    sent: &mut HashMap<String, i64>,
    now: DateTime<Utc>,
) -> Vec<Notification> {
    let mut notes = Vec::new();

    if next.status == AccountStatus::NeedsLogin && prev.is_none_or(|p| p.status != AccountStatus::NeedsLogin) {
        notes.push(Notification {
            title: format!("{label} needs to sign in again"),
            body: "Open Usage Monitor and choose Reconnect.".to_string(),
        });
    }
    if next.status != AccountStatus::Ok {
        return notes;
    }

    for window in &next.windows {
        let reset_ts = window.resets_at.map_or(now.timestamp(), |r| r.timestamp());
        let key_base = format!("{account_id}|{}|{reset_ts}", window.kind.key());

        if let Some(&top) = thresholds.iter().filter(|t| window.used_pct >= f32::from(**t)).max() {
            let top_key = format!("th|{key_base}|{top}");
            if !sent.contains_key(&top_key) {
                let body = window
                    .resets_at
                    .map(|r| format!("Resets in {}.", until(r, now)))
                    .unwrap_or_default();
                notes.push(Notification {
                    title: format!("{label}: {} at {:.0}%", window_name(&window.kind), window.used_pct),
                    body,
                });
                for t in thresholds.iter().filter(|t| **t <= top) {
                    sent.insert(format!("th|{key_base}|{t}"), reset_ts);
                }
            }
        }

        let previous = prev.and_then(|p| p.window(&window.kind));
        if let (Some(before), Some(new_reset)) = (previous, window.resets_at) {
            if let Some(old_reset) = before.resets_at {
                let reset_key = format!("reset|{account_id}|{}|{}", window.kind.key(), old_reset.timestamp());
                if new_reset > old_reset
                    && before.used_pct >= RESET_MIN_PREVIOUS_PCT
                    && window.used_pct < before.used_pct
                    && !sent.contains_key(&reset_key)
                {
                    notes.push(Notification {
                        title: format!("{label}: {} has reset", window_name(&window.kind)),
                        body: format!("Now at {:.0}% used.", window.used_pct),
                    });
                    sent.insert(reset_key, old_reset.timestamp());
                }
            }
        }
    }
    notes
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test notifier::`
Expected: 7 passed.

- [ ] **Step 5: Send notifications after each fetch**

In `src-tauri/src/scheduler.rs`, `start_fetch`, replace the `let merged = { ... };` block with:

```rust
        let thresholds = state.settings.lock().unwrap().thresholds.clone();
        let (merged, notes) = {
            let mut cache = state.cache.lock().unwrap();
            let prev = cache.snapshots.get(&account.id).cloned();
            let merged = merge_result(prev.as_ref(), fresh.clone());
            let mut sent = std::mem::take(&mut cache.sent);
            let notes = crate::notifier::diff(&account.label, &account.id, prev.as_ref(), &merged, &thresholds, &mut sent, Utc::now());
            cache.sent = sent;
            cache.snapshots.insert(account.id.clone(), merged.clone());
            (merged, notes)
        };
        {
            use tauri_plugin_notification::NotificationExt;
            for note in notes {
                let _ = app.notification().builder().title(&note.title).body(&note.body).show();
            }
        }
```

- [ ] **Step 6: Verify**

Run: `cd src-tauri && cargo test -- notifier:: scheduler::`
Expected: 16 passed, and the crate compiles.

- [ ] **Step 7: Commit**

```bash
git add src-tauri
git commit -m "feat(app): notify on thresholds, resets and disconnects"
```

---

### Task 11: Settings screen and launch at login

**Files:**
- Create: `src/components/SettingsView.tsx`
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src/api.ts`, `src/types.ts`, `src/App.tsx`

**Interfaces:**
- Consumes: `Settings`, `CliContext::resolve`, `scheduler::schedule_all`, the alias commands (Task 9), `tauri_plugin_autostart::ManagerExt`.
- Produces: commands `get_settings() -> Settings`, `save_settings(settings) -> Settings`, `cli_status() -> { claude, codex }`.

- [ ] **Step 1: Add the commands**

Append to `src-tauri/src/commands.rs`:

```rust
use crate::store::settings::Settings;

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
    use tauri_plugin_autostart::ManagerExt;
    let next = settings.normalized();
    let prev = state.settings.lock().unwrap().clone();
    if next.launch_at_login != prev.launch_at_login {
        let autolaunch = app.autolaunch();
        let result = if next.launch_at_login { autolaunch.enable() } else { autolaunch.disable() };
        result.map_err(|e| format!("Could not change launch at login: {e}"))?;
    }
    *state.settings.lock().unwrap() = next.clone();
    state.save_settings();
    if next.claude_path != prev.claude_path || next.codex_path != prev.codex_path {
        let resolved = crate::providers::CliContext::resolve(&next, &state.shell, &state.paths.home, state.paths.work_dir());
        *state.cli.lock().unwrap() = resolved;
    }
    if next.interval_minutes != prev.interval_minutes
        || next.claude_path != prev.claude_path
        || next.codex_path != prev.codex_path
    {
        scheduler::schedule_all(&app);
    }
    Ok(next)
}
```

Register `commands::get_settings, commands::save_settings, commands::cli_status`.

- [ ] **Step 2: Frontend types and API**

Append to `src/types.ts`:

```ts
export interface Settings {
  intervalMinutes: number;
  thresholds: number[];
  claudePath: string | null;
  codexPath: string | null;
  launchAtLogin: boolean;
}

export interface CliStatus {
  claude: string | null;
  codex: string | null;
}
```

Add to `api` in `src/api.ts` (and import the types):

```ts
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  cliStatus: () => invoke<CliStatus>("cli_status"),
```

- [ ] **Step 3: Write the settings view**

Create `src/components/SettingsView.tsx`:

```tsx
import { useEffect, useState } from "react";
import { api } from "../api";
import type { AliasStatus, CliStatus, Settings } from "../types";

interface Props {
  onDone: () => void;
}

export function SettingsView({ onDone }: Props) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [thresholdText, setThresholdText] = useState("");
  const [cli, setCli] = useState<CliStatus | null>(null);
  const [aliases, setAliases] = useState<AliasStatus | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    api.getSettings().then((s) => {
      setSettings(s);
      setThresholdText(s.thresholds.join(", "));
    });
    api.cliStatus().then(setCli);
    api.aliasesStatus().then(setAliases);
  }, []);

  if (!settings) return null;

  async function save() {
    const thresholds = thresholdText
      .split(",")
      .map((t) => Number(t.trim()))
      .filter((n) => Number.isFinite(n));
    try {
      const saved = await api.saveSettings({ ...settings!, thresholds });
      setSettings(saved);
      setThresholdText(saved.thresholds.join(", "));
      setCli(await api.cliStatus());
      setMessage("Saved.");
    } catch (e) {
      setMessage(String(e));
    }
  }

  async function toggleAliases() {
    try {
      setAliases(aliases?.installed ? await api.uninstallAliases() : await api.installAliases());
    } catch (e) {
      setMessage(String(e));
    }
  }

  const update = (patch: Partial<Settings>) => setSettings({ ...settings, ...patch });

  return (
    <div className="view">
      <header className="view-header">
        <button onClick={onDone}>← Back</button>
        <h2>Settings</h2>
      </header>
      <div className="form">
        <label>
          Refresh every (minutes, 2–30)
          <input
            type="number"
            min={2}
            max={30}
            value={settings.intervalMinutes}
            onChange={(e) => update({ intervalMinutes: Number(e.target.value) })}
          />
        </label>
        <label>
          Notify at (% used, comma-separated)
          <input value={thresholdText} onChange={(e) => setThresholdText(e.target.value)} />
        </label>
        <label>
          Claude CLI path (leave empty to detect)
          <input
            placeholder={cli?.claude ?? "Not found"}
            value={settings.claudePath ?? ""}
            onChange={(e) => update({ claudePath: e.target.value || null })}
          />
        </label>
        <label>
          Codex CLI path (leave empty to detect)
          <input
            placeholder={cli?.codex ?? "Not found"}
            value={settings.codexPath ?? ""}
            onChange={(e) => update({ codexPath: e.target.value || null })}
          />
        </label>
        <label className="checkbox">
          <input
            type="checkbox"
            checked={settings.launchAtLogin}
            onChange={(e) => update({ launchAtLogin: e.target.checked })}
          />
          Launch at login
        </label>
        <button onClick={save}>Save</button>
      </div>
      <h3>Shell aliases</h3>
      <p className="muted">
        {aliases?.installed
          ? `Installed in ${aliases.targets.join(", ")}. Open a new terminal to use them.`
          : "Adds one line to your shell profile that loads an alias per account."}
      </p>
      <button onClick={toggleAliases}>{aliases?.installed ? "Uninstall aliases" : "Install aliases"}</button>
      {message && <p className="muted">{message}</p>}
    </div>
  );
}
```

- [ ] **Step 4: Route to settings**

In `src/App.tsx`:
1. Add `{ name: "settings" }` to the `View` union.
2. Import `SettingsView`, and add `if (view.name === "settings") return <SettingsView onDone={toPanel} />;` next to the other view branches.
3. In the panel header, wrap the buttons in a `<div className="card-actions">`: `<button title="Settings" onClick={() => setView({ name: "settings" })}>⚙</button>` plus the existing "+ Add account" button.
4. Link `CliMissing` cards to settings (spec §5). Add an `onOpenSettings: () => void` prop to `AccountCard`, render `{snapshot?.status.type === "cliMissing" && <button onClick={onOpenSettings}>Set CLI path</button>}` right after the status paragraph, and pass `onOpenSettings={() => setView({ name: "settings" })}` from `App.tsx`.

- [ ] **Step 5: Typecheck and compile**

Run: `pnpm exec tsc --noEmit && cd src-tauri && cargo build`
Expected: no errors.

- [ ] **Step 6: Manual check of the whole v1**

Run: `pnpm tauri dev` and check:
- Saving the interval as 1 stores 2.
- A bogus Claude path makes the Claude cards show "CLI not found"; clearing the path recovers.
- "Install aliases" adds a marked block to `~/.zshrc`, and in a new terminal `claude-main --version` works. "Uninstall" removes the block.
- Toggling "Launch at login" does not error.
- A notification appears by temporarily setting the thresholds to `1` and pressing refresh on a card.

Restore the settings and stop the dev server.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(app): settings screen with cli paths, thresholds and autostart"
```

---

## After the last task

- Run the complete Rust test suite once: `cd src-tauri && cargo test`.
- Cross-platform release builds (`tauri-action` matrix) are out of v1 scope. They are added only on request.
- **Manual checks on Windows and Linux (the user runs them there):**
  - The tray tooltip shows the numbers.
  - The panel opens (on Linux, from the menu).
  - Imported accounts fetch successfully.
  - The PowerShell alias works on Windows.
