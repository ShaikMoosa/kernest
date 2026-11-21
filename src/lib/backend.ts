// Typed access to the Rust backend (Tauri commands).
// When the UI runs in a plain browser (`npm run dev` without Tauri) an in-memory
// mock stands in, so screens can be previewed and tested without the desktop shell.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { type PathStroke, clonePaths, pathsToSvg, pointBounds, rectPath, transformPaths, type EditPath } from "./outline";
import { translateMessage } from "./i18n";
import { designBounds } from "./axisMap";

export interface GlyphSetInfo {
  id: string;
  name: string;
  count: number;
  /** Some of the set's characters, for the New Font preview. */
  sample: string;
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
  /** Every Unicode value, the first being `unicode`. */
  unicodes: string[];
  character: string | null;
  category: string;
  subcategory: string;
  script: string;
  export: boolean;
  width: number;
  lsb: number | null;
  rsb: number | null;
  leftGroup: string | null;
  rightGroup: string | null;
  tags: string[];
  /** The name this glyph exports under when production names are used. */
  productionName?: string;
  /** Writing direction set on the glyph; absent means automatic (from glyph data). */
  direction?: "LTR" | "RTL" | "BIDI";
  /** Vertical metrics in this master: advance, origin and sidebearings. */
  vertical?: VerticalMetricsView;
  /** SVG path data in font units, y up. Empty when the glyph has no outlines. */
  svg: string;
  /** False when this glyph's masters can't be interpolated. */
  compatible: boolean;
  /** Compatible masters that would morph badly (start points, direction, order, kinks). */
  interpolationWarnings?: boolean;
  /** A locked glyph refuses every edit and is skipped by filters. */
  locked?: boolean;
  /** Colour layers in this master, bottom to top (colour glyphs only). */
  color?: ColorLayerView[];
}

/** A glyph's vertical metrics in one master, for vertical typesetting. */
export interface VerticalMetricsView {
  advance: number;
  origin: number;
  tsb: number | null;
  bsb: number | null;
  /** True when the layer has its own vertical values (not the defaults). */
  own: boolean;
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
  /** Saved smart and list filters for the Font View sidebar. */
  filters: GlyphFilter[];
  status: DocStatus;
}

/** One smart-filter condition. Boolean rules match when the property equals `value`. */
export type FilterRule =
  | { type: "category"; value: string }
  | { type: "subcategory"; value: string }
  | { type: "case"; value: string }
  | { type: "script"; value: string }
  | { type: "hasComponents"; value: boolean }
  | { type: "hasAnchors"; value: boolean }
  | { type: "hasUnicode"; value: boolean }
  | { type: "compatible"; value: boolean }
  | { type: "interpolationWarnings"; value: boolean }
  | { type: "export"; value: boolean }
  | { type: "metricsKeysOutOfSync"; value: boolean }
  | { type: "colorGlyph"; value: boolean }
  | { type: "smartGlyph"; value: boolean }
  | { type: "nameContains"; value: string }
  | { type: "nameStarts"; value: string }
  | { type: "nameEnds"; value: string }
  | { type: "tagsContain"; value: string };

/** All/Any over rules and nested groups. An empty group matches every glyph. */
export interface FilterRuleGroup {
  all: boolean;
  rules: FilterRule[];
  groups: FilterRuleGroup[];
}

/** A saved Font View filter: smart filters use `rules`, list filters use `glyphs`. */
export interface GlyphFilter {
  name: string;
  kind: "smart" | "list";
  rules?: FilterRuleGroup;
  glyphs?: string[];
}

/** A component reference: base glyph name and affine transform [xx, xy, yx, yy, dx, dy]. */
export interface ComponentEdit {
  base: string;
  transform: number[];
  /** Values on the base glyph's smart axes (smart components only). */
  smart?: Record<string, number>;
  /** Smart handles to drag on the canvas (anchors named after smart axes). */
  handles?: SmartHandle[];
  /** Whether anchors place this component; false once it is moved by hand (default true). */
  align?: boolean;
}

/** A smart handle: where an axis' anchor sits now and at the axis' ends. */
export interface SmartHandle {
  axis: string;
  at: [number, number];
  atMin: [number, number];
  atMax: [number, number];
  min: number;
  max: number;
  value: number;
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
  /** The outlines the engine draws for stroked paths and paths with parts (missing in the browser preview). */
  strokeSvg?: string;
  /** Paths (counting paths only) drawn by the engine rather than filled by the editor. */
  enginePaths?: number[];
  /** The layer has masks: strokeSvg is the whole masked outline. */
  engineAll?: boolean;
  /** Review notes on the canvas. */
  annotations?: Annotation[];
  /** The placed image, if any. */
  image?: LayerImage | null;
  /** Drawing order of the shapes (paths and components numbered separately). */
  shapeOrder?: { kind: "path" | "component"; index: number }[];
  /** Corner, cap and segment components on this layer's paths. */
  parts?: PathPart[];
  anchors: { name: string; x: number; y: number }[];
  /** This layer's manual PostScript hints (empty means auto stems are used). */
  hints: Hint[];
  /** The automatic stem hints the master's stems produce for this layer. */
  autoHints: Hint[];
  /** Metrics keys such as "=n" (left/right sidebearing, width). */
  metricsKeys: { left?: string; right?: string; width?: string };
  /** This glyph's smart axes, if it is a smart glyph. */
  smartAxes?: SmartAxis[];
  /** Why this glyph's masters can't be interpolated (empty when they can). */
  issues: string[];
  /** Compatible masters that would morph badly, and whether Fix can repair them. */
  interpolationWarnings?: string[];
  canFixInterpolation?: boolean;
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
  kind: "master" | "brace" | "bracket" | "smart" | "color" | "backup";
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
  /** avar: user → design points, sorted by user value. Empty = user == design. */
  map?: [number, number][];
  /** STAT: the named values on this axis, in user coordinates. Empty = no names. */
  labels?: AxisValue[];
}

/** One STAT axis value label. Values are in user coordinates, as in the exported font. */
export interface AxisValue {
  name: string;
  value: number;
  /** Optional range of user values this label covers (a format 2 axis value). */
  min?: number | null;
  max?: number | null;
  /** Whether apps may leave this name out of a style name (e.g. "Regular"). */
  elidable?: boolean;
  /** A linked user value (a format 3 axis value), e.g. Bold linking to Regular. */
  linkedValue?: number | null;
}

/** A Glyphs custom parameter kept from the source; `value` is Glyphs syntax. */
export interface CustomParameter {
  name: string;
  value: string;
  disabled?: boolean;
}

/** A note or correction mark on a layer. */
/** How a node travels from one master to the next one up an axis (see Rust `hoi::HoiKind`). */
export type HoiKind =
  | { type: "through"; x: number; y: number }
  | { type: "handles"; a: [number, number]; b: [number, number] }
  | { type: "rotation"; center: [number, number]; turn?: "auto" | "clockwise" | "counterclockwise" };

/** One node's interpolation curve along one axis, kept on the lower master. */
export interface HoiCurve {
  axis: string;
  path: number;
  node: number;
  kind: HoiKind;
}

/** What the interpolation-curve tool shows for one master layer and axis. */
export interface HoiView {
  axes: string[];
  axis: string;
  upName?: string | null;
  downName?: string | null;
  /** Node positions per path in the next master up / down (null when not compatible). */
  up?: [number, number][][] | null;
  down?: [number, number][][] | null;
  curves: HoiCurve[];
}

export interface Annotation {
  kind: "text" | "arrow" | "circle" | "plus" | "minus";
  x: number;
  y: number;
  text?: string;
  /** Arrow direction in degrees (0 points right). */
  angle?: number;
  /** Text box width or circle diameter; 0 means the default. */
  size?: number;
}

/** An image placed on a layer, positioned by its bottom-left corner. */
export interface LayerImage {
  path: string;
  x: number;
  y: number;
  scaleX: number;
  scaleY: number;
  angle?: number;
  /** [x, y, width, height] in image points from the bottom-left corner. */
  crop?: [number, number, number, number] | null;
  locked?: boolean;
}

/** The picture of a layer image, for drawing. */
/** A glyph's sbix bitmap (kind "bitmap") or SVG drawing (kind "svg"), placed in font units. */
export interface GlyphPicture {
  kind: "bitmap" | "svg";
  /** Pixel size, for bitmaps (0 for the SVG). */
  ppem: number;
  path: string;
  x: number;
  y: number;
  width: number;
  height: number;
  /** The SVG's scale (1 for bitmaps). */
  scale: number;
  dataUrl: string | null;
  file: string;
}

export interface InstallReport {
  /** Styles now available in other apps. */
  installed: string[];
  report: ExportReport;
}

export interface ExportDestination {
  id: "user-fonts" | "adobe" | "web-preview";
  path: string;
  exists: boolean;
}

/** What Kerning › Compress or Clean Up changed. */
export interface KernTidy {
  moved: number;
  removed: number;
  kept: number;
}

export interface ImageData {
  dataUrl: string | null;
  width: number;
  height: number;
  file: string;
}

/** A corner, cap or segment component placed on a path node. */
export interface PathPart {
  kind: "corner" | "cap" | "segment";
  glyph: string;
  path: number;
  node: number;
  scale?: [number, number];
  align?: "out" | "in" | "middle" | "unaligned";
  fit?: boolean;
}

/** OpenType line spacing and underline values; a missing one is worked out on export. */
export interface VerticalMetrics {
  typoAscender?: number | null;
  typoDescender?: number | null;
  typoLineGap?: number | null;
  hheaAscender?: number | null;
  hheaDescender?: number | null;
  hheaLineGap?: number | null;
  winAscent?: number | null;
  winDescent?: number | null;
  underlineThickness?: number | null;
  underlinePosition?: number | null;
}

export interface MasterView {
  id: string;
  name: string;
  location: Location;
  metrics: Metrics;
  alignmentZones: Zone[];
  stems: Stem[];
  /** Missing in the browser preview. */
  vertical?: VerticalMetrics;
}

/** A current glyph whose incoming layer from another font can't interpolate. */
export interface ForeignGlyphIssue {
  glyph: string;
  issues: string[];
}

/** What adding one master from another font would do, shown before committing. */
export interface ForeignMasterPreview {
  masterId: string;
  name: string;
  location: Location;
  /** Glyphs the source master has a layer for. */
  glyphCount: number;
  /** Current glyphs that also exist in the source master. */
  matched: number;
  /** Current glyphs the source lacks; they become empty layers. */
  missing: string[];
  incompatible: ForeignGlyphIssue[];
}

/** The result of adding a master from another font: the new info and the empty-layer glyphs. */
export interface AddMasterFromResult {
  info: FontInfo;
  missing: string[];
}

