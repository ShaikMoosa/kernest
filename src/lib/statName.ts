import type { Axis, AxisValue, Instance } from "./backend";
import { designToUser } from "./axisMap";

// The style name apps show for a variable font location, built from the STAT axis
// value labels (S41). Mirrors how STAT names a location: pick each axis's matching
// value, drop the elidable ones, join the rest. With no labels at all, the instance
// keeps its own name. Instances live in design space; labels are in user space.

const EPS = 0.5;

/** The label naming a user value on an axis: an exact point first, else a covering range. */
function matchLabel(axis: Axis, user: number): AxisValue | undefined {
  const labels = axis.labels ?? [];
  const point = labels.find((l) => Math.abs(l.value - user) <= EPS);
  if (point) return point;
  return labels.find(
    (l) => l.min != null && l.max != null && user >= l.min - EPS && user <= l.max + EPS,
  );
}

/** True when any axis carries STAT value labels. */
export function hasLabels(axes: Axis[]): boolean {
  return axes.some((a) => (a.labels?.length ?? 0) > 0);
}

/** The STAT style name for an instance, or its own name when no axis has labels. */
export function statStyleName(axes: Axis[], instance: Instance): string {
  if (!hasLabels(axes)) return instance.name;
  const parts: string[] = [];
  for (const axis of axes) {
    if ((axis.labels?.length ?? 0) === 0) continue;
    const design = instance.location[axis.tag];
    const user = design == null ? axis.default : designToUser(axis, design);
    const label = matchLabel(axis, user);
    if (label && !label.elidable) parts.push(label.name);
  }
  return parts.length > 0 ? parts.join(" ") : "Regular";
}