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

/** The user value an instance sits at on an axis. */
function userAt(axis: Axis, instance: Instance): number {
  const design = instance.location[axis.tag];
  return design == null ? axis.default : designToUser(axis, design);
}

/**
 * Whether a label is elidable: set on the label itself, or by an instance at its
 * value that lists the axis ("Elidable STAT Axis Value Name"), as export does.
 */
function isElidable(axis: Axis, label: AxisValue, instances: Instance[]): boolean {
  if (label.elidable) return true;
  return instances.some((i) =>
    (i.names?.elidableStatAxes ?? []).includes(axis.tag) && Math.abs(userAt(axis, i) - label.value) < 1e-6);
}

/** The STAT style name for an instance, or its own name when no axis has labels. */
export function statStyleName(axes: Axis[], instance: Instance, instances: Instance[] = [instance]): string {
  if (!hasLabels(axes)) return instance.name;
  const parts: string[] = [];
  for (const axis of axes) {
    if ((axis.labels?.length ?? 0) === 0) continue;
    const label = matchLabel(axis, userAt(axis, instance));
    if (label && !isElidable(axis, label, instances)) parts.push(label.name);
  }
  return parts.length > 0 ? parts.join(" ") : "Regular";
}
