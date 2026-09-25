# Usage Monitor — Design Spec

Date: 2026-09-25
Status: Approved design, pending spec review

## 1. Goal

A personal desktop app that shows, for several Claude and Codex subscription accounts, how much of each rate-limit window is used (5-hour and weekly, plus per-model weekly limits when they exist) and when each window resets. The user logs in to each account once from the app, and the app keeps the data current in the background. It must run on macOS, Windows and Linux.

### Success criteria (v1)

- With two or more accounts registered (at least one Claude and one Codex), the tray panel shows each account's plan, the % used of each window, and its reset time, and the data refreshes on its own.
- A new account can be added from the app. The only manual step is authorizing in the browser.
- The pinned account's usage is visible from the tray without opening the panel (text on macOS; tooltip plus a colored icon on Windows and Linux).
- The app sends notifications for threshold crossings, window resets and disconnected accounts.
- Shell aliases let the user start `claude`/`codex` with a given account in any terminal.
- The app builds and runs on macOS, Windows and Linux.

### Non-goals (v1)

- Usage history or charts.
- API-key accounts (pay-as-you-go API usage).
- Syncing accounts across machines.
- Signed installers and auto-update.
- End-to-end UI tests.

## 2. Key constraints and research findings

The research was done on 2026-09-25. The full notes are in the conversation that produced this spec.

- **None of these data sources is a public API.** They are internal endpoints and formats that already changed in 2026. For example, Claude moved from `seven_day_opus` to `limits[]`, and Codex removed the 5h window for Pro plans. Parsers must be tolerant.
- **The app never handles OAuth tokens.** Login, token storage and token refresh are always done by the official CLIs.
  - Refresh tokens rotate. Codex refresh tokens are single use, so refreshing from the app breaks the CLI session (`refresh_token_reused`).
  - Anthropic's terms forbid third parties from collecting or intermediating claude.ai credentials.
- **One config directory per account.**
  - Claude uses `CLAUDE_CONFIG_DIR`. On macOS the Keychain entry is `Claude Code-credentials-<sha256(dir)[:8]>`; on Linux and Windows it is `<dir>/.credentials.json`.
  - Codex uses `CODEX_HOME`, with `<dir>/auth.json`.
- **Claude source:** `claude -p "/usage" --no-session-persistence --output-format stream-json --verbose`, run with `CLAUDE_CONFIG_DIR` set.
  - Verified live on 2026-09-25 with Claude Code 2.1.282. It cost 0 tokens.
  - The output contains `usage_report.rate_limits.limits[]`, with entries shaped like `{kind: "session"|"weekly_all"|"weekly_scoped", percent, resets_at (ISO 8601), severity, is_active, scope.model.display_name}`, plus `extra_usage`.
- **Codex source (primary, not yet validated live):** `codex app-server` over stdio, run with `CODEX_HOME` set.
  - Sequence: `initialize`, then `account/rateLimits/read`.
  - The response carries `rateLimits` and `rateLimitsByLimitId`, each with `primary`/`secondary` entries shaped like `{usedPercent, windowDurationMins, resetsAt}`, plus `planType` and `credits`.
- **Codex source (fallback):** the latest `<CODEX_HOME>/sessions/**/rollout-*.jsonl` event of type `token_count`, read from `payload.rate_limits`.
  - Entries have the shape `{primary|secondary: {used_percent, window_minutes, resets_at (epoch s)}, plan_type}`.
  - This data is only updated when the account is used on this machine.
- **Classify windows by duration or kind, never by the primary/secondary position.** Codex Pro currently has no 5h window, and its `primary` is the weekly window (10080 min).
- **The usage endpoints are themselves rate-limited.** Polling faster than every few minutes risks HTTP 429.

## 3. Architecture

The app is built on Tauri 2. Rust holds all of the logic, and a React + TypeScript + Vite frontend renders the UI. All logic stays in Rust so that polling keeps running while the webview is hidden, and so that parsing is made of pure functions that are easy to test.

```
Frontend (React)  <-- "usage-updated" events ---+
  panel, add-account modal, settings            |
      | invoke(commands)                        |
Rust core --------------------------------------+
  AccountStore -> Scheduler -> Provider (trait)
                      |         |- ClaudeProvider
                      |         +- CodexProvider
                      v
                 UsageState -> Notifier, TrayController
  CliLocator (finds claude/codex binaries per OS)
```

### 3.1 Units

- **`model`:** shared types, with no I/O.
  - `Provider { Claude, Codex }`
  - `Account { id, provider, label, config_dir, pinned, alias_name, created_at }`
  - `UsageSnapshot { plan: Option<String>, windows: Vec<Window>, fetched_at, status: AccountStatus }`
  - `Window { kind: WindowKind, used_pct: f32, resets_at: Option<DateTime<Utc>> }`
  - `WindowKind { FiveHour, Weekly, WeeklyScoped(String), Other(String) }`
  - `AccountStatus { Ok, Stale, NeedsLogin, RateLimited, CliMissing, Error(String) }`
