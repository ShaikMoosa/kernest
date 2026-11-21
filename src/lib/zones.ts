/** Highlighting nodes on metric lines and in alignment zones (Glyphs Handbook 3.2.6). */

export interface ZoneBand { position: number; size: number }

/**
 * How a node's height relates to the vertical metrics:
 * - "line": exactly on a metric line (baseline, x-height, cap height, ascender, descender);
 * - "zone": inside an alignment zone (an overshoot that will be hinted flat);
 * - "near": a few units outside a zone or off a metric line, probably a slip;
 * - null: none of these.
 */
export type ZoneMark = "line" | "zone" | "near" | null;

/** How far outside a zone or off a line still counts as a near miss (font units). */
export const NEAR_MISS = 3;

const EPS = 0.01;

/** A zone's lower and upper edges, whichever way its size points. */
export function zoneRange(z: ZoneBand): [number, number] {
  return z.size < 0 ? [z.position + z.size, z.position] : [z.position, z.position + z.size];
}

export function classifyHeight(y: number, lines: number[], zones: ZoneBand[], near = NEAR_MISS): ZoneMark {
  if (lines.some((l) => Math.abs(y - l) < EPS)) return "line";
  const ranges = zones.filter((z) => z.size !== 0).map(zoneRange);
  if (ranges.some(([lo, hi]) => y >= lo - EPS && y <= hi + EPS)) return "zone";
  const offZone = ranges.some(([lo, hi]) => (y < lo && lo - y <= near) || (y > hi && y - hi <= near));
  const offLine = lines.some((l) => Math.abs(y - l) <= near);
  return offZone || offLine ? "near" : null;
}
