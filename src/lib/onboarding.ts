import type { Axis, Instance, Location } from "./backend";

export type FamilyMode = "single" | "weight" | "weight-width" | "custom";
export interface FamilyLayout { axes: Axis[]; masters: { name: string; location: Location }[] }

export function familyLayout(mode: FamilyMode): FamilyLayout {
  if (mode === "single") return { axes: [], masters: [{ name: "Regular", location: {} }] };
  const axes: Axis[] = [{ name: "Weight", tag: "wght", min: 100, default: 100, max: 900 }];
  if (mode === "weight-width") {
    axes.push({ name: "Width", tag: "wdth", min: 75, default: 75, max: 125 });
    return { axes, masters: [
      { name: "Light Condensed", location: { wght: 100, wdth: 75 } },
      { name: "Bold Condensed", location: { wght: 900, wdth: 75 } },
      { name: "Light Extended", location: { wght: 100, wdth: 125 } },
      { name: "Bold Extended", location: { wght: 900, wdth: 125 } },
    ] };
  }
  return { axes, masters: [{ name: "Light", location: { wght: 100 } }, { name: "Bold", location: { wght: 900 } }] };
}

export function suggestedInstances(axes: Axis[]): Instance[] {
  const base = Object.fromEntries(axes.map(a => [a.tag, a.tag === "wdth" ? Math.max(a.min, Math.min(a.max, 100)) : a.default]));
  const weight = axes.find(a => a.tag === "wght");
  if (!weight) return [{ name: "Regular", location: base, export: true }];
  const weights: [string, number][] = [["Thin", 100], ["ExtraLight", 200], ["Light", 300], ["Regular", 400], ["Medium", 500], ["SemiBold", 600], ["Bold", 700], ["ExtraBold", 800], ["Black", 900]];
  const suggestions = weights.filter(([, value]) => value >= weight.min && value <= weight.max)
    .map(([name, value]) => ({ name, location: { ...base, wght: value }, export: true }));
  return suggestions.length ? suggestions : [{ name: "Regular", location: base, export: true }];
}

export function wrapRows<T>(rows: T[][], width: number, advance: (item: T) => number, cluster?: (item: T) => unknown): T[][] {
  return rows.flatMap(items => {
    const groups: T[][] = [];
    for (const item of items) {
      const last = groups.at(-1);
      if (cluster && last && cluster(last[0]) === cluster(item)) last.push(item);
      else groups.push([item]);
    }
    const lines: T[][] = [[]];
    let used = 0;
    for (const group of groups) {
      const step = group.reduce((n, item) => n + advance(item), 0);
      if (lines.at(-1)!.length && used + step > width) { lines.push([]); used = 0; }
      lines.at(-1)!.push(...group);
      used += step;
    }
    return lines;
  });
}
