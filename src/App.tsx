import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { api } from "./api";
import { AccountCard } from "./components/AccountCard";
import { AddAccountView } from "./components/AddAccountView";
import { LoginView } from "./components/LoginView";
import { SettingsView } from "./components/SettingsView";
import type { Account, UsageSnapshot, UsageUpdated } from "./types";

type View = { name: "panel" } | { name: "add" } | { name: "settings" } | { name: "login"; title: string; start: () => Promise<unknown> };

export default function App() {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [snapshots, setSnapshots] = useState<Record<string, UsageSnapshot>>({});
  const [view, setView] = useState<View>({ name: "panel" });
  const [now, setNow] = useState(new Date());
  const [error, setError] = useState<string | null>(null);
  const toPanel = useCallback(() => setView({ name: "panel" }), []);

  const reload = useCallback(async () => {
    setAccounts(await api.listAccounts());
    setSnapshots(await api.getSnapshots());
  }, []);

  useEffect(() => {
    reload();
    const timer = setInterval(() => setNow(new Date()), 30_000);
    const offUsage = listen<UsageUpdated>("usage-updated", (e) =>
      setSnapshots((s) => ({ ...s, [e.payload.accountId]: e.payload.snapshot })),
    );
    const offAccounts = listen("accounts-changed", () => reload());
    return () => {
      clearInterval(timer);
      offUsage.then((off) => off());
      offAccounts.then((off) => off());
    };
  }, [reload]);

  const run = (action: Promise<unknown>) => action.catch((e) => setError(String(e)));
  const ordered = [...accounts].sort(
    (a, b) => Number(b.pinned) - Number(a.pinned) || a.createdAt.localeCompare(b.createdAt),
  );

  if (view.name === "add")
    return <AddAccountView onDone={toPanel} onStartLogin={(title, start) => setView({ name: "login", title, start })} />;
  if (view.name === "login") return <LoginView title={view.title} start={view.start} onDone={toPanel} />;
  if (view.name === "settings") return <SettingsView onDone={toPanel} />;

  return (
    <main className="panel">
      <header className="panel-header">
        <h1>Usage</h1>
        <div className="card-actions">
          <button title="Settings" onClick={() => setView({ name: "settings" })}>
            ⚙
          </button>
          <button onClick={() => setView({ name: "add" })}>+ Add account</button>
        </div>
      </header>
      {error && (
        <p className="error" onClick={() => setError(null)}>
          {error}
        </p>
      )}
      {ordered.length === 0 && <p className="muted">No accounts yet. Add one to start tracking usage.</p>}
      {ordered.map((account) => (
        <AccountCard
          key={account.id}
          account={account}
          snapshot={snapshots[account.id]}
          now={now}
          onPin={() => run(api.setPinned(account.id))}
          onRefresh={() => run(api.refreshAccount(account.id))}
          onReconnect={() => setView({ name: "login", title: `Reconnect ${account.label}`, start: () => api.reconnect(account.id) })}
          onRemove={(logout, deleteDir) => run(api.removeAccount(account.id, logout, deleteDir))}
          onCopyAlias={() => run(api.aliasLine(account.id).then((line) => navigator.clipboard.writeText(line)))}
          onOpenSettings={() => setView({ name: "settings" })}
        />
      ))}
    </main>
  );
}
