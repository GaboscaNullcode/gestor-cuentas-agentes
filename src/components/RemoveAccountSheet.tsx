// Canvas: canvas/usage-monitor/overlays/RemoveAccount.dc.html
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { providerName } from "../format";
import type { AccountView } from "../types";
import { Button } from "./Button";
import { Checkbox } from "./Checkbox";
import { ProviderMark } from "./ProviderMark";

interface Props {
  account: AccountView;
  onCancel: () => void;
  onRemove: (logout: boolean, deleteDir: boolean) => Promise<unknown>;
}

export function RemoveAccountSheet({ account, onCancel, onRemove }: Props) {
  const [logout, setLogout] = useState(false);
  const [deleteDir, setDeleteDir] = useState(false);
  const [busy, setBusy] = useState(false);
  const cancel = useRef<HTMLButtonElement>(null);
  const deleting = account.canDeleteDir && deleteDir;

  useEffect(() => {
    cancel.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [onCancel]);

  async function remove() {
    setBusy(true);
    await onRemove(logout, deleting);
    setBusy(false);
  }

  return createPortal(
    <div className="scrim" onClick={onCancel}>
      <div
        className="sheet"
        role="dialog"
        aria-modal="true"
        aria-labelledby="remove-title"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="sheet-head">
          <ProviderMark provider={account.provider} size="lg" />
          <div className="stack-4">
            <span id="remove-title" className="sheet-title">
              Remove {account.label}?
            </span>
            <span className="sheet-text">
              Usage Monitor stops tracking it. Nothing else changes unless you check an option.
            </span>
          </div>
        </div>
        <div className="sheet-options">
          <Checkbox checked={logout} onChange={setLogout}>
            Also sign out of the {providerName(account.provider)} CLI
          </Checkbox>
          {account.canDeleteDir && (
            <Checkbox checked={deleteDir} onChange={setDeleteDir}>
              <span className="stack-3">
                <span>Also delete the config folder</span>
                <span className="mono-path">{account.configDir}</span>
              </span>
            </Checkbox>
          )}
        </div>
        <div className="sheet-actions">
          <Button ref={cancel} variant="secondary" onClick={onCancel}>
            Cancel
          </Button>
          <Button variant="danger" disabled={busy} onClick={remove}>
            {deleting ? "Remove and delete" : "Remove"}
          </Button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
