import { useEffect, useState } from "react";
import { errorMessage, type FeatureImportPreview } from "../lib/backend";
import { CloseIcon } from "./Icons";
import { t, translateMessage } from "../lib/i18n";

interface Props {
  path: string;
  preview: FeatureImportPreview;
  onCancel: () => void;
  onApply: () => Promise<void>;
}

/**
 * Import feature code from a .fea file. Lists what is new and what replaces
 * existing features, plus any problems in the merged code, before applying.
 */
export function FeatureImportDialog({ path, preview, onCancel, onApply }: Props) {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setBusy(true);
    try {
      await onApply();
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  const name = path.split(/[\\/]/).pop() ?? path;
  const nothing = preview.added.length === 0 && preview.replaced.length === 0;

  return (
    <div className="scrim" role="presentation">
      <form className="dialog dialog-sm" role="dialog" aria-modal="true" aria-labelledby="fea-title" onSubmit={submit}>
        <header className="dialog-header">
          <h2 id="fea-title" className="k-subtitle">{t("Import Features")}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onCancel}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          <p className="k-caption muted">{name}</p>
          {nothing ? (
            <p className="k-label">{t("No feature tags are added or replaced.")}</p>
          ) : (
            <div className="svg-placements">
              <p className="k-label">
                {t("New features")}: <span className="muted">{preview.added.length > 0 ? preview.added.join(", ") : t("None")}</span>
              </p>
              <p className="k-label">
                {t("Replaces existing features")}: <span className="muted">{preview.replaced.length > 0 ? preview.replaced.join(", ") : t("None")}</span>
              </p>
            </div>
          )}
          {preview.diagnostics.length > 0 && (
            <div className="notice notice-danger k-label">
              <p>{t("The merged code has problems:")}</p>
              <ul className="fea-import-problems">
                {preview.diagnostics.map((d, i) => (
                  <li key={i}>{t("Line {line}: {message}", { line: d.line, message: translateMessage(d.message) })}</li>
                ))}
              </ul>
            </div>
          )}
          {error && <p className="notice notice-danger k-label">{error}</p>}
        </div>
        <footer className="dialog-footer">
          <span className="spacer" />
          <button type="button" className="btn btn-secondary" onClick={onCancel}>{t("Cancel")}</button>
          <button type="submit" className="btn btn-primary" disabled={busy}>{t("Import")}</button>
        </footer>
      </form>
    </div>
  );
}