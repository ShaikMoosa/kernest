import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { backend, type DocStatus, type FeaturesView, type FontInfo, type KernMode, type Location, type MetricName, type Milestone, type ShapedView, type TextDirection, type TextItem, type TextLayout } from "../lib/backend";
import { t } from "../lib/i18n";
import { ColorGlyph } from "./ColorGlyph";
import { wrapRows } from "../lib/onboarding";
import { loadSamples, onSamplesChanged } from "../lib/samples";
import { firstStrongRtl, visualOrder } from "../lib/bidi";
import { kernBar, loadKerningPreview, saveKerningPreview, withKerning, type KerningPreview } from "../lib/kerningPreview";

const PRESETS = loadSamples();
const MISSING_WIDTH = 500;
const MODES: { id: KernMode; label: string; title: string }[] = [
  { id: "group", label: "Group", title: "Kern the two glyphs' kerning groups (or the glyphs, if they have none)" },
  { id: "leftException", label: "Left exception", title: "Kern this left glyph against the right group" },
  { id: "rightException", label: "Right exception", title: "Kern the left group against this right glyph" },
  { id: "exception", label: "Exception", title: "Kern exactly these two glyphs" },
];
const PREVIEWS: { id: KerningPreview; label: string }[] = [
  { id: "off", label: "No kerning" },
  { id: "on", label: "Show kerning" },
  { id: "locked", label: "Kerning, spacing locked" },
];
const DIRECTIONS: { id: TextDirection; label: string; title: string; icon: string }[] = [
  { id: "ltr", label: "Left to right", title: "Left to right (Ctrl+Alt+Shift+←)", icon: "M3 6h11M3 11h7M3 16h11M15 11h6M18 8l3 3-3 3" },
  { id: "rtl", label: "Right to left", title: "Right to left (Ctrl+Alt+Shift+→)", icon: "M10 6h11M14 11h7M10 16h11M3 11h6M6 8l-3 3 3 3" },
  { id: "vertical", label: "Vertical", title: "Vertical, top to bottom (Ctrl+Alt+Shift+↓)", icon: "M18 3v11M13 3v7M8 3v11M13 15v6M10 18l3 3 3-3" },
];

type Glyph = Extract<TextItem, { type: "glyph" }>;
interface Cursor { line: number; pos: number }

/** One item to place: advances and offsets in font units. */
interface GeomItem { adv: number; kern: number; rtl: boolean; vAdv: number; xOff: number; yOff: number }
interface Box { x: number; y: number; w: number; h: number }
/** A placed item: where its glyph origin goes and the box it occupies, in pixels. */
interface Placed { ox: number; oy: number; box: Box }
interface Caret { x1: number; y1: number; x2: number; y2: number }
/** A line (a column in vertical text): cells and carets by logical index. */
interface Row { cells: Placed[]; carets: Caret[]; base: number; start: number; end: number }

/**
 * Lays rows out in the writing direction. Horizontal rows run left to right, or end at
 * the right edge in a right-to-left paragraph, reordered by `visualOrder` unless
 * `ordered` (shaped glyphs come in visual order); kerning sits on the side of the
 * glyph's direction. Vertical rows are columns from right to left, top to bottom.
 */
function layoutRows(lines: GeomItem[][], g: { scale: number; pad: number; asc: number; desc: number; lineH: number; vertical: boolean; paraRtl: boolean; ordered: boolean; minLen: number }): { rows: Row[]; width: number; height: number } {
  const s = g.scale;
  const len = (ln: GeomItem[]) => ln.reduce((n, it) => n + (g.vertical ? it.vAdv : it.adv + it.kern), 0) * s;
  const longest = Math.max(g.minLen, ...lines.map(len));
  const across = g.lineH * s;
  if (g.vertical) {
    const width = Math.max(400, lines.length * across + 2 * g.pad);
    const rows = lines.map((ln): Row => {
      const li = lines.indexOf(ln);
      const cx = width - g.pad - (li + 0.5) * across;
      const caret = (y: number): Caret => ({ x1: cx - across * 0.42, x2: cx + across * 0.42, y1: y, y2: y });
      let pen = g.pad;
      const cells: Placed[] = [];
      const carets: Caret[] = [];
      for (const it of ln) {
        carets.push(caret(pen));
        const w = (it.adv || g.lineH * 0.8) * s;
        cells.push({ ox: cx + it.xOff * s, oy: pen - it.yOff * s, box: { x: cx - w / 2, y: pen, w, h: it.vAdv * s } });
        pen += it.vAdv * s;
      }
      carets.push(caret(pen));
      return { cells, carets, base: cx, start: g.pad, end: pen };
    });
    return { rows, width, height: longest + 2 * g.pad };
  }
  const width = longest + 2 * g.pad;
  const rows = lines.map((ln, li): Row => {
    const by = g.pad + (li * g.lineH + g.asc * 1.1) * s;
    const start = g.paraRtl ? width - g.pad - len(ln) : g.pad;
    let x = start;
    const order = g.ordered ? ln.map((_, i) => i) : visualOrder(ln.map((it) => it.rtl), g.paraRtl);
    const cells: Placed[] = new Array(ln.length);
    for (const i of order) {
      const it = ln[i];
      if (it.rtl) x += it.kern * s;
      cells[i] = { ox: x + it.xOff * s, oy: by - it.yOff * s, box: { x, y: by - g.asc * s, w: it.adv * s, h: (g.asc - g.desc) * s } };
      x += it.adv * s;
      if (!it.rtl) x += it.kern * s;
    }
    // The leading edge is the left one for left-to-right glyphs, the right one otherwise.
    const edge = (i: number, leading: boolean) => { const b = cells[i].box; return ln[i].rtl === leading ? b.x + b.w : b.x; };
    const y1 = by - g.asc * 1.05 * s, y2 = by - g.desc * 1.05 * s;
    const carets = Array.from({ length: ln.length + 1 }, (_, p): Caret => {
      const cx = ln.length === 0 ? start : p < ln.length && (p === 0 || ln[p].rtl === ln[p - 1].rtl) ? edge(p, true) : edge(p - 1, false);
      return { x1: cx, x2: cx, y1, y2 };
    });
    return { cells, carets, base: by, start, end: x };
  });
  const height = lines.length ? g.pad + ((lines.length - 1) * g.lineH + g.asc * 1.1) * s + -g.desc * 1.2 * s + g.pad : 400;
  return { rows, width, height };
}

