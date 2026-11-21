// Writing direction helpers for the text view (SMO-33). The backend resolves each
// item's direction (kernest_core::bidi); this reorders a wrapped row for display.

const RTL_LETTER = /[֐-ࣿיִ-﷿ﹰ-﻿]|[\u{10800}-\u{10FFF}\u{1E800}-\u{1EFFF}]/u;
const STRONG = /[\p{L}\p{Nd}]/u;

/**
 * The visual order (left to right) of a row's items from their resolved directions:
 * runs of one direction, right-to-left runs reversed, and the run order reversed in a
 * right-to-left paragraph.
 */
export function visualOrder(rtl: boolean[], paragraphRtl: boolean): number[] {
  const runs: { start: number; end: number; rtl: boolean }[] = [];
  rtl.forEach((r, i) => {
    const last = runs.at(-1);
    if (last && last.rtl === r) last.end = i + 1;
    else runs.push({ start: i, end: i + 1, rtl: r });
  });
  if (paragraphRtl) runs.reverse();
  return runs.flatMap((run) => {
    const ids = Array.from({ length: run.end - run.start }, (_, k) => run.start + k);
    return run.rtl ? ids.reverse() : ids;
  });
}

/**
 * Whether the text's first strong character (a letter or digit) is right to left;
 * `null` when it has none. `/glyphname` tokens are skipped.
 */
export function firstStrongRtl(text: string): boolean | null {
  for (const ch of text.replace(/\/[^\s/]+/g, " ")) {
    if (RTL_LETTER.test(ch) && /\p{L}/u.test(ch)) return true;
    if (STRONG.test(ch)) return false;
  }
  return null;
}