/** One differing Font Info or metrics field; `field` is a stable label key. */
export interface FieldDiff {
  field: string;
  current: string;
  other: string;
}

/** A master present on one side only, or whose metrics differ. */
export interface MasterDiff {
  name: string;
  onlyIn?: "current" | "other";
  metrics: FieldDiff[];
}

/** A glyph present on one side only, or whose default-master outline/width differs. */
export interface GlyphDiff {
  name: string;
  onlyIn?: "current" | "other";
  paths?: [number, number];
  nodes?: [number, number];
  width?: [number, number];
}

/** A kerning pair with differing values, or present on one side only. */
export interface KernDiff {
  left: string;
  right: string;
  current?: number;
  other?: number;
}

/** S42 · The compare-fonts report window payload. */
export interface CompareView {
  fileName: string;
  familyName: string;
  info: FieldDiff[];
  masters: MasterDiff[];
  glyphs: GlyphDiff[];
  kerning: KernDiff[];
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
  /** Glyph swaps at export: each pair trades drawings ("Rename Glyphs"). */
  renameGlyphs?: [string, string][];
  /** Glyphs left out of this export ("Remove Glyphs"). */
  removeGlyphs?: string[];
  /** This instance's own family name; unset means the font's family name. */
  familyName?: string;
  /** Family names in other languages, by Glyphs language tag ("DEU", "JAN"…). */
  localizedFamilyNames?: Record<string, string>;
  /** Masters (by name) this instance is interpolated without ("Disable Masters"). */
  disabledMasters?: string[];
  /** Naming overrides (Glyphs naming parameters); omitted when none are set. */
  names?: NameOverrides;
}

/** An instance's naming overrides; unset fields use the names export derives. */
export interface NameOverrides {
  /** nameID 6 and the CFF FontName. */
  postscriptFontName?: string;
  /** nameID 4, the full font name. */
  fullName?: string;
  /** nameID 18, the Mac compatible full name. */
  compatibleFullName?: string;
  /** nameID 1, the family name for style linking. */
  styleMapFamilyName?: string;
  /** Output file name without extension. */
  fileName?: string;
  /** Name records set by hand ("Name Table Entry"). */
  records?: NameRecord[];
}

/** One name table record. */
export interface NameRecord {
  nameId: number;
  platformId: number;
  encodingId: number;
  languageId: number;
  value: string;
}

/** A record in Glyphs' "Name Table Entry" syntax, as short as its ids allow. */
export function nameRecordText(r: NameRecord): string {
  const ids = r.platformId === 3 && r.encodingId === 1 && r.languageId === 0x409 ? `${r.nameId}`
    : r.platformId === 1 && r.encodingId === 0 && r.languageId === 0 ? `${r.nameId} 1`
    : `${r.nameId} ${r.platformId} ${r.encodingId} ${r.languageId}`;
  return `${ids}; ${r.value}`;
}

/** A virtual master: a named location only glyphs with intermediate layers there use. */
export interface VirtualMaster {
  name: string;
  location: Location;
}

export interface NewFontSetup {
  familyName: string;
  glyphSets: string[];
  unitsPerEm?: number;
  metrics?: Metrics;
  axes?: Axis[];
  masters?: { name: string; location: Location }[];
  instances?: Instance[];
}

export interface StyleLink {
  bold: boolean;
  italic: boolean;
  /** The instance this one is the bold/italic of (empty for a regular family). */
  base: string;
}

/** One named variable font built from a subset of the family's axes and glyphs. */
export interface VariableFontSetting {
  name: string;
  fileName: string;
  /** Per-axis user-range restrictions; an absent bound uses the full axis range. Omitted when empty. */
  axes?: VariableAxisSubset[];
  /** Glyphs to include (empty = all); components are pulled in automatically. Omitted when empty. */
  glyphs?: string[];
  renameGlyphs?: [string, string][];
  removeGlyphs?: string[];
  /** Masters (by name) left out ("Disable Masters"); the axes shrink to the rest. */
  disabledMasters?: string[];
  /** The variable font's own family name; unset uses the font's. */
  familyName?: string;
  /** Name records set by hand for this variable font. */
  nameRecords?: NameRecord[];
}

export interface VariableAxisSubset {
  tag: string;
  min?: number | null;
  max?: number | null;
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

/** An alignment zone: `position` is the flat edge, `size` the overshoot
 * (positive = a top zone, negative = a bottom zone; the baseline zone is 0, negative). */
export interface Zone {
  position: number;
  size: number;
}

/** A standard stem width used for hinting. */
export interface Stem {
  name: string;
  horizontal: boolean;
  value: number;
}

/** A manual PostScript hint on a layer. Width 0 makes it a ghost hint;
 * `ghostTop` then picks the edge (top/right when true, bottom/left when false). */
export interface Hint {
  horizontal: boolean;
  position: number;
  width: number;
  ghostTop?: boolean;
}

/** PostScript blue zone controls (CFF Private DICT). */
export interface PsHinting {
  blueScale: number;
  blueShift: number;
  blueFuzz: number;
}

export const DEFAULT_PS_HINTING: PsHinting = { blueScale: 0.039625, blueShift: 7, blueFuzz: 1 };

/** One character in the pixel preview, at one size. Paths are SVG data in pixels, y down, baseline at 0. */
export interface PixelGlyph {
  ch: string;
  name: string;
  advance: number;
  hinted: string;
  unhinted: string;
}

export interface PixelSize {
  ppem: number;
  glyphs: PixelGlyph[];
}

export interface PixelPreview {
  unitsPerEm: number;
  sizes: PixelSize[];
  /** Why hinting could not be applied (empty when it could); hinted then shows unhinted. */
  problem: string;
  status: DocStatus;
}

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
  /** Extra variable fonts to export from this family, beyond the full one. */
  variableFonts: VariableFontSetting[];
  exportPresets: ExportPreset[];
  /** TrueType autohinting settings. */
  hinting: Hinting;
  /** TTFAutohint control instructions, applied to TrueType exports. */
  controlInstructions: string;
  /** PostScript blue zone controls. */
  psHinting: PsHinting;
  /** The edit view's snap grid, if set. */
  grid: Grid | null;
  /** Applications use the typo metrics for line spacing (OS/2 fsSelection bit 7). */
  useTypoMetrics: boolean;
  /** Source parameters Kernest keeps but doesn't use (written back to .glyphs). */
  customParameters: CustomParameter[];
  /** Virtual masters: named locations used only by glyphs with intermediate layers there. */
  virtualMasters: VirtualMaster[];
  /** Number values for feature tokens ($padding): by name, then by master id. */
  numbers: Record<string, Record<string, number>>;
  /** Colour palettes (CSS colours #rrggbbaa); every palette has the same length. */
  palettes: string[][];
  /** Per-master palette overrides keyed by master id; empty means none. */
  masterPalettes: Record<string, string[][]>;
  defaultMasterId: string;
  /** The working glyph order; export follows it (.notdef always first). */
  glyphOrder: string[];
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
      /** Resolved right to left by the bidi algorithm, for laying the line out. */
      rtl: boolean;
      /** Vertical advance and origin height (font units), for vertical text. */
      vAdvance: number;
      vOrigin: number;
      tsb: number | null;
      bsb: number | null;
      /** Kerning with the next glyph on the line (none between directions). */
      kern: number;
      kernKind: "group" | "exception" | null;
      kernLeft: string | null;
      kernRight: string | null;
      svg: string;
      /** Colour layers, bottom to top (colour glyphs only). */
      color?: ColorLayerView[];
    }
  | { type: "missing"; text: string; rtl: boolean };

/** A text tab's writing direction. */
export type TextDirection = "ltr" | "rtl" | "vertical";

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
  /** Glyphs in each group when it is the left side of a pair (their right group). */
  leftMembers?: Record<string, string[]>;
  /** Glyphs in each group when it is the right side of a pair (their left group). */
  rightMembers?: Record<string, string[]>;
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

/** What importing a .fea file would change, shown before applying. */
export interface FeatureImportPreview {
  added: string[];
  replaced: string[];
  diagnostics: FeatureDiagnostic[];
}

/** One shaped glyph in the text view's feature preview. */
export interface ShapedGlyph {
  name: string;
  svg: string;
  color?: ColorLayerView[];
  advance: number;
  /** Vertical advance (negative: down), for vertical text. */
  yAdvance: number;
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
  /** Things worth a look in fonts that were built. */
  warnings: string[];
}

export interface AddGlyphsResult {
  added: string[];
  /** Already in the font (same name or code point). */
  skipped: string[];
  status: DocStatus;
}

export interface RenameResult {
  status: DocStatus;
  /** Feature-code lines the rename rewrote, shown as a warning. */
  changedFeatureLines: string[];
}

export interface LanguageCoverage {
  language: string;
  present: number;
  total: number;
}