function NumField({ label, value, title, disabled, onCommit }: { label: string; value: number | null; title?: string; disabled?: boolean; onCommit: (v: number) => void }) {
  const shown = value === null ? "" : String(Math.round(value));
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);
  const commit = (raw: string) => {
    const v = Number(raw.trim().replace("−", "-") || "0");
    if (!Number.isFinite(v) || raw === shown) return setText(shown);
    onCommit(v);
  };
  return (
    <label className="info-field" title={title}>
      <span className="k-caption muted">{label}</span>
      <input className="metric-input" value={text} disabled={disabled} placeholder="—" onChange={(e) => setText(e.target.value)} onBlur={(e) => commit(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") (e.target as HTMLInputElement).blur();
          if (e.key === "Escape") { setText(shown); (e.target as HTMLInputElement).blur(); }
        }} />
    </label>
  );
}

function GroupField({ label, value, disabled, onCommit }: { label: string; value: string | null; disabled?: boolean; onCommit: (v: string | null) => void }) {
  const shown = value ?? "";
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);
  return (
    <label className="info-field">
      <span className="k-caption muted">{label}</span>
      <input className="metric-input group-input" value={text} disabled={disabled} placeholder={t("none")} onChange={(e) => setText(e.target.value)}
        onBlur={(e) => { const v = e.currentTarget.value.trim(); if (v !== shown) onCommit(v || null); }}
        onKeyDown={(e) => {
          if (e.key === "Enter") (e.target as HTMLInputElement).blur();
          if (e.key === "Escape") { setText(shown); (e.target as HTMLInputElement).blur(); }
        }} />
    </label>
  );
}

interface Props {
  masterId: string;
  /** Bumped by the app after any change to the font, so the layout refreshes. */
  version: number;
  active: boolean;
  onChanged: (status: DocStatus) => void;
  onError: (e: unknown) => void;
  onOpenGlyph: (name: string) => void;
  /** Opens the Kerning window with this pair selected (a click on a kerning bar). */
  onOpenKerning?: (left: string, right: string) => void;
  textWidth?: number;
  /** Follow the writing direction of the text's first letter (Appearance setting). */
  autoDirection?: boolean;
  onMilestone?: (milestone: Milestone) => void;
  /** The tab's title follows the sample text. */
  onTitle?: (title: string) => void;
}

