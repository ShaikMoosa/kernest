// Typed access to the Rust backend (Tauri commands).
// When the UI runs in a plain browser (`npm run dev` without Tauri) an in-memory
// mock stands in, so screens can be previewed and tested without the desktop shell.

import { invoke } from "@tauri-apps/api/core";
import { clonePaths, pathsToSvg, pointBounds, rectPath, transformPaths, type EditPath } from "./outline";
import { translateMessage } from "./i18n";

export interface GlyphSetInfo {
  id: string;
  name: string;
  count: number;
}

export interface MasterInfo {
  id: string;
  name: string;
}

export interface Metrics {
  ascender: number;
  capHeight: number;
  xHeight: number;
  descender: number;
  italicAngle: number;
}

export interface DocStatus {
  dirty: boolean;
  canUndo: boolean;
  canRedo: boolean;
}

export interface GlyphSummary {
  name: string;
  unicode: string | null;
  character: string | null;
  category: string;
  export: boolean;
  width: number;
  lsb: number | null;
  rsb: number | null;
  /** SVG path data in font units, y up. Empty when the glyph has no outlines. */
  svg: string;
  /** False when this glyph's masters can't be interpolated. */
  compatible: boolean;
  /** A locked glyph refuses every edit and is skipped by filters. */
  locked?: boolean;
  /** Colour layers in this master, bottom to top (colour glyphs only). */
  color?: ColorLayerView[];
}

/** A colour stop on a gradient: position 0–1, palette index, opacity 0–1. */
export interface ColorStop {
  offset: number;
  color: number;
  alpha: number;
}

/** How a colour layer is filled. Coordinates are in font units. */
export type Paint =
  | { type: "solid"; color: number; alpha: number }
  | { type: "linear"; start: [number, number]; end: [number, number]; stops: ColorStop[] }
  | { type: "radial"; center: [number, number]; radius: number; focus?: [number, number] | null; stops: ColorStop[] };

/** A colour layer as drawn: its outline (SVG path data, font units, y up) and paint. */
export interface ColorLayerView {
  key: string;
  svg: string;
  paint: Paint;
}

export interface FontView {
  familyName: string;
  path: string | null;
  unitsPerEm: number;
  masters: MasterInfo[];
  masterId: string;
  metrics: Metrics;
  glyphs: GlyphSummary[];
  /** The default colour palette (CSS colours), for drawing colour glyphs. */
  palette?: string[];
  status: DocStatus;
}

/** A component reference: base glyph name and affine transform [xx, xy, yx, yy, dx, dy]. */
export interface ComponentEdit {
  base: string;
  transform: number[];
  /** Values on the base glyph's smart axes (smart components only). */
  smart?: Record<string, number>;
  /** Whether anchors place this component; false once it is moved by hand (default true). */
  align?: boolean;
}

/** An axis of a smart glyph, such as Width 0–100. */
export interface SmartAxis {
  name: string;
  min: number;
  max: number;
}

export interface ComponentView extends ComponentEdit {
  /** Whether anchors place this component (false once it is moved by hand). */
  align: boolean;
  /** This component's resolved outline, SVG path data in font units. */
  svg: string;
  /** The base glyph's smart axes, if it is a smart glyph. */
  smartAxes?: SmartAxis[];
}

/** A guide line through (x, y), at `angle` degrees from horizontal. */
export interface Guide {
  x: number;
  y: number;
  angle: number;
  name?: string;
  locked?: boolean;
}

/** The edit view's snap grid; a spacing or subdivision of 0 turns it off. */
export interface Grid {
  spacing: number;
  subdivision: number;
}

/** The reference drawing behind the outline, shaped like the foreground. */
export interface BackgroundView {
  paths: EditPath[];
  components: ComponentView[];
  componentSvg: string;
  anchors: { name: string; x: number; y: number }[];
}

/** Which parts of a layer an edit touches; all empty means the whole layer. */
export interface Selection {
  /** [path index, node index] pairs. */
  nodes: [number, number][];
  components: number[];
  anchors: string[];
}

export const EMPTY_SELECTION: Selection = { nodes: [], components: [], anchors: [] };

export type BackgroundEdit = "toBackground" | "addToBackground" | "swap" | "clear";

/** One crossing of the measure line with the outline; t is 0–1 along the line. */
export interface MeasureHit {
  x: number;
  y: number;
  t: number;
}

export interface Measurement {
  hits: MeasureHit[];
  /** Gaps between consecutive crossings, in font units. */
  distances: number[];
  /** The line's angle in degrees, and its length in font units. */
  angle: number;
  length: number;
}

export interface LayerView {
  glyph: string;
  unicode: string | null;
  masterId: string;
  metrics: Metrics;
  width: number;
  lsb: number | null;
  rsb: number | null;
  paths: EditPath[];
  components: ComponentView[];
  /** All components resolved to outlines, SVG path data in font units. */
  componentSvg: string;
  anchors: { name: string; x: number; y: number }[];
  /** Metrics keys such as "=n" (left/right sidebearing, width). */
  metricsKeys: { left?: string; right?: string; width?: string };
  /** This glyph's smart axes, if it is a smart glyph. */
  smartAxes?: SmartAxis[];
  /** Why this glyph's masters can't be interpolated (empty when they can). */
  issues: string[];
  /** True when the whole glyph is locked against edits. */
  glyphLocked: boolean;
  /** This master's colour layers, bottom to top, and the default palette. */
  colorLayers?: ColorLayerView[];
  palette?: string[];
  /** This layer's paint, when it is a colour layer. */
  paint?: Paint;
  /** Local guides of this layer. */
  guides: Guide[];
  /** Global guides of this layer's master, drawn in every glyph. */
  masterGuides: Guide[];
  /** The reference drawing behind the outline, if any. */
  background?: BackgroundView;
  /** The font's snap grid, if set. */
  grid?: Grid;
  status: DocStatus;
}

export type Location = Record<string, number>;

/** A bracket layer's range on one axis: min ≤ value ≤ max; null = no limit. */
export interface AxisRange {
  tag: string;
  min?: number | null;
  max?: number | null;
}

export interface LayerInfo {
  /** Pass as `masterId` to the layer commands to edit this layer. */
  key: string;
  kind: "master" | "brace" | "bracket" | "smart" | "color";
  label: string;
  /** A colour layer's paint. */
  paint?: Paint;
  masterId: string;
  masterName: string;
  location?: Location;
  bracket?: AxisRange[];
}

export interface Axis {
  name: string;
  tag: string;
  min: number;
  default: number;
  max: number;
  hidden?: boolean;
}

export interface MasterView {
  id: string;
  name: string;
  location: Location;
  metrics: Metrics;
}

export interface Instance {
  name: string;
  location: Location;
  export?: boolean;
  /** OS/2 weight class (100–900); unset means automatic from the location. */
  weightClass?: number;
  /** OS/2 width class (1–9); unset means automatic from the location. */
  widthClass?: number;
  /** Style linking: this instance is the bold/italic of the `base` instance. */
  styleLink?: StyleLink;
}

export interface StyleLink {
  bold: boolean;
  italic: boolean;
  /** The instance this one is the bold/italic of (empty for a regular family). */
  base: string;
}

/** The naming fields on Font Info › General. Empty strings clear a field. */
export interface FontNames {
  designer: string;
  designerUrl: string;
  manufacturer: string;
  manufacturerUrl: string;
  copyright: string;
  vendorId: string;
  license: string;
  licenseUrl: string;
  description: string;
  trademark: string;
  sampleText: string;
}

