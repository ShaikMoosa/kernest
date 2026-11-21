import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { backend, type AlignOp, type Annotation, type HoiKind, type HoiView, type GlyphPicture, type ImageData, type LayerImage, type PathPart, type SmartHandle, type Axis, type BackgroundEdit, type ComponentEdit, type ComponentView, type DocStatus, type DrawItem, type Guide, type Hint, type LayerInfo, type LayerView, type Measurement, type MetricName, type Paint, type PathEditOperation, type Selection } from "../lib/backend";
import { VariationPreview } from "./VariationPreview";
import { LayerDialog, type LayerDialogMode } from "./LayerDialog";
import { SmartDialog, type SmartDialogMode } from "./SmartDialog";
import {
  clonePaths, deleteSelection, ellipsePath, handleOwner, isOn, moveSelection, mulAffine, nodeKey, ORIGIN_LABELS, originPoint, parseKey, pathsToSvg, rectPath, svgBounds,
  type Bounds, type EditNode, type EditPath,
} from "../lib/outline";
import { CloseIcon, SearchIcon } from "./Icons";
import { pickImage, pickPng, pickSvgDrawing } from "../lib/desktop";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { t, tn, translateMessage } from "../lib/i18n";
import { magneticSnap } from "../lib/snap";
import { classifyHeight, zoneRange, type ZoneMark } from "../lib/zones";
import { combineSelection, extendLasso, insidePolygon, isLassoClick, selectModeFor, type LassoPt, type SelectMode } from "../lib/lasso";
import { ColorGlyph } from "./ColorGlyph";
import { PaintDialog } from "./PaintDialog";
import { PixelPreviewPanel } from "./PixelPreviewPanel";

type Tool = "select" | "pen" | "rect" | "ellipse" | "knife" | "pixel" | "annotate" | "hoi" | "hand" | "measure";
type AnnotSub = "select" | Annotation["kind"];
interface Pt { x: number; y: number }
interface View { s: number; ox: number; oy: number }
/** A component as drawn: the server's outline plus a live offset while dragging. */
interface Comp extends ComponentView { dx: number; dy: number }
type GuideScope = "local" | "global";
interface GuideRef { scope: GuideScope; index: number }

const TOOLS: { id: Tool; key: string; shift?: boolean; label: string; icon: React.ReactNode }[] = [
  { id: "select", key: "v", label: "Select (V)", icon: <path d="M6 3l12 9-6 1-3 6z" /> },
  { id: "pen", key: "p", label: "Pen (P)", icon: <><path d="M12 3l6 10-6 8-6-8z" /><circle cx="12" cy="12" r="1.6" /></> },
  { id: "rect", key: "f", label: "Rectangle (F)", icon: <rect x="5" y="6" width="14" height="12" rx="1" /> },
  { id: "ellipse", key: "e", shift: true, label: "Ellipse (Shift+E)", icon: <ellipse cx="12" cy="12" rx="7.5" ry="6.5" /> },
  { id: "knife", key: "e", label: "Knife (E)", icon: <><path d="M4 20l9-9" /><path d="M13 11l3.2-5.4a2.1 2.1 0 0 1 3.6 2.1L15 13z" /></> },
  { id: "pixel", key: "x", label: "Pixel (X)", icon: <><rect x="4" y="4" width="7" height="7" /><rect x="13" y="13" width="7" height="7" /><rect x="4" y="13" width="7" height="7" opacity="0.4" /></> },
  { id: "annotate", key: "a", label: "Annotate (A)", icon: <><path d="M5 4h14v11H9l-4 4z" /><path d="M9 9h6" /></> },
  { id: "hoi", key: "i", label: "Interpolation curves (I)", icon: <><path d="M5 18C6 9 13 6 19 6" strokeDasharray="2 2" /><path d="M5 15l3 3-3 3-3-3z" /><path d="M19 3l3 3-3 3-3-3z" /></> },
  { id: "measure", key: "l", label: "Measure (L)", icon: <><path d="M4 20L20 4" /><path d="M4 15v5h5" /><path d="M20 9V4h-5" /></> },
  { id: "hand", key: "h", label: "Hand (H) · or hold Space", icon: <path d="M8 12V6.5a1.5 1.5 0 0 1 3 0V11V5a1.5 1.5 0 0 1 3 0v6V6.5a1.5 1.5 0 0 1 3 0V14a6 6 0 0 1-6 6h-1a6 6 0 0 1-4.6-2.2L4 15a1.5 1.5 0 0 1 2.2-2z" /> },
];

/** The Select tool's lasso icon, shown in the toolbar while lasso selection is on. */
const LASSO_ICON = <><path d="M12 5c4.4 0 8 2 8 4.6S16.4 14.2 12 14.2 4 12.2 4 9.6 7.6 5 12 5z" strokeDasharray="2.6 2" /><path d="M6.5 12.6c-.8 1.6.2 3.4 2 3.4 1.4 0 1.6 1.6.6 2.6" /></>;
const SELECT_SHAPE_KEY = "kernest.selectShape";
function storedSelectShape(): "rect" | "lasso" {
  try { return localStorage.getItem(SELECT_SHAPE_KEY) === "lasso" ? "lasso" : "rect"; } catch { return "rect"; }
}

/** The Transform panel's align and distribute buttons: operation, label, icon. */
const ALIGN_BUTTONS: [AlignOp, string, React.ReactNode][] = [
  ["left", "Align left", <><path d="M4 3v18" /><rect x="7" y="6" width="11" height="4" rx="1" /><rect x="7" y="14" width="7" height="4" rx="1" /></>],
  ["h-center", "Align centres horizontally", <><path d="M12 3v18" /><rect x="5" y="6" width="14" height="4" rx="1" /><rect x="8" y="14" width="8" height="4" rx="1" /></>],
  ["right", "Align right", <><path d="M20 3v18" /><rect x="6" y="6" width="11" height="4" rx="1" /><rect x="10" y="14" width="7" height="4" rx="1" /></>],
  ["top", "Align top", <><path d="M3 4h18" /><rect x="6" y="7" width="4" height="11" rx="1" /><rect x="14" y="7" width="4" height="7" rx="1" /></>],
  ["v-center", "Align middles vertically", <><path d="M3 12h18" /><rect x="6" y="5" width="4" height="14" rx="1" /><rect x="14" y="8" width="4" height="8" rx="1" /></>],
  ["bottom", "Align bottom", <><path d="M3 20h18" /><rect x="6" y="6" width="4" height="11" rx="1" /><rect x="14" y="10" width="4" height="7" rx="1" /></>],
  ["distribute-h", "Distribute horizontally (equal gaps)", <><path d="M3 4v16M21 4v16" /><rect x="6" y="8" width="3" height="8" rx="1" /><rect x="15" y="8" width="3" height="8" rx="1" /><rect x="10.5" y="8" width="3" height="8" rx="1" /></>],
  ["distribute-v", "Distribute vertically (equal gaps)", <><path d="M4 3h16M4 21h16" /><rect x="8" y="6" width="8" height="3" rx="1" /><rect x="8" y="10.5" width="8" height="3" rx="1" /><rect x="8" y="15" width="8" height="3" rx="1" /></>],
];

/** The Transform panel's boolean buttons: operation, label, icon (two overlapping squares). */
const BOOLEAN_BUTTONS: ["union" | "subtract" | "intersect", string, React.ReactNode][] = [
  ["union", "Union: merge the selected paths (all paths when none is selected)", <path d="M4 4h10v6h6v10H10v-6H4z" fill="currentColor" fillOpacity="0.35" />],
  ["subtract", "Subtract: take the selected paths away from the others (the frontmost when none is selected)", <><path d="M4 4h10v6h-4v4H4z" fill="currentColor" fillOpacity="0.35" /><rect x="10" y="10" width="10" height="10" strokeDasharray="2 2" /></>],
  ["intersect", "Intersect: keep where the selected paths overlap the others", <><rect x="4" y="4" width="10" height="10" strokeDasharray="2 2" /><rect x="10" y="10" width="10" height="10" strokeDasharray="2 2" /><rect x="10" y="10" width="4" height="4" fill="currentColor" fillOpacity="0.6" /></>],
];

const HIT = 7; // px
const EDGE = 10; // px strip at the canvas border: dragging from it pulls out a new guide

/** Commands sent from the app menus to the active Edit View. */
export type EditCommand = "add-component" | "cut" | "copy" | "paste" | "correct-direction" | "add-extremes" | "remove-overlap"
  | "edit-background" | "to-background" | "add-to-background" | "swap-background" | "clear-background"
  | "open-corner" | "reconnect-nodes" | "connect-nodes" | "close-open-paths" | "make-node-first" | "reverse-contours" | "tidy-up" | "round-coordinates"
  | "harmonize" | "super-smooth" | "align-selection" | "union" | "subtract" | "intersect" | "focus-path" | "clear-focus"
  | "select-all" | "deselect-all" | "invert-selection" | "zoom-in" | "zoom-out" | "zoom-fit" | "zoom-actual" | "shape-order";
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
  return { base: c.base, transform: t, smart: c.smart, align: c.align };
};
const fromView = (cs: ComponentView[]): Comp[] => cs.map((c) => ({ ...c, dx: 0, dy: 0 }));

function MetricField({ label, value, disabled, onCommit, metricKey, onKey, onInvalid, decimals }: { label: string; value: number | null; disabled?: boolean; onCommit: (v: number) => void; metricKey?: string; onKey?: (key: string | null) => void; onInvalid?: (message: string) => void; decimals?: boolean }) {
  const shown = value === null ? "" : decimals ? String(Math.round(value * 10) / 10) : String(Math.round(value));
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);
  const commit = (raw: string) => {
    const s = raw.trim();
    // A metrics key such as "=n" or "=|H+10" makes this value follow another glyph.
    if (onKey && s.startsWith("=")) {
      if (s !== metricKey) onKey(s);
      return setText(shown);
    }
    const v = Number(s.replace("−", "-"));
    if (s === "" || raw === shown) return setText(shown);
    if (!Number.isFinite(v)) {
      setText(shown);
      return onInvalid?.(onKey ? t("{name}: enter a number, or a metrics key such as =n.", { name: label }) : t("{name}: enter a number.", { name: label }));
    }
    onCommit(v);
  };
  return (
    <label className="info-field" title={onKey ? t("Type a number, or a metrics key such as =n, =|H or =o+10") : undefined}>
      <span className="k-caption muted">
        {label}
        {metricKey && (
          <span className="metric-key">
            {" "}{metricKey}
            <button type="button" className="metric-key-clear" aria-label={t("Remove the {name} metrics key", { name: label })} onClick={(e) => { e.preventDefault(); onKey?.(null); }}>×</button>
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
          <h2 id="add-comp-title" className="k-subtitle">{t("Add Component")}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onCancel}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          <label className="search">
            <SearchIcon />
            <input autoFocus placeholder={t("Glyph name")} value={q} onChange={(e) => setQ(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "ArrowDown") { setIndex((i) => Math.min(i + 1, shown.length - 1)); e.preventDefault(); }
                else if (e.key === "ArrowUp") { setIndex((i) => Math.max(i - 1, 0)); e.preventDefault(); }
                else if (e.key === "Enter" && shown[index]) onPick(shown[index]);
                else if (e.key === "Escape") onCancel();
              }} />
          </label>
          <ul className="picker-list" role="listbox" aria-label={t("Glyphs")}>
            {shown.map((n, i) => (
              <li key={n} role="option" aria-selected={i === index}>
                <button className={`list-item${i === index ? " is-selected" : ""}`} onClick={() => onPick(n)} onMouseEnter={() => setIndex(i)}>{n}</button>
              </li>
            ))}
            {shown.length === 0 && <li className="k-caption muted">{t("No glyph matches “{query}”.", { query: q })}</li>}
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
  onCommitted: (status: DocStatus) => void; /** A change that adds glyphs (the pixel glyph): the app reloads the font. */ onStructureChanged?: (s: DocStatus) => unknown;
  onDrawn?: () => void;
  handleSize?: "small" | "medium" | "large";
  onError: (e: unknown) => void;
  /** Non-fatal notices, e.g. an edit that only reached the current master. */
  onWarning?: (text: string) => void;
  /** View › Show Metrics: metric lines, labels and the advance box. */
  showMetrics?: boolean;
  /** View › Show Anchors: anchor diamonds and their names. */
  showAnchors?: boolean;
  /** View › Show Nodes: points and their handles. */
  showNodes?: boolean;
  /** View › Show Guides: local and global guides. */
  showGuides?: boolean;
  /** Alignment zone bands and the node highlights for metric lines and zones. */
  showZones?: boolean;
  /** Screen pixels within which a dragged node snaps (0: off). */
  snapDistance?: number;
  /** View › Show Background: the background drawing behind the outline. */
  showBackground?: boolean;
  /** View › Show Image: the layer's placed image (always shown for empty glyphs). */
  showImage?: boolean;
  /** View › Show Annotations: review notes on the canvas. */
  showAnnotations?: boolean;
  /** View › Show Bounding Box: a dashed box around the drawing. */
  showBoundingBox?: boolean;
  /** View › Show Sidebar: the Transform/Hints/Pixel preview column. */
  showSidebar?: boolean;
  onToggleSidebar?: () => void;
  /** Toolbar view toggles (Nodes, Metrics, Anchors, Guides, Background), shared with the View menu. */
  onToggleView?: (view: "nodes" | "metrics" | "anchors" | "guides" | "background" | "zones") => void;
  /** The current master's id; `masterId` differs when a brace or bracket layer is open. */
  masterKey?: string;
  /** Switches this tab to another layer (a layer key). */
  onPickLayer?: (key: string) => void;
  /** Enabled reporter plug-ins; their drawing is overlaid on the canvas. */
  reporters?: string[];
}

