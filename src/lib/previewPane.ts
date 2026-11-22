// The preview pane under the Edit View and the separate preview window (SMO-36):
// settings that persist between sessions, and how one line of glyphs is placed.

/** What the preview draws: the master being edited, another master, or an instance. */
export interface PreviewSettings {
  /** "" = the master being edited; "m:<id>" = a master; "i:<name>" = an instance. */
  source: string;
  /** Text size in pixels. */
  size: number;
  /** White on black instead of black on white. */
  invert: boolean;
}

export const DEFAULT_PREVIEW: PreviewSettings = { source: "", size: 64, invert: false };
export const PREVIEW_SIZES = [24, 32, 48, 64, 96, 128, 192];

const SETTINGS_KEY = "kernest.previewSettings";
const HEIGHT_KEY = "kernest.previewHeight";
export const MIN_PREVIEW_HEIGHT = 80;
export const DEFAULT_PREVIEW_HEIGHT = 160;

/** Reads stored settings, keeping only valid values. */
export function parsePreviewSettings(raw: string | null): PreviewSettings {
  try {
    const v = raw ? JSON.parse(raw) : null;
    if (!v || typeof v !== "object") return { ...DEFAULT_PREVIEW };
    return {
      source: typeof v.source === "string" && (v.source === "" || /^[mi]:/.test(v.source)) ? v.source : "",
      size: typeof v.size === "number" && v.size >= 8 && v.size <= 512 ? v.size : DEFAULT_PREVIEW.size,
      invert: v.invert === true,
    };
  } catch {
    return { ...DEFAULT_PREVIEW };
  }
}

/** The pane under the Edit View and the preview window each keep their own settings. */
export type PreviewPlace = "pane" | "window";
const keyFor = (place: PreviewPlace) => (place === "window" ? `${SETTINGS_KEY}.window` : SETTINGS_KEY);

export function loadPreviewSettings(place: PreviewPlace = "pane"): PreviewSettings {
  try { return parsePreviewSettings(localStorage.getItem(keyFor(place))); } catch { return { ...DEFAULT_PREVIEW }; }
}

export function savePreviewSettings(s: PreviewSettings, place: PreviewPlace = "pane"): void {
  try { localStorage.setItem(keyFor(place), JSON.stringify(s)); } catch { /* a convenience only */ }
}

/** Keeps the pane between its minimum and most of the available height. */
export function clampPreviewHeight(h: number, available: number): number {
  const max = Math.max(MIN_PREVIEW_HEIGHT, Math.round(available * 0.7));
  return Math.round(Math.min(max, Math.max(MIN_PREVIEW_HEIGHT, h)));
}

export function loadPreviewHeight(): number {
  try {
    const v = Number(localStorage.getItem(HEIGHT_KEY));
    return Number.isFinite(v) && v >= MIN_PREVIEW_HEIGHT ? v : DEFAULT_PREVIEW_HEIGHT;
  } catch {
    return DEFAULT_PREVIEW_HEIGHT;
  }
}

export function savePreviewHeight(h: number): void {
  try { localStorage.setItem(HEIGHT_KEY, String(Math.round(h))); } catch { /* a convenience only */ }
}

/** One item of a laid-out line, as the preview needs it. */
export interface LineItem { width: number; kern: number; rtl: boolean }

/**
 * Places one line of glyphs left to right in pixels. `order` is the visual order
 * of the items (from the bidi algorithm). Kerning goes after a left-to-right glyph
 * and before a right-to-left one, as in the text tab. Returns each item's x by
 * index, and the line's width.
 */
export function placeLine(items: LineItem[], order: number[], scale: number): { x: number[]; width: number } {
  const x: number[] = new Array(items.length).fill(0);
  let at = 0;
  for (const i of order) {
    const it = items[i];
    const k = it.kern * scale;
    if (it.rtl) at += k;
    x[i] = at;
    at += it.width * scale;
    if (!it.rtl) at += k;
  }
  return { x, width: at };
}

/** The preview text for a glyph tab when none was typed: the glyph itself. */
export function defaultPreviewText(glyph: string): string {
  return `/${glyph}`;
}
