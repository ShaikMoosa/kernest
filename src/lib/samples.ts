// Settings › Sample strings: the texts offered in the text tab's Samples menu.
// Kept per user in this browser profile (a convenience, like the recent fonts list).

export const DEFAULT_SAMPLES = [
  "HOHOHAVATAR To LT",
  "nnonnoonon HHOHHOOHOH",
  "Hamburgefontsiv",
  "/H/O/H/n/o/n LTLTLT ToTo",
  "/A/V/A/W/A/T/A/Y",
  "0123456789 $€£¥",
];

const KEY = "kernest.sampleStrings";
const EVENT = "kernest:samples";

export function loadSamples(): string[] {
  try {
    const raw = localStorage.getItem(KEY);
    const list = raw ? (JSON.parse(raw) as unknown) : null;
    if (Array.isArray(list) && list.every((s) => typeof s === "string")) return list.length ? list : DEFAULT_SAMPLES;
  } catch {
    // Unreadable storage: fall back to the defaults.
  }
  return DEFAULT_SAMPLES;
}

export function saveSamples(list: string[]) {
  const clean = list.map((s) => s.trim()).filter(Boolean);
  try {
    if (clean.length) localStorage.setItem(KEY, JSON.stringify(clean));
    else localStorage.removeItem(KEY);
  } catch {
    // A convenience only.
  }
  window.dispatchEvent(new Event(EVENT));
}

/** Calls `on` whenever the list changes (from Settings). Returns the unsubscribe. */
export function onSamplesChanged(on: () => void): () => void {
  window.addEventListener(EVENT, on);
  return () => window.removeEventListener(EVENT, on);
}