/** S05 · Edit View — draw and edit one glyph layer. */
export function EditView({ glyph, masterId, version, active, glyphs, onCommitted, onStructureChanged, onDrawn, handleSize = "medium", onError, onWarning, showMetrics = true, showAnchors = true, showNodes = true, showGuides = true, showZones = true, snapDistance = 6, showBackground = true, showImage = true, showAnnotations = true, showBoundingBox = false, showSidebar = true, onToggleSidebar, onToggleView, masterKey, onPickLayer, reporters = [] }: Props) {
  const master = masterKey ?? masterId;
  const nodeScale = handleSize === "small" ? 0.75 : handleSize === "large" ? 1.5 : 1;
  // ---------- Reporter plug-ins ----------
  const [reporterItems, setReporterItems] = useState<DrawItem[]>([]);
  const reporterSeq = useRef(0);
  const reporterKey = reporters.join("|");
  useEffect(() => {
    if (!active || reporters.length === 0) { setReporterItems([]); return; }
    const id = ++reporterSeq.current;
    const timer = setTimeout(() => {
      Promise.all(reporters.map((name) => backend.reporterDraw(name, glyph, masterId).catch(() => [] as DrawItem[])))
        .then((lists) => { if (id === reporterSeq.current) setReporterItems(lists.flat()); });
    }, 250);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, reporterKey, glyph, masterId, version]);
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
      if (!info.axes.length) return onError(t("Add an axis in Font Info first: intermediate and alternate layers are positions on the axes."));
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
  // ---------- Smart glyphs ----------
  const [smartDialog, setSmartDialog] = useState<SmartDialogMode | null>(null);
  const saveSmartDialog = useCallback(async (v: { axes?: { name: string; min: number; max: number }[]; pole?: Record<string, number> }) => {
    if (!smartDialog) return;
    if (smartDialog.kind === "axes") {
      const s = await backend.setSmartAxes(glyph, v.axes ?? []);
      setSmartDialog(null);
      onCommitted(s);
      setLayersVersion((n) => n + 1);
      // The layer view reloads with the new axes.
      backend.glyphLayer(glyph, masterId).then((l) => { setLayer(l); setPaths(l.paths); setComps(fromView(l.components)); }, onError);
    } else {
      const r = await backend.addSmartPole(glyph, master, v.pole ?? {});
      setSmartDialog(null);
      onCommitted(r.status);
      setLayersVersion((n) => n + 1);
      onPickLayer?.(r.key);
    }
  }, [smartDialog, glyph, masterId, master, onCommitted, onPickLayer, onError]);

  // ---------- Colour layers ----------
  const [painting, setPainting] = useState(false);
  const addColorLayer = useCallback(async () => {
    try {
      const r = await backend.addColorLayer(glyph, master);
      onCommitted(r.status);
      setLayersVersion((n) => n + 1);
      onPickLayer?.(r.key);
    } catch (e) {
      onError(e);
    }
  }, [glyph, master, onCommitted, onPickLayer, onError]);
  const moveColorLayer = useCallback(async (up: boolean) => {
    try {
      const r = await backend.moveColorLayer(glyph, masterId, up);
      onCommitted(r.status);
      setLayersVersion((n) => n + 1);
      backend.glyphLayer(glyph, masterId).then(setLayer, onError);
    } catch (e) {
      onError(e);
    }
  }, [glyph, masterId, onCommitted, onError]);
  const savePaint = useCallback(async (paint: Paint, scope: string) => {
    const r = scope === "all"
      ? await backend.setColorPaint(glyph, masterId, paint)
      : await backend.setMasterPaint(glyph, masterId, scope, paint);
    setPainting(false);
    onCommitted(r.status);
    setLayersVersion((n) => n + 1);
    backend.glyphLayer(glyph, masterId).then(setLayer, onError);
  }, [glyph, masterId, onCommitted, onError]);

  const fixInterpolation = useCallback(async () => {
    try {
      const r = await backend.fixInterpolation(glyph);
      onCommitted(r.status);
      backend.glyphLayer(glyph, masterId).then((l) => { setLayer(l); setPaths(l.paths); setComps(fromView(l.components)); }, onError);
    } catch (e) {
      onError(e);
    }
  }, [glyph, masterId, onCommitted, onError]);

  /** Saves the master's drawing as a backup named after the time, and shows it. */
  const addBackup = useCallback(async () => {
    try {
      const now = new Date();
      const name = t("Backup {date}", { date: now.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }) });
      const r = await backend.addBackupLayer(glyph, master, name);
      onCommitted(r.status);
      setLayersVersion((n) => n + 1);
    } catch (e) {
      onError(e);
    }
  }, [glyph, master, onCommitted, onError]);

  /** Swaps the shown backup with the master drawing, then shows the master. */
  const useBackup = useCallback(async () => {
    try {
      onCommitted(await backend.useBackupLayer(glyph, masterId));
      setLayersVersion((n) => n + 1);
      onPickLayer?.(master);
    } catch (e) {
      onError(e);
    }
  }, [glyph, masterId, master, onCommitted, onPickLayer, onError]);

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
  const [anchors, setAnchors] = useState<LayerView["anchors"]>([]);
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const [compSel, setCompSel] = useState<Set<number>>(new Set());
  const [anchorSel, setAnchorSel] = useState<Set<string>>(new Set());
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; wx: number; wy: number; node: [number, number] | null; path: number | null } | null>(null);
  // Focus dims every path but this one, so a single contour can be worked on in peace.
  const [focusPath, setFocusPath] = useState<number | null>(null);
  const [tool, setTool] = useState<Tool>("select");
  const [view, setView] = useState<View>({ s: 0.5, ox: 0, oy: 0 });
  const [size, setSize] = useState({ w: 800, h: 600 });
  const [cursor, setCursor] = useState<Pt | null>(null);
  const [marquee, setMarquee] = useState<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  // Select tool: rectangle or lasso (free-form) selection; Shift+V switches.
  const [selectShape, setSelectShapeState] = useState<"rect" | "lasso">(storedSelectShape);
  const setSelectShape = (shape: "rect" | "lasso") => {
    setSelectShapeState(shape);
    try { localStorage.setItem(SELECT_SHAPE_KEY, shape); } catch { /* not kept */ }
  };
  // Each glyph tab follows the shape last chosen in any tab.
  useEffect(() => { if (active) setSelectShapeState(storedSelectShape()); }, [active]);
  const [lassoPts, setLassoPts] = useState<LassoPt[] | null>(null);
  const [selectMenu, setSelectMenu] = useState(false);
  const selectHold = useRef<number | null>(null);
  const [preview, setPreview] = useState<EditPath | null>(null);
  const [spaceDown, setSpaceDown] = useState(false);
  const [picking, setPicking] = useState(false);
  // Choosing a corner, cap or segment glyph for a node.
  const [partPick, setPartPick] = useState<{ kind: PathPart["kind"]; path: number; node: number } | null>(null);
  const [partsOpen, setPartsOpen] = useState(true);
  // Placed image: its picture, a live drag offset, and the panel.
  const [imageData, setImageData] = useState<ImageData | null>(null);
  const [imgDrag, setImgDrag] = useState<{ dx: number; dy: number } | null>(null);
  const [imageOpen, setImageOpen] = useState(false);
  // sbix bitmaps and the SVG drawing (S63): which one is shown on the canvas.
  const [pictures, setPictures] = useState<GlyphPicture[]>([]);
  const [picturesOpen, setPicturesOpen] = useState(false);
  const [shownPicture, setShownPicture] = useState<number | null>(null);
  const [newPpem, setNewPpem] = useState(128);
  // Annotation tool: the palette's sub-tool, the selected note, a live drag offset.
  const [annSub, setAnnSub] = useState<AnnotSub>("select");
  const [annSel, setAnnSel] = useState<number | null>(null);
  const [annDrag, setAnnDrag] = useState<{ dx: number; dy: number } | null>(null);
  // Interpolation curves (HOI tool): the view for this layer, the axis, the chosen node, a live knob.
  const [hoi, setHoi] = useState<HoiView | null>(null);
  const [hoiAxis, setHoiAxis] = useState("");
  const [hoiSel, setHoiSel] = useState<{ pi: number; ni: number } | null>(null);
  const [hoiDrag, setHoiDrag] = useState<{ pi: number; ni: number; x: number; y: number } | null>(null);
  const [shapesOpen, setShapesOpen] = useState(false);
  const [orderAll, setOrderAll] = useState(true);
  // A smart handle being dragged: which component, which handle, the value so far.
  const [smartDrag, setSmartDrag] = useState<{ ci: number; h: SmartHandle; value: number } | null>(null);
  const [localGuides, setLocalGuides] = useState<Guide[]>([]);
  const [globalGuides, setGlobalGuides] = useState<Guide[]>([]);
  const [guideSel, setGuideSel] = useState<GuideRef | null>(null);
  const [guideDraft, setGuideDraft] = useState<Guide | null>(null);
  const [snapGuide, setSnapGuide] = useState<GuideRef | null>(null);
  // The magnetic line a drag snapped to, drawn as feedback until the mouse is let go.
  const [snapLine, setSnapLine] = useState<{ x?: number; y?: number } | null>(null);
  const [editingBackground, setEditingBackground] = useState(false);
  const [measureLine, setMeasureLine] = useState<{ p0: Pt; p1: Pt } | null>(null);
  const [measurement, setMeasurement] = useState<Measurement | null>(null);
  const [knifeLine, setKnifeLine] = useState<{ p0: Pt; p1: Pt } | null>(null);
  // Pixel tool: the cells painted (or erased) during the current drag.
  const [pixelDraft, setPixelDraft] = useState<{ on: boolean; cells: [number, number][] } | null>(null);
  const [askPixelGlyph, setAskPixelGlyph] = useState(false);
  const [transformOpen, setTransformOpen] = useState(false);
  const [layersOpen, setLayersOpen] = useState(true);
  const [hintsOpen, setHintsOpen] = useState(false);
  const [strokeOpen, setStrokeOpen] = useState(true);
  // Align buttons: what they line the selection up with (SMO-40).
  const [alignTo, setAlignTo] = useState<"selection" | "width" | "baseline" | "xHeight" | "capHeight" | "ascender" | "descender">("selection");
  const [tf, setTf] = useState({ sx: 100, sy: 100, rot: 0, kx: 0, ky: 0, mh: false, mv: false, origin: 4, allMasters: false });
  const [, rerender] = useState(0);

  const wrapRef = useRef<HTMLDivElement>(null);
  const svgRef = useRef<SVGSVGElement>(null);
  const fitted = useRef(false);
  const drag = useRef<null | { mode: string; startScreen: Pt; startWorld: Pt; base: EditPath[]; baseComps: Comp[]; baseAnchors: LayerView["anchors"]; moved: boolean; anchor?: string; selMode?: SelectMode; lasso?: LassoPt[]; pi?: number; ni?: number; startView?: View; lastPaths?: EditPath[]; lastComps?: Comp[]; lastAnchors?: LayerView["anchors"]; scope?: GuideScope; gi?: number; angle?: number; baseLocal?: Guide[]; baseGlobal?: Guide[]; lastGuide?: Guide }>(null);
  const pen = useRef<{ path: number | null; pendingOut: Pt | null }>({ path: null, pendingOut: null });
  const chain = useRef(Promise.resolve());
  const measureSeq = useRef(0);
  const latest = useRef({ layer, paths, comps, anchors, selection, compSel, anchorSel, view, editingBackground, localGuides, globalGuides, guideSel });
  latest.current = { layer, paths, comps, anchors, selection, compSel, anchorSel, view, editingBackground, localGuides, globalGuides, guideSel };

  // ---------- Loading & committing ----------

  useEffect(() => {
    let cancelled = false;
    backend.glyphLayer(glyph, masterId).then((l) => {
      if (cancelled) return;
      setLayer(l);
      setFocusPath(null);
      // While editing the background, the drawing state holds the background's content.
      const bg = latest.current.editingBackground;
      const act = bg ? l.background?.paths ?? [] : l.paths;
      const actComps = bg ? l.background?.components ?? [] : l.components;
      setPaths(act);
      setComps(fromView(actComps));
      setAnchors(bg ? [] : l.anchors);
      setLocalGuides(l.guides);
      setGlobalGuides(l.masterGuides);
      setSelection((sel) => new Set([...sel].filter((k) => { const [pi, ni] = parseKey(k); return !!act[pi]?.nodes[ni]; })));
      setCompSel((sel) => new Set([...sel].filter((i) => i < actComps.length)));
      setAnchorSel((sel) => new Set(bg ? [] : [...sel].filter((n) => l.anchors.some((a) => a.name === n))));
      setGuideSel((sel) => {
        if (!sel) return null;
        const list = sel.scope === "local" ? l.guides : l.masterGuides;
        return sel.index < list.length ? sel : null;
      });
    }, (e) => {
      if (cancelled) return;
      // A brace or bracket layer that no longer exists (undo, removed elsewhere): go back
      // to the master's own layer.
      if (masterId !== master && onPickLayer) onPickLayer(master);
      else onError(e);
    });
    return () => { cancelled = true; };
  }, [glyph, masterId, version, onError, master, onPickLayer]);

  const accept = useCallback((l: LayerView, bgOverride?: boolean) => {
    setLayer(l);
    const bg = bgOverride ?? latest.current.editingBackground;
    setPaths(bg ? l.background?.paths ?? [] : l.paths);
    setComps(fromView(bg ? l.background?.components ?? [] : l.components));
    setAnchors(bg ? [] : l.anchors);
    setLocalGuides(l.guides);
    setGlobalGuides(l.masterGuides);
    setAnchorSel((sel) => new Set(bg ? [] : [...sel].filter((n) => l.anchors.some((a) => a.name === n))));
    onCommitted(l.status);
  }, [onCommitted]);

  useEffect(() => {
    if (!showAnchors) setAnchorSel(new Set());
  }, [showAnchors]);

  // A removed path can't stay focused.
  useEffect(() => {
    setFocusPath((f) => (f !== null && f >= paths.length ? null : f));
  }, [paths.length]);

  // ---------- Anchors ----------

  /** Sends anchor moves (font-unit deltas) as one undoable step; components follow. */
  const moveAnchorsBy = useCallback((moves: { name: string; dx: number; dy: number }[]) => {
    if (!moves.length) return;
    chain.current = chain.current.then(async () => {
      try {
        accept(await backend.moveAnchors(glyph, masterId, moves, true));
      } catch (e) {
        onError(e);
      }
    });
  }, [glyph, masterId, accept, onError]);

  const addAnchorAt = useCallback((x: number, y: number) => {
    chain.current = chain.current.then(async () => {
      const taken = new Set(latest.current.anchors.map((a) => a.name));
      // A sensible first name; the info box renames it right away.
      let name = ["top", "bottom"].find((n) => !taken.has(n));
      if (!name) {
        let i = 1;
        while (taken.has(`anchor_${i}`)) i++;
        name = `anchor_${i}`;
      }
      try {
        accept(await backend.addAnchor(glyph, masterId, name, x, y, true));
        setSelection(new Set());
        setCompSel(new Set());
        setAnchorSel(new Set([name]));
      } catch (e) {
        onError(e);
      }
    });
  }, [glyph, masterId, accept, onError]);

  const renameAnchorTo = useCallback((old: string, newName: string) => {
    chain.current = chain.current.then(async () => {
      try {
        accept(await backend.renameAnchor(glyph, masterId, old, newName, true));
        setAnchorSel(new Set([newName]));
      } catch (e) {
        onError(e);
      }
    });
  }, [glyph, masterId, accept, onError]);

  const removeAnchorsNamed = useCallback((names: string[]) => {
    if (!names.length) return;
    chain.current = chain.current.then(async () => {
      try {
        accept(await backend.removeAnchors(glyph, masterId, names, true));
        setAnchorSel(new Set());
      } catch (e) {
        onError(e);
      }
    });
  }, [glyph, masterId, accept, onError]);

  // ---------- Hints ----------

  /** Replaces the layer's manual hints as one undoable step. */
  const setHints = useCallback((next: Hint[]) => {
    chain.current = chain.current.then(async () => {
      try {
        accept(await backend.setGlyphHints(glyph, masterId, next));
      } catch (e) {
        onError(e);
      }
    });
  }, [glyph, masterId, accept, onError]);

  const setComponentAlign = useCallback((index: number, align: boolean) => {
    chain.current = chain.current.then(async () => {
      try {
        accept(await backend.setComponentAlign(glyph, masterId, index, align));
      } catch (e) {
        onError(e);
      }
    });
  }, [glyph, masterId, accept, onError]);

  const setComponentSmart = useCallback((index: number, values: Record<string, number>) => {
    chain.current = chain.current.then(async () => {
      try {
        accept(await backend.setComponentSmart(glyph, masterId, index, values));
      } catch (e) {
        onError(e);
      }
    });
  }, [glyph, masterId, accept, onError]);

  /** Sends the edited outlines (and components) to the backend as one undoable step. */
  const commit = useCallback((next: EditPath[], nextComps?: Comp[], width?: number) => {
    chain.current = chain.current.then(async () => {
      const l = latest.current.layer;
      if (!l) return;
      const bg = latest.current.editingBackground;
      const cs = (nextComps ?? latest.current.comps).map(toEdit);
      try {
        accept(bg
          ? await backend.updateBackground(glyph, masterId, next, cs, l.background?.anchors ?? [])
          : await backend.updateLayer(glyph, masterId, width ?? l.width, next, cs));
        if (!bg && next.some(path => path.nodes.length > 0)) onDrawn?.();
      } catch (e) {
        setPaths(bg ? l.background?.paths ?? [] : l.paths);
        setComps(fromView(bg ? l.background?.components ?? [] : l.components));
        onError(e);
      }
    });
  }, [glyph, masterId, accept, onError, onDrawn]);

  /** Path operations (knife, corners, connecting, tidy…) as one undoable step. */
  const runPathEdit = useCallback((operation: PathEditOperation, opts?: { paths?: number[]; nodes?: [number, number][]; points?: [number, number][] }) => {
    chain.current = chain.current.then(async () => {
      try {
        const r = await backend.pathEdit(glyph, masterId, operation, opts);
        accept(r.layer);
        // Node indices shift after most operations; start from a clean selection.
        setSelection(new Set());
        if (r.warning) onWarning?.(translateMessage(r.warning));
      } catch (e) {
        onError(e);
      }
    });
  }, [glyph, masterId, accept, onError, onWarning]);

  /** Locks or unlocks one path; a locked path can't be selected or edited. */
  const togglePathLocked = useCallback((pi: number) => {
    chain.current = chain.current.then(async () => {
      try {
        const locked = !(latest.current.paths[pi]?.locked ?? false);
        accept(await backend.setPathLocked(glyph, masterId, pi, locked));
        if (locked) setSelection((sel) => new Set([...sel].filter((k) => parseKey(k)[0] !== pi)));
      } catch (e) {
        onError(e);
      }
    });
  }, [glyph, masterId, accept, onError]);

  const saveAnnotations = useCallback((list: Annotation[]) => {
    chain.current = chain.current.then(async () => {
      try {
        accept(await backend.setAnnotations(glyph, masterId, list));
      } catch (e) {
        onError(e);
      }
    });
  }, [glyph, masterId, accept, onError]);

  /** Glyph › Add Image… (or the Image panel): pick a file and place it. */
  const addImage = useCallback(async () => {
    try {
      const file = await pickImage();
      if (!file) return;
      accept(await backend.addLayerImage(glyph, masterId, file));
      setImageOpen(true);
    } catch (e) {
      onError(e);
    }
  }, [glyph, masterId, accept, onError]);

  // The placed image's picture: fetched when the image (or its file) changes.
  const imagePath = layer?.image?.path ?? null;
  useEffect(() => {
    if (!imagePath) { setImageData(null); return; }
    let live = true;
    backend.layerImageData(glyph, masterId).then((d) => { if (live) setImageData(d); }, () => { if (live) setImageData(null); });
    return () => { live = false; };
  }, [glyph, masterId, imagePath]);
  // The glyph's bitmaps and SVG: fetched per glyph and after each edit.
  useEffect(() => {
    let live = true;
    backend.glyphPictures(glyph).then((p) => { if (live) setPictures(p); }, () => { if (live) setPictures([]); });
    return () => { live = false; };
    // The layer object changes after every edit and undo, which refreshes the list.
  }, [glyph, layer]);
  const pictureCall = useCallback((run: () => Promise<GlyphPicture[]>) => {
    chain.current = chain.current.then(async () => {
      try {
        setPictures(await run());
      } catch (e) {
        onError(e);
      }
    });
  }, [onError]);
  const addBitmap = useCallback(async () => {
    const file = await pickPng();
    if (!file) return;
    pictureCall(() => backend.addGlyphBitmap(glyph, file, newPpem));
    setShownPicture(newPpem);
  }, [glyph, newPpem, pictureCall]);
  const addSvgDrawing = useCallback(async () => {
    const file = await pickSvgDrawing();
    if (!file) return;
    pictureCall(() => backend.addGlyphSvg(glyph, file));
    setShownPicture(0);
  }, [glyph, pictureCall]);
  useEffect(() => {
    if (tool !== "hoi") { setHoi(null); setHoiSel(null); return; }
    let live = true;
    backend.hoiView(glyph, masterId, hoiAxis).then((v) => { if (live) setHoi(v); }, (e) => { if (live) { setHoi(null); onError(e); } });
    return () => { live = false; };
    // The layer object changes after every edit, which refreshes the curves.
  }, [glyph, masterId, tool, hoiAxis, layer, onError]);
  useEffect(() => {
    if (!active || tool !== "annotate" || annSel === null) return;
    const on = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement | null;
      if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA")) return;
      if (e.key === "Delete" || e.key === "Backspace") {
        e.preventDefault();
        e.stopImmediatePropagation();
        const list = layer?.annotations ?? [];
        saveAnnotations(list.filter((_, i) => i !== annSel));
        setAnnSel(null);
      }
    };
    window.addEventListener("keydown", on, true);
    return () => window.removeEventListener("keydown", on, true);
  }, [active, tool, annSel, layer, saveAnnotations]);
  useEffect(() => {
    if (!active) return;
    const on = () => { addImage(); };
    window.addEventListener("kernest:add-image", on);
    return () => window.removeEventListener("kernest:add-image", on);
  }, [active, addImage]);

  /** Runs a stroke command and shows the layer it returns. */
  const strokeCall = useCallback((run: () => Promise<LayerView>) => {
    chain.current = chain.current.then(async () => {
      try {
        accept(await run());
      } catch (e) {
        onError(e);
      }
    });
  }, [accept, onError]);

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

  // ---------- Guides ----------

  /** Saves a guide list as one undoable step; global guides reach every glyph of the master. */
  const saveGuides = useCallback((scope: GuideScope, guides: Guide[]) => {
    chain.current = chain.current.then(async () => {
      try {
        if (scope === "local") accept(await backend.setGuides(glyph, masterId, guides));
        else {
          const info = await backend.setMasterGuides(master, guides);
          onCommitted(info.status);
          const l = await backend.glyphLayer(glyph, masterId);
          setLayer(l);
          setLocalGuides(l.guides);
          setGlobalGuides(l.masterGuides);
        }
      } catch (e) {
        onError(e);
      }
    });
  }, [glyph, masterId, master, accept, onCommitted, onError]);

  const updateGuide = useCallback((ref: GuideRef, patch: Partial<Guide>) => {
    const list = ref.scope === "local" ? latest.current.localGuides : latest.current.globalGuides;
    saveGuides(ref.scope, list.map((g, i) => (i === ref.index ? { ...g, ...patch } : g)));
  }, [saveGuides]);

  const deleteGuide = useCallback((ref: GuideRef) => {
    const list = ref.scope === "local" ? latest.current.localGuides : latest.current.globalGuides;
    saveGuides(ref.scope, list.filter((_, i) => i !== ref.index));
    setGuideSel(null);
  }, [saveGuides]);

  const addGuideAt = useCallback((x: number, y: number, angle: number) => {
    const list = latest.current.localGuides;
    saveGuides("local", [...list, { x, y, angle, locked: false }]);
    setGuideSel({ scope: "local", index: list.length });
  }, [saveGuides]);

  /** Moves the selected guide between the layer and the master as one undoable step. */
  const switchGuideScope = useCallback((ref: GuideRef) => {
    const toGlobal = ref.scope === "local";
    chain.current = chain.current.then(async () => {
      try {
        const l = await backend.setGuideScope(glyph, masterId, ref.index, toGlobal);
        accept(l);
        setGuideSel({ scope: toGlobal ? "global" : "local", index: (toGlobal ? l.masterGuides : l.guides).length - 1 });
      } catch (e) {
        onError(e);
      }
    });
  }, [glyph, masterId, accept, onError]);

  // ---------- Background ----------

  /** Ctrl+B: the background becomes the drawing; the outline shows as a tint behind it. */
  const toggleBackground = useCallback(() => {
    const l = latest.current.layer;
    if (!l) return;
    const next = !latest.current.editingBackground;
    setEditingBackground(next);
    setPaths(next ? l.background?.paths ?? [] : l.paths);
    setComps(fromView(next ? l.background?.components ?? [] : l.components));
    setAnchors(next ? [] : l.anchors);
    setSelection(new Set());
    setCompSel(new Set());
    setAnchorSel(new Set());
    pen.current = { path: null, pendingOut: null };
  }, []);

  /** The current selection for transform and background commands; empty means the whole layer. */
  const currentSelection = useCallback((): Selection => {
    const { selection, compSel, anchorSel, editingBackground } = latest.current;
    return {
      nodes: [...selection].map(parseKey),
      components: [...compSel],
      anchors: editingBackground ? [] : [...anchorSel],
    };
  }, []);

  // ---------- Transform panel ----------

  const contentBounds = (): Bounds | null => {
    const { paths, comps, selection, compSel } = latest.current;
    const useSel = selection.size > 0 || compSel.size > 0;
    const pathIdx = new Set([...selection].map((k) => parseKey(k)[0]));
    let b: Bounds | null = null;
    const add = (nb: Bounds | null) => {
      if (!nb) return;
      b = b === null ? nb : { minX: Math.min(b.minX, nb.minX), minY: Math.min(b.minY, nb.minY), maxX: Math.max(b.maxX, nb.maxX), maxY: Math.max(b.maxY, nb.maxY) };
    };
    paths.forEach((p, i) => { if (!useSel || pathIdx.has(i)) add(svgBounds(pathsToSvg([p]))); });
    comps.forEach((c, i) => {
      if (!useSel || compSel.has(i)) {
        const cb = svgBounds(c.svg);
        if (cb) add({ minX: cb.minX + c.dx, minY: cb.minY + c.dy, maxX: cb.maxX + c.dx, maxY: cb.maxY + c.dy });
      }
    });
    return b;
  };

  const applyTransform = () => {
    const b = contentBounds();
    const o = b ? originPoint(b, tf.origin) : { x: 0, y: 0 };
    const r = (tf.rot * Math.PI) / 180;
    const scale = [tf.sx / 100, 0, 0, tf.sy / 100, 0, 0];
    const skew = [1, Math.tan((tf.ky * Math.PI) / 180), Math.tan((tf.kx * Math.PI) / 180), 1, 0, 0];
    const rotate = [Math.cos(r), Math.sin(r), -Math.sin(r), Math.cos(r), 0, 0];
    const mirror = [tf.mh ? -1 : 1, 0, 0, tf.mv ? -1 : 1, 0, 0];
    // Same order as the engine's panel affine: scale, skew, rotate, mirror.
    // The engine wraps it about the origin, so it is sent without a shift.
    const affine = mulAffine(mirror, mulAffine(rotate, mulAffine(skew, scale)));
    const bg = latest.current.editingBackground;
    chain.current = chain.current.then(async () => {
      try {
        accept(await backend.transform(glyph, masterId, currentSelection(), affine, [o.x, o.y], tf.allMasters && !bg, bg));
      } catch (e) {
        onError(e);
      }
    });
  };

  /** The points that move with a selection: the selected ones and the handles of
   *  selected on-curve nodes (they aren't snap targets for themselves). */
  const movingKeys = (base: EditPath[], sel: Set<string>): Set<string> => {
    const out = new Set(sel);
    for (const k of sel) {
      const [pi, ni] = parseKey(k);
      const p = base[pi];
      if (!p || !isOn(p.nodes[ni])) continue;
      const n = p.nodes.length;
      for (const step of [-1, 1]) {
        const j = p.closed ? (ni + step + n) % n : ni + step;
        if (j >= 0 && j < n && !isOn(p.nodes[j])) out.add(nodeKey(pi, j));
      }
    }
    return out;
  };

  /** What a drag can snap to: the other nodes' x and y, the metric lines, the
   *  sidebearings and the middle of the width. */
  const snapTargets = (exclude: Set<string>): { xs: number[]; ys: number[] } => {
    const l = latest.current.layer;
    const xs: number[] = l ? [0, l.width, l.width / 2] : [0];
    const ys: number[] = l ? [0, l.metrics.xHeight, l.metrics.capHeight, l.metrics.ascender, l.metrics.descender] : [0];
    drag.current?.base.forEach((p, pi) => {
      if (p.locked) return;
      p.nodes.forEach((n, ni) => {
        if (exclude.has(nodeKey(pi, ni))) return;
        xs.push(n.x);
        ys.push(n.y);
      });
    });
    return { xs, ys };
  };

  /** Aligns or distributes the selection; `origin` is for Align Selection ("points"). */
  const runAlign = (op: AlignOp) => {
    const l = latest.current.layer;
    if (!l || latest.current.editingBackground) return;
    const horizontal = op === "left" || op === "h-center" || op === "right";
    const vertical = op === "top" || op === "v-center" || op === "bottom";
    let to: number | undefined;
    if (horizontal && alignTo === "width") to = op === "left" ? 0 : op === "right" ? l.width : l.width / 2;
    if (vertical && alignTo !== "selection" && alignTo !== "width") to = alignTo === "baseline" ? 0 : l.metrics[alignTo];
    const sel = currentSelection();
    chain.current = chain.current.then(async () => {
      try {
        accept(await backend.alignSelection(glyph, masterId, sel, op, to, tf.origin));
      } catch (e) {
        onError(e);
      }
    });
  };

  // runCommand is memoized; it reaches the current origin and target through this.
  const runAlignRef = useRef(runAlign);
  runAlignRef.current = runAlign;

  // ---------- Measure ----------

  const runMeasure = (p0: Pt, p1: Pt) => {
    const seq = ++measureSeq.current;
    backend.measure(glyph, masterId, [p0.x, p0.y], [p1.x, p1.y], latest.current.editingBackground)
      .then((m) => { if (seq === measureSeq.current) setMeasurement(m); }, () => {});
  };

  useEffect(() => {
    if (tool !== "measure") { setMeasureLine(null); setMeasurement(null); }
  }, [tool]);

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
  /** Every on-curve node with where it goes: the next master up and down, its curve and the midpoint knob. */
  const hoiKnobs = () => {
    const out: { pi: number; ni: number; p0: [number, number]; p1?: [number, number]; down?: [number, number]; kind: HoiKind | null; mid: [number, number] }[] = [];
    if (!hoi) return out;
    paths.forEach((path, pi) => path.nodes.forEach((n, ni) => {
      if (!isOn(n)) return;
      const p0: [number, number] = [n.x, n.y];
      const p1 = hoi.up?.[pi]?.[ni];
      const down = hoi.down?.[pi]?.[ni];
      let kind: HoiKind | null = hoi.curves.find((c) => c.path === pi && c.node === ni)?.kind ?? null;
      if (hoiDrag && hoiDrag.pi === pi && hoiDrag.ni === ni) kind = { type: "through", x: hoiDrag.x, y: hoiDrag.y };
      const mid = p1 ? hoiAt(kind, p0, p1, 0.5) : p0;
      out.push({ pi, ni, p0, p1, down, kind, mid });
    }));
    return out;
  };
  const gridStep = layer?.grid && layer.grid.spacing > 0 && layer.grid.subdivision > 0 ? layer.grid.spacing / layer.grid.subdivision : 0;
  const snapV = (v: number) => (gridStep ? Math.round(v / gridStep) * gridStep : Math.round(v));
  const round = (p: Pt): Pt => ({ x: snapV(p.x), y: snapV(p.y) });

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
    if (!showNodes) return null;
    let best: string | null = null;
    let bestD = HIT * nodeScale;
    paths.forEach((p, pi) => {
      if (p.locked) return;
      p.nodes.forEach((n, ni) => {
        const q = toScreen(n.x, n.y);
        const d = Math.hypot(q.x - sp.x, q.y - sp.y) - (isOn(n) ? 0.5 : 0);
        if (d <= bestD) { bestD = d; best = nodeKey(pi, ni); }
      });
    });
    return best;
  }

  /** Topmost path whose outline runs under the point (for the path context menu). */
  function hitPath(wp: Pt): number | null {
    const els = svgRef.current?.querySelectorAll<SVGPathElement>("path[data-path]") ?? [];
    for (let i = els.length - 1; i >= 0; i--) {
      if (els[i].isPointInStroke(new DOMPoint(wp.x, wp.y))) return Number(els[i].dataset.path);
    }
    return null;
  }

  /** Topmost component whose filled outline contains the point. */
  /** The annotation under a screen point: near its spot, or on a circle's ring. */
  function hitAnnotation(sp: Pt): number | null {
    const list = layer?.annotations ?? [];
    for (let i = list.length - 1; i >= 0; i--) {
      const a = list[i];
      const p = toScreen(a.x, a.y);
      const d = Math.hypot(p.x - sp.x, p.y - sp.y);
      if (a.kind === "circle" && Math.abs(d - ((a.size || 120) / 2) * view.s) <= HIT + 2) return i;
      if (a.kind === "text") {
        const w = Math.max(60, (a.size || 200) * view.s);
        const lines = (a.text || "x").split("\n").length;
        if (sp.x >= p.x && sp.x <= p.x + w && sp.y >= p.y && sp.y <= p.y + lines * 16 + 12) return i;
      }
      if (d <= HIT + 6) return i;
    }
    return null;
  }

  /** Whether a font-unit point is on the (unlocked, shown) placed image. */
  function hitImage(wp: Pt): boolean {
    const im = layer?.image;
    if (!im || im.locked || !showImage || !imageData?.width) return false;
    // Into the image's own frame: undo the position, the rotation and the scale.
    const r = (-(im.angle ?? 0) * Math.PI) / 180;
    const dx = wp.x - im.x, dy = wp.y - im.y;
    const lx = (dx * Math.cos(r) - dy * Math.sin(r)) / im.scaleX;
    const ly = (dx * Math.sin(r) + dy * Math.cos(r)) / im.scaleY;
    const [cx, cy, cw, ch] = im.crop ?? [0, 0, imageData.width, imageData.height];
    return lx >= cx && lx <= cx + cw && ly >= cy && ly <= cy + ch;
  }

  function hitComponent(wp: Pt): number | null {
    const els = svgRef.current?.querySelectorAll<SVGPathElement>("path[data-comp]") ?? [];
    for (let i = els.length - 1; i >= 0; i--) {
      const idx = Number(els[i].dataset.comp);
      const c = comps[idx];
      if (c && els[i].isPointInFill(new DOMPoint(wp.x - c.dx, wp.y - c.dy))) return idx;
    }
    return null;
  }

  function hitAnchor(sp: Pt): string | null {
    if (!showAnchors) return null;
    let best: string | null = null;
    let bestD = HIT;
    for (const a of anchors) {
      const q = toScreen(a.x, a.y);
      const d = Math.hypot(q.x - sp.x, q.y - sp.y);
      if (d <= bestD) { bestD = d; best = a.name; }
    }
    return best;
  }

  /** Nearest guide line within reach; local guides win over global ones at equal distance. */
  function hitGuide(sp: Pt): GuideRef | null {
    if (!showGuides) return null;
    let best: GuideRef | null = null;
    let bestD = HIT;
    const test = (gs: Guide[], scope: GuideScope) => gs.forEach((g, index) => {
      const p = toScreen(g.x, g.y);
      const a = (g.angle * Math.PI) / 180;
      const dx = Math.cos(a), dy = -Math.sin(a);
      const dist = Math.abs((sp.x - p.x) * -dy + (sp.y - p.y) * dx);
      if (dist <= bestD) { bestD = dist; best = { scope, index }; }
    });
    test(globalGuides, "global");
    test(localGuides, "local");
    return best;
  }

  // ---------- Pointer interaction ----------

  const activeTool: Tool = spaceDown ? "hand" : tool;

  function onPointerDown(e: React.PointerEvent<SVGSVGElement>) {
    if (!layer) return;
    setCtxMenu(null);
    svgRef.current!.setPointerCapture(e.pointerId);
    const sp = local(e);
    const wp = toWorld(sp.x, sp.y);
    const base = clonePaths(paths);
    const baseComps = comps.map((c) => ({ ...c }));
    const baseAnchors = anchors.map((a) => ({ ...a }));
    if (e.button === 1 || activeTool === "hand") {
      drag.current = { mode: "pan", startScreen: sp, startWorld: wp, base, baseComps, baseAnchors, moved: false, startView: view };
      return;
    }
    if (e.button !== 0) return;

    // A locked glyph refuses every edit; looking and measuring stay possible.
    if (layer.glyphLocked && activeTool !== "measure") {
      onWarning?.(t("This glyph is locked. Unlock it to edit."));
      return;
    }

    if (activeTool === "measure") {
      const p = { x: Math.round(wp.x * 10) / 10, y: Math.round(wp.y * 10) / 10 };
      setMeasureLine({ p0: p, p1: p });
      setMeasurement(null);
      drag.current = { mode: "measure", startScreen: sp, startWorld: p, base, baseComps, baseAnchors, moved: false };
      return;
    }

    if (activeTool === "annotate") {
      const list = layer.annotations ?? [];
      if (annSub === "select") {
        const hit = hitAnnotation(sp);
        setAnnSel(hit);
        if (hit !== null) drag.current = { mode: "annot", startScreen: sp, startWorld: wp, base, baseComps, baseAnchors, moved: false };
        return;
      }
      const p = round(wp);
      const fresh: Annotation = { kind: annSub, x: p.x, y: p.y, ...(annSub === "arrow" ? { angle: 225 } : {}), ...(annSub === "circle" ? { size: 120 } : {}), ...(annSub === "text" ? { text: "", size: 200 } : {}) };
      saveAnnotations([...list, fresh]);
      setAnnSel(list.length);
      if (annSub === "text") setAnnSub("select");
      return;
    }

    if (activeTool === "hoi") {
      // A midpoint knob starts a drag; a node chooses which curve the panel edits.
      for (const k of hoiKnobs()) {
        const s = toScreen(k.mid[0], k.mid[1]);
        if (Math.hypot(s.x - sp.x, s.y - sp.y) <= HIT) {
          setHoiSel({ pi: k.pi, ni: k.ni });
          if (hoi?.up) drag.current = { mode: "hoi", startScreen: sp, startWorld: wp, base, baseComps, baseAnchors, moved: false, pi: k.pi, ni: k.ni };
          return;
        }
      }
      const node = hoiKnobs().find((k) => { const s = toScreen(k.p0[0], k.p0[1]); return Math.hypot(s.x - sp.x, s.y - sp.y) <= HIT; });
      setHoiSel(node ? { pi: node.pi, ni: node.ni } : null);
      return;
    }

    if (activeTool === "pixel") {
      const size = layer.grid?.spacing ?? 0;
      if (size < 2) {
        onWarning?.(t("The pixel tool needs a grid spacing of 2 or more (Font Info › Grid)."));
        return;
      }
      if (!glyphNames.has("pixel")) {
        setAskPixelGlyph(true);
        return;
      }
      if (glyph === "pixel") {
        onWarning?.(t("The pixel glyph can't be drawn with itself."));
        return;
      }
      const c: [number, number] = [Math.floor(wp.x / size) * size, Math.floor(wp.y / size) * size];
      const present = comps.some((k) => k.base === "pixel" && k.transform[4] === c[0] && k.transform[5] === c[1] && k.transform[0] === 1 && k.transform[3] === 1);
      setPixelDraft({ on: !present, cells: [c] });
      drag.current = { mode: "pixel", startScreen: sp, startWorld: wp, base, baseComps, baseAnchors, moved: false };
      return;
    }

    if (activeTool === "knife") {
      const p = round(wp);
      setKnifeLine({ p0: p, p1: p });
      drag.current = { mode: "knife", startScreen: sp, startWorld: p, base, baseComps, baseAnchors, moved: false };
      return;
    }

    if (activeTool === "select" && !editingBackground && !layer.glyphLocked) {
      // Smart handles sit on top of everything else.
      for (const [ci, c] of comps.entries()) {
        for (const h of c.handles ?? []) {
          const p = toScreen(h.at[0] + c.dx, h.at[1] + c.dy);
          if (Math.hypot(p.x - sp.x, p.y - sp.y) <= HIT + 2) {
            setSmartDrag({ ci, h, value: h.value });
            drag.current = { mode: "smart", startScreen: sp, startWorld: wp, base, baseComps, baseAnchors, moved: false };
            return;
          }
        }
      }
    }

    if (activeTool === "select") {
      const ah = hitAnchor(sp);
      const hit = ah ? null : hitNode(sp);
      const gh = ah || hit ? null : hitGuide(sp);
      const comp = ah || hit || gh ? null : hitComponent(wp);
      if (ah) {
        let sel = anchorSel;
        if (e.shiftKey) {
          sel = new Set(anchorSel);
          if (sel.has(ah)) sel.delete(ah); else sel.add(ah);
        } else if (!anchorSel.has(ah)) {
          sel = new Set([ah]);
          setSelection(new Set());
          setCompSel(new Set());
        }
        setAnchorSel(sel);
        setGuideSel(null);
        drag.current = { mode: "anchor", startScreen: sp, startWorld: wp, base, baseComps, baseAnchors, moved: false, anchor: ah };
      } else if (hit) {
        let sel = selection;
        if (e.shiftKey) {
          sel = new Set(selection);
          if (sel.has(hit)) sel.delete(hit); else sel.add(hit);
        } else if (!selection.has(hit)) {
          sel = new Set([hit]);
          setCompSel(new Set());
          setAnchorSel(new Set());
        }
        setSelection(sel);
        setGuideSel(null);
        const [hpi, hni] = parseKey(hit);
        drag.current = { mode: "move", startScreen: sp, startWorld: wp, base, baseComps, baseAnchors, moved: false, pi: hpi, ni: hni };
      } else if (gh) {
        setGuideSel(gh);
        setSelection(new Set());
        setCompSel(new Set());
        setAnchorSel(new Set());
        drag.current = { mode: "guide", startScreen: sp, startWorld: wp, base, baseComps, baseAnchors, moved: false, scope: gh.scope, gi: gh.index, baseLocal: localGuides.map((g) => ({ ...g })), baseGlobal: globalGuides.map((g) => ({ ...g })) };
      } else if (comp !== null) {
        let sel = compSel;
        if (e.shiftKey) {
          sel = new Set(compSel);
          if (sel.has(comp)) sel.delete(comp); else sel.add(comp);
        } else if (!compSel.has(comp)) {
          sel = new Set([comp]);
          setSelection(new Set());
          setAnchorSel(new Set());
        }
        setCompSel(sel);
        setGuideSel(null);
        drag.current = { mode: "move", startScreen: sp, startWorld: wp, base, baseComps, baseAnchors, moved: false };
      } else {
        setGuideSel(null);
        // Dragging out of the canvas edge pulls a new guide: vertical from the sides, horizontal from top/bottom.
        const angle = showGuides && (sp.x < EDGE || sp.x > size.w - EDGE) ? 90 : showGuides && (sp.y < EDGE || sp.y > size.h - EDGE) ? 0 : null;
        if (angle !== null) {
          setGuideDraft({ x: wp.x, y: wp.y, angle, locked: false });
          drag.current = { mode: "new-guide", startScreen: sp, startWorld: wp, base, baseComps, baseAnchors, moved: false, angle };
        } else {
          // Shift adds to the selection, Ctrl takes away; Alt uses the other shape
          // (lasso or rectangle) for this drag.
          const selMode = selectModeFor(e);
          if (selMode === "replace") { setSelection(new Set()); setCompSel(new Set()); setAnchorSel(new Set()); }
          if (selMode === "replace" && hitImage(wp)) {
            drag.current = { mode: "image", startScreen: sp, startWorld: wp, base, baseComps, baseAnchors, moved: false };
            return;
          }
          const lasso = (selectShape === "lasso") !== e.altKey;
          drag.current = { mode: "marquee", startScreen: sp, startWorld: wp, base, baseComps, baseAnchors, moved: false, selMode, lasso: lasso ? [sp] : undefined };
        }
      }
      return;
    }

    if (activeTool === "rect" || activeTool === "ellipse") {
      drag.current = { mode: "shape", startScreen: sp, startWorld: round(wp), base, baseComps, baseAnchors, moved: false };
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
        drag.current = { mode: "pen", startScreen: sp, startWorld: p, base: clonePaths(base), baseComps, baseAnchors, moved: false, pi: pn.path!, ni: open.nodes.length - 1 };
      } else {
        base.push({ closed: false, nodes: [{ ...p, kind: "line", smooth: false }] });
        pen.current = { path: base.length - 1, pendingOut: null };
        drag.current = { mode: "pen", startScreen: sp, startWorld: p, base: clonePaths(base), baseComps, baseAnchors, moved: false, pi: base.length - 1, ni: 0 };
      }
      setPaths(base);
      setSelection(new Set());
      setCompSel(new Set());
      setAnchorSel(new Set());
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

    if (d.mode === "image") {
      setImgDrag({ dx: Math.round(wp.x - d.startWorld.x), dy: Math.round(wp.y - d.startWorld.y) });
      return;
    }
    if (d.mode === "hoi") {
      const p = round(wp);
      setHoiDrag({ pi: d.pi!, ni: d.ni!, x: p.x, y: p.y });
      return;
    }
    if (d.mode === "annot") {
      setAnnDrag({ dx: Math.round(wp.x - d.startWorld.x), dy: Math.round(wp.y - d.startWorld.y) });
      return;
    }
    if (d.mode === "smart") {
      setSmartDrag((sd) => {
        if (!sd) return sd;
        const c = comps[sd.ci];
        const { h } = sd;
        const dx = h.atMax[0] - h.atMin[0], dy = h.atMax[1] - h.atMin[1];
        const len2 = dx * dx + dy * dy;
        if (len2 < 1e-9) return sd;
        const tt = Math.min(1, Math.max(0, ((wp.x - (c?.dx ?? 0) - h.atMin[0]) * dx + (wp.y - (c?.dy ?? 0) - h.atMin[1]) * dy) / len2));
        return { ...sd, value: Math.round((h.min + tt * (h.max - h.min)) * 10) / 10 };
      });
      return;
    }

    if (d.mode === "pan" && d.startView) {
      setView({ ...d.startView, ox: d.startView.ox + sp.x - d.startScreen.x, oy: d.startView.oy + sp.y - d.startScreen.y });
    } else if (d.mode === "anchor") {
      let dx = snapV(wp.x - d.startWorld.x), dy = snapV(wp.y - d.startWorld.y);
      if (e.shiftKey) { if (Math.abs(dx) > Math.abs(dy)) dy = 0; else dx = 0; }
      // The grabbed anchor snaps to nodes and metric lines (Ctrl: off).
      const a0 = d.baseAnchors.find((a) => a.name === d.anchor);
      if (a0 && !e.ctrlKey) {
        const t = snapTargets(new Set());
        const r = magneticSnap({ x: a0.x + dx, y: a0.y + dy }, e.shiftKey && dx === 0 ? [] : t.xs, e.shiftKey && dy === 0 ? [] : t.ys, snapDistance / view.s);
        dx = r.x - a0.x;
        dy = r.y - a0.y;
        setSnapLine(r.lineX !== undefined || r.lineY !== undefined ? { x: r.lineX, y: r.lineY } : null);
      } else setSnapLine(null);
      d.lastAnchors = d.baseAnchors.map((a) => (anchorSel.has(a.name) ? { ...a, x: a.x + dx, y: a.y + dy } : a));
      setAnchors(d.lastAnchors);
    } else if (d.mode === "move") {
      let dx = snapV(wp.x - d.startWorld.x), dy = snapV(wp.y - d.startWorld.y);
      if (e.shiftKey) { if (Math.abs(dx) > Math.abs(dy)) dy = 0; else dx = 0; }
      // A dragged node landing near a guide snaps onto it (Ctrl: no snapping).
      let guideSnapped = false;
      if (showGuides && selection.size && !e.ctrlKey && snapDistance > 0) {
        const [pi, ni] = d.pi !== undefined && d.ni !== undefined && selection.has(nodeKey(d.pi, d.ni)) ? [d.pi, d.ni] : parseKey([...selection][0]);
        const n0 = d.base[pi]?.nodes[ni];
        if (n0) {
          const cand = { x: n0.x + dx, y: n0.y + dy };
          const thresh = snapDistance / view.s;
          let best: { ref: GuideRef; x: number; y: number; dist: number } | null = null;
          const test = (gs: Guide[], scope: GuideScope) => gs.forEach((g, index) => {
            const a = (g.angle * Math.PI) / 180;
            const nx = -Math.sin(a), ny = Math.cos(a);
            const signed = (cand.x - g.x) * nx + (cand.y - g.y) * ny;
            if (Math.abs(signed) <= thresh && (!best || Math.abs(signed) < best.dist)) {
              best = { ref: { scope, index }, x: cand.x - signed * nx, y: cand.y - signed * ny, dist: Math.abs(signed) };
            }
          });
          test(globalGuides, "global");
          test(localGuides, "local");
          if (best) {
            const b = best as { ref: GuideRef; x: number; y: number; dist: number };
            dx = b.x - n0.x;
            dy = b.y - n0.y;
            setSnapGuide(b.ref);
            guideSnapped = true;
          } else setSnapGuide(null);
        }
      } else setSnapGuide(null);
      // Otherwise the grabbed node snaps to other nodes' x and y, the metric lines and
      // the sidebearings (magnetic guides).
      setSnapLine(null);
      if (!guideSnapped && selection.size && !e.ctrlKey) {
        const [pi, ni] = d.pi !== undefined && d.ni !== undefined && selection.has(nodeKey(d.pi, d.ni)) ? [d.pi, d.ni] : parseKey([...selection][0]);
        const n0 = d.base[pi]?.nodes[ni];
        if (n0) {
          const t = snapTargets(movingKeys(d.base, selection));
          const r = magneticSnap({ x: n0.x + dx, y: n0.y + dy }, e.shiftKey && dx === 0 ? [] : t.xs, e.shiftKey && dy === 0 ? [] : t.ys, snapDistance / view.s);
          dx = r.x - n0.x;
          dy = r.y - n0.y;
          if (r.lineX !== undefined || r.lineY !== undefined) setSnapLine({ x: r.lineX, y: r.lineY });
        }
      }
      d.lastPaths = moveSelection(d.base, selection, dx, dy);
      // Moving a component by hand takes it off its anchors.
      d.lastComps = d.baseComps.map((c, i) => (compSel.has(i) && (dx !== 0 || dy !== 0) ? { ...c, dx: c.dx + dx, dy: c.dy + dy, align: false } : c));
      setPaths(d.lastPaths);
      setComps(d.lastComps);
    } else if (d.mode === "guide" && d.scope && d.gi !== undefined) {
      const list = d.scope === "local" ? d.baseLocal! : d.baseGlobal!;
      const g = list[d.gi];
      if (g.locked) return;
      // Guides slide along their normal, keeping their angle.
      const a = (g.angle * Math.PI) / 180;
      const nx = -Math.sin(a), ny = Math.cos(a);
      const proj = (wp.x - d.startWorld.x) * nx + (wp.y - d.startWorld.y) * ny;
      const ng = { ...g, x: g.x + proj * nx, y: g.y + proj * ny };
      d.lastGuide = ng;
      if (d.scope === "local") setLocalGuides(list.map((x, i) => (i === d.gi ? ng : x)));
      else setGlobalGuides(list.map((x, i) => (i === d.gi ? ng : x)));
    } else if (d.mode === "new-guide" && d.angle !== undefined) {
      const ng = { x: wp.x, y: wp.y, angle: d.angle, locked: false };
      d.lastGuide = ng;
      setGuideDraft(ng);
    } else if (d.mode === "measure") {
      const p1 = { x: Math.round(wp.x * 10) / 10, y: Math.round(wp.y * 10) / 10 };
      setMeasureLine({ p0: d.startWorld, p1 });
      runMeasure(d.startWorld, p1);
    } else if (d.mode === "pixel") {
      const size = layer?.grid?.spacing ?? 0;
      if (size >= 2) {
        const c: [number, number] = [Math.floor(wp.x / size) * size, Math.floor(wp.y / size) * size];
        setPixelDraft((dr) => (dr && !dr.cells.some(([x, y]) => x === c[0] && y === c[1]) ? { ...dr, cells: [...dr.cells, c] } : dr));
      }
    } else if (d.mode === "knife") {
      setKnifeLine({ p0: d.startWorld, p1: round(wp) });
    } else if (d.mode === "marquee" && d.lasso) {
      d.lasso = extendLasso(d.lasso, sp);
      setLassoPts(d.lasso);
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
    if (d.mode === "image") {
      const off = imgDrag;
      setImgDrag(null);
      if (off && d.moved && layer?.image && (off.dx || off.dy)) {
        const im = layer.image;
        strokeCall(() => backend.setLayerImage(glyph, masterId, { ...im, x: im.x + off.dx, y: im.y + off.dy }));
      }
      return;
    }
    if (d.mode === "hoi") {
      const k = hoiDrag;
      setHoiDrag(null);
      if (k && d.moved && hoi) strokeCall(() => backend.setHoi(glyph, masterId, hoi.axis, k.pi, k.ni, { type: "through", x: k.x, y: k.y }));
      return;
    }
    if (d.mode === "annot") {
      const off = annDrag;
      setAnnDrag(null);
      if (off && d.moved && annSel !== null && (off.dx || off.dy)) {
        const list = layer?.annotations ?? [];
        saveAnnotations(list.map((a, i) => (i === annSel ? { ...a, x: a.x + off.dx, y: a.y + off.dy } : a)));
      }
      return;
    }
    if (d.mode === "smart") {
      const sd = smartDrag;
      setSmartDrag(null);
      if (sd && d.moved && sd.value !== sd.h.value) {
        const c = comps[sd.ci];
        setComponentSmart(sd.ci, { ...(c?.smart ?? {}), [sd.h.axis]: sd.value });
      }
      return;
    }
    const sp = local(e);
    setSnapGuide(null);
    setSnapLine(null);
    // Commit what the last pointer move computed (state may not have re-rendered yet).
    if (d.mode === "guide" && d.moved && d.scope && d.gi !== undefined && d.lastGuide) {
      const g = { ...d.lastGuide, x: Math.round(d.lastGuide.x), y: Math.round(d.lastGuide.y) };
      const list = (d.scope === "local" ? d.baseLocal! : d.baseGlobal!).map((x, i) => (i === d.gi ? g : x));
      saveGuides(d.scope, list);
    } else if (d.mode === "new-guide") {
      // Dropped back on the edge strip: no guide.
      const inside = sp.x > EDGE && sp.x < size.w - EDGE && sp.y > EDGE && sp.y < size.h - EDGE;
      if (d.moved && inside && d.lastGuide) {
        const g = { ...d.lastGuide, x: Math.round(d.lastGuide.x), y: Math.round(d.lastGuide.y) };
        saveGuides("local", [...latest.current.localGuides, g]);
        setGuideSel({ scope: "local", index: latest.current.localGuides.length });
      }
      setGuideDraft(null);
    } else if (d.mode === "move" && d.moved) commit(d.lastPaths ?? latest.current.paths, d.lastComps ?? latest.current.comps);
    else if (d.mode === "anchor" && d.moved) {
      const fin = d.lastAnchors ?? latest.current.anchors;
      const moves = d.baseAnchors
        .filter((a) => anchorSel.has(a.name))
        .map((a) => {
          const b = fin.find((x) => x.name === a.name) ?? a;
          return { name: a.name, dx: b.x - a.x, dy: b.y - a.y };
        })
        .filter((m) => m.dx !== 0 || m.dy !== 0);
      moveAnchorsBy(moves);
    } else if (d.mode === "marquee" && d.moved && !(d.lasso && isLassoClick(d.lasso))) {
      // Use the drag's own points: state may not have re-rendered yet on a fast drag.
      const mode = d.selMode ?? "replace";
      let inside: (q: Pt) => boolean;
      if (d.lasso) {
        const poly = extendLasso(d.lasso, sp);
        inside = (q) => insidePolygon(q, poly);
      } else {
        const [x0, x1] = [Math.min(d.startScreen.x, sp.x), Math.max(d.startScreen.x, sp.x)];
        const [y0, y1] = [Math.min(d.startScreen.y, sp.y), Math.max(d.startScreen.y, sp.y)];
        inside = (q) => q.x >= x0 && q.x <= x1 && q.y >= y0 && q.y <= y1;
      }
      const nodeHits: string[] = [];
      if (showNodes) paths.forEach((p, pi) => {
        if (p.locked) return;
        p.nodes.forEach((n, ni) => {
          if (inside(toScreen(n.x, n.y))) nodeHits.push(nodeKey(pi, ni));
        });
      });
      setSelection(combineSelection(selection, nodeHits, mode));
      // Components are selected when their whole outline box is inside.
      const compHits: number[] = [];
      svgRef.current?.querySelectorAll<SVGPathElement>("path[data-comp]").forEach((el) => {
        const r = el.getBoundingClientRect(), sr = svgRef.current!.getBoundingClientRect();
        const [l, tp, rt, bt] = [r.left - sr.left, r.top - sr.top, r.right - sr.left, r.bottom - sr.top];
        if (r.width && [{ x: l, y: tp }, { x: rt, y: tp }, { x: rt, y: bt }, { x: l, y: bt }].every(inside)) compHits.push(Number(el.dataset.comp));
      });
      setCompSel(combineSelection(compSel, compHits, mode));
      if (showAnchors) {
        setAnchorSel(combineSelection(anchorSel, anchors.filter((an) => inside(toScreen(an.x, an.y))).map((an) => an.name), mode));
      }
      setMarquee(null);
      setLassoPts(null);
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
    } else if (d.mode === "pixel") {
      const draft = pixelDraft;
      setPixelDraft(null);
      if (draft && draft.cells.length) {
        chain.current = chain.current.then(async () => {
          try {
            accept(await backend.setPixels(glyph, masterId, draft.cells, draft.on));
          } catch (err) {
            onError(err);
          }
        });
      }
    } else if (d.mode === "knife") {
      const p1 = round(toWorld(sp.x, sp.y));
      setKnifeLine(null);
      if (d.moved && (p1.x !== d.startWorld.x || p1.y !== d.startWorld.y)) {
        runPathEdit("knife", { points: [[d.startWorld.x, d.startWorld.y], [p1.x, p1.y]] });
      }
    }
    setMarquee(null);
    setLassoPts(null);
  }

  // ---------- Components & clipboard ----------

  const addComponent = useCallback((base: string) => {
    setPicking(false);
    const { paths, comps, layer } = latest.current;
    if (!layer) return;
    const next = [...comps, { base, transform: [1, 0, 0, 1, 0, 0], align: true, svg: "", dx: 0, dy: 0 }];
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
    if (name === "select-all" || name === "invert-selection") {
      const { paths, comps, selection, compSel } = latest.current;
      const all = new Set(paths.flatMap((p, pi) => p.nodes.map((_, ni) => nodeKey(pi, ni))));
      const allComps = new Set(comps.map((_, i) => i));
      if (name === "select-all") {
        setSelection(all);
        setCompSel(allComps);
      } else {
        setSelection(new Set([...all].filter((k) => !selection.has(k))));
        setCompSel(new Set([...allComps].filter((i) => !compSel.has(i))));
      }
      return;
    }
    if (name === "deselect-all") {
      setSelection(new Set());
      setCompSel(new Set());
      setAnchorSel(new Set());
      return;
    }
    if (name === "zoom-in" || name === "zoom-out") {
      const m = measured.current;
      if (m) zoomAt(name === "zoom-in" ? 1.25 : 0.8, m.w / 2, m.h / 2);
      return;
    }
    if (name === "zoom-fit") return fit();
    if (name === "zoom-actual") {
      const l = latest.current.layer;
      const m = measured.current;
      if (l && m) setView({ s: 1, ox: m.w / 2 - l.width / 2, oy: m.h / 2 + (l.metrics.ascender + l.metrics.descender) / 2 });
      return;
    }
    if (name === "shape-order") return setShapesOpen(true);
    if (name === "focus-path") {
      const sel = [...latest.current.selection].map(parseKey);
      if (sel.length) setFocusPath(sel[0][0]);
      return;
    }
    if (name === "clear-focus") return setFocusPath(null);
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
    if (name === "edit-background") return toggleBackground();
    if (name === "union" || name === "subtract" || name === "intersect") {
      // The paths with a selected node; none selected means all (Union) or the
      // frontmost path against the rest (Subtract, Intersect).
      const pathIdx = [...new Set([...latest.current.selection].map((k) => parseKey(k)[0]))];
      runPathEdit(name, { paths: pathIdx.length ? pathIdx : undefined });
      return;
    }
    if (name === "align-selection") {
      runAlignRef.current("points");
      return;
    }
    if (name === "harmonize" || name === "super-smooth") {
      // The selected on-curve nodes; Harmonize with none selected takes every smooth node.
      const cur = latest.current.paths;
      const sel = [...latest.current.selection].map(parseKey).filter(([pi, ni]) => { const n = cur[pi]?.nodes[ni]; return !!n && isOn(n); });
      if (name === "super-smooth" && !sel.length) return;
      runPathEdit(name, { nodes: sel });
      return;
    }
    if (name === "open-corner" || name === "make-node-first" || name === "connect-nodes" || name === "reconnect-nodes"
      || name === "close-open-paths" || name === "reverse-contours" || name === "tidy-up" || name === "round-coordinates") {
      const sel = [...latest.current.selection].map(parseKey);
      const pathIdx = [...new Set(sel.map(([pi]) => pi))];
      const opts = name === "open-corner" || name === "make-node-first" ? { nodes: sel.slice(0, 1) }
        : name === "connect-nodes" ? { nodes: sel.slice(0, 2) }
        : { paths: pathIdx.length ? pathIdx : undefined };
      runPathEdit(name, opts);
      return;
    }
    if (name === "to-background" || name === "add-to-background" || name === "swap-background" || name === "clear-background") {
      // While drawing on the background, moving its own content aside makes no sense.
      if (latest.current.editingBackground && (name === "to-background" || name === "add-to-background")) return;
      const edit: BackgroundEdit = name === "to-background" ? "toBackground" : name === "add-to-background" ? "addToBackground" : name === "swap-background" ? "swap" : "clear";
      const sel = currentSelection();
      const exit = edit === "swap" || edit === "clear";
      chain.current = chain.current.then(async () => {
        try {
          const l = await backend.backgroundEdit(glyph, masterId, edit, sel);
          if (exit) setEditingBackground(false);
          accept(l, exit ? false : undefined);
          if (exit) { setSelection(new Set()); setCompSel(new Set()); setAnchorSel(new Set()); }
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
      const nextComps = [...comps, ...clip.components.map((c) => ({ ...c, align: c.align ?? true, transform: [...c.transform], svg: "", dx: 0, dy: 0 }))];
      setPaths(nextPaths);
      setSelection(new Set(clip.paths.flatMap((p, i) => p.nodes.map((_, ni) => nodeKey(paths.length + i, ni)))));
      setCompSel(new Set(clip.components.map((_, i) => comps.length + i)));
      commit(nextPaths, nextComps);
    }
  }, [commit, selectedContent, accept, glyph, masterId, onError, toggleBackground, currentSelection, runPathEdit, zoomAt, fit]);

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
      const { paths, comps, anchors, selection, compSel, anchorSel } = latest.current;
      if (e.key === " ") { setSpaceDown(true); e.preventDefault(); return; }
      if (e.ctrlKey && !e.altKey) {
        const k = e.key.toLowerCase();
        if (k === "a") {
          setSelection(new Set(paths.flatMap((p, pi) => (p.locked ? [] : p.nodes.map((_, ni) => nodeKey(pi, ni))))));
          setCompSel(new Set(comps.map((_, i) => i)));
          if (showAnchors && !latest.current.editingBackground) setAnchorSel(new Set(anchors.map((a) => a.name)));
        } else if (k === "0") fit();
        else if (k === "=" || k === "+") zoomAt(1.25, size.w / 2, size.h / 2);
        else if (k === "-") zoomAt(0.8, size.w / 2, size.h / 2);
        else if (k === "c" && !e.shiftKey) runCommand("copy");
        else if (k === "x" && !e.shiftKey) runCommand("cut");
        else if (k === "v" && !e.shiftKey) runCommand("paste");
        else if (k === "c" && e.shiftKey) runCommand("add-component");
        else if (k === "b" && !e.shiftKey) toggleBackground();
        else if (k === "j" && !e.shiftKey) runCommand("to-background");
        else if (k === "j" && e.shiftKey) runCommand("add-to-background");
        else return;
        e.preventDefault();
        return;
      }
      if (e.key === "Enter" && e.shiftKey && e.altKey && selection.size) {
        // Super-smooth (G2) on or off for the selected nodes.
        runCommand("super-smooth");
        e.preventDefault();
        return;
      }
      if (e.altKey) return;
      const nudge: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, 1], ArrowDown: [0, -1] };
      if (nudge[e.key] && showAnchors && anchorSel.size) {
        const m = e.shiftKey ? 10 : 1;
        const [dx, dy] = [nudge[e.key][0] * m, nudge[e.key][1] * m];
        moveAnchorsBy([...anchorSel].map((name) => ({ name, dx, dy })));
        e.preventDefault();
        return;
      }
      if (nudge[e.key] && (selection.size || compSel.size)) {
        const m = e.shiftKey ? 10 : 1;
        const [dx, dy] = [nudge[e.key][0] * m, nudge[e.key][1] * m];
        const next = moveSelection(paths, selection, dx, dy);
        const nextComps = comps.map((c, i) => (compSel.has(i) ? { ...c, dx: c.dx + dx, dy: c.dy + dy, align: false } : c));
        setPaths(next);
        setComps(nextComps);
        commit(next, nextComps);
        e.preventDefault();
        return;
      }
      if (!e.ctrlKey && (e.key === "Delete" || e.key === "Backspace") && latest.current.guideSel) {
        deleteGuide(latest.current.guideSel);
        e.preventDefault();
        return;
      }
      if (!e.ctrlKey && (e.key === "Delete" || e.key === "Backspace") && showAnchors && anchorSel.size) {
        removeAnchorsNamed([...anchorSel]);
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
        // Toggle smooth / corner on selected curve points (a corner is never super-smooth).
        const next = clonePaths(paths);
        let changed = false;
        for (const k of selection) {
          const [pi, ni] = parseKey(k);
          const n = next[pi]?.nodes[ni];
          if (n && isOn(n)) { n.smooth = !n.smooth; if (!n.smooth) n.g2 = false; changed = true; }
        }
        if (changed) { setPaths(next); commit(next); }
        e.preventDefault();
        return;
      }
      if (e.key === "Escape") {
        if (ctxMenu) setCtxMenu(null);
        else if (latest.current.guideSel) setGuideSel(null);
        else if (pen.current.path !== null) pen.current = { path: null, pendingOut: null };
        else if (focusPath !== null) setFocusPath(null);
        else { setSelection(new Set()); setCompSel(new Set()); setAnchorSel(new Set()); }
        setMeasureLine(null);
        setMeasurement(null);
        setKnifeLine(null);
        setGuideDraft(null);
        rerender((n) => n + 1);
        return;
      }
      if (e.key.toLowerCase() === "v" && e.shiftKey) {
        // Shift+V: the Select tool, switching between rectangle and lasso selection.
        setTool("select");
        setSelectShape(selectShape === "lasso" ? "rect" : "lasso");
        pen.current = { path: null, pendingOut: null };
        return;
      }
      const t = TOOLS.find((x) => x.key === e.key.toLowerCase() && !!x.shift === e.shiftKey);
      if (t) {
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
  }, [active, picking, commit, fit, zoomAt, size, runCommand, showAnchors, ctxMenu, focusPath, moveAnchorsBy, removeAnchorsNamed, toggleBackground, deleteGuide, selectShape]);

  const glyphNames = useMemo(() => new Set(glyphs.map((g) => g.name)), [glyphs]);

  // ---------- Rendering ----------

  if (!layer) return <div className="editor"><div className="editor-loading k-caption muted">Loading {glyph}…</div></div>;

  const { ascender, capHeight, xHeight, descender } = layer.metrics;
  const zoneLines = [0, xHeight, capHeight, ascender, descender];
  const lines: [string, number][] = [[t("Ascender"), ascender], [t("Cap height"), capHeight], [t("x-height"), xHeight], [t("Baseline"), 0], [t("Descender"), descender]];
  const x0 = toScreen(0, 0).x, x1 = toScreen(layer.width, 0).x;
  // Stroked paths are drawn as their stroke outlines (from the engine) plus a thin
  // centre line; every other path is filled as usual.
  const enginePaths = new Set(layer.enginePaths ?? []);
  const outline = pathsToSvg(paths.filter((p, i) => !p.stroke && !enginePaths.has(i)));
  const strokedCentres = pathsToSvg(paths.filter((p) => p.stroke));
  // The one path the selection touches, for the Stroke panel.
  const selectedPathSet = new Set([...selection].map((k) => parseKey(k)[0]));
  const strokePath = selectedPathSet.size === 1 ? [...selectedPathSet][0] : null;
  const strokeTarget = strokePath !== null ? paths[strokePath] : undefined;
  // A single selected on-curve node of a stroked path: its pen point number.
  const penNode = (() => {
    if (!strokeTarget?.stroke || selection.size !== 1) return null;
    const [, ni] = parseKey([...selection][0]);
    if (!strokeTarget.nodes[ni] || !isOn(strokeTarget.nodes[ni])) return null;
    return strokeTarget.nodes.slice(0, ni).filter(isOn).length;
  })();
  // Nodes sharing an exact position with another node of the same path: flagged red.
  const dupNodes = new Set<string>();
  paths.forEach((p, pi) => {
    const seen = new Map<string, number>();
    p.nodes.forEach((n, ni) => {
      const k = `${n.x},${n.y}`;
      const first = seen.get(k);
      if (first !== undefined) { dupNodes.add(nodeKey(pi, first)); dupNodes.add(nodeKey(pi, ni)); }
      else seen.set(k, ni);
    });
  });
  const world = `translate(${view.ox} ${view.oy}) scale(${view.s} ${-view.s})`;
  const selGuide = guideSel ? (guideSel.scope === "local" ? localGuides : globalGuides)[guideSel.index] : undefined;
  const one = selection.size === 1 && compSel.size === 0 && anchorSel.size === 0 && !selGuide ? parseKey([...selection][0]) : null;
  const oneNode = one ? paths[one[0]]?.nodes[one[1]] : undefined;
  const oneComp = compSel.size === 1 && selection.size === 0 && anchorSel.size === 0 && !selGuide ? comps[[...compSel][0]] : undefined;
  const oneAnchor = anchorSel.size === 1 && selection.size === 0 && compSel.size === 0 && !selGuide ? anchors.find((a) => a.name === [...anchorSel][0]) : undefined;
  const pendingOut = pen.current.path !== null ? pen.current.pendingOut : null;
  const penPath = pen.current.path !== null ? paths[pen.current.path] : undefined;
  const lastPen = penPath && !penPath.closed ? penPath.nodes[penPath.nodes.length - 1] : undefined;
  const cursorStyle = activeTool === "hand" ? (drag.current?.mode === "pan" ? "grabbing" : "grab") : activeTool === "select" ? "default" : "crosshair";
  const moveComp = (i: number, dx: number, dy: number) => {
    const next = comps.map((c, j) => (j === i ? { ...c, dx: c.dx + dx, dy: c.dy + dy, align: false } : c));
    setComps(next);
    commit(paths, next);
  };

  return (
    <div className="editor">
      <div className="toolbar" role="toolbar" aria-label={t("Tools")}>
        {TOOLS.map((tl) => {
          if (tl.id !== "select") return (
            <button key={tl.id} className={`tool${tool === tl.id ? " is-selected" : ""}`} title={t(tl.label)} aria-label={t(tl.label)} aria-pressed={tool === tl.id}
              onClick={() => { setTool(tl.id); pen.current = { path: null, pendingOut: null }; }}>
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinejoin="round">{tl.icon}</svg>
            </button>
          );
          // Select: hold it or right-click it to choose rectangle or lasso selection.
          const lasso = selectShape === "lasso";
          const label = lasso ? t("Lasso Select (Shift+V)") : t(tl.label);
          const stopHold = () => { if (selectHold.current) { window.clearTimeout(selectHold.current); selectHold.current = null; } };
          const choose = (shape: "rect" | "lasso") => { setSelectShape(shape); setTool("select"); setSelectMenu(false); pen.current = { path: null, pendingOut: null }; };
          return (
            <span key={tl.id} className="tool-wrap">
              <button className={`tool${tool === tl.id ? " is-selected" : ""}`} title={t("{tool} · hold or right-click to choose rectangle or lasso", { tool: label })} aria-label={label} aria-pressed={tool === tl.id} aria-haspopup="menu" aria-expanded={selectMenu}
                onClick={() => { if (selectMenu) return; setTool(tl.id); pen.current = { path: null, pendingOut: null }; }}
                onPointerDown={() => { stopHold(); selectHold.current = window.setTimeout(() => { selectHold.current = null; setSelectMenu(true); }, 450); }}
                onPointerUp={stopHold} onPointerLeave={stopHold}
                onContextMenu={(e) => { e.preventDefault(); stopHold(); setSelectMenu(true); }}>
                <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinejoin="round">{lasso ? LASSO_ICON : tl.icon}</svg>
              </button>
              {selectMenu && <>
                <span className="tool-menu-scrim" onPointerDown={() => setSelectMenu(false)} />
                <div className="ctx-menu tool-menu" role="menu" aria-label={t("Selection shape")}
                  onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); setSelectMenu(false); } }}>
                  <button type="button" role="menuitemradio" aria-checked={!lasso} autoFocus={!lasso} onClick={() => choose("rect")}>
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinejoin="round">{tl.icon}</svg>{t("Select (V)")}
                  </button>
                  <button type="button" role="menuitemradio" aria-checked={lasso} autoFocus={lasso} onClick={() => choose("lasso")}>
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinejoin="round">{LASSO_ICON}</svg>{t("Lasso Select (Shift+V)")}
                  </button>
                </div>
              </>}
            </span>
          );
        })}
        <span className="toolbar-sep" />
        <button className="btn btn-secondary btn-sm" onClick={() => setPicking(true)} title={t("Add Component (Ctrl+Shift+C)")}>{t("Add Component")}</button>
        <span className="toolbar-sep" />
        {layer.glyphLocked && (
          <span className="locked-badge k-caption" role="status" title={t("This glyph is locked. Unlock it to edit.")}>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
              <rect x="5" y="11" width="14" height="9" rx="2" /><path d="M8 11V7a4 4 0 0 1 8 0v4" />
            </svg>
            {t("Locked")}
          </span>
        )}
        <span className="toolbar-hint k-caption muted" title={hint(activeTool, selectShape === "lasso")}>{toolName(activeTool, selectShape === "lasso")}</span>
        <span className="spacer" />
        {onToggleView && (
          <div className="segmented view-toggles" role="group" aria-label={t("Show")}>
            {([["nodes", "Nodes", showNodes], ["metrics", "Metrics", showMetrics], ["anchors", "Anchors", showAnchors], ["guides", "Guides", showGuides], ["zones", "Zones", showZones], ["background", "Background", showBackground]] as const).map(([key, label, on]) => (
              <button key={key} type="button" className={`segment${on ? " is-selected" : ""}`} aria-pressed={on} onClick={() => onToggleView(key)}
                title={t("Show or hide {what}", { what: t(label) })}>{t(label)}</button>
            ))}
          </div>
        )}
        <button type="button" className="icon-btn sidebar-toggle" aria-pressed={showSidebar} onClick={onToggleSidebar}
          title={t("Show or hide the sidebar")} aria-label={t("Sidebar")}>
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M15 4v16" /></svg>
        </button>
      </div>
      <div className="editor-columns">
      <div className="canvas-wrap" ref={wrapRef}>
        <svg ref={svgRef} className="canvas" width={size.w} height={size.h} style={{ cursor: cursorStyle }}
          onPointerDown={onPointerDown} onPointerMove={onPointerMove} onPointerUp={onPointerUp} onPointerLeave={() => setCursor(null)}
          onContextMenu={(e) => {
            if (activeTool !== "select") return;
            e.preventDefault();
            const sp = local(e);
            const w = toWorld(sp.x, sp.y);
            const wp = round(w);
            const hit = layer.glyphLocked ? null : hitNode(sp);
            if (hit && !selection.has(hit)) {
              setSelection(new Set([hit]));
              setCompSel(new Set());
              setAnchorSel(new Set());
            }
            const path = layer.glyphLocked || editingBackground ? null : hit ? parseKey(hit)[0] : hitPath(w);
            setCtxMenu({ x: sp.x, y: sp.y, wx: wp.x, wy: wp.y, node: hit ? parseKey(hit) : null, path });
          }}
          onDoubleClick={(e) => {
            if (tool !== "select") return;
            const sp = local(e);
            const gh = hitGuide(sp);
            if (gh) {
              setGuideSel(gh);
              setSelection(new Set());
              setCompSel(new Set());
              setAnchorSel(new Set());
              return;
            }
            const hit = hitNode(sp);
            if (!hit) return;
            const [pi, ni] = parseKey(hit);
            if (e.shiftKey && isOn(paths[pi].nodes[ni]) && !layer.glyphLocked) {
              // Shift+double-click: the node becomes super-smooth, or back to smooth.
              setSelection(new Set([hit]));
              runPathEdit("super-smooth", { nodes: [[pi, ni]] });
              return;
            }
            setSelection(new Set(paths[pi].nodes.map((_, k) => nodeKey(pi, k))));
          }}>
          {/* Metric lines */}
          {showMetrics && lines.map(([label, y]) => {
            const sy = toScreen(0, y).y;
            return (
              <g key={label}>
                <line x1={0} x2={size.w} y1={sy} y2={sy} className={y === 0 ? "metric-line baseline" : "metric-line"} />
                <text x={16} y={sy - 4} className="metric-label">{label} {Math.round(y)}</text>
              </g>
            );
          })}
          {showMetrics && <rect x={x0} y={toScreen(0, ascender).y} width={Math.max(0, x1 - x0)} height={(ascender - descender) * view.s} className="advance-box" />}
          {/* Alignment zones: faint bands across the canvas. */}
          {showZones && !editingBackground && (layer.zones ?? []).filter((z) => z.size !== 0).map((z, i) => {
            const [lo, hi] = zoneRange(z);
            const top = toScreen(0, hi).y, bottom = toScreen(0, lo).y;
            return <rect key={`z${i}`} x={0} y={top} width={size.w} height={Math.max(1, bottom - top)} className="zone-band" pointerEvents="none" />;
          })}
          {/* Hint bands: manual hints, or the automatic ones from the master's stems. */}
          {!editingBackground && (layer.hints.length ? layer.hints : layer.autoHints).map((h, i) => {
            const auto = layer.hints.length === 0;
            const top = toScreen(0, ascender).y, bot = toScreen(0, descender).y;
            if (h.horizontal) {
              const y = toScreen(0, h.position).y;
              return h.width === 0
                ? <line key={`hint${i}`} x1={x0} x2={x1} y1={y} y2={y} className={`hint-ghost${auto ? " is-auto" : ""}`} pointerEvents="none" />
                : <rect key={`hint${i}`} x={x0} y={y - h.width * view.s} width={Math.max(0, x1 - x0)} height={h.width * view.s} className={`hint-band${auto ? " is-auto" : ""}`} pointerEvents="none" />;
            }
            const x = toScreen(h.position, 0).x;
            return h.width === 0
              ? <line key={`hint${i}`} x1={x} x2={x} y1={top} y2={bot} className={`hint-ghost${auto ? " is-auto" : ""}`} pointerEvents="none" />
              : <rect key={`hint${i}`} x={x} y={top} width={h.width * view.s} height={bot - top} className={`hint-band${auto ? " is-auto" : ""}`} pointerEvents="none" />;
          })}
          {/* Placed image (a scan to trace): never exports; shown for empty glyphs too. */}
          {layer.image && (showImage || (paths.length === 0 && comps.length === 0)) && (() => {
            const im = layer.image;
            const off = imgDrag ?? { dx: 0, dy: 0 };
            const ok = imageData?.dataUrl && imageData.width > 0;
            if (!ok) {
              const a = toScreen(im.x + off.dx, im.y + off.dy + 400), b = toScreen(im.x + off.dx + 400, im.y + off.dy);
              return (
                <g pointerEvents="none">
                  <rect x={a.x} y={a.y} width={b.x - a.x} height={b.y - a.y} className="image-missing" />
                  <text x={a.x + 12} y={a.y + 24} className="image-missing-label">{t("Image not found")}</text>
                  <text x={a.x + 12} y={a.y + 42} className="image-missing-file">{im.path}</text>
                </g>
              );
            }
            const w = imageData!.width, h = imageData!.height;
            const crop = im.crop ?? [0, 0, w, h];
            const clipId = `imgclip-${glyph.replace(/[^A-Za-z0-9_-]/g, "_")}`;
            return (
              <g transform={world} pointerEvents="none">
                <g transform={`translate(${im.x + off.dx} ${im.y + off.dy}) rotate(${im.angle ?? 0}) scale(${im.scaleX} ${-im.scaleY})`}>
                  <clipPath id={clipId}><rect x={crop[0]} y={-(crop[1] + crop[3])} width={crop[2]} height={crop[3]} /></clipPath>
                  <image href={imageData!.dataUrl!} x={0} y={-h} width={w} height={h} clipPath={`url(#${clipId})`} preserveAspectRatio="none" className={`layer-image${im.locked ? " is-locked" : ""}`} />
                  <rect x={crop[0]} y={-(crop[1] + crop[3])} width={crop[2]} height={crop[3]} className="layer-image-frame" vectorEffect="non-scaling-stroke" />
                </g>
              </g>
            );
          })()}
          {/* sbix bitmap or SVG drawing (S63), when one is chosen in its panel. */}
          {shownPicture !== null && (() => {
            const p = pictures.find((q) => (shownPicture === 0 ? q.kind === "svg" : q.ppem === shownPicture));
            if (!p?.dataUrl || p.width <= 0) return null;
            return (
              <g transform={world} pointerEvents="none">
                <g transform={`translate(${p.x} ${p.y}) scale(1 -1)`}>
                  <image href={p.dataUrl} x={0} y={-p.height} width={p.width} height={p.height} preserveAspectRatio="none" className={`glyph-picture${p.kind === "bitmap" ? " is-bitmap" : ""}`} />
                  <rect x={0} y={-p.height} width={p.width} height={p.height} className="glyph-picture-frame" vectorEffect="non-scaling-stroke" />
                </g>
              </g>
            );
          })()}
          {/* Background: a reference drawing behind the outline; never exports. */}
          {showBackground && !editingBackground && layer.background && (
            <g transform={world} pointerEvents="none">
              <path d={layer.background.componentSvg} className="background-fill" />
              <path d={pathsToSvg(layer.background.paths)} className="background-fill" fillRule="nonzero" />
            </g>
          )}
          {/* Glyph */}
          <g transform={world}>
            {/* While editing the background, the outline is the tinted reference. */}
            {editingBackground && <path d={pathsToSvg(layer.paths) + layer.componentSvg} className="background-fill" fillRule="nonzero" pointerEvents="none" />}
            {comps.map((c, i) => (
              <g key={`c${i}`} transform={`translate(${c.dx} ${c.dy})`}>
                <path d={c.svg} data-comp={i} className={`component-fill${compSel.has(i) ? " is-selected" : ""}${layer.engineAll ? " is-masked-away" : ""}`} />
              </g>
            ))}
            {/* Smart handles: drag along the axis' line to set its value. */}
            {comps.map((c, ci) => (c.handles ?? []).map((h) => {
              const dragging = smartDrag && smartDrag.ci === ci && smartDrag.h.axis === h.axis ? smartDrag : null;
              const tt = dragging && h.max !== h.min ? (dragging.value - h.min) / (h.max - h.min) : null;
              const x = (tt === null ? h.at[0] : h.atMin[0] + tt * (h.atMax[0] - h.atMin[0])) + c.dx;
              const y = (tt === null ? h.at[1] : h.atMin[1] + tt * (h.atMax[1] - h.atMin[1])) + c.dy;
              const r = 6 / view.s;
              return (
                <g key={`sh${ci}:${h.axis}`} className="smart-handle">
                  {dragging && <line x1={h.atMin[0] + c.dx} y1={h.atMin[1] + c.dy} x2={h.atMax[0] + c.dx} y2={h.atMax[1] + c.dy} className="smart-track" strokeWidth={1 / view.s} />}
                  <path d={`M${x} ${y - r}L${x + r} ${y}L${x} ${y + r}L${x - r} ${y}Z`} className="smart-diamond">
                    <title>{t("{axis}: {value} (drag to change)", { axis: h.axis, value: Math.round((dragging?.value ?? h.value) * 10) / 10 })}</title>
                  </path>
                </g>
              );
            }))}
            {/* A component whose base glyph is missing: a dashed box where it would sit. */}
            {comps.map((c, i) => glyphNames.has(c.base) ? null : (
              <rect key={`cm${i}`} x={c.transform[4] + c.dx} y={c.transform[5] + c.dy} width={500} height={layer.metrics.capHeight} className="component-missing">
                <title>{t("Missing glyph: {name}", { name: c.base })}</title>
              </rect>
            ))}
            {focusPath !== null && paths[focusPath] ? <>
              <path d={pathsToSvg(paths.filter((p, i) => i !== focusPath && !p.stroke && !enginePaths.has(i)))} className="glyph-fill is-dimmed" fillRule="nonzero" pointerEvents="none" />
              {!paths[focusPath].stroke && !enginePaths.has(focusPath) && <path d={pathsToSvg([paths[focusPath]])} className="glyph-fill" fillRule="nonzero" />}
            </> : <path d={outline} className="glyph-fill" fillRule="nonzero" />}
            {layer.engineAll && <path d={pathsToSvg(paths.filter((p) => !p.mask && !p.stroke))} className="glyph-fill is-masked-away" fillRule="nonzero" pointerEvents="none" />}
            {layer.strokeSvg && <path d={layer.strokeSvg} className="glyph-fill" fillRule="nonzero" pointerEvents="none" />}
            {paths.some((p) => p.mask) && <path d={pathsToSvg(paths.filter((p) => p.mask))} className="mask-outline" fill="none" strokeWidth={1.5 / view.s} pointerEvents="none" />}
            {(layer.parts ?? []).map((pt, i) => {
              const nd = paths[pt.path]?.nodes[pt.node];
              if (!nd) return null;
              const r = 7 / view.s;
              return <rect key={`part${i}`} x={nd.x - r} y={nd.y - r} width={2 * r} height={2 * r} className={`part-marker is-${pt.kind}`} strokeWidth={1.5 / view.s} pointerEvents="none"><title>{pt.glyph}</title></rect>;
            })}
            {strokedCentres && <path d={strokedCentres} className="stroke-centre" fill="none" strokeWidth={1.5 / view.s} pointerEvents="none" />}
            {/* Invisible strokes: the right-click target for lock and focus; locked paths turn red on hover. */}
            {!editingBackground && paths.map((p, pi) => (
              <path key={`ph${pi}`} d={pathsToSvg([p])} data-path={pi} className={`path-hit${p.locked ? " is-locked" : ""}`} fill="none" strokeWidth={(HIT + 3) / view.s} pointerEvents="stroke" />
            ))}
            {preview && <path d={pathsToSvg([preview])} className="preview-outline" />}
          </g>
          {/* Colour layers, over the glyph's fill, under its points */}
          {(layer?.colorLayers?.length ?? 0) > 0 && (
            <g transform={`translate(${view.ox} ${view.oy}) scale(${view.s} ${view.s})`} pointerEvents="none">
              <ColorGlyph layers={layer?.colorLayers ?? []} palette={layer?.palette ?? []}
                opacity={currentLayer?.kind === "color" ? 0.55 : 0.35} highlight={currentLayer?.kind === "color" ? masterId : undefined} />
            </g>
          )}
          {/* Component labels */}
          {comps.map((c, i) => {
            const at = toScreen(c.transform[4] + c.dx, c.transform[5] + c.dy);
            const missing = !glyphNames.has(c.base);
            return compSel.has(i) || missing ? <text key={`cl${i}`} x={at.x + 4} y={at.y + 14} className={`component-label${missing ? " is-missing" : ""}`}>{c.base}</text> : null;
          })}
          {/* Handles and nodes */}
          {showNodes && <>
            {paths.map((p, pi) => p.nodes.map((n, ni) => {
              if (isOn(n)) return null;
              const owner = handleOwner(p, ni);
              if (owner < 0) return null;
              const a = toScreen(n.x, n.y), b = toScreen(p.nodes[owner].x, p.nodes[owner].y);
              return <line key={`h${pi}:${ni}`} x1={a.x} y1={a.y} x2={b.x} y2={b.y} className="handle-line" opacity={focusPath !== null && pi !== focusPath ? 0.3 : undefined} />;
            }))}
            {pendingOut && lastPen && (() => {
              const a = toScreen(lastPen.x, lastPen.y), b = toScreen(pendingOut.x, pendingOut.y);
              return <g><line x1={a.x} y1={a.y} x2={b.x} y2={b.y} className="handle-line" /><circle cx={b.x} cy={b.y} r={3 * nodeScale} className="node-off" /></g>;
            })()}
            {showZones && !editingBackground && paths.map((p, pi) => p.nodes.map((n, ni) => {
              if (!isOn(n)) return null;
              const mark = classifyHeight(n.y, zoneLines, layer.zones ?? []);
              return mark ? <ZoneHighlight key={`zh${pi}:${ni}`} mark={mark} at={toScreen(n.x, n.y)} scale={nodeScale} /> : null;
            }))}
            {paths.map((p, pi) => p.nodes.map((n, ni) => {
              const at = toScreen(n.x, n.y);
              // The start node gets an arrow toward the next node: the path's direction.
              let dir: number | undefined;
              if (ni === 0 && p.nodes.length > 1) {
                const b = toScreen(p.nodes[1].x, p.nodes[1].y);
                if (b.x !== at.x || b.y !== at.y) dir = Math.atan2(b.y - at.y, b.x - at.x);
              }
              return <NodeMark key={`n${pi}:${ni}`} n={n} at={at} scale={nodeScale} selected={selection.has(nodeKey(pi, ni))} first={ni === 0} dir={dir} duplicate={dupNodes.has(nodeKey(pi, ni))} dim={focusPath !== null && pi !== focusPath} />;
            }))}
          </>}
          {/* Anchors: diamonds with their names, over the points. */}
          {showAnchors && anchors.map((a) => {
            const p = toScreen(a.x, a.y);
            const sel = anchorSel.has(a.name);
            return (
              <g key={`a:${a.name}`} pointerEvents="none">
                <rect x={-4.5} y={-4.5} width={9} height={9} transform={`translate(${p.x} ${p.y}) rotate(45)`} className={`anchor-mark${sel ? " is-selected" : ""}`} />
                <text x={p.x + 8} y={p.y - 7} className="anchor-label">{a.name}</text>
              </g>
            );
          })}
          {/* Background anchors: shown while editing the background, but not editable there. */}
          {editingBackground && (layer.background?.anchors ?? []).map((a) => {
            const p = toScreen(a.x, a.y);
            return (
              <g key={`ba:${a.name}`} pointerEvents="none">
                <rect x={-4.5} y={-4.5} width={9} height={9} transform={`translate(${p.x} ${p.y}) rotate(45)`} className="background-anchor" />
                <text x={p.x + 8} y={p.y - 7} className="anchor-label">{a.name}</text>
              </g>
            );
          })}
          {/* Guides: global (master-wide) in red, local in purple; the draft follows the cursor. */}
          {showGuides && [
            ...globalGuides.map((g, index) => ({ g, scope: "global" as const, index })),
            ...localGuides.map((g, index) => ({ g, scope: "local" as const, index })),
            ...(guideDraft ? [{ g: guideDraft, scope: "local" as const, index: -1 }] : []),
          ].map(({ g, scope, index }) => {
            const p = toScreen(g.x, g.y);
            const a = (g.angle * Math.PI) / 180;
            const dx = Math.cos(a), dy = -Math.sin(a);
            const L = size.w + size.h;
            const sel = guideSel?.scope === scope && guideSel.index === index;
            const snapped = snapGuide?.scope === scope && snapGuide.index === index;
            const cls = `guide${scope === "global" ? " is-global" : ""}${sel ? " is-selected" : ""}${snapped ? " is-snapped" : ""}${g.locked ? " is-locked" : ""}${index === -1 ? " guide-draft" : ""}`;
            return (
              <g key={`${scope}:${index}`} className={cls} pointerEvents="none">
                <line x1={p.x - dx * L} y1={p.y - dy * L} x2={p.x + dx * L} y2={p.y + dy * L} className="guide-line" />
                <circle cx={p.x} cy={p.y} r={4} className="guide-handle" />
                {(sel || g.name || g.locked) && (
                  <text x={p.x + 8} y={p.y - 8} className="guide-label">
                    {g.name || `${Math.round(g.angle * 10) / 10}°`}{scope === "global" ? ` · ${t("global")}` : ""}
                  </text>
                )}
                {g.locked && (
                  <g className="guide-lock" transform={`translate(${p.x - 24} ${p.y - 18}) scale(0.55)`}>
                    <rect x="5" y="11" width="14" height="9" rx="2" />
                    <path d="M8 11V7a4 4 0 0 1 8 0v4" />
                  </g>
                )}
              </g>
            );
          })}
          {/* Bounding box of the drawing (control points included). */}
          {showBoundingBox && (() => {
            let b: Bounds | null = svgBounds(outline);
            for (const c of comps) {
              const cb = svgBounds(c.svg);
              if (!cb) continue;
              const nb = { minX: cb.minX + c.dx, minY: cb.minY + c.dy, maxX: cb.maxX + c.dx, maxY: cb.maxY + c.dy };
              b = b === null ? nb : { minX: Math.min(b.minX, nb.minX), minY: Math.min(b.minY, nb.minY), maxX: Math.max(b.maxX, nb.maxX), maxY: Math.max(b.maxY, nb.maxY) };
            }
            if (!b) return null;
            const bb = b as Bounds;
            const p0 = toScreen(bb.minX, bb.maxY), p1 = toScreen(bb.maxX, bb.minY);
            return <rect x={p0.x} y={p0.y} width={Math.max(0, p1.x - p0.x)} height={Math.max(0, p1.y - p0.y)} className="bbox-line" pointerEvents="none" />;
          })()}
          {/* Reporter plug-ins: drawing instructions over the canvas, in font units. */}
          {reporterItems.length > 0 && (
            <g pointerEvents="none">
              {reporterItems.map((it, i) => {
                if (it.kind === "line") {
                  const a = toScreen(it.x0, it.y0), b = toScreen(it.x1, it.y1);
                  return <line key={i} x1={a.x} y1={a.y} x2={b.x} y2={b.y} stroke={it.color ?? "#3dd68c"} strokeWidth={it.width ?? 1.5} />;
                }
                if (it.kind === "path") {
                  return <path key={i} d={it.d} transform={world} stroke={it.color ?? "#3dd68c"} fill={it.fill ?? "none"} strokeWidth={1.5 / view.s} />;
                }
                const p = toScreen(it.x, it.y);
                return <text key={i} x={p.x} y={p.y} fontSize={(it.size ?? 24) * view.s} fill={it.color ?? "#3dd68c"} className="reporter-text">{it.text}</text>;
              })}
            </g>
          )}
          {/* Measure: the dragged line, square crossings, distances and the angle. */}
          {measureLine && (() => {
            const a = toScreen(measureLine.p0.x, measureLine.p0.y), b = toScreen(measureLine.p1.x, measureLine.p1.y);
            const hits = measurement?.hits ?? [];
            const fallbackAngle = Math.atan2(measureLine.p1.y - measureLine.p0.y, measureLine.p1.x - measureLine.p0.x) * 180 / Math.PI;
            return (
              <g pointerEvents="none">
                <line x1={a.x} y1={a.y} x2={b.x} y2={b.y} className="measure-line" />
                {hits.map((h, i) => {
                  const p = toScreen(h.x, h.y);
                  return <rect key={`mh${i}`} x={p.x - 3.5} y={p.y - 3.5} width={7} height={7} className="measure-hit" />;
                })}
                {(measurement?.distances ?? []).map((dist, i) => {
                  const h0 = hits[i], h1 = hits[i + 1];
                  if (!h0 || !h1) return null;
                  const m = toScreen((h0.x + h1.x) / 2, (h0.y + h1.y) / 2);
                  return <text key={`md${i}`} x={m.x + 8} y={m.y - 8} className="measure-label">{Math.round(dist * 10) / 10}</text>;
                })}
                <text x={b.x + 8} y={b.y + 14} className="measure-label">{(measurement ? measurement.angle : fallbackAngle).toFixed(1)}°</text>
              </g>
            );
          })()}
          {showAnnotations && (layer.annotations ?? []).map((a, i) => {
            const off = annDrag && annSel === i ? annDrag : { dx: 0, dy: 0 };
            const p = toScreen(a.x + off.dx, a.y + off.dy);
            const sel = activeTool === "annotate" && annSel === i;
            const cls = `annotation is-${a.kind}${sel ? " is-selected" : ""}`;
            if (a.kind === "text") {
              const w = (a.size || 200) * view.s;
              const lines = (a.text || t("Note")).split("\n");
              return (
                <g key={`an${i}`} className={cls} pointerEvents="none">
                  <rect x={p.x} y={p.y} width={Math.max(60, w)} height={lines.length * 16 + 12} rx={4} />
                  <text x={p.x + 8} y={p.y + 18}>{lines.map((l, k) => <tspan key={k} x={p.x + 8} dy={k ? 16 : 0}>{l}</tspan>)}</text>
                </g>
              );
            }
            if (a.kind === "arrow") {
              const r = ((a.angle ?? 0) * Math.PI) / 180;
              const dx = Math.cos(r), dy = -Math.sin(r);
              const tail = { x: p.x - dx * 90, y: p.y - dy * 90 };
              const head = (s: number) => ({ x: p.x - dx * 14 + -dy * 7 * s, y: p.y - dy * 14 + dx * 7 * s });
              const h1 = head(1), h2 = head(-1);
              return (
                <g key={`an${i}`} className={cls} pointerEvents="none">
                  <line x1={tail.x} y1={tail.y} x2={p.x} y2={p.y} />
                  <path d={`M${h1.x} ${h1.y}L${p.x} ${p.y}L${h2.x} ${h2.y}`} fill="none" />
                </g>
              );
            }
            if (a.kind === "circle") {
              return <circle key={`an${i}`} className={cls} cx={p.x} cy={p.y} r={((a.size || 120) / 2) * view.s} fill="none" pointerEvents="none" />;
            }
            return (
              <g key={`an${i}`} className={cls} pointerEvents="none">
                <line x1={p.x - 9} y1={p.y} x2={p.x + 9} y2={p.y} />
                {a.kind === "plus" && <line x1={p.x} y1={p.y - 9} x2={p.x} y2={p.y + 9} />}
              </g>
            );
          })}
          {activeTool === "hoi" && hoi && hoiKnobs().map((k) => {
            const sel = hoiSel?.pi === k.pi && hoiSel.ni === k.ni;
            const cur = toScreen(k.p0[0], k.p0[1]);
            const dia = (p: Pt, cls: string) => <path className={cls} d={`M${p.x} ${p.y - 5}L${p.x + 5} ${p.y}L${p.x} ${p.y + 5}L${p.x - 5} ${p.y}Z`} />;
            let line = "";
            if (k.p1) {
              for (let i = 0; i <= 24; i++) {
                const q = hoiAt(k.kind, k.p0, k.p1, i / 24), s = toScreen(q[0], q[1]);
                line += `${i ? "L" : "M"}${s.x} ${s.y}`;
              }
            }
            const mid = toScreen(k.mid[0], k.mid[1]);
            return (
              <g key={`hoi${k.pi}:${k.ni}`} className={`hoi${sel ? " is-selected" : ""}${k.kind ? " is-curved" : ""}`} pointerEvents="none">
                {line && <path className="hoi-line" d={line} fill="none" />}
                {k.down && dia(toScreen(k.down[0], k.down[1]), "hoi-down")}
                {k.p1 && dia(toScreen(k.p1[0], k.p1[1]), "hoi-up")}
                {dia(cur, "hoi-here")}
                {k.p1 && <circle className="hoi-knob" cx={mid.x} cy={mid.y} r={4} />}
              </g>
            );
          })}
          {smartDrag && (() => {
            const c = comps[smartDrag.ci];
            const { h } = smartDrag;
            const tt = h.max !== h.min ? (smartDrag.value - h.min) / (h.max - h.min) : 0;
            const p = toScreen(h.atMin[0] + tt * (h.atMax[0] - h.atMin[0]) + (c?.dx ?? 0), h.atMin[1] + tt * (h.atMax[1] - h.atMin[1]) + (c?.dy ?? 0));
            return <text x={p.x + 10} y={p.y - 10} className="smart-label">{`${h.axis} ${smartDrag.value}`}</text>;
          })()}
          {pixelDraft && layer.grid && pixelDraft.cells.map(([x, y]) => {
            const size = layer.grid!.spacing;
            const a = toScreen(x, y + size), b = toScreen(x + size, y);
            return <rect key={`px${x}:${y}`} x={a.x} y={a.y} width={b.x - a.x} height={b.y - a.y} className={`pixel-draft${pixelDraft.on ? "" : " is-erase"}`} pointerEvents="none" />;
          })}
          {knifeLine && (() => {
            const a = toScreen(knifeLine.p0.x, knifeLine.p0.y), b = toScreen(knifeLine.p1.x, knifeLine.p1.y);
            return <line x1={a.x} y1={a.y} x2={b.x} y2={b.y} className="knife-line" pointerEvents="none" />;
          })()}
          {snapLine?.x !== undefined && <line x1={toScreen(snapLine.x, 0).x} x2={toScreen(snapLine.x, 0).x} y1={0} y2={size.h} className="snap-line" pointerEvents="none" />}
          {snapLine?.y !== undefined && <line x1={0} x2={size.w} y1={toScreen(0, snapLine.y).y} y2={toScreen(0, snapLine.y).y} className="snap-line" pointerEvents="none" />}
          {lassoPts && lassoPts.length > 1 && <polygon className="marquee" points={lassoPts.map((p) => `${p.x},${p.y}`).join(" ")} pointerEvents="none" />}
          {marquee && <rect className="marquee" x={Math.min(marquee.x0, marquee.x1)} y={Math.min(marquee.y0, marquee.y1)} width={Math.abs(marquee.x1 - marquee.x0)} height={Math.abs(marquee.y1 - marquee.y0)} />}
        </svg>
        {activeTool === "annotate" && (() => {
          const list = layer.annotations ?? [];
          const sel = annSel !== null ? list[annSel] : undefined;
          const put = (patch: Partial<Annotation>) => annSel !== null && saveAnnotations(list.map((x, i) => (i === annSel ? { ...x, ...patch } : x)));
          const tools: [AnnotSub, string, string][] = [["select", "↖", t("Select annotations")], ["text", "T", t("Text note")], ["arrow", "↗", t("Arrow")], ["circle", "○", t("Circle")], ["plus", "+", t("Plus: make thicker")], ["minus", "−", t("Minus: make thinner")]];
          return (
            <div className="annot-palette" role="toolbar" aria-label={t("Annotate")}>
              <span className="k-caption muted">{t("Annotate")}</span>
              {tools.map(([id, glyphChar, label]) => (
                <button key={id} type="button" className={`icon-btn${annSub === id ? " is-active" : ""}`} aria-pressed={annSub === id} title={label} aria-label={label} onClick={() => setAnnSub(id)}>{glyphChar}</button>
              ))}
              {sel && <>
                <span className="toolbar-sep" />
                {sel.kind === "text" && (
                  <textarea className="metric-input annot-text-input" rows={2} defaultValue={sel.text ?? ""} key={`${annSel}:${sel.text}`} aria-label={t("Note")} placeholder={t("Note")}
                    onBlur={(e) => { const v = e.currentTarget.value; if (v !== (sel.text ?? "")) put({ text: v }); }} />
                )}
                {sel.kind === "arrow" && <MetricField onInvalid={onError} label={t("Angle °")} value={sel.angle ?? 0} onCommit={(v) => put({ angle: v })} />}
                {(sel.kind === "circle" || sel.kind === "text") && <MetricField onInvalid={onError} label={sel.kind === "circle" ? t("Diameter") : t("Width")} value={sel.size || (sel.kind === "circle" ? 120 : 200)} onCommit={(v) => put({ size: Math.max(1, v) })} />}
                <button type="button" className="icon-btn" aria-label={t("Delete annotation")} title={t("Delete annotation")}
                  onClick={() => { saveAnnotations(list.filter((_, i) => i !== annSel)); setAnnSel(null); }}><CloseIcon size={14} /></button>
              </>}
            </div>
          );
        })()}
        {activeTool === "hoi" && hoi && (() => {
          const k = hoiSel ? hoiKnobs().find((x) => x.pi === hoiSel.pi && x.ni === hoiSel.ni) : undefined;
          const put = (kind: HoiKind | null) => k && strokeCall(() => backend.setHoi(glyph, masterId, hoi.axis, k.pi, k.ni, kind));
          const kind = k?.kind ?? null;
          const type = kind?.type ?? "linear";
          const midOf = (a: [number, number], b: [number, number]): [number, number] => [Math.round((a[0] + b[0]) / 2), Math.round((a[1] + b[1]) / 2)];
          const choose = (v: string) => {
            if (!k?.p1) return;
            const [p0, p1] = [k.p0, k.p1];
            if (v === "linear") put(null);
            else if (v === "through") { const m = midOf(p0, p1); put({ type: "through", x: m[0], y: m[1] }); }
            else if (v === "handles") put({ type: "handles", a: [Math.round(p0[0] + (p1[0] - p0[0]) / 3), Math.round(p0[1] + (p1[1] - p0[1]) / 3)], b: [Math.round(p0[0] + ((p1[0] - p0[0]) * 2) / 3), Math.round(p0[1] + ((p1[1] - p0[1]) * 2) / 3)] });
            else put({ type: "rotation", center: midOf(p0, p1), turn: "auto" });
          };
          return (
            <div className="annot-palette hoi-panel" role="toolbar" aria-label={t("Interpolation curves")}>
              <span className="k-caption muted">{t("Interpolation curves")}</span>
              {hoi.axes.length > 1 && (
                <select className="metric-input" aria-label={t("Axis")} value={hoi.axis} onChange={(e) => { setHoiAxis(e.target.value); setHoiSel(null); }}>
                  {hoi.axes.map((a) => <option key={a} value={a}>{a}</option>)}
                </select>
              )}
              {hoi.axes.length === 0 && <span className="k-caption">{t("Interpolation curves need two masters on an axis.")}</span>}
              {hoi.axes.length > 0 && !hoi.upName && <span className="k-caption">{t("No master further up this axis: curves go on the lower master.")}</span>}
              {hoi.upName && !hoi.up && <span className="k-caption">{t("{name} isn't compatible with this master.", { name: hoi.upName })}</span>}
              {hoi.upName && hoi.up && !k && <span className="k-caption muted">{t("To {name}: pick a node.", { name: hoi.upName })}</span>}
              {k?.p1 && <>
                <span className="toolbar-sep" />
                <select className="metric-input" aria-label={t("Curve type")} value={type} onChange={(e) => choose(e.target.value)}>
                  <option value="linear">{t("Linear")}</option>
                  <option value="through">{t("Intermediate point")}</option>
                  <option value="handles">{t("Handles")}</option>
                  <option value="rotation">{t("Rotation")}</option>
                </select>
                {kind?.type === "through" && <>
                  <MetricField onInvalid={onError} label="X" value={kind.x} onCommit={(v) => put({ ...kind, x: v })} />
                  <MetricField onInvalid={onError} label="Y" value={kind.y} onCommit={(v) => put({ ...kind, y: v })} />
                </>}
                {kind?.type === "handles" && <>
                  <MetricField onInvalid={onError} label={t("Handle 1 X")} value={kind.a[0]} onCommit={(v) => put({ ...kind, a: [v, kind.a[1]] })} />
                  <MetricField onInvalid={onError} label={t("Handle 1 Y")} value={kind.a[1]} onCommit={(v) => put({ ...kind, a: [kind.a[0], v] })} />
                  <MetricField onInvalid={onError} label={t("Handle 2 X")} value={kind.b[0]} onCommit={(v) => put({ ...kind, b: [v, kind.b[1]] })} />
                  <MetricField onInvalid={onError} label={t("Handle 2 Y")} value={kind.b[1]} onCommit={(v) => put({ ...kind, b: [kind.b[0], v] })} />
                </>}
                {kind?.type === "rotation" && <>
                  <MetricField onInvalid={onError} label={t("Center X")} value={kind.center[0]} onCommit={(v) => put({ ...kind, center: [v, kind.center[1]] })} />
                  <MetricField onInvalid={onError} label={t("Center Y")} value={kind.center[1]} onCommit={(v) => put({ ...kind, center: [kind.center[0], v] })} />
                  <button type="button" className="btn btn-secondary btn-sm" title={t("Put the centre halfway between the two masters' positions")} onClick={() => put({ ...kind, center: midOf(k.p0, k.p1!) })}>{t("Center")}</button>
                  <select className="metric-input" aria-label={t("Direction")} value={kind.turn ?? "auto"} onChange={(e) => put({ ...kind, turn: e.target.value as "auto" | "clockwise" | "counterclockwise" })}>
                    <option value="auto">{t("Auto")}</option>
                    <option value="clockwise">{t("Clockwise")}</option>
                    <option value="counterclockwise">{t("Counterclockwise")}</option>
                  </select>
                </>}
              </>}
            </div>
          );
        })()}
        {askPixelGlyph && (
          <div className="scrim" role="presentation" onMouseDown={(e) => e.target === e.currentTarget && setAskPixelGlyph(false)}>
            <div className="dialog dialog-sm" role="dialog" aria-modal="true" aria-labelledby="pixel-glyph-title">
              <header className="dialog-header">
                <h2 id="pixel-glyph-title" className="k-subtitle">{t("No glyph named “pixel”")}</h2>
              </header>
              <div className="dialog-body">
                <p className="k-body">{t("The pixel tool draws components of the “pixel” glyph. Create a {n} × {n} square?", { n: layer.grid?.spacing ?? 0 })}</p>
              </div>
              <footer className="dialog-footer">
                <span className="spacer" />
                <button type="button" className="btn btn-secondary" onClick={() => setAskPixelGlyph(false)}>{t("Cancel")}</button>
                <button type="button" className="btn btn-primary" autoFocus onClick={() => {
                  setAskPixelGlyph(false);
                  backend.addPixelGlyph(layer.grid?.spacing ?? 50).then((st) => (onStructureChanged ?? onCommitted)(st), onError);
                }}>{t("Add")}</button>
              </footer>
            </div>
          </div>
        )}
        {ctxMenu && (
          <div className="ctx-menu" style={{ left: ctxMenu.x, top: ctxMenu.y }} role="menu">
            {layer.glyphLocked ? (
              <span className="ctx-note k-caption muted">{t("This glyph is locked. Unlock it to edit.")}</span>
            ) : <>
              {ctxMenu.node && !editingBackground && <>
                <button type="button" role="menuitem" onClick={() => { runCommand("open-corner"); setCtxMenu(null); }}>{t("Open Corner")}</button>
                <button type="button" role="menuitem" onClick={() => { runCommand("make-node-first"); setCtxMenu(null); }}>{t("Make Node First")}</button>
                {selection.size === 2 && <button type="button" role="menuitem" onClick={() => { runCommand("connect-nodes"); setCtxMenu(null); }}>{t("Connect Nodes")}</button>}
                <button type="button" role="menuitem" onClick={() => { runCommand("reconnect-nodes"); setCtxMenu(null); }}>{t("Reconnect Nodes")}</button>
                <span className="ctx-sep" />
                <button type="button" role="menuitem" onClick={() => { runCommand("close-open-paths"); setCtxMenu(null); }}>{t("Close Open Paths")}</button>
                <button type="button" role="menuitem" onClick={() => { runCommand("reverse-contours"); setCtxMenu(null); }}>{t("Reverse Selected Contours")}</button>
                <button type="button" role="menuitem" onClick={() => { runCommand("tidy-up"); setCtxMenu(null); }}>{t("Tidy up Paths")}</button>
                <span className="ctx-sep" />
                {(["corner", "cap", "segment"] as const).map((kind) => (
                  <button key={kind} type="button" role="menuitem" onClick={() => {
                    const prefix = kind === "corner" ? "_corner" : kind === "cap" ? "_cap" : "_segment";
                    if (glyphs.some((g) => g.name.startsWith(prefix))) setPartPick({ kind, path: ctxMenu.node![0], node: ctxMenu.node![1] });
                    else onWarning?.(t("There are no {prefix} glyphs yet. Add one (for example {prefix}.serif) and draw its open path.", { prefix }));
                    setCtxMenu(null);
                  }}>
                    {kind === "corner" ? t("Add Corner Component…") : kind === "cap" ? t("Add Cap Component…") : t("Add Segment Component…")}
                  </button>
                ))}
                <span className="ctx-sep" />
              </>}
              {ctxMenu.path !== null && paths[ctxMenu.path] && !editingBackground && <>
                <button type="button" role="menuitem" onClick={() => { togglePathLocked(ctxMenu.path!); setCtxMenu(null); }}>
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">
                    <rect x="5" y="11" width="14" height="9" rx="2" />
                    {paths[ctxMenu.path].locked ? <path d="M8 11V7a4 4 0 0 1 7.5-2" /> : <path d="M8 11V7a4 4 0 0 1 8 0v4" />}
                  </svg>
                  {paths[ctxMenu.path].locked ? t("Unlock Path") : t("Lock Path")}
                </button>
                <button type="button" role="menuitem" onClick={() => { setFocusPath(focusPath === ctxMenu.path ? null : ctxMenu.path); setCtxMenu(null); }}>
                  {focusPath === ctxMenu.path ? t("Clear Focus") : t("Focus on This Path")}
                </button>
                <span className="ctx-sep" />
              </>}
              {!editingBackground && <button type="button" role="menuitem" onClick={() => { addAnchorAt(ctxMenu.wx, ctxMenu.wy); setCtxMenu(null); }}>{t("Add Anchor")}</button>}
              <button type="button" role="menuitem" onClick={() => { addGuideAt(ctxMenu.wx, ctxMenu.wy, 0); setCtxMenu(null); }}>{t("Add Horizontal Guide")}</button>
              <button type="button" role="menuitem" onClick={() => { addGuideAt(ctxMenu.wx, ctxMenu.wy, 90); setCtxMenu(null); }}>{t("Add Vertical Guide")}</button>
            </>}
          </div>
        )}
        {editingBackground && (
          <div className="bg-banner" role="status">{t("Editing the background · Ctrl+B returns to the outline")}</div>
        )}
        {layer.issues.length > 0 && (
          <div className="notice notice-warning issues-banner k-label" role="status">
            ⚠ {t("This glyph's masters can't be interpolated:")}
            <ul>{layer.issues.map((i) => <li key={i}>{translateMessage(i)}</li>)}</ul>
          </div>
        )}
        {layer.issues.length === 0 && (layer.interpolationWarnings?.length ?? 0) > 0 && (
          <div className="notice interp-banner issues-banner k-label" role="status">
            {t("This glyph's masters may not interpolate cleanly:")}
            <ul>{layer.interpolationWarnings!.map((i) => <li key={i}>{translateMessage(i)}</li>)}</ul>
            {layer.canFixInterpolation && (
              <button type="button" className="btn btn-secondary btn-sm" onClick={fixInterpolation}
                title={t("Line up contour order, direction and start points with the default master. Kinks need fixing by hand.")}>
                {t("Fix order, direction and start points")}
              </button>
            )}
          </div>
        )}
        {paths.length === 0 && comps.length === 0 && !preview && (
          <div className="canvas-empty">
            <p className="k-subtitle">{t("Empty glyph")}</p>
            <p className="canvas-empty-keys k-caption muted">
              <kbd>P</kbd> {t("Draw")} <kbd>F</kbd> {t("Rectangle")} <kbd>{"Ctrl+V"}</kbd> {t("Paste")} <kbd>{"Ctrl+Shift+C"}</kbd> {t("Add component")}
            </p>
          </div>
        )}
        <div className="editor-bottom">
        <div className="info-box in-stack" role="group" aria-label={t("Glyph info")}>
          <div className="info-title">
            <span className="k-body-strong">{layer.glyph}</span>
            <span className="k-code muted">{layer.unicode ? `U+${layer.unicode}` : ""}</span>
          </div>
          <MetricField onInvalid={onError} label={t("LSB")} value={layer.lsb} disabled={layer.lsb === null || editingBackground} onCommit={(v) => setMetric("lsb", v)} metricKey={editingBackground ? undefined : layer.metricsKeys?.left} onKey={(k) => setMetricsKey("lsb", k)} />
          <MetricField onInvalid={onError} label={t("Width")} value={layer.width} onCommit={(v) => setMetric("width", v)} metricKey={layer.metricsKeys?.width} onKey={(k) => setMetricsKey("width", k)} />
          <MetricField onInvalid={onError} label={t("RSB")} value={layer.rsb} disabled={layer.rsb === null || editingBackground} onCommit={(v) => setMetric("rsb", v)} metricKey={editingBackground ? undefined : layer.metricsKeys?.right} onKey={(k) => setMetricsKey("rsb", k)} />
          <span className="info-sep" />
          {selGuide && guideSel ? (
            <>
              <div className="info-title">
                <span className="k-body-strong">{t("Guide")}</span>
                <span className="k-caption muted">{guideSel.scope === "local" ? t("local · layer {name}", { name: currentLayer?.label ?? master }) : t("global · every glyph")}</span>
              </div>
              <MetricField onInvalid={onError} label="X" value={selGuide.x} onCommit={(v) => updateGuide(guideSel, { x: v })} />
              <MetricField onInvalid={onError} label="Y" value={selGuide.y} onCommit={(v) => updateGuide(guideSel, { y: v })} />
              <MetricField onInvalid={onError} decimals label={t("Angle")} value={selGuide.angle} onCommit={(v) => updateGuide(guideSel, { angle: v })} />
              <button type="button" className={`align-toggle${selGuide.locked ? " is-manual" : " is-auto"}`} aria-pressed={selGuide.locked ?? false}
                title={selGuide.locked ? t("Locked: this guide can't be moved. Click to unlock.") : t("Unlocked: this guide can be dragged. Click to lock.")}
                onClick={() => updateGuide(guideSel, { locked: !selGuide.locked })}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
                  <rect x="5" y="11" width="14" height="9" rx="2" />
                  {selGuide.locked ? <path d="M8 11V7a4 4 0 0 1 8 0v4" /> : <path d="M8 11V7a4 4 0 0 1 7.5-2" />}
                </svg>
              </button>
              <button type="button" className="btn btn-secondary btn-sm" onClick={() => switchGuideScope(guideSel)}
                title={guideSel.scope === "local" ? t("Draw this guide in every glyph of this master") : t("Draw this guide only in this layer")}>
                {guideSel.scope === "local" ? t("Make global") : t("Make local")}
              </button>
              <GuideNameField name={selGuide.name ?? ""} onRename={(n) => updateGuide(guideSel, { name: n || undefined })} />
            </>
          ) : oneNode && one ? (
            <>
              <MetricField onInvalid={onError} label="X" value={oneNode.x} onCommit={(v) => { const next = moveSelection(paths, selection, v - oneNode.x, 0); setPaths(next); commit(next); }} />
              <MetricField onInvalid={onError} label="Y" value={oneNode.y} onCommit={(v) => { const next = moveSelection(paths, selection, 0, v - oneNode.y); setPaths(next); commit(next); }} />
            </>
          ) : oneComp ? (
            <>
              <span className="info-comp k-label" title={t("Selected component")}>◇ {oneComp.base}</span>
              <button type="button" className={`align-toggle${oneComp.align ? " is-auto" : " is-manual"}`} aria-pressed={oneComp.align}
                title={oneComp.align ? t("Placed by its anchors. Click to place it by hand.") : t("Placed by hand. Click to place it by its anchors.")}
                onClick={() => setComponentAlign([...compSel][0], !oneComp.align)}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
                  <rect x="5" y="11" width="14" height="9" rx="2" />
                  {oneComp.align ? <path d="M8 11V7a4 4 0 0 1 8 0v4" /> : <path d="M8 11V7a4 4 0 0 1 7.5-2" />}
                </svg>
              </button>
              <MetricField onInvalid={onError} label="X" value={oneComp.transform[4] + oneComp.dx} onCommit={(v) => moveComp([...compSel][0], v - (oneComp.transform[4] + oneComp.dx), 0)} />
              <MetricField onInvalid={onError} label="Y" value={oneComp.transform[5] + oneComp.dy} onCommit={(v) => moveComp([...compSel][0], 0, v - (oneComp.transform[5] + oneComp.dy))} />
              {(oneComp.smartAxes ?? []).map((a) => (
                <MetricField key={a.name} onInvalid={onError} label={a.name} value={oneComp.smart?.[a.name] ?? a.min}
                  onCommit={(v) => setComponentSmart([...compSel][0], { ...(oneComp.smart ?? {}), [a.name]: v })} />
              ))}
            </>
          ) : oneAnchor ? (
            <>
              <AnchorNameField name={oneAnchor.name} onRename={(n) => renameAnchorTo(oneAnchor.name, n)} />
              <MetricField onInvalid={onError} label="X" value={oneAnchor.x} onCommit={(v) => moveAnchorsBy([{ name: oneAnchor.name, dx: v - oneAnchor.x, dy: 0 }])} />
              <MetricField onInvalid={onError} label="Y" value={oneAnchor.y} onCommit={(v) => moveAnchorsBy([{ name: oneAnchor.name, dx: 0, dy: v - oneAnchor.y }])} />
            </>
          ) : (
            <span className="k-caption muted info-count">{selectionLabel(selection.size, compSel.size, anchorSel.size)}</span>
          )}
        </div>
        </div>
      </div>
      {showSidebar && (
        <aside className="edit-sidebar" aria-label={t("Sidebar")}>
        <VariationPreview glyph={glyph} masterId={master} version={version} />
        <div className="transform-panel layers-panel">
          <button type="button" className="transform-head" aria-expanded={layersOpen} onClick={() => setLayersOpen((o) => !o)}>
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round"><path d="M6 9l6 6 6-6" /></svg>
            {t("Layers")}
          </button>
          {layersOpen && (
            <div className="layers-body">
              <ul className="layer-list" role="listbox" aria-label={t("Layer")}>
                {layerList
                  .filter((l) => (l.kind === "master" ? true : l.kind === "brace" || l.masterId === master))
                  .map((l) => {
                    const current = l.key === masterId;
                    const switchable = l.kind !== "master" || l.key === master;
                    return (
                      <li key={l.key}>
                        <button type="button" role="option" aria-selected={current} disabled={!switchable} className={`layer-row${current ? " is-selected" : ""}`}
                          title={switchable ? undefined : t("Switch masters with the master buttons at the top of the window")}
                          onClick={() => onPickLayer?.(l.key)}>
                          <span className="layer-name">{l.kind === "brace" ? `{${l.label}}` : l.kind === "color" ? t("Colour {n}", { n: l.label }) : l.label}</span>
                          <span className="k-caption muted">{l.kind === "master" ? t("master") : l.kind === "brace" ? t("intermediate") : l.kind === "bracket" ? t("alternate") : l.kind === "smart" ? t("smart pole") : l.kind === "color" ? t("colour") : t("backup")}</span>
                        </button>
                      </li>
                    );
                  })}
              </ul>
              <div className="layer-actions">
                <button className="btn btn-secondary btn-sm" onClick={() => openLayerDialog("brace", false)} title={t("Add an intermediate (brace) layer")}>{t("+ Intermediate")}</button>
                <button className="btn btn-secondary btn-sm" onClick={() => openLayerDialog("bracket", false)} title={t("Add an alternate design (bracket layers)")}>{t("+ Alternate")}</button>
                <button className="btn btn-secondary btn-sm" onClick={addColorLayer} title={t("Add a colour layer (colour fonts): a drawing filled with a palette colour or gradient")}>{t("+ Colour")}</button>
                <button className="btn btn-secondary btn-sm" onClick={addBackup} title={t("Save a copy of this master's drawing as a backup layer. Backups aren't exported.")}>{t("+ Backup")}</button>
                <button className="btn btn-secondary btn-sm" onClick={() => setSmartDialog({ kind: "axes", axes: layer?.smartAxes ?? [] })} title={t("Make this a smart glyph: components of it get their own axes")}>{t("Smart…")}</button>
                {(layer?.smartAxes?.length ?? 0) > 0 && (
                  <button className="btn btn-secondary btn-sm" onClick={() => setSmartDialog({ kind: "pole", axes: layer?.smartAxes ?? [] })} title={t("Add the shape at another position on the smart axes")}>{t("+ Pole")}</button>
                )}
              </div>
              <div className="layer-actions">
                {currentLayer && currentLayer.kind !== "master" && (
                  <>
                    {(currentLayer.kind === "brace" || currentLayer.kind === "bracket") && (
                      <button className="btn btn-secondary btn-sm" onClick={() => openLayerDialog(currentLayer.kind as "brace" | "bracket", true)}>{currentLayer.kind === "brace" ? t("Move…") : t("Ranges…")}</button>
                    )}
                    {currentLayer.kind === "backup" && (
                      <button className="btn btn-secondary btn-sm" onClick={useBackup} title={t("Make this the master's drawing; the current master drawing is kept as this backup")}>{t("Use as master")}</button>
                    )}
                    {currentLayer.kind === "color" && (
                      <>
                        <button className="btn btn-secondary btn-sm" onClick={() => setPainting(true)} title={t("Choose this layer's colour or gradient")}>{t("Paint…")}</button>
                        <button className="btn btn-secondary btn-sm" onClick={() => moveColorLayer(false)} title={t("Move this colour layer down (drawn earlier)")} aria-label={t("Move down")}>↓</button>
                        <button className="btn btn-secondary btn-sm" onClick={() => moveColorLayer(true)} title={t("Move this colour layer up (drawn later, on top)")} aria-label={t("Move up")}>↑</button>
                      </>
                    )}
                    <button className="btn btn-secondary btn-sm" onClick={removeCurrentLayer} title={currentLayer.kind === "bracket" ? t("Removes this alternate from every master") : currentLayer.kind === "smart" ? t("Removes this pole") : currentLayer.kind === "color" ? t("Removes this colour layer from every master") : currentLayer.kind === "backup" ? t("Removes this backup") : t("Removes this intermediate layer")}>{t("Remove")}</button>
                  </>
                )}
              </div>
            </div>
          )}
        </div>
        <div className="transform-panel">
          <button type="button" className="transform-head" aria-expanded={transformOpen} onClick={() => setTransformOpen((o) => !o)}>
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round"><path d="M6 9l6 6 6-6" /></svg>
            {t("Transform")}
          </button>
          {transformOpen && (
            <div className="transform-body">
              <MetricField onInvalid={onError} decimals label={t("Scale X %")} value={tf.sx} onCommit={(v) => setTf({ ...tf, sx: v })} />
              <MetricField onInvalid={onError} decimals label={t("Scale Y %")} value={tf.sy} onCommit={(v) => setTf({ ...tf, sy: v })} />
              <MetricField onInvalid={onError} decimals label={t("Rotate °")} value={tf.rot} onCommit={(v) => setTf({ ...tf, rot: v })} />
              <MetricField onInvalid={onError} decimals label={t("Skew X °")} value={tf.kx} onCommit={(v) => setTf({ ...tf, kx: v })} />
              <MetricField onInvalid={onError} decimals label={t("Skew Y °")} value={tf.ky} onCommit={(v) => setTf({ ...tf, ky: v })} />
              <button type="button" className={`align-toggle${tf.mh ? " is-auto" : " is-manual"}`} aria-pressed={tf.mh}
                title={t("Mirror horizontally (flip left to right)")} aria-label={t("Mirror horizontally")}
                onClick={() => setTf({ ...tf, mh: !tf.mh })}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><path d="M12 3v18" strokeDasharray="2 2" /><path d="M8 8l-4 4 4 4" /><path d="M16 8l4 4-4 4" /></svg>
              </button>
              <button type="button" className={`align-toggle${tf.mv ? " is-auto" : " is-manual"}`} aria-pressed={tf.mv}
                title={t("Mirror vertically (flip top to bottom)")} aria-label={t("Mirror vertically")}
                onClick={() => setTf({ ...tf, mv: !tf.mv })}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round"><path d="M3 12h18" strokeDasharray="2 2" /><path d="M8 8l4-4 4 4" /><path d="M8 16l4 4 4-4" /></svg>
              </button>
              <div className="origin-picker" role="radiogroup" aria-label={t("Transform origin")}>
                {ORIGIN_LABELS.map((label, i) => (
                  <button key={label} type="button" role="radio" aria-checked={tf.origin === i} className={`origin-dot${tf.origin === i ? " is-active" : ""}`}
                    title={t(label)} aria-label={t(label)} onClick={() => setTf({ ...tf, origin: i })} />
                ))}
              </div>
              <label className="check-row" title={t("Apply the transform to this layer in every master")}>
                <input type="checkbox" checked={tf.allMasters} disabled={editingBackground} onChange={(e) => setTf({ ...tf, allMasters: e.target.checked })} />
                <span className="k-body">{t("All masters")}</span>
              </label>
              <button type="button" className="btn btn-primary btn-sm" onClick={applyTransform}>{t("Apply")}</button>
              <div className="align-row" role="group" aria-label={t("Align")}>
                {ALIGN_BUTTONS.map(([op, label, icon]) => {
                  const disabled = editingBackground || layer.glyphLocked || (selection.size === 0 && compSel.size === 0 && anchorSel.size === 0);
                  return (
                    <button key={op} type="button" className="icon-btn align-btn" title={t(label)} aria-label={t(label)} disabled={disabled} onClick={() => runAlign(op)}>
                      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round">{icon}</svg>
                    </button>
                  );
                })}
                {BOOLEAN_BUTTONS.map(([op, label, icon]) => (
                  <button key={op} type="button" className="icon-btn align-btn" title={t(label)} aria-label={t(label)}
                    disabled={editingBackground || layer.glyphLocked || paths.filter((p) => p.closed && !p.locked).length < (op === "union" ? 1 : 2)}
                    onClick={() => runCommand(op)}>
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6">{icon}</svg>
                  </button>
                ))}
                <label className="align-to" title={t("Align to the selection's box, the glyph's width (left, centre, right) or a metric line (top, middle, bottom)")}>
                  <span className="k-caption muted">{t("to")}</span>
                  <select className="select" value={alignTo} aria-label={t("Align to")} onChange={(e) => setAlignTo(e.target.value as typeof alignTo)}>
                    <option value="selection">{t("Selection")}</option>
                    <option value="width">{t("Glyph width")}</option>
                    <option value="baseline">{t("Baseline")}</option>
                    <option value="xHeight">{t("x-height")}</option>
                    <option value="capHeight">{t("Cap height")}</option>
                    <option value="ascender">{t("Ascender")}</option>
                    <option value="descender">{t("Descender")}</option>
                  </select>
                </label>
              </div>
            </div>
          )}
        </div>
        {(layer.parts ?? []).length > 0 && (
        <div className="transform-panel parts-panel">
          <button type="button" className="transform-head" aria-expanded={partsOpen} onClick={() => setPartsOpen((o) => !o)}>
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round"><path d="M6 9l6 6 6-6" /></svg>
            {t("Corner components")}
            <span className="k-caption muted">{(layer.parts ?? []).length}</span>
          </button>
          {partsOpen && (
            <div className="transform-body stroke-body">
              {(layer.parts ?? []).map((pt, i) => {
                const disabled = editingBackground || layer.glyphLocked;
                const put = (patch: Partial<PathPart>) => strokeCall(() => backend.setPathPart(glyph, masterId, i, { ...pt, ...patch }));
                const prefix = pt.kind === "corner" ? "_corner" : pt.kind === "cap" ? "_cap" : "_segment";
                const scale = pt.scale ?? [1, 1];
                return (
                  <div key={i} className="part-row">
                    <div className="part-head">
                      <span className="k-caption muted">{pt.kind === "corner" ? t("Corner") : pt.kind === "cap" ? t("Cap") : t("Segment")} · {t("node {n}", { n: pt.node + 1 })}</span>
                      <button type="button" className="icon-btn" aria-label={t("Remove {name}", { name: pt.glyph })} title={t("Remove")} disabled={disabled}
                        onClick={() => strokeCall(() => backend.setPathPart(glyph, masterId, i, null))}><CloseIcon size={14} /></button>
                    </div>
                    <select className="select" value={pt.glyph} disabled={disabled} aria-label={t("Glyph")} onChange={(e) => put({ glyph: e.target.value })}>
                      {[...new Set([pt.glyph, ...glyphs.map((g) => g.name).filter((n) => n.startsWith(prefix))])].map((n) => <option key={n} value={n}>{n}</option>)}
                    </select>
                    <div className="part-scale">
                      <MetricField onInvalid={onError} decimals label={t("Width %")} value={Math.abs(scale[0]) * 100} disabled={disabled} onCommit={(v) => put({ scale: [Math.sign(scale[0] || 1) * v / 100, scale[1]] })} />
                      <MetricField onInvalid={onError} decimals label={t("Height %")} value={scale[1] * 100} disabled={disabled} onCommit={(v) => put({ scale: [scale[0], v / 100] })} />
                    </div>
                    {pt.kind === "corner" && (
                      <div className="segmented stroke-seg" role="radiogroup" aria-label={t("Alignment")}>
                        {([["out", "◁", t("Align to the outgoing stroke")], ["in", "▷", t("Align to the incoming stroke")], ["middle", "◁▷", t("Align halfway between the strokes")]] as const).map(([v, label, title]) => (
                          <button key={v} type="button" role="radio" aria-checked={(pt.align ?? "out") === v} title={title} aria-label={title} disabled={disabled}
                            className={`segment${(pt.align ?? "out") === v ? " is-selected" : ""}`} onClick={() => put({ align: v })}>{label}</button>
                        ))}
                        <button type="button" className="segment" title={t("Flip the corner")} aria-label={t("Flip the corner")} disabled={disabled}
                          onClick={() => put({ scale: [-scale[0], scale[1]] })}>⇋</button>
                      </div>
                    )}
                    {pt.kind === "cap" && (
                      <label className="check-row">
                        <input type="checkbox" checked={!!pt.fit} disabled={disabled} onChange={(e) => put({ fit: e.target.checked })} />
                        <span className="k-body">{t("Fit to stroke width")}</span>
                      </label>
                    )}
                  </div>
                );
              })}
              <div className="hints-add">
                <button type="button" className="btn btn-secondary btn-sm" disabled={editingBackground || layer.glyphLocked}
                  title={t("Turn every corner, cap and segment component of this glyph into outline, in all masters")}
                  onClick={() => strokeCall(() => backend.decomposeParts(glyph, masterId))}>{t("Decompose")}</button>
              </div>
            </div>
          )}
        </div>
        )}
        <div className="transform-panel image-panel">
          <button type="button" className="transform-head" aria-expanded={imageOpen} onClick={() => setImageOpen((o) => !o)}>
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round"><path d="M6 9l6 6 6-6" /></svg>
            {t("Image")}
            {layer.image && <span className="k-caption muted">{layer.image.path.split(/[\\/]/).pop()}</span>}
          </button>
          {imageOpen && (() => {
            const disabled = editingBackground || layer.glyphLocked;
            const im = layer.image;
            if (!im) {
              return (
                <div className="transform-body">
                  <p className="k-caption muted stroke-note">{t("Place a scan or sketch behind the outline to trace it. Images never export.")}</p>
                  <button type="button" className="btn btn-secondary btn-sm" disabled={disabled} onClick={addImage}>{t("Add Image…")}</button>
                </div>
              );
            }
            const w = imageData?.width ?? 0, h = imageData?.height ?? 0;
            const put = (patch: Partial<LayerImage>) => strokeCall(() => backend.setLayerImage(glyph, masterId, { ...im, ...patch }));
            return (
              <div className="transform-body stroke-body">
                {!imageData?.dataUrl && <p className="notice notice-warning k-caption">{t("Image not found: {file}", { file: imageData?.file ?? im.path })}</p>}
                <div className="part-scale">
                  <MetricField onInvalid={onError} label="X" value={im.x} disabled={disabled || im.locked} onCommit={(v) => put({ x: v })} />
                  <MetricField onInvalid={onError} label="Y" value={im.y} disabled={disabled || im.locked} onCommit={(v) => put({ y: v })} />
                </div>
                <div className="part-scale">
                  <MetricField onInvalid={onError} label={t("W")} value={w ? w * im.scaleX : null} disabled={disabled || im.locked || !w} onCommit={(v) => w && put({ scaleX: v / w })} />
                  <MetricField onInvalid={onError} label={t("H")} value={h ? h * im.scaleY : null} disabled={disabled || im.locked || !h} onCommit={(v) => h && put({ scaleY: v / h })} />
                  <MetricField onInvalid={onError} decimals label={t("Rotate °")} value={im.angle ?? 0} disabled={disabled || im.locked} onCommit={(v) => put({ angle: v })} />
                </div>
                <label className="check-row" title={t("A locked image can't be moved or changed")}>
                  <input type="checkbox" checked={!!im.locked} disabled={disabled} onChange={(e) => put({ locked: e.target.checked })} />
                  <span className="k-body">{t("Lock image")}</span>
                </label>
                <div className="hints-add">
                  <button type="button" className="btn btn-secondary btn-sm" disabled={disabled || im.locked || !w}
                    title={t("Show only the part of the image inside the glyph's width, from descender to ascender")}
                    onClick={() => {
                      const { ascender, descender } = layer.metrics;
                      const cx = Math.max(0, (0 - im.x) / im.scaleX), cy = Math.max(0, (descender - im.y) / im.scaleY);
                      const cw = Math.min(w - cx, layer.width / im.scaleX), ch = Math.min(h - cy, (ascender - descender) / im.scaleY);
                      if (cw > 0 && ch > 0) put({ crop: [cx, cy, cw, ch] });
                    }}>{t("Crop to Layer Bounds")}</button>
                  <button type="button" className="btn btn-secondary btn-sm" disabled={disabled || im.locked || !im.crop} onClick={() => put({ crop: null })}>{t("Reset Crop")}</button>
                </div>
                <div className="hints-add">
                  <button type="button" className="btn btn-secondary btn-sm" disabled={!imageData?.dataUrl} onClick={() => imageData && revealItemInDir(imageData.file).catch(onError)}>{t("Reveal")}</button>
                  <button type="button" className="btn btn-secondary btn-sm" disabled={disabled} onClick={addImage}>{t("Replace…")}</button>
                  <button type="button" className="btn btn-secondary btn-sm" disabled={disabled} onClick={() => strokeCall(() => backend.setLayerImage(glyph, masterId, null))}>{t("Remove")}</button>
                </div>
              </div>
            );
          })()}
        </div>
        <div className="transform-panel pictures-panel">
          <button type="button" className="transform-head" aria-expanded={picturesOpen} onClick={() => setPicturesOpen((o) => !o)}>
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round"><path d="M6 9l6 6 6-6" /></svg>
            {t("Bitmaps and SVG")}
            {pictures.length > 0 && <span className="k-caption muted">{pictures.length}</span>}
          </button>
          {picturesOpen && (() => {
            const disabled = layer.glyphLocked;
            const sel = pictures.find((q) => (shownPicture === 0 ? q.kind === "svg" : q.ppem === shownPicture)) ?? null;
            const key = (q: GlyphPicture) => (q.kind === "svg" ? 0 : q.ppem);
            return (
              <div className="transform-body stroke-body">
                <p className="k-caption muted stroke-note">{t("Colour pictures for apps that show sbix bitmaps or SVG glyphs. The outline stays the fallback everywhere else.")}</p>
                {pictures.length > 0 && (
                  <ul className="picture-list" role="listbox" aria-label={t("Bitmaps and SVG")}>
                    {pictures.map((q) => (
                      <li key={q.kind + q.ppem} role="option" aria-selected={sel === q}
                        className={`picture-row${sel === q ? " is-selected" : ""}`}
                        onClick={() => setShownPicture(sel === q ? null : key(q))}>
                        <span className="k-body">{q.kind === "svg" ? t("SVG drawing") : t("Bitmap {n} px", { n: q.ppem })}</span>
                        <span className="k-caption muted" title={q.file}>{q.dataUrl ? q.path.split(/[\/]/).pop() : t("Not found")}</span>
                      </li>
                    ))}
                  </ul>
                )}
                {sel && (
                  <>
                    {!sel.dataUrl && <p className="notice notice-warning k-caption">{t("Image not found: {file}", { file: sel.file })}</p>}
                    <div className="part-scale">
                      <MetricField onInvalid={onError} label="X" value={sel.x} disabled={disabled} onCommit={(v) => pictureCall(() => backend.placeGlyphPicture(glyph, key(sel), v, sel.y))} />
                      <MetricField onInvalid={onError} label="Y" value={sel.y} disabled={disabled} onCommit={(v) => pictureCall(() => backend.placeGlyphPicture(glyph, key(sel), sel.x, v))} />
                      {sel.kind === "svg" && (
                        <MetricField onInvalid={onError} label={t("H")} value={sel.height} disabled={disabled || sel.height <= 0}
                          onCommit={(v) => { if (v > 0 && sel.height > 0) pictureCall(() => backend.placeGlyphPicture(glyph, 0, sel.x, sel.y, (sel.scale * v) / sel.height)); }} />
                      )}
                    </div>
                    <div className="hints-add">
                      <button type="button" className="btn btn-secondary btn-sm" disabled={!sel.dataUrl} onClick={() => revealItemInDir(sel.file).catch(onError)}>{t("Reveal")}</button>
                      <button type="button" className="btn btn-secondary btn-sm" disabled={disabled}
                        onClick={() => { pictureCall(() => backend.removeGlyphPicture(glyph, key(sel))); setShownPicture(null); }}>{t("Remove")}</button>
                    </div>
                  </>
                )}
                <div className="hints-add picture-add">
                  <label className="k-caption muted" htmlFor="picture-ppem">{t("Pixel size")}</label>
                  <input id="picture-ppem" className="metric-input" type="number" min={1} max={2048} value={newPpem}
                    onChange={(e) => setNewPpem(Math.max(1, Math.min(2048, Math.round(Number(e.target.value) || 0))))} />
                  <button type="button" className="btn btn-secondary btn-sm" disabled={disabled} onClick={() => { addBitmap().catch(onError); }}>{t("Add Bitmap…")}</button>
                </div>
                <div className="hints-add">
                  <button type="button" className="btn btn-secondary btn-sm" disabled={disabled} onClick={() => { addSvgDrawing().catch(onError); }}>{pictures.some((q) => q.kind === "svg") ? t("Replace SVG…") : t("Add SVG…")}</button>
                </div>
                <p className="k-caption muted">{t("Export switches live in Font Info › Custom Parameters: Export sbix Table, SBIX to SVG and Color Layers to SVG.")}</p>
              </div>
            );
          })()}
        </div>
        <div className="transform-panel shapes-panel">
          <button type="button" className="transform-head" aria-expanded={shapesOpen} onClick={() => setShapesOpen((o) => !o)}>
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round"><path d="M6 9l6 6 6-6" /></svg>
            {t("Shapes")}
            <span className="k-caption muted">{paths.length + comps.length}</span>
          </button>
          {shapesOpen && (() => {
            const disabled = editingBackground || layer.glyphLocked;
            // The layer's shapes in drawing order: paths and components interleaved as stored.
            const order = layer.shapeOrder ?? [...paths.map((_, i) => ({ kind: "path" as const, index: i })), ...comps.map((_, i) => ({ kind: "component" as const, index: i }))];
            const move = (from: number, to: number) => {
              if (to < 0 || to >= order.length) return;
              const ids = order.map((_, i) => i);
              const [x] = ids.splice(from, 1);
              ids.splice(to, 0, x);
              strokeCall(() => backend.reorderShapes(glyph, masterId, ids, orderAll));
            };
            return (
              <div className="transform-body stroke-body">
                <p className="k-caption muted stroke-note">{t("Top draws first. A mask cuts every shape above it in this list.")}</p>
                <ol className="shape-list">
                  {order.map((s, i) => {
                    const p = s.kind === "path" ? paths[s.index] : null;
                    const label = s.kind === "path" ? t("Path {n}", { n: s.index + 1 }) : comps[s.index]?.base ?? "";
                    return (
                      <li key={i} className={`shape-row${s.kind === "path" && strokePath === s.index ? " is-selected" : ""}`}>
                        <span className="k-body">{label}</span>
                        {p?.mask && <span className="pill pill-group">{t("mask")}</span>}
                        {p?.stroke && <span className="pill pill-group">{t("stroke")}</span>}
                        {p?.locked && <span className="k-caption muted">🔒</span>}
                        <span className="spacer" />
                        <button type="button" className="icon-btn" disabled={disabled || i === 0} aria-label={t("Move up")} title={t("Move up")} onClick={() => move(i, i - 1)}>↑</button>
                        <button type="button" className="icon-btn" disabled={disabled || i === order.length - 1} aria-label={t("Move down")} title={t("Move down")} onClick={() => move(i, i + 1)}>↓</button>
                      </li>
                    );
                  })}
                </ol>
                <label className="check-row" title={t("Reorder the shapes the same way in every master")}>
                  <input type="checkbox" checked={orderAll} onChange={(e) => setOrderAll(e.target.checked)} />
                  <span className="k-body">{t("All masters")}</span>
                </label>
                {strokePath !== null && strokeTarget && (
                  <label className="check-row" title={t("Subtracts this path from the shapes above it in the list")}>
                    <input type="checkbox" checked={!!strokeTarget.mask} disabled={disabled || !strokeTarget.closed}
                      onChange={(e) => strokeCall(() => backend.setPathMask(glyph, masterId, strokePath, e.target.checked))} />
                    <span className="k-body">{t("Mask")}</span>
                    <span className="k-caption muted">{t("Subtracts this path from the shapes above it in the list")}</span>
                  </label>
                )}
              </div>
            );
          })()}
        </div>
        <div className="transform-panel stroke-panel">
          <button type="button" className="transform-head" aria-expanded={strokeOpen} onClick={() => setStrokeOpen((o) => !o)}>
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round"><path d="M6 9l6 6 6-6" /></svg>
            {t("Stroke")}
          </button>
          {strokeOpen && (() => {
            const disabled = editingBackground || layer.glyphLocked;
            if (strokePath === null || !strokeTarget) {
              return <p className="k-caption muted stroke-note">{t("Select a path to draw it with a pen instead of filling it.")}</p>;
            }
            const s = strokeTarget.stroke;
            if (!s) {
              return (
                <div className="transform-body">
                  <button type="button" className="btn btn-secondary btn-sm" disabled={disabled}
                    onClick={() => strokeCall(() => backend.setPathStroke(glyph, masterId, strokePath, { width: 40, position: 0, join: "round", startCap: "butt", endCap: "butt" }))}>{t("Stroke this path")}</button>
                </div>
              );
            }
            const put = (patch: Partial<typeof s>) => strokeCall(() => backend.setPathStroke(glyph, masterId, strokePath, { ...s, ...patch }));
            const seg = <K extends string>(value: K, options: [K, string][], onPick: (v: K) => void, label: string) => (
              <div className="segmented stroke-seg" role="radiogroup" aria-label={label}>
                {options.map(([v, text]) => (
                  <button key={v} type="button" role="radio" aria-checked={value === v} disabled={disabled}
                    className={`segment${value === v ? " is-selected" : ""}`} onClick={() => onPick(v)}>{text}</button>
                ))}
              </div>
            );
            const pos = s.position ?? 0;
            const caps: ["butt" | "round" | "square", string][] = [["butt", t("Flat")], ["round", t("Round")], ["square", t("Square")]];
            return (
              <div className="transform-body stroke-body">
                <label className="info-field">
                  <span className="k-caption muted">{t("Brush")}</span>
                  <select className="select" value={s.brush ?? ""} disabled={disabled} aria-label={t("Brush")}
                    title={t("A brush glyph (named _brush.something, with start and end anchors) drawn along the path instead of a pen")}
                    onChange={(e) => put({ brush: e.target.value || undefined })}>
                    <option value="">{t("Pen (no brush)")}</option>
                    {glyphs.filter((g) => g.name.startsWith("_brush")).map((g) => <option key={g.name} value={g.name}>{g.name}</option>)}
                  </select>
                </label>
                {!s.brush && <>
                <MetricField onInvalid={onError} decimals label={t("Width")} value={s.width} disabled={disabled} onCommit={(v) => put({ width: v })} />
                <span className="k-caption muted">{t("Position")}</span>
                {seg(pos <= -100 ? "l" : pos >= 100 ? "r" : pos === 0 ? "c" : "x", [["l", t("Left")], ["c", t("Centre")], ["r", t("Right")]], (v) => put({ position: v === "l" ? -100 : v === "r" ? 100 : 0 }), t("Position"))}
                <MetricField onInvalid={onError} decimals label={t("Position (−100 to 100)")} value={pos} disabled={disabled} onCommit={(v) => put({ position: v })} />
                <span className="k-caption muted">{t("Join")}</span>
                {seg(s.join ?? "round", [["miter", t("Miter")], ["round", t("Round")], ["bevel", t("Bevel")]], (v) => put({ join: v }), t("Join"))}
                {!strokeTarget.closed && <>
                  <span className="k-caption muted">{t("Start cap")}</span>
                  {seg(s.startCap ?? "butt", caps, (v) => put({ startCap: v }), t("Start cap"))}
                  <span className="k-caption muted">{t("End cap")}</span>
                  {seg(s.endCap ?? "butt", caps, (v) => put({ endCap: v }), t("End cap"))}
                </>}
                {strokeTarget.closed && (
                  <label className="check-row">
                    <input type="checkbox" checked={!!s.fill} disabled={disabled} onChange={(e) => put({ fill: e.target.checked })} />
                    <span className="k-body">{t("Fill (closed paths)")}</span>
                  </label>
                )}
                {penNode !== null && (
                  <div className="pen-point">
                    <span className="k-caption muted">{t("Pen point (node {n})", { n: penNode + 1 })}</span>
                    <MetricField onInvalid={onError} decimals label={t("Pen width")} value={s.widths?.[penNode] ?? s.width} disabled={disabled}
                      onCommit={(v) => strokeCall(() => backend.setPenWidth(glyph, masterId, strokePath, penNode, v))} />
                    {s.widths?.[penNode] !== undefined && (
                      <button type="button" className="btn btn-secondary btn-sm" disabled={disabled}
                        title={t("Use the stroke's width at this node again")}
                        onClick={() => strokeCall(() => backend.setPenWidth(glyph, masterId, strokePath, penNode, null))}>{t("Reset")}</button>
                    )}
                  </div>
                )}
                {penNode === null && <p className="k-caption muted stroke-note">{t("Select one node to give it its own pen width.")}</p>}
                </>}
                <div className="hints-add">
                  <button type="button" className="btn btn-secondary btn-sm" disabled={disabled}
                    title={t("Turn every stroked path of this glyph into outlines, in all masters")}
                    onClick={() => strokeCall(() => backend.expandStrokes(glyph, masterId))}>{t("Expand Outline")}</button>
                  <button type="button" className="btn btn-secondary btn-sm" disabled={disabled}
                    onClick={() => strokeCall(() => backend.setPathStroke(glyph, masterId, strokePath, null))}>{t("Remove stroke")}</button>
                </div>
              </div>
            );
          })()}
        </div>
        <div className="transform-panel hints-panel">
          <button type="button" className="transform-head" aria-expanded={hintsOpen} onClick={() => setHintsOpen((o) => !o)}>
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round"><path d="M6 9l6 6 6-6" /></svg>
            {t("Hints")}
            {layer.hints.length > 0 && <span className="k-caption muted">{tn(layer.hints.length, "{n} hint", "{n} hints")}</span>}
          </button>
          {hintsOpen && (
            <div className="transform-body hints-body">
              {layer.hints.length === 0 && (
                <p className="k-caption muted hints-note">
                  {layer.autoHints.length > 0
                    ? tn(layer.autoHints.length, "Automatic: {n} hint from the master's stems.", "Automatic: {n} hints from the master's stems.")
                    : t("No hints yet. Add stems in Font Info for automatic hints, or add hints here.")}
                </p>
              )}
              {layer.hints.map((h, i) => {
                const patch = (p: Partial<Hint>) => setHints(layer.hints.map((x, j) => (j === i ? { ...x, ...p } : x)));
                const disabled = editingBackground || layer.glyphLocked;
                return (
                  <div key={i} className="hint-row">
                    <select className="select" value={h.horizontal ? "h" : "v"} aria-label={t("Hint direction")} disabled={disabled}
                      onChange={(e) => patch({ horizontal: e.target.value === "h" })}>
                      <option value="h">{t("Horizontal")}</option>
                      <option value="v">{t("Vertical")}</option>
                    </select>
                    <MetricField onInvalid={onError} label={t("Position")} value={h.position} disabled={disabled} onCommit={(v) => patch({ position: v })} />
                    <MetricField onInvalid={onError} label={t("Width")} value={h.width} disabled={disabled} onCommit={(v) => patch({ width: v, ...(v !== 0 ? { ghostTop: undefined } : {}) })} />
                    {h.width === 0 && (
                      <button type="button" className="align-toggle is-manual" disabled={disabled}
                        title={t("Which edge this ghost hint marks")}
                        onClick={() => patch({ ghostTop: !h.ghostTop })}>
                        {h.horizontal ? (h.ghostTop ? t("Top") : t("Bottom")) : (h.ghostTop ? t("Right") : t("Left"))}
                      </button>
                    )}
                    <button type="button" className="icon-btn" aria-label={t("Remove hint")} title={t("Remove hint")} disabled={disabled}
                      onClick={() => setHints(layer.hints.filter((_, j) => j !== i))}><CloseIcon size={14} /></button>
                  </div>
                );
              })}
              <div className="hints-add">
                <button type="button" className="btn btn-secondary btn-sm" disabled={editingBackground || layer.glyphLocked}
                  onClick={() => setHints([...layer.hints, { horizontal: true, position: 0, width: 80 }])}>{t("+ Horizontal")}</button>
                <button type="button" className="btn btn-secondary btn-sm" disabled={editingBackground || layer.glyphLocked}
                  onClick={() => setHints([...layer.hints, { horizontal: false, position: 0, width: 80 }])}>{t("+ Vertical")}</button>
                <button type="button" className="btn btn-secondary btn-sm" disabled={editingBackground || layer.glyphLocked}
                  title={t("A ghost hint marks a single edge, with no width")}
                  onClick={() => setHints([...layer.hints, { horizontal: true, position: 0, width: 0, ghostTop: true }])}>{t("+ Ghost")}</button>
              </div>
              <p className="k-caption muted hints-note">{t("Manual hints replace the automatic ones for this glyph. A width of 0 makes a ghost hint.")}</p>
            </div>
          )}
        </div>
        <PixelPreviewPanel glyph={glyph} unicode={layer.unicode} master={master} version={version} active={active} />
        </aside>
      )}
      </div>
      <footer className="statusbar k-caption edit-status">
        <span>{t("Layer: {name}", { name: currentLayer ? (currentLayer.kind === "master" ? currentLayer.label : `${currentLayer.masterName} · ${currentLayer.label}`) : master })} · {t("{selected} of {total} nodes selected", { selected: selection.size, total: paths.reduce((n, p) => n + p.nodes.filter(isOn).length, 0) })}</span>
        <span className="spacer" />
        {cursor && <span className="k-code">{cursor.x}, {cursor.y}</span>}
        <button type="button" className="link-btn" onClick={fit} title={t("Fit glyph (Ctrl+0)")}>{t("Fit")}</button>
        <span className="zoom-control" role="group" aria-label={t("Zoom")}>
          <button type="button" className="icon-btn" aria-label={t("Zoom out")} title={t("Zoom out (Ctrl+−)")} onClick={() => { const m = measured.current; if (m) zoomAt(0.8, m.w / 2, m.h / 2); }}>−</button>
          <span className="k-code">{Math.round(view.s * 100)}%</span>
          <button type="button" className="icon-btn" aria-label={t("Zoom in")} title={t("Zoom in (Ctrl+=)")} onClick={() => { const m = measured.current; if (m) zoomAt(1.25, m.w / 2, m.h / 2); }}>+</button>
        </span>
      </footer>
      {picking && <ComponentPicker names={glyphs.map((g) => g.name)} exclude={glyph} onPick={addComponent} onCancel={() => setPicking(false)} />}
      {partPick && (() => {
        const prefix = partPick.kind === "corner" ? "_corner" : partPick.kind === "cap" ? "_cap" : "_segment";
        const names = glyphs.map((g) => g.name).filter((n) => n.startsWith(prefix));
        return <ComponentPicker names={names} exclude={glyph} onCancel={() => setPartPick(null)} onPick={(name) => {
          const pick = partPick;
          setPartPick(null);
          strokeCall(() => backend.addPathPart(glyph, masterId, { kind: pick.kind, glyph: name, path: pick.path, node: pick.node }));
        }} />;
      })()}
      {painting && layer?.paint && (
        <PaintDialog glyph={glyph} paint={layer.paint} palette={layer.palette ?? []} width={layer.width}
          height={layer.metrics.capHeight}
          masters={layerList.filter((l) => l.kind === "master").map((l) => ({ id: l.masterId, name: l.masterName }))}
          onLoadMaster={(mid) => backend.masterColorPaint(glyph, masterId, mid)}
          onCancel={() => setPainting(false)} onSave={savePaint} />
      )}
      {smartDialog && <SmartDialog glyph={glyph} mode={smartDialog} onCancel={() => setSmartDialog(null)} onSave={saveSmartDialog} />}
      {layerDialog && <LayerDialog glyph={glyph} axes={layerDialog.axes} mode={layerDialog.mode} onCancel={() => setLayerDialog(null)} onSave={saveLayerDialog} />}
    </div>
  );
}

