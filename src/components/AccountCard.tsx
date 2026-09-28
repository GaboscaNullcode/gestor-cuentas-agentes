// Canvas: canvas/usage-monitor/components/AccountCard.dc.html
import { Ellipsis, RefreshCw, Star } from "lucide-react";
import { useCallback, useRef, useState } from "react";
import { formatAgo, planLabel, providerName } from "../format";
import type { AccountStatus, AccountView, UsageSnapshot } from "../types";
import { AccountMenu } from "./AccountMenu";
import { Button } from "./Button";
import { IconButton } from "./IconButton";
import { ProviderMark } from "./ProviderMark";
import { RemoveAccountSheet } from "./RemoveAccountSheet";
import { StatusChip, statusTone } from "./StatusChip";
import { NoFiveHourBar, SkeletonBar, WindowBar } from "./WindowBar";

// Statuses where signing in again may fix the account; not ok, rate limited or a missing CLI.
const RECONNECTABLE: AccountStatus["type"][] = ["needsLogin", "stale", "error"];

const NOTICES: Record<Exclude<AccountStatus["type"], "ok">, string> = {
  stale: "Showing the last good reading.",
  rateLimited: "Limit reached. Usage resumes at the next reset.",
  needsLogin: "The CLI session expired.",
  cliMissing: "The CLI could not be found.",
  error: "The last update failed.",
};

interface Props {
  account: AccountView;
  snapshot: UsageSnapshot | undefined;
  now: Date;
  /** Flashes the border once, after the account was just signed in. */
  flash: boolean;
  onPin: () => void;
  onRename: (label: string) => void;
  /** Async actions report their own failures; the card only waits for them to settle. */
  onRefresh: () => Promise<unknown>;
  onReconnect: () => void;
  onRemove: (logout: boolean, deleteDir: boolean) => Promise<unknown>;
  /** Resolves true once the alias line is on the clipboard. */
  onCopyAlias: () => Promise<boolean>;
  onShowFolder: () => void;
  onOpenSettings: () => void;
}

export function AccountCard(props: Props) {
  const { account, snapshot, now, flash, onPin, onRename, onRefresh, onReconnect, onRemove, onCopyAlias, onShowFolder, onOpenSettings } =
    props;
  const [menuOpen, setMenuOpen] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [draft, setDraft] = useState<string | null>(null);
  const moreButton = useRef<HTMLButtonElement>(null);
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
  const closeMenu = useCallback(() => setMenuOpen(false), []);
  const cancelRemove = useCallback(() => {
    setRemoving(false);
    moreButton.current?.focus();
  }, []);

  async function refresh() {
    setRefreshing(true);
    await onRefresh();
    setRefreshing(false);
  }

  const status = snapshot?.status.type;
  // A rate-limited reading is current, so it stays at full strength; other problems mean stale numbers.
  const dimmed = status !== undefined && status !== "ok" && status !== "rateLimited";
  const canReconnect = status !== undefined && RECONNECTABLE.includes(status);
  const hasFiveHour = snapshot?.windows.some((w) => w.kind.type === "fiveHour") ?? false;
  const errorDetail = snapshot?.status.type === "error" ? snapshot.status.message : (snapshot?.lastError ?? undefined);

  return (
    <section className={`card ${flash ? "flash" : ""}`} aria-label={account.label}>
      <header className="card-header">
        <ProviderMark provider={account.provider} />
        <div className="card-title">
          <div className="card-name-row">
            {draft === null ? (
              <button type="button" className="card-name" title="Rename" onClick={startRename}>
                {account.label}
              </button>
            ) : (
              <input
                className="card-rename"
                aria-label="Account name"
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
            {snapshot?.plan && <span className="plan-pill">{planLabel(snapshot.plan)}</span>}
          </div>
          <span className="card-provider">{providerName(account.provider)}</span>
        </div>
        <div className="card-actions">
          <IconButton
            icon={Star}
            filled={account.pinned}
            label={account.pinned ? "Shown in tray" : "Show in tray"}
            aria-pressed={account.pinned}
            onClick={onPin}
          />
          <IconButton
            ref={moreButton}
            icon={Ellipsis}
            label="More actions"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            active={menuOpen}
            onClick={() => setMenuOpen((open) => !open)}
          />
        </div>
      </header>
      {snapshot === undefined && (
        <div className="notice">
          <StatusChip status="pending" />
        </div>
      )}
      {status !== undefined && status !== "ok" && (
        <div className={`notice ${statusTone(status)}`} title={errorDetail}>
          <StatusChip status={status} />
          <span className="notice-text">{NOTICES[status]}</span>
          {canReconnect && (
            <Button variant="secondary" size="sm" onClick={onReconnect}>
              Reconnect
            </Button>
          )}
          {status === "cliMissing" && (
            <Button variant="secondary" size="sm" onClick={onOpenSettings}>
              Set path
            </Button>
          )}
        </div>
      )}
      {snapshot === undefined && (
        <div className="card-bars">
          <SkeletonBar />
          <SkeletonBar />
        </div>
      )}
      {snapshot && snapshot.windows.length > 0 && (
        <div className="card-bars">
          {!hasFiveHour && <NoFiveHourBar dimmed={dimmed} />}
          {snapshot.windows.map((w, i) => (
            <WindowBar key={i} window={w} now={now} dimmed={dimmed} />
          ))}
        </div>
      )}
      <footer className="card-footer">
        <span>{snapshot ? `Updated ${formatAgo(snapshot.fetchedAt, now)}` : ""}</span>
        <button type="button" className="link-btn" disabled={refreshing} onClick={refresh}>
          <RefreshCw size={13} className={refreshing ? "spin" : ""} aria-hidden="true" />
          <span>Refresh</span>
        </button>
      </footer>
      {menuOpen && (
        <AccountMenu
          anchor={moreButton}
          aliasName={account.aliasName}
          onClose={closeMenu}
          onRename={() => {
            closeMenu();
            startRename();
          }}
          onCopyAlias={onCopyAlias}
          onShowFolder={() => {
            closeMenu();
            onShowFolder();
          }}
          onRemove={() => {
            closeMenu();
            setRemoving(true);
          }}
        />
      )}
      {removing && (
        <RemoveAccountSheet
          account={account}
          onCancel={cancelRemove}
          onRemove={async (logout, deleteDir) => {
            await onRemove(logout, deleteDir);
            setRemoving(false);
          }}
        />
      )}
    </section>
  );
}