export interface MissingGlyph {
  name: string;
  unicode: string;
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
  newFont(setup: NewFontSetup): Promise<FontView>;
  openSample(): Promise<FontView>;
  openFont(path: string): Promise<FontView>;
  saveFont(path?: string): Promise<string>;
  closeFont(): Promise<void>;
  fontView(masterId?: string): Promise<FontView>;
  /** Lines up contour order, direction and start points with the default master. */
  fixInterpolation(glyph: string): Promise<{ changed: number; status: DocStatus }>;
  glyphLayer(glyph: string, masterId: string): Promise<LayerView>;
  /** Replaces outlines and width; components are replaced too when given. */
  updateLayer(glyph: string, masterId: string, width: number, paths: EditPath[], components?: ComponentEdit[]): Promise<LayerView>;
  setMetric(glyph: string, masterId: string, metric: MetricName, value: number): Promise<LayerView>;
  /** Replaces a layer's manual PostScript hints (empty restores the auto stems). */
  setGlyphHints(glyph: string, masterId: string, hints: Hint[]): Promise<LayerView>;
  undo(): Promise<UndoResult | null>;
  redo(): Promise<UndoResult | null>;
  /** Lays out text for a master, or at an interpolated `location` (axis tag → value). */
  textLayout(text: string, masterId: string, location?: Location, direction?: TextDirection): Promise<TextLayout>;
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
  previewFeatureImport(path: string): Promise<FeatureImportPreview>;
  importFeatures(path: string): Promise<FeaturesView>;
  /** Shapes text with feature settings like "ss01=1" against a compiled preview. */
  shapeText(text: string, masterId: string, location?: Location, features?: string[], direction?: TextDirection): Promise<ShapedView>;
  /** Hinted and unhinted outlines of text at pixel sizes, for the edit view's pixel preview. */
  pixelPreview(text: string, masterId: string, location: Location | undefined, sizes: number[]): Promise<PixelPreview>;
  setAxes(axes: Axis[]): Promise<FontInfo>;
  /** Sets the TrueType autohinting settings. */
  setHinting(hinting: Hinting): Promise<FontInfo>;
  /** Replaces a master's alignment zones (Font Info › Masters). */
  setAlignmentZones(masterId: string, zones: Zone[]): Promise<FontInfo>;
  /** Replaces a master's standard stems (Font Info › Masters). */
  setStems(masterId: string, stems: Stem[]): Promise<FontInfo>;
  /** Sets the TTFAutohint control instructions (empty clears them). */
  setControlInstructions(text: string): Promise<FontInfo>;
  /** Sets or clears (null restores defaults) the PostScript blue zone controls. */
  setPsHinting(ps: PsHinting | null): Promise<FontInfo>;
  /** Sets (or clears, with null) the edit view's snap grid. */
  setGrid(grid: Grid | null): Promise<FontInfo>;
  /** Replaces a master's global guides, drawn in every glyph of that master. */
  setMasterGuides(masterId: string, guides: Guide[]): Promise<FontInfo>;
  setVerticalMetrics(masterId: string, vertical: VerticalMetrics): Promise<FontInfo>;
  setUseTypoMetrics(on: boolean): Promise<FontInfo>;
  setCustomParameters(parameters: CustomParameter[]): Promise<FontInfo>;
  setVirtualMasters(virtualMasters: VirtualMaster[]): Promise<FontInfo>;
  /** Replaces the number values for feature tokens. */
  setNumbers(numbers: Record<string, Record<string, number>>): Promise<FontInfo>;
  /** What a feature token ($padding, ${expr}, $[predicate]) stands for. */
  describeFeatureToken(token: string): Promise<string>;
  /** Replaces a layer's local guides. */
  setGuides(glyph: string, masterId: string, guides: Guide[]): Promise<LayerView>;
  /** Moves guide `index` between the layer's local list and the master's global list. */
  setGuideScope(glyph: string, masterId: string, index: number, toGlobal: boolean): Promise<LayerView>;
  /** Applies affine [xx, xy, yx, yy, dx, dy] around `origin` to the selection (empty = whole layer). */
  transform(glyph: string, masterId: string, selection: Selection, affine: number[], origin: [number, number], allMasters: boolean, onBackground?: boolean): Promise<LayerView>;
  /** Aligns or distributes the selection; `to` replaces the selection's box on that axis. */
  alignSelection(glyph: string, masterId: string, selection: Selection, op: AlignOp, to?: number, origin?: number): Promise<LayerView>;
  /** Moves the selection to / adds it to / swaps it with / clears the background. */
  backgroundEdit(glyph: string, masterId: string, edit: BackgroundEdit, selection: Selection): Promise<LayerView>;
  /** Replaces the background's outlines and anchors (drawing in background edit mode). */
  updateBackground(glyph: string, masterId: string, paths: EditPath[], components: ComponentEdit[], anchors: { name: string; x: number; y: number }[]): Promise<LayerView>;
  /** Where the segment p0–p1 crosses the outline, and the gaps between crossings. */
  measure(glyph: string, masterId: string, p0: [number, number], p1: [number, number], onBackground?: boolean): Promise<Measurement>;
  /** Sets the colour palettes (#rrggbb or #rrggbbaa). */
  setPalettes(palettes: string[][]): Promise<FontInfo>;
  /** Sets or clears one master's palette override (null clears it). */
  setMasterPalettes(masterId: string, palettes: string[][] | null): Promise<FontInfo>;
  /** Adds a colour layer on top of the glyph's others, in every master. */
  addColorLayer(glyph: string, masterId: string): Promise<{ key: string; status: DocStatus }>;
  /** Sets a colour layer's paint (in every master). */
  setColorPaint(glyph: string, key: string, paint: Paint): Promise<{ key: string; status: DocStatus }>;
  /** Sets a colour layer's paint in one master only. */
  setMasterPaint(glyph: string, key: string, masterId: string, paint: Paint): Promise<{ key: string; status: DocStatus }>;
  /** Reads one master's paint and effective palette for a colour layer. */
  masterColorPaint(glyph: string, key: string, masterId: string): Promise<{ paint: Paint; palette: string[] }>;
  /** Moves a colour layer up or down the stack. */
  moveColorLayer(glyph: string, key: string, up: boolean): Promise<{ key: string; status: DocStatus }>;
  /** Adds a master copied from `copyFrom` (outlines, metrics, kerning). */
  addMaster(copyFrom: string, name: string, location: Location): Promise<FontInfo>;
  /** Reports compatibility of every master in another font against this one, before adding. */
  previewForeignMasters(path: string): Promise<ForeignMasterPreview[]>;
  /** Adds a master copied from another font's `masterId`, placed at `location` here. */
  addMasterFrom(path: string, masterId: string, name: string, location: Location): Promise<AddMasterFromResult>;
  /** S42 · Read-only report of how another font differs from the open one. */
  compareFonts(path: string): Promise<CompareView>;
  /** S43 · One interpolated master per instance, opened as a new unsaved document. */
  generateInstances(): Promise<FontView>;
  updateMaster(id: string, name: string, location: Location, metrics: Metrics): Promise<FontInfo>;
  removeMaster(id: string): Promise<FontInfo>;
  setInstances(instances: Instance[]): Promise<FontInfo>;
  /** Reads name table entries, one per line in Glyphs' syntax ("0; © 2026 Me"). */
  parseNameRecords(text: string): Promise<NameRecord[]>;
  /** Replaces the list of extra variable fonts built from this family. */
  setVariableFonts(variableFonts: VariableFontSetting[]): Promise<FontInfo>;
  setExportPresets(presets: ExportPreset[]): Promise<FontInfo>;
  exportPreset(name: string, outDir?: string): Promise<ExportReport>;
  checkFonts(paths: string[], profile?: "opentype" | "universal"): Promise<QualityReport>;
  saveQualityReport(path: string, report: QualityReport): Promise<string>;
  /** Reorders the working glyphs; export follows the new order. */
  setGlyphOrder(order: string[]): Promise<FontInfo>;
  compatibility(): Promise<{ glyph: string; issues: string[] }[]>;
  /** Compiles a variable font and/or static fonts (TrueType) into `outDir`. */
  exportFont(variable: boolean, statics: boolean, outDir: string, options?: ExportFormats, instances?: string[]): Promise<ExportReport>;
  /** Writes editable UFO sources and a designspace into `outDir`. */
  exportUfo(outDir: string, options?: UfoExportOptions): Promise<UfoExportReport>;
  /** Builds static TTFs and loads them for this Windows session only. */
  testInstall(instances?: string[]): Promise<InstallReport>;
  /** Unloads the Test Install fonts; returns how many were loaded. */
  testUninstall(): Promise<number>;
  /** Installs static TTFs into the per-user Fonts folder (no admin rights). */
  installForUser(instances?: string[]): Promise<InstallReport>;
  /** Quick export destinations: the per-user Fonts folder, Adobe's and the web preview's. */
  exportDestinations(): Promise<ExportDestination[]>;
  /** Exports WOFF2 into the web preview folder and returns the local page address. */
  webPreview(): Promise<string>;
  /** Sets (or clears, with null) a metrics key such as "=n" and applies it. */
  setMetricsKey(glyph: string, masterId: string, metric: MetricName, key: string | null): Promise<LayerView>;
  /** Glyph › Update Metrics: applies every metrics key in the font. */
  updateMetrics(): Promise<{ problems: string[]; status: DocStatus }>;
  /** Font View › Batch: applies a path operation to every selected glyph (one undo step). */
  batchPathOperation(glyphs: string[], masterId: string, operation: BatchOperation): Promise<{ status: DocStatus }>;
  /** Font View › Batch › Update Metrics: applies metrics keys only to the selected glyphs. */
  updateMetricsFor(glyphs: string[]): Promise<{ problems: string[]; status: DocStatus }>;
  /** The named glyphs with every master's layer, for the Kernest clipboard. */
  copyGlyphs(names: string[]): Promise<ClipGlyph[]>;
  /** Edit › Paste Special: same-named glyphs, the selection in order, or as components. */
  pasteSpecial(mode: PasteMode, targets: string[], glyphs: ClipGlyph[]): Promise<{ status: DocStatus }>;
  /** Opens a .glyphs, .glyphspackage, .ufo or .designspace source as a new, unsaved font. */
  importFont(path: string): Promise<ImportResult>;
  /** Writes the open font as a Glyphs 3 .glyphs source (a copy out). Returns the path written. */
  saveGlyphs(path: string): Promise<string>;
  /** Pastes SVG text into one glyph's layer as one undo step. */
  importSvgText(glyph: string, masterId: string, svg: string, placement: SvgPlacement): Promise<SvgTextResult>;
  /** Imports one SVG file per glyph, matched by file name, as one undo step. */
  importSvgFiles(paths: string[], masterId: string, placement: SvgPlacement): Promise<SvgFilesResult>;
  /** Reads a metrics source (AFM or another font) without changing the open font. */
  readMetricsSource(path: string): Promise<MetricsSourceInfo>;
  /** Imports widths/sidebearings/kerning from a source into one master, as one undo step. */
  importMetrics(path: string, masterId: string, sourceMasterId: string, keep: KeepSide, widths: boolean, kerning: boolean): Promise<MetricsResult>;
  /** Adds glyphs typed as names/characters/code points plus whole glyph sets. */
  addGlyphs(text: string, glyphSets: string[]): Promise<AddGlyphsResult>;
  /** Glyph › Duplicate Glyph: copies named a.001 after each glyph. */
  duplicateGlyphs(names: string[]): Promise<{ added: string[]; status: DocStatus }>;
  /** Removes glyphs; components using them are decomposed. */
  removeGlyphs(names: string[]): Promise<DocStatus>;
  /** Marks glyphs as exported or not (unexported glyphs are left out of fonts). */
  setGlyphExport(names: string[], exportGlyph: boolean): Promise<DocStatus>;
  /** Replaces one glyph's tags (trimmed, deduped). One undoable step. */
  setGlyphTags(glyph: string, tags: string[]): Promise<DocStatus>;
  setGlyphUnicodes(glyph: string, unicodes: string, take: boolean): Promise<DocStatus>;
  /** Sets a glyph's production name (empty clears it). One undoable step. */
  setProductionName(glyph: string, name: string): Promise<DocStatus>;
  setWritingDirection(glyph: string, direction: string): Promise<DocStatus>;
  setVerticalMetric(glyph: string, masterId: string, which: "advance" | "tsb" | "bsb" | "reset", value: number | null): Promise<DocStatus>;
  /** Replaces the saved smart and list filters. One undoable step. */
  setFilters(filters: GlyphFilter[]): Promise<DocStatus>;
  /** The glyph names a filter matches; read-only preview for the rule builder. */
  evaluateFilter(filter: GlyphFilter): Promise<string[]>;
  /** Renames glyphs from an old→new map and updates every reference. One undoable step. */
  renameGlyphs(renames: Record<string, string>): Promise<RenameResult>;
  /** Per language: how many of its code points the font encodes, out of how many. */
  languageCoverage(): Promise<LanguageCoverage[]>;
  /** The glyphs a language still needs; read-only, for the sidebar's Missing list. */
  missingGlyphs(language: string): Promise<MissingGlyph[]>;
  /** Adds every glyph a language is missing, as one undoable step. */
  addMissing(language: string): Promise<AddGlyphsResult>;
  /** Correct Path Direction or Add Extremes on one layer (Add Extremes: only `paths`, if given). */
  pathOperation(glyph: string, masterId: string, operation: PathOperation, paths?: number[]): Promise<LayerView>;
  /** Node-context-menu path edits; `warning` when only the current master could change. */
  pathEdit(glyph: string, masterId: string, operation: PathEditOperation, opts?: { paths?: number[]; nodes?: [number, number][]; points?: [number, number][] }): Promise<PathEditResult>;
  /** Locks or unlocks one path (index among the layer's paths, not components). */
  setPathLocked(glyph: string, masterId: string, path: number, locked: boolean): Promise<LayerView>;
  setPathStroke(glyph: string, masterId: string, path: number, stroke: PathStroke | null): Promise<LayerView>;
  setPenWidth(glyph: string, masterId: string, path: number, node: number, width: number | null): Promise<LayerView>;
  expandStrokes(glyph: string, masterId: string): Promise<LayerView>;
  addPixelGlyph(size: number): Promise<DocStatus>;
  addPathPart(glyph: string, masterId: string, part: PathPart): Promise<LayerView>;
  setPathMask(glyph: string, masterId: string, path: number, mask: boolean): Promise<LayerView>;
  hoiView(glyph: string, masterId: string, axis: string): Promise<HoiView>;
  setHoi(glyph: string, masterId: string, axis: string, path: number, node: number, kind: HoiKind | null): Promise<LayerView>;
  setAnnotations(glyph: string, masterId: string, annotations: Annotation[]): Promise<LayerView>;
  setLayerImage(glyph: string, masterId: string, image: LayerImage | null): Promise<LayerView>;
  addLayerImage(glyph: string, masterId: string, file: string): Promise<LayerView>;
  layerImageData(glyph: string, masterId: string): Promise<ImageData | null>;
  /** sbix bitmaps (smallest first) and the SVG drawing of a glyph. */
  glyphPictures(glyph: string): Promise<GlyphPicture[]>;
  addGlyphBitmap(glyph: string, file: string, ppem: number): Promise<GlyphPicture[]>;
  addGlyphSvg(glyph: string, file: string): Promise<GlyphPicture[]>;
  /** Moves the bitmap for ppem (0 = the SVG); scale resizes the SVG. */
  placeGlyphPicture(glyph: string, ppem: number, x: number, y: number, scale?: number): Promise<GlyphPicture[]>;
  removeGlyphPicture(glyph: string, ppem: number): Promise<GlyphPicture[]>;
  reorderShapes(glyph: string, masterId: string, order: number[], allLayers: boolean): Promise<LayerView>;
  setPathPart(glyph: string, masterId: string, index: number, part: PathPart | null): Promise<LayerView>;
  decomposeParts(glyph: string, masterId: string): Promise<LayerView>;
  setPixels(glyph: string, masterId: string, cells: [number, number][], on: boolean): Promise<LayerView>;
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
  addBackupLayer(glyph: string, masterId: string, name: string): Promise<{ key: string; status: DocStatus }>;
  useBackupLayer(glyph: string, key: string): Promise<DocStatus>;
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
  getSettings(): Promise<Settings>;
  setAppearance(appearance: Appearance): Promise<Settings>;
  setOnboarding(onboarding: Onboarding): Promise<Settings>;
  recordMilestone(milestone: Milestone): Promise<Settings>;
  setUpdateCheck(enabled: boolean): Promise<Settings>;
  /** force: check now (Help › Check for Updates…), even when automatic checks are off. */
  checkUpdates(force?: boolean): Promise<UpdateInfo>;
  /** Downloads the setup program for `version`, checks its checksum, runs it and quits. */
  installUpdate(version: string): Promise<void>;
  /** Kerning › Compress (compress) or Clean Up, in one master or all, as one undo step. */
  tidyKerning(masterId: string, allMasters: boolean, compress: boolean): Promise<{ tidy: KernTidy; status: DocStatus }>;
  /** Kerning › Paste: adds pairs; overwrite replaces values already there. */
  pasteKerning(masterId: string, pairs: KernPair[], overwrite: boolean): Promise<DocStatus>;
  setPluginEnabled(name: string, enabled: boolean): Promise<Settings>;
  setShortcut(action: string, shortcut: string): Promise<Settings>;
  resetShortcuts(): Promise<Settings>;
  setPythonPath(path: string | null): Promise<Settings>;
  /** Scripts in the Scripts folder (folders become submenus). */
  listScripts(): Promise<ScriptEntry[]>;
  /** Plug-ins in the Plugins folder, with their enabled state. */
  listPlugins(): Promise<PluginInfo[]>;
  /** Runs a macro (`code`) or a script file (`path`) as one undo step. */
  runScript(opts: RunScriptOptions): Promise<ScriptResult>;
  cancelScript(): Promise<void>;
  /** Runs a filter plug-in over the chosen glyphs as one undo step. */
  runFilterPlugin(opts: RunFilterPluginOptions): Promise<ScriptResult>;
  /** A reporter's drawing instructions for one glyph, without changing the font. */
  reporterDraw(name: string, glyph: string, masterId: string): Promise<DrawItem[]>;
  openScriptsFolder(): Promise<void>;
  openPluginsFolder(): Promise<void>;
  /** Subscribes to streamed script output; resolves to an unsubscribe function. */
  onScriptOutput(cb: (line: OutputLine) => void): Promise<() => void>;
  /** The Python Kernest would run, for Settings › Python. */
  pythonInfo(): Promise<PythonInfo>;
}

