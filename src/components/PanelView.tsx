// Canvas: canvas/usage-monitor/screens/Panel.dc.html
import { Plus, Settings, X } from "lucide-react";
import { useState, type ReactNode } from "react";
import { currentPct, level, providerName } from "../format";
import type { AccountView, Provider, UsageSnapshot } from "../types";
import { Button } from "./Button";
import { IconButton } from "./IconButton";
import { ProviderMark } from "./ProviderMark";

type Filter = "all" | Provider;

const PROVIDERS: Provider[] = ["claude", "codex"];
const FILTER_KEY = "usage-monitor.filter";

function loadFilter(): Filter {
  try {
    const saved = localStorage.getItem(FILTER_KEY);
    return saved === "claude" || saved === "codex" ? saved : "all";
  } catch {
    // Storage can be unavailable (private mode, blocked site data); the filter is a convenience.
    return "all";
  }
}

function saveFilter(filter: Filter) {
  try {
    localStorage.setItem(FILTER_KEY, filter);
  } catch {
    // Same as loadFilter: losing the remembered filter is acceptable.
  }
}

interface Headroom {
  account: AccountView;
  leftPct: number;
}

/** Among accounts with status ok, the one whose tightest window has the most room left. */
function mostHeadroom(accounts: AccountView[], snapshots: Record<string, UsageSnapshot>, now: Date): Headroom | null {
  let best: Headroom | null = null;
  for (const account of accounts) {
    const snapshot = snapshots[account.id];
    if (snapshot?.status.type !== "ok" || snapshot.windows.length === 0) continue;
    const leftPct = 100 - Math.max(...snapshot.windows.map((w) => currentPct(w, now)));
    if (!best || leftPct > best.leftPct) best = { account, leftPct };
  }
  return best;
}

function plural(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

interface Props {
  /** Already sorted for display. */
  accounts: AccountView[];
  snapshots: Record<string, UsageSnapshot>;
  now: Date;
  error: string | null;
  onDismissError: () => void;
  onAdd: () => void;
  onOpenSettings: () => void;
  renderCard: (account: AccountView) => ReactNode;
}

export function PanelView({ accounts, snapshots, now, error, onDismissError, onAdd, onOpenSettings, renderCard }: Props) {
  const [savedFilter, setSavedFilter] = useState<Filter>(loadFilter);
  const counts = { claude: 0, codex: 0 };
  for (const a of accounts) counts[a.provider]++;
  const mixed = counts.claude > 0 && counts.codex > 0;
  // With a single provider the filter is hidden, so a remembered filter must not hide cards.
  const filter: Filter = mixed ? savedFilter : "all";
  const visible = filter === "all" ? accounts : accounts.filter((a) => a.provider === filter);
  const attention = visible.filter((a) => {
    const status = snapshots[a.id]?.status.type;
    return status !== undefined && status !== "ok";
  }).length;
  const noun = filter === "all" ? "account" : `${providerName(filter)} account`;
  const subtitle =
    accounts.length === 0
      ? "No accounts yet"
      : [plural(visible.length, noun, `${noun}s`), attention > 0 && plural(attention, "needs attention", "need attention")]
          .filter(Boolean)
          .join(" · ");
  const headroom = PROVIDERS.filter((p) => filter === "all" || filter === p).flatMap((provider) => {
    const best = mostHeadroom(
      visible.filter((a) => a.provider === provider),
      snapshots,
      now,
    );
    return best ? [{ provider, ...best }] : [];
  });

  function choose(next: Filter) {
    setSavedFilter(next);
    saveFilter(next);
  }

  return (
    <main className="screen">
      <header className="panel-header">
        <div className="panel-heading">
          <h1 className="screen-title">Usage</h1>
          <span className="panel-subtitle">{subtitle}</span>
        </div>
        <IconButton icon={Settings} label="Settings" onClick={onOpenSettings} />
        <Button size="sm" icon={Plus} onClick={onAdd}>
          Add
        </Button>
      </header>
      {error && (
        <div className="banner" role="alert">
          <span className="banner-text">{error}</span>
          <IconButton icon={X} label="Dismiss" onClick={onDismissError} />
        </div>
      )}
      {accounts.length === 0 ? (
        <div className="empty">
          <div className="empty-marks">
            <ProviderMark provider="claude" size="lg" />
            <ProviderMark provider="codex" size="lg" />
          </div>
          <span className="empty-title">Track your Claude and Codex limits</span>
          <span className="empty-text">
            Add an account to see its 5-hour and weekly usage here and in the tray. Sessions already signed in on this
            computer can be imported in one click.
          </span>
          <div className="empty-action">
            <Button icon={Plus} onClick={onAdd}>
              Add account
            </Button>
          </div>
        </div>
      ) : (
        <>
          {mixed && (
            <div className="segmented filter" role="group" aria-label="Filter by provider">
              {(["all", ...PROVIDERS] as Filter[]).map((option) => (
                <button
                  key={option}
                  type="button"
                  aria-pressed={filter === option}
                  className={`segment ${filter === option ? "selected" : ""}`}
                  onClick={() => choose(option)}
                >
                  {option !== "all" && <ProviderMark provider={option} size="sm" />}
                  <span>{option === "all" ? "All" : providerName(option)}</span>
                  <span className="count">{option === "all" ? accounts.length : counts[option]}</span>
                </button>
              ))}
            </div>
          )}
          {headroom.length > 0 && (
            <div className="headroom">
              <span className="section-label">Most headroom now</span>
              <div className="headroom-cells" style={{ gridTemplateColumns: `repeat(${headroom.length}, minmax(0, 1fr))` }}>
                {headroom.map(({ provider, account, leftPct }) => (
                  <div key={provider} className="headroom-cell">
                    <ProviderMark provider={provider} size="sm" />
                    <span className="headroom-name">{account.label}</span>
                    <span className={`headroom-pct ${level(100 - leftPct)}`}>{Math.round(leftPct)}% left</span>
                  </div>
                ))}
              </div>
            </div>
          )}
          <div className="card-list">{visible.map(renderCard)}</div>
        </>
      )}
    </main>
  );
}
