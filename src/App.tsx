import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { api } from "./api";
import { AccountCard } from "./components/AccountCard";
import { AddAccountView } from "./components/AddAccountView";
import { LoginView, type LoginTarget } from "./components/LoginView";
import { PanelView } from "./components/PanelView";
import { SettingsView } from "./components/SettingsView";
import type { AccountView, UsageSnapshot, UsageUpdated } from "./types";

type View = { name: "panel" } | { name: "add" } | { name: "settings" } | ({ name: "login" } & LoginTarget);

const FLASH_MS = 1500;

export default function App() {
  const [accounts, setAccounts] = useState<AccountView[]>([]);
  const [snapshots, setSnapshots] = useState<Record<string, UsageSnapshot>>({});
  const [view, setView] = useState<View>({ name: "panel" });
  const [now, setNow] = useState(new Date());
  const [error, setError] = useState<string | null>(null);
  const [flashId, setFlashId] = useState<string | null>(null);
  const toPanel = useCallback(() => setView({ name: "panel" }), []);
  const signedIn = useCallback((accountId: string) => {
    setFlashId(accountId);
    setView({ name: "panel" });
  }, []);

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

  useEffect(() => {
    if (!flashId) return;
    const timer = setTimeout(() => setFlashId(null), FLASH_MS);
    return () => clearTimeout(timer);
  }, [flashId]);

  /** Runs a backend action, surfaces its failure in the banner and resolves whether it succeeded. */
  const run = (action: Promise<unknown>): Promise<boolean> =>
    action.then(
      () => true,
      (e) => {
        setError(String(e));
        return false;
      },
    );
  const ordered = [...accounts].sort(
    (a, b) => Number(b.pinned) - Number(a.pinned) || a.createdAt.localeCompare(b.createdAt),
  );

  if (view.name === "add")
    return (
      <AddAccountView
        aliasNames={accounts.map((a) => a.aliasName)}
        onDone={toPanel}
        onStartLogin={(target) => setView({ name: "login", ...target })}
      />
    );
  if (view.name === "login")
    return (
      <LoginView
        provider={view.provider}
        accountLabel={view.accountLabel}
        start={view.start}
        onSuccess={signedIn}
        onDone={toPanel}
      />
    );
  if (view.name === "settings") return <SettingsView onDone={toPanel} />;

  return (
    <PanelView
      accounts={ordered}
      snapshots={snapshots}
      now={now}
      error={error}
      onDismissError={() => setError(null)}
      onAdd={() => setView({ name: "add" })}
      onOpenSettings={() => setView({ name: "settings" })}
      renderCard={(account) => (
        <AccountCard
          key={account.id}
          account={account}
          snapshot={snapshots[account.id]}
          now={now}
          flash={account.id === flashId}
          onPin={() => run(api.setPinned(account.id))}
          onRename={(label) => run(api.renameAccount(account.id, label))}
          onRefresh={() => run(api.refreshAccount(account.id))}
          onReconnect={() =>
            setView({
              name: "login",
              provider: account.provider,
              accountLabel: account.label,
              start: () => api.reconnect(account.id),
            })
          }
          onRemove={(logout, deleteDir) => run(api.removeAccount(account.id, logout, deleteDir))}
          onCopyAlias={() => run(api.aliasLine(account.id).then((line) => navigator.clipboard.writeText(line)))}
          onShowFolder={() => run(api.openConfigDir(account.id))}
          onOpenSettings={() => setView({ name: "settings" })}
        />
      )}
    />
  );
}