export type PathOperation = "correct-direction" | "add-extremes" | "remove-overlap";
export type BatchOperation =
  | "correct-direction"
  | "remove-overlap"
  | "tidy-up"
  | "round-coordinates"
  | "decompose";
export type PasteMode = "same-name" | "selection" | "components";
/** A glyph on the Kernest clipboard, as the engine serialises it (all masters' layers). */
export interface ClipGlyph {
  name: string;
  unicodes?: number[];
  layers: unknown[];
  [key: string]: unknown;
}

export type AlignOp = "left" | "h-center" | "right" | "top" | "v-center" | "bottom" | "distribute-h" | "distribute-v" | "points";

export type PathEditOperation =
  | "knife"
  | "open-corner"
  | "reconnect-nodes"
  | "connect-nodes"
  | "close-open-paths"
  | "make-node-first"
  | "reverse-contours"
  | "tidy-up"
  | "round-coordinates"
  | "harmonize"
  | "super-smooth";

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

/** How imported SVG outlines are placed in font units (S23). */
export type SvgPlacement = "fit-cap-height" | "keep-size" | "keep-as-is";

export interface SvgTextResult {
  warnings: string[];
  status: DocStatus;
}

export interface SvgFilesResult {
  changed: string[];
  warnings: string[];
  status: DocStatus;
}

/** Which sidebearing stays fixed when an imported width is applied (S24). */
export type KeepSide = "left" | "right";

export interface MetricsSourceInfo {
  kind: "afm" | "font";
  masters: { id: string; name: string }[];
  glyphs: number;
  kerningPairs: number;
}

export interface MetricsResult {
  changed: string[];
  warnings: string[];
  status: DocStatus;
}

export type CheckSeverity = "PASS" | "WARN" | "FAIL" | "SKIP" | "INFO" | "ERROR";
export interface QualityCheck {
  id: string;
  title: string;
  status: CheckSeverity;
  file: string | null;
  glyphs: string[];
  messages: { code: string | null; message: string }[];
}
export interface QualityReport {
  profile: string;
  files: number;
  checks: QualityCheck[];
  failed: number;
  warnings: number;
  passed: number;
  skipped: number;
  errors: number;
}

export interface ExportPreset {
  name: string;
  formats: ("ttf" | "otf" | "woff2")[];
  instances: string[];
  options: { variable: boolean; statics: boolean; removeOverlaps: boolean; applyMetricsKeys: boolean; autohint: boolean; productionNames: boolean };
  folder: string;
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
  /** Name glyphs in the compiled fonts by their production names. */
  productionNames: boolean;
}

/** Choices for exporting editable UFO sources (File › Export › UFO…). */
export interface UfoExportOptions {
  /** Master ids to include; empty means every master. */
  masters: string[];
  /** Flatten smart components to outlines instead of keeping them as components. */
  decomposeSmart: boolean;
  /** Write colour layers as named UFO layers. */
  colorLayers: boolean;
}

export interface UfoExportReport {
  /** The UFO folders and designspace written. */
  files: string[];
  /** Anything the UFO format can't carry. */
  problems: string[];
}

// ---------- Scripts, plug-ins and settings (phase 6) ----------

export type Milestone = "created" | "drawn" | "spaced" | "kerned" | "instance" | "exported";
export type CanvasElement = "cornerNode" | "smoothNode" | "zones" | "stroke" | "background" | "otherLayers" | "canvas" | "kerningNegative" | "kerningPositive";
export interface Appearance {
  theme: "system" | "light" | "dark" | null;
  language: "en" | "es" | "ar" | null;
  handleSize: "small" | "medium" | "large";
  textWidth: number;
  /** Text tabs follow the direction of their text's first letter. */
  autoDirection: boolean;
  colors: { light: Partial<Record<CanvasElement, string>>; dark: Partial<Record<CanvasElement, string>> };
}
export interface Onboarding {
  welcomeDone: boolean;
  experience: "beginner" | "experienced" | null;
  tourEnabled: boolean;
  tourStep: number;
  completed: Milestone[];
}
export interface UpdateInfo {
  currentVersion: string;
  latestVersion: string | null;
  /** What's new in the latest release, in a line or two. */
  notes?: string | null;
  available: boolean;
  checkedAt: number | null;
  error: string | null;
}

