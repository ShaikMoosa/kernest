import { useEffect, useState } from "react";
import { errorMessage, type ClipGlyph, type PasteMode } from "../lib/backend";
import { readGlyphClipboard } from "../lib/glyphClipboard";
import { CloseIcon } from "./Icons";
import { t, tn } from "../lib/i18n";

interface Props {
  /** The glyphs a selection or components paste lands in. */
  targets: string[];
  onCancel: () => void;
  onPaste: (mode: PasteMode, glyphs: ClipGlyph[]) => Promise<void>;
}

const MODES: { id: PasteMode; label: string; hint: string }[] = [
  { id: "same-name", label: "Same-named glyphs", hint: "Replaces the layers of glyphs with the same names. Names not in this font are ignored." },
  { id: "selection", label: "Into the selection", hint: "Pastes the copied glyphs into the selected glyphs, in order. Names and unicodes stay as they are." },
  { id: "components", label: "As components", hint: "Adds the copied glyphs as components in each selected glyph. Glyphs missing from this font are added first." },
];

/** Edit › Paste Special… — paste copied glyphs by name, into the selection, or as components. */
export function PasteSpecialDialog({ targets, onCancel, onPaste }: Props) {
  const [clip, setClip] = useState<ClipGlyph[] | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [mode, setMode] = useState<PasteMode>("same-name");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    readGlyphClipboard().then((g) => { setClip(g); setLoaded(true); });
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  const needsTargets = mode !== "same-name";
  const canPaste = !!clip && !busy && (!needsTargets || targets.length > 0);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!clip || !canPaste) return;
    setBusy(true);
    try {
      await onPaste(mode, clip);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  return (
    <div className="scrim" role="presentation">
      <form className="dialog dialog-sm" role="dialog" aria-modal="true" aria-labelledby="paste-special-title" onSubmit={submit}>
        <header className="dialog-header">
          <h2 id="paste-special-title" className="k-subtitle">{t("Paste Special")}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onCancel}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          {loaded && (clip ? (
            <>
              <p className="k-label">{tn(clip.length, "{n} glyph on the clipboard", "{n} glyphs on the clipboard")}</p>
              <p className="k-caption muted paste-clip-names">{clip.map((g) => g.name).join(" ")}</p>
              <div className="paste-modes" role="radiogroup" aria-label={t("Paste Special")}>
                {MODES.map((m) => (
                  <label key={m.id} className="check-row">
                    <input type="radio" name="paste-mode" checked={mode === m.id}
                      disabled={m.id !== "same-name" && targets.length === 0}
                      onChange={() => { setMode(m.id); setError(null); }} />
                    <span className="k-label">
                      {t(m.label)}
                      <span className="muted k-caption"> — {t(m.hint)}</span>
                    </span>
                  </label>
                ))}
              </div>
              {needsTargets && targets.length === 0 && <p className="k-caption muted">{t("Select glyphs in Font View first.")}</p>}
            </>
          ) : (
            <p className="notice k-label">{t("The clipboard has no copied glyphs. Use Edit › Copy Glyphs first.")}</p>
          ))}
          {error && <p className="notice notice-danger k-label">{error}</p>}
        </div>
        <footer className="dialog-footer">
          <span className="spacer" />
          <button type="button" className="btn btn-secondary" onClick={onCancel}>{t("Cancel")}</button>
          <button type="submit" className="btn btn-primary" disabled={!canPaste}>{t("Paste")}</button>
        </footer>
      </form>
    </div>
  );
}