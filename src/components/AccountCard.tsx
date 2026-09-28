import { useRef, useState } from "react";
import { formatAgo, planLabel, providerName, statusLabel } from "../format";
import type { AccountStatus, AccountView, UsageSnapshot } from "../types";
import { WindowBar } from "./WindowBar";

// Statuses where signing in again may fix the account; not ok, rate limited or a missing CLI.
const RECONNECTABLE: AccountStatus["type"][] = ["needsLogin", "stale", "error"];

interface Props {
  account: AccountView;
  snapshot: UsageSnapshot | undefined;
  now: Date;
  onPin: () => void;
  onRename: (label: string) => void;
  onRefresh: () => void;
  onReconnect: () => void;
  onRemove: (logout: boolean, deleteDir: boolean) => void;
  onCopyAlias: () => void;
  onOpenSettings: () => void;
}

export function AccountCard({ account, snapshot, now, onPin, onRename, onRefresh, onReconnect, onRemove, onCopyAlias, onOpenSettings }: Props) {
  const [removing, setRemoving] = useState(false);
  const [logout, setLogout] = useState(false);
  const [deleteDir, setDeleteDir] = useState(false);
  const [draft, setDraft] = useState<string | null>(null);
  // Enter/Escape unmount the input, which can also fire blur; this makes the edit end once.
  const editing = useRef(false);
  const startRename = () => {
    editing.current = true;
    setDraft(account.label);
  };
  const endRename = (save: boolean) => {
    if (!editing.current) return;
    editing.current = false;
    const next = draft?.trim();
    if (save && next && next !== account.label) onRename(next);
    setDraft(null);
  };
  const label = snapshot ? statusLabel(snapshot.status) : "Waiting for first update";
  const status = snapshot?.status.type === "error" ? `${label}: ${snapshot.status.message}` : label;
  const dimmed = snapshot !== undefined && snapshot.status.type !== "ok";
  const canReconnect = snapshot !== undefined && RECONNECTABLE.includes(snapshot.status.type);
  const hasFiveHour = snapshot?.windows.some((w) => w.kind.type === "fiveHour") ?? false;
  return (
    <section className="card">
      <header className="card-header">
        <div>
          <span className={`provider ${account.provider}`}>{providerName(account.provider)}</span>
          {draft === null ? (
            <strong className="account-name" title="Rename" onClick={startRename}>
              {account.label}
            </strong>
          ) : (
            <input
              className="rename"
              autoFocus
              maxLength={40}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onBlur={() => endRename(true)}
              onKeyDown={(e) => {
                if (e.key === "Enter") endRename(true);
                if (e.key === "Escape") endRename(false);
              }}
            />
          )}
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
      {snapshot?.status.type === "cliMissing" && <button onClick={onOpenSettings}>Set CLI path</button>}
      {canReconnect && <button onClick={onReconnect}>Reconnect</button>}
      {removing && (
        <div className="confirm">
          <p>Remove {account.label} from Usage Monitor?</p>
          <label className="checkbox">
            <input type="checkbox" checked={logout} onChange={(e) => setLogout(e.target.checked)} />
            Also sign out of the CLI
          </label>
          {account.canDeleteDir && (
            <label className="checkbox">
              <input type="checkbox" checked={deleteDir} onChange={(e) => setDeleteDir(e.target.checked)} />
              Also delete <code>{account.configDir}</code>
            </label>
          )}
          <div className="card-actions">
            <button onClick={() => onRemove(logout, account.canDeleteDir && deleteDir)}>Remove</button>
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
