import { useEffect, useRef, useState } from "react";
import { errorMessage, type GlyphSetInfo } from "../lib/backend";
import { CloseIcon } from "./Icons";
import { t, tn } from "../lib/i18n";

interface Props {
  sets: GlyphSetInfo[];
  onCancel: () => void;
  onAdd: (text: string, sets: string[], recipes: boolean) => Promise<void>;
}

/** Glyph › Add Glyphs… — names, characters or code points, and whole glyph sets. */
export function AddGlyphsDialog({ sets, onCancel, onAdd }: Props) {
  const [text, setText] = useState("");
  const [chosen, setChosen] = useState<string[]>([]);
  const [recipes, setRecipes] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    ref.current?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  const toggle = (id: string) => setChosen((c) => (c.includes(id) ? c.filter((x) => x !== id) : [...c, id]));

  async function submit(e?: React.FormEvent) {
    e?.preventDefault();
    if (!text.trim() && chosen.length === 0) return setError(t("Type glyph names or characters, or choose a glyph set."));
    setBusy(true);
    try {
      await onAdd(text, chosen, recipes);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  return (
    <div className="scrim" role="presentation">
      <form className="dialog add-glyphs-dialog" role="dialog" aria-modal="true" aria-labelledby="add-glyphs-title" onSubmit={submit}>
        <header className="dialog-header">
          <h2 id="add-glyphs-title" className="k-subtitle">{t("Add Glyphs")}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onCancel}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          <label className="field">
            <span className="k-label">{t("Glyph names, characters or code points")}</span>
            <textarea
              ref={ref}
              className="text-input add-glyphs-text"
              value={text}
              rows={4}
              spellCheck={false}
              placeholder="A.alt  a.sc  Ж  U+20AC  uni0416"
              onChange={(e) => { setText(e.target.value); setError(null); }}
              onKeyDown={(e) => { if (e.key === "Enter" && e.ctrlKey) submit(); }}
            />
            <span className="k-caption muted">{t("Separate with spaces, commas or new lines. Glyphs already in the font are skipped.")}</span>
          </label>
          <fieldset className="field">
            <legend className="k-overline muted">{t("Add glyph sets")}</legend>
            {sets.map((s) => (
              <label key={s.id} className="check-row">
                <input type="checkbox" checked={chosen.includes(s.id)} onChange={() => { toggle(s.id); setError(null); }} />
                <span className="k-body">{t(s.name)}</span>
                <span className="spacer" />
                <span className="k-caption muted">{tn(s.count, "{n} glyph", "{n} glyphs")}</span>
              </label>
            ))}
          </fieldset>
          <label className="check-row">
            <input type="checkbox" checked={recipes} onChange={(e) => setRecipes(e.target.checked)} />
            <span className="k-body">{t("Build composites from their parts")}</span>
          </label>
          <p className="k-caption muted">{t("Aacute becomes A + acutecomb, placed by anchors, when both parts are in the font.")}</p>
          {error && <p className="notice notice-danger k-label">{error}</p>}
        </div>
        <footer className="dialog-footer">
          <span className="k-caption muted">{t("New glyphs get a layer in every master.")}</span>
          <span className="spacer" />
          <button type="button" className="btn btn-secondary" onClick={onCancel}>{t("Cancel")}</button>
          <button type="submit" className="btn btn-primary" disabled={busy}>{t("Add Glyphs")}</button>
        </footer>
      </form>
    </div>
  );
}
