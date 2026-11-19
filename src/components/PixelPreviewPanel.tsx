import { useEffect, useRef, useState } from "react";
import { backend, type PixelGlyph, type PixelPreview } from "../lib/backend";
import { t, translateMessage } from "../lib/i18n";

interface Props {
  glyph: string;
  unicode: string | null;
  master: string;
  version: number;
  active: boolean;
}

/** Draws one column of the preview: fill the pixel-space paths at 1×, then keep
 * only pixels past half coverage, so the result is a crisp on/off pixel grid. */
function PixelCell({ glyphs, ppem, kind, themeTick }: { glyphs: PixelGlyph[]; ppem: number; kind: "hinted" | "unhinted"; themeTick: number }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const scale = Math.max(2, Math.min(8, Math.floor(96 / ppem)));
    const width = Math.max(1, Math.ceil(glyphs.reduce((s, g) => s + g.advance, 0)));
    const baseline = Math.round(ppem * 0.8);
    const off = document.createElement("canvas");
    off.width = width;
    off.height = ppem;
    const octx = off.getContext("2d");
    if (!octx) return;
    octx.translate(0, baseline);
    let x = 0;
    for (const g of glyphs) {
      octx.save();
      octx.translate(x, 0);
      octx.fill(new Path2D(g[kind]));
      octx.restore();
      x += g.advance;
    }
    const data = octx.getImageData(0, 0, width, ppem).data;
    canvas.width = width * scale;
    canvas.height = ppem * scale;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const ink = getComputedStyle(canvas).color;
    ctx.fillStyle = ink;
    ctx.globalAlpha = 0.12;
    for (let gx = 0; gx <= width; gx++) ctx.fillRect(gx * scale, 0, 1, ppem * scale);
    for (let gy = 0; gy <= ppem; gy++) ctx.fillRect(0, gy * scale, width * scale, 1);
    ctx.globalAlpha = 0.3;
    ctx.fillRect(0, baseline * scale, width * scale, 1);
    ctx.globalAlpha = 1;
    for (let py = 0; py < ppem; py++) {
      for (let px = 0; px < width; px++) {
        if (data[(py * width + px) * 4 + 3] >= 128) ctx.fillRect(px * scale, py * scale, scale, scale);
      }
    }
  }, [glyphs, ppem, kind, themeTick]);
  return <canvas ref={ref} className="pixel-canvas" />;
}

/** S62 · The edit view's pixel preview: the current glyph or a test string at
 * chosen sizes, hinted and unhinted side by side. */
export function PixelPreviewPanel({ glyph, unicode, master, version, active }: Props) {
  const [open, setOpen] = useState(false);
  const char = unicode ? String.fromCodePoint(parseInt(unicode, 16)) : "";
  const [text, setText] = useState(char);
  const [sizesText, setSizesText] = useState("12, 16, 24");
  const [sizes, setSizes] = useState<number[]>([12, 16, 24]);
  const [view, setView] = useState<PixelPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [themeTick, setThemeTick] = useState(0);
  const seq = useRef(0);

  // A new glyph brings its own character into the preview.
  useEffect(() => { setText(char); }, [char]);

  // Canvas colours come from the theme; redraw when it changes.
  useEffect(() => {
    const bump = () => setThemeTick((n) => n + 1);
    const mo = new MutationObserver(bump);
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme"] });
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener("change", bump);
    return () => { mo.disconnect(); mq.removeEventListener("change", bump); };
  }, []);

  useEffect(() => {
    if (!open || !active) return;
    if (text.trim() === "") { setView(null); setError(null); return; }
    const id = ++seq.current;
    setBusy(true);
    const timer = setTimeout(() => {
      backend.pixelPreview(text, master, undefined, sizes).then((v) => {
        if (id !== seq.current) return;
        setView(v);
        setError(null);
        setBusy(false);
      }, (e) => {
        if (id !== seq.current) return;
        setView(null);
        setError(translateMessage(e instanceof Error ? e.message : String(e)));
        setBusy(false);
      });
    }, 300);
    return () => clearTimeout(timer);
  }, [open, active, text, sizes, master, version]);

  const commitSizes = (v: string) => {
    const list: number[] = [];
    for (const part of v.split(/[,\s]+/)) {
      const n = Number(part);
      if (Number.isInteger(n) && n >= 1 && n <= 512 && !list.includes(n)) list.push(n);
    }
    if (list.length === 0) { setSizesText(sizes.join(", ")); return; }
    setSizes(list);
    setSizesText(list.join(", "));
  };

  return (
    <div className="transform-panel pixel-panel">
      <button type="button" className="transform-head" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round"><path d="M6 9l6 6 6-6" /></svg>
        {t("Pixel preview")}
      </button>
      {open && (
        <div className="transform-body pixel-body">
          <div className="pixel-controls">
            <label className="info-field"><span className="k-caption muted">{t("Text")}</span>
              <input className="metric-input text-field pixel-text" value={text} aria-label={t("Preview text")}
                placeholder={char || glyph} onChange={(e) => setText(e.target.value)} />
            </label>
            <label className="info-field" title={t("Sizes in pixels, separated by commas")}><span className="k-caption muted">{t("Sizes (px)")}</span>
              <input className="metric-input text-field pixel-sizes" defaultValue={sizesText} key={sizesText} aria-label={t("Pixel sizes")}
                onBlur={(e) => commitSizes(e.currentTarget.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") (e.target as HTMLInputElement).blur();
                  if (e.key === "Escape") { e.currentTarget.value = sizesText; (e.target as HTMLInputElement).blur(); }
                }} />
            </label>
            {busy && <span className="k-caption muted">{t("Rendering…")}</span>}
          </div>
          {error && <p className="notice notice-warning k-label pixel-note" role="status">⚠ {error}</p>}
          {view && view.problem !== "" && <p className="notice notice-warning k-label pixel-note" role="status">⚠ {translateMessage(view.problem)}</p>}
          {text.trim() === "" && <p className="k-caption muted pixel-note">{t("Type some text to preview.")}</p>}
          {view && view.sizes.map((s) => (
            <div key={s.ppem} className="pixel-row">
              <span className="k-caption muted pixel-size">{t("{n} px", { n: s.ppem })}</span>
              <div className="pixel-col">
                <span className="k-caption muted">{t("Hinted")}</span>
                <PixelCell glyphs={s.glyphs} ppem={s.ppem} kind="hinted" themeTick={themeTick} />
              </div>
              <div className="pixel-col">
                <span className="k-caption muted">{t("Unhinted")}</span>
                <PixelCell glyphs={s.glyphs} ppem={s.ppem} kind="unhinted" themeTick={themeTick} />
              </div>
            </div>
          ))}
          <p className="k-caption muted pixel-note">{t("Hinted uses the export's TrueType hinting. Zones and stems change it; the unhinted side stays the same.")}</p>
        </div>
      )}
    </div>
  );
}