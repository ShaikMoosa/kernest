import test from "node:test";
import assert from "node:assert/strict";
import { kernBar, withKerning } from "../src/lib/kerningPreview.ts";

test("a negative kern covers the overlap at the end of the left glyph", () => {
  // A 600-unit glyph at x = 100, kern -60, 0.1 px per unit.
  const b = kernBar(100, 600, -60, false, 0.1);
  assert.equal(b.x, 154);
  assert.equal(b.w, 6);
});

test("a positive kern covers the gap after the left glyph", () => {
  const b = kernBar(100, 600, 40, false, 0.1);
  assert.equal(b.x, 160);
  assert.equal(b.w, 4);
});

test("right-to-left kerning sits before the glyph box", () => {
  assert.deepEqual(kernBar(100, 600, -60, true, 0.1), { x: 100, w: 6 });
  assert.deepEqual(kernBar(100, 600, 40, true, 0.1), { x: 96, w: 4 });
});

test("kerning off turns the kern feature off and replaces any setting", () => {
  assert.deepEqual(withKerning(["kern=1", "liga=1"], "off"), ["liga=1", "kern=0"]);
  assert.deepEqual(withKerning(["liga=1"], "off"), ["liga=1", "kern=0"]);
  assert.deepEqual(withKerning(["kern=1"], "on"), ["kern=1"]);
  assert.deepEqual(withKerning(["kern=1"], "locked"), ["kern=1"]);
});
