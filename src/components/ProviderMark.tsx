// Canvas: canvas/usage-monitor/components/ProviderMark.dc.html
import claudeSvg from "../assets/providers/claude.svg?raw";
import codexSvg from "../assets/providers/codex.svg?raw";
import { providerName } from "../format";
import type { Provider } from "../types";

const MARKS: Record<Provider, string> = { claude: claudeSvg, codex: codexSvg };

interface Props {
  provider: Provider;
  size?: "sm" | "md" | "lg";
}

/** Provider logo on a tinted tile. The SVGs are bundled assets drawn at 1em in currentColor. */
export function ProviderMark({ provider, size = "md" }: Props) {
  return (
    <span
      className={`provider-mark ${provider} ${size}`}
      title={providerName(provider)}
      aria-hidden="true"
      dangerouslySetInnerHTML={{ __html: MARKS[provider] }}
    />
  );
}
