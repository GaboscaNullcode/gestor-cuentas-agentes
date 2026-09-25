import { formatReset, level, windowLabel } from "../format";
import type { UsageWindow } from "../types";

interface Props {
  window: UsageWindow;
  now: Date;
  dimmed: boolean;
}

export function WindowBar({ window, now, dimmed }: Props) {
  const passed = window.resetsAt !== null && new Date(window.resetsAt).getTime() <= now.getTime();
  const pct = passed ? 0 : window.usedPct;
  return (
    <div className={`bar-row ${dimmed ? "dimmed" : ""}`}>
      <span className="bar-label">{windowLabel(window.kind)}</span>
      <div className="bar-track">
        <div className={`bar-fill ${level(pct)}`} style={{ width: `${Math.min(pct, 100)}%` }} />
      </div>
      <span className="bar-pct">{Math.round(pct)}%</span>
      <span className="bar-reset">{formatReset(window.resetsAt, now)}</span>
    </div>
  );
}
