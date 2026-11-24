/** Font View glyph selection (Handbook 5.5.1), kept apart from the component so it can be tested. */

export type SelectionCommand = "select-all" | "deselect-all" | "invert-selection";

/** The selection after clicking `name`: Ctrl toggles it, Shift adds the range from the
 * last selected glyph, a plain click selects just it. `shown` is the visible order. */
export function clickSelection(
  prev: ReadonlySet<string>,
  shown: readonly string[],
  name: string,
  mods: { ctrl: boolean; shift: boolean },
): Set<string> {
  if (mods.ctrl) {
    const next = new Set(prev);
    if (next.has(name)) next.delete(name);
    else next.add(name);
    return next;
  }
  if (mods.shift && prev.size > 0) {
    const last = [...prev].pop()!;
    const [a, b] = [shown.indexOf(last), shown.indexOf(name)].sort((x, y) => x - y);
    if (a >= 0) return new Set([...prev, ...shown.slice(a, b + 1)]);
  }
  return new Set([name]);
}

/** Edit › Select All, Deselect All and Invert Selection, over the glyphs shown. */
export function commandSelection(cmd: SelectionCommand, prev: ReadonlySet<string>, shown: readonly string[]): Set<string> {
  switch (cmd) {
    case "select-all":
      return new Set(shown);
    case "deselect-all":
      return new Set();
    case "invert-selection":
      return new Set(shown.filter((n) => !prev.has(n)));
  }
}
