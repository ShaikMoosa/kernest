import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { GlyphPropertiesFields, ColorLabelPicker } from "./GlyphPropertiesFields";
import { labelCss, labelKey, labelName } from "../lib/colorLabels";
import { backend, errorMessage, type FontView, type ColorLabel, type GlyphFilter, type GlyphProperty, type GlyphSummary, type LanguageCoverage, type MissingGlyph } from "../lib/backend";
import { SearchIcon } from "./Icons";
import { t, tn } from "../lib/i18n";
import { ColorGlyph } from "./ColorGlyph";
import { GlyphListView, type DragHandlers, type ListEdit } from "./GlyphListView";
import { SmartFilterDialog } from "./SmartFilterDialog";
import { BatchMenu } from "./BatchMenu";

const CATEGORY_ORDER = ["Uppercase", "Lowercase", "Letter", "Number", "Punctuation", "Symbol", "Separator", "Mark", "Other"];

/** Rows drawn above and below the visible part of the grid. */
const GRID_OVERSCAN = 3;

/** S03 · The glyph grid, drawing only the rows in view (large fonts have thousands
 *  of glyphs). Rows above and below are stood in for by two spacers. */
function VirtualGrid({ glyphs, cellSize, onKeyDown, renderCell }: {
  glyphs: GlyphSummary[];
  cellSize: number;
  onKeyDown: (e: React.KeyboardEvent<HTMLDivElement>) => void;
  renderCell: (g: GlyphSummary) => React.ReactNode;
}) {
  const box = useRef<HTMLDivElement>(null);
  const [view, setView] = useState({ top: 0, height: 800, width: 800, gap: 8, padTop: 4, padX: 40 });
  const [rowH, setRowH] = useState(0);
  const measure = useCallback(() => {
    const el = box.current;
    if (!el) return;
    const cs = getComputedStyle(el);
    setView({
      top: el.scrollTop,
      height: el.clientHeight,
      width: el.clientWidth,
      gap: parseFloat(cs.rowGap) || 0,
      padTop: parseFloat(cs.paddingTop) || 0,
      padX: (parseFloat(cs.paddingLeft) || 0) + (parseFloat(cs.paddingRight) || 0),
    });
  }, []);
  useLayoutEffect(() => {
    measure();
    const el = box.current;
    if (!el) return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [measure]);
  const { gap } = view;
  const inner = Math.max(cellSize, view.width - view.padX);
  // Same rule as `repeat(auto-fill, minmax(cell, 1fr))`.
  const cols = Math.max(1, Math.floor((inner + gap) / (cellSize + gap)));
  const cellW = (inner - gap * (cols - 1)) / cols;
  const row = (rowH || cellW + 24) + gap;
  const rows = Math.ceil(glyphs.length / cols);
  const first = Math.max(0, Math.floor((view.top - view.padTop) / row) - GRID_OVERSCAN);
  const last = Math.min(rows, Math.ceil((view.top + view.height) / row) + GRID_OVERSCAN);
  // The real row height (art plus label) once a cell is on screen.
  useLayoutEffect(() => {
    const cell = box.current?.querySelector<HTMLElement>(".glyph-cell");
    const h = cell?.getBoundingClientRect().height ?? 0;
    if (h > 0 && Math.abs(h - rowH) > 0.5) setRowH(h);
  });
  return (
    <div ref={box} className="glyph-grid" role="list" style={{ "--k-cell-size": `${cellSize}px` } as React.CSSProperties} onScroll={measure} onKeyDown={onKeyDown}>
      {first > 0 && <div className="glyph-grid-spacer" style={{ height: first * row - gap }} aria-hidden="true" />}
      {glyphs.slice(first * cols, last * cols).map(renderCell)}
      {last < rows && <div className="glyph-grid-spacer" style={{ height: (rows - last) * row - gap }} aria-hidden="true" />}
    </div>
  );
}

function GlyphCell({ glyph, font, selected, onSelect, onOpen, drag, dragging, drop }: { glyph: GlyphSummary; font: FontView; selected: boolean; onSelect: (e: React.MouseEvent) => void; onOpen: () => void; drag: DragHandlers; dragging: boolean; drop: boolean }) {
  const { ascender, descender } = font.metrics;
  const h = ascender - descender;
  // Square view box centred on the advance width, so every glyph uses the same scale.
  const x0 = glyph.width / 2 - h / 2;
  const empty = !glyph.svg;
  // Colour labels: the glyph's fills the cell, the layer's the right half (Glyphs' look).
  const tint: React.CSSProperties = {};
  const g = glyph.colorLabel !== undefined ? labelCss(glyph.colorLabel, 0.38) : null;
  const l = glyph.layerColorLabel !== undefined ? labelCss(glyph.layerColorLabel, 0.38) : null;
  if (g || l) tint.backgroundImage = `linear-gradient(to right, ${g ?? "transparent"} 50%, ${l ?? g} 50%)`;
  const labels = [glyph.colorLabel !== undefined ? t(labelName(glyph.colorLabel)) : null, glyph.layerColorLabel !== undefined ? t("layer: {colour}", { colour: t(labelName(glyph.layerColorLabel)) }) : null].filter(Boolean).join(", ");
  return (
    <button className={`glyph-cell${selected ? " is-selected" : ""}${empty ? " is-empty" : ""}${dragging ? " is-dragging" : ""}${drop ? " is-drop" : ""}`} style={tint} {...drag} onClick={onSelect} onDoubleClick={onOpen} title={labels ? `${t("{name} — double-click to edit", { name: glyph.name })} (${labels})` : t("{name} — double-click to edit", { name: glyph.name })}>
      <span className="glyph-art">
        {empty ? (
          <span className="glyph-placeholder">{glyph.character?.trim() ? glyph.character : ""}</span>
        ) : (
          <svg viewBox={`${x0} ${-ascender} ${h} ${h}`} aria-hidden="true">
            {glyph.color?.length
              ? <ColorGlyph layers={glyph.color} palette={font.palette ?? []} />
              : <path d={glyph.svg} transform="scale(1,-1)" />}
          </svg>
        )}
      </span>
      {!glyph.compatible && <span className="glyph-incompatible" title={t("Masters can't be interpolated (open the glyph to see why)")} aria-label={t("Incompatible masters")} />}
      {glyph.compatible && glyph.interpolationWarnings && <span className="glyph-interp-warning" title={t("Masters may not interpolate cleanly (open the glyph to see why)")} aria-label={t("Interpolation warnings")} />}
      {glyph.locked && (
        <span className="glyph-locked" title={t("This glyph is locked. Unlock it to edit.")} aria-label={t("Locked")}>
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" aria-hidden="true">
            <rect x="5" y="11" width="14" height="9" rx="2" /><path d="M8 11V7a4 4 0 0 1 8 0v4" />
          </svg>
        </span>
      )}
      <span className="glyph-label">
        <span className="glyph-name">{glyph.name}</span>
        <span className="glyph-unicode">{glyph.unicode ?? ""}</span>
      </span>
    </button>
  );
}

/** S03 · Font View — grid of glyphs with category sidebar and glyph info. */
export function FontViewScreen({ font, onOpenGlyph, onSelectionChange, onAddGlyphs, onSetExport, onSetFilters, onSetTags, onSetProductionName, onSetUnicodes, onSetGroups, onRenameGlyphs, onBatch, onReorder, onSetMetric, onRenameGlyph, onAddMissing, onAddNamed, onSetDirection, onSetVertical, onSetGlyphProperty, onSetColorLabel }: { font: FontView; onOpenGlyph: (name: string) => void; onSelectionChange?: (names: string[]) => void; onAddGlyphs?: () => void; onSetExport?: (names: string[], exportGlyph: boolean) => void; onSetFilters?: (filters: GlyphFilter[]) => Promise<void>; onSetTags?: (glyph: string, tags: string[]) => Promise<void>; onSetProductionName?: (glyph: string, name: string) => Promise<void>; onSetUnicodes?: (glyph: string, text: string) => Promise<void>; onSetGroups?: (glyph: string, left: string | null, right: string | null) => Promise<void>; onRenameGlyphs?: () => void; onBatch?: (operation: string) => void; onReorder?: (order: string[]) => void; onSetMetric?: (glyph: string, metric: "lsb" | "rsb" | "width", value: number) => Promise<void>; onRenameGlyph?: (glyph: string, name: string) => void; onAddMissing?: (language: string) => Promise<void>; onAddNamed?: (name: string) => void; onSetDirection?: (glyph: string, direction: string) => Promise<void>; onSetVertical?: (glyph: string, which: "advance" | "tsb" | "bsb" | "reset", value: number | null) => Promise<void>; onSetGlyphProperty?: (names: string[], property: GlyphProperty, value: string) => Promise<void>; onSetColorLabel?: (names: string[], layer: string | null, label: ColorLabel | null) => Promise<void> }) {
  const [category, setCategory] = useState<string>("All");
  // Sidebar › Colours: show only glyphs with this label (glyph or layer).
  const [colorFilter, setColorFilter] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const [mode, setMode] = useState<"grid" | "list">(() => (localStorage.getItem("kernest.fontViewMode") === "list" ? "list" : "grid"));
  const [cellSize, setCellSize] = useState(() => Math.min(160, Math.max(64, Number(localStorage.getItem("kernest.cellSize")) || 84)));
  const [activeFilter, setActiveFilter] = useState<string | null>(null);
  const [filterMatch, setFilterMatch] = useState<Set<string> | null>(null);
  const [filterDialog, setFilterDialog] = useState<{ initial: GlyphFilter | null } | null>(null);
  const [filterError, setFilterError] = useState<string | null>(null);
  const [languages, setLanguages] = useState<LanguageCoverage[]>([]);
  const [openLanguage, setOpenLanguage] = useState<string | null>(null);
  const [missing, setMissing] = useState<MissingGlyph[]>([]);
  const [languageError, setLanguageError] = useState<string | null>(null);
  // Drag-to-reorder: the glyph being dragged and the one it would drop before.
  const [dragName, setDragName] = useState<string | null>(null);
  const [dropName, setDropName] = useState<string | null>(null);

  // Coverage follows the glyph set; the Missing list loads only for the open language.
  useEffect(() => {
    let live = true;
    backend.languageCoverage()
      .then((l) => { if (live) { setLanguages(l); setLanguageError(null); } })
      .catch((e) => { if (live) setLanguageError(errorMessage(e)); });
    return () => { live = false; };
  }, [font.glyphs]);
  useEffect(() => {
    if (!openLanguage) { setMissing([]); return; }
    let live = true;
    backend.missingGlyphs(openLanguage)
      .then((m) => { if (live) setMissing(m); })
      .catch((e) => { if (live) setLanguageError(errorMessage(e)); });
    return () => { live = false; };
  }, [openLanguage, font.glyphs]);

  // Evaluate the active filter against the current font; a deleted filter deselects itself.
  useEffect(() => {
    if (!activeFilter) { setFilterMatch(null); return; }
    const f = font.filters.find((x) => x.name === activeFilter);
    if (!f) { setActiveFilter(null); return; }
    let live = true;
    backend.evaluateFilter(f)
      .then((names) => { if (live) { setFilterMatch(new Set(names)); setFilterError(null); } })
      .catch((e) => { if (live) { setFilterMatch(new Set()); setFilterError(errorMessage(e)); } });
    return () => { live = false; };
  }, [activeFilter, font.filters, font.glyphs]);

  // Keep the selection to glyphs that still exist, and tell the app (for Remove Glyph).
  useEffect(() => {
    const names = new Set(font.glyphs.map((g) => g.name));
    const kept = [...selection].filter((n) => names.has(n));
    if (kept.length !== selection.size) setSelection(new Set(kept));
    onSelectionChange?.(font.glyphs.filter((g) => selection.has(g.name)).map((g) => g.name));
  }, [selection, font.glyphs, onSelectionChange]);

  const incompatibleCount = useMemo(() => font.glyphs.filter((g) => !g.compatible).length, [font.glyphs]);
  const warningCount = useMemo(() => font.glyphs.filter((g) => g.compatible && g.interpolationWarnings).length, [font.glyphs]);
  const counts = useMemo(() => {
    const m = new Map<string, number>();
    for (const g of font.glyphs) m.set(g.category, (m.get(g.category) ?? 0) + 1);
    return CATEGORY_ORDER.filter((c) => m.has(c)).map((c) => [c, m.get(c)!] as const);
  }, [font.glyphs]);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return font.glyphs.filter((g) => {
      if (filterMatch && !filterMatch.has(g.name)) return false;
      if (colorFilter !== null && ![g.colorLabel, g.layerColorLabel].some((c) => c !== undefined && labelKey(c) === colorFilter)) return false;
      if (category === "Incompatible") { if (g.compatible) return false; }
      else if (category === "Interpolation warnings") { if (!g.compatible || !g.interpolationWarnings) return false; }
      else if (category !== "All" && g.category !== category) return false;
      if (!q) return true;
      return g.name.toLowerCase().includes(q) || g.unicode?.toLowerCase() === q.replace(/^u\+/, "") || g.character === query.trim();
    });
  }, [font.glyphs, category, query, filterMatch, colorFilter]);
  // The labels in use, for Sidebar › Colours: key, label, how many glyphs carry it.
  const colorCounts = useMemo(() => {
    const m = new Map<string, { label: ColorLabel; n: number }>();
    for (const g of font.glyphs) {
      const seen = new Set<string>();
      for (const c of [g.colorLabel, g.layerColorLabel]) {
        if (c === undefined) continue;
        const k = labelKey(c);
        if (seen.has(k)) continue;
        seen.add(k);
        const e = m.get(k);
        if (e) e.n += 1; else m.set(k, { label: c, n: 1 });
      }
    }
    return [...m.entries()].sort((a, b) => (typeof a[1].label === "number" ? a[1].label : 99) - (typeof b[1].label === "number" ? b[1].label : 99));
  }, [font.glyphs]);

  const drawn = font.glyphs.filter((g) => g.svg).length;
  // "144 glyphs · Latin" when everything shown is in one script.
  const shownScript = useMemo(() => {
    const scripts = new Set(shown.map((g) => g.script).filter((s) => s && s !== "Common"));
    return scripts.size === 1 ? [...scripts][0] : null;
  }, [shown]);
  const selectedGlyphs = font.glyphs.filter((g) => selection.has(g.name));
  // Edit › Select All, Deselect All, Invert Selection and Find… from the menu bar.
  const searchRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const on = (e: Event) => {
      const cmd = (e as CustomEvent<string>).detail;
      if (cmd === "select-all") setSelection(new Set(shown.map((g) => g.name)));
      else if (cmd === "deselect-all") setSelection(new Set());
      else if (cmd === "invert-selection") setSelection(new Set(shown.filter((g) => !selection.has(g.name)).map((g) => g.name)));
      else if (cmd === "find") { searchRef.current?.focus(); searchRef.current?.select(); }
    };
    window.addEventListener("kernest:fontview", on);
    return () => window.removeEventListener("kernest:fontview", on);
  }, [shown, selection]);
  const one = selectedGlyphs.length === 1 ? selectedGlyphs[0] : null;

  // Info-box drafts follow the selected glyph and any refresh (undo, list edits).
  const [tagsDraft, setTagsDraft] = useState("");
  const [prodDraft, setProdDraft] = useState("");
  const [uniDraft, setUniDraft] = useState("");
  const [leftDraft, setLeftDraft] = useState("");
  const [rightDraft, setRightDraft] = useState("");
  const oneTags = one?.tags.join(", ") ?? "";
  const oneProd = one?.productionName ?? "";
  const oneUni = one?.unicodes.join(", ") ?? "";
  const oneLeft = one?.leftGroup ?? "";
  const oneRight = one?.rightGroup ?? "";
  useEffect(() => { setTagsDraft(oneTags); }, [one?.name, oneTags]);
  useEffect(() => { setProdDraft(oneProd); }, [one?.name, oneProd]);
  useEffect(() => { setUniDraft(oneUni); }, [one?.name, oneUni]);
  useEffect(() => { setLeftDraft(oneLeft); setRightDraft(oneRight); }, [one?.name, oneLeft, oneRight]);

  function commitTags() {
    if (one && onSetTags && tagsDraft !== oneTags) onSetTags(one.name, tagsDraft.split(","));
  }
  function commitUnicodes() {
    if (one && onSetUnicodes && uniDraft.trim() !== oneUni) onSetUnicodes(one.name, uniDraft).catch(() => setUniDraft(oneUni));
  }
  function commitProd() {
    if (one && onSetProductionName && prodDraft.trim() !== oneProd) onSetProductionName(one.name, prodDraft);
  }
  function commitGroups() {
    if (one && onSetGroups && (leftDraft !== oneLeft || rightDraft !== oneRight)) {
      onSetGroups(one.name, leftDraft.trim() || null, rightDraft.trim() || null);
    }
  }
  function listEdit(edit: ListEdit) {
    if (edit.kind === "metric") onSetMetric?.(edit.glyph, edit.metric, edit.value);
    else if (edit.kind === "groups") onSetGroups?.(edit.glyph, edit.left, edit.right);
    else if (edit.kind === "rename") onRenameGlyph?.(edit.glyph, edit.name);
    else if (edit.kind === "unicodes") onSetUnicodes?.(edit.glyph, edit.text).catch(() => {});
    else onSetTags?.(edit.glyph, edit.tags);
  }
  function switchMode(m: "grid" | "list") {
    setMode(m);
    localStorage.setItem("kernest.fontViewMode", m);
  }
  function resizeCells(v: number) {
    setCellSize(v);
    localStorage.setItem("kernest.cellSize", String(v));
  }

  function select(g: GlyphSummary, e: React.MouseEvent) {
    setSelection((prev) => {
      if (e.ctrlKey) {
        const next = new Set(prev);
        if (next.has(g.name)) next.delete(g.name);
        else next.add(g.name);
        return next;
      }
      if (e.shiftKey && prev.size > 0) {
        const names = shown.map((x) => x.name);
        const last = [...prev].pop()!;
        const [a, b] = [names.indexOf(last), names.indexOf(g.name)].sort((x, y) => x - y);
        if (a >= 0) return new Set([...prev, ...names.slice(a, b + 1)]);
      }
      return new Set([g.name]);
    });
  }

  const fmt = (v: number | null) => (v === null ? "—" : String(Math.round(v)));

  // Move `drag` to just before `drop` in the full working order (not the filtered view),
  // so a drag reorders the font even when only some glyphs are shown.
  function reorder(drag: string, drop: string) {
    if (!onReorder || drag === drop) return;
    const order = font.glyphs.map((g) => g.name).filter((n) => n !== drag);
    const at = order.indexOf(drop);
    if (at < 0) return;
    order.splice(at, 0, drag);
    onReorder(order);
  }

  // HTML5 drag handlers shared by the grid cells and the list rows. Empty when the
  // app can't reorder (browser mock), so nothing is draggable there.
  function dragProps(name: string): DragHandlers {
    if (!onReorder) return {};
    return {
      draggable: true,
      onDragStart: (e) => { e.dataTransfer.setData("text/plain", name); e.dataTransfer.effectAllowed = "move"; setDragName(name); },
      onDragEnd: () => { setDragName(null); setDropName(null); },
      onDragOver: (e) => { if (!dragName) return; e.preventDefault(); e.dataTransfer.dropEffect = "move"; if (dropName !== name) setDropName(name); },
      onDragLeave: () => { if (dropName === name) setDropName(null); },
      onDrop: (e) => {
        e.preventDefault();
        const drag = dragName ?? e.dataTransfer.getData("text/plain");
        setDragName(null); setDropName(null);
        if (drag) reorder(drag, name);
      },
    };
  }

  async function saveFilters(next: GlyphFilter[]) {
    if (!onSetFilters) return;
    try {
      await onSetFilters(next);
      setFilterError(null);
    } catch (e) {
      setFilterError(errorMessage(e));
      throw e;
    }
  }

  async function saveSelectionAsList() {
    const base = t("Selection");
    const taken = new Set(font.filters.map((f) => f.name.toLowerCase()));
    let name = base;
    for (let i = 2; taken.has(name.toLowerCase()); i++) name = `${base} ${i}`;
    await saveFilters([...font.filters, { name, kind: "list", glyphs: [...selection] }]);
    setActiveFilter(name);
  }

  async function saveFromDialog(saved: GlyphFilter) {
    const editing = filterDialog?.initial;
    const next = editing
      ? font.filters.map((f) => (f.name === editing.name ? saved : f))
      : [...font.filters, saved];
    await saveFilters(next);
    setFilterDialog(null);
    setActiveFilter(saved.name);
  }

  const glyphInfo = (
    <section className="glyph-info" aria-label={t("Glyph info")}>
      {one ? (
        <>
          <div className="row">
            <span className="k-body-strong">{one.name}</span>
            <span className="spacer" />
            <span className="k-code muted">{one.unicode ? `U+${one.unicode}` : t("unencoded")}</span>
          </div>
          <div className="metric-row">
            {([["lsb", "LSB", one.lsb], ["width", "Width", one.width], ["rsb", "RSB", one.rsb]] as const).map(([key, label, value]) => (
              <label key={key}>
                <span className="k-caption muted">{t(label)}</span>
                {onSetMetric && value !== null ? (
                  <input key={`${one.name}:${value}`} className="metric-input" type="number" defaultValue={Math.round(value)} aria-label={t(label)}
                    onBlur={(e) => { const v = Number(e.currentTarget.value); if (e.currentTarget.value !== "" && Number.isFinite(v) && v !== Math.round(value)) onSetMetric(one.name, key, v).catch(() => {}); }}
                    onKeyDown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); if (e.key === "Escape") { e.currentTarget.value = String(Math.round(value)); e.currentTarget.blur(); } }} />
                ) : <output className="metric">{fmt(value)}</output>}
              </label>
            ))}
          </div>
          <p className="k-caption muted">{t(one.category)}{one.subcategory ? ` · ${t(one.subcategory)}` : ""} · {t(one.script)}{one.export ? "" : ` · ${t("not exported")}`}</p>
          {onSetUnicodes && (
            <label className="field info-field">
              <span className="k-caption muted">{t("Unicode")}</span>
              <input
                className="text-input k-code"
                value={uniDraft}
                placeholder={t("unencoded")}
                title={t("Type values like 0041 or U+0041, or the character itself; separate several with commas")}
                onChange={(e) => setUniDraft(e.target.value)}
                onBlur={commitUnicodes}
                onKeyDown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); if (e.key === "Escape") { setUniDraft(oneUni); e.currentTarget.blur(); } }}
              />
            </label>
          )}
          {onSetTags && (
            <label className="field info-field">
              <span className="k-caption muted">{t("Tags")}</span>
              <input
                className="text-input"
                value={tagsDraft}
                placeholder={t("tag, tag")}
                onChange={(e) => setTagsDraft(e.target.value)}
                onBlur={commitTags}
                onKeyDown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); }}
              />
            </label>
          )}
          {onSetGroups && (
            <div className="row info-groups">
              <label className="field">
                <span className="k-caption muted">{t("Left group")}</span>
                <input
                  className="text-input"
                  value={leftDraft}
                  onChange={(e) => setLeftDraft(e.target.value)}
                  onBlur={commitGroups}
                  onKeyDown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); }}
                />
              </label>
              <label className="field">
                <span className="k-caption muted">{t("Right group")}</span>
                <input
                  className="text-input"
                  value={rightDraft}
                  onChange={(e) => setRightDraft(e.target.value)}
                  onBlur={commitGroups}
                  onKeyDown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); }}
                />
              </label>
            </div>
          )}
          {onSetDirection && (
            <label className="field info-field">
              <span className="k-caption muted">{t("Writing direction")}</span>
              <select className="select" value={one.direction ?? ""} aria-label={t("Writing direction")} onChange={(e) => { onSetDirection(one.name, e.target.value).catch(() => {}); }}>
                <option value="">{t("Automatic")}</option>
                <option value="LTR">{t("Left to right")}</option>
                <option value="RTL">{t("Right to left")}</option>
                <option value="BIDI">{t("Both directions")}</option>
              </select>
            </label>
          )}
          {onSetVertical && one.vertical && (
            <fieldset className="vertical-metrics" title={t("Spacing for vertical typesetting. Editing a value turns vertical metrics on for the font.")}>
              <legend className="k-caption muted">{t("Vertical metrics")}</legend>
              <div className="metric-row">
                {([["tsb", "TSB", one.vertical.tsb], ["advance", "Height", one.vertical.advance], ["bsb", "BSB", one.vertical.bsb]] as const).map(([key, label, value]) => (
                  <label key={key}>
                    <span className="k-caption muted">{t(label)}</span>
                    {value !== null ? (
                      <input key={`${one.name}:${key}:${value}`} className="metric-input" type="number" defaultValue={Math.round(value)} aria-label={t(label)}
                        onBlur={(e) => { const v = Number(e.currentTarget.value); if (e.currentTarget.value !== "" && Number.isFinite(v) && v !== Math.round(value)) onSetVertical(one.name, key, v).catch(() => {}); }}
                        onKeyDown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); if (e.key === "Escape") { e.currentTarget.value = String(Math.round(value)); e.currentTarget.blur(); } }} />
                    ) : <output className="metric">{fmt(value)}</output>}
                  </label>
                ))}
              </div>
              {one.vertical.own && (
                <button type="button" className="link-btn k-caption" onClick={() => { onSetVertical(one.name, "reset", null).catch(() => {}); }}>{t("Use the default vertical metrics")}</button>
              )}
            </fieldset>
          )}
          {onSetProductionName && (
            <label className="field info-field">
              <span className="k-caption muted">{t("Production name")}</span>
              <input
                className="text-input"
                value={prodDraft}
                placeholder={one.name}
                onChange={(e) => setProdDraft(e.target.value)}
                onBlur={commitProd}
                onKeyDown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); }}
              />
            </label>
          )}
        </>
      ) : (
        <p className="k-caption muted">{selectedGlyphs.length > 1 ? tn(selectedGlyphs.length, "{n} glyph selected", "{n} glyphs selected") : t("Select a glyph to see its metrics.")}</p>
      )}
      {selectedGlyphs.length > 0 && onSetExport && (
        <label className="check-row" title={t("Glyphs that aren't exported (such as parts used only as components) are left out of fonts")}>
          <input
            type="checkbox"
            checked={selectedGlyphs.every((g) => g.export)}
            ref={(el) => { if (el) el.indeterminate = selectedGlyphs.some((g) => g.export) && !selectedGlyphs.every((g) => g.export); }}
            onChange={(e) => onSetExport(selectedGlyphs.map((g) => g.name), e.target.checked)}
            aria-label={t("Export the selected glyphs")}
          />
          <span className="k-caption">{selectedGlyphs.length > 1 ? t("Export these glyphs") : t("Export this glyph")}</span>
        </label>
      )}
      {selectedGlyphs.length > 0 && onSetColorLabel && <>
        <ColorLabelPicker caption="Colour label" current={selectedGlyphs.map((g) => g.colorLabel)} onPick={(c) => onSetColorLabel(selectedGlyphs.map((g) => g.name), null, c)} />
        <ColorLabelPicker caption="Layer colour" current={selectedGlyphs.map((g) => g.layerColorLabel)} onPick={(c) => onSetColorLabel(selectedGlyphs.map((g) => g.name), font.masterId, c)} />
      </>}
      {selectedGlyphs.length > 0 && onSetGlyphProperty && <GlyphPropertiesFields glyphs={selectedGlyphs} onSet={onSetGlyphProperty} />}
    </section>
  );

  return (
    <div className={`fontview${mode === "list" ? " is-list" : ""}`}>
      <aside className="sidebar">
        <label className="search">
          <SearchIcon />
          <input ref={searchRef} placeholder={t("Search name, Unicode, character")} value={query} onChange={(e) => setQuery(e.target.value)} />
        </label>
        <p className="k-overline muted sidebar-heading">{t("Categories")}</p>
        <ul className="list">
          {[["All", font.glyphs.length] as const, ...counts, ...(incompatibleCount ? [["Incompatible", incompatibleCount] as const] : []), ...(warningCount ? [["Interpolation warnings", warningCount] as const] : [])].map(([c, n]) => (
            <li key={c}>
              <button className={`list-item${category === c ? " is-selected" : ""}`} onClick={() => setCategory(c)}>
                <span>{t(c)}</span>
                <span className="list-meta">{n}</span>
              </button>
            </li>
          ))}
        </ul>
        {colorCounts.length > 0 && <>
          <p className="k-overline muted sidebar-heading">{t("Colours")}</p>
          <ul className="list">
            {colorCounts.map(([k, { label, n }]) => (
              <li key={k}>
                <button className={`list-item${colorFilter === k ? " is-selected" : ""}`} aria-pressed={colorFilter === k} onClick={() => setColorFilter(colorFilter === k ? null : k)}>
                  <span className="color-label-name"><span className="color-swatch" style={{ background: labelCss(label) }} aria-hidden="true" />{t(labelName(label))}</span>
                  <span className="list-meta">{n}</span>
                </button>
              </li>
            ))}
          </ul>
        </>}
        <p className="k-overline muted sidebar-heading">{t("Languages")}</p>
        <ul className="list">
          {languages.map((l) => (
            <li key={l.language} className="language-item">
              <button className={`list-item${openLanguage === l.language ? " is-selected" : ""}`} aria-expanded={openLanguage === l.language} onClick={() => setOpenLanguage(openLanguage === l.language ? null : l.language)}>
                <span>{t(l.language)}</span>
                <span className="list-meta">{l.present} / {l.total}</span>
              </button>
              {openLanguage === l.language && (
                <div className="language-missing">
                  {missing.length ? (
                    <>
                      <p className="k-caption muted">{tn(missing.length, "{n} glyph missing", "{n} glyphs missing")}</p>
                      <div className="missing-list">
                        {missing.map((m) => {
                          const ch = String.fromCodePoint(parseInt(m.unicode, 16));
                          return <span key={m.name} className="missing-chip" title={`${m.name} · U+${m.unicode}`}>{ch.trim() ? ch : m.name}</span>;
                        })}
                      </div>
                      {onAddMissing && (
                        <button type="button" className="btn btn-secondary btn-sm" onClick={() => onAddMissing(l.language).catch(() => {})}>{t("Add Missing")}</button>
                      )}
                    </>
                  ) : (
                    <p className="k-caption muted">{t("Nothing missing")}</p>
                  )}
                </div>
              )}
            </li>
          ))}
        </ul>
        {languageError && <p className="notice notice-danger k-caption sidebar-notice">{languageError}</p>}
        {onSetFilters && (
          <>
            <p className="k-overline muted sidebar-heading">{t("Filters")}</p>
            <ul className="list">
              {font.filters.map((f) => (
                <li key={f.name} className="filter-item">
                  <button className={`list-item${activeFilter === f.name ? " is-selected" : ""}`} onClick={() => setActiveFilter(activeFilter === f.name ? null : f.name)} title={f.kind === "list" ? t("List filter") : t("Smart filter")}>
                    <span>{f.name}</span>
                    <span className="list-meta">{f.kind === "list" ? t("list") : t("smart")}</span>
                  </button>
                  <button type="button" className="icon-btn" aria-label={t("Edit filter {name}", { name: f.name })} onClick={() => setFilterDialog({ initial: f })}>
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><path d="M4 20h4L19 9a2.8 2.8 0 0 0-4-4L4 16v4Z" /></svg>
                  </button>
                  <button type="button" className="icon-btn" aria-label={t("Delete filter {name}", { name: f.name })} onClick={() => saveFilters(font.filters.filter((x) => x.name !== f.name)).catch(() => {})}>
                    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
                  </button>
                </li>
              ))}
            </ul>
            <div className="sidebar-actions">
              <button type="button" className="btn btn-secondary btn-sm" onClick={() => setFilterDialog({ initial: null })}>{t("New Filter…")}</button>
              <button type="button" className="btn btn-secondary btn-sm" disabled={selection.size === 0} onClick={() => saveSelectionAsList().catch(() => {})}>{t("Save Selection as List")}</button>
            </div>
            {filterError && <p className="notice notice-danger k-caption sidebar-notice">{filterError}</p>}
          </>
        )}
        <span className="spacer" />
        {mode === "grid" && glyphInfo}
      </aside>
      <main className="grid-area">
        <div className="grid-header">
          <div className="segmented" role="group" aria-label={t("Font view layout")}>
            <button type="button" className={`segment${mode === "grid" ? " is-selected" : ""}`} aria-pressed={mode === "grid"} onClick={() => switchMode("grid")}>{t("Grid")}</button>
            <button type="button" className={`segment${mode === "list" ? " is-selected" : ""}`} aria-pressed={mode === "list"} onClick={() => switchMode("list")}>{t("List")}</button>
          </div>
          <div className="grid-title">
            <h2 className="k-subtitle">{activeFilter ?? (category === "All" ? t("All glyphs") : t(category))}</h2>
            <span className="k-caption muted">{tn(shown.length, "{n} glyph", "{n} glyphs")}{shownScript ? ` · ${t(shownScript)}` : ""}</span>
          </div>
          <span className="spacer" />
          {mode === "grid" && (
            <label className="cell-size">
              <span className="k-caption muted">{t("Cell size")}</span>
              <input type="range" min={64} max={160} step={8} value={cellSize} onChange={(e) => resizeCells(Number(e.target.value))} aria-label={t("Cell size")} />
            </label>
          )}
          {(onBatch || onRenameGlyphs) && <BatchMenu count={selection.size} onRun={onBatch} onRename={onRenameGlyphs} />}
          {onAddGlyphs && <button type="button" className="btn btn-primary btn-sm" onClick={onAddGlyphs}>{t("Add Glyphs…")}</button>}
        </div>
        {shown.length ? (
          mode === "list" ? (
            <GlyphListView glyphs={shown} metrics={font.metrics} selection={selection} onSelect={select} onOpen={onOpenGlyph} onEdit={listEdit} onSetExport={(names, on) => onSetExport?.(names, on)} dragProps={dragProps} dragName={dragName} dropName={dropName} />
          ) : (
          <VirtualGrid glyphs={shown} cellSize={cellSize} onKeyDown={(e) => {
            // Enter opens the selected glyphs in Edit View tabs.
            if (e.key === "Enter" && selection.size) {
              e.preventDefault();
              for (const g of font.glyphs) if (selection.has(g.name)) onOpenGlyph(g.name);
            }
          }} renderCell={(g) => (
            <GlyphCell key={g.name} glyph={g} font={font} selected={selection.has(g.name)} onSelect={(e) => select(g, e)} onOpen={() => onOpenGlyph(g.name)} drag={dragProps(g.name)} dragging={dragName === g.name} drop={dropName === g.name && dragName !== g.name} />
          )} />
          )
        ) : (
          font.glyphs.length === 0 ? (
            <div className="empty">
              <div className="empty-art">Aa</div>
              <p className="k-subtitle">{t("This font has no glyphs yet")}</p>
              <p className="k-body muted">{t("Add glyphs by name or character, or start from glyph sets.")}</p>
              {onAddGlyphs && <button className="btn btn-primary" onClick={onAddGlyphs}>{t("Add Glyphs…")}</button>}
            </div>
          ) : (
            <div className="empty">
              <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="empty-icon" aria-hidden="true"><circle cx="11" cy="11" r="6.5" /><path d="m20 20-4.2-4.2" /></svg>
              <p className="k-subtitle">{query.trim() ? t("No glyphs match “{query}”", { query }) : t("No glyphs here")}</p>
              {(category !== "All" || activeFilter || colorFilter !== null) && (
                <p className="k-caption muted">{t("Showing: {what}", { what: activeFilter ?? t(category) })}</p>
              )}
              <div className="row">
                <button className="btn btn-primary" onClick={() => { setQuery(""); setCategory("All"); setActiveFilter(null); setColorFilter(null); }}>{t("Clear search & filters")}</button>
                {onAddNamed && /^[A-Za-z_.][A-Za-z0-9_.\-]*$/.test(query.trim()) && (
                  <button className="btn btn-secondary" onClick={() => { const name = query.trim(); setQuery(""); setCategory("All"); setActiveFilter(null); onAddNamed(name); }}>
                    {t("Add “{name}” as a new glyph", { name: query.trim() })}
                  </button>
                )}
              </div>
            </div>
          )
        )}
      </main>
      {mode === "list" && (
        <aside className="glyph-props" aria-label={t("Glyph Properties")}>
          <div className="row">
            <h2 className="k-subtitle">{t("Glyph Properties")}</h2>
            <span className="spacer" />
            {one && <span className="k-caption muted">{one.name}</span>}
          </div>
          {one && (
            <div className="glyph-props-preview" aria-hidden="true">
              {one.svg ? (
                <svg viewBox={`${one.width / 2 - (font.metrics.ascender - font.metrics.descender) / 2} ${-font.metrics.ascender} ${font.metrics.ascender - font.metrics.descender} ${font.metrics.ascender - font.metrics.descender}`}>
                  <path d={one.svg} transform="scale(1,-1)" />
                </svg>
              ) : <span className="glyph-placeholder">{one.character?.trim() ?? ""}</span>}
            </div>
          )}
          {glyphInfo}
        </aside>
      )}
      <footer className="statusbar k-caption">
        <span>{t("{selected} selected · {shown} shown · {total} total · {drawn} drawn", { selected: selection.size, shown: shown.length, total: font.glyphs.length, drawn })}</span>
        <span className="spacer" />
        <span>{t("Master: {master}", { master: font.masters.find((m) => m.id === font.masterId)?.name ?? "" })} · {font.path ?? t("Not saved yet")}</span>
      </footer>
      {filterDialog && onSetFilters && (
        <SmartFilterDialog initial={filterDialog.initial} onCancel={() => setFilterDialog(null)} onSave={saveFromDialog} />
      )}
    </div>
  );
}