export function initialSettings(): Settings {
  return {
    pluginEnabled: {}, shortcuts: {}, pythonPath: null,
    appearance: { theme: null, language: null, handleSize: "medium", textWidth: 16000, autoDirection: true, colors: { light: {}, dark: {} } },
    onboarding: { welcomeDone: false, experience: null, tourEnabled: false, tourStep: 0, completed: [] },
    updates: { enabled: false, lastChecked: null, latestVersion: null, error: null },
  };
}

export interface Settings {
  /** Plug-in name → enabled; a name absent here is enabled. */
  pluginEnabled: Record<string, boolean>;
  /** Action id → shortcut text; empty means the default. */
  shortcuts: Record<string, string>;
  /** Overrides the bundled Python when set. */
  pythonPath: string | null;
  appearance: Appearance;
  onboarding: Onboarding;
  updates: { enabled: boolean; lastChecked: number | null; latestVersion: string | null; error: string | null };
}

export interface PythonInfo {
  path: string;
  version: string | null;
  bundled: boolean;
}

export interface ScriptEntry {
  name: string;
  path: string;
  /** Submenu path relative to the Scripts folder ("" at the top level). */
  folder: string;
}

export interface PluginParam {
  name: string;
  label: string;
  type: string;
  default: number | string | boolean;
  min?: number;
  max?: number;
}

export interface PluginInfo {
  name: string;
  type: string;
  entry: string;
  version: string;
  minKernest: string;
  description: string;
  parameters: PluginParam[];
  enabled: boolean;
  /** Folder name under Plugins, the stable id. */
  id: string;
}

export interface OutputLine {
  stream: string;
  text: string;
}

/** A reporter drawing instruction, in font units over the canvas. */
export type DrawItem =
  | { kind: "line"; x0: number; y0: number; x1: number; y1: number; color?: string; width?: number }
  | { kind: "path"; d: string; color?: string; fill?: string }
  | { kind: "text"; x: number; y: number; text: string; size?: number; color?: string };

export interface ScriptResult {
  ok: boolean;
  output: OutputLine[];
  error?: string;
  applied: boolean;
  revision: number;
  draw: DrawItem[];
}

export interface RunScriptOptions {
  code?: string;
  path?: string;
  current?: string;
  masterId?: string;
  glyphs?: string[];
}

export interface RunFilterPluginOptions {
  name: string;
  params: Record<string, number | string | boolean>;
  current?: string;
  masterId?: string;
  glyphs?: string[];
}

const tauriBackend: Backend = {
  glyphSets: () => invoke("glyph_sets"),
  newFont: (setup) => invoke("new_font", { setup }),
  openSample: () => invoke("open_sample"),
  openFont: (path) => invoke("open_font", { path }),
  saveFont: (path) => invoke("save_font", { path: path ?? null }),
  closeFont: () => invoke("close_font"),
  fontView: (masterId) => invoke("font_view", { masterId: masterId ?? null }),
  fixInterpolation: (glyph) => invoke("fix_interpolation", { glyph }),
  glyphLayer: (glyph, masterId) => invoke("glyph_layer", { glyph, masterId }),
  updateLayer: (glyph, masterId, width, paths, components) =>
    invoke("update_layer", { glyph, masterId, width, paths, components: components ?? null }),
  setMetric: (glyph, masterId, metric, value) => invoke("set_metric", { glyph, masterId, metric, value }),
  setGlyphHints: (glyph, masterId, hints) => invoke("set_glyph_hints", { glyph, masterId, hints }),
  undo: () => invoke("undo"),
  redo: () => invoke("redo"),
  textLayout: (text, masterId, location, direction) => invoke("text_layout", { text, masterId, location: location ?? null, direction: direction ?? null }),
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
  previewFeatureImport: (path) => invoke("preview_feature_import", { path }),
  importFeatures: (path) => invoke("import_features", { path }),
  shapeText: (text, masterId, location, features, direction) => invoke("shape_text", { text, masterId, location: location ?? null, features: features ?? [], direction: direction ?? null }),
  pixelPreview: (text, masterId, location, sizes) => invoke("pixel_preview", { text, masterId, location: location ?? null, sizes }),
  setHinting: (hinting) => invoke("set_hinting", { hinting }),
  setAlignmentZones: (masterId, zones) => invoke("set_alignment_zones", { masterId, zones }),
  setStems: (masterId, stems) => invoke("set_stems", { masterId, stems }),
  setControlInstructions: (text) => invoke("set_control_instructions", { text }),
  setPsHinting: (ps) => invoke("set_ps_hinting", { psHinting: ps }),
  setGrid: (grid) => invoke("set_grid", { grid }),
  setMasterGuides: (masterId, guides) => invoke("set_master_guides", { masterId, guides }),
  setVerticalMetrics: (masterId, vertical) => invoke("set_vertical_metrics", { masterId, vertical }),
  setUseTypoMetrics: (on) => invoke("set_use_typo_metrics", { on }),
  setCustomParameters: (parameters) => invoke("set_custom_parameters", { parameters }),
  setVirtualMasters: (virtualMasters) => invoke("set_virtual_masters", { virtualMasters }),
  setNumbers: (numbers) => invoke("set_numbers", { numbers }),
  describeFeatureToken: (token) => invoke("describe_feature_token", { token }),
  setGuides: (glyph, masterId, guides) => invoke("set_guides", { glyph, masterId, guides }),
  setGuideScope: (glyph, masterId, index, toGlobal) => invoke("set_guide_scope", { glyph, masterId, index, toGlobal }),
  transform: (glyph, masterId, selection, affine, origin, allMasters, onBackground) =>
    invoke("transform", { glyph, masterId, selection, affine, origin, allMasters, onBackground: onBackground ?? false }),
  alignSelection: (glyph, masterId, selection, op, to, origin) => invoke("align_selection", { glyph, masterId, selection, op, to: to ?? null, origin: origin ?? null }),
  backgroundEdit: (glyph, masterId, edit, selection) => invoke("background_edit", { glyph, masterId, edit, selection }),
  updateBackground: (glyph, masterId, paths, components, anchors) => invoke("update_background", { glyph, masterId, paths, components, anchors }),
  measure: (glyph, masterId, p0, p1, onBackground) => invoke("measure", { glyph, masterId, p0, p1, onBackground: onBackground ?? false }),
  setPalettes: (palettes) => invoke("set_palettes", { palettes }),
  setMasterPalettes: (masterId, palettes) => invoke("set_master_palettes", { masterId, palettes }),
  addColorLayer: (glyph, masterId) => invoke("add_color_layer", { glyph, masterId }),
  setColorPaint: (glyph, key, paint) => invoke("set_color_paint", { glyph, key, paint }),
  setMasterPaint: (glyph, key, masterId, paint) => invoke("set_master_paint", { glyph, key, masterId, paint }),
  masterColorPaint: (glyph, key, masterId) => invoke("master_color_paint", { glyph, key, masterId }),
  moveColorLayer: (glyph, key, up) => invoke("move_color_layer", { glyph, key, up }),
  setAxes: (axes) => invoke("set_axes", { axes }),
  addMaster: (copyFrom, name, location) => invoke("add_master", { copyFrom, name, location }),
  previewForeignMasters: (path) => invoke("preview_foreign_masters", { path }),
  addMasterFrom: (path, masterId, name, location) => invoke("add_master_from", { path, masterId, name, location }),
  compareFonts: (path) => invoke("compare_fonts", { path }),
  generateInstances: () => invoke("generate_instances"),
  updateMaster: (id, name, location, metrics) => invoke("update_master", { id, name, location, metrics }),
  removeMaster: (id) => invoke("remove_master", { id }),
  setInstances: (instances) => invoke("set_instances", { instances }),
  parseNameRecords: (text) => invoke("parse_name_records", { text }),
  setVariableFonts: (variableFonts) => invoke("set_variable_fonts", { variableFonts }),
  setExportPresets: (presets) => invoke("set_export_presets", { presets }),
  exportPreset: (name, outDir) => invoke("export_preset", { name, outDir }),
  checkFonts: (paths, profile) => invoke("check_fonts", { paths, profile }),
  saveQualityReport: (path, report) => invoke("save_quality_report", { path, report }),
  setGlyphOrder: (order) => invoke("set_glyph_order", { order }),
  compatibility: () => invoke("compatibility"),
  exportFont: (variable, statics, outDir, options, instances) => invoke("export_font", { variable, statics, outDir, ...options, instances }),
  testInstall: (instances) => invoke("test_install", { instances }),
  testUninstall: () => invoke("test_uninstall"),
  installForUser: (instances) => invoke("install_for_user", { instances }),
  exportDestinations: () => invoke("export_destinations"),
  webPreview: () => invoke("web_preview"),
  exportUfo: (outDir, options) => invoke("export_ufo", { outDir, ...options }),
  setMetricsKey: (glyph, masterId, metric, key) => invoke("set_metrics_key", { glyph, masterId, metric, key }),
  updateMetrics: () => invoke("update_metrics"),
  batchPathOperation: (glyphs, masterId, operation) =>
    invoke("batch_path_operation", { glyphs, masterId, operation }),
  updateMetricsFor: (glyphs) => invoke("update_metrics_for", { glyphs }),
  copyGlyphs: (names) => invoke("copy_glyphs", { names }),
  pasteSpecial: (mode, targets, glyphs) => invoke("paste_special", { mode, targets, glyphs }),
  importFont: (path) => invoke("import_font", { path }),
  saveGlyphs: (path) => invoke("save_glyphs", { path }),
  importSvgText: (glyph, masterId, svg, placement) => invoke("import_svg_text", { glyph, masterId, svg, placement }),
  importSvgFiles: (paths, masterId, placement) => invoke("import_svg_files", { paths, masterId, placement }),
  readMetricsSource: (path) => invoke("read_metrics_source", { path }),
  importMetrics: (path, masterId, sourceMasterId, keep, widths, kerning) => invoke("import_metrics", { path, masterId, sourceMasterId, keep, widths, kerning }),
  addGlyphs: (text, glyphSets) => invoke("add_glyphs", { text, glyphSets }),
  removeGlyphs: (names) => invoke("remove_glyphs", { names }),
  duplicateGlyphs: (names) => invoke("duplicate_glyphs", { names }),
  setGlyphExport: (names, exportGlyph) => invoke("set_glyph_export", { names, export: exportGlyph }),
  setGlyphTags: (glyph, tags) => invoke("set_glyph_tags", { glyph, tags }),
  setGlyphUnicodes: (glyph, unicodes, take) => invoke("set_glyph_unicodes", { glyph, unicodes, take }),
  setProductionName: (glyph, name) => invoke("set_production_name", { glyph, name }),
  setWritingDirection: (glyph, direction) => invoke("set_writing_direction", { glyph, direction }),
  setVerticalMetric: (glyph, masterId, which, value) => invoke("set_vertical_metric", { glyph, masterId, which, value }),
  setFilters: (filters) => invoke("set_filters", { filters }),
  evaluateFilter: (filter) => invoke("evaluate_filter", { filter }),
  renameGlyphs: (renames) => invoke("rename_glyphs", { renames }),
  languageCoverage: () => invoke("language_coverage"),
  missingGlyphs: (language) => invoke("missing_glyphs", { language }),
  addMissing: (language) => invoke("add_missing", { language }),
  pathOperation: (glyph, masterId, operation, paths) => invoke("path_operation", { glyph, masterId, operation, paths }),
  pathEdit: (glyph, masterId, operation, opts) => invoke("path_edit", { glyph, masterId, operation, paths: opts?.paths, nodes: opts?.nodes, points: opts?.points }),
  setPathLocked: (glyph, masterId, path, locked) => invoke("set_path_locked", { glyph, masterId, path, locked }),
  setPathStroke: (glyph, masterId, path, stroke) => invoke("set_path_stroke", { glyph, masterId, path, stroke }),
  setPenWidth: (glyph, masterId, path, node, width) => invoke("set_pen_width", { glyph, masterId, path, node, width }),
  expandStrokes: (glyph, masterId) => invoke("expand_strokes", { glyph, masterId }),
  addPixelGlyph: (size) => invoke("add_pixel_glyph", { size }),
  addPathPart: (glyph, masterId, part) => invoke("add_path_part", { glyph, masterId, part }),
  setPathMask: (glyph, masterId, path, mask) => invoke("set_path_mask", { glyph, masterId, path, mask }),
  hoiView: (glyph, masterId, axis) => invoke("hoi_view", { glyph, masterId, axis }),
  setHoi: (glyph, masterId, axis, path, node, kind) => invoke("set_hoi", { glyph, masterId, axis, path, node, kind }),
  setAnnotations: (glyph, masterId, annotations) => invoke("set_annotations", { glyph, masterId, annotations }),
  setLayerImage: (glyph, masterId, image) => invoke("set_layer_image", { glyph, masterId, image }),
  addLayerImage: (glyph, masterId, file) => invoke("add_layer_image", { glyph, masterId, file }),
  layerImageData: (glyph, masterId) => invoke("layer_image_data", { glyph, masterId }),
  glyphPictures: (glyph) => invoke("glyph_pictures", { glyph }),
  addGlyphBitmap: (glyph, file, ppem) => invoke("add_glyph_bitmap", { glyph, file, ppem }),
  addGlyphSvg: (glyph, file) => invoke("add_glyph_svg", { glyph, file }),
  placeGlyphPicture: (glyph, ppem, x, y, scale) => invoke("place_glyph_picture", { glyph, ppem, x, y, scale }),
  removeGlyphPicture: (glyph, ppem) => invoke("remove_glyph_picture", { glyph, ppem }),
  reorderShapes: (glyph, masterId, order, allLayers) => invoke("reorder_shapes", { glyph, masterId, order, allLayers }),
  setPathPart: (glyph, masterId, index, part) => invoke("set_path_part", { glyph, masterId, index, part }),
  decomposeParts: (glyph, masterId) => invoke("decompose_parts", { glyph, masterId }),
  setPixels: (glyph, masterId, cells, on) => invoke("set_pixels", { glyph, masterId, cells, on }),
  setGlyphLocked: (glyph, locked) => invoke("set_glyph_locked", { glyph, locked }),
  runFilter: (current, masterId, glyphs, name, params, allMasters) => invoke("run_filter", { current, masterId, glyphs, name, params, allMasters }),
  filterPreview: (glyph, masterId, name, params) => invoke("filter_preview", { glyph, masterId, name, params }),
  glyphLayers: (glyph) => invoke("glyph_layers", { glyph }),
  addBraceLayer: (glyph, masterId, location) => invoke("add_brace_layer", { glyph, masterId, location }),
  addBracketLayers: (glyph, masterId, ranges) => invoke("add_bracket_layers", { glyph, masterId, ranges }),
  updateLayerPosition: (glyph, key, location, ranges) => invoke("update_layer_position", { glyph, key, location, ranges }),
  removeLayer: (glyph, key) => invoke("remove_layer", { glyph, key }),
  addBackupLayer: (glyph, masterId, name) => invoke("add_backup_layer", { glyph, masterId, name }),
  useBackupLayer: (glyph, key) => invoke("use_backup_layer", { glyph, key }),
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
  getSettings: () => invoke("get_settings"),
  setAppearance: (appearance) => invoke("set_appearance", { appearance }),
  setOnboarding: (onboarding) => invoke("set_onboarding", { onboarding }),
  recordMilestone: (milestone) => invoke("record_milestone", { milestone }),
  setUpdateCheck: (enabled) => invoke("set_update_check", { enabled }),
  checkUpdates: (force) => invoke("check_updates", { force }),
  installUpdate: (version) => invoke("install_update", { version }),
  tidyKerning: (masterId, allMasters, compress) => invoke("tidy_kerning", { masterId, allMasters, compress }),
  pasteKerning: (masterId, pairs, overwrite) => invoke("paste_kerning", { masterId, pairs, overwrite }),
  setPluginEnabled: (name, enabled) => invoke("set_plugin_enabled", { name, enabled }),
  setShortcut: (action, shortcut) => invoke("set_shortcut", { action, shortcut }),
  resetShortcuts: () => invoke("reset_shortcuts"),
  setPythonPath: (path) => invoke("set_python_path", { path }),
  listScripts: () => invoke("list_scripts"),
  listPlugins: () => invoke("list_plugins"),
  runScript: (opts) => invoke("run_script", { code: opts.code ?? null, path: opts.path ?? null, current: opts.current ?? null, masterId: opts.masterId ?? null, glyphs: opts.glyphs ?? [] }),
  cancelScript: () => invoke("cancel_script"),
  runFilterPlugin: (opts) => invoke("run_filter_plugin", { name: opts.name, params: opts.params, current: opts.current ?? null, masterId: opts.masterId ?? null, glyphs: opts.glyphs ?? [] }),
  reporterDraw: (name, glyph, masterId) => invoke("reporter_draw", { name, glyph, masterId }),
  openScriptsFolder: () => invoke("open_scripts_folder"),
  openPluginsFolder: () => invoke("open_plugins_folder"),
  onScriptOutput: (cb) => listen<OutputLine>("script-output", (e) => cb(e.payload)),
  pythonInfo: () => invoke("python_info"),
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
  hints?: Hint[];
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
  variableFonts: VariableFontSetting[];
  exportPresets: ExportPreset[];
  hinting?: Hinting;
  controlInstructions?: string;
  psHinting?: PsHinting | null;
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

const snap = (s: Snapshot): Snapshot => ({ width: s.width, paths: clonePaths(s.paths), components: s.components.map((c) => ({ base: c.base, transform: [...c.transform] })), hints: (s.hints ?? []).map((h) => ({ ...h })) });

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
    familyName, unitsPerEm: 1000, versionMajor: 1, versionMinor: 0, names: { ...EMPTY_NAMES }, notes: "", axes: [], instances: [], variableFonts: [], exportPresets: [],
    masters: [{ id: "m01", name: "Regular", location: {}, metrics: { ...METRICS }, alignmentZones: [], stems: [] }],
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
    filters: [],
    glyphs: d.glyphs.map((g) => ({ name: g.name, unicode: g.unicode, unicodes: g.unicode ? [g.unicode] : [], character: g.character, category: g.category, subcategory: "", script: "Latin", export: true, width: g.width, ...sb(d, g), leftGroup: null, rightGroup: null, tags: [], svg: pathsToSvg(resolved(d, g)), compatible: true })),
  };
}