const off = (p: Pt): EditNode => ({ x: p.x, y: p.y, kind: "offcurve", smooth: false });

/** The selected guide's name, editable in place; empty clears it. */
function GuideNameField({ name, onRename }: { name: string; onRename: (n: string) => void }) {
  const [text, setText] = useState(name);
  useEffect(() => setText(name), [name]);
  const commit = (raw: string) => {
    const s = raw.trim();
    if (s !== name) onRename(s);
    else setText(name);
  };
  return (
    <label className="info-field">
      <span className="k-caption muted">{t("Name")}</span>
      <input
        className="metric-input anchor-name-input"
        value={text}
        aria-label={t("Guide name")}
        placeholder={t("Unnamed")}
        spellCheck={false}
        onChange={(e) => setText(e.target.value)}
        onBlur={(e) => commit(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") (e.target as HTMLInputElement).blur();
          if (e.key === "Escape") { setText(name); (e.target as HTMLInputElement).value = name; (e.target as HTMLInputElement).blur(); }
        }}
      />
    </label>
  );
}

function selectionLabel(points: number, components: number, anchors: number): string {
  const parts: string[] = [];
  if (points) parts.push(tn(points, "{n} point", "{n} points"));
  if (components) parts.push(tn(components, "{n} component", "{n} components"));
  if (anchors) parts.push(tn(anchors, "{n} anchor", "{n} anchors"));
  if (parts.length > 1) return t("Selected: {list}", { list: parts.join(" · ") });
  if (points) return tn(points, "{n} point selected", "{n} points selected");
  if (components) return tn(components, "{n} component selected", "{n} components selected");
  if (anchors) return tn(anchors, "{n} anchor selected", "{n} anchors selected");
  return t("No selection");
}

