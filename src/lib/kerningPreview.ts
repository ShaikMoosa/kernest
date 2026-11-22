/** How the text view shows kerning (SMO-34): off, on, or on with spacing locked. */
export type KerningPreview = "off" | "on" | "locked";

const KEY = "kernest.kerningPreview";

export function loadKerningPreview(): KerningPreview {
  try {
    const v = localStorage.getItem(KEY);
    return v === "off" || v === "locked" ? v : "on";
  } catch {
    return "on";
  }
}

export function saveKerningPreview(v: KerningPreview): void {
  try { localStorage.setItem(KEY, v); } catch { /* a convenience only */ }
}

/**
 * The bar showing a pair's kerning, in pixels: it covers the gap the kerning
 * opens (positive) or the overlap it makes (negative) after the left glyph's
 * advance. `x` is the left edge of the glyph's box. In right-to-left text the
 * kerning sits before the glyph's box, on its left.
 */
export function kernBar(x: number, width: number, kern: number, rtl: boolean, scale: number): { x: number; w: number } {
  const w = Math.abs(kern) * scale;
  if (rtl) return { x: x + Math.min(-kern, 0) * scale, w };
  return { x: x + (width + Math.min(kern, 0)) * scale, w };
}

/** Feature settings for the shaped preview: kerning off replaces any kern setting. */
export function withKerning(settings: string[], preview: KerningPreview): string[] {
  if (preview !== "off") return settings;
  return [...settings.filter((s) => s.split("=")[0].trim() !== "kern"), "kern=0"];
}