export const EMPTY_NAMES: FontNames = {
  designer: "", designerUrl: "", manufacturer: "", manufacturerUrl: "", copyright: "",
  vendorId: "", license: "", licenseUrl: "", description: "", trademark: "", sampleText: "",
};

/** TrueType autohinting settings (ttfautohint options). */
export interface Hinting {
  /** Smallest and largest text sizes, in pixels per em, that get hinting. */
  rangeMin: number;
  rangeMax: number;
  /** Up to this size the x-height is rounded up; 0 turns it off. */
  increaseXHeight: number;
  /** Hint composite glyphs as a whole. */
  hintComposites: boolean;
}

export const DEFAULT_HINTING: Hinting = { rangeMin: 8, rangeMax: 50, increaseXHeight: 14, hintComposites: false };

export interface FontInfo {
  familyName: string;
  unitsPerEm: number;
  versionMajor: number;
  versionMinor: number;
  names: FontNames;
  /** Designer notes, saved with the font but never exported. */
  notes: string;
  axes: Axis[];
  masters: MasterView[];
  instances: Instance[];
  /** TrueType autohinting settings. */
  hinting: Hinting;
  /** The edit view's snap grid, if set. */
  grid: Grid | null;
  /** Colour palettes (CSS colours #rrggbbaa); every palette has the same length. */
  palettes: string[][];
  defaultMasterId: string;
  /** Design-space problems that block interpolation or export. */
  warnings: string[];
  /** Number of glyphs whose masters can't be interpolated. */
  incompatible: number;
  status: DocStatus;
}

export interface UndoResult {
  kind: "layer" | "kerning" | "groups" | "font";
  glyph: string | null;
  masterId: string | null;
  status: DocStatus;
}

export type MetricName = "lsb" | "rsb" | "width";

/** Which sides of a pair are kerned as groups ("group") or as the glyph itself. */
export type KernMode = "group" | "leftException" | "rightException" | "exception";

export type TextItem =
  | {
      type: "glyph";
      name: string;
      unicode: string | null;
      /** Drawn from the default master because its masters can't be interpolated. */
      incompatible: boolean;
      width: number;
      lsb: number | null;
      rsb: number | null;
      leftGroup: string | null;
      rightGroup: string | null;
      /** Kerning with the next glyph on the line. */
      kern: number;
      kernKind: "group" | "exception" | null;
      kernLeft: string | null;
      kernRight: string | null;
      svg: string;
      /** Colour layers, bottom to top (colour glyphs only). */
      color?: ColorLayerView[];
    }
  | { type: "missing"; text: string };

export interface TextLayout {
  lines: TextItem[][];
  metrics: Metrics;
  /** The default colour palette, for drawing colour glyphs. */
  palette?: string[];
  status: DocStatus;
}

export interface KernPair {
  left: string;
  right: string;
  value: number;
}

export interface KerningList {
  pairs: KernPair[];
  groups: string[];
  status: DocStatus;
}

/** A problem in the hand-written feature text, at a 1-based line and column. */
export interface FeatureDiagnostic {
  line: number;
  column: number;
  message: string;
}

export interface FeaturesView {
  /** The designer's hand-written feature code. */
  text: string;
  /** The code generated from the automatic features that are switched on. */
  generated: string;
  automatic: string[];
  /** Every feature Kernest could generate from the glyph names. */
  available: string[];
  /** Features the designer wrote by hand (never generated). */
  handwritten: string[];
  diagnostics: FeatureDiagnostic[];
  status: DocStatus;
}

/** One shaped glyph in the text view's feature preview. */
export interface ShapedGlyph {
  name: string;
  svg: string;
  color?: ColorLayerView[];
  advance: number;
  xOffset: number;
  yOffset: number;
  /** Byte offset in the line's text, for caret placement. */
  cluster: number;
}

export interface ShapedView {
  lines: ShapedGlyph[][];
  metrics: Metrics;
  palette?: string[];
  status: DocStatus;
}

export interface ExportedFile {
  path: string;
  kind: "variable" | "static";
  format: "ttf" | "otf" | "woff2";
  style: string;
  bytes: number;
}

export interface ExportReport {
  files: ExportedFile[];
  /** Targets that could not be built, with the reason. */
  problems: string[];
}

export interface AddGlyphsResult {
  added: string[];
  /** Already in the font (same name or code point). */
  skipped: string[];
  status: DocStatus;
}

/** One anchor's move, in font units. */
export interface AnchorMove {
  name: string;
  dx: number;
  dy: number;
}

export interface DefaultAnchorsResult {
  /** How many of the named glyphs received anchors. */
  updated: number;
  status: DocStatus;
}

export interface CompositesResult {
  built: string[];
  /** No recipe, missing parts, or already drawn. */
  skipped: string[];
  status: DocStatus;
}