- **`AccountStore`:** loads and saves `accounts.json` in the OS app-data directory. It never stores secrets.
- **`CliLocator`:** finds the `claude` and `codex` binaries. It tries these locations in order:
  1. A user-configured path from the settings.
  2. The `PATH` of the login shell, on macOS and Linux. GUI apps on macOS do not inherit the shell's `PATH`, so this has to be read explicitly (`$SHELL -lc 'command -v claude'`).
  3. The current process `PATH`.
  4. Well-known locations per OS: `~/.local/bin`, `/opt/homebrew/bin`, `/usr/local/bin`, `%APPDATA%\npm\*.cmd`, `%USERPROFILE%\.local\bin`.
  
  On Windows, `.cmd` shims are run through `cmd /C`.
- **`Provider` trait:**
  - `fetch_usage(&Account) -> Result<UsageSnapshot, FetchError>`
  - `login(&Account) -> Result<(), LoginError>`, which is cancellable
  - `login_status(&Account) -> Result<bool, _>`
  - `logout(&Account)`
  - `alias_lines(&Account) -> AliasSet`
- **`ClaudeProvider`:** runs the Claude command from section 2 and passes the JSON lines to `parse_claude_usage(&str) -> UsageSnapshot`. That parser is a pure function.
  - Mapping: `session` → `FiveHour`, `weekly_all` → `Weekly`, `weekly_scoped` → `WeeklyScoped(display_name)`, and any other kind → `Other(kind)`.
  - It uses the parsed text of `resets_at`.
- **`CodexProvider`:** runs `codex app-server` with `CODEX_HOME` set, performs the JSON-RPC handshake, calls `account/rateLimits/read`, and then terminates the process. The response goes to `parse_codex_rate_limits(&Value) -> UsageSnapshot`, a pure function.
  - Mapping by duration: 300 min → `FiveHour`, 10080 min → `Weekly`, anything else → `Other("<n>m")`.
  - Extra limit IDs (for example `codex_bengalfox`) are mapped by the same rules, with the model or limit name kept in `WeeklyScoped`/`Other`.
  - If the spike rejects app-server, the fallback is `parse_codex_rollout(&str)` over the newest rollout file.
- **`Scheduler`:** a tokio task that runs one loop per account.
  - The default interval is 5 min, configurable from 2 to 30 min. Accounts are staggered by `interval / N`.
  - At most one fetch runs per account at a time, with a 45 s timeout.
  - Manual refresh is allowed at most once every 60 s per account.
  - On error, the interval doubles up to a 60 min cap and resets on the first success.
  - Accounts in `NeedsLogin` are not polled until the user reconnects them.
  - When a `resets_at` passes, the next fetch is brought forward.
- **`UsageState`:** holds the latest snapshot per account, persists it to `usage-cache.json` so the UI can render immediately at startup, and emits `usage-updated` to the frontend.
- **`Notifier`:** a pure function `diff(prev, next, settings, sent) -> Vec<Notification>` plus a sender based on `tauri-plugin-notification`.
  - **Threshold crossed:** thresholds default to 80 % and 95 %. The deduplication key is `(account, window kind, resets_at, threshold)`.
  - **Window reset:** the previous `used_pct` was at least 50 % and the new window has a later `resets_at` with a lower value.
  - **Account disconnected:** the status changes to `NeedsLogin`. This is sent once per transition.
- **`TrayController`:**
  - macOS: the tray title shows the pinned account, for example `34% · 71%` (5h · weekly), or `— · 18%` when there is no 5h window.
  - Windows and Linux: the tooltip carries the same text, and the icon is colored by the worst status across all accounts (green below 50 %, yellow from 50 %, red from 80 %, gray on error).
  - Clicking the icon toggles the panel window.
- **`AliasWriter`:**
  - Writes `aliases.sh` (zsh/bash) and `aliases.ps1` (PowerShell) to the app config directory, regenerated whenever the accounts change.
  - "Install in my shell" appends one marked `source` line to `~/.zshrc`, `~/.bashrc` or `$PROFILE`, and "Uninstall" removes it.
  - Example alias for zsh/bash:
    ```
    alias claude-personal='CLAUDE_CONFIG_DIR="$HOME/.claude-personal" claude'
    ```
  - Example function for PowerShell:
    ```
    function claude-personal { $env:CLAUDE_CONFIG_DIR="$HOME\.claude-personal"; try { claude @args } finally { Remove-Item Env:CLAUDE_CONFIG_DIR } }
    ```

## 4. Flows

### 4.1 First launch

1. The app detects `~/.claude` and `~/.codex` (or the `CLAUDE_CONFIG_DIR`/`CODEX_HOME` values already set) and offers to import them as "main" accounts without a new login.
2. The user can also choose "Add existing" and point to any directory that is already logged in.

### 4.2 Add account

