/** Free-form (lasso) selection in the Select tool (Glyphs Handbook 3.2.2). */

export interface LassoPt { x: number; y: number }

/** How a drag selection combines with what's already selected. */
export type SelectMode = "replace" | "add" | "subtract";

/** Shift adds to the selection, Ctrl takes away from it. */
export function selectModeFor(e: { shiftKey: boolean; ctrlKey: boolean }): SelectMode {
  return e.ctrlKey ? "subtract" : e.shiftKey ? "add" : "replace";
}

/** Whether `p` is inside the closed polygon `poly`, by the even-odd rule, so a lasso
 *  that crosses itself leaves out the parts it loops over twice. */
export function insidePolygon(p: LassoPt, poly: LassoPt[]): boolean {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const a = poly[i], b = poly[j];
    if ((a.y > p.y) !== (b.y > p.y) && p.x < ((b.x - a.x) * (p.y - a.y)) / (b.y - a.y) + a.x) inside = !inside;
  }
  return inside;
}

/** Adds a point to a lasso being drawn, skipping ones closer than `minStep` pixels to
 *  the last, so a long drag doesn't keep thousands of points. */
export function extendLasso(points: LassoPt[], p: LassoPt, minStep = 2): LassoPt[] {
  const last = points[points.length - 1];
  if (last && Math.hypot(p.x - last.x, p.y - last.y) < minStep) return points;
  return [...points, p];
}

/** A lasso too small to enclose anything is a click, not a selection. */
export function isLassoClick(points: LassoPt[], minSize = 3): boolean {
  if (points.length < 3) return true;
  const xs = points.map((p) => p.x), ys = points.map((p) => p.y);
  return Math.max(...xs) - Math.min(...xs) < minSize && Math.max(...ys) - Math.min(...ys) < minSize;
}

/** The new selection after a drag that hit `hits`. */
export function combineSelection<T>(current: Iterable<T>, hits: Iterable<T>, mode: SelectMode): Set<T> {
  if (mode === "replace") return new Set(hits);
  const out = new Set(current);
  for (const h of hits) {
    if (mode === "add") out.add(h); else out.delete(h);
  }
  return out;
}
