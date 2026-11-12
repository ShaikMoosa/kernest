// Typed access to the Rust backend (Tauri commands).
// When the UI runs in a plain browser (`npm run dev` without Tauri) a small
// in-memory mock stands in, so screens can be previewed and tested.

import { invoke } from "@tauri-apps/api/core";

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
}

export const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export interface Backend {
  glyphSets(): Promise<GlyphSetInfo[]>;
  newFont(familyName: string, glyphSets: string[]): Promise<FontView>;
  openSample(): Promise<FontView>;
  openFont(path: string): Promise<FontView>;
  saveFont(path?: string): Promise<string>;
  fontView(masterId?: string): Promise<FontView>;
}

const tauriBackend: Backend = {
  glyphSets: () => invoke("glyph_sets"),
  newFont: (familyName, glyphSets) => invoke("new_font", { familyName, glyphSets }),
  openSample: () => invoke("open_sample"),
  openFont: (path) => invoke("open_font", { path }),
  saveFont: (path) => invoke("save_font", { path: path ?? null }),
  fontView: (masterId) => invoke("font_view", { masterId: masterId ?? null }),
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

const rect = (x0: number, y0: number, x1: number, y1: number) => `M${x0} ${y0}L${x1} ${y0}L${x1} ${y1}L${x0} ${y1}Z`;
const MOCK_OUTLINES: Record<string, [number, string]> = {
  H: [680, rect(80, 0, 170, 700) + rect(510, 0, 600, 700) + rect(80, 310, 600, 390)],
  I: [250, rect(80, 0, 170, 700)],
  L: [540, rect(80, 0, 170, 700) + rect(80, 0, 500, 80)],
  T: [600, rect(255, 0, 345, 620) + rect(40, 620, 560, 700)],
  n: [600, rect(75, 0, 160, 500) + rect(440, 0, 525, 380) + rect(75, 380, 525, 500)],
  l: [240, rect(75, 0, 160, 760)],
};

function mockFont(familyName: string, codepoints: number[], withOutlines: boolean): FontView {
  const glyphs = codepoints.map((cp): GlyphSummary => {
    const ch = String.fromCodePoint(cp);
    const name = ASCII_NAMES[cp] ?? (/[A-Za-z]/.test(ch) ? ch : `uni${cp.toString(16).toUpperCase().padStart(4, "0")}`);
    const drawn = withOutlines ? MOCK_OUTLINES[name] : undefined;
    const width = drawn?.[0] ?? (cp === 0x20 ? 250 : 600);
    return {
      name,
      unicode: cp.toString(16).toUpperCase().padStart(4, "0"),
      character: ch,
      category: category(ch),
      export: true,
      width,
      lsb: drawn ? 80 : null,
      rsb: drawn ? 80 : null,
      svg: drawn?.[1] ?? "",
    };
  });
  return {
    familyName,
    path: null,
    unitsPerEm: 1000,
    masters: [{ id: "m01", name: "Regular" }],
    masterId: "m01",
    metrics: { ascender: 800, capHeight: 700, xHeight: 500, descender: -200, italicAngle: 0 },
    glyphs,
  };
}

const range = (a: number, b: number) => Array.from({ length: b - a + 1 }, (_, i) => a + i);
const MOCK_SETS: Record<string, number[]> = {
  "latin-basic": range(0x20, 0x7e),
  "latin-1": range(0xa0, 0xff),
  cyrillic: range(0x410, 0x44f),
};

let mockDoc: FontView | null = null;
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
    const cps = [...new Set(sets.flatMap((s) => MOCK_SETS[s] ?? []))];
    return (mockDoc = mockFont(familyName.trim(), cps, false));
  },
  async openSample() {
    return (mockDoc = mockFont("Kernest Sample", range(0x20, 0x7e), true));
  },
  async openFont() {
    throw "Opening files needs the desktop app (npm run tauri dev).";
  },
  async saveFont() {
    throw "Saving needs the desktop app (npm run tauri dev).";
  },
  async fontView() {
    if (!mockDoc) throw "No font is open.";
    return mockDoc;
  },
};

export const backend: Backend = isTauri ? tauriBackend : mockBackend;

/** Turns a rejected command (a string from Rust, or an Error) into a message. */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
