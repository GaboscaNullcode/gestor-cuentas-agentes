// Canvas: canvas/usage-monitor/components/StatusChip.dc.html
import { statusLabel } from "../format";
import type { AccountStatus } from "../types";

export type ChipStatus = AccountStatus["type"] | "pending";
export type Tone = "ok" | "warn" | "danger" | "neutral";

export function statusTone(status: ChipStatus): Tone {
  switch (status) {
    case "ok":
      return "ok";
    case "stale":
    case "rateLimited":
      return "warn";
    case "needsLogin":
    case "cliMissing":
    case "error":
      return "danger";
    case "pending":
      return "neutral";
  }
}

interface Props {
  status: ChipStatus;
}

export function StatusChip({ status }: Props) {
  const label = status === "pending" ? "Waiting for first update" : (statusLabel(status) ?? "Up to date");
  return (
    <span className={`status-chip ${statusTone(status)}`}>
      <span className="dot" aria-hidden="true" />
      <span>{label}</span>
    </span>
  );
}
