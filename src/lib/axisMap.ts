import type { Axis } from "./backend";

// User ↔ design coordinate maths for an axis map (avar), mirroring kernest-core's
// designspace so editor readouts match the exported font. min/default/max are user
// coordinates; masters and instances live in design space.

type Point = [number, number];

/** Piecewise-linear through `pts` (sorted by input), clamped at the ends like fontTools. */
function mapValue(pts: Point[], v: number): number {
  if (pts.length === 0) return v;
  if (v <= pts[0][0]) return pts[0][1];
  for (let i = 0; i < pts.length - 1; i++) {
    const [a, oa] = pts[i];
    const [b, ob] = pts[i + 1];
    if (v <= b) return b === a ? ob : oa + ((v - a) / (b - a)) * (ob - oa);
  }
  return pts[pts.length - 1][1];
}

/** User → design through the axis map (identity when there is no map). */
export function userToDesign(axis: Axis, v: number): number {
  return mapValue(axis.map ?? [], v);
}

/** Design → user: the inverse map, for showing design locations in user terms. */
export function designToUser(axis: Axis, v: number): number {
  const inverted: Point[] = (axis.map ?? []).map(([u, d]) => [d, u]);
  return mapValue(inverted, v);
}

/** The axis extremes in design coordinates: min/default/max through the map.
 *  Masters and instances live in design space, so this is what interpolation uses. */
export function designBounds(axis: Axis): [number, number, number] {
  return [
    userToDesign(axis, axis.min),
    userToDesign(axis, axis.default),
    userToDesign(axis, axis.max),
  ];
}

/** True when the axis bends user away from design (a non-empty, non-identity map). */
export function hasMap(axis: Axis): boolean {
  return (axis.map?.length ?? 0) > 0;
}