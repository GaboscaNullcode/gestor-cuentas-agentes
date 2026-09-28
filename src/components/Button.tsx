// Canvas: canvas/usage-monitor/components/Button.dc.html
import type { LucideIcon } from "lucide-react";
import type { ComponentProps } from "react";

interface Props extends ComponentProps<"button"> {
  variant?: "primary" | "secondary" | "ghost" | "danger";
  size?: "md" | "sm";
  icon?: LucideIcon;
}

export function Button({ variant = "primary", size = "md", icon: Icon, className, children, ...rest }: Props) {
  return (
    <button type="button" className={`btn ${variant} ${size} ${className ?? ""}`} {...rest}>
      {Icon && <Icon size={14} aria-hidden="true" />}
      <span>{children}</span>
    </button>
  );
}
