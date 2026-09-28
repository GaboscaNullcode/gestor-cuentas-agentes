# Usage Monitor

A tray app for macOS, Windows and Linux that shows how much of each Claude and Codex subscription account you have used. For every account it shows:

- the percentage used of the **5-hour** window and of the **weekly** window (plus per-model weekly limits when the provider reports them);
- **when each window resets**;
- the plan (Max, Pro, Plus…) and whether the account needs you to sign in again.

You can keep several accounts signed in at the same time, see all of them in one panel, and switch between them in your terminal with one alias per account.

```
Menu bar:  ◐ 34% · 71%

┌───────────────────────────────────────┐
│ Claude · Work (Max)          ★ ↻ ⌘ ✕  │
│ 5h    ███░░░░░░░  34%   in 2h 10m     │
│ Week  ███████░░░  71%   Mon 09:00     │
│ Codex · Personal (Pro)       ☆ ↻ ⌘ ✕  │
│ 5h    No 5h limit                     │
│ Week  ██░░░░░░░░  18%   Thu 10:00     │
│ Updated 2 min ago                     │
└───────────────────────────────────────┘
```

---

## How it works

The app **never reads, stores or refreshes OAuth tokens**. Instead:

1. **One config directory per account.** Claude Code reads its session from `CLAUDE_CONFIG_DIR` and Codex from `CODEX_HOME`. Each account you add gets its own directory, for example `~/.claude-work` or `~/.codex-personal`, so their sessions never interfere.
2. **The official CLIs do all the authenticated work.** For each account, the app runs the real `claude` / `codex` binary with that account's directory set in the environment:

   | Provider | Usage | Sign-in status | Sign in / out |
   |---|---|---|---|
   | Claude | `claude -p /usage --no-session-persistence --output-format stream-json --verbose` | `claude auth status` | `claude auth login` / `claude auth logout` |
   | Codex | `codex app-server` (JSON-RPC: `initialize` → `account/rateLimits/read`) | `codex login status` | `codex login` / `codex logout` |

3. **The CLIs refresh their own tokens.** Refresh tokens rotate, and Codex refresh tokens are single use. If the app refreshed them itself, it would break the CLI's session. Leaving it to the CLIs keeps every account usable from your terminal.
4. **Parsers are tolerant.** None of these outputs are public, stable APIs. Unknown fields are ignored, missing fields become "unknown", and windows are identified by their **duration or kind**, never by their position. For example, a Codex Pro account currently has no 5-hour window, and its "primary" window is the weekly one.

```
┌──────────── Usage Monitor (Tauri) ────────────┐
│ React panel ◄── events ── Rust core           │
│                           ├─ Scheduler (polls │
│                           │   every N min)    │
│                           ├─ ClaudeProvider ──┼──► claude  (CLAUDE_CONFIG_DIR=…)
│                           ├─ CodexProvider  ──┼──► codex   (CODEX_HOME=…)
│                           ├─ Notifier, Tray   │
│                           └─ Account store    │
└───────────────────────────────────────────────┘
```

---

## Requirements

