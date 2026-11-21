import test from "node:test";
import assert from "node:assert/strict";
import { classifyHeight, zoneRange } from "../src/lib/zones.ts";

const lines = [0, 500, 700, 750, -250];
const zones = [{ position: 0, size: -15 }, { position: 500, size: 15 }, { position: 700, size: 15 }];

test("nodes on a metric line are highlighted", () => {
  assert.equal(classifyHeight(500, lines, zones), "line");
  assert.equal(classifyHeight(0, lines, zones), "line");
  assert.equal(classifyHeight(-250, lines, []), "line");
});

test("overshoots inside a zone count as in the zone", () => {
  // An "o" overshoot at -12 in a -15 baseline zone.
  assert.equal(classifyHeight(-12, lines, zones), "zone");
  assert.equal(classifyHeight(-15, lines, zones), "zone");
  assert.equal(classifyHeight(512, lines, zones), "zone");
  assert.deepEqual(zoneRange({ position: 0, size: -15 }), [-15, 0]);
});

test("a few units outside a zone or off a line is a near miss", () => {
  assert.equal(classifyHeight(-17, lines, zones), "near", "2 below the baseline zone");
  assert.equal(classifyHeight(517, lines, zones), "near", "2 above the x-height zone");
  assert.equal(classifyHeight(748, lines, zones), "near", "2 below the ascender line");
  assert.equal(classifyHeight(-19, lines, zones), null, "4 off is deliberate");
  assert.equal(classifyHeight(300, lines, zones), null);
});
