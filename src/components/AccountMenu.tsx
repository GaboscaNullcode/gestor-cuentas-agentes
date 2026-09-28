// Canvas: canvas/usage-monitor/overlays/AccountMenu.dc.html
import { Check, Folder, Pencil, Terminal, Trash2 } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type KeyboardEvent, type RefObject } from "react";
import { createPortal } from "react-dom";

const GAP = 4;
const EDGE = 8;

interface Props {
  anchor: RefObject<HTMLButtonElement | null>;
  aliasName: string;
  onRename: () => void;
  /** Resolves true once the alias line is on the clipboard. */
  onCopyAlias: () => Promise<boolean>;
  onShowFolder: () => void;
  onRemove: () => void;
  onClose: () => void;
}

export function AccountMenu({ anchor, aliasName, onRename, onCopyAlias, onShowFolder, onRemove, onClose }: Props) {
  const menu = useRef<HTMLDivElement>(null);
  const [style, setStyle] = useState<CSSProperties>({ visibility: "hidden" });
  const [copied, setCopied] = useState(false);

  // Right-aligned under the anchor; flips above it when the window has no room below.
  useLayoutEffect(() => {
    const rect = anchor.current?.getBoundingClientRect();
    const height = menu.current?.offsetHeight ?? 0;
    if (!rect) return;
    const right = window.innerWidth - rect.right;
    const fitsBelow = rect.bottom + GAP + height <= window.innerHeight - EDGE;
    setStyle(fitsBelow ? { top: rect.bottom + GAP, right } : { bottom: window.innerHeight - rect.top + GAP, right });
  }, [anchor]);

  const placed = style.visibility !== "hidden";
  useEffect(() => {
    if (placed) items()[0]?.focus();
  }, [placed]);

  useEffect(() => {
    const onPointer = (e: PointerEvent) => {
      const target = e.target as Node;
      if (!menu.current?.contains(target) && !anchor.current?.contains(target)) onClose();
    };
    // The menu is fixed to where the anchor was; scrolling the list would detach it.
    const onScroll = (e: Event) => {
      if (!menu.current?.contains(e.target as Node)) onClose();
    };
    document.addEventListener("pointerdown", onPointer);
    document.addEventListener("scroll", onScroll, true);
    return () => {
      document.removeEventListener("pointerdown", onPointer);
      document.removeEventListener("scroll", onScroll, true);
    };
  }, [anchor, onClose]);

  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(timer);
  }, [copied]);

  function items(): HTMLButtonElement[] {
    return Array.from(menu.current?.querySelectorAll<HTMLButtonElement>("[role=menuitem]") ?? []);
  }

  function onKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
      anchor.current?.focus();
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const list = items();
    const index = list.indexOf(document.activeElement as HTMLButtonElement);
    const step = e.key === "ArrowDown" ? 1 : -1;
    list[(index + step + list.length) % list.length]?.focus();
  }

  async function copy() {
    if (await onCopyAlias()) setCopied(true);
  }

  return createPortal(
    <div ref={menu} role="menu" aria-label="Account actions" className="menu" style={style} onKeyDown={onKeyDown}>
      <button type="button" role="menuitem" className="menu-item" onClick={onRename}>
        <Pencil size={15} aria-hidden="true" />
        <span>Rename</span>
      </button>
      <button type="button" role="menuitem" className="menu-item" onClick={copy}>
        {copied ? <Check size={15} aria-hidden="true" /> : <Terminal size={15} aria-hidden="true" />}
        <span className="grow">{copied ? "Copied" : "Copy alias"}</span>
        <span className="menu-item-hint">{aliasName}</span>
      </button>
      <button type="button" role="menuitem" className="menu-item" onClick={onShowFolder}>
        <Folder size={15} aria-hidden="true" />
        <span>Show config folder</span>
      </button>
      <div className="menu-divider" role="separator" />
      <button type="button" role="menuitem" className="menu-item danger" onClick={onRemove}>
        <Trash2 size={15} aria-hidden="true" />
        <span>Remove account…</span>
      </button>
    </div>,
    document.body,
  );
}
