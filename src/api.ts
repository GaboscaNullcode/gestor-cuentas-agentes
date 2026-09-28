import { invoke } from "@tauri-apps/api/core";
import type { Account, AccountView, AliasStatus, CliStatus, DetectedAccount, Provider, Settings, UsageSnapshot } from "./types";

export const api = {
  listAccounts: () => invoke<AccountView[]>("list_accounts"),
  getSnapshots: () => invoke<Record<string, UsageSnapshot>>("get_snapshots"),
  refreshAccount: (id: string) => invoke<void>("refresh_account", { id }),
  setPinned: (id: string) => invoke<void>("set_pinned", { id }),
  renameAccount: (id: string, label: string) => invoke<void>("rename_account", { id, label }),
  detectExisting: () => invoke<DetectedAccount[]>("detect_existing"),
  addExisting: (provider: Provider, label: string, configDir: string, useDefaultDir: boolean) =>
    invoke<Account>("add_existing", { provider, label, configDir, useDefaultDir }),
  proposeConfigDir: (provider: Provider, label: string) => invoke<string>("propose_config_dir", { provider, label }),
  addAccount: (provider: Provider, label: string, configDir: string) =>
    invoke<Account>("add_account", { provider, label, configDir }),
  reconnect: (id: string) => invoke<void>("reconnect", { id }),
  submitLoginCode: (code: string) => invoke<void>("submit_login_code", { code }),
  cancelLogin: () => invoke<void>("cancel_login"),
  removeAccount: (id: string, logout: boolean, deleteDir: boolean) =>
    invoke<void>("remove_account", { id, logout, deleteDir }),
  aliasLine: (id: string) => invoke<string>("alias_line", { id }),
  openConfigDir: (id: string) => invoke<void>("open_config_dir", { id }),
  aliasesStatus: () => invoke<AliasStatus>("aliases_status"),
  installAliases: () => invoke<AliasStatus>("install_aliases"),
  uninstallAliases: () => invoke<AliasStatus>("uninstall_aliases"),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  cliStatus: () => invoke<CliStatus>("cli_status"),
};