1. The user picks a provider and a label. The app proposes a `config_dir` (`~/.claude-<slug>` or `~/.codex-<slug>`), and the user can edit it.
2. The app creates the directory and runs the login command with the directory set in the environment: `claude auth login` with `CLAUDE_CONFIG_DIR`, or `codex login` with `CODEX_HOME`. The CLI opens the browser, and the panel shows "Waiting for authorization…" with a Cancel button.
3. When the process exits successfully, the app checks `claude auth status` / `codex login status`, runs the first fetch and shows the detected plan.
4. If the login fails or is cancelled, the app removes the directory, but only if the app created it and it is still empty.

Only one login runs at a time, because `codex login` binds a fixed local port. If `claude auth login` turns out to require a TTY (to be verified in the spike), the fallback is to run it under a PTY with the `portable-pty` crate.

### 4.3 Reconnect

This is the same as step 2 of adding an account, run on the existing directory. On success, the account status becomes `Ok` and polling resumes.

### 4.4 Remove account

Removing an account deletes it from `accounts.json` and regenerates the aliases. Running `logout` and deleting the directory are separate options, each with explicit confirmation. Nothing is deleted by default.

## 5. UI

All UI text is in English.

- **Tray panel:** a small frameless window anchored to the tray. It shows one card per account with the provider icon, label, plan, status badge and "updated 2 min ago".
  - Each window has a bar with its % and reset time. The reset time is relative when it is less than 24 h away ("in 2h 10m") and shown as a weekday plus time otherwise ("Mon 09:00").
  - Bar colors change at 50 % and 80 %. A missing 5h window is shown as "No 5h limit".
  - Card actions: pin, refresh, copy alias, reconnect (when `NeedsLogin`), remove.
- **Add account modal:** the flow from section 4.2.
- **Settings:** polling interval, notification thresholds, CLI paths, install/uninstall aliases, launch at login (`tauri-plugin-autostart`).
- **States:** `Stale` shows grayed bars with the age of the data. `CliMissing` links to the CLI path setting. `Error` shows a short message, with the details in the log.

## 6. Persistence and logging

- App-data directory (from `tauri::path`): `accounts.json`, `settings.json`, `usage-cache.json` (snapshots plus the notification deduplication keys already sent).
- App-config directory: `aliases.sh` and `aliases.ps1`.
- App-log directory: a rotating log with a trimmed copy of each fetch output. Before anything is written, values under keys that look like secrets (`token`, `access`, `refresh`, `authorization`, `cookie`) are redacted.

## 7. Error handling

| Status | Detected when | Behavior |
|---|---|---|
| `Ok` | The fetch succeeds | Normal interval |
| `Stale` | The fetch fails but a previous snapshot exists | Show the old data with its age, apply backoff |
| `NeedsLogin` | The CLI reports that the user is not logged in or the session expired | Stop polling, show "Reconnect", notify once |
| `RateLimited` | The output mentions 429 or a rate limit | Backoff, no notification |
| `CliMissing` | `CliLocator` finds no binary | Show a settings link, retry when the settings change |
| `Error` | Anything else | Backoff, short message |

Parsers ignore unknown fields and treat missing fields as `None`. A limit with an unknown kind is shown under its raw name instead of failing the fetch.

## 8. Testing

- **Parsers:** Rust unit tests built on real JSON fixtures captured during the spike (`tests/fixtures/`), plus hand-made variants: the legacy Claude `seven_day_opus` shape, Codex with no 5h window, missing fields, unknown kinds.
- **Notifier:** unit tests for crossing a threshold, not notifying twice, window resets, disconnection.
- **Scheduler:** backoff and stagger tests using a fake clock.
- **UI:** no automated tests. Cross-platform verification is manual: the app launches on each OS and shows real accounts. A CI build matrix with `tauri-action` will be added only when requested.

## 9. Milestones

Each milestone is committed separately.

0. **Spike (throwaway code).** Confirm live:
   - `codex app-server` with `initialize` and `account/rateLimits/read`: the handshake parameters and the response.
   - Whether `claude auth login` works without a TTY.
   - The real duration of `claude -p "/usage"`.
   
   Capture the fixtures.
1. **Rust core.** Model, parsers with tests, `CliLocator`, `AccountStore`, and a debug command that fetches the existing accounts.
2. **Tray and panel.** Scheduler, `UsageState`, tray behavior per OS, and the panel UI with its states.
3. **Account management.** Import existing accounts, add, reconnect, remove, aliases.
4. **Notifications and settings.** `Notifier`, the settings screen, autostart.

## 10. Open risks

- The internal endpoints and formats may change without notice. Tolerant parsing plus fixtures keep the fix small when they do.
- `codex app-server` may not be usable from outside the CLI. In that case the app falls back to rollout files, whose data is only fresh when the account is used on this machine.
- Codex on native Windows vs WSL: if a Codex account lives inside WSL, its `CODEX_HOME` is on the Linux filesystem and it has to be invoked through `wsl.exe`. This is not supported in v1 unless the spike shows it is trivial.
- Anthropic's terms: using the official binary unmodified is the most defensible option, but polling for usage is not explicitly covered by any policy.
