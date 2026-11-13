import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { backend, type Axis, type ComponentEdit, type ComponentView, type DocStatus, type LayerInfo, type LayerView, type MetricName } from "../lib/backend";
import { LayerDialog, type LayerDialogMode } from "./LayerDialog";
import {
  clonePaths, deleteSelection, ellipsePath, handleOwner, isOn, moveSelection, nodeKey, parseKey, pathsToSvg, rectPath,
  type EditNode, type EditPath,
} from "../lib/outline";
import { CloseIcon, SearchIcon } from "./Icons";

type Tool = "select" | "pen" | "rect" | "ellipse" | "hand";
interface Pt { x: number; y: number }
interface View { s: number; ox: number; oy: number }
/** A component as drawn: the server's outline plus a live offset while dragging. */
interface Comp extends ComponentView { dx: number; dy: number }

const TOOLS: { id: Tool; key: string; label: string; icon: React.ReactNode }[] = [
  { id: "select", key: "v", label: "Select (V)", icon: <path d="M6 3l12 9-6 1-3 6z" /> },
  { id: "pen", key: "p", label: "Pen (P)", icon: <><path d="M12 3l6 10-6 8-6-8z" /><circle cx="12" cy="12" r="1.6" /></> },
  { id: "rect", key: "f", label: "Rectangle (F)", icon: <rect x="5" y="6" width="14" height="12" rx="1" /> },
  { id: "ellipse", key: "e", label: "Ellipse (E)", icon: <ellipse cx="12" cy="12" rx="7.5" ry="6.5" /> },
  { id: "hand", key: "h", label: "Hand (H) · or hold Space", icon: <path d="M8 12V6.5a1.5 1.5 0 0 1 3 0V11V5a1.5 1.5 0 0 1 3 0v6V6.5a1.5 1.5 0 0 1 3 0V14a6 6 0 0 1-6 6h-1a6 6 0 0 1-4.6-2.2L4 15a1.5 1.5 0 0 1 2.2-2z" /> },
];

const HIT = 7; // px

/** Commands sent from the app menus to the active Edit View. */
export type EditCommand = "add-component" | "cut" | "copy" | "paste" | "correct-direction" | "add-extremes" | "remove-overlap";
export const sendEditCommand = (name: EditCommand) => window.dispatchEvent(new CustomEvent("kernest:edit", { detail: name }));

// Clipboard shared by all Edit View tabs; also mirrored to the system clipboard as JSON.
const CLIP_MARK = "kernest-clipboard/1";
let clipboard: { paths: EditPath[]; components: ComponentEdit[] } | null = null;

async function readClipboard(): Promise<typeof clipboard> {
  try {
    const text = await navigator.clipboard.readText();
    const data = JSON.parse(text);
    if (data?.format === CLIP_MARK && Array.isArray(data.paths) && Array.isArray(data.components)) return data;
  } catch {
    // Not our data, or the clipboard is unavailable: fall back to the in-app copy.
  }
  return clipboard;
}

function writeClipboard(data: NonNullable<typeof clipboard>) {
  clipboard = data;
  navigator.clipboard?.writeText(JSON.stringify({ format: CLIP_MARK, ...data })).catch(() => {});
}

const toEdit = (c: Comp): ComponentEdit => {
  const t = [...c.transform];
  t[4] += c.dx;
  t[5] += c.dy;
  return { base: c.base, transform: t };
};
const fromView = (cs: ComponentView[]): Comp[] => cs.map((c) => ({ ...c, dx: 0, dy: 0 }));

function MetricField({ label, value, disabled, onCommit, metricKey, onKey, onInvalid }: { label: string; value: number | null; disabled?: boolean; onCommit: (v: number) => void; metricKey?: string; onKey?: (key: string | null) => void; onInvalid?: (message: string) => void }) {
  const shown = value === null ? "" : String(Math.round(value));
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);
  const commit = (raw: string) => {
    const t = raw.trim();
    // A metrics key such as "=n" or "=|H+10" makes this value follow another glyph.
    if (onKey && t.startsWith("=")) {
      if (t !== metricKey) onKey(t);
      return setText(shown);
    }
    const v = Number(t.replace("−", "-"));
    if (t === "" || raw === shown) return setText(shown);
    if (!Number.isFinite(v)) {
      setText(shown);
      return onInvalid?.(onKey ? `${label}: enter a number, or a metrics key such as =n.` : `${label}: enter a number.`);
    }
    onCommit(v);
  };
  return (
    <label className="info-field" title={onKey ? "Type a number, or a metrics key such as =n, =|H or =o+10" : undefined}>
      <span className="k-caption muted">
        {label}
        {metricKey && (
          <span className="metric-key">
            {" "}{metricKey}
            <button type="button" className="metric-key-clear" aria-label={`Remove the ${label} metrics key`} onClick={(e) => { e.preventDefault(); onKey?.(null); }}>×</button>
          </span>
        )}
      </span>
      <input
        className="metric-input"
        value={text}
        disabled={disabled}
        placeholder="—"
        onChange={(e) => setText(e.target.value)}
        onBlur={(e) => commit(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") (e.target as HTMLInputElement).blur();
          if (e.key === "Escape") { setText(shown); (e.target as HTMLInputElement).blur(); }
        }}
      />
    </label>
  );
}

