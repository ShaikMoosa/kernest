import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { backend, type DocStatus, type FeaturesView, type FontInfo, type KernMode, type Location, type MetricName, type ShapedView, type TextItem, type TextLayout } from "../lib/backend";
import { t } from "../lib/i18n";
import { ColorGlyph } from "./ColorGlyph";

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
  const latest = useRef({ layout, cursor, mode, previewing: false, shapeOn: false });
  latest.current = { layout, cursor, mode, previewing: latest.current?.previewing ?? false, shapeOn: latest.current?.shapeOn ?? false };

  const [info, setInfo] = useState<FontInfo | null>(null);
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

  // ---------- Feature preview ----------
  const [featView, setFeatView] = useState<FeaturesView | null>(null);
  const [featOpen, setFeatOpen] = useState(false);
  const [shapeOn, setShapeOn] = useState(false);
  // Only the tags the user flipped; the rest follow what's in the font.
  const [overrides, setOverrides] = useState<Record<string, boolean>>({});
  const [shaped, setShaped] = useState<ShapedView | null>(null);
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
  const featChecked = (tag: string) => overrides[tag] ?? (featView?.automatic.includes(tag) || featView?.handwritten.includes(tag) || false);
  // Explicit on/off per tag so default-on features like liga can be switched off too.
  const featSettings = useMemo(() => featTags.map((tag) => `${tag}=${featChecked(tag) ? 1 : 0}`), [featTags, overrides, featView]);

  const reload = useCallback(() => backend.textLayout(text, masterId, location).then(setLayout, onError), [text, masterId, location, onError]);
  const reloadShaped = useCallback(() => backend.shapeText(text, masterId, location, featSettings).then(setShaped, onError), [text, masterId, location, featSettings, onError]);
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

  const run = useCallback((task: () => Promise<DocStatus>) => {
    if (latest.current.previewing) return onError(t("Spacing edits apply to a master. Set the preview back to “Current master” to edit."));
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

  // Shaped preview geometry: advances and offsets come from the shaper, not kerning.
  const showShaped = shapeOn && shaped !== null;
  const sMetrics = shaped?.metrics;
  const sLineH = sMetrics ? (sMetrics.ascender - sMetrics.descender) * 1.25 : 1250;
  const sBaseline = (line: number) => pad + (line * sLineH + (sMetrics?.ascender ?? 800) * 1.1) * scale;
  const sPositions = useMemo(() => (shaped?.lines ?? []).map((line) => {
    const xs = [pad];
    let x = pad;
    for (const g of line) {
      x += g.advance * scale;
      xs.push(x);
    }
    return xs;
  }), [shaped, scale]);

  const activePositions = showShaped ? sPositions : positions;
  const width = Math.max(800, ...activePositions.map((xs) => xs[xs.length - 1] + pad));
  const activeLines = showShaped ? shaped.lines.length : layout?.lines.length;
  const activeBaseline = showShaped ? sBaseline : baseline;
  const activeDescender = (showShaped ? sMetrics : metrics)?.descender ?? -200;
  const height = activeLines ? activeBaseline(activeLines - 1) + (-activeDescender * 1.2) * scale + pad : 400;

  function placeCursor(e: React.MouseEvent) {
    if (!layout || shapeOn) return;
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
    const r = svgRef.current!.getBoundingClientRect();
    const x = e.clientX - r.left, y = e.clientY - r.top;
    if (showShaped) {
      for (let li = 0; li < shaped.lines.length; li++) {
        if (Math.abs(y - (sBaseline(li) - 300 * scale)) > sLineH * scale * 0.6) continue;
        const xs = sPositions[li];
        for (let i = 0; i < shaped.lines[li].length; i++) {
          if (x >= xs[i] && x < xs[i + 1]) return shaped.lines[li][i].name;
        }
      }
      return null;
    }
    if (!layout) return null;
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
      if (latest.current.shapeOn) return;
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
  const pairLabel = (g?: Glyph) => (g?.kernLeft ? `${g.kernLeft} | ${g.kernRight}` : t("no kerning"));

  return (
    <div className="textview">
      <div className="toolbar" role="toolbar" aria-label={t("Text tools")}>
        <input className="text-input" value={text} onChange={(e) => setText(e.target.value)} aria-label={t("Sample text")} placeholder={t("Type text, or /glyphname")} spellCheck={false} />
        <select className="select" aria-label={t("Sample strings")} value="" onChange={(e) => e.target.value && setText(e.target.value)}>
          <option value="">{t("Samples…")}</option>
          {PRESETS.map((p) => <option key={p} value={p}>{p}</option>)}
        </select>
        <span className="toolbar-sep" />
        <div className="segmented" role="radiogroup" aria-label={t("Kerning mode")}>
          {MODES.map((m) => (
            <button key={m.id} role="radio" aria-checked={mode === m.id} title={t(m.title)} className={`segment${mode === m.id ? " is-selected" : ""}`} onClick={() => setMode(m.id)}>{t(m.label)}</button>
          ))}
        </div>
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
      <div className="text-canvas">
        <svg ref={svgRef} width={width} height={height} style={{ minWidth: "100%" }} onMouseDown={placeCursor} onDoubleClick={(e) => { const n = glyphAt(e); if (n) onOpenGlyph(n); }}>
          {!showShaped && layout?.lines.map((ln, li) => {
            const by = baseline(li);
            const xs = positions[li];
            const pairKern = cursor.line === li ? (ln[cursor.pos - 1]?.type === "glyph" ? (ln[cursor.pos - 1] as Glyph) : undefined) : undefined;
            return (
              <g key={li}>
                {showMetrics && metrics && [metrics.ascender, metrics.xHeight, 0, metrics.descender].map((m) => (
                  <line key={m} x1={0} x2={Math.max(width, 6000)} y1={by - m * scale} y2={by - m * scale} className={m === 0 ? "metric-line baseline" : "metric-line"} />
                ))}
                {ln.map((it, i) => it.type === "glyph" ? (
                  <g key={i}>
                    {showMetrics && <rect x={xs[i]} y={by - (metrics?.ascender ?? 800) * scale} width={it.width * scale} height={((metrics?.ascender ?? 800) - (metrics?.descender ?? -200)) * scale} className="sb-box" />}
                    {it.svg ? (<>
                      {it.color?.length ? (
                        <g transform={`translate(${xs[i]} ${by}) scale(${scale} ${scale})`}><title>{it.name}</title><ColorGlyph layers={it.color} palette={layout?.palette ?? []} /></g>
                      ) : (
                        <path d={it.svg} transform={`translate(${xs[i]} ${by}) scale(${scale} ${-scale})`} className={it.incompatible ? "text-glyph is-incompatible" : "text-glyph"}><title>{it.incompatible ? t("{name}: masters can't be interpolated (default master shown)", { name: it.name }) : it.name}</title></path>
                      )}
                    </>) : (
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
          {showShaped && shaped.lines.map((ln, li) => {
            const by = sBaseline(li);
            const xs = sPositions[li];
            return (
              <g key={li}>
                {showMetrics && sMetrics && [sMetrics.ascender, sMetrics.xHeight, 0, sMetrics.descender].map((m) => (
                  <line key={m} x1={0} x2={Math.max(width, 6000)} y1={by - m * scale} y2={by - m * scale} className={m === 0 ? "metric-line baseline" : "metric-line"} />
                ))}
                {ln.map((g, i) => (
                  <g key={i} transform={`translate(${xs[i] + g.xOffset * scale} ${by - g.yOffset * scale})`}>
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
          <NumField label={prev ? `◂ ${prev.name}` : `◂ ${t("kern")}`} value={prev ? prev.kern : null} disabled={!prev || previewing} title={prev ? pairLabel(prev) : undefined}
            onCommit={(v) => prev && kern(prev, current, () => v)} />
          <NumField label={t("LSB")} value={current.lsb} disabled={current.lsb === null || previewing} onCommit={(v) => setMetric(current.name, "lsb", v)} />
          <NumField label={t("Width")} value={current.width} disabled={previewing} onCommit={(v) => setMetric(current.name, "width", v)} />
          <NumField label={t("RSB")} value={current.rsb} disabled={current.rsb === null || previewing} onCommit={(v) => setMetric(current.name, "rsb", v)} />
          <NumField label={next ? `${next.name} ▸` : `${t("kern")} ▸`} value={next ? current.kern : null} disabled={!next || previewing} title={next ? pairLabel(current) : undefined}
            onCommit={(v) => next && kern(current, next, () => v)} />
          <span className="info-sep" />
          <GroupField label={t("Left group")} value={current.leftGroup} disabled={previewing} onCommit={(v) => setGroups(current.name, v, current.rightGroup)} />
          <GroupField label={t("Right group")} value={current.rightGroup} disabled={previewing} onCommit={(v) => setGroups(current.name, current.leftGroup, v)} />
        </div>
      )}
      <div className="text-hint k-caption muted">{shapeOn ? t("Feature preview · spacing edits are paused: turn off Shape preview to edit") : previewing ? t("Previewing an interpolated location · spacing edits are paused: choose “Current master” to edit") : t("Click to place the cursor · ←/→ move · Alt+←/→ kerns ±10 (Alt+Shift ±1) · double-click a glyph to edit it")}</div>
    </div>
  );
}