/** S06 · Text view for spacing and kerning. */
export function TextView({ masterId, version, active, onChanged, onError, onOpenGlyph, onOpenKerning, textWidth = 16000, autoDirection = true, onMilestone, onTitle }: Props) {
  const [text, setText] = useState(PRESETS[0]);
  const [samples, setSamples] = useState(loadSamples);
  useEffect(() => onSamplesChanged(() => setSamples(loadSamples())), []);
  useEffect(() => { onTitle?.(text.trim()); }, [text, onTitle]);
  // The tab's writing direction (session state, per tab).
  const [direction, setDirection] = useState<TextDirection>("ltr");
  const vertical = direction === "vertical";
  const paraRtl = direction === "rtl";
  // Typing right-to-left text switches the tab to right to left, and back.
  useEffect(() => {
    if (!autoDirection) return;
    const rtl = firstStrongRtl(text);
    if (rtl !== null) setDirection((d) => (d === "vertical" ? d : rtl ? "rtl" : "ltr"));
  }, [text, autoDirection]);
  const [info, setInfo] = useState<FontInfo | null>(null);
  const [rawLayout, setLayout] = useState<TextLayout | null>(null);
  // Kerning preview: off sets the text without kerning; locked keeps spacing fixed.
  const [kernPreview, setKernPreview] = useState<KerningPreview>(loadKerningPreview);
  useEffect(() => saveKerningPreview(kernPreview), [kernPreview]);
  const kernOff = kernPreview === "off";
  const spacingLocked = kernPreview === "locked";
  // The setting is in 1/1000 em; advances are in font units, so scale by the UPM.
  const wrapWidth = textWidth / 1000 * (info?.unitsPerEm ?? 1000);
  const layout = useMemo(() => {
    if (!rawLayout) return null;
    const em = rawLayout.metrics.ascender - rawLayout.metrics.descender;
    const lines = kernOff ? rawLayout.lines.map((l) => l.map((it) => (it.type === "glyph" ? { ...it, kern: 0 } : it))) : rawLayout.lines;
    const advance = vertical
      ? (item: TextItem) => (item.type === "glyph" ? item.vAdvance : em)
      : (item: TextItem) => (item.type === "glyph" ? item.width + item.kern : MISSING_WIDTH);
    return { ...rawLayout, lines: wrapRows(lines, wrapWidth, advance) };
  }, [rawLayout, wrapWidth, vertical, kernOff]);
  const [size, setSize] = useState(140);
  const [mode, setMode] = useState<KernMode>("group");
  const [cursor, setCursor] = useState<Cursor>({ line: 0, pos: 1 });
  const [showMetrics, setShowMetrics] = useState(true);
  const svgRef = useRef<SVGSVGElement>(null);
  const chain = useRef(Promise.resolve());
  const latest = useRef({ layout, cursor, mode, direction, kernOff, previewing: false, shapeOn: false });
  latest.current = { layout, cursor, mode, direction, kernOff, previewing: latest.current?.previewing ?? false, shapeOn: latest.current?.shapeOn ?? false };

  // "" = the current master; "i:<name>" = an instance; "custom" = the sliders below.
  const [preview, setPreview] = useState("");
  const [custom, setCustom] = useState<Location>({});
  useEffect(() => { backend.fontInfo().then(setInfo, () => setInfo(null)); }, [version]);
  const location: Location | undefined = useMemo(() => {
    if (preview === "custom") return custom;
    if (preview.startsWith("i:")) return info?.instances.find((i) => i.name === preview.slice(2))?.location;
    return undefined;
  }, [preview, custom, info]);
  // Fall back to the current master if the previewed instance or design space went away (e.g. undo).
  useEffect(() => {
    if (!info) return;
    if (preview === "custom" && (info.axes.length === 0 || info.masters.length < 2)) setPreview("");
    if (preview.startsWith("i:") && !info.instances.some((i) => i.name === preview.slice(2))) setPreview("");
  }, [info, preview]);
  const previewing = location !== undefined;
  latest.current.previewing = previewing;

  // ---------- Preview panel: the text in every master ----------
  const [panelOpen, setPanelOpen] = useState(() => { try { return localStorage.getItem("kernest.textPreviewPanel") !== "0"; } catch { return true; } });
  const [panelSize, setPanelSize] = useState(36);
  const [panelLayouts, setPanelLayouts] = useState<{ id: string; name: string; layout: TextLayout }[]>([]);
  // The panel runs horizontally; vertical tabs preview left to right.
  const panelDirection: TextDirection = paraRtl ? "rtl" : "ltr";
  useEffect(() => { try { localStorage.setItem("kernest.textPreviewPanel", panelOpen ? "1" : "0"); } catch { /* a convenience only */ } }, [panelOpen]);
  useEffect(() => {
    if (!panelOpen || !info) return;
    let live = true;
    const id = setTimeout(() => {
      Promise.all(info.masters.map((m) => backend.textLayout(text, m.id, undefined, panelDirection).then((layout) => ({ id: m.id, name: m.name, layout }))))
        .then((rows) => { if (live) setPanelLayouts(rows); }, () => {});
    }, 200);
    return () => { live = false; clearTimeout(id); };
  }, [panelOpen, info, text, version, panelDirection]);

  // ---------- Feature preview ----------
  const [featView, setFeatView] = useState<FeaturesView | null>(null);
  const [featOpen, setFeatOpen] = useState(false);
  const [shapeOn, setShapeOn] = useState(false);
  // Only the tags the user flipped; the rest follow what's in the font.
  const [overrides, setOverrides] = useState<Record<string, boolean>>({});
  const [rawShaped, setShaped] = useState<ShapedView | null>(null);
  const shaped = useMemo(() => {
    if (!rawShaped) return null;
    if (vertical) return { ...rawShaped, lines: wrapRows(rawShaped.lines, wrapWidth, (glyph) => -glyph.yAdvance, (glyph) => glyph.cluster) };
    // Shaped lines come in visual order: wrap a right-to-left line from its right end.
    if (paraRtl) return { ...rawShaped, lines: rawShaped.lines.flatMap((line) => wrapRows([[...line].reverse()], wrapWidth, (glyph) => glyph.advance, (glyph) => glyph.cluster).map((row) => row.reverse())) };
    return { ...rawShaped, lines: wrapRows(rawShaped.lines, wrapWidth, (glyph) => glyph.advance, (glyph) => glyph.cluster) };
  }, [rawShaped, wrapWidth, vertical, paraRtl]);
  latest.current.shapeOn = shapeOn;
  const featRef = useRef<HTMLDivElement>(null);
  useEffect(() => { backend.features().then(setFeatView, () => setFeatView(null)); }, [version]);
  useEffect(() => {
    if (!featOpen) return;
    const close = (e: MouseEvent) => { if (!featRef.current?.contains(e.target as Node)) setFeatOpen(false); };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") setFeatOpen(false); };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", esc);
    return () => { document.removeEventListener("mousedown", close); document.removeEventListener("keydown", esc); };
  }, [featOpen]);
  const featTags = useMemo(() => featView ? [...new Set([...featView.available, ...featView.handwritten])].sort() : [], [featView]);
  // aalt only lists alternates for glyph palettes; text never applies it unasked.
  const featChecked = (tag: string) => overrides[tag] ?? (tag !== "aalt" && (featView?.automatic.includes(tag) || featView?.handwritten.includes(tag) || false));
  // Explicit on/off per tag so default-on features like liga can be switched off too.
  const featSettings = useMemo(() => withKerning(featTags.map((tag) => `${tag}=${featChecked(tag) ? 1 : 0}`), kernPreview), [featTags, overrides, featView, kernPreview]);

  const reload = useCallback(() => backend.textLayout(text, masterId, location, direction).then(result => { setLayout(result); if (preview.startsWith("i:")) onMilestone?.("instance"); }, onError), [text, masterId, location, direction, preview, onMilestone, onError]);
  const reloadShaped = useCallback(() => backend.shapeText(text, masterId, location, featSettings, direction).then(result => { setShaped(result); if (preview.startsWith("i:")) onMilestone?.("instance"); }, onError), [text, masterId, location, featSettings, direction, preview, onMilestone, onError]);
  useEffect(() => {
    if (!shapeOn) { reload(); return; }
    // Shaping compiles a preview font; don't do it on every keystroke.
    const id = setTimeout(reloadShaped, 250);
    return () => clearTimeout(id);
  }, [reload, reloadShaped, version, shapeOn]);

  // Keep the cursor inside the text.
  useEffect(() => {
    if (!layout) return;
    setCursor((c) => {
      const line = Math.min(c.line, layout.lines.length - 1);
      return { line, pos: Math.min(c.pos, layout.lines[line]?.length ?? 0) };
    });
  }, [layout]);

  const run = useCallback((task: () => Promise<DocStatus>, milestone?: Milestone) => {
    if (latest.current.previewing) return onError(t("Spacing edits apply to a master. Set the preview back to “Current master” to edit."));
    chain.current = chain.current.then(async () => {
      try {
        onChanged(await task());
        if (milestone) onMilestone?.(milestone);
        await reload();
      } catch (e) {
        onError(e);
      }
    });
  }, [onChanged, onError, reload, onMilestone]);

  /**
   * Sets the kerning of (left, right). In Group mode the pair that currently applies
   * is edited (or a group pair created); exception modes always write that exception.
   */
  const kern = useCallback((left: Glyph, right: Glyph, value: (current: number) => number) => {
    const m = latest.current.mode;
    run(async () => {
      if (m === "group" && left.kernLeft && left.kernRight) return backend.setKerningPair(masterId, left.kernLeft, left.kernRight, value(left.kern));
      return (await backend.setGlyphKerning(masterId, left.name, right.name, m, value(left.kern))).status;
    }, "kerned");
  }, [masterId, run]);

  const setMetric = (glyph: string, metric: MetricName, v: number) => run(async () => (await backend.setMetric(glyph, masterId, metric, v)).status, "spaced");
  const setVertical = (glyph: string, which: "advance" | "tsb" | "bsb", v: number) => run(() => backend.setVerticalMetric(glyph, masterId, which, v), "spaced");
  const setGroups = (glyph: string, l: string | null, r: string | null) => run(() => backend.setGlyphGroups(glyph, l, r));

  // ---------- Geometry ----------

  const metrics = layout?.metrics;
  const scale = size / 1000;
  const pad = 32;
  const minLen = textWidth * scale;
  const asc = metrics?.ascender ?? 800;
  const desc = metrics?.descender ?? -200;
  const lineH = (asc - desc) * 1.25;
  const geom = useMemo(() => layoutRows((layout?.lines ?? []).map((line) => line.map((it): GeomItem => {
    const em = asc - desc;
    if (it.type === "missing") return { adv: MISSING_WIDTH, kern: 0, rtl: it.rtl, vAdv: em, xOff: vertical ? -MISSING_WIDTH / 2 : 0, yOff: vertical ? -asc : 0 };
    return { adv: it.width, kern: vertical ? 0 : it.kern, rtl: it.rtl, vAdv: it.vAdvance, xOff: vertical ? -it.width / 2 : 0, yOff: vertical ? -it.vOrigin : 0 };
  })), { scale, pad, asc, desc, lineH, vertical, paraRtl, ordered: false, minLen }), [layout, scale, asc, desc, lineH, vertical, paraRtl, minLen]);

  // Shaped preview geometry: advances and offsets come from the shaper, not kerning.
  const showShaped = shapeOn && shaped !== null;
  const sMetrics = shaped?.metrics;
  const sAsc = sMetrics?.ascender ?? 800;
  const sDesc = sMetrics?.descender ?? -200;
  const sGeom = useMemo(() => layoutRows((shaped?.lines ?? []).map((line) => line.map((g): GeomItem => ({
    adv: g.advance, kern: 0, rtl: false, vAdv: -g.yAdvance, xOff: g.xOffset, yOff: g.yOffset,
  }))), { scale, pad, asc: sAsc, desc: sDesc, lineH: (sAsc - sDesc) * 1.25, vertical, paraRtl, ordered: true, minLen }), [shaped, scale, sAsc, sDesc, vertical, paraRtl, minLen]);

  const active_ = showShaped ? sGeom : geom;
  const width = active_.width;
  const height = active_.height;
  // Right-to-left and vertical text start at the right edge: keep it in view.
  const canvasRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = canvasRef.current;
    if (el) el.scrollLeft = direction === "ltr" ? 0 : el.scrollWidth;
  }, [direction, width]);

  /** The row nearest the point, by its baseline (or column centre in vertical text). */
  function nearestRow(rows: Row[], x: number, y: number): number {
    let best = 0;
    rows.forEach((r, i) => {
      const d = vertical ? Math.abs(r.base - x) : Math.abs((r.carets[0].y1 + r.carets[0].y2) / 2 - y);
      const b = rows[best];
      const db = vertical ? Math.abs(b.base - x) : Math.abs((b.carets[0].y1 + b.carets[0].y2) / 2 - y);
      if (d < db) best = i;
    });
    return best;
  }

  function placeCursor(e: React.MouseEvent) {
    if (!layout || shapeOn || geom.rows.length === 0) return;
    const r = svgRef.current!.getBoundingClientRect();
    const x = e.clientX - r.left, y = e.clientY - r.top;
    const line = nearestRow(geom.rows, x, y);
    const carets = geom.rows[line].carets;
    let best = 0;
    carets.forEach((c, i) => {
      const along = (k: Caret) => (vertical ? Math.abs(k.y1 - y) : Math.abs(k.x1 - x));
      if (along(c) < along(carets[best])) best = i;
    });
    setCursor({ line, pos: best });
  }

  function glyphAt(e: React.MouseEvent): string | null {
    const r = svgRef.current!.getBoundingClientRect();
    const x = e.clientX - r.left, y = e.clientY - r.top;
    const hit = (rows: Row[]) => {
      for (let li = 0; li < rows.length; li++) {
        const i = rows[li].cells.findIndex((c) => x >= c.box.x && x < c.box.x + c.box.w && y >= c.box.y && y < c.box.y + c.box.h);
        if (i >= 0) return { li, i };
      }
      return null;
    };
    if (showShaped) {
      const h = hit(sGeom.rows);
      return h ? shaped.lines[h.li][h.i].name : null;
    }
    if (!layout) return null;
    const h = hit(geom.rows);
    const it = h ? layout.lines[h.li][h.i] : undefined;
    return it?.type === "glyph" ? it.name : null;
  }

  // ---------- Keyboard ----------

  useEffect(() => {
    if (!active) return;
    const onKey = (e: KeyboardEvent) => {
      // Ctrl+Alt+Shift+←/→/↓ sets the writing direction, also while typing.
      if (e.ctrlKey && e.altKey && e.shiftKey && (e.key === "ArrowLeft" || e.key === "ArrowRight" || e.key === "ArrowDown")) {
        setDirection(e.key === "ArrowLeft" ? "ltr" : e.key === "ArrowRight" ? "rtl" : "vertical");
        e.preventDefault();
        return;
      }
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      if (latest.current.shapeOn) return;
      const { layout, cursor, direction: dir } = latest.current;
      if (!layout) return;
      const line = layout.lines[cursor.line] ?? [];
      if (e.altKey && !e.ctrlKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
        const left = line[cursor.pos - 1], right = line[cursor.pos];
        // Kerning is horizontal; in right-to-left text the next glyph is on the left.
        // With the kerning preview off, the kerning shortcuts are off too.
        if (dir !== "vertical" && !latest.current.kernOff && left?.type === "glyph" && right?.type === "glyph") {
          const step = (e.shiftKey ? 1 : 10) * (e.key === "ArrowLeft" ? -1 : 1) * (dir === "rtl" ? -1 : 1);
          kern(left, right, (v) => v + step);
        }
        e.preventDefault();
        return;
      }
      if (e.ctrlKey || e.altKey) return;
      const move = (c: Cursor) => { setCursor(c); e.preventDefault(); };
      const forward = () => move(cursor.pos < line.length ? { ...cursor, pos: cursor.pos + 1 } : cursor);
      const back = () => move(cursor.pos > 0 ? { ...cursor, pos: cursor.pos - 1 } : cursor);
      const toLine = (d: number) => {
        const l = cursor.line + d;
        if (l >= 0 && l < layout.lines.length) move({ line: l, pos: Math.min(cursor.pos, layout.lines[l].length) });
      };
      if (dir === "vertical") {
        if (e.key === "ArrowDown") forward();
        else if (e.key === "ArrowUp") back();
        else if (e.key === "ArrowLeft") toLine(1);
        else if (e.key === "ArrowRight") toLine(-1);
      } else if (e.key === "ArrowLeft") { if (dir === "rtl") forward(); else back(); }
      else if (e.key === "ArrowRight") { if (dir === "rtl") back(); else forward(); }
      else if (e.key === "ArrowUp") toLine(-1);
      else if (e.key === "ArrowDown") toLine(1);
      if (e.key === "Home") move({ ...cursor, pos: 0 });
      else if (e.key === "End") move({ ...cursor, pos: line.length });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [active, kern]);

  // ---------- Render ----------

  const line = layout?.lines[cursor.line] ?? [];
  const before = line[cursor.pos - 1];
  const after = line[cursor.pos];
  const current: Glyph | undefined = after?.type === "glyph" ? after : before?.type === "glyph" ? before : undefined;
  const ci = current ? line.indexOf(current) : -1;
  const prev = ci > 0 && line[ci - 1].type === "glyph" ? (line[ci - 1] as Glyph) : undefined;
  const next = ci >= 0 && line[ci + 1]?.type === "glyph" ? (line[ci + 1] as Glyph) : undefined;
  const pairLabel = (g?: Glyph) => (g?.kernLeft ? `${g.kernLeft} | ${g.kernRight}` : t("no kerning"));
  // The previous glyph sits to the right in right-to-left text.
  const prevLabel = prev ? (paraRtl ? `${prev.name} ▸` : `◂ ${prev.name}`) : paraRtl ? `${t("kern")} ▸` : `◂ ${t("kern")}`;
  const nextLabel = next ? (paraRtl ? `◂ ${next.name}` : `${next.name} ▸`) : paraRtl ? `◂ ${t("kern")}` : `${t("kern")} ▸`;

  return (
    <div className="textview">
      <div className="toolbar" role="toolbar" aria-label={t("Text tools")}>
        <input className="text-input" value={text} dir="auto" onChange={(e) => setText(e.target.value)} aria-label={t("Sample text")} placeholder={t("Type text, or /glyphname")} spellCheck={false} />
        <select className="select" aria-label={t("Sample strings")} value="" onChange={(e) => e.target.value && setText(e.target.value)}>
          <option value="">{t("Samples…")}</option>
          {samples.map((p, i) => <option key={`${i}:${p}`} value={p}>{p}</option>)}
        </select>
        <span className="toolbar-sep" />
        <div className="segmented dir-seg" role="radiogroup" aria-label={t("Writing direction")}>
          {DIRECTIONS.map((d) => (
            <button key={d.id} role="radio" aria-checked={direction === d.id} aria-label={t(d.label)} title={t(d.title)} className={`segment${direction === d.id ? " is-selected" : ""}`} onClick={() => setDirection(d.id)}>
              <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={d.icon} /></svg>
            </button>
          ))}
        </div>
        <span className="toolbar-sep" />
        <div className="segmented" role="radiogroup" aria-label={t("Kerning mode")}>
          {MODES.map((m) => (
            <button key={m.id} role="radio" aria-checked={mode === m.id} title={t(m.title)} className={`segment${mode === m.id ? " is-selected" : ""}`} onClick={() => setMode(m.id)}>{t(m.label)}</button>
          ))}
        </div>
        <select className="select" aria-label={t("Kerning preview")} value={kernPreview} onChange={(e) => setKernPreview(e.target.value as KerningPreview)}
          title={t("No kerning sets the text without kerning and turns the kerning shortcuts off. Locked keeps the spacing fixed so only kerning changes.")}>
          {PREVIEWS.map((p) => <option key={p.id} value={p.id}>{t(p.label)}</option>)}
        </select>
        <span className="toolbar-sep" />
        <div className="location-picker">
          <select className="select" aria-label={t("Preview location")} value={preview} onChange={(e) => {
            const v = e.target.value;
            if (v === "custom" && info) setCustom(Object.fromEntries(info.axes.map((a) => [a.tag, custom[a.tag] ?? a.default])));
            setPreview(v);
          }}>
            <option value="">{t("Current master")}</option>
            {info?.instances.map((i) => <option key={i.name} value={`i:${i.name}`}>{t("Instance: {name}", { name: i.name })}</option>)}
            {info && info.axes.length > 0 && info.masters.length > 1 && <option value="custom">{t("Custom location…")}</option>}
          </select>
          {preview === "custom" && info?.axes.map((a) => (
            <label key={a.tag} className="axis-slider k-caption" title={a.name}>
              {a.tag}
              <input type="range" min={a.min} max={a.max} step={1} value={custom[a.tag] ?? a.default} onChange={(e) => setCustom({ ...custom, [a.tag]: Number(e.target.value) })} />
              <span className="k-code">{Math.round(custom[a.tag] ?? a.default)}</span>
            </label>
          ))}
        </div>
        <div className="feat-picker" ref={featRef}>
          <button className={`btn btn-secondary btn-sm${shapeOn ? " is-on" : ""}`} aria-expanded={featOpen} aria-haspopup="true" onClick={() => setFeatOpen(!featOpen)}>{t("Features")}</button>
          {featOpen && (
            <div className="feat-pop" role="group" aria-label={t("Feature preview")}>
              {!featView ? (
                <p className="k-caption muted">{t("Feature preview needs the desktop app.")}</p>
              ) : (
                <>
                  <label className="check-row" title={t("Shape the sample text with the font's OpenType features")}>
                    <input type="checkbox" checked={shapeOn} onChange={(e) => setShapeOn(e.target.checked)} />
                    <span className="k-body">{t("Shape preview")}</span>
                  </label>
                  {featTags.length === 0 ? (
                    <p className="k-caption muted">{t("No features in this font yet. Add them in Font Info.")}</p>
                  ) : featTags.map((tag) => (
                    <label key={tag} className="check-row">
                      <input type="checkbox" checked={featChecked(tag)} onChange={(e) => setOverrides({ ...overrides, [tag]: e.target.checked })} />
                      <span className="k-body fea-tag">{tag}</span>
                    </label>
                  ))}
                </>
              )}
            </div>
          )}
        </div>
        <span className="spacer" />
        <label className="check-row k-caption"><input type="checkbox" checked={showMetrics} onChange={(e) => setShowMetrics(e.target.checked)} /> {t("Metrics")}</label>
        <input type="range" min={24} max={320} value={size} onChange={(e) => setSize(Number(e.target.value))} aria-label={t("Text size")} className="size-slider" />
        <span className="k-caption muted zoom">{size} px</span>
      </div>
      <div className="text-stage">
      {!text.trim() && (
        <div className="empty text-empty">
          <span className="text-empty-cursor" aria-hidden="true" />
          <p className="k-subtitle">{t("Type to add glyphs")}</p>
          <p className="k-body muted">{t("Use /glyphname for glyphs without a character. Or start with a sample string:")}</p>
          <div className="text-empty-samples">
            {samples.slice(0, 6).map((p, i) => <button key={`${i}:${p}`} type="button" className="chip-btn" onClick={() => setText(p)}>{p}</button>)}
          </div>
        </div>
      )}
      <div className="text-canvas" ref={canvasRef} hidden={!text.trim()}>
        <svg ref={svgRef} width={width} height={height} style={{ minWidth: "100%" }} data-direction={direction} onMouseDown={placeCursor} onDoubleClick={(e) => { const n = glyphAt(e); if (n) onOpenGlyph(n); }}>
          {!showShaped && layout?.lines.map((ln, li) => {
            const row = geom.rows[li];
            if (!row) return null;
            const by = row.base;
            const pairKern = cursor.line === li && !vertical ? (ln[cursor.pos - 1]?.type === "glyph" ? (ln[cursor.pos - 1] as Glyph) : undefined) : undefined;
            const caret = cursor.line === li ? row.carets[cursor.pos] : undefined;
            return (
              <g key={li}>
                {showMetrics && metrics && !vertical && ([[t("Ascender"), metrics.ascender], [t("x-height"), metrics.xHeight], [t("Baseline"), 0], [t("Descender"), metrics.descender]] as const).map(([label, m]) => (
                  <g key={label}>
                    <line x1={0} x2={Math.max(width, 6000)} y1={by - m * scale} y2={by - m * scale} className={m === 0 ? "metric-line baseline" : "metric-line"} />
                    <text x={paraRtl ? row.start - 24 : row.end + 24} y={by - m * scale - 4} className="metric-label" textAnchor={paraRtl ? "end" : undefined}>{label}</text>
                  </g>
                ))}
                {showMetrics && vertical && <line x1={by} x2={by} y1={0} y2={height} className="metric-line baseline" />}
                {ln.map((it, i) => {
                  const cell = row.cells[i];
                  const b = cell.box;
                  return it.type === "glyph" ? (
                    <g key={i}>
                      {showMetrics && <rect x={b.x} y={b.y} width={b.w} height={b.h} className="sb-box" />}
                      {it.svg ? (<>
                        {it.color?.length ? (
                          <g transform={`translate(${cell.ox} ${cell.oy}) scale(${scale} ${scale})`}><title>{it.name}</title><ColorGlyph layers={it.color} palette={layout?.palette ?? []} /></g>
                        ) : (
                          <path d={it.svg} transform={`translate(${cell.ox} ${cell.oy}) scale(${scale} ${-scale})`} className={it.incompatible ? "text-glyph is-incompatible" : "text-glyph"}><title>{it.incompatible ? t("{name}: masters can't be interpolated (default master shown)", { name: it.name }) : it.name}</title></path>
                        )}
                      </>) : (
                        <text x={b.x + b.w / 2} y={cell.oy} className="text-placeholder" fontSize={size * 0.7}>{it.unicode ? String.fromCodePoint(parseInt(it.unicode, 16)) : "?"}</text>
                      )}
                      {!vertical && it.kern !== 0 && (() => {
                        // A bar over the gap or overlap the kerning makes; exceptions get a marker too.
                        const bar = kernBar(b.x, it.width, it.kern, it.rtl, scale);
                        const exception = it.kernKind === "exception";
                        const value = it.kern > 0 ? `+${it.kern}` : String(it.kern);
                        const pair = it.kernLeft && it.kernRight ? `${it.kernLeft} | ${it.kernRight}` : `${it.name} | ?`;
                        const tip = exception
                          ? t("Exception kerning {value} ({pair}) · click to open it in the Kerning window", { value, pair })
                          : t("Group kerning {value} ({pair}) · click to open it in the Kerning window", { value, pair });
                        const cx = bar.x + bar.w / 2;
                        return (
                          <g className={`kern-bar-group${exception ? " is-exception" : ""}`} role="button" aria-label={tip}
                            onMouseDown={(e) => e.stopPropagation()} onDoubleClick={(e) => e.stopPropagation()}
                            onClick={() => it.kernLeft && it.kernRight && onOpenKerning?.(it.kernLeft, it.kernRight)}>
                            <title>{tip}</title>
                            <rect x={cx - Math.max(bar.w, 16) / 2} y={by + 2} width={Math.max(bar.w, 16)} height={exception ? 36 : 28} className="kern-hit" />
                            <rect x={bar.x} y={by + 4} width={Math.max(bar.w, 2)} height={6} className={`kern-bar ${it.kern < 0 ? "neg" : "pos"}`} />
                            {exception && <path d={`M${cx} ${by + 12}l4 4-4 4-4-4z`} className="kern-exception-mark" />}
                            <text x={cx} y={by + (exception ? 32 : 24)} className={`kern-mark ${it.kern < 0 ? "neg" : "pos"}`}>{value}</text>
                          </g>
                        );
                      })()}
                    </g>
                  ) : (
                    <g key={i}>
                      {vertical
                        ? <rect x={b.x + 4} y={b.y + 4} width={Math.max(0, b.w - 8)} height={Math.max(0, b.h - 8)} className="missing-box" />
                        : <rect x={b.x + 4} y={by - 600 * scale} width={MISSING_WIDTH * scale - 8} height={600 * scale} className="missing-box" />}
                      <text x={b.x + b.w / 2} y={vertical ? b.y + b.h / 2 : by - 260 * scale} className="missing-text">{it.text}</text>
                    </g>
                  );
                })}
                {caret && (
                  <>
                    <line x1={caret.x1} x2={caret.x2} y1={caret.y1} y2={caret.y2} className="text-cursor" />
                    {pairKern && !kernOff && ln[cursor.pos]?.type === "glyph" && (
                      <g transform={`translate(${caret.x1} ${by - desc * scale + 22})`}>
                        {(() => {
                          const value = pairKern.kern === 0 ? "0" : pairKern.kern > 0 ? `+${pairKern.kern}` : String(pairKern.kern);
                          const label = pairKern.kernLeft ? `${value}  ${pairKern.kernLeft} | ${pairKern.kernRight}` : value;
                          const w = Math.max(40, label.length * 7.4 + 16);
                          return <>
                            <rect x={-w / 2} y={-14} width={w} height={22} rx={6} className={`kern-badge${pairKern.kernKind === "exception" ? " is-exception" : ""}`} />
                            <text x={0} y={2} className="kern-badge-text" xmlSpace="preserve">{label}</text>
                          </>;
                        })()}
                      </g>
                    )}
                  </>
                )}
              </g>
            );
          })}
          {showShaped && shaped.lines.map((ln, li) => {
            const row = sGeom.rows[li];
            if (!row) return null;
            const by = row.base;
            return (
              <g key={li}>
                {showMetrics && sMetrics && !vertical && [sMetrics.ascender, sMetrics.xHeight, 0, sMetrics.descender].map((m) => (
                  <line key={m} x1={0} x2={Math.max(width, 6000)} y1={by - m * scale} y2={by - m * scale} className={m === 0 ? "metric-line baseline" : "metric-line"} />
                ))}
                {showMetrics && vertical && <line x1={by} x2={by} y1={0} y2={height} className="metric-line baseline" />}
                {ln.map((g, i) => (
                  <g key={i} transform={`translate(${row.cells[i].ox} ${row.cells[i].oy})`}>
                    <title>{g.name}</title>
                    {g.color?.length ? (
                      <g transform={`scale(${scale} ${scale})`}><ColorGlyph layers={g.color} palette={shaped.palette ?? []} /></g>
                    ) : g.svg ? (
                      <path d={g.svg} transform={`scale(${scale} ${-scale})`} className="text-glyph" />
                    ) : null}
                  </g>
                ))}
              </g>
            );
          })}
        </svg>
      </div>
      {!shapeOn && current && (
        <div className="info-box text-info" role="group" aria-label={t("Spacing")}>
          <div className="info-title">
            <span className="k-body-strong">{current.name}</span>
            <span className="k-code muted">{current.unicode ? `U+${current.unicode}` : ""}</span>
          </div>
          <NumField label={prevLabel} value={prev && !vertical && !kernOff ? prev.kern : null} disabled={!prev || previewing || vertical || kernOff} title={prev ? pairLabel(prev) : undefined}
            onCommit={(v) => prev && kern(prev, current, () => v)} />
          {vertical ? (<>
            <NumField label={t("TSB")} value={current.tsb} disabled={current.tsb === null || previewing || spacingLocked} onCommit={(v) => setVertical(current.name, "tsb", v)} />
            <NumField label={t("Height")} value={current.vAdvance} disabled={previewing || spacingLocked} onCommit={(v) => setVertical(current.name, "advance", v)} />
            <NumField label={t("BSB")} value={current.bsb} disabled={current.bsb === null || previewing || spacingLocked} onCommit={(v) => setVertical(current.name, "bsb", v)} />
          </>) : (<>
            <NumField label={t("LSB")} value={current.lsb} disabled={current.lsb === null || previewing || spacingLocked} onCommit={(v) => setMetric(current.name, "lsb", v)} />
            <NumField label={t("Width")} value={current.width} disabled={previewing || spacingLocked} onCommit={(v) => setMetric(current.name, "width", v)} />
            <NumField label={t("RSB")} value={current.rsb} disabled={current.rsb === null || previewing || spacingLocked} onCommit={(v) => setMetric(current.name, "rsb", v)} />
          </>)}
          <NumField label={nextLabel} value={next && !vertical && !kernOff ? current.kern : null} disabled={!next || previewing || vertical || kernOff} title={next ? pairLabel(current) : undefined}
            onCommit={(v) => next && kern(current, next, () => v)} />
          <span className="info-sep" />
          <GroupField label={t("Left group")} value={current.leftGroup} disabled={previewing} onCommit={(v) => setGroups(current.name, v, current.rightGroup)} />
          <GroupField label={t("Right group")} value={current.rightGroup} disabled={previewing} onCommit={(v) => setGroups(current.name, current.leftGroup, v)} />
        </div>
      )}
      </div>
      <section className={`text-preview-panel${panelOpen ? "" : " is-closed"}`} aria-label={t("Preview")}>
        <div className="row text-preview-head">
          <button type="button" className="transform-head" aria-expanded={panelOpen} onClick={() => setPanelOpen((o) => !o)}>
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round"><path d="M6 9l6 6 6-6" /></svg>
            {t("Preview")}
          </button>
          <span className="spacer" />
          {panelOpen && (
            <label className="k-caption text-preview-size">
              {t("Size")}
              <select className="select" value={panelSize} onChange={(e) => setPanelSize(Number(e.target.value))} aria-label={t("Preview size")}>
                {[24, 36, 48, 64].map((n) => <option key={n} value={n}>{n}</option>)}
              </select>
            </label>
          )}
        </div>
        {panelOpen && (
          <div className="text-preview-rows">
            {panelLayouts.map((row) => {
              const sc = panelSize / 1000;
              const rasc = row.layout.metrics.ascender;
              let x = 0;
              // Lines follow each other in reading order; right-to-left text from the right.
              const lines = paraRtl ? [...row.layout.lines].reverse() : row.layout.lines;
              const items = lines.flatMap((ln, li) => {
                const out = visualOrder(ln.map((it) => it.rtl), paraRtl).map((i) => {
                  const it = ln[i];
                  const kernPx = it.type === "glyph" && !kernOff ? it.kern * sc : 0;
                  if (it.rtl) x += kernPx;
                  const at = x;
                  x += (it.type === "glyph" ? it.width : MISSING_WIDTH) * sc;
                  if (!it.rtl) x += kernPx;
                  return it.type === "glyph" && it.svg ? <path key={`${li}:${i}`} d={it.svg} transform={`translate(${at} ${rasc * sc}) scale(${sc} ${-sc})`} /> : null;
                });
                x += 300 * sc;
                return out;
              });
              return (
                <div key={row.id} className={`text-preview-row${row.id === masterId ? " is-current" : ""}`}>
                  <span className="k-caption text-preview-name">{row.name}</span>
                  <svg width={Math.max(1, x)} height={(rasc - row.layout.metrics.descender) * sc} aria-label={row.name}>{items}</svg>
                </div>
              );
            })}
          </div>
        )}
      </section>
      <footer className="statusbar k-caption text-status" title={t("Click to place the cursor · arrow keys move · Alt+←/→ kerns ±10 (Alt+Shift ±1) · Ctrl+Alt+Shift+←/→/↓ sets the writing direction · double-click a glyph to edit it")}>
        <span>{shapeOn ? t("Feature preview · spacing edits are paused: turn off Shape preview to edit") : previewing ? t("Previewing an interpolated location · spacing edits are paused: choose “Current master” to edit")
          : before?.type === "glyph" && after?.type === "glyph" ? t("Text mode · cursor between {left} | {right}", { left: before.name, right: after.name }) : t("Text mode")}</span>
        <span className="spacer" />
        {!shapeOn && !previewing && !vertical && kernOff && <span>{t("Kerning preview off · kerning shortcuts are off")}</span>}
        {!shapeOn && !previewing && !vertical && !kernOff && before?.type === "glyph" && after?.type === "glyph" && (
          <span>{spacingLocked ? `${t("Spacing locked")} · ` : ""}{before.kernLeft ? t("Kerning {value} ({pair}) · Alt+←/→", { value: before.kern, pair: `${before.kernLeft} | ${before.kernRight}` }) : t("No kerning · Alt+←/→ to kern")}</span>
        )}
      </footer>
    </div>
  );
}
