import test from "node:test";
import assert from "node:assert/strict";
import { findTextShortcut, SHORTCUT_ACTIONS } from "../src/lib/shortcuts.ts";

const key = (k, mods = {}) => ({ key: k, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods });

test("Alt+Left decreases kerning by 1, and by 10 with Shift", () => {
  assert.deepEqual(findTextShortcut(key("ArrowLeft", { altKey: true }), {}), { id: "text-kern-decrease", step: 1 });
  assert.deepEqual(findTextShortcut(key("ArrowLeft", { altKey: true, shiftKey: true }), {}), { id: "text-kern-decrease", step: 10 });
  assert.deepEqual(findTextShortcut(key("ArrowRight", { altKey: true }), {}), { id: "text-kern-increase", step: 1 });
});

test("Ctrl changes the right sidebearing, Ctrl+Alt the left one", () => {
  assert.deepEqual(findTextShortcut(key("ArrowRight", { ctrlKey: true }), {}), { id: "text-rsb-increase", step: 1 });
  assert.deepEqual(findTextShortcut(key("ArrowLeft", { ctrlKey: true, shiftKey: true }), {}), { id: "text-rsb-decrease", step: 10 });
  assert.deepEqual(findTextShortcut(key("ArrowLeft", { ctrlKey: true, altKey: true }), {}), { id: "text-lsb-increase", step: 1 });
  assert.deepEqual(findTextShortcut(key("ArrowRight", { ctrlKey: true, altKey: true, shiftKey: true }), {}), { id: "text-lsb-decrease", step: 10 });
});

test("Ctrl+Alt+Up and Down move the outline; Ctrl+Alt+E toggles an exception", () => {
  assert.deepEqual(findTextShortcut(key("ArrowUp", { ctrlKey: true, altKey: true }), {}), { id: "text-shift-right", step: 1 });
  assert.deepEqual(findTextShortcut(key("ArrowDown", { ctrlKey: true, altKey: true, shiftKey: true }), {}), { id: "text-shift-left", step: 10 });
  assert.equal(findTextShortcut(key("e", { ctrlKey: true, altKey: true }), {})?.id, "text-kern-exception");
});

test("the writing direction keys need Shift and don't clash with spacing", () => {
  assert.equal(findTextShortcut(key("L", { ctrlKey: true, altKey: true, shiftKey: true }), {})?.id, "text-ltr");
  assert.equal(findTextShortcut(key("R", { ctrlKey: true, altKey: true, shiftKey: true }), {})?.id, "text-rtl");
  assert.equal(findTextShortcut(key("V", { ctrlKey: true, altKey: true, shiftKey: true }), {})?.id, "text-vertical");
  assert.equal(findTextShortcut(key("l", { ctrlKey: true, altKey: true }), {}), null);
});

test("plain arrows and other keys are not text shortcuts", () => {
  assert.equal(findTextShortcut(key("ArrowLeft"), {}), null);
  assert.equal(findTextShortcut(key("ArrowLeft", { metaKey: true, altKey: true }), {}), null);
});

test("a rebinding in Settings › Shortcuts replaces the default", () => {
  const overrides = { "text-kern-decrease": "Alt+Comma" };
  assert.equal(findTextShortcut(key("ArrowLeft", { altKey: true }), overrides), null);
  assert.deepEqual(findTextShortcut(key(",", { altKey: true }), overrides), { id: "text-kern-decrease", step: 1 });
});

test("no two default shortcuts share keys", () => {
  const seen = new Map();
  for (const a of SHORTCUT_ACTIONS) {
    const k = a.default.toLowerCase();
    assert.ok(!seen.has(k), `${a.id} and ${seen.get(k)} both use ${a.default}`);
    seen.set(k, a.id);
  }
});
