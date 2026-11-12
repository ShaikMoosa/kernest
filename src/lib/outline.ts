// Outline helpers used while editing: SVG output, node topology and shape builders.
// Coordinates are font units, y up. The Rust core is the source of truth for
// exact metrics; these helpers keep the canvas live during a drag.

export type NodeKind = "line" | "curve" | "qcurve" | "offcurve";

export interface EditNode {
  x: number;
  y: number;
  kind: NodeKind;
  smooth: boolean;
}

export interface EditPath {
  closed: boolean;
  nodes: EditNode[];
}

export const isOn = (n: EditNode) => n.kind !== "offcurve";

const fmt = (v: number) => String(Math.round(v * 100) / 100);

/** SVG path data for a set of paths (font units, y up). Mirrors `geometry::svg_path` in Rust. */
export function pathsToSvg(paths: EditPath[]): string {
  let d = "";
  for (const p of paths) {
    const n = p.nodes.length;
    const start = p.nodes.findIndex(isOn);
    if (start < 0) continue;
    const order = Array.from({ length: n }, (_, i) => p.nodes[(start + i) % n]);
    const pt = (q: EditNode) => `${fmt(q.x)} ${fmt(q.y)}`;
    d += `M${pt(order[0])}`;
    let offs: EditNode[] = [];
    const seq = p.closed ? [...order.slice(1), order[0]] : order.slice(1);
    for (const q of seq) {
      if (!isOn(q)) {
        offs.push(q);
        continue;
      }
      if (offs.length === 0) d += `L${pt(q)}`;
      else if (offs.length === 1) d += `Q${pt(offs[0])} ${pt(q)}`;
      else if (offs.length === 2 && q.kind !== "qcurve") d += `C${pt(offs[0])} ${pt(offs[1])} ${pt(q)}`;
      else {
        for (let i = 0; i < offs.length - 1; i++) {
          const a = offs[i], b = offs[i + 1];
          d += `Q${pt(a)} ${fmt((a.x + b.x) / 2)} ${fmt((a.y + b.y) / 2)}`;
        }
        d += `Q${pt(offs[offs.length - 1])} ${pt(q)}`;
      }
      offs = [];
    }
    if (p.closed) d += "Z";
  }
  return d;
}

export const clonePaths = (paths: EditPath[]): EditPath[] => paths.map((p) => ({ closed: p.closed, nodes: p.nodes.map((n) => ({ ...n })) }));

/** Index of the node before/after `i` in a path, or -1 at the ends of an open path. */
export function neighbour(p: EditPath, i: number, dir: 1 | -1): number {
  const j = i + dir;
  if (j >= 0 && j < p.nodes.length) return j;
  if (!p.closed) return -1;
  return (j + p.nodes.length) % p.nodes.length;
}

/** For an off-curve point, the on-curve point its handle is drawn to. */
export function handleOwner(p: EditPath, i: number): number {
  const prev = neighbour(p, i, -1);
  if (prev >= 0 && isOn(p.nodes[prev])) return prev;
  const next = neighbour(p, i, 1);
  if (next >= 0 && isOn(p.nodes[next])) return next;
  return -1;
}

/** Off-curve points attached to on-curve point `i` (its incoming and outgoing handles). */
export function attachedHandles(p: EditPath, i: number): number[] {
  const out: number[] = [];
  for (const dir of [-1, 1] as const) {
    const j = neighbour(p, i, dir);
    if (j >= 0 && !isOn(p.nodes[j])) out.push(j);
  }
  return out;
}

export const nodeKey = (pi: number, ni: number) => `${pi}:${ni}`;
export const parseKey = (k: string) => k.split(":").map(Number) as [number, number];

/** Moves the selected nodes by (dx, dy). Handles of moved on-curve points move with them. */
export function moveSelection(base: EditPath[], selection: Set<string>, dx: number, dy: number): EditPath[] {
  const out = clonePaths(base);
  const moved = new Set<string>();
  const move = (pi: number, ni: number) => {
    const k = nodeKey(pi, ni);
    if (moved.has(k)) return;
    moved.add(k);
    out[pi].nodes[ni].x = base[pi].nodes[ni].x + dx;
    out[pi].nodes[ni].y = base[pi].nodes[ni].y + dy;
  };
  for (const k of selection) {
    const [pi, ni] = parseKey(k);
    if (!base[pi]?.nodes[ni]) continue;
    move(pi, ni);
    if (isOn(base[pi].nodes[ni])) for (const h of attachedHandles(base[pi], ni)) move(pi, h);
  }
  return out;
}

