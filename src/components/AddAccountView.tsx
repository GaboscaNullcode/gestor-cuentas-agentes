import { useEffect, useState } from "react";
import { api } from "../api";
import { providerName } from "../format";
import type { DetectedAccount, Provider } from "../types";

interface Props {
  onDone: () => void;
  onStartLogin: (title: string, start: () => Promise<unknown>) => void;
}

export function AddAccountView({ onDone, onStartLogin }: Props) {
  const [detected, setDetected] = useState<DetectedAccount[]>([]);
  const [labels, setLabels] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
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
    </div>
  );
}
