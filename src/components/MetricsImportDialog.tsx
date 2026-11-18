import { useEffect, useState } from "react";
import { errorMessage, type KeepSide, type MetricsSourceInfo } from "../lib/backend";
import { CloseIcon } from "./Icons";
import { t, tn } from "../lib/i18n";

interface Props {
  path: string;
  info: MetricsSourceInfo;
  onCancel: () => void;
  onApply: (sourceMasterId: string, keep: KeepSide, widths: boolean, kerning: boolean) => Promise<void>;
}

const STORAGE_KEY = "kernest.metricsKeep";

/**
 * S24 · Import widths, sidebearings and kerning from an AFM file or another font.
 * The chosen sidebearing stays fixed while the imported width is applied.
 */
export function MetricsImportDialog({ path, info, onCancel, onApply }: Props) {
  const [sourceMasterId, setSourceMasterId] = useState(info.masters[0]?.id ?? "");
  const [keep, setKeep] = useState<KeepSide>(() => (localStorage.getItem(STORAGE_KEY) === "right" ? "right" : "left"));
  const [widths, setWidths] = useState(true);
  const [kerning, setKerning] = useState(info.kerningPairs > 0);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!widths && !kerning) return setError(t("Choose what to import."));
    setError(null);
    setBusy(true);
    try {
      localStorage.setItem(STORAGE_KEY, keep);
      await onApply(sourceMasterId, keep, widths, kerning);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  const name = path.split(/[\\/]/).pop() ?? path;

  return (
    <div className="scrim" role="presentation">
      <form className="dialog dialog-sm" role="dialog" aria-modal="true" aria-labelledby="metrics-title" onSubmit={submit}>
        <header className="dialog-header">
          <h2 id="metrics-title" className="k-subtitle">{t("Import Metrics")}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onCancel}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          <p className="k-caption muted">
            {name} · {tn(info.glyphs, "{n} glyph", "{n} glyphs")} · {tn(info.kerningPairs, "{n} kerning pair", "{n} kerning pairs")}
          </p>
          {info.kind === "font" && info.masters.length > 0 && (
            <label className="field">
              <span className="k-label">{t("Source master")}</span>
              <select className="select" value={sourceMasterId} aria-label={t("Source master")} onChange={(e) => setSourceMasterId(e.target.value)}>
                {info.masters.map((m) => <option key={m.id} value={m.id}>{m.name}</option>)}
              </select>
            </label>
          )}
          <div className="svg-placements">
            <label className="check-row">
              <input type="checkbox" checked={widths} onChange={(e) => setWidths(e.target.checked)} />
              <span className="k-label">{t("Widths and sidebearings")}</span>
            </label>
            <label className="check-row">
              <input type="checkbox" checked={kerning} onChange={(e) => setKerning(e.target.checked)} />
              <span className="k-label">{t("Kerning")}</span>
            </label>
          </div>
          {widths && (
            <div className="svg-placements" role="radiogroup" aria-label={t("Keep sidebearing")}>
              <span className="k-label">{t("Keep sidebearing")}</span>
              <label className="check-row">
                <input type="radio" name="keep-side" checked={keep === "left"} onChange={() => setKeep("left")} />
                <span className="k-label">{t("Left")}<span className="muted k-caption"> — {t("The outline slides to keep the source's left sidebearing.")}</span></span>
              </label>
              <label className="check-row">
                <input type="radio" name="keep-side" checked={keep === "right"} onChange={() => setKeep("right")} />
                <span className="k-label">{t("Right")}<span className="muted k-caption"> — {t("The outline slides to keep the source's right sidebearing.")}</span></span>
              </label>
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