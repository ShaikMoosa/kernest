/** Edit View tool logic (Handbook 3.1, 3.2, 3.8), kept apart from the component so it can be tested. */

import type { EditNode, EditPath } from "./outline";

// Small copies of outline.ts helpers: type-only imports keep this file loadable by the
// Node test runner, which doesn't resolve extensionless sibling imports.
const isOn = (n: EditNode) => n.kind !== "offcurve";
const nodeKey = (pi: number, ni: number) => `${pi}:${ni}`;
const clonePaths = (paths: EditPath[]): EditPath[] =>
  paths.map((p) => ({ ...p, nodes: p.nodes.map((n) => ({ ...n })) }));

export interface Pt {
  x: number;
  y: number;
}

const off = (p: Pt): EditNode => ({ x: p.x, y: p.y, kind: "offcurve", smooth: false });

// ---------- Pen (Draw tool) ----------

/** The path being drawn and the outgoing handle of its last curve point, if any. */
export interface PenState {
  path: number | null;
  pendingOut: Pt | null;
}

export interface PenDown {
  paths: EditPath[];
  pen: PenState;
  /** The path was closed on its first point. */
  closed: boolean;
  /** The node a drag from here pulls handles out of. */
  drag?: { pi: number; ni: number };
}

/**
 * A pen click at `p`: starts a path, adds a corner point (or a curve point after a
 * dragged one), or closes the open path when `nearFirst` says the click hit its
 * first point and it has at least two points. `paths` is changed in place.
 */
export function penDown(paths: EditPath[], pen: PenState, p: Pt, nearFirst: (first: EditNode) => boolean): PenDown {
  const open = pen.path !== null && paths[pen.path] && !paths[pen.path].closed ? paths[pen.path] : null;
  if (!open || pen.path === null) {
    paths.push({ closed: false, nodes: [{ ...p, kind: "line", smooth: false }] });
    return { paths, pen: { path: paths.length - 1, pendingOut: null }, closed: false, drag: { pi: paths.length - 1, ni: 0 } };
  }
  const first = open.nodes[0];
  if (open.nodes.filter(isOn).length >= 2 && nearFirst(first)) {
    if (pen.pendingOut) {
      open.nodes.push(off(pen.pendingOut), off(first));
      first.kind = "curve";
    } else first.kind = "line";
    open.closed = true;
    return { paths, pen: { path: null, pendingOut: null }, closed: true };
  }
  if (pen.pendingOut) open.nodes.push(off(pen.pendingOut), off(p), { ...p, kind: "curve", smooth: false });
  else open.nodes.push({ ...p, kind: "line", smooth: false });
  return { paths, pen: { path: pen.path, pendingOut: null }, closed: false, drag: { pi: pen.path, ni: open.nodes.length - 1 } };
}

/**
 * Dragging from the point just placed at `origin`: it becomes a smooth curve point
 * with handles mirrored around it, the incoming one now and the outgoing one
 * (`pendingOut`) for the next segment. Returns new paths; `base` is left alone.
 */
export function penDrag(base: EditPath[], pi: number, ni: number, origin: Pt, to: Pt): { paths: EditPath[]; pendingOut: Pt } {
  const v = { x: Math.round(to.x - origin.x), y: Math.round(to.y - origin.y) };
  const next = clonePaths(base);
  const p = next[pi];
  let i = ni;
  p.nodes[i].smooth = true;
  const inHandle = { x: origin.x - v.x, y: origin.y - v.y };
  if (i > 0) {
    if (p.nodes[i].kind === "curve") p.nodes[i - 1] = off(inHandle);
    else {
      const prev = p.nodes[i - 1];
      p.nodes.splice(i, 0, off(prev), off(inHandle));
      i += 2;
      p.nodes[i].kind = "curve";
    }
  }
  return { paths: next, pendingOut: { x: origin.x + v.x, y: origin.y + v.y } };
}

// ---------- Selecting ----------

/** Every node of the unlocked paths (Ctrl+A, Edit › Select All). */
export function allNodeKeys(paths: EditPath[]): Set<string> {
  return new Set(paths.flatMap((p, pi) => (p.locked ? [] : p.nodes.map((_, ni) => nodeKey(pi, ni)))));
}

/** Edit › Invert Selection: the unlocked nodes that aren't selected. */
export function invertNodeSelection(paths: EditPath[], selection: ReadonlySet<string>): Set<string> {
  return new Set([...allNodeKeys(paths)].filter((k) => !selection.has(k)));
}

/** Double-clicking a node selects its whole path. */
export function pathNodeKeys(paths: EditPath[], pi: number): Set<string> {
  return new Set(paths[pi]?.nodes.map((_, ni) => nodeKey(pi, ni)) ?? []);
}

/**
 * The selection after clicking the node `hit`: Shift adds or removes it; a plain
 * click selects only it, unless it is already selected (so a drag moves the whole
 * selection). `fresh` says whether other kinds of selection should be cleared.
 */
export function clickNode(selection: ReadonlySet<string>, hit: string, shift: boolean): { selection: Set<string>; fresh: boolean } {
  if (shift) {
    const next = new Set(selection);
    if (next.has(hit)) next.delete(hit);
    else next.add(hit);
    return { selection: next, fresh: false };
  }
  if (selection.has(hit)) return { selection: new Set(selection), fresh: false };
  return { selection: new Set([hit]), fresh: true };
}

// ---------- Moving and converting ----------

const ARROWS: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, 1], ArrowDown: [0, -1] };

/** How far an arrow key moves the selection: 1 unit, 10 with Shift, 100 with Ctrl. */
export function nudgeStep(key: string, mods: { shift: boolean; ctrl: boolean }): [number, number] | null {
  const d = ARROWS[key];
  if (!d) return null;
  const m = mods.ctrl ? 100 : mods.shift ? 10 : 1;
  return [d[0] * m, d[1] * m];
}

/**
 * Enter: switches the selected on-curve nodes between smooth and corner. A corner is
 * never super-smooth. Returns null when nothing changes.
 */
export function toggleSmooth(paths: EditPath[], selection: ReadonlySet<string>): EditPath[] | null {
  const next = clonePaths(paths);
  let changed = false;
  for (const k of selection) {
    const [pi, ni] = k.split(":").map(Number);
    const n = next[pi]?.nodes[ni];
    if (n && isOn(n) && !next[pi].locked) {
      n.smooth = !n.smooth;
      if (!n.smooth) n.g2 = false;
      changed = true;
    }
  }
  return changed ? next : null;
}

// ---------- Panning ----------

/** The canvas view: scale and the screen position of the font origin. */
export interface View {
  s: number;
  ox: number;
  oy: number;
}

/** The Hand tool (or Space): the canvas follows the pointer from where the drag began. */
export function panTo(start: View, from: Pt, to: Pt): View {
  return { ...start, ox: start.ox + to.x - from.x, oy: start.oy + to.y - from.y };
}

/**
 * The mouse wheel without Ctrl or Alt (those zoom): scrolling pans vertically, Shift
 * turns it into horizontal panning, and a trackpad's sideways scroll pans sideways.
 */
export function wheelPan(view: View, e: { deltaX: number; deltaY: number; shiftKey: boolean }): View {
  if (e.shiftKey) return { ...view, ox: view.ox - e.deltaY };
  return { ...view, ox: view.ox - e.deltaX, oy: view.oy - e.deltaY };
}