/** Picker for Glyph › Add Component. */
function ComponentPicker({ names, exclude, onPick, onCancel }: { names: string[]; exclude: string; onPick: (name: string) => void; onCancel: () => void }) {
  const [q, setQ] = useState("");
  const [index, setIndex] = useState(0);
  const shown = useMemo(() => {
    const raw = q.trim();
    const s = raw.toLowerCase();
    // Exact name first (case-sensitive, then not), then prefix matches, then other matches; stable within groups.
    const rank = (n: string) => (n === raw ? 0 : n.toLowerCase() === s ? 1 : n.toLowerCase().startsWith(s) ? 2 : 3);
    return names
      .filter((n) => n !== exclude && (!s || n.toLowerCase().includes(s)))
      .map((n, i) => [n, rank(n), i] as const)
      .sort((a, b) => a[1] - b[1] || a[2] - b[2])
      .map(([n]) => n)
      .slice(0, 200);
  }, [names, exclude, q]);
  useEffect(() => setIndex(0), [q]);
  return (
    <div className="scrim" role="presentation" onMouseDown={(e) => e.target === e.currentTarget && onCancel()}>
      <div className="dialog dialog-sm" role="dialog" aria-modal="true" aria-labelledby="add-comp-title">
        <header className="dialog-header">
          <h2 id="add-comp-title" className="k-subtitle">Add Component</h2>
          <button type="button" className="icon-btn" aria-label="Close" onClick={onCancel}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          <label className="search">
            <SearchIcon />
            <input autoFocus placeholder="Glyph name" value={q} onChange={(e) => setQ(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "ArrowDown") { setIndex((i) => Math.min(i + 1, shown.length - 1)); e.preventDefault(); }
                else if (e.key === "ArrowUp") { setIndex((i) => Math.max(i - 1, 0)); e.preventDefault(); }
                else if (e.key === "Enter" && shown[index]) onPick(shown[index]);
                else if (e.key === "Escape") onCancel();
              }} />
          </label>
          <ul className="picker-list" role="listbox" aria-label="Glyphs">
            {shown.map((n, i) => (
              <li key={n} role="option" aria-selected={i === index}>
                <button className={`list-item${i === index ? " is-selected" : ""}`} onClick={() => onPick(n)} onMouseEnter={() => setIndex(i)}>{n}</button>
              </li>
            ))}
            {shown.length === 0 && <li className="k-caption muted">No glyph matches “{q}”.</li>}
          </ul>
        </div>
      </div>
    </div>
  );
}

interface Props {
  glyph: string;
  masterId: string;
  /** Bumped by the app when the layer changed elsewhere (undo/redo). */
  version: number;
  active: boolean;
  /** All glyph names and widths in the font, for Add Component. */
  glyphs: { name: string; width: number }[];
  onCommitted: (status: DocStatus) => void;
  onError: (e: unknown) => void;
  /** View › Show Metrics: metric lines, labels and the advance box. */
  showMetrics?: boolean;
  /** The current master's id; `masterId` differs when a brace or bracket layer is open. */
  masterKey?: string;
  /** Switches this tab to another layer (a layer key). */
  onPickLayer?: (key: string) => void;
}

