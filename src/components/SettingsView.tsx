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
    // The backend expects integers (u8/u32); drop anything it could not deserialize.
    const thresholds = thresholdText
      .split(",")
      .map((t) => Number(t.trim()))
      .filter((n) => Number.isInteger(n) && n >= 0 && n <= 255);
    const intervalMinutes = Number.isFinite(settings!.intervalMinutes) ? Math.round(settings!.intervalMinutes) : 0;
    try {
      const saved = await api.saveSettings({ ...settings!, intervalMinutes: Math.max(0, intervalMinutes), thresholds });
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
