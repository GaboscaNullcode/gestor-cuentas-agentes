import { useState } from "react";
import { formatAgo, planLabel, providerName, statusLabel } from "../format";
import type { Account, UsageSnapshot } from "../types";
import { WindowBar } from "./WindowBar";

interface Props {
  account: Account;
  snapshot: UsageSnapshot | undefined;
  now: Date;
  onPin: () => void;
  onRefresh: () => void;
  onReconnect: () => void;
  onRemove: (logout: boolean, deleteDir: boolean) => void;
  onCopyAlias: () => void;
}

export function AccountCard({ account, snapshot, now, onPin, onRefresh, onReconnect, onRemove, onCopyAlias }: Props) {
  const [removing, setRemoving] = useState(false);
  const [logout, setLogout] = useState(false);
  const [deleteDir, setDeleteDir] = useState(false);
  const label = snapshot ? statusLabel(snapshot.status) : "Waiting for first update";
  const status = snapshot?.status.type === "error" ? `${label}: ${snapshot.status.message}` : label;
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
          <button title={`Copy alias ${account.aliasName}`} onClick={onCopyAlias}>
            ⌘
          </button>
          <button title="Remove" onClick={() => setRemoving(true)}>
            ✕
          </button>
        </div>
      </header>
      {status && (
        <p className={`status ${snapshot?.status.type ?? "pending"}`} title={snapshot?.lastError ?? undefined}>
          {status}
        </p>
      )}
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
