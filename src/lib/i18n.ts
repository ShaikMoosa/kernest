/**
 * Translations. The English text is the key: `t("Export")`, `t("Delete {name}?", { name })`.
 * Plurals use the language's own plural rules (Arabic has six forms):
 * `tn(n, "{n} glyph", "{n} glyphs")`, whose key is the English plural form.
 *
 * Messages from the Rust side arrive in English; `translateMessage` looks them up as
 * well, including messages with parts filled in (see the `{…}` templates in the
 * dictionaries).
 */
import { ar } from "../locales/ar";
import { es } from "../locales/es";

export type Lang = "en" | "es" | "ar";

/** A translation: text, or one text per plural category. */
export type Entry = string | Partial<Record<Intl.LDMLPluralRule, string>>;
export type Dict = Record<string, Entry>;

export const LANGUAGES: { id: Lang; name: string }[] = [
  { id: "en", name: "English" },
  { id: "es", name: "Español" },
  { id: "ar", name: "العربية" },
];

const DICTS: Record<Exclude<Lang, "en">, Dict> = { es, ar };
const KEY = "kernest.language";

let current: Lang = "en";

/** The saved language, or Windows' language if Kernest has it, or English. */
export function loadLanguage(): Lang {
  try {
    const saved = localStorage.getItem(KEY);
    if (saved === "en" || saved === "es" || saved === "ar") return saved;
  } catch {
    // Use the system language.
  }
  const sys = (navigator.language || "en").slice(0, 2).toLowerCase();
  return sys === "es" || sys === "ar" ? sys : "en";
}

export function language(): Lang {
  return current;
}

/** Switches the language and the page direction (Arabic is right to left). */
export function applyLanguage(lang: Lang) {
  current = lang;
  document.documentElement.lang = lang;
  document.documentElement.dir = lang === "ar" ? "rtl" : "ltr";
}

export function saveLanguage(lang: Lang) {
  applyLanguage(lang);
  try {
    localStorage.setItem(KEY, lang);
  } catch {
    // The language still applies for this session.
  }
}

const fill = (text: string, vars?: Record<string, string | number>) =>
  vars ? text.replace(/\{(\w+)\}/g, (m, k: string) => (k in vars ? String(vars[k]) : m)) : text;

function lookup(en: string): Entry | undefined {
  return current === "en" ? undefined : DICTS[current][en];
}

/** Translates `en`, filling in `{name}` placeholders from `vars`. */
export function t(en: string, vars?: Record<string, string | number>): string {
  const e = lookup(en);
  const text = typeof e === "string" ? e : (e?.other ?? en);
  return fill(text, vars);
}

/** Translates a count: `one` and `other` are the English forms; `{n}` is the count. */
export function tn(n: number, one: string, other: string, vars?: Record<string, string | number>): string {
  const all = { n, ...vars };
  const e = lookup(other);
  if (e === undefined) return fill(n === 1 ? one : other, all);
  if (typeof e === "string") return fill(e, all);
  const rule = new Intl.PluralRules(current).select(n);
  return fill(e[rule] ?? e.other ?? other, all);
}

// ---------- Messages from the Rust side ----------

interface Template {
  re: RegExp;
  names: string[];
  key: string;
}

const templates = new Map<Lang, Template[]>();
const escapeRe = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** The dictionary's keys with `{…}` parts, as patterns that match filled-in messages. */
function templatesFor(lang: Exclude<Lang, "en">): Template[] {
  let list = templates.get(lang);
  if (!list) {
    list = Object.keys(DICTS[lang])
      .filter((k) => /\{\w+\}/.test(k))
      .map((key) => {
        const names: string[] = [];
        const parts = key.split(/(\{\w+\})/);
        const body = parts
          .map((p) => {
            const m = p.match(/^\{(\w+)\}$/);
            if (!m) return escapeRe(p);
            names.push(m[1]);
            return "([\\s\\S]*?)";
          })
          .join("");
        return { re: new RegExp(`^${body}$`), names, key };
      })
      // Longer, more specific templates first.
      .sort((a, b) => b.key.length - a.key.length);
    templates.set(lang, list);
  }
  return list;
}

/** Template parts that hold messages of their own, translated in turn. */
const NESTED = new Set(["e", "msg", "reason", "label", "shape", "other", "issues", "problem"]);
/** Template parts that are usually names, but sometimes fixed words ("a master"). */
const NAMES = new Set(["who", "ref", "label"]);

/** Translates a message from the font engine (English text, possibly with names and
 * numbers filled in). Parts that are themselves messages are translated too. */
export function translateMessage(msg: string, depth = 0): string {
  if (current === "en" || depth > 3) return msg;
  const dict = DICTS[current];
  const exact = dict[msg];
  if (exact !== undefined) return typeof exact === "string" ? exact : (exact.other ?? msg);
  for (const tpl of templatesFor(current)) {
    const m = msg.match(tpl.re);
    if (!m) continue;
    const vars: Record<string, string> = {};
    tpl.names.forEach((name, i) => {
      const v = m[i + 1];
      if (NESTED.has(name)) vars[name] = translateMessage(v, depth + 1);
      else if (NAMES.has(name)) vars[name] = translateMessage(v, depth + 1);
      else vars[name] = v;
    });
    // A list of issues joined with "; " is translated item by item.
    if (vars.issues) vars.issues = m[tpl.names.indexOf("issues") + 1].split("; ").map((x) => translateMessage(x, depth + 1)).join("; ");
    const e = dict[tpl.key];
    if (typeof e === "object" && vars.n !== undefined) {
      const form = e[new Intl.PluralRules(current).select(Number(vars.n))] ?? e.other ?? tpl.key;
      return fill(form, vars);
    }
    return t(tpl.key, vars);
  }
  return msg;
}
