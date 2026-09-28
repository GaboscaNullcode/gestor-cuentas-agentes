// Canvas: canvas/usage-monitor/screens/SignIn.dc.html
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Check, ExternalLink, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { api } from "../api";
import { providerName } from "../format";
import type { LoginFinished, LoginProgress, Provider } from "../types";
import { Button } from "./Button";
import { ProviderMark } from "./ProviderMark";

export interface LoginTarget {
  provider: Provider;
  accountLabel: string;
  /** Starts the CLI sign-in; called after event listeners are attached, and again on Try again. */
  start: () => Promise<unknown>;
}

interface Props extends LoginTarget {
  onSuccess: (accountId: string) => void;
  onDone: () => void;
}

type StepState = "done" | "current" | "failed" | "pending";

function Step({ state, number, last, children }: { state: StepState; number: number; last?: boolean; children: ReactNode }) {
  return (
    <div className={`step ${state} ${last ? "last" : ""}`}>
      <div className="step-marker" aria-hidden="true">
        {state === "done" && <Check size={13} />}
        {state === "failed" && <X size={13} />}
        {state === "pending" && number}
      </div>
      <div className="step-body">{children}</div>
    </div>
  );
}

export function LoginView({ provider, accountLabel, start, onSuccess, onDone }: Props) {
  const [progress, setProgress] = useState<LoginProgress | null>(null);
  const [code, setCode] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const started = useRef(false);

  const begin = useCallback(() => {
    setProgress(null);
    setError(null);
    setFailed(false);
    start().catch((e) => {
      setFailed(true);
      setError(String(e));
    });
  }, [start]);

  useEffect(() => {
    const offProgress = listen<LoginProgress>("login-progress", (e) => setProgress(e.payload));
    const offFinished = listen<LoginFinished>("login-finished", (e) => {
      if (e.payload.ok) onSuccess(e.payload.accountId);
      else {
        setFailed(true);
        setError(e.payload.error ?? "Sign-in failed.");
      }
    });
    Promise.all([offProgress, offFinished]).then(() => {
      if (started.current) return;
      started.current = true;
      begin();
    });
    return () => {
      offProgress.then((off) => off());
      offFinished.then((off) => off());
    };
  }, [begin, onSuccess]);

  async function submit() {
    if (!code.trim()) return;
    try {
      await api.submitLoginCode(code);
      setCode("");
    } catch (e) {
      setError(String(e));
    }
  }

  async function cancel() {
    try {
      await api.cancelLogin();
      onDone();
    } catch (e) {
      setError(String(e));
    }
  }

  const opened = progress !== null;
  const firstState: StepState = opened ? "done" : failed ? "failed" : "current";
  const secondState: StepState = !opened ? "pending" : failed ? "failed" : "current";
  const errorLine = error && <span className="step-error">{error}</span>;

  return (
    <main className="screen">
      <header className="signin-header">
        <ProviderMark provider={provider} size="lg" />
        <div className="stack-6">
          <h1 className="signin-title">Sign in to {providerName(provider)}</h1>
          <span className="signin-subtitle">for the account {accountLabel}</span>
        </div>
      </header>
      <div className="steps" aria-live="polite">
        <Step state={firstState} number={1}>
          <span className="step-title">
            {opened ? "Opened the sign-in page in your browser" : "Opening the sign-in page in your browser"}
          </span>
          {firstState === "failed" && errorLine}
        </Step>
        <Step state={secondState} number={2} last={!progress?.needsCode}>
          <span className="step-title">Authorize Usage Monitor there</span>
          {secondState === "current" && (
            <>
              <span className="step-text">This window updates by itself when you finish.</span>
              {progress?.url && (
                <button type="button" className="step-link" onClick={() => openUrl(progress.url!)}>
                  <span>Open the page again</span>
                  <ExternalLink size={13} aria-hidden="true" />
                </button>
              )}
              {!progress?.needsCode && errorLine}
            </>
          )}
          {secondState === "failed" && errorLine}
        </Step>
        {progress?.needsCode && !failed && (
          <Step state="pending" number={3} last>
            <label className="step-title" htmlFor="login-code">
              Paste the code the browser shows
            </label>
            <div className="code-row">
              <input
                id="login-code"
                className="field mono"
                placeholder="Code"
                autoComplete="off"
                value={code}
                onChange={(e) => setCode(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") submit();
                }}
              />
              <Button disabled={!code.trim()} onClick={submit}>
                Submit
              </Button>
            </div>
            {errorLine}
          </Step>
        )}
      </div>
      <footer className="signin-footer">
        {failed ? (
          <>
            <Button variant="ghost" onClick={onDone}>
              Back
            </Button>
            <Button onClick={begin}>Try again</Button>
          </>
        ) : (
          <Button variant="ghost" onClick={cancel}>
            Cancel
          </Button>
        )}
      </footer>
    </main>
  );
}
