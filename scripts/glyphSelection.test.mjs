// Handbook 5.5.1 (Selecting Glyphs) and 5.4.4 (Copy Glyph Names), SMO-78.
import test from "node:test";
import assert from "node:assert/strict";
import { clickSelection, commandSelection } from "../src/lib/glyphSelection.ts";
import { formatGlyphNames } from "../src/lib/glyphNames.ts";

const shown = ["A", "B", "C", "D", "E", "F"];
const plain = { ctrl: false, shift: false };
const ctrl = { ctrl: true, shift: false };
const shift = { ctrl: false, shift: true };
const list = (s) => [...s];

test("a plain click selects just that glyph", () => {
  assert.deepEqual(list(clickSelection(new Set(["A", "B"]), shown, "D", plain)), ["D"]);
});

test("Ctrl+click adds a glyph or takes it out", () => {
  const two = clickSelection(new Set(["A"]), shown, "C", ctrl);
  assert.deepEqual(list(two), ["A", "C"]);
  assert.deepEqual(list(clickSelection(two, shown, "A", ctrl)), ["C"]);
});

test("Shift+click adds the range from the last selected glyph, either way", () => {
  assert.deepEqual(list(clickSelection(new Set(["B"]), shown, "E", shift)).sort(), ["B", "C", "D", "E"]);
  assert.deepEqual(list(clickSelection(new Set(["E"]), shown, "B", shift)).sort(), ["B", "C", "D", "E"]);
  // Nothing selected yet: Shift+click is a plain click.
  assert.deepEqual(list(clickSelection(new Set(), shown, "C", shift)), ["C"]);
  // The last glyph is hidden by a search: just that glyph.
  assert.deepEqual(list(clickSelection(new Set(["Z"]), shown, "C", shift)), ["C"]);
});

test("Select All, Deselect All and Invert Selection work on the glyphs shown", () => {
  const filtered = ["B", "D", "F"];
  assert.deepEqual(list(commandSelection("select-all", new Set(), filtered)), filtered);
  assert.deepEqual(list(commandSelection("deselect-all", new Set(shown), filtered)), []);
  assert.deepEqual(list(commandSelection("invert-selection", new Set(["B", "C"]), filtered)), ["D", "F"]);
});

test("a large selection is handled in one go", () => {
  const many = Array.from({ length: 5000 }, (_, i) => `g${i}`);
  const all = commandSelection("select-all", new Set(), many);
  assert.equal(all.size, 5000);
  assert.equal(clickSelection(new Set(["g10"]), many, "g4009", shift).size, 4000);
  assert.equal(commandSelection("invert-selection", new Set(many.slice(0, 1000)), many).size, 4000);
});

const glyphs = [
  { name: "A", unicode: "0041" },
  { name: "a.ss01", unicode: null },
  { name: "f_f", unicode: null },
  { name: "Aacute", unicode: "00C1" },
];

test("Copy Glyph Names writes all six formats", () => {
  assert.equal(formatGlyphNames(glyphs, "space"), "A a.ss01 f_f Aacute");
  assert.equal(formatGlyphNames(glyphs, "comma"), "A, a.ss01, f_f, Aacute");
  assert.equal(formatGlyphNames(glyphs, "lines"), "A\na.ss01\nf_f\nAacute");
  assert.equal(formatGlyphNames(glyphs, "slash"), "/A/a.ss01/f_f/Aacute");
  assert.equal(formatGlyphNames(glyphs, "python"), "['A', 'a.ss01', 'f_f', 'Aacute']");
  // Unencoded glyphs have no codepoint to list.
  assert.equal(formatGlyphNames(glyphs, "unicode"), "U+0041 U+00C1");
});

test("Copy Glyph Names with nothing selected is empty", () => {
  for (const f of ["space", "comma", "lines", "slash", "unicode"]) assert.equal(formatGlyphNames([], f), "");
  assert.equal(formatGlyphNames([], "python"), "[]");
});
