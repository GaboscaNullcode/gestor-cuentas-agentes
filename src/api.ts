import { invoke } from "@tauri-apps/api/core";
import type { Account, DetectedAccount, Provider, UsageSnapshot } from "./types";

export const api = {
  listAccounts: () => invoke<Account[]>("list_accounts"),
  getSnapshots: () => invoke<Record<string, UsageSnapshot>>("get_snapshots"),
  refreshAccount: (id: string) => invoke<void>("refresh_account", { id }),
  setPinned: (id: string) => invoke<void>("set_pinned", { id }),
  detectExisting: () => invoke<DetectedAccount[]>("detect_existing"),
  addExisting: (provider: Provider, label: string, configDir: string, useDefaultDir: boolean) =>
    invoke<Account>("add_existing", { provider, label, configDir, useDefaultDir }),
};
