import { useEffect, useState } from "react";
import { errorMessage, type ColorStop, type Paint } from "../lib/backend";
import { t } from "../lib/i18n";
import { AddIcon, CloseIcon } from "./Icons";

interface Props {
  glyph: string;
  paint: Paint;
  palette: string[];
  /** For sensible default gradient points: the glyph's width and height. */
  width: number;
  height: number;
  onCancel: () => void;
  onSave: (paint: Paint) => Promise<void>;
}

const num = (s: string) => Number(s.trim().replace("−", "-"));

/** Palette colours to choose from, as swatches. */
function Swatches({ palette, value, onPick, label }: { palette: string[]; value: number; onPick: (i: number) => void; label: string }) {
  return (
    <span className="swatches" role="radiogroup" aria-label={label}>
      {palette.map((c, i) => (
        <button key={i} type="button" role="radio" aria-checked={value === i} title={t("Colour {n}", { n: i + 1 })}
          className={`swatch${value === i ? " is-selected" : ""}`} style={{ background: c }} onClick={() => onPick(i)} />
      ))}
    </span>
  );
}

/** Edit View › Paint…: how a colour layer is filled. */
export function PaintDialog({ glyph, paint, palette, width, height, onCancel, onSave }: Props) {
  const [draft, setDraft] = useState<Paint>(paint);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  const firstColor = draft.type === "solid" ? draft.color : draft.stops[0]?.color ?? 0;
  const defaultStops = (): ColorStop[] => [
    { offset: 0, color: firstColor, alpha: 1 },
    { offset: 1, color: (firstColor + 1) % Math.max(palette.length, 1), alpha: 1 },
  ];
  const setType = (type: Paint["type"]) => {
    if (type === draft.type) return;
    const stops = draft.type === "solid" ? defaultStops() : draft.stops;
    if (type === "solid") setDraft({ type, color: firstColor, alpha: 1 });
    else if (type === "linear") setDraft({ type, start: [0, 0], end: [Math.round(width), 0], stops });
    else setDraft({ type, center: [Math.round(width / 2), Math.round(height / 2)], radius: Math.round(Math.max(width, height) / 2), stops });
  };
  const setStop = (i: number, patch: Partial<ColorStop>) => {
    if (draft.type === "solid") return;
    setDraft({ ...draft, stops: draft.stops.map((s, j) => (j === i ? { ...s, ...patch } : s)) } as Paint);
  };
  const point = (label: string, value: [number, number], set: (v: [number, number]) => void) => (
    <span className="paint-point">
      <span className="k-caption muted">{label}</span>
      <input className="metric-input" inputMode="decimal" aria-label={`${label} x`} value={value[0]} onChange={(e) => set([num(e.target.value), value[1]])} />
      <input className="metric-input" inputMode="decimal" aria-label={`${label} y`} value={value[1]} onChange={(e) => set([value[0], num(e.target.value)])} />
    </span>
  );

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    const coords = draft.type === "linear" ? [...draft.start, ...draft.end]
      : draft.type === "radial" ? [...draft.center, draft.radius] : [];
    if (coords.some((v) => !Number.isFinite(v))) return setError(t("Points must be numbers."));
    setBusy(true);
    try {
      await onSave(draft);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  return (
    <div className="scrim" role="presentation">
      <form className="dialog paint-dialog" role="dialog" aria-modal="true" aria-labelledby="paint-title" onSubmit={submit}>
        <header className="dialog-header">
          <h2 id="paint-title" className="k-subtitle">{t("Colour layer paint")} · {glyph}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onCancel}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          <div className="segmented" role="radiogroup" aria-label={t("Fill")}>
            {(["solid", "linear", "radial"] as const).map((k) => (
              <button key={k} type="button" role="radio" aria-checked={draft.type === k} className={`segment${draft.type === k ? " is-selected" : ""}`} onClick={() => setType(k)}>
                {{ solid: t("Solid colour"), linear: t("Linear gradient"), radial: t("Radial gradient") }[k]}
              </button>
            ))}
          </div>
          {draft.type === "solid" ? (
            <div className="field">
              <span className="k-label">{t("Colour")}</span>
              <Swatches palette={palette} value={draft.color} label={t("Colour")} onPick={(i) => setDraft({ ...draft, color: i })} />
              <label className="paint-alpha"><span className="k-caption muted">{t("Opacity (%)")}</span>
                <input className="metric-input" inputMode="decimal" value={Math.round(draft.alpha * 100)} onChange={(e) => setDraft({ ...draft, alpha: Math.min(1, Math.max(0, num(e.target.value) / 100 || 0)) })} />
              </label>
            </div>
          ) : (
            <>
              <div className="field">
                <span className="k-label">{t("Points (font units)")}</span>
                <span className="row">
                  {draft.type === "linear" ? (
                    <>
                      {point(t("Start"), draft.start, (v) => setDraft({ ...draft, start: v }))}
                      {point(t("End"), draft.end, (v) => setDraft({ ...draft, end: v }))}
                    </>
                  ) : (
                    <>
                      {point(t("Centre"), draft.center, (v) => setDraft({ ...draft, center: v }))}
                      <span className="paint-point">
                        <span className="k-caption muted">{t("Radius")}</span>
                        <input className="metric-input" inputMode="decimal" aria-label={t("Radius")} value={draft.radius} onChange={(e) => setDraft({ ...draft, radius: num(e.target.value) })} />
                      </span>
                    </>
                  )}
                </span>
              </div>
              <div className="field">
                <span className="k-label">{t("Colour stops")}</span>
                {draft.stops.map((s, i) => (
                  <span key={i} className="row paint-stop">
                    <Swatches palette={palette} value={s.color} label={t("Stop {n} colour", { n: i + 1 })} onPick={(c) => setStop(i, { color: c })} />
                    <label><span className="k-caption muted">{t("Position (%)")}</span>
                      <input className="metric-input" inputMode="decimal" value={Math.round(s.offset * 100)} onChange={(e) => setStop(i, { offset: Math.min(1, Math.max(0, num(e.target.value) / 100 || 0)) })} />
                    </label>
                    <label><span className="k-caption muted">{t("Opacity (%)")}</span>
                      <input className="metric-input" inputMode="decimal" value={Math.round(s.alpha * 100)} onChange={(e) => setStop(i, { alpha: Math.min(1, Math.max(0, num(e.target.value) / 100 || 0)) })} />
                    </label>
                    <button type="button" className="icon-btn" aria-label={t("Remove stop {n}", { n: i + 1 })} disabled={draft.stops.length <= 2}
                      onClick={() => setDraft({ ...draft, stops: draft.stops.filter((_, j) => j !== i) } as Paint)}><CloseIcon size={14} /></button>
                  </span>
                ))}
                <button type="button" className="btn btn-secondary btn-sm" onClick={() => setDraft({ ...draft, stops: [...draft.stops, { offset: 1, color: firstColor, alpha: 1 }] } as Paint)}>
                  <AddIcon size={14} /> {t("Add stop")}
                </button>
              </div>
            </>
          )}
          <p className="k-caption muted">{t("The paint applies to this colour layer in every master. Colours come from the palette in Font Info.")}</p>
          {error && <p className="notice notice-danger k-label">{error}</p>}
        </div>
        <footer className="dialog-footer">
          <span className="spacer" />
          <button type="button" className="btn btn-secondary" onClick={onCancel}>{t("Cancel")}</button>
          <button type="submit" className="btn btn-primary" disabled={busy}>{t("Save")}</button>
        </footer>
      </form>
    </div>
  );
}
