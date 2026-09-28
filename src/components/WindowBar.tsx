// Canvas: canvas/usage-monitor/components/WindowBar.dc.html
import { currentPct, formatReset, hasReset, level, pacePct, windowLabel } from "../format";
import type { UsageWindow } from "../types";

interface Props {
  window: UsageWindow;
  now: Date;
  dimmed: boolean;
}

export function WindowBar({ window, now, dimmed }: Props) {
  const pct = currentPct(window, now);
  const label = windowLabel(window.kind);
  const pace = pacePct(window, now);
  const reset = hasReset(window, now) ? "Resetting" : window.resetsAt ? `resets ${formatReset(window.resetsAt, now)}` : "";
  return (
    <div className={`window-bar ${dimmed ? "dimmed" : ""}`}>
      <div className="window-bar-head">
        <span className="window-bar-label" title={label}>
          {label}
        </span>
        <span className="window-bar-reset">{reset}</span>
        <span className={`window-bar-pct ${level(pct)}`}>{Math.round(pct)}%</span>
      </div>
      <div className="window-bar-track">
        <div className={`window-bar-fill ${level(pct)}`} style={{ width: `${Math.min(pct, 100)}%` }} />
        {pace !== null && (
          <div
            className="window-bar-pace"
            style={{ left: `calc(${pace}% - 1px)` }}
            title={`Pace: ${Math.round(pace)}% of the window elapsed`}
          />
        )}
      </div>
    </div>
  );
}

/** Placeholder row for plans without a five-hour window. */
export function NoFiveHourBar({ dimmed }: { dimmed: boolean }) {
  return (
    <div className={`window-bar ${dimmed ? "dimmed" : ""}`}>
      <div className="window-bar-head">
        <span className="window-bar-label">5h</span>
        <span className="window-bar-reset">No 5h limit on this plan</span>
        <span className="window-bar-pct none">—</span>
      </div>
      <div className="window-bar-track none" />
    </div>
  );
}

/** Loading row shown while an account waits for its first snapshot. */
export function SkeletonBar() {
  return (
    <div className="window-bar skeleton" aria-hidden="true">
      <div className="window-bar-head">
        <span className="skeleton-text" />
      </div>
      <div className="window-bar-track" />
    </div>
  );
}
