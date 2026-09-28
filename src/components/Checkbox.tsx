// Canvas: canvas/usage-monitor/components/Checkbox.dc.html
import { Check } from "lucide-react";
import type { ReactNode } from "react";

interface Props {
  checked: boolean;
  onChange: (checked: boolean) => void;
  /** The label; the whole row is the hit target. */
  children: ReactNode;
}

export function Checkbox({ checked, onChange, children }: Props) {
  return (
    <label className="checkbox">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span className="box" aria-hidden="true">
        {checked && <Check size={12} strokeWidth={3} />}
      </span>
      <span className="checkbox-label">{children}</span>
    </label>
  );
}
