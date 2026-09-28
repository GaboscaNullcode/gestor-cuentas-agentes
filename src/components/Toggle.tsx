// Canvas: canvas/usage-monitor/components/Toggle.dc.html
interface Props {
  on: boolean;
  label: string;
  disabled?: boolean;
  onChange: (on: boolean) => void;
}

export function Toggle({ on, label, disabled, onChange }: Props) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-label={label}
      disabled={disabled}
      className={`toggle ${on ? "on" : ""}`}
      onClick={() => onChange(!on)}
    >
      <span className="knob" aria-hidden="true" />
    </button>
  );
}
