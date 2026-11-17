import { useEffect, useRef, useState } from "react";
import { errorMessage } from "../lib/backend";
import { CloseIcon } from "./Icons";
import { t } from "../lib/i18n";

interface Props {
  /** Names to start with (the current selection, if any). */
  initial: string;
  onCancel: () => void;
  onCreate: (names: string[]) => Promise<void>;
}

/** Glyph › Create Composites… — build glyphs from their parts, e.g. Aacute = A + acutecomb. */
export function CompositesDialog({ initial, onCancel, onCreate }: Props) {
  const [text, setText] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    ref.current?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  async function submit(e?: React.FormEvent) {
    e?.preventDefault();
    const names = text.split(/[\s,]+/).filter(Boolean);
    if (names.length === 0) return setError(t("Type the glyph names to build."));
    setBusy(true);
    try {
      await onCreate(names);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  return (
    <div className="scrim" role="presentation">
      <form className="dialog dialog-sm" role="dialog" aria-modal="true" aria-labelledby="composites-title" onSubmit={submit}>
        <header className="dialog-header">
          <h2 id="composites-title" className="k-subtitle">{t("Create Composites")}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onCancel}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          <label className="field">
            <span className="k-label">{t("Glyph names")}</span>
            <textarea
              ref={ref}
              className="text-input add-glyphs-text"
              value={text}
              rows={3}
              spellCheck={false}
              placeholder="Aacute Acircumflex AEacute"
              onChange={(e) => { setText(e.target.value); setError(null); }}
              onKeyDown={(e) => { if (e.key === "Enter" && e.ctrlKey) submit(); }}
            />
            <span className="k-caption muted">{t("Each glyph is built from its parts: Aacute from A and acutecomb. The parts must already be in the font.")}</span>
          </label>
          {error && <p className="notice notice-danger k-label">{error}</p>}
        </div>
        <footer className="dialog-footer">
          <span className="k-caption muted">{t("Glyphs that already have outlines are left alone.")}</span>
          <span className="spacer" />
          <button type="button" className="btn btn-secondary" onClick={onCancel}>{t("Cancel")}</button>
          <button type="submit" className="btn btn-primary" disabled={busy}>{t("Create Composites")}</button>
        </footer>
      </form>
    </div>
  );
}