/** S05 · Edit View — draw and edit one glyph layer. */
export function EditView({ glyph, masterId, version, active, glyphs, onCommitted, onError, showMetrics = true, masterKey, onPickLayer }: Props) {
  const master = masterKey ?? masterId;
  // ---------- Layers (master, brace, bracket) ----------
  const [layerList, setLayerList] = useState<LayerInfo[]>([]);
  const [layerDialog, setLayerDialog] = useState<{ mode: LayerDialogMode; axes: Axis[] } | null>(null);
  const [layersVersion, setLayersVersion] = useState(0);
  useEffect(() => {
    backend.glyphLayers(glyph).then(setLayerList, () => setLayerList([]));
  }, [glyph, version, masterId, layersVersion]);
  const currentLayer = layerList.find((l) => l.key === masterId);
  const openLayerDialog = useCallback(async (kind: "brace" | "bracket", editing: boolean) => {
    try {
      const info = await backend.fontInfo();
      if (!info.axes.length) return onError("Add an axis in Font Info first: intermediate and alternate layers are positions on the axes.");
      const here = info.masters.find((m) => m.id === master)?.location ?? {};
      const mode: LayerDialogMode = kind === "brace"
        ? { kind, editing, initial: editing && currentLayer?.location ? currentLayer.location : here }
        : { kind, editing, initial: editing && currentLayer?.bracket ? currentLayer.bracket : [] };
      setLayerDialog({ mode, axes: info.axes });
    } catch (e) {
      onError(e);
    }
  }, [master, currentLayer, onError]);
  const saveLayerDialog = useCallback(async (v: { location?: Record<string, number>; ranges?: { tag: string; min?: number | null; max?: number | null }[] }) => {
    if (!layerDialog) return;
    const { mode } = layerDialog;
    const r = mode.editing
      ? await backend.updateLayerPosition(glyph, masterId, v.location, v.ranges)
      : mode.kind === "brace"
        ? await backend.addBraceLayer(glyph, master, v.location ?? {})
        : await backend.addBracketLayers(glyph, master, v.ranges ?? []);
    setLayerDialog(null);
    onCommitted(r.status);
    setLayersVersion((n) => n + 1);
    onPickLayer?.(r.key);
  }, [layerDialog, glyph, masterId, master, onCommitted, onPickLayer]);
  const removeCurrentLayer = useCallback(async () => {
    try {
      const s = await backend.removeLayer(glyph, masterId);
      onCommitted(s);
      setLayersVersion((n) => n + 1);
      onPickLayer?.(master);
    } catch (e) {
      onError(e);
    }
  }, [glyph, masterId, master, onCommitted, onPickLayer, onError]);

  const [layer, setLayer] = useState<LayerView | null>(null);
  const [paths, setPaths] = useState<EditPath[]>([]);
  const [comps, setComps] = useState<Comp[]>([]);
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const [compSel, setCompSel] = useState<Set<number>>(new Set());
  const [tool, setTool] = useState<Tool>("select");
  const [view, setView] = useState<View>({ s: 0.5, ox: 0, oy: 0 });
  const [size, setSize] = useState({ w: 800, h: 600 });
  const [cursor, setCursor] = useState<Pt | null>(null);
  const [marquee, setMarquee] = useState<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  const [preview, setPreview] = useState<EditPath | null>(null);
  const [spaceDown, setSpaceDown] = useState(false);
  const [picking, setPicking] = useState(false);
  const [, rerender] = useState(0);

  const wrapRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const fitted = useRef(false);
  const drag = useRef<null | { mode: string; startScreen: Pt; startWorld: Pt; base: EditPath[]; baseComps: Comp[]; moved: boolean; additive?: boolean; pi?: number; ni?: number; startView?: View; lastPaths?: EditPath[]; lastComps?: Comp[] }>(null);
  const pen = useRef<{ path: number | null; pendingOut: Pt | null }>({ path: null, pendingOut: null });
  const chain = useRef(Promise.resolve());
  const latest = useRef({ layer, paths, comps, selection, compSel, view });
  latest.current = { layer, paths, comps, selection, compSel, view };

  // ---------- Loading & committing ----------

  useEffect(() => {
    let cancelled = false;
    backend.glyphLayer(glyph, masterId).then((l) => {
      if (cancelled) return;
      setLayer(l);
      setPaths(l.paths);
      setComps(fromView(l.components));
      setSelection((sel) => new Set([...sel].filter((k) => { const [pi, ni] = parseKey(k); return !!l.paths[pi]?.nodes[ni]; })));
      setCompSel((sel) => new Set([...sel].filter((i) => i < l.components.length)));
    }, (e) => {
      if (cancelled) return;
      // A brace or bracket layer that no longer exists (undo, removed elsewhere): go back
      // to the master's own layer.
      if (masterId !== master && onPickLayer) onPickLayer(master);
      else onError(e);
    });
    return () => { cancelled = true; };
  }, [glyph, masterId, version, onError, master, onPickLayer]);

  const accept = useCallback((l: LayerView) => {
    setLayer(l);
    setPaths(l.paths);
    setComps(fromView(l.components));
    onCommitted(l.status);
  }, [onCommitted]);

  /** Sends the edited outlines (and components) to the backend as one undoable step. */
  const commit = useCallback((next: EditPath[], nextComps?: Comp[], width?: number) => {
    chain.current = chain.current.then(async () => {
      const l = latest.current.layer;
      if (!l) return;
      const cs = (nextComps ?? latest.current.comps).map(toEdit);
      try {
        accept(await backend.updateLayer(glyph, masterId, width ?? l.width, next, cs));
      } catch (e) {
        setPaths(l.paths);
        setComps(fromView(l.components));
        onError(e);
      }
    });
  }, [glyph, masterId, accept, onError]);

  const setMetricsKey = useCallback((metric: MetricName, key: string | null) => {
    chain.current = chain.current.then(async () => {
      try {
        accept(await backend.setMetricsKey(glyph, masterId, metric, key));
      } catch (e) {
        onError(e);
      }
    });
  }, [glyph, masterId, accept, onError]);

  const setMetric = useCallback((metric: MetricName, value: number) => {
    chain.current = chain.current.then(async () => {
      try {
        // Typing a number replaces the metrics key for that value.
        const keys = latest.current.layer?.metricsKeys;
        const has = metric === "lsb" ? keys?.left : metric === "rsb" ? keys?.right : keys?.width;
        if (has) accept(await backend.setMetricsKey(glyph, masterId, metric, null));
        accept(await backend.setMetric(glyph, masterId, metric, value));
      } catch (e) {
        onError(e);
      }
    });
  }, [glyph, masterId, accept, onError]);

  // ---------- View ----------

  const loaded = layer !== null;
  const measured = useRef<{ w: number; h: number } | null>(null);
  useLayoutEffect(() => {
    // The canvas container only exists once the layer has loaded.
    const el = wrapRef.current;
    if (!el || !loaded) return;
    const measure = () => {
      const r = el.getBoundingClientRect();
      if (r.width < 10) return; // hidden tab
      measured.current = { w: r.width, h: r.height };
      setSize((s) => (s.w === r.width && s.h === r.height ? s : { w: r.width, h: r.height }));
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    window.addEventListener("resize", measure);
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", measure);
    };
  }, [loaded, active]);

  const fit = useCallback(() => {
    const l = latest.current.layer;
    const m = measured.current;
    if (!l || !m) return;
    const { ascender, descender } = l.metrics;
    const h = (ascender - descender) * 1.3;
    const w = Math.max(l.width, 200) * 1.6;
    const s = Math.min(m.h / h, m.w / w);
    setView({ s, ox: m.w / 2 - (l.width * s) / 2, oy: m.h / 2 + ((ascender + descender) / 2) * s });
  }, []);

  useEffect(() => {
    if (layer && !fitted.current && measured.current) {
      fitted.current = true;
      fit();
    }
  }, [layer, size, fit]);

  const toScreen = (x: number, y: number): Pt => ({ x: view.ox + x * view.s, y: view.oy - y * view.s });
  const toWorld = (sx: number, sy: number): Pt => ({ x: (sx - view.ox) / view.s, y: (view.oy - sy) / view.s });
  const local = (e: { clientX: number; clientY: number }): Pt => {
    const r = svgRef.current!.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };
  const round = (p: Pt): Pt => ({ x: Math.round(p.x), y: Math.round(p.y) });

  const zoomAt = useCallback((factor: number, sx: number, sy: number) => {
    setView((v) => {
      const s = Math.min(40, Math.max(0.02, v.s * factor));
      return { s, ox: sx - ((sx - v.ox) * s) / v.s, oy: sy - ((sy - v.oy) * s) / v.s };
    });
  }, []);

  useEffect(() => {
    // Attached once the canvas exists (after the layer loads).
    const el = svgRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const r = el.getBoundingClientRect();
      if (e.ctrlKey || e.altKey) zoomAt(Math.pow(1.0015, -e.deltaY), e.clientX - r.left, e.clientY - r.top);
      else if (e.shiftKey) setView((v) => ({ ...v, ox: v.ox - e.deltaY }));
      else setView((v) => ({ ...v, ox: v.ox - e.deltaX, oy: v.oy - e.deltaY }));
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [zoomAt, loaded]);

  // ---------- Hit testing ----------

  function hitNode(sp: Pt): string | null {
    let best: string | null = null;
    let bestD = HIT;
    paths.forEach((p, pi) => p.nodes.forEach((n, ni) => {
      const q = toScreen(n.x, n.y);
      const d = Math.hypot(q.x - sp.x, q.y - sp.y) - (isOn(n) ? 0.5 : 0);
      if (d <= bestD) { bestD = d; best = nodeKey(pi, ni); }
    }));
    return best;
  }

  /** Topmost component whose filled outline contains the point. */
  function hitComponent(wp: Pt): number | null {
    const els = svgRef.current?.querySelectorAll<SVGPathElement>("path[data-comp]") ?? [];
    for (let i = els.length - 1; i >= 0; i--) {
      const idx = Number(els[i].dataset.comp);
      const c = comps[idx];
      if (c && els[i].isPointInFill(new DOMPoint(wp.x - c.dx, wp.y - c.dy))) return idx;
    }
    return null;
  }

  // ---------- Pointer interaction ----------

  const activeTool: Tool = spaceDown ? "hand" : tool;

  function onPointerDown(e: React.PointerEvent<SVGSVGElement>) {
    if (!layer) return;
    svgRef.current!.setPointerCapture(e.pointerId);
    const sp = local(e);
    const wp = toWorld(sp.x, sp.y);
    const base = clonePaths(paths);
    const baseComps = comps.map((c) => ({ ...c }));
    if (e.button === 1 || activeTool === "hand") {
      drag.current = { mode: "pan", startScreen: sp, startWorld: wp, base, baseComps, moved: false, startView: view };
      return;
    }
    if (e.button !== 0) return;

    if (activeTool === "select") {
      const hit = hitNode(sp);
      const comp = hit ? null : hitComponent(wp);
      if (hit) {
        let sel = selection;
        if (e.shiftKey) {
          sel = new Set(selection);
          if (sel.has(hit)) sel.delete(hit); else sel.add(hit);
        } else if (!selection.has(hit)) {
          sel = new Set([hit]);
          setCompSel(new Set());
        }
        setSelection(sel);
        drag.current = { mode: "move", startScreen: sp, startWorld: wp, base, baseComps, moved: false };
      } else if (comp !== null) {
        let sel = compSel;
        if (e.shiftKey) {
          sel = new Set(compSel);
          if (sel.has(comp)) sel.delete(comp); else sel.add(comp);
        } else if (!compSel.has(comp)) {
          sel = new Set([comp]);
          setSelection(new Set());
        }
        setCompSel(sel);
        drag.current = { mode: "move", startScreen: sp, startWorld: wp, base, baseComps, moved: false };
      } else {
        if (!e.shiftKey) { setSelection(new Set()); setCompSel(new Set()); }
        drag.current = { mode: "marquee", startScreen: sp, startWorld: wp, base, baseComps, moved: false, additive: e.shiftKey };
      }
      return;
    }

    if (activeTool === "rect" || activeTool === "ellipse") {
      drag.current = { mode: "shape", startScreen: sp, startWorld: round(wp), base, baseComps, moved: false };
      return;
    }

    if (activeTool === "pen") {
      const p = round(wp);
      const pn = pen.current;
      const open = pn.path !== null && base[pn.path] && !base[pn.path].closed ? base[pn.path] : null;
      if (open) {
        const first = open.nodes[0];
        const fs = toScreen(first.x, first.y);
        if (open.nodes.filter(isOn).length >= 2 && Math.hypot(fs.x - sp.x, fs.y - sp.y) <= HIT + 1) {
          // Close the path on its first point.
          if (pn.pendingOut) {
            open.nodes.push(off(pn.pendingOut), off(first));
            first.kind = "curve";
          } else first.kind = "line";
          open.closed = true;
          pen.current = { path: null, pendingOut: null };
          setPaths(base);
          commit(base);
          rerender((n) => n + 1);
          return;
        }
        if (pn.pendingOut) open.nodes.push(off(pn.pendingOut), off(p), { ...p, kind: "curve", smooth: false });
        else open.nodes.push({ ...p, kind: "line", smooth: false });
        pn.pendingOut = null;
        drag.current = { mode: "pen", startScreen: sp, startWorld: p, base: clonePaths(base), baseComps, moved: false, pi: pn.path!, ni: open.nodes.length - 1 };
      } else {
        base.push({ closed: false, nodes: [{ ...p, kind: "line", smooth: false }] });
        pen.current = { path: base.length - 1, pendingOut: null };
        drag.current = { mode: "pen", startScreen: sp, startWorld: p, base: clonePaths(base), baseComps, moved: false, pi: base.length - 1, ni: 0 };
      }
      setPaths(base);
      setSelection(new Set());
      setCompSel(new Set());
    }
  }

  /** Rectangle or ellipse from drag start `a` to `b` (Shift makes it square). */
  const shapeFor = (a: Pt, b0: Pt, square: boolean): EditPath => {
    let b = b0;
    if (square) {
      const side = Math.max(Math.abs(b.x - a.x), Math.abs(b.y - a.y));
      b = { x: a.x + Math.sign(b.x - a.x || 1) * side, y: a.y + Math.sign(b.y - a.y || 1) * side };
    }
    return tool === "ellipse" ? ellipsePath(a.x, a.y, b.x, b.y) : rectPath(a.x, a.y, b.x, b.y);
  };

  function onPointerMove(e: React.PointerEvent<SVGSVGElement>) {
    const sp = local(e);
    const wp = toWorld(sp.x, sp.y);
    setCursor(round(wp));
    const d = drag.current;
    if (!d) return;
    if (!d.moved && Math.hypot(sp.x - d.startScreen.x, sp.y - d.startScreen.y) < 3) return;
    d.moved = true;

    if (d.mode === "pan" && d.startView) {
      setView({ ...d.startView, ox: d.startView.ox + sp.x - d.startScreen.x, oy: d.startView.oy + sp.y - d.startScreen.y });
    } else if (d.mode === "move") {
      let dx = Math.round(wp.x - d.startWorld.x), dy = Math.round(wp.y - d.startWorld.y);
      if (e.shiftKey) { if (Math.abs(dx) > Math.abs(dy)) dy = 0; else dx = 0; }
      d.lastPaths = moveSelection(d.base, selection, dx, dy);
      d.lastComps = d.baseComps.map((c, i) => (compSel.has(i) ? { ...c, dx: c.dx + dx, dy: c.dy + dy } : c));
      setPaths(d.lastPaths);
      setComps(d.lastComps);
    } else if (d.mode === "marquee") {
      setMarquee({ x0: d.startScreen.x, y0: d.startScreen.y, x1: sp.x, y1: sp.y });
    } else if (d.mode === "shape") {
      setPreview(shapeFor(d.startWorld, round(wp), e.shiftKey));
    } else if (d.mode === "pen" && d.pi !== undefined && d.ni !== undefined) {
      const o = d.startWorld;
      const v = { x: Math.round(wp.x - o.x), y: Math.round(wp.y - o.y) };
      const next = clonePaths(d.base);
      const p = next[d.pi];
      let ni = d.ni;
      const node = p.nodes[ni];
      node.smooth = true;
      const inHandle = { x: o.x - v.x, y: o.y - v.y };
      if (ni > 0) {
        if (node.kind === "curve") p.nodes[ni - 1] = off(inHandle);
        else {
          const prev = p.nodes[ni - 1];
          p.nodes.splice(ni, 0, off(prev), off(inHandle));
          ni += 2;
          p.nodes[ni].kind = "curve";
        }
      }
      pen.current.pendingOut = { x: o.x + v.x, y: o.y + v.y };
      d.lastPaths = next;
      setPaths(next);
    }
  }

  function onPointerUp(e: React.PointerEvent<SVGSVGElement>) {
    const d = drag.current;
    drag.current = null;
    if (!d) return;
    const sp = local(e);
    // Commit what the last pointer move computed (state may not have re-rendered yet).
    if (d.mode === "move" && d.moved) commit(d.lastPaths ?? latest.current.paths, d.lastComps ?? latest.current.comps);
    else if (d.mode === "marquee" && d.moved) {
      // Use the drag's own start point: state may not have re-rendered yet on a fast drag.
      const marquee = { x0: d.startScreen.x, y0: d.startScreen.y };
      const [x0, x1] = [Math.min(marquee.x0, sp.x), Math.max(marquee.x0, sp.x)];
      const [y0, y1] = [Math.min(marquee.y0, sp.y), Math.max(marquee.y0, sp.y)];
      const sel = new Set(d.additive ? selection : []);
      paths.forEach((p, pi) => p.nodes.forEach((n, ni) => {
        const q = toScreen(n.x, n.y);
        if (q.x >= x0 && q.x <= x1 && q.y >= y0 && q.y <= y1) sel.add(nodeKey(pi, ni));
      }));
      setSelection(sel);
      // Components are selected when their whole outline is inside the box.
      const cs = new Set(d.additive ? compSel : []);
      svgRef.current?.querySelectorAll<SVGPathElement>("path[data-comp]").forEach((el) => {
        const r = el.getBoundingClientRect(), s = svgRef.current!.getBoundingClientRect();
        if (r.width && r.left - s.left >= x0 && r.right - s.left <= x1 && r.top - s.top >= y0 && r.bottom - s.top <= y1) cs.add(Number(el.dataset.comp));
      });
      setCompSel(cs);
      setMarquee(null);
    } else if (d.mode === "shape") {
      const shape = d.moved ? shapeFor(d.startWorld, round(toWorld(sp.x, sp.y)), e.shiftKey) : null;
      if (shape) {
        const next = [...latest.current.paths, shape];
        setPaths(next);
        commit(next);
        setSelection(new Set(shape.nodes.map((_, ni) => nodeKey(next.length - 1, ni))));
        setCompSel(new Set());
      }
      setPreview(null);
    } else if (d.mode === "pen") {
      commit(d.lastPaths ?? d.base);
      rerender((n) => n + 1);
    }
    setMarquee(null);
  }

  // ---------- Components & clipboard ----------

  const addComponent = useCallback((base: string) => {
    setPicking(false);
    const { paths, comps, layer } = latest.current;
    if (!layer) return;
    const next = [...comps, { base, transform: [1, 0, 0, 1, 0, 0], svg: "", dx: 0, dy: 0 }];
    // An empty glyph takes the width of its first component (e.g. building Á from A).
    const empty = paths.length === 0 && comps.length === 0;
    const baseWidth = glyphs.find((g) => g.name === base)?.width;
    commit(paths, next, empty && baseWidth !== undefined ? baseWidth : undefined);
    setSelection(new Set());
    setCompSel(new Set([next.length - 1]));
  }, [commit, glyphs]);

  /** Whole paths that contain a selected point, plus selected components. */
  const selectedContent = useCallback(() => {
    const { paths, comps, selection, compSel } = latest.current;
    const pathIdx = new Set([...selection].map((k) => parseKey(k)[0]));
    return {
      pathIdx,
      data: { paths: clonePaths(paths.filter((_, i) => pathIdx.has(i))), components: comps.filter((_, i) => compSel.has(i)).map(toEdit) },
    };
  }, []);

  const runCommand = useCallback(async (name: EditCommand) => {
    if (name === "add-component") return setPicking(true);
    if (name === "correct-direction" || name === "add-extremes" || name === "remove-overlap") {
      // Add Extremes works on the selected paths, or on every path when none is selected.
      const only = name === "add-extremes" ? [...new Set([...latest.current.selection].map((k) => parseKey(k)[0]))] : undefined;
      chain.current = chain.current.then(async () => {
        try {
          accept(await backend.pathOperation(glyph, masterId, name, only));
        } catch (e) {
          onError(e);
        }
      });
      return;
    }
    const { pathIdx, data } = selectedContent();
    if (name === "copy" || name === "cut") {
      if (!data.paths.length && !data.components.length) return;
      writeClipboard(data);
      if (name === "cut") {
        const { paths, comps, compSel } = latest.current;
        const nextPaths = paths.filter((_, i) => !pathIdx.has(i));
        const nextComps = comps.filter((_, i) => !compSel.has(i));
        setPaths(nextPaths);
        setComps(nextComps);
        setSelection(new Set());
        setCompSel(new Set());
        pen.current = { path: null, pendingOut: null };
        commit(nextPaths, nextComps);
      }
      return;
    }
    if (name === "paste") {
      const clip = await readClipboard();
      if (!clip) return;
      const { paths, comps } = latest.current;
      const nextPaths = [...paths, ...clonePaths(clip.paths)];
      const nextComps = [...comps, ...clip.components.map((c) => ({ ...c, transform: [...c.transform], svg: "", dx: 0, dy: 0 }))];
      setPaths(nextPaths);
      setSelection(new Set(clip.paths.flatMap((p, i) => p.nodes.map((_, ni) => nodeKey(paths.length + i, ni)))));
      setCompSel(new Set(clip.components.map((_, i) => comps.length + i)));
      commit(nextPaths, nextComps);
    }
  }, [commit, selectedContent, accept, glyph, masterId, onError]);

  useEffect(() => {
    if (!active) return;
    const onCmd = (e: Event) => runCommand((e as CustomEvent<EditCommand>).detail);
    window.addEventListener("kernest:edit", onCmd);
    return () => window.removeEventListener("kernest:edit", onCmd);
  }, [active, runCommand]);

  // ---------- Keyboard ----------

  useEffect(() => {
    if (!active || picking) return;
    const typing = (t: EventTarget | null) => t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement;
    const onDown = (e: KeyboardEvent) => {
      if (typing(e.target)) return;
      const { paths, comps, selection, compSel } = latest.current;
      if (e.key === " ") { setSpaceDown(true); e.preventDefault(); return; }
      if (e.ctrlKey && !e.altKey) {
        const k = e.key.toLowerCase();
        if (k === "a") {
          setSelection(new Set(paths.flatMap((p, pi) => p.nodes.map((_, ni) => nodeKey(pi, ni)))));
          setCompSel(new Set(comps.map((_, i) => i)));
        } else if (k === "0") fit();
        else if (k === "=" || k === "+") zoomAt(1.25, size.w / 2, size.h / 2);
        else if (k === "-") zoomAt(0.8, size.w / 2, size.h / 2);
        else if (k === "c" && !e.shiftKey) runCommand("copy");
        else if (k === "x" && !e.shiftKey) runCommand("cut");
        else if (k === "v" && !e.shiftKey) runCommand("paste");
        else if (k === "c" && e.shiftKey) runCommand("add-component");
        else return;
        e.preventDefault();
        return;
      }
      if (e.altKey) return;
      const nudge: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, 1], ArrowDown: [0, -1] };
      if (nudge[e.key] && (selection.size || compSel.size)) {
        const m = e.shiftKey ? 10 : 1;
        const [dx, dy] = [nudge[e.key][0] * m, nudge[e.key][1] * m];
        const next = moveSelection(paths, selection, dx, dy);
        const nextComps = comps.map((c, i) => (compSel.has(i) ? { ...c, dx: c.dx + dx, dy: c.dy + dy } : c));
        setPaths(next);
        setComps(nextComps);
        commit(next, nextComps);
        e.preventDefault();
        return;
      }
      if (!e.ctrlKey && (e.key === "Delete" || e.key === "Backspace") && (selection.size || compSel.size)) {
        const next = deleteSelection(paths, selection);
        const nextComps = comps.filter((_, i) => !compSel.has(i));
        setSelection(new Set());
        setCompSel(new Set());
        setPaths(next);
        setComps(nextComps);
        commit(next, nextComps);
        pen.current = { path: null, pendingOut: null };
        e.preventDefault();
        return;
      }
      if (e.key === "Enter" && selection.size) {
        // Toggle smooth / corner on selected curve points.
        const next = clonePaths(paths);
        let changed = false;
        for (const k of selection) {
          const [pi, ni] = parseKey(k);
          const n = next[pi]?.nodes[ni];
          if (n && isOn(n)) { n.smooth = !n.smooth; changed = true; }
        }
        if (changed) { setPaths(next); commit(next); }
        e.preventDefault();
        return;
      }
      if (e.key === "Escape") {
        if (pen.current.path !== null) pen.current = { path: null, pendingOut: null };
        else { setSelection(new Set()); setCompSel(new Set()); }
        rerender((n) => n + 1);
        return;
      }
      const t = TOOLS.find((x) => x.key === e.key.toLowerCase());
      if (t && !e.shiftKey) {
        setTool(t.id);
        pen.current = { path: null, pendingOut: null };
      }
    };
    const onUp = (e: KeyboardEvent) => { if (e.key === " ") setSpaceDown(false); };
    window.addEventListener("keydown", onDown);
    window.addEventListener("keyup", onUp);
    return () => {
      window.removeEventListener("keydown", onDown);
      window.removeEventListener("keyup", onUp);
    };
  }, [active, picking, commit, fit, zoomAt, size, runCommand]);

  // ---------- Rendering ----------

  if (!layer) return <div className="editor"><div className="editor-loading k-caption muted">Loading {glyph}…</div></div>;

  const { ascender, capHeight, xHeight, descender } = layer.metrics;
  const lines: [string, number][] = [["Ascender", ascender], ["Cap height", capHeight], ["x-height", xHeight], ["Baseline", 0], ["Descender", descender]];
  const x0 = toScreen(0, 0).x, x1 = toScreen(layer.width, 0).x;
  const outline = pathsToSvg(paths);
  const one = selection.size === 1 && compSel.size === 0 ? parseKey([...selection][0]) : null;
  const oneNode = one ? paths[one[0]]?.nodes[one[1]] : undefined;
  const oneComp = compSel.size === 1 && selection.size === 0 ? comps[[...compSel][0]] : undefined;
  const pendingOut = pen.current.path !== null ? pen.current.pendingOut : null;
  const penPath = pen.current.path !== null ? paths[pen.current.path] : undefined;
  const lastPen = penPath && !penPath.closed ? penPath.nodes[penPath.nodes.length - 1] : undefined;
  const cursorStyle = activeTool === "hand" ? (drag.current?.mode === "pan" ? "grabbing" : "grab") : activeTool === "select" ? "default" : "crosshair";
  const moveComp = (i: number, dx: number, dy: number) => {
    const next = comps.map((c, j) => (j === i ? { ...c, dx: c.dx + dx, dy: c.dy + dy } : c));
    setComps(next);
    commit(paths, next);
  };

  return (
    <div className="editor">
      <div className="toolbar" role="toolbar" aria-label="Tools">
        {TOOLS.map((t) => (
          <button key={t.id} className={`tool${tool === t.id ? " is-selected" : ""}`} title={t.label} aria-label={t.label} aria-pressed={tool === t.id}
            onClick={() => { setTool(t.id); pen.current = { path: null, pendingOut: null }; }}>
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinejoin="round">{t.icon}</svg>
          </button>
        ))}
        <span className="toolbar-sep" />
        <button className="btn btn-secondary btn-sm" onClick={() => setPicking(true)} title="Add Component (Ctrl+Shift+C)">Add Component</button>
        <span className="toolbar-sep" />
        <label className="layer-picker" title="Intermediate (brace) and alternate (bracket) layers of this glyph">
          <span className="k-caption muted">Layer</span>
          <select className="text-input layer-select" value={masterId} onChange={(e) => onPickLayer?.(e.target.value)} aria-label="Layer">
            {layerList
              .filter((l) => (l.kind === "master" ? l.key === master : l.kind === "brace" || l.masterId === master))
              .map((l) => (
                <option key={l.key} value={l.key}>
                  {l.kind === "master" ? `${l.label} (master)` : l.kind === "brace" ? `Intermediate ${l.label}` : `Alternate ${l.label}`}
                </option>
              ))}
          </select>
        </label>
        <button className="btn btn-secondary btn-sm" onClick={() => openLayerDialog("brace", false)} title="Add an intermediate (brace) layer">+ Intermediate</button>
        <button className="btn btn-secondary btn-sm" onClick={() => openLayerDialog("bracket", false)} title="Add an alternate design (bracket layers)">+ Alternate</button>
        {currentLayer && currentLayer.kind !== "master" && (
          <>
            <button className="btn btn-secondary btn-sm" onClick={() => openLayerDialog(currentLayer.kind as "brace" | "bracket", true)}>{currentLayer.kind === "brace" ? "Move…" : "Ranges…"}</button>
            <button className="btn btn-secondary btn-sm" onClick={removeCurrentLayer} title={currentLayer.kind === "bracket" ? "Removes this alternate from every master" : "Removes this intermediate layer"}>Remove</button>
          </>
        )}
        <span className="toolbar-hint k-caption muted">{hint(activeTool)}</span>
        <span className="spacer" />
        <span className="k-caption muted">{cursor ? `${cursor.x}, ${cursor.y}` : ""}</span>
        <button className="btn btn-secondary btn-sm" onClick={fit} title="Fit glyph (Ctrl+0)">Fit</button>
        <span className="k-caption muted zoom">{Math.round(view.s * 100)}%</span>
      </div>
      <div className="canvas-wrap" ref={wrapRef}>
        <svg ref={svgRef} className="canvas" width={size.w} height={size.h} style={{ cursor: cursorStyle }}
          onPointerDown={onPointerDown} onPointerMove={onPointerMove} onPointerUp={onPointerUp} onPointerLeave={() => setCursor(null)}
          onDoubleClick={(e) => {
            if (tool !== "select") return;
            const hit = hitNode(local(e));
            if (!hit) return;
            const [pi] = parseKey(hit);
            setSelection(new Set(paths[pi].nodes.map((_, ni) => nodeKey(pi, ni))));
          }}>
          {/* Metric lines */}
          {showMetrics && lines.map(([label, y]) => {
            const sy = toScreen(0, y).y;
            return (
              <g key={label}>
                <line x1={0} x2={size.w} y1={sy} y2={sy} className={y === 0 ? "metric-line baseline" : "metric-line"} />
                <text x={x1 + 8} y={sy - 4} className="metric-label">{label} {Math.round(y)}</text>
              </g>
            );
          })}
          {showMetrics && <rect x={x0} y={toScreen(0, ascender).y} width={Math.max(0, x1 - x0)} height={(ascender - descender) * view.s} className="advance-box" />}
          {/* Glyph */}
          <g transform={`translate(${view.ox} ${view.oy}) scale(${view.s} ${-view.s})`}>
            {comps.map((c, i) => (
              <g key={`c${i}`} transform={`translate(${c.dx} ${c.dy})`}>
                <path d={c.svg} data-comp={i} className={`component-fill${compSel.has(i) ? " is-selected" : ""}`} />
              </g>
            ))}
            <path d={outline} className="glyph-fill" fillRule="nonzero" />
            {preview && <path d={pathsToSvg([preview])} className="preview-outline" />}
          </g>
          {/* Component labels */}
          {comps.map((c, i) => {
            const at = toScreen(c.transform[4] + c.dx, c.transform[5] + c.dy);
            return compSel.has(i) ? <text key={`cl${i}`} x={at.x + 4} y={at.y + 14} className="component-label">{c.base}</text> : null;
          })}
          {/* Handles and nodes */}
          {paths.map((p, pi) => p.nodes.map((n, ni) => {
            if (isOn(n)) return null;
            const owner = handleOwner(p, ni);
            if (owner < 0) return null;
            const a = toScreen(n.x, n.y), b = toScreen(p.nodes[owner].x, p.nodes[owner].y);
            return <line key={`h${pi}:${ni}`} x1={a.x} y1={a.y} x2={b.x} y2={b.y} className="handle-line" />;
          }))}
          {pendingOut && lastPen && (() => {
            const a = toScreen(lastPen.x, lastPen.y), b = toScreen(pendingOut.x, pendingOut.y);
            return <g><line x1={a.x} y1={a.y} x2={b.x} y2={b.y} className="handle-line" /><circle cx={b.x} cy={b.y} r={3} className="node-off" /></g>;
          })()}
          {paths.map((p, pi) => p.nodes.map((n, ni) => <NodeMark key={`n${pi}:${ni}`} n={n} at={toScreen(n.x, n.y)} selected={selection.has(nodeKey(pi, ni))} first={ni === 0 && !p.closed} />))}
          {marquee && <rect className="marquee" x={Math.min(marquee.x0, marquee.x1)} y={Math.min(marquee.y0, marquee.y1)} width={Math.abs(marquee.x1 - marquee.x0)} height={Math.abs(marquee.y1 - marquee.y0)} />}
        </svg>
        {layer.issues.length > 0 && (
          <div className="notice notice-warning issues-banner k-label" role="status">
            ⚠ This glyph's masters can't be interpolated:
            <ul>{layer.issues.map((i) => <li key={i}>{i}</li>)}</ul>
          </div>
        )}
        {paths.length === 0 && comps.length === 0 && !preview && (
          <div className="canvas-empty">
            <p className="k-subtitle">Empty glyph</p>
            <p className="k-caption muted">Press P to draw with the pen, F for a rectangle, or Ctrl+Shift+C to add a component.</p>
          </div>
        )}
        <div className="info-box" role="group" aria-label="Glyph info">
          <div className="info-title">
            <span className="k-body-strong">{layer.glyph}</span>
            <span className="k-code muted">{layer.unicode ? `U+${layer.unicode}` : ""}</span>
          </div>
          <MetricField onInvalid={onError} label="LSB" value={layer.lsb} disabled={layer.lsb === null} onCommit={(v) => setMetric("lsb", v)} metricKey={layer.metricsKeys?.left} onKey={(k) => setMetricsKey("lsb", k)} />
          <MetricField onInvalid={onError} label="Width" value={layer.width} onCommit={(v) => setMetric("width", v)} metricKey={layer.metricsKeys?.width} onKey={(k) => setMetricsKey("width", k)} />
          <MetricField onInvalid={onError} label="RSB" value={layer.rsb} disabled={layer.rsb === null} onCommit={(v) => setMetric("rsb", v)} metricKey={layer.metricsKeys?.right} onKey={(k) => setMetricsKey("rsb", k)} />
          <span className="info-sep" />
          {oneNode && one ? (
            <>
              <MetricField onInvalid={onError} label="X" value={oneNode.x} onCommit={(v) => { const next = moveSelection(paths, selection, v - oneNode.x, 0); setPaths(next); commit(next); }} />
              <MetricField onInvalid={onError} label="Y" value={oneNode.y} onCommit={(v) => { const next = moveSelection(paths, selection, 0, v - oneNode.y); setPaths(next); commit(next); }} />
            </>
          ) : oneComp ? (
            <>
              <span className="info-comp k-label" title="Selected component">◇ {oneComp.base}</span>
              <MetricField onInvalid={onError} label="X" value={oneComp.transform[4] + oneComp.dx} onCommit={(v) => moveComp([...compSel][0], v - (oneComp.transform[4] + oneComp.dx), 0)} />
              <MetricField onInvalid={onError} label="Y" value={oneComp.transform[5] + oneComp.dy} onCommit={(v) => moveComp([...compSel][0], 0, v - (oneComp.transform[5] + oneComp.dy))} />
            </>
          ) : (
            <span className="k-caption muted info-count">{selectionLabel(selection.size, compSel.size)}</span>
          )}
        </div>
      </div>
      {picking && <ComponentPicker names={glyphs.map((g) => g.name)} exclude={glyph} onPick={addComponent} onCancel={() => setPicking(false)} />}
      {layerDialog && <LayerDialog glyph={glyph} axes={layerDialog.axes} mode={layerDialog.mode} onCancel={() => setLayerDialog(null)} onSave={saveLayerDialog} />}
    </div>
  );
}

