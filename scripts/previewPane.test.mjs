import test from "node:test";
import assert from "node:assert/strict";
import { clampPreviewHeight, parsePreviewSettings, placeLine, DEFAULT_PREVIEW, MIN_PREVIEW_HEIGHT } from "../src/lib/previewPane.ts";

test("stored settings are read back, and bad values fall back to the defaults", () => {
  assert.deepEqual(parsePreviewSettings(JSON.stringify({ source: "i:Bold", size: 96, invert: true })), { source: "i:Bold", size: 96, invert: true });
  assert.deepEqual(parsePreviewSettings(null), DEFAULT_PREVIEW);
  assert.deepEqual(parsePreviewSettings("not json"), DEFAULT_PREVIEW);
  assert.deepEqual(parsePreviewSettings(JSON.stringify({ source: "x:1", size: 9000, invert: "yes" })), DEFAULT_PREVIEW);
});

test("the pane keeps a minimum height and leaves room for the editor", () => {
  assert.equal(clampPreviewHeight(10, 800), MIN_PREVIEW_HEIGHT);
  assert.equal(clampPreviewHeight(300, 800), 300);
  assert.equal(clampPreviewHeight(700, 800), 560);
  assert.equal(clampPreviewHeight(200, 50), MIN_PREVIEW_HEIGHT);
});

test("a left-to-right line puts kerning after each glyph", () => {
  const items = [{ width: 600, kern: -60, rtl: false }, { width: 500, kern: 0, rtl: false }];
  const r = placeLine(items, [0, 1], 0.1);
  assert.deepEqual(r.x, [0, 54]);
  assert.equal(r.width, 104);
});

test("right-to-left items take their kerning before the glyph", () => {
  // Visual order puts item 1 on the left.
  const items = [{ width: 600, kern: -60, rtl: true }, { width: 500, kern: 0, rtl: true }];
  const r = placeLine(items, [1, 0], 0.1);
  assert.deepEqual(r.x, [44, 0]);
  assert.equal(r.width, 104);
});
