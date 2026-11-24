// Edit View tools (SMO-76): Handbook 3.1.1, 3.2.1, 3.2.3, 3.2.4, 3.2.17, 3.8.2 and 3.11.1.
import test from "node:test";
import assert from "node:assert/strict";
import {
  allNodeKeys, clickNode, invertNodeSelection, nudgeStep, panTo, pathNodeKeys, penDown, penDrag, toggleSmooth, wheelPan,
} from "../src/lib/editTools.ts";
import { moveSelection, pathsToSvg } from "../src/lib/outline.ts";

const on = (x, y, kind = "line", smooth = false) => ({ x, y, kind, smooth });
const off = (x, y) => ({ x, y, kind: "offcurve", smooth: false });
const list = (s) => [...s].sort();
const never = () => false;

test("3.1.1 Draw: clicks add corner points and clicking the first point closes the path", () => {
  const paths = [];
  let pen = { path: null, pendingOut: null };
  for (const p of [{ x: 0, y: 0 }, { x: 100, y: 0 }, { x: 100, y: 100 }]) {
    const r = penDown(paths, pen, p, never);
    pen = r.pen;
    assert.equal(r.closed, false);
  }
  assert.equal(paths.length, 1);
  assert.deepEqual(paths[0].nodes.map((n) => [n.x, n.y, n.kind]), [[0, 0, "line"], [100, 0, "line"], [100, 100, "line"]]);
  // Clicking near the first point closes the path; the pen is free again.
  const r = penDown(paths, pen, { x: 1, y: 1 }, (first) => first.x === 0 && first.y === 0);
  assert.equal(r.closed, true);
  assert.equal(paths[0].closed, true);
  assert.equal(paths[0].nodes.length, 3, "no extra point at the click");
  assert.deepEqual(r.pen, { path: null, pendingOut: null });
  assert.equal(pathsToSvg(paths), "M0 0L100 0L100 100L0 0Z");
  // The next click starts a new path.
  penDown(paths, r.pen, { x: 300, y: 0 }, never);
  assert.equal(paths.length, 2);
});

test("3.1.1 Draw: a single point can't be closed onto itself", () => {
  const paths = [];
  const first = penDown(paths, { path: null, pendingOut: null }, { x: 0, y: 0 }, () => true);
  const r = penDown(paths, first.pen, { x: 0, y: 0 }, () => true);
  assert.equal(r.closed, false);
  assert.equal(paths[0].nodes.length, 2);
});

test("3.1.1 Draw: dragging makes a smooth curve point with mirrored handles", () => {
  const paths = [];
  let r = penDown(paths, { path: null, pendingOut: null }, { x: 0, y: 0 }, never);
  r = penDown(paths, r.pen, { x: 200, y: 0 }, never);
  // Drag from (200,0) to (200,80): the point gets an incoming handle at (200,-80).
  const d = penDrag(paths, r.drag.pi, r.drag.ni, { x: 200, y: 0 }, { x: 200.4, y: 79.6 });
  assert.deepEqual(d.pendingOut, { x: 200, y: 80 });
  const nodes = d.paths[0].nodes;
  assert.deepEqual(nodes.map((n) => [n.x, n.y, n.kind]), [[0, 0, "line"], [0, 0, "offcurve"], [200, -80, "offcurve"], [200, 0, "curve"]]);
  assert.equal(nodes[3].smooth, true);
  assert.equal(paths[0].nodes.length, 2, "the drag works on a copy");
  // The next click uses the outgoing handle: a curve segment to the new point.
  const next = penDown(d.paths, { path: 0, pendingOut: d.pendingOut }, { x: 400, y: 0 }, never);
  assert.deepEqual(next.paths[0].nodes.slice(4).map((n) => [n.x, n.y, n.kind]), [[200, 80, "offcurve"], [400, 0, "offcurve"], [400, 0, "curve"]]);
  // Dragging again from a point that's already a curve point only moves its handle.
  const again = penDrag(next.paths, 0, 6, { x: 400, y: 0 }, { x: 400, y: 50 });
  assert.equal(again.paths[0].nodes.length, 7);
  assert.deepEqual([again.paths[0].nodes[5].x, again.paths[0].nodes[5].y], [400, -50]);
});

test("3.1.1 Draw: closing after a drag makes the closing segment a curve", () => {
  const paths = [];
  let r = penDown(paths, { path: null, pendingOut: null }, { x: 0, y: 0 }, never);
  r = penDown(paths, r.pen, { x: 100, y: 0 }, never);
  r = penDown(paths, r.pen, { x: 100, y: 100 }, never);
  const d = penDrag(paths, 0, r.drag.ni, { x: 100, y: 100 }, { x: 60, y: 140 });
  const c = penDown(d.paths, { path: 0, pendingOut: d.pendingOut }, { x: 0, y: 0 }, () => true);
  assert.equal(c.closed, true);
  assert.equal(c.paths[0].nodes[0].kind, "curve");
  assert.deepEqual(c.paths[0].nodes.slice(-2).map((n) => [n.x, n.y]), [[60, 140], [0, 0]]);
});

const square = { closed: true, nodes: [on(0, 0), on(100, 0), on(100, 100), on(0, 100)] };
const locked = { closed: true, locked: true, nodes: [on(200, 0), on(300, 0), on(300, 100)] };

