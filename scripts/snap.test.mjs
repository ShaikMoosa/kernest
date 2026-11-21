import test from "node:test";
import assert from "node:assert/strict";
import { magneticSnap } from "../src/lib/snap.ts";

const ys = [0, 500, 700, 750, -250];
const xs = [0, 300, 600];

test("a node dragged to y = 2 lands on the baseline", () => {
  const r = magneticSnap({ x: 123, y: 2 }, xs, ys, 6);
  assert.equal(r.y, 0);
  assert.equal(r.lineY, 0);
  assert.equal(r.x, 123);
  assert.equal(r.lineX, undefined);
});

test("the closest target wins and both axes snap on their own", () => {
  const r = magneticSnap({ x: 297, y: 503 }, [295, 300], [500, 506], 6);
  assert.deepEqual([r.x, r.y], [295, 500]);
  assert.deepEqual([r.lineX, r.lineY], [295, 500]);
});

test("out of reach, or with snapping off, nothing moves", () => {
  assert.deepEqual(magneticSnap({ x: 150, y: 250 }, xs, ys, 6), { x: 150, y: 250 });
  assert.deepEqual(magneticSnap({ x: 1, y: 2 }, xs, ys, 0), { x: 1, y: 2 });
  // Zoomed out, the same 6 pixels reach further in font units.
  assert.equal(magneticSnap({ x: 1, y: 20 }, xs, ys, 6 / 0.25).y, 0);
});
