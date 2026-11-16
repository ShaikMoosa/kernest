import { useEffect, useState } from "react";
import type { Axis, AxisRange, Location } from "../lib/backend";
import { CloseIcon } from "./Icons";

export type LayerDialogMode =
  | { kind: "brace"; initial: Location; editing: boolean }
  | { kind: "bracket"; initial: AxisRange[]; editing: boolean };

interface Props {
  glyph: string;
  axes: Axis[];
  mode: LayerDialogMode;
  onCancel: () => void;
  /** Resolves when saved; rejects with a message to show. */
  onSave: (value: { location?: Location; ranges?: AxisRange[] }) => Promise<void>;
}

const num = (s: string) => (s.trim() === "" ? undefined : Number(s));

/**
 * Add or edit a brace (intermediate) layer — a location on the axes — or an alternate
 * (bracket layers) — axis ranges where it replaces the normal design.
 */
export function LayerDialog({ glyph, axes, mode, onCancel, onSave }: Props) {
  const brace = mode.kind === "brace";
  const [loc, setLoc] = useState<Record<string, string>>(() =>
    Object.fromEntries(axes.map((a) => [a.tag, String(brace ? (mode.initial as Location)[a.tag] ?? a.default : "")]))
  );
  const [ranges, setRanges] = useState<Record<string, { min: string; max: string }>>(() =>
    Object.fromEntries(
      axes.map((a) => {
        const r = !brace ? (mode.initial as AxisRange[]).find((x) => x.tag === a.tag) : undefined;
        return [a.tag, { min: r?.min != null ? String(r.min) : "", max: r?.max != null ? String(r.max) : "" }];
      })
    )
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
    let value: { location?: Location; ranges?: AxisRange[] };
    if (brace) {
      const location: Location = {};
      for (const a of axes) {
        const v = num(loc[a.tag]);
        if (v === undefined || Number.isNaN(v)) return setError(`Enter a value for ${a.name}.`);
        location[a.tag] = v;
      }
      value = { location };
    } else {
      const out: AxisRange[] = [];
      for (const a of axes) {
        const min = num(ranges[a.tag].min), max = num(ranges[a.tag].max);
        if (Number.isNaN(min) || Number.isNaN(max)) return setError(`${a.name}: enter numbers.`);
        if (min !== undefined || max !== undefined) out.push({ tag: a.tag, min: min ?? null, max: max ?? null });
      }
      if (!out.length) return setError("Give a minimum or a maximum on at least one axis.");
      value = { ranges: out };
    }
    setBusy(true);
    try {
      await onSave(value);
    } catch (err) {
      setError(String(err));
      setBusy(false);
    }
  }

  const title = brace
    ? `${mode.editing ? "Move" : "Add"} intermediate layer`
    : `${mode.editing ? "Edit" : "Add"} alternate`;
  return (
    <div className="scrim" role="presentation">
      <form className="dialog layer-dialog" role="dialog" aria-modal="true" aria-labelledby="layer-title" onSubmit={submit}>
        <header className="dialog-header">
          <h2 id="layer-title" className="k-subtitle">{title} · {glyph}</h2>
          <button type="button" className="icon-btn" aria-label="Close" onClick={onCancel}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          <p className="k-caption muted">
            {brace
              ? "An intermediate (brace) layer is an extra drawing at its own location between masters. It must match the masters point for point; it starts as the glyph interpolated there."
              : "An alternate (bracket layers) is a second design used wherever the location is inside these ranges, for example a simpler dollar sign at heavy weights. Every master gets a copy to edit. A range includes both its minimum and its maximum; leave a box empty for no limit."}
          </p>
          {axes.map((a) => (
            <div key={a.tag} className="field layer-axis">
              <span className="k-label">{a.name} <span className="muted">({a.tag}, {a.min}–{a.max})</span></span>
              {brace ? (
                <input className="text-input" inputMode="decimal" value={loc[a.tag]} onChange={(e) => setLoc({ ...loc, [a.tag]: e.target.value })} aria-label={`${a.name} location`} />
              ) : (
                <span className="row">
                  <input className="text-input" inputMode="decimal" placeholder="from (no limit)" value={ranges[a.tag].min} onChange={(e) => setRanges({ ...ranges, [a.tag]: { ...ranges[a.tag], min: e.target.value } })} aria-label={`${a.name} minimum`} />
                  <span className="muted">to</span>
                  <input className="text-input" inputMode="decimal" placeholder="to (no limit)" value={ranges[a.tag].max} onChange={(e) => setRanges({ ...ranges, [a.tag]: { ...ranges[a.tag], max: e.target.value } })} aria-label={`${a.name} maximum`} />
                </span>
              )}
            </div>
          ))}
          {error && <p className="notice notice-danger k-label">{error}</p>}
        </div>
        <footer className="dialog-footer">
          <span className="spacer" />
          <button type="button" className="btn btn-secondary" onClick={onCancel}>Cancel</button>
          <button type="submit" className="btn btn-primary" disabled={busy}>{mode.editing ? "Save" : "Add"}</button>
        </footer>
      </form>
    </div>
  );
}
