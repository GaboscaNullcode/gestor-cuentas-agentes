import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { api } from "./api";
import { AccountCard } from "./components/AccountCard";
import { AddAccountView } from "./components/AddAccountView";
import type { Account, UsageSnapshot, UsageUpdated } from "./types";

type View = { name: "panel" } | { name: "add" };

export default function App() {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [snapshots, setSnapshots] = useState<Record<string, UsageSnapshot>>({});
  const [view, setView] = useState<View>({ name: "panel" });
  const [now, setNow] = useState(new Date());
  const [error, setError] = useState<string | null>(null);

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

  if (view.name === "add") return <AddAccountView onDone={() => setView({ name: "panel" })} />;

  return (
    <main className="panel">
      <header className="panel-header">
        <h1>Usage</h1>
        <button onClick={() => setView({ name: "add" })}>+ Add account</button>
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
        />
      ))}
    </main>
  );
}
