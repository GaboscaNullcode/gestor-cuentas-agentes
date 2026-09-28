// Canvas: canvas/usage-monitor/components/IconButton.dc.html
import type { LucideIcon } from "lucide-react";
import type { ComponentProps } from "react";

interface Props extends ComponentProps<"button"> {
  icon: LucideIcon;
  /** Accessible name and tooltip. */
  label: string;
  /** Draws the icon filled, as the pinned star does. */
  filled?: boolean;
  active?: boolean;
}

export function IconButton({ icon: Icon, label, filled = false, active = false, className, ...rest }: Props) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      className={`icon-btn ${filled ? "filled" : ""} ${active ? "active" : ""} ${className ?? ""}`}
      {...rest}
    >
      <Icon size={16} fill={filled ? "currentColor" : "none"} aria-hidden="true" />
    </button>
  );
}
