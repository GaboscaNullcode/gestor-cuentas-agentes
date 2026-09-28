// Canvas: canvas/usage-monitor/screens/Settings.dc.html
import { Check, ChevronLeft, Plus, X } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api";
import { level, providerName } from "../format";
import type { AliasStatus, CliStatus, Provider, Settings } from "../types";
import { IconButton } from "./IconButton";
import { ProviderMark } from "./ProviderMark";
import { StatusChip } from "./StatusChip";
import { Toggle } from "./Toggle";

const INTERVALS = [2, 5, 10, 15, 30];
const SAVED_MS = 1500;
const PATH_KEYS: Record<Provider, "claudePath" | "codexPath"> = { claude: "claudePath", codex: "codexPath" };

/** The backend expects integers (u8/u32); drop anything it could not deserialize. */
function sanitize(settings: Settings): Settings {
  const thresholds = settings.thresholds.filter((n) => Number.isInteger(n) && n >= 0 && n <= 255);
  const intervalMinutes = Number.isFinite(settings.intervalMinutes) ? Math.round(settings.intervalMinutes) : 0;
  return { ...settings, intervalMinutes: Math.max(0, intervalMinutes), thresholds };
}

interface Props {
  onDone: () => void;
}

export function SettingsView({ onDone }: Props) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [cli, setCli] = useState<CliStatus | null>(null);
  const [aliases, setAliases] = useState<AliasStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [adding, setAdding] = useState(false);
  const [newThreshold, setNewThreshold] = useState("");
  const [editingPath, setEditingPath] = useState<Provider | null>(null);
  const [pathDraft, setPathDraft] = useState("");

  useEffect(() => {
    api.getSettings().then(setSettings).catch((e) => setError(String(e)));
    api.cliStatus().then(setCli).catch((e) => setError(String(e)));
    api.aliasesStatus().then(setAliases).catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    if (savedAt === null) return;
    const timer = setTimeout(() => setSavedAt(null), SAVED_MS);
    return () => clearTimeout(timer);
  }, [savedAt]);

  if (!settings) return null;
  const current = settings;

  async function save(patch: Partial<Settings>) {
    try {
      setSettings(await api.saveSettings(sanitize({ ...current, ...patch })));
      setCli(await api.cliStatus());
      setError(null);
      setSavedAt(Date.now());
    } catch (e) {
      setError(String(e));
    }
  }

  async function toggleAliases() {
    try {
      setAliases(aliases?.installed ? await api.uninstallAliases() : await api.installAliases());
      setError(null);
      setSavedAt(Date.now());
    } catch (e) {
      setError(String(e));
    }
  }

  function addThreshold() {
    const value = Number(newThreshold);
    setAdding(false);
    setNewThreshold("");
    if (newThreshold.trim() === "" || !Number.isInteger(value) || value < 0 || value > 100) return;
    if (current.thresholds.includes(value)) return;
    save({ thresholds: [...current.thresholds, value].sort((a, b) => a - b) });
  }

  function editPath(provider: Provider) {
    setEditingPath(provider);
    setPathDraft(current[PATH_KEYS[provider]] ?? "");
  }

  function savePath(provider: Provider) {
    setEditingPath(null);
    const next = pathDraft.trim() || null;
    if (next !== current[PATH_KEYS[provider]]) save({ [PATH_KEYS[provider]]: next });
  }

  return (
    <main className="screen">
      <header className="screen-header">
        <IconButton icon={ChevronLeft} label="Back" onClick={onDone} />
        <h1 className="screen-title grow">Settings</h1>
        {savedAt !== null && (
          <span key={savedAt} className="saved" role="status">
            <Check size={14} aria-hidden="true" />
            Saved
          </span>
        )}
      </header>
      <div className="screen-body settings-body">
        {error && (
          <p className="inline-error" role="alert">
            {error}
          </p>
        )}
        <section className="stack-8">
          <h2 className="section-label">Updates</h2>
          <div className="group">
            <div className="group-row column">
              <span className="row-title" id="interval-label">
                Refresh every
              </span>
              <div className="segmented interval" role="group" aria-labelledby="interval-label">
                {INTERVALS.map((minutes) => (
                  <button
                    key={minutes}
                    type="button"
                    aria-pressed={current.intervalMinutes === minutes}
                    className={`segment ${current.intervalMinutes === minutes ? "selected" : ""}`}
                    onClick={() => save({ intervalMinutes: minutes })}
                  >
                    {minutes} min
                  </button>
                ))}
              </div>
            </div>
            <div className="group-row column">
              <div className="stack-4">
                <span className="row-title">Notify when usage reaches</span>
                <span className="row-hint">One notification per window and threshold.</span>
              </div>
              <div className="chips">
                {current.thresholds.map((t) => (
                  <span key={t} className={`chip ${level(t)}`}>
                    <span>{t}%</span>
                    <button
                      type="button"
                      className="chip-remove"
                      aria-label={`Remove ${t}%`}
                      title={`Remove ${t}%`}
                      onClick={() => save({ thresholds: current.thresholds.filter((x) => x !== t) })}
                    >
                      <X size={12} aria-hidden="true" />
                    </button>
                  </span>
                ))}
                {adding ? (
                  <input
                    className="chip chip-input"
                    type="number"
                    min={0}
                    max={100}
                    aria-label="New threshold, percent"
                    autoFocus
                    value={newThreshold}
                    onChange={(e) => setNewThreshold(e.target.value)}
                    onBlur={() => {
                      setAdding(false);
                      setNewThreshold("");
                    }}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") addThreshold();
                      if (e.key === "Escape") {
                        setAdding(false);
                        setNewThreshold("");
                      }
                    }}
                  />
                ) : (
                  <button type="button" className="chip chip-add" onClick={() => setAdding(true)}>
                    <Plus size={12} aria-hidden="true" />
                    <span>Add</span>
                  </button>
                )}
              </div>
            </div>
            <div className="group-row">
              <span className="row-title grow">Launch at login</span>
              <Toggle label="Launch at login" on={current.launchAtLogin} onChange={(on) => save({ launchAtLogin: on })} />
            </div>
          </div>
        </section>
        <section className="stack-8">
          <h2 className="section-label">Command-line tools</h2>
          <div className="group">
            {(["claude", "codex"] as Provider[]).map((provider) => {
              const detected = cli?.[provider] ?? null;
              const missing = cli !== null && detected === null;
              const editing = editingPath === provider || missing;
              return (
                <div key={provider} className="group-row cli-row">
                  <div className="cli-row-main">
                    <ProviderMark provider={provider} />
                    <div className="cli-info">
                      <span className="row-title">{providerName(provider)} CLI</span>
                      {detected && <span className="mono-path ellipsis">{detected}</span>}
                    </div>
                    {missing ? (
                      <StatusChip status="cliMissing" />
                    ) : (
                      !editing && (
                        <button type="button" className="text-btn" onClick={() => editPath(provider)}>
                          Change
                        </button>
                      )
                    )}
                  </div>
                  {editing && (
                    <input
                      className="field mono"
                      aria-label={`${providerName(provider)} CLI path`}
                      placeholder="Leave empty to detect automatically"
                      autoFocus={editingPath === provider}
                      value={editingPath === provider ? pathDraft : (current[PATH_KEYS[provider]] ?? "")}
                      onFocus={() => editingPath !== provider && editPath(provider)}
                      onChange={(e) => setPathDraft(e.target.value)}
                      onBlur={() => savePath(provider)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") e.currentTarget.blur();
                      }}
                    />
                  )}
                </div>
              );
            })}
          </div>
        </section>
        <section className="stack-8">
          <h2 className="section-label">Shell aliases</h2>
          <div className="group">
            <div className="group-row">
              <div className="stack-4 grow">
                <span className="row-title">One alias per account</span>
                <span className="row-hint">
                  {aliases?.installed ? (
                    <>
                      Installed in <span className="mono">{aliases.targets.join(", ")}</span>. Open a new terminal to use
                      them.
                    </>
                  ) : (
                    "Adds one line to your shell profile that loads an alias per account."
                  )}
                </span>
              </div>
              <Toggle
                label="Shell aliases"
                on={aliases?.installed ?? false}
                disabled={aliases === null}
                onChange={toggleAliases}
              />
            </div>
          </div>
        </section>
      </div>
    </main>
  );
}
