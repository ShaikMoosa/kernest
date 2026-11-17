import { useEffect, useRef, useState } from "react";
import { backend, errorMessage, type FilterName, type FilterParams } from "../lib/backend";
import { mulAffine, ORIGIN_LABELS, originPoint, pathsToSvg, svgBounds } from "../lib/outline";
import { CloseIcon } from "./Icons";
import { t, tn } from "../lib/i18n";

interface Props {
  name: FilterName;
  /** The glyph shown in the live preview. */
  glyph: string;
  masterId: string;
  /** Glyphs selected in the Font View; empty when nothing is selected. */
  selection: string[];
  onCancel: () => void;
  onApply: (name: FilterName, params: FilterParams, glyphs: string[], allMasters: boolean) => Promise<void>;
}

interface Values {
  radius: string; offsetX: string; offsetY: string; makeStroke: boolean;
  mx: string; my: string; sx: string; sy: string; rot: string; kx: string; ky: string; origin: number;
}

const DEFAULTS: Values = { radius: "10", offsetX: "10", offsetY: "0", makeStroke: false, mx: "0", my: "0", sx: "100", sy: "100", rot: "0", kx: "0", ky: "0", origin: 4 };

const load = (name: FilterName): { values: Values; allMasters: boolean } => {
  try {
    const saved = JSON.parse(localStorage.getItem(`kernest.filter.${name}`) ?? "null");
    if (saved && typeof saved === "object") {
      return { values: { ...DEFAULTS, ...saved.values }, allMasters: saved.allMasters !== false };
    }
  } catch {
    // Start from the defaults.
  }
  return { values: DEFAULTS, allMasters: true };
};

const TITLES: Record<FilterName, string> = { "round-corners": "Round Corners", "offset-curve": "Offset Curve", transformations: "Transformations" };

/**
 * S16 · A filter's settings with a live preview on the current glyph. Apply runs it
 * over the chosen scope as one undoable step; the values are remembered per filter.
 */
