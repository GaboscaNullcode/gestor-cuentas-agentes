import type { AccountStatus, Provider, WindowKind } from "./types";

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

export function statusLabel(status: AccountStatus): string | null {
  switch (status.type) {
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
