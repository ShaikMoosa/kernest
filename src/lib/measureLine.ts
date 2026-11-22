// SMO-37 · Measuring at a height across text, and along measurement guides.

/** One labelled span of the measurement line inside a glyph, in font units. */
export interface Span {
  x0: number;
  x1: number;
  /** "side": from the glyph's edge to its outline; "inside": between two crossings. */
  kind: "side" | "inside";
}

/**
 * The spans the text tab's measurement line labels for one glyph: from the left
 * edge (x = 0) to the first crossing, between each pair of consecutive crossings
 * (stems and counters), and from the last crossing to the advance width. A glyph
 * the line doesn't cross gets no labels.
 */
export function measureSpans(crossings: number[], width: number): Span[] {
  if (crossings.length === 0) return [];
  const xs = [...crossings].sort((a, b) => a - b);
  const out: Span[] = [{ x0: 0, x1: xs[0], kind: "side" }];
  for (let i = 0; i + 1 < xs.length; i++) out.push({ x0: xs[i], x1: xs[i + 1], kind: "inside" });
  out.push({ x0: xs[xs.length - 1], x1: width, kind: "side" });
  return out;
}

/** A label's text: whole units, or one decimal when the distance isn't whole. */
export function formatDistance(d: number): string {
  const r = Math.round(d * 10) / 10;
  return Number.isInteger(r) ? String(r) : r.toFixed(1);
}

/**
 * The two ends of a long segment along a guide, far enough to cross any outline:
 * the guide passes through (x, y) at `angle` degrees, counter-clockwise from the x axis.
 */
export function guideSegment(g: { x: number; y: number; angle: number }, reach = 20000): { p0: [number, number]; p1: [number, number] } {
  const a = (g.angle * Math.PI) / 180;
  const dx = Math.cos(a) * reach, dy = Math.sin(a) * reach;
  return { p0: [g.x - dx, g.y - dy], p1: [g.x + dx, g.y + dy] };
}

const HEIGHT_KEY = "kernest.measureHeight";
const SHOW_KEY = "kernest.measureLine";

export function loadMeasureLine(): { show: boolean; height: number | null } {
  try {
    const h = Number(localStorage.getItem(HEIGHT_KEY));
    return { show: localStorage.getItem(SHOW_KEY) === "1", height: localStorage.getItem(HEIGHT_KEY) !== null && Number.isFinite(h) ? h : null };
  } catch {
    return { show: false, height: null };
  }
}

export function saveMeasureLine(show: boolean, height: number | null): void {
  try {
    localStorage.setItem(SHOW_KEY, show ? "1" : "0");
    if (height !== null) localStorage.setItem(HEIGHT_KEY, String(Math.round(height)));
  } catch { /* a convenience only */ }
}