test("3.2.1 Selecting: click, Shift+click, double-click, Select All and Invert", () => {
  let r = clickNode(new Set(["0:0", "0:1"]), "0:2", false);
  assert.deepEqual([list(r.selection), r.fresh], [["0:2"], true]);
  // Clicking a selected node keeps the selection, so dragging moves all of it.
  r = clickNode(new Set(["0:0", "0:1"]), "0:1", false);
  assert.deepEqual([list(r.selection), r.fresh], [["0:0", "0:1"], false]);
  // Shift adds a node or takes it out.
  assert.deepEqual(list(clickNode(new Set(["0:0"]), "0:3", true).selection), ["0:0", "0:3"]);
  assert.deepEqual(list(clickNode(new Set(["0:0", "0:3"]), "0:3", true).selection), ["0:0"]);
  // Double-clicking a node selects its whole path.
  assert.deepEqual(list(pathNodeKeys([square, locked], 0)), ["0:0", "0:1", "0:2", "0:3"]);
  assert.equal(pathNodeKeys([square], 5).size, 0);
});

test("3.2.17 Locking: locked paths stay out of Select All and Invert Selection", () => {
  const paths = [square, locked];
  assert.deepEqual(list(allNodeKeys(paths)), ["0:0", "0:1", "0:2", "0:3"]);
  assert.deepEqual(list(invertNodeSelection(paths, new Set(["0:0", "0:2"]))), ["0:1", "0:3"]);
  // Enter leaves a locked path's nodes alone even if they were selected somehow.
  const curvy = { closed: true, locked: true, nodes: [on(0, 0, "curve", true), off(10, 0), off(20, 0)] };
  assert.equal(toggleSmooth([curvy], new Set(["0:0"])), null);
});

test("3.2.3 Moving: arrow keys move by 1, Shift by 10, Ctrl by 100", () => {
  assert.deepEqual(nudgeStep("ArrowLeft", { shift: false, ctrl: false }), [-1, 0]);
  assert.deepEqual(nudgeStep("ArrowUp", { shift: true, ctrl: false }), [0, 10]);
  assert.deepEqual(nudgeStep("ArrowDown", { shift: false, ctrl: true }), [0, -100]);
  assert.deepEqual(nudgeStep("ArrowRight", { shift: true, ctrl: true }), [100, 0]);
  assert.equal(nudgeStep("a", { shift: false, ctrl: false }), null);
});

test("3.2.3 Moving: a node takes its handles along; a handle moves alone", () => {
  const curve = { closed: false, nodes: [on(0, 0), off(30, 50), off(70, 50), on(100, 0, "curve")] };
  const moved = moveSelection([curve], new Set(["0:3"]), 10, -5);
  assert.deepEqual(moved[0].nodes.map((n) => [n.x, n.y]), [[0, 0], [30, 50], [80, 45], [110, -5]]);
  const handle = moveSelection([curve], new Set(["0:1"]), 0, 20);
  assert.deepEqual(handle[0].nodes.map((n) => [n.x, n.y]), [[0, 0], [30, 70], [70, 50], [100, 0]]);
  assert.deepEqual(curve.nodes[3], on(100, 0, "curve"), "the original is untouched");
});

test("3.2.4 Converting: Enter switches selected points between smooth and corner", () => {
  const p = { closed: true, nodes: [on(0, 0, "curve", false), off(10, 20), off(30, 20), { ...on(40, 0, "curve", true), g2: true }, off(50, -20), off(-10, -20)] };
  const next = toggleSmooth([p], new Set(["0:0", "0:3", "0:1"]));
  assert.equal(next[0].nodes[0].smooth, true);
  assert.equal(next[0].nodes[3].smooth, false);
  assert.equal(next[0].nodes[3].g2, false, "a corner is never super-smooth");
  assert.equal(next[0].nodes[1].smooth, false, "handles are skipped");
  assert.equal(p.nodes[0].smooth, false, "the original is untouched");
  assert.equal(toggleSmooth([p], new Set(["0:1"])), null, "only handles: nothing to do");
});

test("3.8.2 Panning: the Hand tool follows the pointer; the wheel scrolls", () => {
  const view = { s: 0.5, ox: 100, oy: 400 };
  assert.deepEqual(panTo(view, { x: 10, y: 10 }, { x: 60, y: -20 }), { s: 0.5, ox: 150, oy: 370 });
  // Scrolling down moves the drawing up; Shift scrolls sideways; a trackpad does both.
  assert.deepEqual(wheelPan(view, { deltaX: 0, deltaY: 30, shiftKey: false }), { s: 0.5, ox: 100, oy: 370 });
  assert.deepEqual(wheelPan(view, { deltaX: 0, deltaY: 30, shiftKey: true }), { s: 0.5, ox: 70, oy: 400 });
  assert.deepEqual(wheelPan(view, { deltaX: -12, deltaY: 8, shiftKey: false }), { s: 0.5, ox: 112, oy: 392 });
});

test("3.11.1 Info box: typing a node's X or Y moves it there with its handles", () => {
  // The info box's X and Y fields call moveSelection with the difference.
  const curve = { closed: false, nodes: [on(0, 0), off(30, 50), off(70, 50), on(100, 0, "curve")] };
  const node = curve.nodes[3];
  const typedX = 140;
  const moved = moveSelection([curve], new Set(["0:3"]), typedX - node.x, 0);
  assert.deepEqual([moved[0].nodes[3].x, moved[0].nodes[2].x], [140, 110]);
});
