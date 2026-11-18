import { backend, type ClipGlyph } from "./backend";

// Whole-glyph clipboard shared by Font View and the Edit menu; mirrored to the
// system clipboard as JSON so copies survive between windows and fonts.
const GLYPH_MARK = "kernest-glyphs/1";
let inApp: ClipGlyph[] | null = null;

export async function copyGlyphsToClipboard(names: string[]): Promise<number> {
  const glyphs = await backend.copyGlyphs(names);
  inApp = glyphs;
  navigator.clipboard?.writeText(JSON.stringify({ format: GLYPH_MARK, glyphs })).catch(() => {});
  return glyphs.length;
}

export async function readGlyphClipboard(): Promise<ClipGlyph[] | null> {
  try {
    const text = await navigator.clipboard.readText();
    const data = JSON.parse(text);
    if (data?.format === GLYPH_MARK && Array.isArray(data.glyphs) && data.glyphs.length > 0) return data.glyphs;
  } catch {
    // Not our data, or the clipboard is unavailable: fall back to the in-app copy.
  }
  return inApp;
}