export interface ImportResult {
  view: FontView;
  /** What was left out or changed while converting. */
  warnings: string[];
  /** The file or package that was imported. */
  source: string;
}

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export interface Backend {
  glyphSets(): Promise<GlyphSetInfo[]>;
  newFont(familyName: string, glyphSets: string[]): Promise<FontView>;
  openSample(): Promise<FontView>;
  openFont(path: string): Promise<FontView>;
  saveFont(path?: string): Promise<string>;
  closeFont(): Promise<void>;
  fontView(masterId?: string): Promise<FontView>;
  glyphLayer(glyph: string, masterId: string): Promise<LayerView>;
  /** Replaces outlines and width; components are replaced too when given. */
  updateLayer(glyph: string, masterId: string, width: number, paths: EditPath[], components?: ComponentEdit[]): Promise<LayerView>;
  setMetric(glyph: string, masterId: string, metric: MetricName, value: number): Promise<LayerView>;
  undo(): Promise<UndoResult | null>;
  redo(): Promise<UndoResult | null>;
  /** Lays out text for a master, or at an interpolated `location` (axis tag → value). */
  textLayout(text: string, masterId: string, location?: Location): Promise<TextLayout>;
  /** Kerns two glyphs; `mode` decides whether their groups or the glyphs are kerned. 0 removes the pair. */
  setGlyphKerning(masterId: string, left: string, right: string, mode: KernMode, value: number): Promise<{ left: string; right: string; value: number; status: DocStatus }>;
  kerningList(masterId: string): Promise<KerningList>;
  /** Sets a pair by its sides (glyph names or @groups). 0 removes the pair. */
  setKerningPair(masterId: string, left: string, right: string, value: number): Promise<DocStatus>;
  setGlyphGroups(glyph: string, leftGroup: string | null, rightGroup: string | null): Promise<DocStatus>;
  fontInfo(): Promise<FontInfo>;
  setFontInfo(familyName: string, unitsPerEm: number, versionMajor: number, versionMinor: number): Promise<FontInfo>;
  /** Sets the naming fields (Font Info › General). */
  setFontNames(names: FontNames): Promise<FontInfo>;
  /** Sets the designer notes (saved, never exported). */
  setNotes(text: string): Promise<FontInfo>;
  /** The Features editor: hand-written text, generated code and diagnostics. */
  features(): Promise<FeaturesView>;
  setFeatures(text: string): Promise<FeaturesView>;
  setAutomaticFeatures(automatic: string[]): Promise<FeaturesView>;
  /** Shapes text with feature settings like "ss01=1" against a compiled preview. */
  shapeText(text: string, masterId: string, location?: Location, features?: string[]): Promise<ShapedView>;
  setAxes(axes: Axis[]): Promise<FontInfo>;
  /** Sets the TrueType autohinting settings. */
  setHinting(hinting: Hinting): Promise<FontInfo>;
  /** Sets (or clears, with null) the edit view's snap grid. */
  setGrid(grid: Grid | null): Promise<FontInfo>;
  /** Replaces a master's global guides, drawn in every glyph of that master. */
  setMasterGuides(masterId: string, guides: Guide[]): Promise<FontInfo>;
  /** Replaces a layer's local guides. */
  setGuides(glyph: string, masterId: string, guides: Guide[]): Promise<LayerView>;
  /** Moves guide `index` between the layer's local list and the master's global list. */
  setGuideScope(glyph: string, masterId: string, index: number, toGlobal: boolean): Promise<LayerView>;
  /** Applies affine [xx, xy, yx, yy, dx, dy] around `origin` to the selection (empty = whole layer). */
  transform(glyph: string, masterId: string, selection: Selection, affine: number[], origin: [number, number], allMasters: boolean, onBackground?: boolean): Promise<LayerView>;
  /** Moves the selection to / adds it to / swaps it with / clears the background. */
  backgroundEdit(glyph: string, masterId: string, edit: BackgroundEdit, selection: Selection): Promise<LayerView>;
  /** Replaces the background's outlines and anchors (drawing in background edit mode). */
  updateBackground(glyph: string, masterId: string, paths: EditPath[], components: ComponentEdit[], anchors: { name: string; x: number; y: number }[]): Promise<LayerView>;
  /** Where the segment p0–p1 crosses the outline, and the gaps between crossings. */
  measure(glyph: string, masterId: string, p0: [number, number], p1: [number, number], onBackground?: boolean): Promise<Measurement>;
  /** Sets the colour palettes (#rrggbb or #rrggbbaa). */
  setPalettes(palettes: string[][]): Promise<FontInfo>;
  /** Adds a colour layer on top of the glyph's others, in every master. */
  addColorLayer(glyph: string, masterId: string): Promise<{ key: string; status: DocStatus }>;
  /** Sets a colour layer's paint (in every master). */
  setColorPaint(glyph: string, key: string, paint: Paint): Promise<{ key: string; status: DocStatus }>;
  /** Moves a colour layer up or down the stack. */
  moveColorLayer(glyph: string, key: string, up: boolean): Promise<{ key: string; status: DocStatus }>;
  /** Adds a master copied from `copyFrom` (outlines, metrics, kerning). */
  addMaster(copyFrom: string, name: string, location: Location): Promise<FontInfo>;
  updateMaster(id: string, name: string, location: Location, metrics: Metrics): Promise<FontInfo>;
  removeMaster(id: string): Promise<FontInfo>;
  setInstances(instances: Instance[]): Promise<FontInfo>;
  compatibility(): Promise<{ glyph: string; issues: string[] }[]>;
  /** Compiles a variable font and/or static fonts (TrueType) into `outDir`. */
  exportFont(variable: boolean, statics: boolean, outDir: string, options?: ExportFormats): Promise<ExportReport>;
  /** Sets (or clears, with null) a metrics key such as "=n" and applies it. */
  setMetricsKey(glyph: string, masterId: string, metric: MetricName, key: string | null): Promise<LayerView>;
  /** Glyph › Update Metrics: applies every metrics key in the font. */
  updateMetrics(): Promise<{ problems: string[]; status: DocStatus }>;
  /** Opens a .glyphs, .glyphspackage, .ufo or .designspace source as a new, unsaved font. */
  importFont(path: string): Promise<ImportResult>;
  /** Adds glyphs typed as names/characters/code points plus whole glyph sets. */
  addGlyphs(text: string, glyphSets: string[]): Promise<AddGlyphsResult>;
  /** Removes glyphs; components using them are decomposed. */
  removeGlyphs(names: string[]): Promise<DocStatus>;
  /** Marks glyphs as exported or not (unexported glyphs are left out of fonts). */
  setGlyphExport(names: string[], exportGlyph: boolean): Promise<DocStatus>;
  /** Correct Path Direction or Add Extremes on one layer (Add Extremes: only `paths`, if given). */
  pathOperation(glyph: string, masterId: string, operation: PathOperation, paths?: number[]): Promise<LayerView>;
  /** Node-context-menu path edits; `warning` when only the current master could change. */
  pathEdit(glyph: string, masterId: string, operation: PathEditOperation, opts?: { paths?: number[]; nodes?: [number, number][]; points?: [number, number][] }): Promise<PathEditResult>;
  /** Locks or unlocks one path (index among the layer's paths, not components). */
  setPathLocked(glyph: string, masterId: string, path: number, locked: boolean): Promise<LayerView>;
  /** Locks or unlocks a whole glyph against edits. */
  setGlyphLocked(glyph: string, locked: boolean): Promise<DocStatus>;
  /** Runs a filter over the chosen glyphs (empty = the current one) as one undo step. */
  runFilter(current: string, masterId: string, glyphs: string[], name: FilterName, params: FilterParams, allMasters: boolean): Promise<FilterResult>;
  /** A filter's result on one layer without changing the document: the dialogs' live preview. */
  filterPreview(glyph: string, masterId: string, name: FilterName, params: FilterParams): Promise<LayerView>;
  /** Master, brace and bracket layers of a glyph. */
  glyphLayers(glyph: string): Promise<LayerInfo[]>;
  addBraceLayer(glyph: string, masterId: string, location: Location): Promise<{ key: string; status: DocStatus }>;
  addBracketLayers(glyph: string, masterId: string, ranges: AxisRange[]): Promise<{ key: string; status: DocStatus }>;
  updateLayerPosition(glyph: string, key: string, location?: Location, ranges?: AxisRange[]): Promise<{ key: string; status: DocStatus }>;
  removeLayer(glyph: string, key: string): Promise<DocStatus>;
  /** Gives a glyph smart axes (an empty list makes it ordinary again). */
  setSmartAxes(glyph: string, axes: SmartAxis[]): Promise<DocStatus>;
  /** Adds a smart pole layer at a position on the glyph's smart axes. */
  addSmartPole(glyph: string, masterId: string, pole: Record<string, number>): Promise<{ key: string; status: DocStatus }>;
  /** Sets a component's smart values (component `index` among the layer's components). */
  setComponentSmart(glyph: string, masterId: string, index: number, values: Record<string, number>): Promise<LayerView>;

  addAnchor(glyph: string, masterId: string, name: string, x: number, y: number, allMasters: boolean): Promise<LayerView>;
  moveAnchors(glyph: string, masterId: string, moves: AnchorMove[], allMasters: boolean): Promise<LayerView>;
  renameAnchor(glyph: string, masterId: string, oldName: string, newName: string, allMasters: boolean): Promise<LayerView>;
  removeAnchors(glyph: string, masterId: string, names: string[], allMasters: boolean): Promise<LayerView>;
  setDefaultAnchors(glyphs: string[]): Promise<DefaultAnchorsResult>;
  setComponentAlign(glyph: string, masterId: string, index: number, align: boolean): Promise<LayerView>;
  createComposites(names: string[]): Promise<CompositesResult>;
  /** Paths Kernest was started with (Explorer's "Open in Kernest", "Open with"); once. */
  takeLaunchPaths(): Promise<string[]>;
}

export type PathOperation = "correct-direction" | "add-extremes" | "remove-overlap";

export type PathEditOperation =
  | "knife"
  | "open-corner"
  | "reconnect-nodes"
  | "connect-nodes"
  | "close-open-paths"
  | "make-node-first"
  | "reverse-contours"
  | "tidy-up"
  | "round-coordinates";

