/** Magnetic snapping while dragging (Glyphs Handbook 3.6.1). */

export interface SnapResult {
  x: number;
  y: number;
  /** The vertical line snapped to (an x), if any: drawn as feedback. */
  lineX?: number;
  /** The horizontal line snapped to (a y), if any. */
  lineY?: number;
}

/** The target in `values` closest to `v`, if one is within `tolerance`. */
function nearest(v: number, values: number[], tolerance: number): number | undefined {
  let best: number | undefined;
  let dist = tolerance;
  for (const t of values) {
    const d = Math.abs(t - v);
    if (d <= dist && (best === undefined || d < dist)) {
      best = t;
      dist = d;
    }
  }
  return best;
}

/**
 * Snaps a dragged point to the closest target on each axis independently: `xs` are
 * vertical lines (other nodes' x, the sidebearings, the middle of the width), `ys`
 * horizontal ones (other nodes' y, the metric lines). `tolerance` is in font units,
 * so callers divide a screen distance by the zoom.
 */
export function magneticSnap(p: { x: number; y: number }, xs: number[], ys: number[], tolerance: number): SnapResult {
  const out: SnapResult = { x: p.x, y: p.y };
  if (tolerance <= 0) return out;
  const sx = nearest(p.x, xs, tolerance);
  const sy = nearest(p.y, ys, tolerance);
  if (sx !== undefined) { out.x = sx; out.lineX = sx; }
  if (sy !== undefined) { out.y = sy; out.lineY = sy; }
  return out;
}
