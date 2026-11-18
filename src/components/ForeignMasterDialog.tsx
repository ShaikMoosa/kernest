import { useEffect, useState } from "react";
import { errorMessage, type Axis, type ForeignMasterPreview, type Location } from "../lib/backend";
import { designBounds } from "../lib/axisMap";
import { CloseIcon } from "./Icons";
import { t, tn, translateMessage } from "../lib/i18n";

interface Selection {
  masterId: string;
  name: string;
  location: Location;
}

interface Props {
  path: string;
  previews: ForeignMasterPreview[];
  /** The current font's axes; the new masters are placed in this design space. */
  axes: Axis[];
  onCancel: () => void;
  onAdd: (selection: Selection[]) => Promise<void>;
}

/** The starting location for an incoming master: its own coordinate on a shared axis, else the axis maximum. */
function startLocation(preview: ForeignMasterPreview, axes: Axis[]): Record<string, string> {
  const out: Record<string, string> = {};
  for (const a of axes) {
    const [min, , max] = designBounds(a);
    const raw = preview.location[a.tag];
    const v = raw === undefined ? max : Math.min(max, Math.max(min, raw));
    out[a.tag] = String(v);
  }
  return out;
}

/**
 * S25 · Add masters from another font. Each master shows its compatibility with this
 * font before adding: how many glyphs line up, which are missing (they become empty
 * layers) and which won't interpolate. The chosen masters are placed on this font's axes.
 */
export function ForeignMasterDialog({ path, previews, axes, onCancel, onAdd }: Props) {
  const [sel, setSel] = useState<Record<string, { name: string; loc: Record<string, string> }>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  const toggle = (p: ForeignMasterPreview) => {
    setSel((s) => {
      const next = { ...s };
      if (next[p.masterId]) delete next[p.masterId];
      else next[p.masterId] = { name: p.name, loc: startLocation(p, axes) };
      return next;
    });
  };
  const patch = (id: string, part: Partial<{ name: string; loc: Record<string, string> }>) =>
    setSel((s) => ({ ...s, [id]: { ...s[id], ...part } }));

  const count = Object.keys(sel).length;
  const name = path.split(/[\\/]/).pop() ?? path;

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (count === 0) return setError(t("Choose at least one master."));
    const selection: Selection[] = [];
    for (const [masterId, s] of Object.entries(sel)) {
      if (!s.name.trim()) return setError(t("Enter a master name."));
      const location: Location = {};
      for (const a of axes) {
        const v = Number(s.loc[a.tag]);
        if (!Number.isFinite(v)) return setError(t("{name} must be a number.", { name: a.name }));
        location[a.tag] = v;
      }
      selection.push({ masterId, name: s.name.trim(), location });
    }
    setError(null);
    setBusy(true);
    try {
      await onAdd(selection);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  return (
    <div className="scrim" role="presentation">
      <form className="dialog dialog-sm" role="dialog" aria-modal="true" aria-labelledby="foreign-title" onSubmit={submit}>
        <header className="dialog-header">
          <h2 id="foreign-title" className="k-subtitle">{t("Add Masters from Font")}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onCancel}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          <p className="k-caption muted">{name}</p>
          {previews.length === 0 && <p className="k-caption muted">{t("That font has no masters.")}</p>}
          {previews.map((p) => {
            const chosen = sel[p.masterId];
            return (
              <div key={p.masterId} className={`fm-master${chosen ? " fm-chosen" : ""}`}>
                <label className="check-row fm-head">
                  <input type="checkbox" checked={!!chosen} onChange={() => toggle(p)} />
                  <span className="k-label fm-name">{p.name}</span>
                  <span className="spacer" />
                  <span className="k-caption muted">
                    {tn(p.glyphCount, "{n} glyph", "{n} glyphs")} · {tn(p.matched, "{n} match", "{n} matches")}
                  </span>
                </label>
                {p.missing.length > 0 && (
                  <p className="notice notice-warning k-label fm-note">
                    {tn(p.missing.length, "{n} glyph is missing and becomes an empty layer:", "{n} glyphs are missing and become empty layers:")} {p.missing.join(", ")}
                  </p>
                )}
                {p.incompatible.length > 0 && (
                  <details className="fm-issues">
                    <summary className="k-label notice notice-warning">
                      {tn(p.incompatible.length, "{n} glyph can't be interpolated", "{n} glyphs can't be interpolated")}
                    </summary>
                    <ul className="fm-issue-list">
                      {p.incompatible.map((i) => (
                        <li key={i.glyph} className="k-caption">
                          <strong>{i.glyph}</strong>: {i.issues.map((s) => translateMessage(s)).join("; ")}
                        </li>
                      ))}
                    </ul>
                  </details>
                )}
                {chosen && (
                  <div className="fm-fields">
                    <label className="field">
                      <span className="k-label">{t("New master name")}</span>
                      <input className="metric-input text-field" value={chosen.name} aria-label={t("New master name")}
                        onChange={(e) => patch(p.masterId, { name: e.target.value })} />
                    </label>
                    {axes.map((a) => (
                      <label key={a.tag} className="field">
                        <span className="k-label">{a.tag}</span>
                        <input className="metric-input" type="number" value={chosen.loc[a.tag] ?? ""} aria-label={`${p.name} ${a.name}`}
                          onChange={(e) => patch(p.masterId, { loc: { ...chosen.loc, [a.tag]: e.target.value } })} />
                      </label>
                    ))}
                    {axes.length === 0 && <p className="k-caption muted">{t("Tip: add an axis first so masters can sit at different locations.")}</p>}
                  </div>
                )}
              </div>
            );
          })}
          {error && <p className="notice notice-danger k-label">{error}</p>}
        </div>
        <footer className="dialog-footer">
          <span className="spacer" />
          <button type="button" className="btn btn-secondary" onClick={onCancel}>{t("Cancel")}</button>
          <button type="submit" className="btn btn-primary" disabled={busy || count === 0}>
            {count > 0 ? tn(count, "Add {n} master", "Add {n} masters") : t("Add")}
          </button>
        </footer>
      </form>
    </div>
  );
}