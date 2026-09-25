export type Provider = "claude" | "codex";

export interface Account {
  id: string;
  provider: Provider;
  label: string;
  configDir: string;
  useDefaultDir: boolean;
  pinned: boolean;
  aliasName: string;
  createdAt: string;
}

export type WindowKind =
  | { type: "fiveHour" }
  | { type: "weekly" }
  | { type: "weeklyScoped"; name: string }
  | { type: "other"; name: string };

export interface UsageWindow {
  kind: WindowKind;
  usedPct: number;
  resetsAt: string | null;
}

export type AccountStatus =
  | { type: "ok" }
  | { type: "stale" }
  | { type: "needsLogin" }
  | { type: "rateLimited" }
  | { type: "cliMissing" }
  | { type: "error"; message: string };

export interface UsageSnapshot {
  plan: string | null;
  windows: UsageWindow[];
  fetchedAt: string;
  status: AccountStatus;
  lastError: string | null;
}

export interface UsageUpdated {
  accountId: string;
  snapshot: UsageSnapshot;
}

export interface DetectedAccount {
  provider: Provider;
  configDir: string;
  useDefaultDir: boolean;
}

export interface LoginProgress {
  accountId: string;
  url: string | null;
  needsCode: boolean;
}

export interface LoginFinished {
  accountId: string;
  ok: boolean;
  error: string | null;
}

export interface AliasStatus {
  installed: boolean;
  targets: string[];
}
