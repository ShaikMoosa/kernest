import test from "node:test";
import assert from "node:assert/strict";
import { familyLayout, suggestedInstances, wrapRows } from "../src/lib/onboarding.ts";

test("weight-width setup has four different corners including the default", () => {
  const layout = familyLayout("weight-width");
  assert.equal(layout.axes.length, 2);
  assert.equal(layout.masters.length, 4);
  assert.deepEqual(layout.masters.map(m => [m.location.wght, m.location.wdth]), [[100,75],[900,75],[100,125],[900,125]]);
  assert.ok(layout.masters.some(m => layout.axes.every(a => m.location[a.tag] === a.default)));
});
test("single style is independent of a previous family and suggestions stay in range", () => {
  const layout = familyLayout("single");
  assert.equal(layout.axes.length, 0);
  assert.equal(layout.masters.length, 1);
  assert.deepEqual(layout.masters[0].location, {});
  const axes = [{name:"Weight",tag:"wght",min:300,default:300,max:700}];
  const suggestions = suggestedInstances(axes);
  assert.ok(suggestions.some(i => i.name === "Regular" && i.location.wght === 400));
  assert.ok(suggestions.every(i => i.location.wght >= 300 && i.location.wght <= 700));
});
test("text wraps by advance, preserves explicit empty lines and keeps wide glyphs", () => {
  assert.deepEqual(wrapRows([[500,500,500,500],[],[2000]],1000,x=>x), [[500,500],[500,500],[],[2000]]);
});
test("shaped text never breaks a cluster between its base and marks", () => {
  const glyphs = [{cluster:0,advance:600},{cluster:1,advance:600},{cluster:1,advance:0},{cluster:2,advance:400}];
  const rows = wrapRows([glyphs], 1000, g=>g.advance, g=>g.cluster);
  assert.deepEqual(rows.map(r=>r.map(g=>g.cluster)), [[0],[1,1,2]]);
});
