import { useEffect, useRef, useState } from "react";
import { errorMessage, type GlyphSetInfo } from "../lib/backend";
import { CloseIcon } from "./Icons";
import { t, tn } from "../lib/i18n";

interface Props {
  sets: GlyphSetInfo[];
  onCancel: () => void;
  onCreate: (familyName: string, sets: string[]) => Promise<void>;
}

/** S02 · New Font — family name and starting glyph sets. */
export function NewFontDialog({ sets, onCancel, onCreate }: Props) {
  const [name, setName] = useState("Untitled Sans");
  const [chosen, setChosen] = useState<string[]>(["latin-basic"]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.select();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  const total = sets.filter((s) => chosen.includes(s.id)).reduce((n, s) => n + s.count, 0);
  const toggle = (id: string) => setChosen((c) => (c.includes(id) ? c.filter((x) => x !== id) : [...c, id]));

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim()) return setError(t("Enter a family name."));
    setBusy(true);
    try {
      await onCreate(name.trim(), chosen);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  return (
    <div className="scrim" role="presentation">
      <form className="dialog" role="dialog" aria-modal="true" aria-labelledby="new-font-title" onSubmit={submit}>
        <header className="dialog-header">
          <h2 id="new-font-title" className="k-subtitle">{t("New Font")}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onCancel}>
            <CloseIcon />
          </button>
        </header>
        <div className="dialog-body">
          <label className="field">
            <span className="k-label">{t("Family name")}</span>
            <input ref={inputRef} value={name} onChange={(e) => { setName(e.target.value); setError(null); }} />
          </label>
          <fieldset className="field">
            <legend className="k-overline muted">{t("Start with glyph sets")}</legend>
            {sets.map((s) => (
              <label key={s.id} className="check-row">
                <input type="checkbox" checked={chosen.includes(s.id)} onChange={() => toggle(s.id)} />
                <span className="k-body">{t(s.name)}</span>
                <span className="spacer" />
                <span className="k-caption muted">{tn(s.count, "{n} glyph", "{n} glyphs")}</span>
              </label>
            ))}
          </fieldset>
          {error && <p className="notice notice-danger k-label">{error}</p>}
        </div>
        <footer className="dialog-footer">
          <span className="k-caption muted">{tn(total, "Total: {n} glyph · 1 master (Regular) · 1000 UPM", "Total: {n} glyphs · 1 master (Regular) · 1000 UPM")}</span>
          <span className="spacer" />
          <button type="button" className="btn btn-secondary" onClick={onCancel}>{t("Cancel")}</button>
          <button type="submit" className="btn btn-primary" disabled={busy}>{t("Create Font")}</button>
        </footer>
      </form>
    </div>
  );
}
