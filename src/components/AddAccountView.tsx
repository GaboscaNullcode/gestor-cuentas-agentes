// Canvas: canvas/usage-monitor/screens/AddAccount.dc.html
import { ChevronLeft, Folder } from "lucide-react";
import { useEffect, useState } from "react";
import { api } from "../api";
import { aliasPreview, providerName } from "../format";
import type { DetectedAccount, Provider } from "../types";
import { Button } from "./Button";
import { Checkbox } from "./Checkbox";
import { IconButton } from "./IconButton";
import type { LoginTarget } from "./LoginView";
import { ProviderMark } from "./ProviderMark";

const PROVIDERS: Provider[] = ["claude", "codex"];

interface Props {
  /** Alias names already taken, to preview the one the backend will assign. */
  aliasNames: string[];
  onDone: () => void;
  onStartLogin: (target: LoginTarget) => void;
}

export function AddAccountView({ aliasNames, onDone, onStartLogin }: Props) {
  const [detected, setDetected] = useState<DetectedAccount[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [provider, setProvider] = useState<Provider>("claude");
  const [label, setLabel] = useState("");
  const [dir, setDir] = useState("");
  const [dirEdited, setDirEdited] = useState(false);
  const [advanced, setAdvanced] = useState(false);
  const [alreadySignedIn, setAlreadySignedIn] = useState(false);

  useEffect(() => {
    if (dirEdited || !label.trim()) return;
    api.proposeConfigDir(provider, label).then(setDir);
  }, [provider, label, dirEdited]);

  useEffect(() => {
    api.detectExisting().then(setDetected).catch((e) => setError(String(e)));
  }, []);

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
    onStartLogin({ provider, accountLabel: label.trim(), start: () => api.addAccount(provider, label, dir) });
  }

  async function importAccount(d: DetectedAccount) {
    try {
      await api.addExisting(d.provider, "Main", d.configDir, d.useDefaultDir);
      onDone();
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <main className="screen">
      <header className="screen-header">
        <IconButton icon={ChevronLeft} label="Back" onClick={onDone} />
        <h1 className="screen-title">Add account</h1>
      </header>
      <div className="screen-body add-body">
        {detected.length > 0 && (
          <section className="stack-8">
            <h2 className="section-label">Found on this computer</h2>
            {detected.map((d) => (
              <div key={d.configDir} className="detected-row">
                <ProviderMark provider={d.provider} />
                <div className="detected-info">
                  <span className="row-title">{providerName(d.provider)} · signed in</span>
                  <span className="mono-path">{d.configDir}</span>
                </div>
                <Button variant="secondary" size="sm" onClick={() => importAccount(d)}>
                  Import
                </Button>
              </div>
            ))}
          </section>
        )}
        <section className="stack-12">
          <h2 className="section-label">New account</h2>
          <div className="provider-tiles" role="group" aria-label="Provider">
            {PROVIDERS.map((p) => (
              <button
                key={p}
                type="button"
                aria-pressed={provider === p}
                className={`provider-tile ${provider === p ? "selected" : ""}`}
                onClick={() => setProvider(p)}
              >
                <ProviderMark provider={p} />
                <span>{providerName(p)}</span>
              </button>
            ))}
          </div>
          <label className="field-label">
            Name
            <input
              className="field"
              placeholder="e.g. Personal"
              maxLength={40}
              value={label}
              onChange={(e) => setLabel(e.target.value)}
            />
            <span className="field-help">
              {label.trim()
                ? `Shown on the card and used for the shell alias ${aliasPreview(provider, label, aliasNames)}.`
                : "Shown on the card and used for the shell alias."}
            </span>
          </label>
          <div className="advanced">
            <button type="button" className="advanced-toggle" aria-expanded={advanced} onClick={() => setAdvanced((a) => !a)}>
              <span className="grow">Advanced</span>
              <span className="advanced-action">{advanced ? "Hide" : "Show"}</span>
            </button>
            {advanced ? (
              <>
                <label className="path-field">
                  <Folder size={14} aria-hidden="true" />
                  <input
                    aria-label="Config folder"
                    placeholder="Config folder"
                    value={dir}
                    onChange={(e) => {
                      setDir(e.target.value);
                      setDirEdited(true);
                    }}
                  />
                </label>
                <Checkbox checked={alreadySignedIn} onChange={setAlreadySignedIn}>
                  This folder is already signed in
                </Checkbox>
              </>
            ) : (
              dir && <span className="mono-path advanced-summary">{dir}</span>
            )}
          </div>
        </section>
        {error && (
          <p className="inline-error" role="alert">
            {error}
          </p>
        )}
      </div>
      <footer className="screen-footer">
        <Button disabled={!label.trim() || !dir.trim()} onClick={submitNew}>
          {alreadySignedIn ? "Add account" : "Continue to sign in"}
        </Button>
      </footer>
    </main>
  );
}