export interface PathEditResult {
  layer: LayerView;
  /** Set when the edit could only be applied to the current master. */
  warning?: string;
}

export type FilterName = "round-corners" | "offset-curve" | "transformations";

/** A filter's settings; each dialog only sends its own fields. */
export interface FilterParams {
  radius?: number;
  offsetX?: number;
  offsetY?: number;
  makeStroke?: boolean;
  affine?: number[];
  origin?: [number, number];
}

export interface FilterResult {
  changed: string[];
  warnings?: string[];
  layer: LayerView;
}

export interface ExportFormats {
  ttf: boolean;
  otf: boolean;
  woff2: boolean;
  /** Merge overlapping contours in static fonts. */
  removeOverlaps: boolean;
  /** Apply metrics keys before building. */
  applyMetricsKeys: boolean;
  /** Add TrueType hinting to TTF and WOFF2 files. */
  autohint: boolean;
}

const tauriBackend: Backend = {
  glyphSets: () => invoke("glyph_sets"),
  newFont: (familyName, glyphSets) => invoke("new_font", { familyName, glyphSets }),
  openSample: () => invoke("open_sample"),
  openFont: (path) => invoke("open_font", { path }),
  saveFont: (path) => invoke("save_font", { path: path ?? null }),
  closeFont: () => invoke("close_font"),
  fontView: (masterId) => invoke("font_view", { masterId: masterId ?? null }),
  glyphLayer: (glyph, masterId) => invoke("glyph_layer", { glyph, masterId }),
  updateLayer: (glyph, masterId, width, paths, components) =>
    invoke("update_layer", { glyph, masterId, width, paths, components: components ?? null }),
  setMetric: (glyph, masterId, metric, value) => invoke("set_metric", { glyph, masterId, metric, value }),
  undo: () => invoke("undo"),
  redo: () => invoke("redo"),
  textLayout: (text, masterId, location) => invoke("text_layout", { text, masterId, location: location ?? null }),
  setGlyphKerning: (masterId, left, right, mode, value) => invoke("set_glyph_kerning", { masterId, left, right, mode, value }),
  kerningList: (masterId) => invoke("kerning_list", { masterId }),
  setKerningPair: (masterId, left, right, value) => invoke("set_kerning_pair", { masterId, left, right, value }),
  setGlyphGroups: (glyph, leftGroup, rightGroup) => invoke("set_glyph_groups", { glyph, leftGroup, rightGroup }),
  fontInfo: () => invoke("font_info"),
  setFontInfo: (familyName, unitsPerEm, versionMajor, versionMinor) => invoke("set_font_info", { familyName, unitsPerEm, versionMajor, versionMinor }),
  setFontNames: (names) => invoke("set_font_names", { names }),
  setNotes: (text) => invoke("set_notes", { text }),
  features: () => invoke("features"),
  setFeatures: (text) => invoke("set_features", { text }),
  setAutomaticFeatures: (automatic) => invoke("set_automatic_features", { automatic }),
  shapeText: (text, masterId, location, features) => invoke("shape_text", { text, masterId, location: location ?? null, features: features ?? [] }),
  setHinting: (hinting) => invoke("set_hinting", { hinting }),
  setGrid: (grid) => invoke("set_grid", { grid }),
  setMasterGuides: (masterId, guides) => invoke("set_master_guides", { masterId, guides }),
  setGuides: (glyph, masterId, guides) => invoke("set_guides", { glyph, masterId, guides }),
  setGuideScope: (glyph, masterId, index, toGlobal) => invoke("set_guide_scope", { glyph, masterId, index, toGlobal }),
  transform: (glyph, masterId, selection, affine, origin, allMasters, onBackground) =>
    invoke("transform", { glyph, masterId, selection, affine, origin, allMasters, onBackground: onBackground ?? false }),
  backgroundEdit: (glyph, masterId, edit, selection) => invoke("background_edit", { glyph, masterId, edit, selection }),
  updateBackground: (glyph, masterId, paths, components, anchors) => invoke("update_background", { glyph, masterId, paths, components, anchors }),
  measure: (glyph, masterId, p0, p1, onBackground) => invoke("measure", { glyph, masterId, p0, p1, onBackground: onBackground ?? false }),
  setPalettes: (palettes) => invoke("set_palettes", { palettes }),
  addColorLayer: (glyph, masterId) => invoke("add_color_layer", { glyph, masterId }),
  setColorPaint: (glyph, key, paint) => invoke("set_color_paint", { glyph, key, paint }),
  moveColorLayer: (glyph, key, up) => invoke("move_color_layer", { glyph, key, up }),
  setAxes: (axes) => invoke("set_axes", { axes }),
  addMaster: (copyFrom, name, location) => invoke("add_master", { copyFrom, name, location }),
  updateMaster: (id, name, location, metrics) => invoke("update_master", { id, name, location, metrics }),
  removeMaster: (id) => invoke("remove_master", { id }),
  setInstances: (instances) => invoke("set_instances", { instances }),
  compatibility: () => invoke("compatibility"),
  exportFont: (variable, statics, outDir, options) => invoke("export_font", { variable, statics, outDir, ...options }),
  setMetricsKey: (glyph, masterId, metric, key) => invoke("set_metrics_key", { glyph, masterId, metric, key }),
  updateMetrics: () => invoke("update_metrics"),
  importFont: (path) => invoke("import_font", { path }),
  addGlyphs: (text, glyphSets) => invoke("add_glyphs", { text, glyphSets }),
  removeGlyphs: (names) => invoke("remove_glyphs", { names }),
  setGlyphExport: (names, exportGlyph) => invoke("set_glyph_export", { names, export: exportGlyph }),
  pathOperation: (glyph, masterId, operation, paths) => invoke("path_operation", { glyph, masterId, operation, paths }),
  pathEdit: (glyph, masterId, operation, opts) => invoke("path_edit", { glyph, masterId, operation, paths: opts?.paths, nodes: opts?.nodes, points: opts?.points }),
  setPathLocked: (glyph, masterId, path, locked) => invoke("set_path_locked", { glyph, masterId, path, locked }),
  setGlyphLocked: (glyph, locked) => invoke("set_glyph_locked", { glyph, locked }),
  runFilter: (current, masterId, glyphs, name, params, allMasters) => invoke("run_filter", { current, masterId, glyphs, name, params, allMasters }),
  filterPreview: (glyph, masterId, name, params) => invoke("filter_preview", { glyph, masterId, name, params }),
  glyphLayers: (glyph) => invoke("glyph_layers", { glyph }),
  addBraceLayer: (glyph, masterId, location) => invoke("add_brace_layer", { glyph, masterId, location }),
  addBracketLayers: (glyph, masterId, ranges) => invoke("add_bracket_layers", { glyph, masterId, ranges }),
  updateLayerPosition: (glyph, key, location, ranges) => invoke("update_layer_position", { glyph, key, location, ranges }),
  removeLayer: (glyph, key) => invoke("remove_layer", { glyph, key }),
  takeLaunchPaths: () => invoke("take_launch_paths"),
  setSmartAxes: (glyph, axes) => invoke("set_smart_axes", { glyph, axes }),
  addSmartPole: (glyph, masterId, pole) => invoke("add_smart_pole", { glyph, masterId, pole }),
  setComponentSmart: (glyph, masterId, index, values) => invoke("set_component_smart", { glyph, masterId, index, values }),
  addAnchor: (glyph, masterId, name, x, y, allMasters) => invoke("add_anchor", { glyph, masterId, name, x, y, allMasters }),
  moveAnchors: (glyph, masterId, moves, allMasters) => invoke("move_anchors", { glyph, masterId, moves, allMasters }),
  renameAnchor: (glyph, masterId, oldName, newName, allMasters) => invoke("rename_anchor", { glyph, masterId, oldName, newName, allMasters }),
  removeAnchors: (glyph, masterId, names, allMasters) => invoke("remove_anchors", { glyph, masterId, names, allMasters }),
  setDefaultAnchors: (glyphs) => invoke("set_default_anchors", { glyphs }),
  setComponentAlign: (glyph, masterId, index, align) => invoke("set_component_align", { glyph, masterId, index, align }),
  createComposites: (names) => invoke("create_composites", { names }),
};

