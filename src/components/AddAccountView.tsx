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
