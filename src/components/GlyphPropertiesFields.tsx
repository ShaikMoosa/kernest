import { useEffect, useState } from "react";
import type { GlyphProperty, GlyphSummary } from "../lib/backend";
import { COLOR_LABELS, labelCss, labelKey, labelName, type ColorLabel } from "../lib/colorLabels";
import { t } from "../lib/i18n";

const CATEGORIES = ["Letter", "Number", "Punctuation", "Symbol", "Separator", "Mark", "Other"];
const CASES: [string, string][] = [["noCase", "No case"], ["upper", "Upper"], ["lower", "Lower"], ["smallCaps", "Small caps"], ["minor", "Minor"], ["other", "Other"]];
const MIXED = "\u0000mixed";

const overrideOf = (g: GlyphSummary, p: GlyphProperty): string | undefined =>
  p === "category" ? g.categoryOverride : p === "subCategory" ? g.subcategoryOverride : p === "case" ? g.caseOverride : p === "script" ? g.scriptOverride : g.sortName;

/** The value the selection shares for `p` ("" when none is set, MIXED when they differ). */
function shared(glyphs: GlyphSummary[], p: GlyphProperty): string {
  const values = new Set(glyphs.map((g) => overrideOf(g, p) ?? ""));
  return values.size === 1 ? [...values][0] : MIXED;
}

/** Category, subcategory, case, script and sort name of the selected glyphs, each
 *  "Automatic" (from glyph data) unless set (Glyphs Handbook 5.3). */
export function GlyphPropertiesFields({ glyphs, onSet }: { glyphs: GlyphSummary[]; onSet: (names: string[], property: GlyphProperty, value: string) => Promise<void> }) {
  const names = glyphs.map((g) => g.name);
  const one = glyphs.length === 1 ? glyphs[0] : null;
  const save = (p: GlyphProperty, v: string) => onSet(names, p, v);
  const set = (p: GlyphProperty, v: string) => { save(p, v).catch(() => {}); };
  const any = (p: GlyphProperty) => glyphs.some((g) => overrideOf(g, p) !== undefined);
  const reset = (p: GlyphProperty, label: string) => any(p) && (
    <button type="button" className="link-btn k-caption glyph-prop-reset" title={t("Use the value from glyph data")} aria-label={t("Reset {property} to automatic", { property: t(label) })} onClick={() => set(p, "")}>{t("Automatic")}</button>
  );
  const auto = (value: string) => (one ? t("Automatic: {value}", { value: value || t("none") }) : t("Automatic"));

  const select = (p: GlyphProperty, label: string, options: [string, string][], autoValue: string) => {
    const v = shared(glyphs, p);
    return (
      <label className="field info-field glyph-prop">
        <span className="k-caption muted">{t(label)} {reset(p, label)}</span>
        <select className="select" value={v} aria-label={t(label)} onChange={(e) => { if (e.target.value !== MIXED) set(p, e.target.value); }}>
          {v === MIXED && <option value={MIXED}>{t("Mixed")}</option>}
          <option value="">{auto(autoValue)}</option>
          {options.map(([value, text]) => <option key={value} value={value}>{t(text)}</option>)}
        </select>
      </label>
    );
  };

  return (
    <fieldset className="glyph-overrides" aria-label={t("Glyph properties")}>
      <div className="row">
        {select("category", "Category", CATEGORIES.map((c) => [c, c]), one ? (["Uppercase", "Lowercase"].includes(one.category) ? "Letter" : one.category) : "")}
      </div>
      <div className="row">
        {select("case", "Case", CASES, one ? (one.category === "Uppercase" ? t("Upper") : one.category === "Lowercase" ? t("Lower") : t("No case")) : "")}
      </div>
      <div className="row">
        <TextProp glyphs={glyphs} p="subCategory" caption="Subcategory" placeholder={one ? one.subcategory || t("none") : ""} onSet={save} reset={reset("subCategory", "Subcategory")} />
        <TextProp glyphs={glyphs} p="script" caption="Script" placeholder={one ? one.script.toLowerCase() : ""} onSet={save} reset={reset("script", "Script")} />
      </div>
      {one && <TextProp glyphs={glyphs} p="sortName" caption="Sort name" placeholder={one.name} onSet={save} reset={reset("sortName", "Sort name")} />}
    </fieldset>
  );
}

function TextProp({ glyphs, p, caption: label, placeholder, onSet, reset }: { glyphs: GlyphSummary[]; p: GlyphProperty; caption: string; placeholder: string; onSet: (p: GlyphProperty, v: string) => Promise<void>; reset: React.ReactNode }) {
  const v = shared(glyphs, p);
  const shown = v === MIXED ? "" : v;
  const [draft, setDraft] = useState(shown);
  useEffect(() => setDraft(shown), [shown, glyphs.map((g) => g.name).join("\n")]);
  return (
    <label className="field info-field glyph-prop">
      <span className="k-caption muted">{t(label)} {reset}</span>
      <input className="text-input" value={draft} aria-label={t(label)} placeholder={v === MIXED ? t("Mixed") : placeholder || t("Automatic")}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => { if (draft.trim() !== shown) onSet(p, draft.trim()).catch(() => setDraft(shown)); }}
        onKeyDown={(e) => { if (e.key === "Enter") e.currentTarget.blur(); if (e.key === "Escape") { setDraft(shown); e.currentTarget.blur(); } }} />
    </label>
  );
}

/** The twelve label colours and "none" as swatch buttons, for the selected glyphs'
 *  (or their layers') colour label. */
export function ColorLabelPicker({ caption, current, onPick }: { caption: string; current: (ColorLabel | undefined)[]; onPick: (label: ColorLabel | null) => Promise<void> }) {
  const keys = new Set(current.map((c) => (c === undefined ? "" : labelKey(c))));
  const only = keys.size === 1 ? [...keys][0] : null;
  const custom = current.find((c) => c !== undefined && typeof c !== "number");
  return (
    <div className="field info-field color-label-picker" role="group" aria-label={t(caption)}>
      <span className="k-caption muted">{t(caption)}{only === null ? ` · ${t("Mixed")}` : ""}</span>
      <div className="color-swatches">
        <button type="button" className={`color-swatch-btn is-none${only === "" ? " is-selected" : ""}`} aria-pressed={only === ""} title={t("No colour")} aria-label={t("No colour")} onClick={() => { onPick(null).catch(() => {}); }}>×</button>
        {COLOR_LABELS.map(([name], i) => (
          <button key={i} type="button" className={`color-swatch-btn${only === String(i) ? " is-selected" : ""}`} style={{ background: labelCss(i) }} aria-pressed={only === String(i)} title={t(name)} aria-label={t(name)} onClick={() => { onPick(i).catch(() => {}); }} />
        ))}
        {custom !== undefined && (
          <span className={`color-swatch-btn is-custom${only === labelKey(custom) ? " is-selected" : ""}`} style={{ background: labelCss(custom) }} title={t(labelName(custom))} role="img" aria-label={t(labelName(custom))} />
        )}
      </div>
    </div>
  );
}