// ---------- Browser preview mock ----------

const ASCII_NAMES: Record<number, string> = {
  0x20: "space", 0x21: "exclam", 0x22: "quotedbl", 0x23: "numbersign", 0x24: "dollar", 0x25: "percent",
  0x26: "ampersand", 0x27: "quotesingle", 0x28: "parenleft", 0x29: "parenright", 0x2a: "asterisk", 0x2b: "plus",
  0x2c: "comma", 0x2d: "hyphen", 0x2e: "period", 0x2f: "slash", 0x30: "zero", 0x31: "one", 0x32: "two",
  0x33: "three", 0x34: "four", 0x35: "five", 0x36: "six", 0x37: "seven", 0x38: "eight", 0x39: "nine",
  0x3a: "colon", 0x3b: "semicolon", 0x3c: "less", 0x3d: "equal", 0x3e: "greater", 0x3f: "question", 0x40: "at",
  0x5b: "bracketleft", 0x5c: "backslash", 0x5d: "bracketright", 0x5e: "asciicircum", 0x5f: "underscore",
  0x60: "grave", 0x7b: "braceleft", 0x7c: "bar", 0x7d: "braceright", 0x7e: "asciitilde",
};

function category(ch: string): string {
  if (/\p{Lu}/u.test(ch)) return "Uppercase";
  if (/\p{Ll}/u.test(ch)) return "Lowercase";
  if (/\p{N}/u.test(ch)) return "Number";
  if (/\s/u.test(ch)) return "Separator";
  if (/[$+<=>^`|~]/.test(ch)) return "Symbol";
  if (/\p{P}/u.test(ch)) return "Punctuation";
  return "Symbol";
}

const MOCK_OUTLINES: Record<string, [number, EditPath[]]> = {
  H: [680, [rectPath(80, 0, 170, 700), rectPath(510, 0, 600, 700), rectPath(80, 310, 600, 390)]],
  I: [250, [rectPath(80, 0, 170, 700)]],
  L: [540, [rectPath(80, 0, 170, 700), rectPath(80, 0, 500, 80)]],
  T: [600, [rectPath(255, 0, 345, 620), rectPath(40, 620, 560, 700)]],
  n: [600, [rectPath(75, 0, 160, 500), rectPath(440, 0, 525, 380), rectPath(75, 380, 525, 500)]],
  o: [580, [rectPath(50, 0, 530, 500)]],
  l: [240, [rectPath(75, 0, 160, 760)]],
};

interface Snapshot {
  width: number;
  paths: EditPath[];
  components: ComponentEdit[];
}

interface MockGlyph extends Snapshot {
  name: string;
  unicode: string;
  character: string;
  category: string;
  leftGroup: string | null;
  rightGroup: string | null;
}

type MockChange =
  | { kind: "layer"; glyph: string; before: Snapshot; after: Snapshot }
  | { kind: "kerning"; before: KernPair[]; after: KernPair[] }
  | { kind: "groups"; glyph: string; before: [string | null, string | null]; after: [string | null, string | null] }
  | { kind: "font"; before: MockMeta; after: MockMeta };

/** Font-level data the preview mock can edit. (The mock shares one drawing across masters.) */
interface MockMeta {
  familyName: string;
  unitsPerEm: number;
  versionMajor: number;
  versionMinor: number;
  names: FontNames;
  notes: string;
  axes: Axis[];
  masters: MasterView[];
  instances: Instance[];
  hinting?: Hinting;
  grid?: Grid | null;
}

interface MockDoc extends MockMeta {
  glyphs: MockGlyph[];
  kerning: KernPair[];
  undo: MockChange[];
  redo: MockChange[];
  revision: number;
  savedRevision: number;
}

const METRICS: Metrics = { ascender: 800, capHeight: 700, xHeight: 500, descender: -200, italicAngle: 0 };
let mock: MockDoc | null = null;

const snap = (s: Snapshot): Snapshot => ({ width: s.width, paths: clonePaths(s.paths), components: s.components.map((c) => ({ base: c.base, transform: [...c.transform] })) });

function mockDoc(familyName: string, codepoints: number[], withOutlines: boolean): MockDoc {
  const glyphs = codepoints.map((cp): MockGlyph => {
    const ch = String.fromCodePoint(cp);
    const name = ASCII_NAMES[cp] ?? (/[A-Za-z]/.test(ch) ? ch : `uni${cp.toString(16).toUpperCase().padStart(4, "0")}`);
    const drawn = withOutlines ? MOCK_OUTLINES[name] : undefined;
    return {
      name,
      unicode: cp.toString(16).toUpperCase().padStart(4, "0"),
      character: ch,
      category: category(ch),
      width: drawn?.[0] ?? (cp === 0x20 ? 250 : 600),
      paths: drawn ? clonePaths(drawn[1]) : [],
      components: [],
      leftGroup: null,
      rightGroup: null,
    };
  });
  const kerning = withOutlines ? [{ left: "T", right: "o", value: -60 }, { left: "L", right: "T", value: -80 }] : [];
  return {
    familyName, unitsPerEm: 1000, versionMajor: 1, versionMinor: 0, names: { ...EMPTY_NAMES }, notes: "", axes: [], instances: [],
    masters: [{ id: "m01", name: "Regular", location: {}, metrics: { ...METRICS } }],
    glyphs, kerning, undo: [], redo: [], revision: 0, savedRevision: 0,
  };
}

const need = () => {
  if (!mock) throw "No font is open.";
  return mock;
};
const status = (d: MockDoc): DocStatus => ({ dirty: d.revision !== d.savedRevision, canUndo: d.undo.length > 0, canRedo: d.redo.length > 0 });
const findGlyph = (d: MockDoc, name: string) => {
  const g = d.glyphs.find((x) => x.name === name);
  if (!g) throw `No glyph named "${name}"`;
  return g;
};

/** Outlines of a component, resolved recursively (depth-limited like the Rust core). */
function resolveComponent(d: MockDoc, c: ComponentEdit, depth = 0): EditPath[] {
  const base = d.glyphs.find((g) => g.name === c.base);
  if (!base || depth > 16) return [];
  return transformPaths([...base.paths, ...base.components.flatMap((k) => resolveComponent(d, k, depth + 1))], c.transform);
}
const resolved = (d: MockDoc, g: MockGlyph) => [...g.paths, ...g.components.flatMap((c) => resolveComponent(d, c))];
const sb = (d: MockDoc, g: MockGlyph) => {
  const b = pointBounds(resolved(d, g));
  return b ? { lsb: b.minX, rsb: g.width - b.maxX } : { lsb: null, rsb: null };
};

function mockFontView(d: MockDoc, masterId?: string): FontView {
  const master = d.masters.find((m) => m.id === masterId) ?? d.masters[0];
  return {
    familyName: d.familyName,
    path: null,
    unitsPerEm: d.unitsPerEm,
    masters: d.masters.map((m) => ({ id: m.id, name: m.name })),
    masterId: master.id,
    metrics: master.metrics,
    status: status(d),
    glyphs: d.glyphs.map((g) => ({ name: g.name, unicode: g.unicode, character: g.character, category: g.category, export: true, width: g.width, ...sb(d, g), svg: pathsToSvg(resolved(d, g)), compatible: true })),
  };
}

function mockLayer(d: MockDoc, g: MockGlyph): LayerView {
  const components = g.components.map((c) => ({ base: c.base, transform: [...c.transform], align: true, svg: pathsToSvg(resolveComponent(d, c)) }));
  return {
    glyph: g.name, unicode: g.unicode, masterId: "m01", metrics: METRICS, width: g.width, ...sb(d, g),
    paths: clonePaths(g.paths), components, componentSvg: components.map((c) => c.svg).join(""), anchors: [], metricsKeys: {}, issues: [], glyphLocked: false, guides: [], masterGuides: [], status: status(d),
  };
}

function wouldCycle(d: MockDoc, glyph: string, base: string): boolean {
  const stack = [base];
  const seen = new Set<string>();
  while (stack.length) {
    const n = stack.pop()!;
    if (n === glyph) return true;
    if (seen.has(n)) continue;
    seen.add(n);
    d.glyphs.find((g) => g.name === n)?.components.forEach((c) => stack.push(c.base));
  }
  return false;
}

function record(d: MockDoc, c: MockChange) {
  d.undo.push(c);
  d.redo = [];
  d.revision++;
}

function mockApply(d: MockDoc, g: MockGlyph, next: Snapshot) {
  const before = snap(g);
  if (JSON.stringify(before) === JSON.stringify(snap(next))) return;
  Object.assign(g, snap(next));
  record(d, { kind: "layer", glyph: g.name, before, after: snap(next) });
}

// Kerning lookup mirrors kernest_core::kerning.
const leftKey = (d: MockDoc, name: string, group: boolean) => {
  const g = d.glyphs.find((x) => x.name === name)?.rightGroup;
  return group && g ? `@${g}` : name;
};
const rightKey = (d: MockDoc, name: string, group: boolean) => {
  const g = d.glyphs.find((x) => x.name === name)?.leftGroup;
  return group && g ? `@${g}` : name;
};
function pairValue(d: MockDoc, l: string, r: string) {
  const lg = leftKey(d, l, true), rg = rightKey(d, r, true);
  for (const [a, b] of [[l, r], [l, rg], [lg, r], [lg, rg]]) {
    const p = d.kerning.find((k) => k.left === a && k.right === b);
    if (p) return { value: p.value, kind: (a.startsWith("@") && b.startsWith("@") ? "group" : "exception") as "group" | "exception", left: a, right: b };
  }
  return null;
}
function setPair(d: MockDoc, left: string, right: string, value: number) {
  const before = d.kerning.map((p) => ({ ...p }));
  const v = Math.round(value);
  const i = d.kerning.findIndex((p) => p.left === left && p.right === right);
  const after = before.map((p) => ({ ...p }));
  if (i >= 0 && v === 0) after.splice(i, 1);
  else if (i >= 0) after[i].value = v;
  else if (v !== 0) after.push({ left, right, value: v });
  if (JSON.stringify(before) === JSON.stringify(after)) return;
  d.kerning = after;
  record(d, { kind: "kerning", before, after });
}

function parseText(d: MockDoc, text: string): ({ name: string } | { missing: string })[][] {
  const byChar = new Map(d.glyphs.map((g) => [g.character, g.name]));
  return text.split("\n").map((line) => {
    const out: ({ name: string } | { missing: string })[] = [];
    const chars = [...line.replace(/\r$/, "")];
    for (let i = 0; i < chars.length; i++) {
      if (chars[i] === "/" && /[\p{L}\p{N}._-]/u.test(chars[i + 1] ?? "")) {
        let j = i + 1;
        while (j < chars.length && /[\p{L}\p{N}._-]/u.test(chars[j])) j++;
        const name = chars.slice(i + 1, j).join("");
        out.push(d.glyphs.some((g) => g.name === name) ? { name } : { missing: `/${name}` });
        i = chars[j] === " " ? j : j - 1;
        continue;
      }
      const name = byChar.get(chars[i]);
      out.push(name ? { name } : { missing: chars[i] });
    }
    return out;
  });
}

function undoStep(d: MockDoc, c: MockChange, forward: boolean): UndoResult {
  if (c.kind === "layer") Object.assign(findGlyph(d, c.glyph), snap(forward ? c.after : c.before));
  else if (c.kind === "kerning") d.kerning = (forward ? c.after : c.before).map((p) => ({ ...p }));
  else if (c.kind === "font") Object.assign(d, metaCopy(forward ? c.after : c.before));
  else {
    const g = findGlyph(d, c.glyph);
    [g.leftGroup, g.rightGroup] = forward ? c.after : c.before;
  }
  d.revision++;
  const glyph = c.kind === "layer" || c.kind === "groups" ? c.glyph : null;
  return { kind: c.kind, glyph, masterId: c.kind === "layer" || c.kind === "kerning" ? "m01" : null, status: status(d) };
}

const metaCopy = (m: MockMeta): MockMeta => JSON.parse(JSON.stringify({
  familyName: m.familyName, unitsPerEm: m.unitsPerEm, versionMajor: m.versionMajor, versionMinor: m.versionMinor,
  names: m.names, notes: m.notes, axes: m.axes, masters: m.masters, instances: m.instances, hinting: m.hinting, grid: m.grid,
}));

function mockInfo(d: MockDoc): FontInfo {
  const def = d.masters.find((m) => d.axes.every((a) => (m.location[a.tag] ?? a.default) === a.default)) ?? d.masters[0];
  const warnings: string[] = [];
  if (d.masters.length > 1 && d.axes.length === 0) warnings.push("There are several masters but no axes. Add an axis in Font Info › Axes.");
  if (d.axes.length && !d.masters.some((m) => d.axes.every((a) => (m.location[a.tag] ?? a.default) === a.default)))
    warnings.push("No master sits at the default location of every axis; interpolation needs one.");
  return { ...metaCopy(d), hinting: d.hinting ?? DEFAULT_HINTING, grid: d.grid ?? null, palettes: [], defaultMasterId: def.id, warnings, incompatible: 0, status: status(d) };
}

/** Applies a font-level edit as one undoable step (mirrors History::apply_font). */
function editMeta(d: MockDoc, change: (m: MockMeta) => void): FontInfo {
  const before = metaCopy(d);
  const after = metaCopy(d);
  change(after);
  if (JSON.stringify(before) !== JSON.stringify(after)) {
    Object.assign(d, after);
    record(d, { kind: "font", before, after });
  }
  return mockInfo(d);
}

function checkLocation(d: MockDoc, loc: Location): Location {
  const out: Location = {};
  for (const a of d.axes) {
    const v = loc[a.tag] ?? a.default;
    if (!Number.isFinite(v) || v < a.min || v > a.max) throw `${a.name} must be between ${a.min} and ${a.max}.`;
    out[a.tag] = v;
  }
  return out;
}

const range = (a: number, b: number) => Array.from({ length: b - a + 1 }, (_, i) => a + i);
const MOCK_SETS: Record<string, number[]> = { "latin-basic": range(0x20, 0x7e), "latin-1": range(0xa0, 0xff), cyrillic: range(0x410, 0x44f) };

const mockBackend: Backend = {
  async glyphSets() {
    return [
      { id: "latin-basic", name: "Latin Basic (ASCII)", count: 95 },
      { id: "latin-1", name: "Latin-1 Supplement", count: 96 },
      { id: "cyrillic", name: "Cyrillic Basic", count: 64 },
    ];
  },
  async newFont(familyName, sets) {
    if (!familyName.trim()) throw "Enter a family name.";
    mock = mockDoc(familyName.trim(), [...new Set(sets.flatMap((s) => MOCK_SETS[s] ?? []))], false);
    return mockFontView(mock);
  },
  async openSample() {
    mock = mockDoc("Kernest Sample", range(0x20, 0x7e), true);
    return mockFontView(mock);
  },
  async openFont() {
    throw "Opening files needs the desktop app (npm run tauri dev).";
  },
  async saveFont() {
    throw "Saving needs the desktop app (npm run tauri dev).";
  },
  async closeFont() {
    mock = null;
  },
  async fontView(masterId) {
    return mockFontView(need(), masterId);
  },
  async fontInfo() {
    return mockInfo(need());
  },
  async setFontInfo(familyName, unitsPerEm, versionMajor, versionMinor) {
    if (!familyName.trim()) throw "Enter a family name.";
    if (unitsPerEm < 16 || unitsPerEm > 16384) throw "Units per em must be between 16 and 16384.";
    return editMeta(need(), (m) => Object.assign(m, { familyName: familyName.trim(), unitsPerEm, versionMajor, versionMinor }));
  },
  async setFontNames(names) {
    const trim = Object.fromEntries(Object.entries(names).map(([k, v]) => [k, v.trim()])) as unknown as FontNames;
    return editMeta(need(), (m) => { m.names = trim; });
  },
  async setNotes(text) {
    return editMeta(need(), (m) => { m.notes = text; });
  },
  async features() {
    throw "The Features editor needs the desktop app (npm run tauri dev).";
  },
  async setFeatures() {
    throw "The Features editor needs the desktop app (npm run tauri dev).";
  },
  async setAutomaticFeatures() {
    throw "The Features editor needs the desktop app (npm run tauri dev).";
  },
  async shapeText() {
    throw "The feature preview needs the desktop app (npm run tauri dev).";
  },
  async setHinting(h) {
    const ok = (v: number) => Number.isInteger(v) && v >= 1 && v <= 1000;
    if (!ok(h.rangeMin) || !ok(h.rangeMax)) throw "Hinting sizes must be between 1 and 1000 pixels per em.";
    if (h.rangeMin > h.rangeMax) throw "The smallest hinted size can't be larger than the largest.";
    if (h.increaseXHeight !== 0 && !(ok(h.increaseXHeight) && h.increaseXHeight >= 6)) throw "Increase x-height must be 0 (off) or between 6 and 1000.";
    return editMeta(need(), (m) => { m.hinting = { ...h }; });
  },
  async setGrid(grid) {
    const whole = (v: number) => Number.isInteger(v) && v >= 0 && v <= 65535;
    if (grid && (!whole(grid.spacing) || !whole(grid.subdivision))) throw "Grid spacing and subdivision must be whole numbers.";
    return editMeta(need(), (m) => { m.grid = grid && grid.spacing > 0 && grid.subdivision > 0 ? { ...grid } : null; });
  },
  async setMasterGuides() {
    throw "Guides need the desktop app (npm run tauri dev).";
  },
  async setGuides() {
    throw "Guides need the desktop app (npm run tauri dev).";
  },
  async setGuideScope() {
    throw "Guides need the desktop app (npm run tauri dev).";
  },
  async transform() {
    throw "Transforms need the desktop app (npm run tauri dev).";
  },
  async backgroundEdit() {
    throw "The background layer needs the desktop app (npm run tauri dev).";
  },
  async updateBackground() {
    throw "The background layer needs the desktop app (npm run tauri dev).";
  },
  async measure() {
    throw "Measuring needs the desktop app (npm run tauri dev).";
  },
  async setAxes(axes) {
    const tags = new Set<string>();
    for (const a of axes) {
      if (!a.name.trim()) throw "Every axis needs a name.";
      if (a.tag.length !== 4) throw `Axis tag "${a.tag}" must be 4 ASCII characters, like wght.`;
      if (tags.has(a.tag)) throw `Axis tag "${a.tag}" is used twice.`;
      tags.add(a.tag);
      if (!(a.min <= a.default && a.default <= a.max)) throw `Axis "${a.name}" needs minimum ≤ default ≤ maximum.`;
    }
    return editMeta(need(), (m) => {
      const fix = (loc: Location) => {
        for (const k of Object.keys(loc)) if (!tags.has(k)) delete loc[k];
        for (const a of axes) loc[a.tag] ??= a.default;
      };
      m.masters.forEach((x) => fix(x.location));
      m.instances.forEach((x) => fix(x.location));
      m.axes = axes.map((a) => ({ ...a }));
    });
  },
  async addMaster(copyFrom, name, location) {
    const d = need();
    const src = d.masters.find((m) => m.id === copyFrom);
    if (!src) throw `No master with id "${copyFrom}".`;
    if (!name.trim()) throw "Enter a master name.";
    const loc = checkLocation(d, location);
    let n = 1;
    while (d.masters.some((m) => m.id === `m${String(n).padStart(2, "0")}`)) n++;
    return editMeta(d, (m) => m.masters.push({ id: `m${String(n).padStart(2, "0")}`, name: name.trim(), location: loc, metrics: { ...src.metrics } }));
  },
  async updateMaster(id, name, location, metrics) {
    const d = need();
    if (!name.trim()) throw "Enter a master name.";
    const loc = checkLocation(d, location);
    return editMeta(d, (m) => {
      const x = m.masters.find((k) => k.id === id);
      if (!x) throw `No master with id "${id}".`;
      Object.assign(x, { name: name.trim(), location: loc, metrics: { ...metrics } });
    });
  },
  async removeMaster(id) {
    const d = need();
    if (d.masters.length === 1) throw "A font needs at least one master.";
    return editMeta(d, (m) => { m.masters = m.masters.filter((k) => k.id !== id); });
  },
  async setInstances(instances) {
    const d = need();
    const names = new Set<string>();
    const clean = instances.map((i) => {
      const name = i.name.trim();
      if (!name) throw "Every instance needs a name.";
      if (names.has(name)) throw `Instance name "${name}" is used twice.`;
      names.add(name);
      return { name, location: checkLocation(d, i.location), export: i.export ?? true, weightClass: i.weightClass, widthClass: i.widthClass, styleLink: i.styleLink };
    });
    return editMeta(d, (m) => { m.instances = clean; });
  },
  async exportFont() {
    throw "Exporting fonts needs the desktop app (npm run tauri dev).";
  },
  async setMetricsKey() {
    throw "Metrics keys need the desktop app (npm run tauri dev).";
  },
  async updateMetrics() {
    throw "Metrics keys need the desktop app (npm run tauri dev).";
  },
  async importFont() {
    throw "Importing fonts needs the desktop app (npm run tauri dev).";
  },
  async addGlyphs() {
    throw "Adding glyphs needs the desktop app (npm run tauri dev).";
  },
  async removeGlyphs() {
    throw "Removing glyphs needs the desktop app (npm run tauri dev).";
  },
  async setGlyphExport() {
    throw "Changing export settings needs the desktop app (npm run tauri dev).";
  },
  async pathOperation() {
    throw "Path operations need the desktop app (npm run tauri dev).";
  },
  async pathEdit() {
    throw "Path operations need the desktop app (npm run tauri dev).";
  },
  async setPathLocked() {
    throw "Locking paths needs the desktop app (npm run tauri dev).";
  },
  async setGlyphLocked() {
    throw "Locking glyphs needs the desktop app (npm run tauri dev).";
  },
  async runFilter() {
    throw "Filters need the desktop app (npm run tauri dev).";
  },
  async filterPreview() {
    throw "Filters need the desktop app (npm run tauri dev).";
  },
  async glyphLayers(glyph) {
    const d = need();
    return d.masters.filter(() => findGlyph(d, glyph)).map((m) => ({ key: m.id, kind: "master" as const, label: m.name, masterId: m.id, masterName: m.name }));
  },
  async addBraceLayer() {
    throw "Intermediate layers need the desktop app (npm run tauri dev).";
  },
  async addBracketLayers() {
    throw "Alternate layers need the desktop app (npm run tauri dev).";
  },
  async updateLayerPosition() {
    throw "Intermediate and alternate layers need the desktop app (npm run tauri dev).";
  },
  async removeLayer() {
    throw "Intermediate and alternate layers need the desktop app (npm run tauri dev).";
  },
  async takeLaunchPaths() {
    return [];
  },
  async setPalettes() {
    throw "Colour fonts need the desktop app (npm run tauri dev).";
  },
  async addColorLayer() {
    throw "Colour fonts need the desktop app (npm run tauri dev).";
  },
  async setColorPaint() {
    throw "Colour fonts need the desktop app (npm run tauri dev).";
  },
  async moveColorLayer() {
    throw "Colour fonts need the desktop app (npm run tauri dev).";
  },
  async setSmartAxes() {
    throw "Smart components need the desktop app (npm run tauri dev).";
  },
  async addSmartPole() {
    throw "Smart components need the desktop app (npm run tauri dev).";
  },
  async setComponentSmart() {
    throw "Smart components need the desktop app (npm run tauri dev).";
  },
  async addAnchor() {
    throw "Anchors need the desktop app (npm run tauri dev).";
  },
  async moveAnchors() {
    throw "Anchors need the desktop app (npm run tauri dev).";
  },
  async renameAnchor() {
    throw "Anchors need the desktop app (npm run tauri dev).";
  },
  async removeAnchors() {
    throw "Anchors need the desktop app (npm run tauri dev).";
  },
  async setDefaultAnchors() {
    throw "Anchors need the desktop app (npm run tauri dev).";
  },
  async setComponentAlign() {
    throw "Anchors need the desktop app (npm run tauri dev).";
  },
  async createComposites() {
    throw "Building composites needs the desktop app (npm run tauri dev).";
  },
  async compatibility() {
    return [];
  },
  async glyphLayer(glyph) {
    const d = need();
    return mockLayer(d, findGlyph(d, glyph));
  },
  async updateLayer(glyph, _m, width, paths, components) {
    const d = need();
    const g = findGlyph(d, glyph);
    for (const c of components ?? []) {
      if (!d.glyphs.some((x) => x.name === c.base)) throw `There is no glyph named "${c.base}" to use as a component.`;
      if (wouldCycle(d, glyph, c.base)) throw `"${c.base}" can't be a component of "${glyph}": it would contain itself.`;
    }
    mockApply(d, g, { width, paths, components: components ?? g.components });
    return mockLayer(d, g);
  },
  async setMetric(glyph, _m, metric, value) {
    const d = need();
    const g = findGlyph(d, glyph);
    const b = pointBounds(resolved(d, g));
    if (metric === "width") mockApply(d, g, { ...g, width: value });
    else if (!b) throw `"${glyph}" has no outlines to set a sidebearing on`;
    else if (metric === "rsb") mockApply(d, g, { ...g, width: b.maxX + value });
    else {
      const dx = value - b.minX;
      mockApply(d, g, {
        width: g.width + dx,
        paths: transformPaths(g.paths, [1, 0, 0, 1, dx, 0]),
        components: g.components.map((c) => ({ base: c.base, transform: [c.transform[0], c.transform[1], c.transform[2], c.transform[3], c.transform[4] + dx, c.transform[5]] })),
      });
    }
    return mockLayer(d, g);
  },
  async undo() {
    const d = need();
    const c = d.undo.pop();
    if (!c) return null;
    d.redo.push(c);
    return undoStep(d, c, false);
  },
  async redo() {
    const d = need();
    const c = d.redo.pop();
    if (!c) return null;
    d.undo.push(c);
    return undoStep(d, c, true);
  },
  async textLayout(text) {
    const d = need();
    const lines = parseText(d, text).map((line) =>
      line.map((t, i): TextItem => {
        if ("missing" in t) return { type: "missing", text: t.missing };
        const g = findGlyph(d, t.name);
        const next = line[i + 1];
        const k = next && "name" in next ? pairValue(d, t.name, next.name) : null;
        return {
          type: "glyph", name: g.name, unicode: g.unicode, incompatible: false, width: g.width, ...sb(d, g), leftGroup: g.leftGroup, rightGroup: g.rightGroup,
          kern: k?.value ?? 0, kernKind: k?.kind ?? null, kernLeft: k?.left ?? null, kernRight: k?.right ?? null, svg: pathsToSvg(resolved(d, g)),
        };
      }),
    );
    return { lines, metrics: METRICS, status: status(d) };
  },
  async setGlyphKerning(_m, left, right, mode, value) {
    const d = need();
    findGlyph(d, left);
    findGlyph(d, right);
    const l = leftKey(d, left, mode === "group" || mode === "rightException");
    const r = rightKey(d, right, mode === "group" || mode === "leftException");
    setPair(d, l, r, value);
    return { left: l, right: r, value: Math.round(value), status: status(d) };
  },
  async kerningList() {
    const d = need();
    const groups = [...new Set(d.glyphs.flatMap((g) => [g.leftGroup, g.rightGroup]).filter((g): g is string => !!g))].sort();
    return { pairs: d.kerning.map((p) => ({ ...p })), groups, status: status(d) };
  },
  async setKerningPair(_m, left, right, value) {
    const d = need();
    for (const s of [left.trim(), right.trim()]) {
      if (!s) throw "A kerning pair needs a glyph or @group on both sides.";
      if (s.startsWith("@") ? s.length < 2 : !d.glyphs.some((g) => g.name === s)) throw `There is no glyph named "${s}".`;
    }
    setPair(d, left.trim(), right.trim(), value);
    return status(d);
  },
  async setGlyphGroups(glyph, leftGroup, rightGroup) {
    const d = need();
    const g = findGlyph(d, glyph);
    const clean = (s: string | null) => (s ? s.trim().replace(/^@/, "") || null : null);
    const after: [string | null, string | null] = [clean(leftGroup), clean(rightGroup)];
    if (after.some((s) => s && /\s/.test(s))) throw "Group names cannot contain spaces";
    const before: [string | null, string | null] = [g.leftGroup, g.rightGroup];
    if (before[0] !== after[0] || before[1] !== after[1]) {
      [g.leftGroup, g.rightGroup] = after;
      record(d, { kind: "groups", glyph, before, after });
    }
    return status(d);
  },
};

export const backend: Backend = isTauri ? tauriBackend : mockBackend;

/** Turns a rejected command (a string from Rust, or an Error) into a message. */
/** An error as text in the UI language. */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return translateMessage(e);
  if (e instanceof Error) return translateMessage(e.message);
  return String(e);
}
