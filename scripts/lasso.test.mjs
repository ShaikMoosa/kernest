import test from "node:test";
import assert from "node:assert/strict";
import { insidePolygon, extendLasso, isLassoClick, combineSelection, selectModeFor } from "../src/lib/lasso.ts";

const square = [{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 10, y: 10 }, { x: 0, y: 10 }];

test("points inside a lasso are hit, points outside are not", () => {
  assert.ok(insidePolygon({ x: 5, y: 5 }, square));
  assert.ok(!insidePolygon({ x: 15, y: 5 }, square));
  assert.ok(!insidePolygon({ x: 5, y: -1 }, square));
  // A concave L: the notch is outside.
  const l = [{ x: 0, y: 0 }, { x: 10, y: 0 }, { x: 10, y: 4 }, { x: 4, y: 4 }, { x: 4, y: 10 }, { x: 0, y: 10 }];
  assert.ok(insidePolygon({ x: 2, y: 8 }, l));
  assert.ok(insidePolygon({ x: 8, y: 2 }, l));
  assert.ok(!insidePolygon({ x: 8, y: 8 }, l));
});

test("a self-intersecting lasso uses the even-odd rule", () => {
  // A square drawn twice around: the inside is covered twice, so it's out.
  const twice = [...square, ...square];
  assert.ok(!insidePolygon({ x: 5, y: 5 }, twice));
  // A figure eight: both loops are in.
  const eight = [{ x: 0, y: 0 }, { x: 10, y: 10 }, { x: 10, y: 0 }, { x: 0, y: 10 }];
  assert.ok(insidePolygon({ x: 2, y: 5 }, eight));
  assert.ok(insidePolygon({ x: 8, y: 5 }, eight));
  assert.ok(!insidePolygon({ x: 5, y: 1 }, eight));
});

test("exactly the three nodes inside the loop are selected", () => {
  const nodes = { a: { x: 2, y: 2 }, b: { x: 8, y: 3 }, c: { x: 5, y: 8 }, d: { x: 20, y: 5 }, e: { x: -4, y: 4 } };
  const hits = Object.entries(nodes).filter(([, p]) => insidePolygon(p, square)).map(([k]) => k);
  assert.deepEqual(hits, ["a", "b", "c"]);
});

test("Shift adds, Ctrl subtracts, plain replaces", () => {
  assert.equal(selectModeFor({ shiftKey: false, ctrlKey: false }), "replace");
  assert.equal(selectModeFor({ shiftKey: true, ctrlKey: false }), "add");
  assert.equal(selectModeFor({ shiftKey: false, ctrlKey: true }), "subtract");
  assert.deepEqual([...combineSelection(["a", "b"], ["c"], "replace")], ["c"]);
  assert.deepEqual([...combineSelection(["a", "b"], ["c"], "add")].sort(), ["a", "b", "c"]);
  assert.deepEqual([...combineSelection(["a", "b"], ["b", "c"], "subtract")], ["a"]);
});

test("short drags are clicks and close points are skipped", () => {
  assert.ok(isLassoClick([{ x: 0, y: 0 }, { x: 1, y: 1 }]));
  assert.ok(isLassoClick([{ x: 0, y: 0 }, { x: 1, y: 1 }, { x: 2, y: 0 }]));
  assert.ok(!isLassoClick(square));
  let pts = extendLasso([], { x: 0, y: 0 });
  pts = extendLasso(pts, { x: 1, y: 0 });
  pts = extendLasso(pts, { x: 3, y: 0 });
  assert.deepEqual(pts, [{ x: 0, y: 0 }, { x: 3, y: 0 }]);
});
