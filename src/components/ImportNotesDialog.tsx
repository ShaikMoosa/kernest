import { useEffect } from "react";
import { CloseIcon } from "./Icons";
import { t, translateMessage } from "../lib/i18n";

/** Shown after an import that left something out, so nothing disappears silently. */
export function ImportNotesDialog({ source, familyName, warnings, onClose }: { source: string; familyName: string; warnings: string[]; onClose: () => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => (e.key === "Escape" || e.key === "Enter") && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const file = source.split(/[\\/]/).pop() ?? source;
  return (
    <div className="scrim" role="presentation">
      <div className="dialog import-dialog" role="dialog" aria-modal="true" aria-labelledby="import-title">
        <header className="dialog-header">
          <h2 id="import-title" className="k-subtitle">{t("Imported {name}", { name: familyName })}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onClose}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          <p className="k-body">
            {t("{file} was converted to a Kernest font. A few things couldn’t be brought over:", { file })}
          </p>
          {warnings.map((w) => <p key={w} className="notice notice-warning k-label">⚠ {translateMessage(w)}</p>)}
          <p className="k-caption muted">{t("The original file is unchanged. Use Save As to keep this font as a .kernest font.")}</p>
        </div>
        <footer className="dialog-footer">
          <span className="spacer" />
          <button type="button" className="btn btn-primary" onClick={onClose} autoFocus>{t("OK")}</button>
        </footer>
      </div>
    </div>
  );
}
