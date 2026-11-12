import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { backend, type DocStatus, type KernMode, type MetricName, type TextItem, type TextLayout } from "../lib/backend";

const PRESETS = ["HOHOHAVATAR To LT", "nnonnoonon HHOHHOOHOH", "Hamburgefontsiv", "/H/O/H/n/o/n LTLTLT ToTo"];
const MISSING_WIDTH = 500;
const MODES: { id: KernMode; label: string; title: string }[] = [
  { id: "group", label: "Group", title: "Kern the two glyphs' kerning groups (or the glyphs, if they have none)" },
  { id: "leftException", label: "Left exception", title: "Kern this left glyph against the right group" },
  { id: "rightException", label: "Right exception", title: "Kern the left group against this right glyph" },
  { id: "exception", label: "Exception", title: "Kern exactly these two glyphs" },
];

type Glyph = Extract<TextItem, { type: "glyph" }>;
interface Cursor { line: number; pos: number }

function NumField({ label, value, title, disabled, onCommit }: { label: string; value: number | null; title?: string; disabled?: boolean; onCommit: (v: number) => void }) {
  const shown = value === null ? "" : String(Math.round(value));
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);
  const commit = () => {
    const v = Number(text.trim().replace("−", "-") || "0");
    if (!Number.isFinite(v) || text === shown) return setText(shown);
    onCommit(v);
  };
  return (
    <label className="info-field" title={title}>
      <span className="k-caption muted">{label}</span>
      <input className="metric-input" value={text} disabled={disabled} placeholder="—" onChange={(e) => setText(e.target.value)} onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") (e.target as HTMLInputElement).blur();
          if (e.key === "Escape") { setText(shown); (e.target as HTMLInputElement).blur(); }
        }} />
    </label>
  );
}

function GroupField({ label, value, onCommit }: { label: string; value: string | null; onCommit: (v: string | null) => void }) {
  const shown = value ?? "";
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);
  return (
    <label className="info-field">
      <span className="k-caption muted">{label}</span>
      <input className="metric-input group-input" value={text} placeholder="none" onChange={(e) => setText(e.target.value)}
        onBlur={() => { if (text.trim() !== shown) onCommit(text.trim() || null); }}
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
}

