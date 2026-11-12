import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { backend, type DocStatus, type LayerView, type MetricName } from "../lib/backend";
import {
  clonePaths, deleteSelection, ellipsePath, handleOwner, isOn, moveSelection, nodeKey, parseKey, pathsToSvg, rectPath,
  type EditNode, type EditPath,
} from "../lib/outline";

type Tool = "select" | "pen" | "rect" | "ellipse" | "hand";
interface Pt { x: number; y: number }
interface View { s: number; ox: number; oy: number }

const TOOLS: { id: Tool; key: string; label: string; icon: React.ReactNode }[] = [
  { id: "select", key: "v", label: "Select (V)", icon: <path d="M6 3l12 9-6 1-3 6z" /> },
  { id: "pen", key: "p", label: "Pen (P)", icon: <><path d="M12 3l6 10-6 8-6-8z" /><circle cx="12" cy="12" r="1.6" /></> },
  { id: "rect", key: "f", label: "Rectangle (F)", icon: <rect x="5" y="6" width="14" height="12" rx="1" /> },
  { id: "ellipse", key: "e", label: "Ellipse (E)", icon: <ellipse cx="12" cy="12" rx="7.5" ry="6.5" /> },
  { id: "hand", key: "h", label: "Hand (H) · or hold Space", icon: <path d="M8 12V6.5a1.5 1.5 0 0 1 3 0V11V5a1.5 1.5 0 0 1 3 0v6V6.5a1.5 1.5 0 0 1 3 0V14a6 6 0 0 1-6 6h-1a6 6 0 0 1-4.6-2.2L4 15a1.5 1.5 0 0 1 2.2-2z" /> },
];

const HIT = 7; // px

function MetricField({ label, value, disabled, onCommit }: { label: string; value: number | null; disabled?: boolean; onCommit: (v: number) => void }) {
  const shown = value === null ? "" : String(Math.round(value));
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);
  const commit = () => {
    const v = Number(text.trim().replace("−", "-"));
    if (text.trim() === "" || !Number.isFinite(v) || text === shown) return setText(shown);
    onCommit(v);
  };
  return (
    <label className="info-field">
      <span className="k-caption muted">{label}</span>
      <input
        className="metric-input"
        value={text}
        disabled={disabled}
        placeholder="—"
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") (e.target as HTMLInputElement).blur();
          if (e.key === "Escape") { setText(shown); (e.target as HTMLInputElement).blur(); }
        }}
      />
    </label>
  );
}

interface Props {
  glyph: string;
  masterId: string;
  /** Bumped by the app when the layer changed elsewhere (undo/redo). */
  version: number;
  active: boolean;
  onCommitted: (status: DocStatus) => void;
  onError: (e: unknown) => void;
}