/** Deletes the selected on-curve points (and their handles). Paths left with fewer than two points are removed. */
export function deleteSelection(base: EditPath[], selection: Set<string>): EditPath[] {
  const result: EditPath[] = [];
  base.forEach((p, pi) => {
    const drop = new Set<number>();
    p.nodes.forEach((n, ni) => {
      if (!selection.has(nodeKey(pi, ni))) return;
      drop.add(ni);
      if (isOn(n)) attachedHandles(p, ni).forEach((h) => drop.add(h));
    });
    if (drop.size === 0) return result.push(p);
    const kept = p.nodes.filter((_, i) => !drop.has(i)).map((n) => ({ ...n }));
    const nodes = normalizeSegments({ closed: p.closed, nodes: kept });
    if (nodes.filter(isOn).length >= 2) result.push({ closed: p.closed, nodes });
  });
  return result;
}

/**
 * Makes every segment valid after points were removed: a cubic keeps exactly two
 * handles, a quadratic run keeps its handles, anything else becomes a straight line.
 */
function normalizeSegments(p: EditPath): EditNode[] {
  const first = p.nodes.findIndex(isOn);
  if (first < 0) return [];
  // Open paths must start on-curve; leading handles there belong to no segment.
  const nodes = p.closed ? [...p.nodes.slice(first), ...p.nodes.slice(0, first)] : p.nodes.slice(first);
  const out: EditNode[] = [nodes[0]];
  let run: EditNode[] = [];
  const close = (n: EditNode) => {
    if (run.length === 2 && n.kind === "curve") return [...run];
    if (run.length >= 1 && n.kind === "qcurve") return [...run];
    n.kind = "line";
    return [];
  };
  for (const n of nodes.slice(1)) {
    if (!isOn(n)) {
      run.push(n);
      continue;
    }
    out.push(...close(n), n);
    run = [];
  }
  if (p.closed) {
    // The trailing run is the segment that ends at the first point.
    out.push(...close(out[0]));
  }
  return out;
}

/** A rectangle path with counter-clockwise (outer) direction. */
export function rectPath(x0: number, y0: number, x1: number, y1: number): EditPath {
  const [l, r] = x0 < x1 ? [x0, x1] : [x1, x0];
  const [b, t] = y0 < y1 ? [y0, y1] : [y1, y0];
  const n = (x: number, y: number): EditNode => ({ x, y, kind: "line", smooth: false });
  return { closed: true, nodes: [n(l, b), n(r, b), n(r, t), n(l, t)] };
}

/** An ellipse inside the box, as four smooth cubic segments, counter-clockwise. */
export function ellipsePath(x0: number, y0: number, x1: number, y1: number): EditPath {
  const cx = (x0 + x1) / 2, cy = (y0 + y1) / 2;
  const rx = Math.abs(x1 - x0) / 2, ry = Math.abs(y1 - y0) / 2;
  const k = 0.5523;
  const r = (v: number) => Math.round(v);
  const on = (x: number, y: number): EditNode => ({ x: r(x), y: r(y), kind: "curve", smooth: true });
  const off = (x: number, y: number): EditNode => ({ x: r(x), y: r(y), kind: "offcurve", smooth: false });
  return {
    closed: true,
    nodes: [
      off(cx + rx, cy + ry * k), off(cx + rx * k, cy + ry), on(cx, cy + ry),
      off(cx - rx * k, cy + ry), off(cx - rx, cy + ry * k), on(cx - rx, cy),
      off(cx - rx, cy - ry * k), off(cx - rx * k, cy - ry), on(cx, cy - ry),
      off(cx + rx * k, cy - ry), off(cx + rx, cy - ry * k), on(cx + rx, cy),
    ],
  };
}

/** Bounds of the points (on- and off-curve). A quick estimate used by the preview mock. */
export function pointBounds(paths: EditPath[]): { minX: number; maxX: number } | null {
  let minX = Infinity, maxX = -Infinity;
  for (const p of paths) for (const n of p.nodes) {
    minX = Math.min(minX, n.x);
    maxX = Math.max(maxX, n.x);
  }
  return minX === Infinity ? null : { minX, maxX };
}