/** The selected anchor's name, editable in place. */
function AnchorNameField({ name, onRename }: { name: string; onRename: (n: string) => void }) {
  const [text, setText] = useState(name);
  useEffect(() => setText(name), [name]);
  const commit = (raw: string) => {
    const s = raw.trim();
    if (s && s !== name) onRename(s);
    else setText(name);
  };
  return (
    <label className="info-field">
      <span className="k-caption muted">{t("Anchor")}</span>
      <input
        className="metric-input anchor-name-input"
        value={text}
        aria-label={t("Anchor name")}
        spellCheck={false}
        onChange={(e) => setText(e.target.value)}
        onBlur={(e) => commit(e.currentTarget.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") (e.target as HTMLInputElement).blur();
          if (e.key === "Escape") { setText(name); (e.target as HTMLInputElement).value = name; (e.target as HTMLInputElement).blur(); }
        }}
      />
    </label>
  );
}

/** Where a node is at time `t` on its interpolation curve (mirrors Rust `HoiKind::at`). */
function hoiAt(kind: HoiKind | null, p0: [number, number], p1: [number, number], t: number): [number, number] {
  const lerp = (a: number, b: number) => a + (b - a) * t;
  if (!kind) return [lerp(p0[0], p1[0]), lerp(p0[1], p1[1])];
  const m = 1 - t;
  if (kind.type === "through") {
    const c = [2 * kind.x - (p0[0] + p1[0]) / 2, 2 * kind.y - (p0[1] + p1[1]) / 2];
    return [m * m * p0[0] + 2 * m * t * c[0] + t * t * p1[0], m * m * p0[1] + 2 * m * t * c[1] + t * t * p1[1]];
  }
  if (kind.type === "handles") {
    const f = (a: number, b: number, c: number, d: number) => m * m * m * a + 3 * m * m * t * b + 3 * m * t * t * c + t * t * t * d;
    return [f(p0[0], kind.a[0], kind.b[0], p1[0]), f(p0[1], kind.a[1], kind.b[1], p1[1])];
  }
  const [cx, cy] = kind.center;
  const r0 = Math.hypot(p0[0] - cx, p0[1] - cy), r1 = Math.hypot(p1[0] - cx, p1[1] - cy);
  if (r0 < 1e-9 || r1 < 1e-9) return [lerp(p0[0], p1[0]), lerp(p0[1], p1[1])];
  const a0 = Math.atan2(p0[1] - cy, p0[0] - cx), a1 = Math.atan2(p1[1] - cy, p1[0] - cx);
  const tau = Math.PI * 2;
  let d = (((a1 - a0) % tau) + tau) % tau;
  if (kind.turn === "clockwise" || ((kind.turn ?? "auto") === "auto" && d > Math.PI)) d -= tau;
  const a = a0 + d * t, r = lerp(r0, r1);
  return [cx + r * Math.cos(a), cy + r * Math.sin(a)];
}