/** S05 · Edit View — draw and edit one glyph layer. */
export function EditView({ glyph, masterId, version, active, onCommitted, onError }: Props) {
  const [layer, setLayer] = useState<LayerView | null>(null);
  const [paths, setPaths] = useState<EditPath[]>([]);
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const [tool, setTool] = useState<Tool>("select");
  const [view, setView] = useState<View>({ s: 0.5, ox: 0, oy: 0 });
  const [size, setSize] = useState({ w: 800, h: 600 });
  const [cursor, setCursor] = useState<Pt | null>(null);
  const [marquee, setMarquee] = useState<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  const [preview, setPreview] = useState<EditPath | null>(null);
  const [spaceDown, setSpaceDown] = useState(false);
  const [, rerender] = useState(0);

  const wrapRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const fitted = useRef(false);
  const drag = useRef<null | { mode: string; startScreen: Pt; startWorld: Pt; base: EditPath[]; moved: boolean; additive?: boolean; pi?: number; ni?: number; startView?: View }>(null);
  const pen = useRef<{ path: number | null; pendingOut: Pt | null }>({ path: null, pendingOut: null });
  const chain = useRef(Promise.resolve());
  const latest = useRef({ layer, paths, selection, view });
  latest.current = { layer, paths, selection, view };

  // ---------- Loading & committing ----------

  useEffect(() => {
    let cancelled = false;
    backend.glyphLayer(glyph, masterId).then((l) => {
      if (cancelled) return;
      setLayer(l);
      setPaths(l.paths);
      setSelection((sel) => new Set([...sel].filter((k) => { const [pi, ni] = parseKey(k); return !!l.paths[pi]?.nodes[ni]; })));
    }, onError);
    return () => { cancelled = true; };
  }, [glyph, masterId, version, onError]);

  const accept = useCallback((l: LayerView) => {
    setLayer(l);
    setPaths(l.paths);
    onCommitted(l.status);
  }, [onCommitted]);

  const commit = useCallback((next: EditPath[], width?: number) => {
    chain.current = chain.current.then(async () => {
      const l = latest.current.layer;
      if (!l) return;
      try {
        accept(await backend.updateLayer(glyph, masterId, width ?? l.width, next));
      } catch (e) {
        setPaths(l.paths);
        onError(e);
      }
    });
  }, [glyph, masterId, accept, onError]);

  const setMetric = useCallback((metric: MetricName, value: number) => {
    chain.current = chain.current.then(async () => {
      try {
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
  }, [zoomAt]);

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

  // ---------- Pointer interaction ----------

  const activeTool: Tool = spaceDown ? "hand" : tool;

  function onPointerDown(e: React.PointerEvent<SVGSVGElement>) {
    if (!layer) return;
    svgRef.current!.setPointerCapture(e.pointerId);
    const sp = local(e);
    const wp = toWorld(sp.x, sp.y);
    const base = clonePaths(paths);
    if (e.button === 1 || activeTool === "hand") {
      drag.current = { mode: "pan", startScreen: sp, startWorld: wp, base, moved: false, startView: view };
      return;
    }
    if (e.button !== 0) return;

    if (activeTool === "select") {
      const hit = hitNode(sp);
      if (hit) {
        let sel = selection;
        if (e.shiftKey) {
          sel = new Set(selection);
          if (sel.has(hit)) sel.delete(hit); else sel.add(hit);
        } else if (!selection.has(hit)) sel = new Set([hit]);
        setSelection(sel);
        drag.current = { mode: "move", startScreen: sp, startWorld: wp, base, moved: false };
      } else {
        if (!e.shiftKey) setSelection(new Set());
        drag.current = { mode: "marquee", startScreen: sp, startWorld: wp, base, moved: false, additive: e.shiftKey };
      }
      return;
    }

    if (activeTool === "rect" || activeTool === "ellipse") {
      drag.current = { mode: "shape", startScreen: sp, startWorld: round(wp), base, moved: false };
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
        drag.current = { mode: "pen", startScreen: sp, startWorld: p, base: clonePaths(base), moved: false, pi: pn.path!, ni: open.nodes.length - 1 };
      } else {
        base.push({ closed: false, nodes: [{ ...p, kind: "line", smooth: false }] });
        pen.current = { path: base.length - 1, pendingOut: null };
        drag.current = { mode: "pen", startScreen: sp, startWorld: p, base: clonePaths(base), moved: false, pi: base.length - 1, ni: 0 };
      }
      setPaths(base);
      setSelection(new Set());
    }
  }

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
      setPaths(moveSelection(d.base, selection, dx, dy));
    } else if (d.mode === "marquee") {
      setMarquee({ x0: d.startScreen.x, y0: d.startScreen.y, x1: sp.x, y1: sp.y });
    } else if (d.mode === "shape") {
      const a = d.startWorld;
      let b = round(wp);
      if (e.shiftKey) {
        const side = Math.max(Math.abs(b.x - a.x), Math.abs(b.y - a.y));
        b = { x: a.x + Math.sign(b.x - a.x || 1) * side, y: a.y + Math.sign(b.y - a.y || 1) * side };
      }
      setPreview(tool === "ellipse" ? ellipsePath(a.x, a.y, b.x, b.y) : rectPath(a.x, a.y, b.x, b.y));
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
      setPaths(next);
    }
  }

  function onPointerUp(e: React.PointerEvent<SVGSVGElement>) {
    const d = drag.current;
    drag.current = null;
    if (!d) return;
    const sp = local(e);
    if (d.mode === "move" && d.moved) commit(latest.current.paths);
    else if (d.mode === "marquee" && marquee) {
      const [x0, x1] = [Math.min(marquee.x0, sp.x), Math.max(marquee.x0, sp.x)];
      const [y0, y1] = [Math.min(marquee.y0, sp.y), Math.max(marquee.y0, sp.y)];
      const sel = new Set(d.additive ? selection : []);
      paths.forEach((p, pi) => p.nodes.forEach((n, ni) => {
        const q = toScreen(n.x, n.y);
        if (q.x >= x0 && q.x <= x1 && q.y >= y0 && q.y <= y1) sel.add(nodeKey(pi, ni));
      }));
      setSelection(sel);
      setMarquee(null);
    } else if (d.mode === "shape") {
      if (preview && d.moved) {
        const next = [...latest.current.paths, preview];
        setPaths(next);
        commit(next);
        setSelection(new Set(preview.nodes.map((_, ni) => nodeKey(next.length - 1, ni))));
      }
      setPreview(null);
    } else if (d.mode === "pen") {
      commit(latest.current.paths);
      rerender((n) => n + 1);
    }
    setMarquee(null);
  }

  // ---------- Keyboard ----------

  useEffect(() => {
    if (!active) return;
    const typing = (t: EventTarget | null) => t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement;
    const onDown = (e: KeyboardEvent) => {
      if (typing(e.target)) return;
      const { paths, selection } = latest.current;
      if (e.key === " ") { setSpaceDown(true); e.preventDefault(); return; }
      if (e.ctrlKey && !e.altKey) {
        const k = e.key.toLowerCase();
        if (k === "a") setSelection(new Set(paths.flatMap((p, pi) => p.nodes.map((_, ni) => nodeKey(pi, ni)))));
        else if (k === "0") fit();
        else if (k === "=" || k === "+") zoomAt(1.25, size.w / 2, size.h / 2);
        else if (k === "-") zoomAt(0.8, size.w / 2, size.h / 2);
        else return;
        e.preventDefault();
        return;
      }
      if (e.altKey) return;
      const nudge: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, 1], ArrowDown: [0, -1] };
      if (nudge[e.key] && selection.size) {
        const m = e.shiftKey ? 10 : 1;
        const next = moveSelection(paths, selection, nudge[e.key][0] * m, nudge[e.key][1] * m);
        setPaths(next);
        commit(next);
        e.preventDefault();
        return;
      }
      if ((e.key === "Delete" || e.key === "Backspace") && selection.size) {
        const next = deleteSelection(paths, selection);
        setSelection(new Set());
        setPaths(next);
        commit(next);
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
        else setSelection(new Set());
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
  }, [active, commit, fit, zoomAt, size]);

  // ---------- Rendering ----------

  if (!layer) return <div className="editor"><div className="editor-loading k-caption muted">Loading {glyph}…</div></div>;

  const { ascender, capHeight, xHeight, descender } = layer.metrics;
  const lines: [string, number][] = [["Ascender", ascender], ["Cap height", capHeight], ["x-height", xHeight], ["Baseline", 0], ["Descender", descender]];
  const x0 = toScreen(0, 0).x, x1 = toScreen(layer.width, 0).x;
  const outline = pathsToSvg(paths);
  const one = selection.size === 1 ? parseKey([...selection][0]) : null;
  const oneNode = one ? paths[one[0]]?.nodes[one[1]] : undefined;
  const pendingOut = pen.current.path !== null ? pen.current.pendingOut : null;
  const penPath = pen.current.path !== null ? paths[pen.current.path] : undefined;
  const lastPen = penPath && !penPath.closed ? penPath.nodes[penPath.nodes.length - 1] : undefined;
  const cursorStyle = activeTool === "hand" ? (drag.current?.mode === "pan" ? "grabbing" : "grab") : activeTool === "select" ? "default" : "crosshair";

  return (
    <div className="editor">
      <div className="toolbar" role="toolbar" aria-label="Tools">
        {TOOLS.map((t) => (
          <button key={t.id} className={`tool${tool === t.id ? " is-selected" : ""}`} title={t.label} aria-label={t.label} aria-pressed={tool === t.id}
            onClick={() => { setTool(t.id); pen.current = { path: null, pendingOut: null }; }}>
            <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinejoin="round">{t.icon}</svg>
          </button>
        ))}
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
          {lines.map(([label, y]) => {
            const sy = toScreen(0, y).y;
            return (
              <g key={label}>
                <line x1={0} x2={size.w} y1={sy} y2={sy} className={y === 0 ? "metric-line baseline" : "metric-line"} />
                <text x={x1 + 8} y={sy - 4} className="metric-label">{label} {Math.round(y)}</text>
              </g>
            );
          })}
          <rect x={x0} y={toScreen(0, ascender).y} width={Math.max(0, x1 - x0)} height={(ascender - descender) * view.s} className="advance-box" />
          {/* Glyph */}
          <g transform={`translate(${view.ox} ${view.oy}) scale(${view.s} ${-view.s})`}>
            {layer.componentSvg && <path d={layer.componentSvg} className="component-fill" />}
            <path d={outline} className="glyph-fill" fillRule="nonzero" />
            {preview && <path d={pathsToSvg([preview])} className="preview-outline" />}
          </g>
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
        {paths.length === 0 && !layer.componentSvg && !preview && (
          <div className="canvas-empty">
            <p className="k-subtitle">Empty glyph</p>
            <p className="k-caption muted">Press P to draw with the pen, or F for a rectangle.</p>
          </div>
        )}
        <div className="info-box" role="group" aria-label="Glyph info">
          <div className="info-title">
            <span className="k-body-strong">{layer.glyph}</span>
            <span className="k-code muted">{layer.unicode ? `U+${layer.unicode}` : ""}</span>
          </div>
          <MetricField label="LSB" value={layer.lsb} disabled={layer.lsb === null} onCommit={(v) => setMetric("lsb", v)} />
          <MetricField label="Width" value={layer.width} onCommit={(v) => setMetric("width", v)} />
          <MetricField label="RSB" value={layer.rsb} disabled={layer.rsb === null} onCommit={(v) => setMetric("rsb", v)} />
          <span className="info-sep" />
          {oneNode && one ? (
            <>
              <MetricField label="X" value={oneNode.x} onCommit={(v) => { const next = moveSelection(paths, selection, v - oneNode.x, 0); setPaths(next); commit(next); }} />
              <MetricField label="Y" value={oneNode.y} onCommit={(v) => { const next = moveSelection(paths, selection, 0, v - oneNode.y); setPaths(next); commit(next); }} />
            </>
          ) : (
            <span className="k-caption muted info-count">{selection.size ? `${selection.size} points selected` : "No selection"}</span>
          )}
        </div>
      </div>
    </div>
  );
}

const off = (p: Pt): EditNode => ({ x: p.x, y: p.y, kind: "offcurve", smooth: false });

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
