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