function mockLayer(d: MockDoc, g: MockGlyph): LayerView {
  const components = g.components.map((c) => ({ base: c.base, transform: [...c.transform], align: true, svg: pathsToSvg(resolveComponent(d, c)) }));
  return {
    glyph: g.name, unicode: g.unicode, masterId: "m01", metrics: METRICS, width: g.width, ...sb(d, g),
    paths: clonePaths(g.paths), components, componentSvg: components.map((c) => c.svg).join(""), anchors: [], hints: (g.hints ?? []).map((h) => ({ ...h })), autoHints: [], metricsKeys: {}, issues: [], glyphLocked: false, guides: [], masterGuides: [], status: status(d),
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
  // Outline edits keep the layer's hints, as update_layer does.
  const after = snap(next.hints === undefined ? { ...next, hints: g.hints } : next);
  if (JSON.stringify(before) === JSON.stringify(after)) return;
  Object.assign(g, after);
  record(d, { kind: "layer", glyph: g.name, before, after });
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
  names: m.names, notes: m.notes, axes: m.axes, masters: m.masters, instances: m.instances, variableFonts: m.variableFonts, exportPresets: m.exportPresets, hinting: m.hinting, controlInstructions: m.controlInstructions, psHinting: m.psHinting, grid: m.grid,
}));

