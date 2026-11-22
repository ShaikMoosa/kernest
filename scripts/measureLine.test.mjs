import test from "node:test";
import assert from "node:assert/strict";
import { formatDistance, guideSegment, measureSpans } from "../src/lib/measureLine.ts";

test("an n-like glyph gets both sidebearings, both stems and the counter", () => {
  // Stems 80–170 and 510–600 in a 680-unit glyph, as in the sample H below its bar.
  const spans = measureSpans([510, 80, 600, 170], 680);
  assert.deepEqual(spans.map((s) => [s.x1 - s.x0, s.kind]), [
    [80, "side"], [90, "inside"], [340, "inside"], [90, "inside"], [80, "side"],
  ]);
});

test("a glyph the line misses gets no labels", () => {
  assert.deepEqual(measureSpans([], 500), []);
});

test("distances are whole numbers unless they have a fraction", () => {
  assert.equal(formatDistance(88), "88");
  assert.equal(formatDistance(88.04), "88");
  assert.equal(formatDistance(87.66), "87.7");
});

test("a guide's segment runs along its angle through its position", () => {
  const h = guideSegment({ x: 0, y: 250, angle: 0 }, 1000);
  assert.deepEqual(h, { p0: [-1000, 250], p1: [1000, 250] });
  const v = guideSegment({ x: 300, y: 0, angle: 90 }, 1000);
  assert.ok(Math.abs(v.p0[0] - 300) < 1e-9 && Math.abs(v.p0[1] + 1000) < 1e-9);
  assert.ok(Math.abs(v.p1[0] - 300) < 1e-9 && Math.abs(v.p1[1] - 1000) < 1e-9);
});
