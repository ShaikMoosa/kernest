// Typed access to the Rust backend (Tauri commands).
// When the UI runs in a plain browser (`npm run dev` without Tauri) an in-memory
// mock stands in, so screens can be previewed and tested without the desktop shell.

import { invoke } from "@tauri-apps/api/core";
import { clonePaths, pathsToSvg, pointBounds, rectPath, transformPaths, type EditPath } from "./outline";

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
}

export interface FontView {
  familyName: string;
  path: string | null;
  unitsPerEm: number;
  masters: MasterInfo[];
  masterId: string;
  metrics: Metrics;
  glyphs: GlyphSummary[];
  status: DocStatus;
}

/** A component reference: base glyph name and affine transform [xx, xy, yx, yy, dx, dy]. */
export interface ComponentEdit {
  base: string;
  transform: number[];
}

export interface ComponentView extends ComponentEdit {
  /** This component's resolved outline, SVG path data in font units. */
  svg: string;
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
  status: DocStatus;
}

export interface UndoResult {
  glyph: string;
  masterId: string;
  status: DocStatus;
}

export type MetricName = "lsb" | "rsb" | "width";

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
}

interface MockDoc {
  familyName: string;
  glyphs: MockGlyph[];
  undo: { glyph: string; before: Snapshot; after: Snapshot }[];
  redo: MockDoc["undo"];
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
    };
  });
  return { familyName, glyphs, undo: [], redo: [], revision: 0, savedRevision: 0 };
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

function mockFontView(d: MockDoc): FontView {
  return {
    familyName: d.familyName,
    path: null,
    unitsPerEm: 1000,
    masters: [{ id: "m01", name: "Regular" }],
    masterId: "m01",
    metrics: METRICS,
    status: status(d),
    glyphs: d.glyphs.map((g) => ({ name: g.name, unicode: g.unicode, character: g.character, category: g.category, export: true, width: g.width, ...sb(d, g), svg: pathsToSvg(resolved(d, g)) })),
  };
}

function mockLayer(d: MockDoc, g: MockGlyph): LayerView {
  const components = g.components.map((c) => ({ base: c.base, transform: [...c.transform], svg: pathsToSvg(resolveComponent(d, c)) }));
  return {
    glyph: g.name, unicode: g.unicode, masterId: "m01", metrics: METRICS, width: g.width, ...sb(d, g),
    paths: clonePaths(g.paths), components, componentSvg: components.map((c) => c.svg).join(""), anchors: [], status: status(d),
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

function mockApply(d: MockDoc, g: MockGlyph, next: Snapshot) {
  const before = snap(g);
  if (JSON.stringify(before) === JSON.stringify(snap(next))) return;
  Object.assign(g, snap(next));
  d.undo.push({ glyph: g.name, before, after: snap(next) });
  d.redo = [];
  d.revision++;
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
  async fontView() {
    return mockFontView(need());
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
    Object.assign(findGlyph(d, c.glyph), snap(c.before));
    d.redo.push(c);
    d.revision++;
    return { glyph: c.glyph, masterId: "m01", status: status(d) };
  },
  async redo() {
    const d = need();
    const c = d.redo.pop();
    if (!c) return null;
    Object.assign(findGlyph(d, c.glyph), snap(c.after));
    d.undo.push(c);
    d.revision++;
    return { glyph: c.glyph, masterId: "m01", status: status(d) };
  },
};

export const backend: Backend = isTauri ? tauriBackend : mockBackend;

/** Turns a rejected command (a string from Rust, or an Error) into a message. */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