export function FilterDialog({ name, glyph, masterId, selection, onCancel, onApply }: Props) {
  const [init] = useState(() => load(name));
  const [v, setV] = useState<Values>(init.values);
  const [allMasters, setAllMasters] = useState(init.allMasters);
  const [useSelection, setUseSelection] = useState(selection.length > 0);
  const [baseD, setBaseD] = useState("");
  const [previewD, setPreviewD] = useState<string | null>(null);
  const [locked, setLocked] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const seq = useRef(0);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  // The drawing as it is now: the preview's reference, and the transform origin's box.
  useEffect(() => {
    let cancelled = false;
    backend.glyphLayer(glyph, masterId).then((l) => {
      if (cancelled) return;
      setBaseD(pathsToSvg(l.paths) + l.componentSvg);
      setLocked(l.glyphLocked);
    }, () => {});
    return () => { cancelled = true; };
  }, [glyph, masterId]);

  const num = (s: string) => {
    const n = Number(s);
    return s.trim() !== "" && Number.isFinite(n) ? n : null;
  };

  /** The settings as the engine takes them, or null while a field is not a number. */
  function params(): FilterParams | null {
    if (name === "round-corners") {
      const radius = num(v.radius);
      return radius === null || radius < 0 ? null : { radius };
    }
    if (name === "offset-curve") {
      const x = num(v.offsetX), y = num(v.offsetY);
      return x === null || y === null ? null : { offsetX: x, offsetY: y, makeStroke: v.makeStroke };
    }
    const mx = num(v.mx), my = num(v.my), sx = num(v.sx), sy = num(v.sy), rot = num(v.rot), kx = num(v.kx), ky = num(v.ky);
    if ([mx, my, sx, sy, rot, kx, ky].some((n) => n === null)) return null;
    const b = svgBounds(baseD);
    const o = b ? originPoint(b, v.origin) : { x: 0, y: 0 };
    const r = (rot! * Math.PI) / 180;
    const scale = [sx! / 100, 0, 0, sy! / 100, 0, 0];
    const skew = [1, Math.tan((ky! * Math.PI) / 180), Math.tan((kx! * Math.PI) / 180), 1, 0, 0];
    const rotate = [Math.cos(r), Math.sin(r), -Math.sin(r), Math.cos(r), 0, 0];
    const move = [1, 0, 0, 1, mx!, my!];
    // Same order as the Edit View's transform panel: scale, skew, rotate, then move.
    const affine = mulAffine(move, mulAffine(rotate, mulAffine(skew, scale)));
    return { affine, origin: [o.x, o.y] };
  }

  const p = params();
  const key = p ? JSON.stringify(p) : null;

  // Live preview, a short pause after the last keystroke.
  useEffect(() => {
    if (!key) return;
    const id = ++seq.current;
    const timer = setTimeout(() => {
      backend.filterPreview(glyph, masterId, name, JSON.parse(key)).then((l) => {
        if (id === seq.current) setPreviewD(pathsToSvg(l.paths) + l.componentSvg);
      }, () => {});
    }, 250);
    return () => clearTimeout(timer);
  }, [key, glyph, masterId, name]);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!p) return setError(t("Enter numbers in every field."));
    setError(null);
    setBusy(true);
    try {
      try {
        localStorage.setItem(`kernest.filter.${name}`, JSON.stringify({ values: v, allMasters }));
      } catch {
        // The filter still runs with these values.
      }
      await onApply(name, p, useSelection ? selection : [], allMasters);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  const field = (label: string, k: keyof Values, suffix?: string) => (
    <label className="field">
      <span className="k-label">{label}{suffix ? <span className="muted"> ({suffix})</span> : null}</span>
      <input className="text-input" inputMode="decimal" value={v[k] as string} aria-label={label}
        onChange={(e) => setV({ ...v, [k]: e.target.value })} />
    </label>
  );

  return (
    <div className="scrim" role="presentation">
      <form className="dialog dialog-sm filter-dialog" role="dialog" aria-modal="true" aria-labelledby="filter-title" onSubmit={submit}>
        <header className="dialog-header">
          <h2 id="filter-title" className="k-subtitle">{t(TITLES[name])} · {glyph}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onCancel}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          <Preview baseD={baseD} previewD={previewD} />
          <p className="k-caption muted">
            {name === "round-corners" && t("Rounds sharp corners with a curve of this radius. Every master keeps the same number of nodes.")}
            {name === "offset-curve" && t("Moves the outline outwards or inwards. Make stroke turns a path into a filled band.")}
            {name === "transformations" && t("Moves, scales, rotates and skews the drawing. Locked paths stay put.")}
          </p>
          {name === "round-corners" && field(t("Radius"), "radius", t("font units"))}
          {name === "offset-curve" && <>
            <div className="filter-grid">
              {field(t("Horizontal offset"), "offsetX")}
              {field(t("Vertical offset"), "offsetY")}
            </div>
            <label className="check-row">
              <input type="checkbox" checked={v.makeStroke} onChange={(e) => setV({ ...v, makeStroke: e.target.checked })} />
              <span className="k-label">{t("Make stroke")}</span>
            </label>
          </>}
          {name === "transformations" && <>
            <div className="filter-grid">
              {field(t("Move X"), "mx")}
              {field(t("Move Y"), "my")}
              {field(t("Scale X"), "sx", "%")}
              {field(t("Scale Y"), "sy", "%")}
              {field(t("Rotate"), "rot", "°")}
              {field(t("Skew X"), "kx", "°")}
              {field(t("Skew Y"), "ky", "°")}
            </div>
            <div className="origin-picker" role="radiogroup" aria-label={t("Transform origin")}>
              {ORIGIN_LABELS.map((label, i) => (
                <button key={label} type="button" role="radio" aria-checked={v.origin === i} className={`origin-dot${v.origin === i ? " is-active" : ""}`}
                  title={t(label)} aria-label={t(label)} onClick={() => setV({ ...v, origin: i })} />
              ))}
            </div>
          </>}
          <div className="filter-scope">
            <label className="check-row">
              <input type="checkbox" checked={allMasters} onChange={(e) => setAllMasters(e.target.checked)} />
              <span className="k-label" title={t("Apply on every master and intermediate layer where the glyph stays compatible")}>{t("All masters")}</span>
            </label>
            {selection.length > 0 && (
              <label className="check-row">
                <input type="checkbox" checked={useSelection} onChange={(e) => setUseSelection(e.target.checked)} />
                <span className="k-label">{tn(selection.length, "Selected glyph ({n})", "Selected glyphs ({n})")}</span>
              </label>
            )}
          </div>
          {locked && <p className="notice notice-warning k-label">{t("This glyph is locked. Unlock it to edit.")}</p>}
          {error && <p className="notice notice-danger k-label">{error}</p>}
        </div>
        <footer className="dialog-footer">
          <span className="spacer" />
          <button type="button" className="btn btn-secondary" onClick={onCancel}>{t("Cancel")}</button>
          <button type="submit" className="btn btn-primary" disabled={busy || locked || !p}>{t("Apply")}</button>
        </footer>
      </form>
    </div>
  );
}

/** The current drawing in grey, the filter's result over it in the selection colour. */
function Preview({ baseD, previewD }: { baseD: string; previewD: string | null }) {
  const b = svgBounds(`${baseD} ${previewD ?? ""}`);
  if (!b) return <div className="filter-preview filter-preview-empty k-caption muted">{t("Nothing to preview")}</div>;
  const pad = Math.max(b.maxX - b.minX, b.maxY - b.minY) * 0.12 + 10;
  const w = b.maxX - b.minX + pad * 2, h = b.maxY - b.minY + pad * 2;
  return (
    <svg className="filter-preview" viewBox={`${b.minX - pad} ${-(b.maxY + pad)} ${w} ${h}`} role="img" aria-label={t("Filter preview")}>
      <path d={baseD} className="filter-preview-base" fillRule="nonzero" />
      {previewD && <path d={previewD} className="filter-preview-result" fillRule="nonzero" />}
    </svg>
  );
}