import type { AccountStatus, Provider, UsageWindow, WindowKind } from "./types";

export type Level = "green" | "yellow" | "red";

export function level(pct: number): Level {
  if (pct >= 80) return "red";
  if (pct >= 50) return "yellow";
  return "green";
}

export function windowLabel(kind: WindowKind): string {
  switch (kind.type) {
    case "fiveHour":
      return "5h";
    case "weekly":
      return "Week";
    case "weeklyScoped":
      return `Week · ${kind.name}`;
    case "other":
      return kind.name;
  }
}

export function hasReset(window: UsageWindow, now: Date): boolean {
  return window.resetsAt !== null && new Date(window.resetsAt).getTime() <= now.getTime();
}

/** Used percentage as the user should read it: a window whose reset has passed starts over at 0. */
export function currentPct(window: UsageWindow, now: Date): number {
  return hasReset(window, now) ? 0 : window.usedPct;
}

const WINDOW_MINUTES: Partial<Record<WindowKind["type"], number>> = {
  fiveHour: 300,
  weekly: 10080,
  weeklyScoped: 10080,
};

/** Percentage of the window already elapsed, or null when its length or reset time is unknown. */
export function pacePct(window: UsageWindow, now: Date): number | null {
  const length = WINDOW_MINUTES[window.kind.type];
  if (!length || !window.resetsAt || hasReset(window, now)) return null;
  const minutesLeft = (new Date(window.resetsAt).getTime() - now.getTime()) / 60000;
  return Math.min(100, Math.max(0, 100 * (1 - minutesLeft / length)));
}

/** Mirrors `slugify` in src-tauri/src/store/accounts.rs so the UI can preview alias names. */
export function slugify(label: string): string {
  const slug = label
    .toLowerCase()
    .replace(/[^a-z0-9]/g, "-")
    .split("-")
    .filter((part) => part !== "")
    .join("-");
  return slug || "account";
}

/** Mirrors `unique_alias` in src-tauri/src/store/accounts.rs. */
export function aliasPreview(provider: Provider, label: string, existing: string[]): string {
  const base = `${provider}-${slugify(label)}`;
  let candidate = base;
  for (let n = 2; existing.includes(candidate); n++) candidate = `${base}-${n}`;
  return candidate;
}

/** "in 2h 10m" under a day, "Mon 09:00" beyond, "resetting…" once passed. */
export function formatReset(iso: string | null, now: Date): string {
  if (!iso) return "";
  const at = new Date(iso);
  const minutes = Math.round((at.getTime() - now.getTime()) / 60000);
  if (minutes <= 0) return "resetting…";
  if (minutes < 60 * 24) {
    const h = Math.floor(minutes / 60);
    const m = minutes % 60;
    return h > 0 ? `in ${h}h ${m}m` : `in ${m}m`;
  }
  return at.toLocaleString("en-US", { weekday: "short", hour: "2-digit", minute: "2-digit", hour12: false });
}

export function formatAgo(iso: string, now: Date): string {
  const minutes = Math.floor((now.getTime() - new Date(iso).getTime()) / 60000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  return `${Math.floor(minutes / 60)}h ago`;
}

export function statusLabel(status: AccountStatus["type"]): string | null {
  switch (status) {
    case "ok":
      return null;
    case "stale":
      return "Stale";
    case "needsLogin":
      return "Signed out";
    case "rateLimited":
      return "Rate limited";
    case "cliMissing":
      return "CLI not found";
    case "error":
      return "Error";
  }
}

export function providerName(provider: Provider): string {
  return provider === "claude" ? "Claude" : "Codex";
}

export function planLabel(plan: string | null): string {
  if (!plan) return "";
  return plan.charAt(0).toUpperCase() + plan.slice(1);
}
