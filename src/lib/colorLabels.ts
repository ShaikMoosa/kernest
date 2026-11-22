/** Colour labels on glyphs and layers (Glyphs Handbook 5.3.5): Glyphs' twelve colours
 *  by index, or a colour of a label's own as RGBA. */

export type ColorLabel = number | [number, number, number, number];

/** Glyphs' twelve label colours, by index: name and RGB. Same as Rust COLOR_LABELS. */
export const COLOR_LABELS: [string, [number, number, number]][] = [
  ["Red", [247, 74, 74]],
  ["Orange", [247, 151, 54]],
  ["Brown", [168, 115, 66]],
  ["Yellow", [247, 222, 54]],
  ["Light Green", [140, 214, 87]],
  ["Dark Green", [58, 156, 80]],
  ["Cyan", [79, 201, 219]],
  ["Blue", [51, 117, 230]],
  ["Purple", [148, 87, 207]],
  ["Pink", [237, 101, 170]],
  ["Light Gray", [189, 189, 189]],
  ["Dark Gray", [110, 110, 110]],
];

export function labelRgba(c: ColorLabel): [number, number, number, number] {
  if (typeof c === "number") {
    const rgb = COLOR_LABELS[c]?.[1] ?? [128, 128, 128];
    return [rgb[0], rgb[1], rgb[2], 255];
  }
  return c;
}

/** A CSS colour for the label, at `alpha` times its own opacity (tints stay readable
 *  on light and dark backgrounds). */
export function labelCss(c: ColorLabel, alpha = 1): string {
  const [r, g, b, a] = labelRgba(c);
  return `rgb(${r} ${g} ${b} / ${((a / 255) * alpha).toFixed(3)})`;
}

/** The colour's English name ("Light Green"), or "Custom colour" for an RGBA label. */
export function labelName(c: ColorLabel): string {
  return typeof c === "number" ? COLOR_LABELS[c]?.[0] ?? "Custom colour" : "Custom colour";
}

/** A stable key for grouping and filtering ("4", or "rgba(1,2,3,255)"). */
export function labelKey(c: ColorLabel): string {
  return typeof c === "number" ? String(c) : `rgba(${c.join(",")})`;
}
