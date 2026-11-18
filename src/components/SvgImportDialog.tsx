import { useEffect, useState } from "react";
import { errorMessage, type SvgPlacement } from "../lib/backend";
import { CloseIcon } from "./Icons";
import { t, tn } from "../lib/i18n";

interface Props {
  mode: "files" | "paste";
  /** The SVG files chosen for import (files mode). */
  files?: string[];
  /** The glyph the pasted SVG lands in (paste mode). */
  glyph?: string;
  onCancel: () => void;
  onApply: (placement: SvgPlacement, svgText: string | null) => Promise<void>;
}

const PLACEMENTS: { id: SvgPlacement; label: string; hint: string }[] = [
  { id: "fit-cap-height", label: "Fit cap height", hint: "Scales the drawing to the master's cap height and sits it on the baseline." },
  { id: "keep-size", label: "Keep size at origin", hint: "Keeps the drawing's size and moves its bottom-left corner to the origin." },
  { id: "keep-as-is", label: "Keep as is", hint: "Only flips the vertical axis; the coordinates stay as they are." },
];

const STORAGE_KEY = "kernest.svgPlacement";

/**
 * S23 · Choose how imported SVG outlines are placed. Files mode imports one file per
 * glyph (matched by name); paste mode imports SVG text copied from a vector editor.
 */
export function SvgImportDialog({ mode, files = [], glyph, onCancel, onApply }: Props) {
  const [placement, setPlacement] = useState<SvgPlacement>(() => {
    const saved = localStorage.getItem(STORAGE_KEY);
    return PLACEMENTS.some((p) => p.id === saved) ? (saved as SvgPlacement) : "fit-cap-height";
  });
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (mode === "paste" && text.trim() === "") return setError(t("Paste the SVG text first."));
    setError(null);
    setBusy(true);
    try {
      localStorage.setItem(STORAGE_KEY, placement);
      await onApply(placement, mode === "paste" ? text : null);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  const title = mode === "files" ? t("Import SVG outlines") : t("Paste SVG");

  return (
    <div className="scrim" role="presentation">
      <form className="dialog dialog-sm" role="dialog" aria-modal="true" aria-labelledby="svg-title" onSubmit={submit}>
        <header className="dialog-header">
          <h2 id="svg-title" className="k-subtitle">{mode === "paste" && glyph ? `${title} · ${glyph}` : title}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onCancel}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          {mode === "files" ? (
            <p className="k-caption muted">
              {tn(files.length, "{n} SVG file selected.", "{n} SVG files selected.")}{" "}
              {t("Each file is matched to a glyph by its name; files with no matching glyph are left out.")}
            </p>
          ) : (
            <label className="field">
              <span className="k-label">{t("SVG text")}</span>
              <textarea className="text-input svg-paste" rows={8} value={text} aria-label={t("SVG text")}
                placeholder={t("Paste SVG copied from Figma, Illustrator or another vector editor.")}
                onChange={(e) => setText(e.target.value)} />
            </label>
          )}
          <div className="svg-placements" role="radiogroup" aria-label={t("Placement")}>
            {PLACEMENTS.map((p) => (
              <label key={p.id} className="check-row">
                <input type="radio" name="svg-placement" checked={placement === p.id} onChange={() => setPlacement(p.id)} />
                <span className="k-label">
                  {t(p.label)}
                  <span className="muted k-caption"> — {t(p.hint)}</span>
                </span>
              </label>
            ))}
          </div>
          <p className="k-caption muted">{t("Strokes, text and images are left out; convert them to filled outlines in your vector editor first.")}</p>
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