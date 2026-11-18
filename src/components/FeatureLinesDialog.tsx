import { useEffect } from "react";
import { CloseIcon } from "./Icons";
import { t, tn } from "../lib/i18n";

/** Shown after a rename rewrote feature code, so the change never passes silently. */
export function FeatureLinesDialog({ lines, onClose }: { lines: string[]; onClose: () => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => (e.key === "Escape" || e.key === "Enter") && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="scrim" role="presentation">
      <div className="dialog feature-lines-dialog" role="dialog" aria-modal="true" aria-labelledby="feature-lines-title">
        <header className="dialog-header">
          <h2 id="feature-lines-title" className="k-subtitle">{t("Feature code changed")}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onClose}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          <p className="k-body">{tn(lines.length, "The rename rewrote {n} line of feature code. Check it in the Features editor:", "The rename rewrote {n} lines of feature code. Check them in the Features editor:")}</p>
          <pre className="feature-lines" dir="ltr">{lines.join("\n")}</pre>
        </div>
        <footer className="dialog-footer">
          <span className="spacer" />
          <button type="button" className="btn btn-primary" onClick={onClose} autoFocus>{t("OK")}</button>
        </footer>
      </div>
    </div>
  );
}