/** The active tool and its shortcut, as the toolbar shows it ("Select  V"). */
function toolName(tool: Tool, lasso = false): string {
  if (tool === "select" && lasso) return `${t("Lasso Select")}  Shift+V`;
  const tl = TOOLS.find((x) => x.id === tool);
  if (!tl) return "";
  const [name] = t(tl.label).split(/ \(/);
  return `${name}  ${tl.shift ? "Shift+" : ""}${tl.key.toUpperCase()}`;
}

function hint(tool: Tool, lasso = false): string {
  switch (tool) {
    case "select": return lasso
      ? t("Draw around nodes to select them · Shift adds, Ctrl removes · Alt+drag for a rectangle · drag to move · Shift+V switches")
      : t("Click or drag to select · Shift adds, Ctrl removes · Alt+drag for a lasso · drag to move · arrows nudge (Shift ×10) · Enter smooth/corner · Delete removes");
    case "pen": return t("Click for corners, drag for curves · click the first point to close · Esc ends the path");
    case "rect": return t("Drag to draw a rectangle · Shift for a square");
    case "pixel": return t("Click a grid cell to add or remove a pixel · drag to paint");
    case "annotate": return t("Pick a mark in the palette and click to place it · select to move or delete");
    case "hoi": return t("Drag a node's midpoint knob to curve its way to the next master · click a node to choose the curve type");
    case "ellipse": return t("Drag to draw an ellipse · Shift for a circle");
    case "knife": return t("Drag a line across the outline to cut paths · Esc cancels");
    case "measure": return t("Drag a line across the outline to measure distances and angles · Esc clears");
    case "hand": return t("Drag to pan · Ctrl+wheel zooms");
  }
}

/** The mark behind a node on a metric line (diamond), in an alignment zone (ring) or
 *  just outside one (warning triangle): shapes as well as colours tell them apart. */
function ZoneHighlight({ mark, at, scale }: { mark: Exclude<ZoneMark, null>; at: Pt; scale: number }) {
  const r = 8 * scale;
  const label = mark === "line" ? t("On a metric line") : mark === "zone" ? t("In an alignment zone") : t("Just outside an alignment zone or metric line");
  return (
    <g className={`zone-mark zone-${mark}`} pointerEvents="none">
      <title>{label}</title>
      {mark === "line" && <path d={`M${at.x} ${at.y - r}L${at.x + r} ${at.y}L${at.x} ${at.y + r}L${at.x - r} ${at.y}Z`} />}
      {mark === "zone" && <circle cx={at.x} cy={at.y} r={r} />}
      {mark === "near" && <>
        <path d={`M${at.x + r} ${at.y - r * 2.2}L${at.x + r * 2} ${at.y - r * 0.6}L${at.x} ${at.y - r * 0.6}Z`} />
        <path d={`M${at.x + r} ${at.y - r * 1.75}V${at.y - r * 1.15}`} className="zone-bang" />
      </>}
    </g>
  );
}

/** A five-pointed star around `at`: the super-smooth node marker. */
function starPath(at: Pt, outer: number, inner: number): string {
  const pts: string[] = [];
  for (let i = 0; i < 10; i++) {
    const r = i % 2 ? inner : outer, a = -Math.PI / 2 + (i * Math.PI) / 5;
    pts.push(`${(at.x + Math.cos(a) * r).toFixed(2)} ${(at.y + Math.sin(a) * r).toFixed(2)}`);
  }
  return `M${pts.join("L")}Z`;
}

function NodeMark({ n, at, scale, selected, first, dir, duplicate, dim }: { n: EditNode; at: Pt; scale: number; selected: boolean; first: boolean; dir?: number; duplicate?: boolean; dim?: boolean }) {
  const cls = selected ? " is-selected" : "";
  const mark = !isOn(n) ? <circle cx={at.x} cy={at.y} r={3 * scale} className={`node-off${cls}`} />
    : n.g2 ? <path d={starPath(at, 6.5 * scale, 2.9 * scale)} className={`node-smooth node-g2${cls}${first ? " is-first" : ""}`} />
    : n.smooth ? <circle cx={at.x} cy={at.y} r={4.5 * scale} className={`node-smooth${cls}${first ? " is-first" : ""}`} />
    : <rect x={at.x - 4 * scale} y={at.y - 4 * scale} width={8 * scale} height={8 * scale} className={`node-corner${cls}${first ? " is-first" : ""}`} />;
  return (
    <g opacity={dim ? 0.3 : undefined}>
      {mark}
      {dir !== undefined && (
        <path d="M0 -3.2L7 0L0 3.2Z" className="node-start-arrow"
          transform={`translate(${at.x + Math.cos(dir) * 11 * scale} ${at.y + Math.sin(dir) * 11 * scale}) rotate(${(dir * 180) / Math.PI})`} />
      )}
      {duplicate && <circle cx={at.x} cy={at.y} r={7.5 * scale} className="node-duplicate" />}
    </g>
  );
}