- **Claude Code** (`claude`) and/or **Codex CLI** (`codex`) installed. The app finds them on your login shell's `PATH` or in the usual install folders (`~/.local/bin`, `/opt/homebrew/bin`, npm global folders…). You can also set their paths in Settings.
- To build from source:
  - [Rust](https://rustup.rs) stable (1.82 or newer)
  - Node.js 22+ and [pnpm](https://pnpm.io)
  - Linux only: `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev`
  - Windows only: the WebView2 runtime (preinstalled on Windows 10/11)

---

## Running it

```bash
pnpm install
pnpm tauri dev      # development build with hot reload
pnpm tauri build    # release bundle (.dmg / .msi / .AppImage / .deb) in src-tauri/target/release/bundle
```

The app has no main window: it lives in the menu bar (macOS) or system tray (Windows/Linux). Click the icon to open the panel. On Linux, where most trays don't report clicks, use the **Open panel** menu item.

---

## Adding accounts

Open the panel and choose **+ Add account**. There are three ways to add one.

### Import a session that already exists on this computer

The app finds your existing `~/.claude` and `~/.codex` sessions and lists them under **Already signed in on this computer**. Give each a label (for example "Main") and choose **Import**. You don't have to sign in again.

If your shell exports `CLAUDE_CONFIG_DIR` or `CODEX_HOME` (in `.zshrc`, for example), the app reads those values from your login shell and imports the account with them. This matters on macOS: Claude stores the session in the Keychain under a name derived from `CLAUDE_CONFIG_DIR`, so the app must use exactly the same setting you do.

### Sign in to a new account

1. Choose the provider and type a label, for example "Personal". The app proposes a directory such as `~/.claude-personal`, which you can edit.
2. Choose **Sign in**. The app creates the directory and runs the official login (`claude auth login` or `codex login`) inside it.
3. Your browser opens. Sign in **with the account you want to add**. If the browser doesn't open, the panel shows a link.
4. Claude sometimes shows a code in the browser instead of finishing on its own. Paste it into the field that appears in the panel.
5. When the CLI confirms the session, the account appears with its usage.

If you cancel or the sign-in fails, the account is removed. The directory is removed too, but only if the app created it and it is still empty.

Only one sign-in can run at a time, because `codex login` listens on a fixed local port (1455).

### Add a directory that is already signed in

Check **This directory is already signed in**, enter its path and choose **Add**. The app does not start a sign-in.

### Reconnecting and removing

- **Reconnect** appears on a card whose session expired or was revoked, or whose data is stale or failing. It runs the same sign-in on the same directory.
- **✕ Remove** takes the account out of the app. Two optional boxes let you also **sign out of the CLI** and **delete the directory**. Nothing is deleted unless you tick the box.

The app only offers to delete a directory **it created itself**. It never deletes `~/.claude`, `~/.codex`, your home folder, anything outside your home folder, or imported directories.

---

## Switching accounts in the terminal

Every account gets an alias named after its provider and label, for example `claude-work` or `codex-personal`:

```bash
claude-work          # Claude Code signed in as the "Work" account
codex-personal       # Codex signed in as the "Personal" account
```

You can run different accounts in different terminals at the same time.

- **⌘ (Copy alias)** on a card copies that account's alias definition.
- **Settings → Install aliases** adds **one marked block** to your shell profile. That block loads a file the app maintains:

  ```bash
  # >>> usage-monitor aliases >>>
  [ -f '<app config dir>/aliases.sh' ] && . '<app config dir>/aliases.sh'
  # <<< usage-monitor aliases <<<
  ```

  - macOS/Linux: the block goes into `~/.zshrc` and/or `~/.bashrc`.
  - Windows: it goes into your PowerShell `$PROFILE`, which loads `aliases.ps1`. The aliases are PowerShell functions that set the variable and restore it afterwards.
  - When you add or remove accounts, only the app's own `aliases.sh` / `aliases.ps1` are rewritten. Your profile is never touched again.
  - **Uninstall aliases** removes the block.
  - If a profile can't be read (for example a UTF-16 `$PROFILE`), the app refuses to write to it instead of overwriting it.

Open a new terminal after installing.

---

## The tray

| | macOS | Windows | Linux |
|---|---|---|---|
| Text next to the icon | pinned account: `34% · 71%` (5h · week), or `— · 18%` without a 5h window | — | pinned account (needs a tray that supports labels) |
| Icon color | worst state across all accounts | same | same |
| Tooltip | every account | every account | not supported by most trays |
| Open panel | click the icon | click the icon | **Open panel** menu item |

Icon colors: **green** below 50%, **yellow** from 50%, **red** from 80%, **gray** when an account needs attention (signed out, CLI missing, error).

Use **☆ / ★** on a card to choose which account is pinned to the menu bar text.

The tray menu also has **Refresh all** and **Quit**. Quitting cancels any sign-in in progress.

---

## Refreshing and account states

- Each account is refreshed every **5 minutes** by default (configurable from 2 to 30). Accounts are spread across the interval so the CLIs don't all start at once.
- **↻** on a card refreshes it immediately. Manual refreshes are limited to one per minute per account, including **Refresh all**.
- When a window's reset time passes, the next refresh is moved to shortly after it. The bar shows `resetting…` until then.
- A Claude refresh takes a few seconds, because it starts the CLI and your Claude Code session hooks run. A Codex refresh takes under a second.

| Card state | Meaning | What the app does |
|---|---|---|
| (normal) | Last refresh succeeded | Refreshes on schedule |
| Stale | Last refresh failed; older data is shown grayed out with its age | Retries with backoff (doubling, up to 60 min) |
| Signed out | The CLI reports no valid session | Stops refreshing it and shows **Reconnect** |
| Rate limited | The usage endpoint asked to slow down | Backs off quietly |
| CLI not found | `claude` / `codex` could not be found | Shows **Set CLI path** |
| Error: … | Anything else, with the message | Retries with backoff and shows **Reconnect** |

---

## Notifications

- **Threshold reached:** a window crosses 80% or 95% (configurable). Each threshold notifies once per window: after the window resets it can notify again.
- **Window reset:** a window that was at 50% or more has reset.
- **Account needs to sign in again:** sent once each time an account becomes signed out.

---

## Settings

| Setting | Default |
|---|---|
| Refresh every (minutes) | 5 (2–30) |
| Notify at (% used) | 80, 95 |
| Claude CLI path | empty = detect automatically |
| Codex CLI path | empty = detect automatically |
| Launch at login | off |
| Shell aliases | Install / Uninstall |

If you enable **Launch at login** while running `pnpm tauri dev`, the development build is registered. Turn it off afterwards and enable it from a release build.

---

## Where data is stored

The app keeps only account metadata and the latest usage figures. It never stores credentials.

| File | Contents |
|---|---|
| `accounts.json` | label, provider, config directory, pinned flag, alias name |
| `settings.json` | your settings |
| `usage-cache.json` | last snapshot per account and which notifications were already sent |
| `aliases.sh`, `aliases.ps1` | generated aliases |
| `usage-monitor.log` | rotating log; secret-looking values are redacted |

Locations (the app identifier is `com.gabosca.usagemonitor`):

| | Data and config | Logs |
|---|---|---|
| macOS | `~/Library/Application Support/com.gabosca.usagemonitor/` | `~/Library/Logs/com.gabosca.usagemonitor/` |
| Windows | `%APPDATA%\com.gabosca.usagemonitor\` | `%LOCALAPPDATA%\com.gabosca.usagemonitor\logs\` |
| Linux | `~/.local/share/com.gabosca.usagemonitor/`, `~/.config/com.gabosca.usagemonitor/` | `~/.local/share/com.gabosca.usagemonitor/logs/` |

The CLIs run with a neutral working directory (`<data dir>/work`), so no project-specific hooks or settings apply.

If a file is ever corrupted, it is moved aside as `*.json.corrupt` instead of being silently replaced.

---

## Troubleshooting

- **"CLI not found":** apps opened from Finder on macOS don't inherit your shell's `PATH`. The app reads it from your login shell, but if detection still fails, set the path in **Settings** (run `which claude` / `which codex` in a terminal to find it).
- **An imported Claude account shows "Signed out" although `claude` works in your terminal:** check whether you export `CLAUDE_CONFIG_DIR` in a file your login shell doesn't read. Remove the account and import it again, or add it with **This directory is already signed in**.
- **The Codex sign-in fails right away:** another `codex login` may still be holding port 1455. Close it and try again.
- **Numbers look wrong or a window is missing:** providers change these outputs without notice. Check the log (see above); each refresh logs the relevant, redacted output.

---

## Known limitations

- **Unofficial data sources.** The CLI outputs used here (`/usage` in stream-json, `account/rateLimits/read`) are not documented APIs and may change. When they do, the parsers degrade to "unknown" instead of crashing, but figures can be missing until the app is updated.
- **Anthropic's terms** forbid third-party tools from handling claude.ai credentials. This app avoids that by only running the unmodified official CLI, but polling usage is not explicitly covered by any policy. Don't set very short intervals.
- **Codex Pro** plans currently have no 5-hour window; the card shows "No 5h limit".
- **Codex installed inside WSL** on Windows is not supported.
- The Windows-specific code (PowerShell aliases, process cleanup) has not yet been exercised on a Windows machine.
- No usage history or charts, no API-key accounts, no sync between computers, no signed installers or auto-update.

---

## Development

```
src/                     React + TypeScript panel (Vite)
  App.tsx                views: panel, add account, sign-in, settings
  api.ts, types.ts       typed wrappers for the Rust commands and events
  components/            AccountCard, WindowBar, AddAccountView, LoginView, SettingsView
src-tauri/src/           Rust core
  model.rs               Account, UsageSnapshot, Window, WindowKind, AccountStatus
  parse/                 tolerant parsers for Claude and Codex output (+ Codex JSON-RPC client)
  cli/                   binary lookup (login-shell PATH) and process runner / tree kill
  providers/             per-account CLI invocations, error classification
  store/                 JSON persistence, accounts, settings, cache, detection
  scheduler.rs           polling, backoff, stale handling, manual-refresh gate
  login.rs               sign-in sessions (browser URL, code paste, cancel)
  notifier.rs            notification rules and dedupe
  tray.rs                tray icon, title, tooltip, panel toggle
  aliases.rs             alias generation and shell profile block
  commands.rs            Tauri commands used by the UI
src-tauri/tests/fixtures real CLI outputs used by the parser tests
docs/superpowers/        design spec and implementation plan
```

Checks:

```bash
cd src-tauri && cargo test     # Rust unit tests (parsers, scheduler, notifier, store, aliases…)
pnpm exec tsc --noEmit         # TypeScript typecheck
```

The design decisions, research findings and the reasoning behind them are in [`docs/superpowers/specs/2026-09-25-usage-monitor-design.md`](docs/superpowers/specs/2026-09-25-usage-monitor-design.md). The step-by-step implementation plan is in [`docs/superpowers/plans/2026-09-25-usage-monitor.md`](docs/superpowers/plans/2026-09-25-usage-monitor.md).
