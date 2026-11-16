// Checks the translations: every UI string passed to t() / tn() has a Spanish and an
// Arabic translation with the same {placeholders}, and no translation is left over.
// Also lists English text in components that isn't passed through t() (so it would
// never be translated). Run: npm run check:i18n
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const root = new URL("..", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const files = (dir) =>
  readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    return statSync(p).isDirectory() ? files(p) : /\.(tsx?|mjs)$/.test(f) ? [p] : [];
  });
const sources = files(join(root, "src")).filter((f) => !/[\\/]locales[\\/]/.test(f) && !f.endsWith("backend.ts") && !f.endsWith("i18n.ts"));

// Double-quoted JS string literal; JSON.parse unescapes it.
const STR = String.raw`"((?:[^"\\]|\\.)*)"`;
const unq = (s) => JSON.parse(`"${s}"`);
const keys = new Map(); // key -> "file" (first use)
for (const f of sources) {
  const text = readFileSync(f, "utf8");
  for (const m of text.matchAll(new RegExp(String.raw`\bt\(\s*${STR}`, "g"))) keys.set(unq(m[1]), f);
  for (const m of text.matchAll(new RegExp(String.raw`\btn\([^,]+,\s*${STR}\s*,\s*${STR}`, "g"))) {
    keys.set(unq(m[2]), f);
    keys.set(`${unq(m[1])}\u0000one`, f); // the singular is only English
  }
}
for (const k of [...keys.keys()]) if (k.endsWith("\u0000one")) keys.delete(k);

// Keys used through variables (data shown in the UI), listed in locales/dynamic.ts.
const dyn = readFileSync(join(root, "src/locales/dynamic.ts"), "utf8");
for (const m of dyn.matchAll(new RegExp(STR, "g"))) keys.set(unq(m[1]), "src/locales/dynamic.ts");
// Messages from the Rust side, listed in locales/engine.ts.
const engine = readFileSync(join(root, "src/locales/engine.ts"), "utf8");
for (const m of engine.matchAll(new RegExp(STR, "g"))) keys.set(unq(m[1]), "src/locales/engine.ts");

if (process.argv.includes("--keys")) {
  // Lists every key (for writing a new dictionary).
  for (const [k, f] of keys) console.log(JSON.stringify(k) + "	" + f.replace(root, ""));
  process.exit(0);
}

const ENGLISH_ONLY = new Set(["s", "was", "are"]);
const holes = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(",");
let problems = 0;
const report = (msg) => { problems++; console.log(msg); };

// Dictionaries are TS modules of plain data; load them by stripping the type bits.
async function loadDict(name) {
  const src = readFileSync(join(root, `src/locales/${name}.ts`), "utf8")
    .replace(/^import .*$/m, "")
    .replace(/export const (\w+): Dict =/, "export const $1 =");
  const url = "data:text/javascript;base64," + Buffer.from(src).toString("base64");
  return Object.values(await import(url))[0];
}

for (const lang of ["es", "ar"]) {
  const dict = await loadDict(lang);
  for (const [k, f] of keys) {
    const e = dict[k];
    if (e === undefined) { report(`${lang}: missing ${JSON.stringify(k)}  (${f.replace(root, "")})`); continue; }
    const forms = typeof e === "string" ? [e] : Object.values(e);
    if (typeof e !== "string" && !e.other) report(`${lang}: no "other" form for ${JSON.stringify(k)}`);
    for (const v of forms) {
      // Plural forms may leave out {n} (Arabic "one"/"two" often say the number in words).
      // {s}, {was} and {are} are English grammar filled in by the engine; translations
      // don't use them. {n} may be left out of a plural form.
      const skip = (h) => !h || ENGLISH_ONLY.has(h) || (typeof e !== "string" && h === "n");
      const want = holes(k).split(",").filter((h) => !skip(h)).join(",");
      const got = holes(v).split(",").filter((h) => !skip(h)).join(",");
      if (want !== got) report(`${lang}: placeholders differ in ${JSON.stringify(k)} → ${JSON.stringify(v)}`);
    }
  }
  for (const k of Object.keys(dict)) if (!keys.has(k)) report(`${lang}: unused ${JSON.stringify(k)}`);
}

// English text in components that isn't translated.
for (const f of sources.filter((f) => f.endsWith(".tsx"))) {
  const lines = readFileSync(f, "utf8").split("\n");
  lines.forEach((line, i) => {
    const hits = [
      ...line.matchAll(/>([^<>{}]*[A-Za-z]{2,}[^<>{}]*)</g),
      ...line.matchAll(/\b(?:title|aria-label|placeholder|alt|label)="([^"]*[A-Za-z]{2,}[^"]*)"/g),
    ].map((m) => m[1].trim())// Skip code that looks like text (comparisons, generics, calls) and sample strings.
      .filter((s) => s && !/^(Kernest|Hamb|Aa|TTF|OTF|WOFF2)$/.test(s) && !/[=(&:\\]|Promise|^A\.alt /.test(s));
    for (const h of hits) report(`untranslated: ${f.replace(root, "")}:${i + 1}  ${JSON.stringify(h)}`);
  });
}

console.log(problems ? `\n${problems} problem(s); ${keys.size} strings.` : `Translations complete: ${keys.size} strings in Spanish and Arabic.`);
process.exit(problems ? 1 : 0);
