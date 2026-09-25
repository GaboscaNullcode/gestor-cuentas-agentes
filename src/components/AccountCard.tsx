import { formatAgo, planLabel, providerName, statusLabel } from "../format";
import type { Account, UsageSnapshot } from "../types";
import { WindowBar } from "./WindowBar";

interface Props {
  account: Account;
  snapshot: UsageSnapshot | undefined;
  now: Date;
  onPin: () => void;
  onRefresh: () => void;
}

export function AccountCard({ account, snapshot, now, onPin, onRefresh }: Props) {
  const status = snapshot ? statusLabel(snapshot.status) : "Waiting for first update";
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
        </div>
      </header>
      {status && <p className={`status ${snapshot?.status.type ?? "pending"}`}>{status}</p>}
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