const off = (p: Pt): EditNode => ({ x: p.x, y: p.y, kind: "offcurve", smooth: false });

function selectionLabel(points: number, components: number): string {
  const parts = [points && `${points} point${points === 1 ? "" : "s"}`, components && `${components} component${components === 1 ? "" : "s"}`].filter(Boolean);
  return parts.length ? `${parts.join(", ")} selected` : "No selection";
}

function hint(tool: Tool): string {
  switch (tool) {
    case "select": return "Click or drag to select · drag to move · arrows nudge (Shift ×10) · Enter smooth/corner · Delete removes";
    case "pen": return "Click for corners, drag for curves · click the first point to close · Esc ends the path";
    case "rect": return "Drag to draw a rectangle · Shift for a square";
    case "ellipse": return "Drag to draw an ellipse · Shift for a circle";
    case "hand": return "Drag to pan · Ctrl+wheel zooms";
  }
}

function NodeMark({ n, at, selected, first }: { n: EditNode; at: Pt; selected: boolean; first: boolean }) {
  const cls = selected ? " is-selected" : "";
  if (!isOn(n)) return <circle cx={at.x} cy={at.y} r={3} className={`node-off${cls}`} />;
  if (n.smooth) return <circle cx={at.x} cy={at.y} r={4.5} className={`node-smooth${cls}${first ? " is-first" : ""}`} />;
  return <rect x={at.x - 4} y={at.y - 4} width={8} height={8} className={`node-corner${cls}${first ? " is-first" : ""}`} />;
}