/** S06 · Text view for spacing and kerning. */
export function TextView({ masterId, version, active, onChanged, onError, onOpenGlyph }: Props) {
  const [text, setText] = useState(PRESETS[0]);
  const [layout, setLayout] = useState<TextLayout | null>(null);
  const [size, setSize] = useState(140);
  const [mode, setMode] = useState<KernMode>("group");
  const [cursor, setCursor] = useState<Cursor>({ line: 0, pos: 1 });
  const [showMetrics, setShowMetrics] = useState(true);
  const svgRef = useRef<SVGSVGElement>(null);
  const chain = useRef(Promise.resolve());
  const latest = useRef({ layout, cursor, mode });
  latest.current = { layout, cursor, mode };

  const reload = useCallback(() => backend.textLayout(text, masterId).then(setLayout, onError), [text, masterId, onError]);
  useEffect(() => { reload(); }, [reload, version]);

  // Keep the cursor inside the text.
  useEffect(() => {
    if (!layout) return;
    setCursor((c) => {
      const line = Math.min(c.line, layout.lines.length - 1);
      return { line, pos: Math.min(c.pos, layout.lines[line]?.length ?? 0) };
    });
  }, [layout]);

  const run = useCallback((task: () => Promise<DocStatus>) => {
    chain.current = chain.current.then(async () => {
      try {
        onChanged(await task());
        await reload();
      } catch (e) {
        onError(e);
      }
    });
  }, [onChanged, onError, reload]);

  /**
   * Sets the kerning of (left, right). In Group mode the pair that currently applies
   * is edited (or a group pair created); exception modes always write that exception.
   */
  const kern = useCallback((left: Glyph, right: Glyph, value: (current: number) => number) => {
    const m = latest.current.mode;
    run(async () => {
      if (m === "group" && left.kernLeft && left.kernRight) return backend.setKerningPair(masterId, left.kernLeft, left.kernRight, value(left.kern));
      return (await backend.setGlyphKerning(masterId, left.name, right.name, m, value(left.kern))).status;
    });
  }, [masterId, run]);

  const setMetric = (glyph: string, metric: MetricName, v: number) => run(async () => (await backend.setMetric(glyph, masterId, metric, v)).status);
  const setGroups = (glyph: string, l: string | null, r: string | null) => run(() => backend.setGlyphGroups(glyph, l, r));

  // ---------- Geometry ----------

  const metrics = layout?.metrics;
  const scale = size / 1000;
  const pad = 32;
  const lineH = metrics ? (metrics.ascender - metrics.descender) * 1.25 : 1250;
  const baseline = (line: number) => pad + (line * lineH + (metrics?.ascender ?? 800) * 1.1) * scale;
  const positions = useMemo(() => (layout?.lines ?? []).map((line) => {
    const xs = [pad];
    let x = pad;
    for (const it of line) {
      x += ((it.type === "glyph" ? it.width + it.kern : MISSING_WIDTH)) * scale;
      xs.push(x);
    }
    return xs;
  }), [layout, scale]);
  const width = Math.max(800, ...positions.map((xs) => xs[xs.length - 1] + pad));
  const height = layout ? baseline(layout.lines.length - 1) + (-(metrics?.descender ?? -200) * 1.2) * scale + pad : 400;

  function placeCursor(e: React.MouseEvent) {
    if (!layout) return;
    const r = svgRef.current!.getBoundingClientRect();
    const x = e.clientX - r.left, y = e.clientY - r.top;
    let line = 0;
    while (line < layout.lines.length - 1 && y > baseline(line) - (metrics?.descender ?? -200) * scale + 0.1 * lineH * scale) line++;
    const xs = positions[line];
    let best = 0;
    xs.forEach((bx, i) => { if (Math.abs(bx - x) < Math.abs(xs[best] - x)) best = i; });
    setCursor({ line, pos: best });
  }

  function glyphAt(e: React.MouseEvent): string | null {
    if (!layout) return null;
    const r = svgRef.current!.getBoundingClientRect();
    const x = e.clientX - r.left, y = e.clientY - r.top;
    for (let li = 0; li < layout.lines.length; li++) {
      if (Math.abs(y - (baseline(li) - 300 * scale)) > lineH * scale * 0.6) continue;
      const xs = positions[li];
      for (let i = 0; i < layout.lines[li].length; i++) {
        const it = layout.lines[li][i];
        if (it.type === "glyph" && x >= xs[i] && x < xs[i + 1]) return it.name;
      }
    }
    return null;
  }

  // ---------- Keyboard ----------

  useEffect(() => {
    if (!active) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      const { layout, cursor } = latest.current;
      if (!layout) return;
      const line = layout.lines[cursor.line] ?? [];
      if (e.altKey && !e.ctrlKey && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
        const left = line[cursor.pos - 1], right = line[cursor.pos];
        if (left?.type === "glyph" && right?.type === "glyph") {
          const step = (e.shiftKey ? 1 : 10) * (e.key === "ArrowLeft" ? -1 : 1);
          kern(left, right, (v) => v + step);
        }
        e.preventDefault();
        return;
      }
      if (e.ctrlKey || e.altKey) return;
      const move = (c: Cursor) => { setCursor(c); e.preventDefault(); };
      if (e.key === "ArrowLeft") move(cursor.pos > 0 ? { ...cursor, pos: cursor.pos - 1 } : cursor);
      else if (e.key === "ArrowRight") move(cursor.pos < line.length ? { ...cursor, pos: cursor.pos + 1 } : cursor);
      else if (e.key === "ArrowUp" && cursor.line > 0) move({ line: cursor.line - 1, pos: Math.min(cursor.pos, layout.lines[cursor.line - 1].length) });
      else if (e.key === "ArrowDown" && cursor.line < layout.lines.length - 1) move({ line: cursor.line + 1, pos: Math.min(cursor.pos, layout.lines[cursor.line + 1].length) });
      else if (e.key === "Home") move({ ...cursor, pos: 0 });
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
  const pairLabel = (g?: Glyph) => (g?.kernLeft ? `${g.kernLeft} | ${g.kernRight}` : "no kerning");

  return (
    <div className="textview">
      <div className="toolbar" role="toolbar" aria-label="Text tools">
        <input className="text-input" value={text} onChange={(e) => setText(e.target.value)} aria-label="Sample text" placeholder="Type text, or /glyphname" spellCheck={false} />
        <select className="select" aria-label="Sample strings" value="" onChange={(e) => e.target.value && setText(e.target.value)}>
          <option value="">Samples…</option>
          {PRESETS.map((p) => <option key={p} value={p}>{p}</option>)}
        </select>
        <span className="toolbar-sep" />
        <div className="segmented" role="radiogroup" aria-label="Kerning mode">
          {MODES.map((m) => (
            <button key={m.id} role="radio" aria-checked={mode === m.id} title={m.title} className={`segment${mode === m.id ? " is-selected" : ""}`} onClick={() => setMode(m.id)}>{m.label}</button>
          ))}
        </div>
        <span className="spacer" />
        <label className="check-row k-caption"><input type="checkbox" checked={showMetrics} onChange={(e) => setShowMetrics(e.target.checked)} /> Metrics</label>
        <input type="range" min={24} max={320} value={size} onChange={(e) => setSize(Number(e.target.value))} aria-label="Text size" className="size-slider" />
        <span className="k-caption muted zoom">{size} px</span>
      </div>
      <div className="text-canvas">
        <svg ref={svgRef} width={width} height={height} onMouseDown={placeCursor} onDoubleClick={(e) => { const n = glyphAt(e); if (n) onOpenGlyph(n); }}>
          {layout?.lines.map((ln, li) => {
            const by = baseline(li);
            const xs = positions[li];
            const pairKern = cursor.line === li ? (ln[cursor.pos - 1]?.type === "glyph" ? (ln[cursor.pos - 1] as Glyph) : undefined) : undefined;
            return (
              <g key={li}>
                {showMetrics && metrics && [metrics.ascender, metrics.xHeight, 0, metrics.descender].map((m) => (
                  <line key={m} x1={0} x2={width} y1={by - m * scale} y2={by - m * scale} className={m === 0 ? "metric-line baseline" : "metric-line"} />
                ))}
                {ln.map((it, i) => it.type === "glyph" ? (
                  <g key={i}>
                    {showMetrics && <rect x={xs[i]} y={by - (metrics?.ascender ?? 800) * scale} width={it.width * scale} height={((metrics?.ascender ?? 800) - (metrics?.descender ?? -200)) * scale} className="sb-box" />}
                    {it.svg ? (
                      <path d={it.svg} transform={`translate(${xs[i]} ${by}) scale(${scale} ${-scale})`} className="text-glyph" />
                    ) : (
                      <text x={xs[i] + (it.width * scale) / 2} y={by} className="text-placeholder" fontSize={size * 0.7}>{it.unicode ? String.fromCodePoint(parseInt(it.unicode, 16)) : "?"}</text>
                    )}
                    {it.kern !== 0 && (
                      <text x={xs[i] + (it.width + it.kern) * scale} y={by + 18} className={`kern-mark ${it.kern < 0 ? "neg" : "pos"}`}>{it.kern > 0 ? `+${it.kern}` : it.kern}</text>
                    )}
                  </g>
                ) : (
                  <g key={i}>
                    <rect x={xs[i] + 4} y={by - 600 * scale} width={MISSING_WIDTH * scale - 8} height={600 * scale} className="missing-box" />
                    <text x={xs[i] + (MISSING_WIDTH * scale) / 2} y={by - 260 * scale} className="missing-text">{it.text}</text>
                  </g>
                ))}
                {cursor.line === li && (
                  <>
                    <line x1={xs[cursor.pos]} x2={xs[cursor.pos]} y1={by - (metrics?.ascender ?? 800) * 1.05 * scale} y2={by - (metrics?.descender ?? -200) * 1.05 * scale} className="text-cursor" />
                    {pairKern && ln[cursor.pos]?.type === "glyph" && (
                      <g transform={`translate(${xs[cursor.pos]} ${by - (metrics?.descender ?? -200) * scale + 22})`}>
                        <rect x={-46} y={-14} width={92} height={22} rx={6} className={`kern-badge${pairKern.kernKind === "exception" ? " is-exception" : ""}`} />
                        <text x={0} y={2} className="kern-badge-text">{pairKern.kern === 0 ? "0" : pairKern.kern > 0 ? `+${pairKern.kern}` : pairKern.kern}</text>
                      </g>
                    )}
                  </>
                )}
              </g>
            );
          })}
        </svg>
      </div>
      {current && (
        <div className="info-box text-info" role="group" aria-label="Spacing">
          <div className="info-title">
            <span className="k-body-strong">{current.name}</span>
            <span className="k-code muted">{current.unicode ? `U+${current.unicode}` : ""}</span>
          </div>
          <NumField label={prev ? `◂ ${prev.name}` : "◂ kern"} value={prev ? prev.kern : null} disabled={!prev} title={prev ? pairLabel(prev) : undefined}
            onCommit={(v) => prev && kern(prev, current, () => v)} />
          <NumField label="LSB" value={current.lsb} disabled={current.lsb === null} onCommit={(v) => setMetric(current.name, "lsb", v)} />
          <NumField label="Width" value={current.width} onCommit={(v) => setMetric(current.name, "width", v)} />
          <NumField label="RSB" value={current.rsb} disabled={current.rsb === null} onCommit={(v) => setMetric(current.name, "rsb", v)} />
          <NumField label={next ? `${next.name} ▸` : "kern ▸"} value={next ? current.kern : null} disabled={!next} title={next ? pairLabel(current) : undefined}
            onCommit={(v) => next && kern(current, next, () => v)} />
          <span className="info-sep" />
          <GroupField label="Left group" value={current.leftGroup} onCommit={(v) => setGroups(current.name, v, current.rightGroup)} />
          <GroupField label="Right group" value={current.rightGroup} onCommit={(v) => setGroups(current.name, current.leftGroup, v)} />
        </div>
      )}
      <div className="text-hint k-caption muted">Click to place the cursor · ←/→ move · Alt+←/→ kerns ±10 (Alt+Shift ±1) · double-click a glyph to edit it</div>
    </div>
  );
}