function mockInfo(d: MockDoc): FontInfo {
  const def = d.masters.find((m) => d.axes.every((a) => (m.location[a.tag] ?? a.default) === a.default)) ?? d.masters[0];
  const warnings: string[] = [];
  if (d.masters.length > 1 && d.axes.length === 0) warnings.push("There are several masters but no axes. Add an axis in Font Info › Axes.");
  if (d.axes.length && !d.masters.some((m) => d.axes.every((a) => (m.location[a.tag] ?? a.default) === a.default)))
    warnings.push("No master sits at the default location of every axis; interpolation needs one.");
  return { ...metaCopy(d), hinting: d.hinting ?? DEFAULT_HINTING, controlInstructions: d.controlInstructions ?? "", psHinting: d.psHinting ?? DEFAULT_PS_HINTING, grid: d.grid ?? null, useTypoMetrics: false, customParameters: [], virtualMasters: [], numbers: {}, palettes: [], masterPalettes: {}, defaultMasterId: def.id, glyphOrder: d.glyphs.map((g) => g.name), warnings, incompatible: 0, status: status(d) };
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
const MOCK_SETS: Record<string, number[]> = {
  "latin-basic": range(0x20, 0x7e),
  "latin-1": range(0xa0, 0xff).filter((cp) => cp !== 0xad),
  punctuation: [0x2013, 0x2014, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2026, 0x20ac],
  cyrillic: range(0x410, 0x44f),
  "latin-ext-a": range(0x100, 0x17f),
  "latin-ext-additional": range(0x1e00, 0x1eff),
  greek: range(0x370, 0x3ff).filter((cp) => ![0x378, 0x379, 0x380, 0x381, 0x382, 0x383, 0x38b, 0x38d, 0x3a2].includes(cp)),
  "cyrillic-extended": [...range(0x400, 0x4ff), ...range(0x500, 0x52f)],
  arabic: [...range(0x600, 0x6ff), ...range(0xfb50, 0xfdff), ...range(0xfe70, 0xfeff)],
  hebrew: range(0x590, 0x5ff),
};

const MOCK_LANGUAGES: [string, string[]][] = [
  ["Latin", ["latin-basic", "latin-1", "latin-ext-a", "latin-ext-additional"]],
  ["Greek", ["greek"]],
  ["Cyrillic", ["cyrillic", "cyrillic-extended"]],
  ["Arabic", ["arabic"]],
  ["Hebrew", ["hebrew"]],
];

const mockSample = (id: string) => String.fromCodePoint(...(MOCK_SETS[id] ?? []).filter((cp) => cp > 0x20).slice(0, 64));

const hexCp = (cp: number) => cp.toString(16).toUpperCase().padStart(4, "0");

const mockSettings: Settings = initialSettings();
const mockSettingsView = (): Settings => structuredClone(mockSettings);

const mockBackend: Backend = {
  async glyphSets() {
    return [
      { id: "latin-basic", name: "Latin Basic (ASCII)", count: 95, sample: mockSample("latin-basic") },
      { id: "latin-1", name: "Latin-1 Supplement", count: 95, sample: mockSample("latin-1") },
      { id: "punctuation", name: "General Punctuation", count: 9, sample: mockSample("punctuation") },
      { id: "cyrillic", name: "Cyrillic Basic", count: 64, sample: mockSample("cyrillic") },
      { id: "latin-ext-a", name: "Latin Extended-A", count: 128, sample: mockSample("latin-ext-a") },
      { id: "latin-ext-additional", name: "Latin Extended Additional", count: 256, sample: mockSample("latin-ext-additional") },
      { id: "greek", name: "Greek", count: 135, sample: mockSample("greek") },
      { id: "cyrillic-extended", name: "Cyrillic Extended", count: 298, sample: mockSample("cyrillic-extended") },
      { id: "arabic", name: "Arabic", count: 972, sample: mockSample("arabic") },
      { id: "hebrew", name: "Hebrew", count: 57, sample: mockSample("hebrew") },
    ];
  },
  async newFont(setup) {
    if (!setup.familyName.trim()) throw "Enter a family name.";
    const doc = mockDoc(setup.familyName.trim(), [...new Set(setup.glyphSets.flatMap((s) => MOCK_SETS[s] ?? []))], false);
    doc.unitsPerEm = setup.unitsPerEm ?? 1000;
    doc.axes = structuredClone(setup.axes ?? []);
    doc.instances = structuredClone(setup.instances ?? []);
    doc.masters = (setup.masters ?? [{ name: "Regular", location: {} }]).map((m, i) => ({ ...m, id: `m${String(i + 1).padStart(2, "0")}`, metrics: { ...(setup.metrics ?? METRICS) }, alignmentZones: [], stems: [] }));
    for (const glyph of doc.glyphs) glyph.width *= doc.unitsPerEm / 1000;
    doc.revision = 1;
    mock = doc;
    return mockFontView(doc);
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
  async fixInterpolation() {
    throw "Fixing interpolation needs the desktop app (npm run tauri dev).";
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
  async previewFeatureImport() {
    throw "Importing features needs the desktop app (npm run tauri dev).";
  },
  async importFeatures() {
    throw "Importing features needs the desktop app (npm run tauri dev).";
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
  async setAlignmentZones(masterId, zones) {
    for (const z of zones) {
      if (!Number.isFinite(z.position) || !Number.isFinite(z.size)) throw "Zone positions and sizes must be numbers.";
      if (z.size === 0) throw "A zone needs a size other than 0.";
    }
    return editMeta(need(), (m) => {
      const x = m.masters.find((k) => k.id === masterId);
      if (!x) throw `No master with id "${masterId}"`;
      x.alignmentZones = zones.map((z) => ({ ...z }));
    });
  },
  async setStems(masterId, stems) {
    for (const s of stems) {
      if (!s.name.trim()) throw "Every stem needs a name.";
      if (!Number.isFinite(s.value) || s.value <= 0) throw "Stem widths must be greater than 0.";
    }
    return editMeta(need(), (m) => {
      const x = m.masters.find((k) => k.id === masterId);
      if (!x) throw `No master with id "${masterId}"`;
      x.stems = stems.map((s) => ({ ...s }));
    });
  },
  async setControlInstructions(text) {
    return editMeta(need(), (m) => { m.controlInstructions = text.trimEnd(); });
  },
  async setPsHinting(ps) {
    if (ps) {
      if (!Number.isFinite(ps.blueScale) || ps.blueScale <= 0) throw "Blue scale must be greater than 0.";
      if (!Number.isFinite(ps.blueShift) || ps.blueShift < 0) throw "Blue shift must be 0 or more.";
      if (!Number.isFinite(ps.blueFuzz) || ps.blueFuzz < 0) throw "Blue fuzz must be 0 or more.";
    }
    return editMeta(need(), (m) => {
      m.psHinting = ps && JSON.stringify(ps) !== JSON.stringify(DEFAULT_PS_HINTING) ? { ...ps } : null;
    });
  },
  async pixelPreview() {
    throw "The pixel preview needs the desktop app (npm run tauri dev).";
  },
  async setGrid(grid) {
    const whole = (v: number) => Number.isInteger(v) && v >= 0 && v <= 65535;
    if (grid && (!whole(grid.spacing) || !whole(grid.subdivision))) throw "Grid spacing and subdivision must be whole numbers.";
    return editMeta(need(), (m) => { m.grid = grid && grid.spacing > 0 && grid.subdivision > 0 ? { ...grid } : null; });
  },
  async setMasterGuides() {
    throw "Guides need the desktop app (npm run tauri dev).";
  },
  async setVerticalMetrics() {
    throw "Line spacing needs the desktop app (npm run tauri dev).";
  },
  async setUseTypoMetrics() {
    throw "Line spacing needs the desktop app (npm run tauri dev).";
  },
  async setVirtualMasters() {
    throw "Virtual masters need the desktop app (npm run tauri dev).";
  },
  async setNumbers() {
    throw "Number values need the desktop app (npm run tauri dev).";
  },
  async describeFeatureToken() {
    throw "Token previews need the desktop app (npm run tauri dev).";
  },
  async setCustomParameters() {
    throw "Custom parameters need the desktop app (npm run tauri dev).";
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
  async alignSelection() {
    throw "Aligning needs the desktop app (npm run tauri dev).";
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
        for (const a of axes) loc[a.tag] ??= designBounds(a)[1];
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
    return editMeta(d, (m) => m.masters.push({ id: `m${String(n).padStart(2, "0")}`, name: name.trim(), location: loc, metrics: { ...src.metrics }, alignmentZones: src.alignmentZones.map((z) => ({ ...z })), stems: src.stems.map((s) => ({ ...s })) }));
  },
  async previewForeignMasters() {
    throw "Adding masters from another font needs the desktop app (npm run tauri dev).";
  },
  async addMasterFrom() {
    throw "Adding masters from another font needs the desktop app (npm run tauri dev).";
  },
  async compareFonts() {
    throw "Comparing fonts needs the desktop app (npm run tauri dev).";
  },
  async generateInstances() {
    throw "Generating instances needs the desktop app (npm run tauri dev).";
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
  async parseNameRecords() {
    throw "Name table entries need the desktop app (npm run tauri dev).";
  },
  async setInstances(instances) {
    const d = need();
    const names = new Set<string>();
    const clean = instances.map((i) => {
      const name = i.name.trim();
      if (!name) throw "Every instance needs a name.";
      const fam = (i.familyName ?? "").trim();
      const familyName = fam && fam !== d.familyName ? fam : undefined;
      if (familyName && [...familyName].length > 63) throw `Family name "${familyName}" is too long; use at most 63 characters.`;
      const key = `${familyName ?? ""}\u0000${name}`;
      if (names.has(key)) throw familyName ? `Instance name "${name}" is used twice in family "${familyName}".` : `Instance name "${name}" is used twice.`;
      names.add(key);
      return { ...i, name, location: checkLocation(d, i.location), export: i.export ?? true, familyName };
    });
    return editMeta(d, (m) => { m.instances = clean; });
  },
  async setVariableFonts(variableFonts) {
    const d = need();
    const names = new Set<string>();
    const files = new Set<string>();
    const clean = variableFonts.map((s) => {
      const name = s.name.trim();
      const fileName = s.fileName.trim();
      if (!name) throw "Every variable font needs a name.";
      if (names.has(name)) throw `Variable font name "${name}" is used twice.`;
      names.add(name);
      if (!fileName) throw `Variable font "${name}" needs a file name.`;
      if (/[\\/:]/.test(fileName)) throw `File name "${fileName}" must not contain a path.`;
      if (files.has(fileName)) throw `File name "${fileName}" is used twice.`;
      files.add(fileName);
      const tags = new Set<string>();
      const axes = (s.axes ?? []).map((sub) => {
        const a = d.axes.find((x) => x.tag === sub.tag);
        if (!a) throw `Variable font "${name}" subsets unknown axis "${sub.tag}".`;
        if (tags.has(sub.tag)) throw `Variable font "${name}" lists axis "${sub.tag}" twice.`;
        tags.add(sub.tag);
        const lo = sub.min ?? a.min;
        const hi = sub.max ?? a.max;
        if (!Number.isFinite(lo) || !Number.isFinite(hi) || lo > hi || lo < a.min || hi > a.max)
          throw `Variable font "${name}" range on "${a.name}" must sit inside the axis range, minimum ≤ maximum.`;
        return { tag: sub.tag, min: sub.min ?? null, max: sub.max ?? null };
      });
      const glyphs = s.glyphs ?? [];
      for (const g of glyphs)
        if (!d.glyphs.some((x) => x.name === g)) throw `Variable font "${name}" keeps unknown glyph "${g}".`;
      return { name, fileName, axes, glyphs: [...glyphs], renameGlyphs: s.renameGlyphs, removeGlyphs: s.removeGlyphs, disabledMasters: s.disabledMasters };
    });
    return editMeta(d, (m) => { m.variableFonts = clean; });
  },
  async setGlyphOrder() {
    throw "Glyph order needs the desktop app (npm run tauri dev).";
  },
  async setExportPresets(presets) {
    const d = need();
    return editMeta(d, (m) => { m.exportPresets = structuredClone(presets); });
  },
  async exportPreset() {
    throw "Exporting fonts needs the desktop app (npm run tauri dev).";
  },
  async checkFonts() {
    throw "Font quality checks need the desktop app (npm run tauri dev).";
  },
  async saveQualityReport() {
    throw "Saving needs the desktop app (npm run tauri dev).";
  },
  async exportFont() {
    throw "Exporting fonts needs the desktop app (npm run tauri dev).";
  },
  async testInstall() {
    throw "Test Install needs the desktop app (npm run tauri dev).";
  },
  async testUninstall() {
    return 0;
  },
  async installForUser() {
    throw "Installing fonts needs the desktop app (npm run tauri dev).";
  },
  async exportDestinations() {
    return [];
  },
  async webPreview() {
    throw "The web preview needs the desktop app (npm run tauri dev).";
  },
  async exportUfo() {
    throw "Exporting UFO sources needs the desktop app (npm run tauri dev).";
  },
  async setMetricsKey() {
    throw "Metrics keys need the desktop app (npm run tauri dev).";
  },
  async updateMetrics() {
    throw "Metrics keys need the desktop app (npm run tauri dev).";
  },
  async batchPathOperation() {
    throw "Batch edits need the desktop app (npm run tauri dev).";
  },
  async updateMetricsFor() {
    throw "Metrics keys need the desktop app (npm run tauri dev).";
  },
  async copyGlyphs() {
    throw "Copying glyphs needs the desktop app (npm run tauri dev).";
  },
  async pasteSpecial() {
    throw "Paste Special needs the desktop app (npm run tauri dev).";
  },
  async importFont() {
    throw "Importing fonts needs the desktop app (npm run tauri dev).";
  },
  async saveGlyphs() {
    throw "Saving .glyphs needs the desktop app (npm run tauri dev).";
  },
  async importSvgText() {
    throw "Importing SVG needs the desktop app (npm run tauri dev).";
  },
  async importSvgFiles() {
    throw "Importing SVG needs the desktop app (npm run tauri dev).";
  },
  async readMetricsSource() {
    throw "Importing metrics needs the desktop app (npm run tauri dev).";
  },
  async importMetrics() {
    throw "Importing metrics needs the desktop app (npm run tauri dev).";
  },
  async addGlyphs() {
    throw "Adding glyphs needs the desktop app (npm run tauri dev).";
  },
  async duplicateGlyphs() {
    throw "Duplicating glyphs needs the desktop app (npm run tauri dev).";
  },
  async removeGlyphs() {
    throw "Removing glyphs needs the desktop app (npm run tauri dev).";
  },
  async setGlyphExport() {
    throw "Changing export settings needs the desktop app (npm run tauri dev).";
  },
  async setGlyphTags() {
    throw "Tags need the desktop app (npm run tauri dev).";
  },
  async setGlyphUnicodes() {
    throw "Unicode values need the desktop app (npm run tauri dev).";
  },
  async setWritingDirection() {
    throw "Writing directions need the desktop app (npm run tauri dev).";
  },
  async setVerticalMetric() {
    throw "Vertical metrics need the desktop app (npm run tauri dev).";
  },
  async setProductionName() {
    throw "Production names need the desktop app (npm run tauri dev).";
  },
  async setFilters() {
    throw "Saving filters needs the desktop app (npm run tauri dev).";
  },
  async evaluateFilter() {
    throw "Filters need the desktop app (npm run tauri dev).";
  },
  async renameGlyphs() {
    throw "Renaming needs the desktop app (npm run tauri dev).";
  },
  async languageCoverage() {
    const glyphs = mock?.glyphs ?? [];
    return MOCK_LANGUAGES.map(([language, sets]) => {
      const cps = [...new Set(sets.flatMap((s) => MOCK_SETS[s] ?? []))];
      const present = cps.filter((cp) => glyphs.some((g) => g.unicode === hexCp(cp))).length;
      return { language, present, total: cps.length };
    });
  },
  async missingGlyphs(language) {
    const glyphs = mock?.glyphs ?? [];
    const sets = MOCK_LANGUAGES.find(([l]) => l === language)?.[1] ?? [];
    const cps = [...new Set(sets.flatMap((s) => MOCK_SETS[s] ?? []))];
    return cps
      .filter((cp) => !glyphs.some((g) => g.unicode === hexCp(cp)))
      .map((cp) => ({ name: `uni${hexCp(cp)}`, unicode: hexCp(cp) }));
  },
  async addMissing() {
    throw "Adding glyphs needs the desktop app (npm run tauri dev).";
  },
  async pathOperation() {
    throw "Path operations need the desktop app (npm run tauri dev).";
  },
  async pathEdit() {
    throw "Path operations need the desktop app (npm run tauri dev).";
  },
  async setPathStroke() {
    throw "Strokes need the desktop app (npm run tauri dev).";
  },
  async setPenWidth() {
    throw "Strokes need the desktop app (npm run tauri dev).";
  },
  async expandStrokes() {
    throw "Strokes need the desktop app (npm run tauri dev).";
  },
  async setAnnotations() {
    throw "Annotations need the desktop app (npm run tauri dev).";
  },
  async setLayerImage() {
    throw "Images need the desktop app (npm run tauri dev).";
  },
  async addLayerImage() {
    throw "Images need the desktop app (npm run tauri dev).";
  },
  async layerImageData() {
    return null;
  },
  async glyphPictures() {
    return [];
  },
  async addGlyphBitmap() {
    throw "Bitmaps need the desktop app (npm run tauri dev).";
  },
  async addGlyphSvg() {
    throw "SVG drawings need the desktop app (npm run tauri dev).";
  },
  async placeGlyphPicture() {
    return [];
  },
  async removeGlyphPicture() {
    return [];
  },
  async hoiView() {
    return { axes: [], axis: "", curves: [] };
  },
  async setHoi() {
    throw "Interpolation curves need the desktop app (npm run tauri dev).";
  },
  async setPathMask() {
    throw "Masks need the desktop app (npm run tauri dev).";
  },
  async reorderShapes() {
    throw "Masks need the desktop app (npm run tauri dev).";
  },
  async addPathPart() {
    throw "Corner components need the desktop app (npm run tauri dev).";
  },
  async setPathPart() {
    throw "Corner components need the desktop app (npm run tauri dev).";
  },
  async decomposeParts() {
    throw "Corner components need the desktop app (npm run tauri dev).";
  },
  async addPixelGlyph() {
    throw "The pixel tool needs the desktop app (npm run tauri dev).";
  },
  async setPixels() {
    throw "The pixel tool needs the desktop app (npm run tauri dev).";
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
  async addBackupLayer() {
    throw "Backups need the desktop app (npm run tauri dev).";
  },
  async useBackupLayer() {
    throw "Backups need the desktop app (npm run tauri dev).";
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
  async setMasterPalettes() {
    throw "Colour fonts need the desktop app (npm run tauri dev).";
  },
  async addColorLayer() {
    throw "Colour fonts need the desktop app (npm run tauri dev).";
  },
  async setColorPaint() {
    throw "Colour fonts need the desktop app (npm run tauri dev).";
  },
  async setMasterPaint() {
    throw "Colour fonts need the desktop app (npm run tauri dev).";
  },
  async masterColorPaint() {
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
  async setGlyphHints(glyph, _m, hints) {
    const d = need();
    const g = findGlyph(d, glyph);
    for (const h of hints) {
      if (!Number.isFinite(h.position) || !Number.isFinite(h.width) || h.width < 0) throw "Hint positions must be numbers and widths 0 or more.";
    }
    mockApply(d, g, { ...snap(g), hints: hints.map((h) => ({ ...h })) });
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
        if ("missing" in t) return { type: "missing", text: t.missing, rtl: false };
        const g = findGlyph(d, t.name);
        const next = line[i + 1];
        const k = next && "name" in next ? pairValue(d, t.name, next.name) : null;
        return {
          type: "glyph", name: g.name, unicode: g.unicode, incompatible: false, width: g.width, ...sb(d, g), leftGroup: g.leftGroup, rightGroup: g.rightGroup,
          rtl: false, vAdvance: METRICS.ascender - METRICS.descender, vOrigin: METRICS.ascender, tsb: null, bsb: null,
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
  async getSettings() {
    return mockSettingsView();
  },
  async setAppearance(appearance) {
    mockSettings.appearance = structuredClone(appearance);
    return mockSettingsView();
  },
  async setOnboarding(onboarding) {
    mockSettings.onboarding = structuredClone(onboarding);
    return mockSettingsView();
  },
  async recordMilestone(milestone) {
    if (!mockSettings.onboarding.completed.includes(milestone)) mockSettings.onboarding.completed.push(milestone);
    return mockSettingsView();
  },
  async setUpdateCheck(enabled) {
    mockSettings.updates.enabled = enabled;
    return mockSettingsView();
  },
  async checkUpdates() {
    return { currentVersion: __APP_VERSION__, latestVersion: null, available: false, checkedAt: null, error: null };
  },
  async installUpdate() {
    throw "Updating needs the desktop app.";
  },
  async tidyKerning() {
    throw "Compress and Clean Up need the desktop app (npm run tauri dev).";
  },
  async pasteKerning() {
    throw "Pasting kerning needs the desktop app (npm run tauri dev).";
  },
  async setPluginEnabled(name, enabled) {
    mockSettings.pluginEnabled[name] = enabled;
    return mockSettingsView();
  },
  async setShortcut(action, shortcut) {
    const s = shortcut.trim();
    if (s) mockSettings.shortcuts[action] = s;
    else delete mockSettings.shortcuts[action];
    return mockSettingsView();
  },
  async resetShortcuts() {
    mockSettings.shortcuts = {};
    return mockSettingsView();
  },
  async setPythonPath(path) {
    mockSettings.pythonPath = path && path.trim() ? path.trim() : null;
    return mockSettingsView();
  },
  async listScripts() {
    return [];
  },
  async listPlugins() {
    return [];
  },
  async runScript() {
    throw "Running scripts needs the desktop app (npm run tauri dev).";
  },
  async cancelScript() {},
  async runFilterPlugin() {
    throw "Running plug-ins needs the desktop app (npm run tauri dev).";
  },
  async reporterDraw() {
    return [];
  },
  async openScriptsFolder() {
    throw "Opening folders needs the desktop app (npm run tauri dev).";
  },
  async openPluginsFolder() {
    throw "Opening folders needs the desktop app (npm run tauri dev).";
  },
  async onScriptOutput() {
    return () => {};
  },
  async pythonInfo() {
    return { path: "", version: null, bundled: false };
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
