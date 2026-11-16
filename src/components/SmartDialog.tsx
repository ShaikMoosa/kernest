import { useEffect, useState } from "react";
import { errorMessage, type SmartAxis } from "../lib/backend";
import { AddIcon, CloseIcon } from "./Icons";
import { t } from "../lib/i18n";

export type SmartDialogMode =
  | { kind: "axes"; axes: SmartAxis[] }
  | { kind: "pole"; axes: SmartAxis[] };

interface Props {
  glyph: string;
  mode: SmartDialogMode;
  onCancel: () => void;
  /** Resolves when saved; rejects with a message to show. */
  onSave: (value: { axes?: SmartAxis[]; pole?: Record<string, number> }) => Promise<void>;
}

/**
 * Smart glyphs: edit a glyph's smart axes (such as Width 0–100), or add a pole layer
 * at a position on them.
 */
export function SmartDialog({ glyph, mode, onCancel, onSave }: Props) {
  const [rows, setRows] = useState(() =>
    mode.axes.length
      ? mode.axes.map((a) => ({ name: a.name, min: String(a.min), max: String(a.max) }))
      : [{ name: t("Width"), min: "0", max: "100" }]
  );
  const [pole, setPole] = useState<Record<string, string>>(() =>
    Object.fromEntries(mode.axes.map((a) => [a.name, String(a.max)]))
  );
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
    let value: { axes?: SmartAxis[]; pole?: Record<string, number> };
    if (mode.kind === "axes") {
      const axes: SmartAxis[] = [];
      for (const r of rows) {
        const min = Number(r.min), max = Number(r.max);
        if (!r.name.trim()) return setError(t("Every axis needs a name."));
        if (!Number.isFinite(min) || !Number.isFinite(max)) return setError(t("{name}: enter numbers.", { name: r.name }));
        axes.push({ name: r.name.trim(), min, max });
      }
      value = { axes };
    } else {
      const out: Record<string, number> = {};
      for (const a of mode.axes) {
        const v = Number(pole[a.name]);
        if (!Number.isFinite(v)) return setError(t("{name}: enter a number.", { name: a.name }));
        out[a.name] = v;
      }
      value = { pole: out };
    }
    setBusy(true);
    try {
      await onSave(value);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  const axesMode = mode.kind === "axes";
  return (
    <div className="scrim" role="presentation">
      <form className="dialog layer-dialog" role="dialog" aria-modal="true" aria-labelledby="smart-title" onSubmit={submit}>
        <header className="dialog-header">
          <h2 id="smart-title" className="k-subtitle">{axesMode ? t("Smart axes") : t("Add smart pole")} · {glyph}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onCancel}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          <p className="k-caption muted">
            {axesMode
              ? t("A smart glyph can be used as a component that stretches: each use sets its own value on these axes. The master layer is the shape at every axis' minimum; add poles for the other ends. Remove every axis to make it an ordinary glyph again.")
              : t("A pole is the glyph's shape at this position on its smart axes. It starts as a copy of the master layer; edit it to the other extreme, point for point.")}
          </p>
          {axesMode
            ? rows.map((r, i) => (
                <div key={i} className="field layer-axis">
                  <span className="row">
                    <input className="text-input" value={r.name} aria-label={t("Axis name")} placeholder={t("Name")} onChange={(e) => setRows(rows.map((x, j) => (j === i ? { ...x, name: e.target.value } : x)))} />
                    <input className="text-input" inputMode="decimal" value={r.min} aria-label={t("{name} minimum", { name: r.name })} onChange={(e) => setRows(rows.map((x, j) => (j === i ? { ...x, min: e.target.value } : x)))} />
                    <span className="muted">{t("to")}</span>
                    <input className="text-input" inputMode="decimal" value={r.max} aria-label={t("{name} maximum", { name: r.name })} onChange={(e) => setRows(rows.map((x, j) => (j === i ? { ...x, max: e.target.value } : x)))} />
                    <button type="button" className="icon-btn" aria-label={t("Remove {name}", { name: r.name })} onClick={() => setRows(rows.filter((_, j) => j !== i))}><CloseIcon size={14} /></button>
                  </span>
                </div>
              ))
            : mode.axes.map((a) => (
                <label key={a.name} className="field layer-axis">
                  <span className="k-label">{a.name} <span className="muted">({a.min}–{a.max})</span></span>
                  <input className="text-input" inputMode="decimal" value={pole[a.name]} aria-label={t("{name} pole", { name: a.name })} onChange={(e) => setPole({ ...pole, [a.name]: e.target.value })} />
                </label>
              ))}
          {axesMode && (
            <button type="button" className="btn btn-secondary btn-sm" onClick={() => setRows([...rows, { name: rows.length ? t("Height") : t("Width"), min: "0", max: "100" }])}>
              <AddIcon size={14} /> {t("Add axis")}
            </button>
          )}
          {error && <p className="notice notice-danger k-label">{error}</p>}
        </div>
        <footer className="dialog-footer">
          <span className="spacer" />
          <button type="button" className="btn btn-secondary" onClick={onCancel}>{t("Cancel")}</button>
          <button type="submit" className="btn btn-primary" disabled={busy}>{axesMode ? t("Save") : t("Add pole")}</button>
        </footer>
      </form>
    </div>
  );
}
