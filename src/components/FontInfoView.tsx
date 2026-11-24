import { createContext, Fragment, useCallback, useContext, useEffect, useRef, useState } from "react";
import { backend, errorMessage, nameRecordText, type Axis, type CustomParameter, type GlyphProcessing, type AxisValue, type DocStatus, type FontInfo, type ForeignMasterPreview, type GaspRange, type Hinting, type Instance, type Location, type MasterView, type Metrics, type NameOverrides, type NameRecord, type VariableFontSetting, type VerticalMetrics, type VirtualMaster } from "../lib/backend";
import { designBounds, designToUser, hasMap, userToDesign } from "../lib/axisMap";
import { hasLabels, statStyleName } from "../lib/statName";
import { pickMasterSource } from "../lib/desktop";
import { AddIcon, CloseIcon, FolderIcon, SearchIcon } from "./Icons";
import { FeaturesSection } from "./FeaturesSection";
import { NotesPreview } from "./NotesPreview";
import { ForeignMasterDialog } from "./ForeignMasterDialog";
import { t, tn, translateMessage } from "../lib/i18n";

const AXIS_PRESETS: Axis[] = [
  { name: "Weight", tag: "wght", min: 100, default: 400, max: 900 },
  { name: "Width", tag: "wdth", min: 75, default: 100, max: 125 },
  { name: "Optical size", tag: "opsz", min: 8, default: 14, max: 72 },
  { name: "Slant", tag: "slnt", min: -12, default: 0, max: 0 },
];

const num = (s: string) => Number(s.trim().replace("−", "-"));

/** Design/user readouts round to 2 decimals so interpolated values stay readable. */
const fmtCoord = (v: number) => String(Math.round(v * 100) / 100);

/** One palette colour: a swatch that opens the colour picker, and the #rrggbbaa text. */
function ColorCell({ value, label, onCommit }: { value: string; label: string; onCommit: (v: string) => unknown }) {
  const ref = useRef<HTMLInputElement>(null);
  const alpha = value.length === 9 ? value.slice(7) : "ff";
  // The native "change" event fires once the picker closes (React's onChange fires while dragging).
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const done = () => onCommit(el.value + alpha);
    el.addEventListener("change", done);
    return () => el.removeEventListener("change", done);
  }, [alpha, onCommit]);
  return (
    <span className="color-cell">
      <input ref={ref} type="color" className="color-swatch" aria-label={label} defaultValue={value.slice(0, 7)} key={value} />
      <Field label={label} value={value} width={96} numeric={false} onCommit={(v) => {
        const hex = v.trim().toLowerCase();
        if (!/^#([0-9a-f]{6}|[0-9a-f]{8})$/.test(hex)) { report(label); return false; }
        return onCommit(hex);
      }} />
    </span>
  );
}

let report: (label: string) => void = () => {};

/** Where number fields report input that isn't a number. */
const FieldError = createContext<(message: string) => void>(() => {});

/** A text/number input that commits on Enter or blur and reverts on Escape. */
const LINE_SPACING: { key: keyof VerticalMetrics; label: string }[] = [
  { key: "typoAscender", label: "Typo ascender" },
  { key: "typoDescender", label: "Typo descender" },
  { key: "typoLineGap", label: "Typo line gap" },
  { key: "hheaAscender", label: "hhea ascender" },
  { key: "hheaDescender", label: "hhea descender" },
  { key: "hheaLineGap", label: "hhea line gap" },
  { key: "winAscent", label: "Win ascent" },
  { key: "winDescent", label: "Win descent" },
  { key: "underlineThickness", label: "Underline thickness" },
  { key: "underlinePosition", label: "Underline position" },
];

/** Glyph swaps for one export: "Rename Glyphs" (a=b, …) and "Remove Glyphs" (a, …). */
function SwapFields({ rename, remove, onChange }: { rename?: [string, string][]; remove?: string[]; onChange: (rename: [string, string][], remove: string[]) => unknown }) {
  const report = useContext(FieldError);
  return (
    <>
      <label className="info-field" title={t("Swap glyph drawings in this export: dollar=dollar.alt gives dollar the drawing of dollar.alt")}>
        <span className="k-caption muted">{t("Rename Glyphs")}</span>
        <Field label={t("Rename Glyphs")} value={(rename ?? []).map(([a, b]) => `${a}=${b}`).join(", ")} width={200} mono={false} numeric={false} placeholder="dollar=dollar.alt"
          onCommit={(v) => {
            const pairs: [string, string][] = [];
            for (const part of v.split(/[,\n]+/).map((x) => x.trim()).filter(Boolean)) {
              const [a, b] = part.split("=").map((x) => x.trim());
              if (!a || !b) { report(t("{name}: write a swap as glyph=otherglyph.", { name: part })); return false; }
              pairs.push([a, b]);
            }
            return onChange(pairs, remove ?? []);
          }} />
      </label>
      <label className="info-field" title={t("Glyphs left out of this export, such as the alternates swapped in above. * and ? work as wildcards, such as *.ss01.")}>
        <span className="k-caption muted">{t("Remove Glyphs")}</span>
        <Field label={t("Remove Glyphs")} value={(remove ?? []).join(", ")} width={160} mono={false} numeric={false}
          onCommit={(v) => onChange(rename ?? [], v.split(/[,\s]+/).map((x) => x.trim()).filter(Boolean))} />
      </label>
    </>
  );
}

/** Name table entries, one per line in Glyphs' syntax ("0; © 2026 Me"); read on blur. */
function NameRecordsField({ records, label, onChange }: { records: NameRecord[] | undefined; label: string; onChange: (records: NameRecord[]) => void }) {
  const shown = (records ?? []).map(nameRecordText).join("\n");
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);
  const report = useContext(FieldError);
  return (
    <label className="info-field fi-name-records" title={t("One entry per line: name ID; text. Add the platform (1 Mac, 3 Windows), or platform, encoding and language IDs, before the semicolon. IDs 1, 2, 3, 5 and 6 come from the names above.")}>
      <span className="k-caption muted">{t("Name table entries")}</span>
      <textarea className="k-code" rows={2} aria-label={label} value={text} placeholder={t("0; © 2026 Me")} spellCheck={false}
        onChange={(e) => setText(e.target.value)}
        onBlur={async () => {
          if (text === shown) return;
          try { onChange(await backend.parseNameRecords(text)); } catch (e) { report(errorMessage(e)); setText(shown); }
        }} />
    </label>
  );
}

/** An instance's naming overrides (Glyphs naming parameters), folded away by default. */
function NamingFields({ names, instance, onChange }: { names: NameOverrides | undefined; instance: string; onChange: (names: NameOverrides | undefined) => void }) {
  const n = names ?? {};
  const set = (p: Partial<NameOverrides>) => {
    const next: NameOverrides = { ...n, ...p };
    for (const k of Object.keys(next) as (keyof NameOverrides)[]) {
      const v = next[k];
      if (v === undefined || v === "" || (Array.isArray(v) && v.length === 0)) delete next[k];
    }
    onChange(Object.keys(next).length ? next : undefined);
  };
  const text = (key: "postscriptFontName" | "fullName" | "compatibleFullName" | "styleMapFamilyName" | "fileName", label: string, title: string, width = 180) => (
    <label className="info-field" title={t(title)}><span className="k-caption muted">{t(label)}</span>
      <Field label={t("{field} for {name}", { field: t(label), name: instance })} value={n[key] ?? ""} width={width} mono={false} numeric={false} placeholder={t("Automatic")}
        onCommit={(v) => set({ [key]: v.trim() || undefined })} />
    </label>
  );
  const count = Object.keys(n).length;
  return (
    <details className="fi-names">
      <summary className="k-caption">{count > 0 ? t("Naming ({n} set)", { n: count }) : t("Naming")}</summary>
      <div className="fi-row">
        {text("postscriptFontName", "PostScript name", "nameID 6 and the CFF font name, e.g. MyFont-BoldItalic. ASCII, no spaces, at most 63 characters.")}
        {text("fullName", "Full name", "nameID 4, the complete name apps show, e.g. My Font Bold Italic.")}
        {text("compatibleFullName", "Compatible full name", "nameID 18, the full name older Mac apps show.")}
        {text("styleMapFamilyName", "Style map family", "nameID 1: the family this instance's Regular, Italic, Bold and Bold Italic share for style linking.")}
        {text("fileName", "File name", "The exported file's name without extension, e.g. MyFont-Bold. Characters Windows forbids become _.", 160)}
        <NameRecordsField records={n.records} label={t("Name table entries for {name}", { name: instance })} onChange={(records) => set({ records })} />
      </div>
    </details>
  );
}

/** The export-specific feature code parameters (Glyphs names), in the order offered. */
const FEATURE_PARAMETERS = ["Replace Feature", "Remove Features", "Replace Prefix", "Remove Prefixes", "Replace Class", "Remove Classes"] as const;
const FEATURE_PARAMETER_HINTS: Record<string, string> = {
  "Replace Feature": "liga;\nsub f i by f_i.alt;",
  "Remove Features": "liga, dlig",
  "Replace Prefix": "Languagesystems;\nlanguagesystem DFLT dflt;",
  "Remove Prefixes": "Languagesystems",
  "Replace Class": "Figures; one.osf two.osf",
  "Remove Classes": "Figures",
};

/** One parameter's code, edited in a text area and applied on blur. */
function FeatureCodeArea({ value, label, placeholder, onCommit }: { value: string; label: string; placeholder: string; onCommit: (v: string) => void }) {
  const [text, setText] = useState(value);
  useEffect(() => setText(value), [value]);
  return (
    <textarea className="k-code fi-feature-code-area" rows={3} aria-label={label} value={text} placeholder={placeholder} spellCheck={false} dir="ltr"
      onChange={(e) => setText(e.target.value)} onBlur={() => { if (text !== value) onCommit(text); }} />
  );
}

/**
 * SMO-23 · Export-specific feature code: Glyphs' Replace Feature, Remove Features,
 * Replace/Remove Prefix and Replace/Remove Class parameters for one instance or
 * variable font, applied in order to the generated and hand-written code.
 */
function FeatureCodeFields({ params, owner, onChange }: { params: CustomParameter[] | undefined; owner: string; onChange: (params: CustomParameter[] | undefined) => void }) {
  const list = params ?? [];
  const [warnings, setWarnings] = useState<string[]>([]);
  const key = JSON.stringify(list);
  useEffect(() => {
    if (list.length === 0) { setWarnings([]); return; }
    let live = true;
    backend.checkFeatureParameters(list).then((w) => { if (live) setWarnings(w); }, () => { if (live) setWarnings([]); });
    return () => { live = false; };
  }, [key]); // eslint-disable-line react-hooks/exhaustive-deps
  const set = (i: number, p: Partial<CustomParameter>) => onChange(list.map((x, j) => (j === i ? { ...x, ...p } : x)));
  const on = list.filter((p) => !p.disabled).length;
  return (
    <details className="fi-names fi-feature-code" open={list.length > 0 ? true : undefined}>
      <summary className="k-caption">{list.length > 0 ? tn(on, "Feature code ({n} parameter)", "Feature code ({n} parameters)") : t("Feature code")}</summary>
      <p className="k-caption muted">{t("Change the feature code for this export only. Parameters apply in order, to the generated features and your own code.")}</p>
      {list.map((p, i) => {
        const isList = p.name.startsWith("Remove");
        const label = t("{parameter} for {name}", { parameter: t(p.name), name: owner });
        return (
          <div key={i} className="fi-row fi-feature-param">
            <select className="select" value={p.name} aria-label={t("Parameter")} onChange={(e) => set(i, { name: e.target.value })}>
              {FEATURE_PARAMETERS.map((k) => <option key={k} value={k}>{t(k)}</option>)}
            </select>
            {isList ? (
              <Field label={label} value={p.value} width={240} mono={false} numeric={false} placeholder={FEATURE_PARAMETER_HINTS[p.name]} onCommit={(v) => set(i, { value: v.trim() })} />
            ) : (
              <FeatureCodeArea value={p.value} label={label} placeholder={FEATURE_PARAMETER_HINTS[p.name] ?? ""} onCommit={(v) => set(i, { value: v })} />
            )}
            <label className="check-row" title={t("Apply this parameter when exporting")}>
              <input type="checkbox" checked={!p.disabled} aria-label={t("Use {parameter}", { parameter: t(p.name) })} onChange={(e) => set(i, { disabled: e.target.checked ? undefined : true })} />
              <span className="k-caption">{t("On")}</span>
            </label>
            <button className="icon-btn" aria-label={t("Remove {name}", { name: t(p.name) })} title={t("Remove parameter")} onClick={() => { const next = list.filter((_, j) => j !== i); onChange(next.length ? next : undefined); }}><CloseIcon size={14} /></button>
          </div>
        );
      })}
      {warnings.length > 0 && (
        <ul className="fi-feature-warnings" role="status">
          {warnings.map((w, i) => <li key={i} className="k-caption">{translateMessage(w)}</li>)}
        </ul>
      )}
      <button className="btn btn-secondary btn-sm" onClick={() => onChange([...list, { name: "Remove Features", value: "" }])}><AddIcon size={14} /> {t("Add parameter")}</button>
    </details>
  );
}

/** Glyphs filter strings shown as examples (not translated: they are code). */
const FILTER_EXAMPLES = { pre: "RoundCorner;20", post: "OffsetCurve;10;10" };

/** Glyph lists typed in a field: separated by commas or spaces. */
const nameList = (v: string) => v.split(/[,\s]+/).map((x) => x.trim()).filter(Boolean);
/** Filter strings typed one per line. */
const lineList = (v: string) => v.split("\n").map((x) => x.trim()).filter(Boolean);

/**
 * SMO-28 · Glyph processing for one export: Keep Glyphs, Decompose Glyphs (or every
 * component, for variable fonts), PreFilter, Filter and Remove Overlap off, with the
 * warnings an export would give (renames, patterns that match nothing, filters).
 */
function GlyphProcessingFields({ value, rename, remove, owner, variable, onChange }: {
  value: GlyphProcessing; rename?: [string, string][]; remove?: string[]; owner: string; variable: boolean;
  onChange: (p: Partial<GlyphProcessing>) => void;
}) {
  const [warnings, setWarnings] = useState<string[]>([]);
  const p: GlyphProcessing = {
    keepGlyphs: value.keepGlyphs, decomposeGlyphs: value.decomposeGlyphs, decomposeComponents: value.decomposeComponents,
    preFilters: value.preFilters, filters: value.filters, keepOverlaps: value.keepOverlaps,
  };
  const key = JSON.stringify([p, rename ?? [], remove ?? []]);
  useEffect(() => {
    let live = true;
    backend.checkGlyphProcessing(rename ?? [], remove ?? [], p).then((w) => { if (live) setWarnings(w); }, () => { if (live) setWarnings([]); });
    return () => { live = false; };
  }, [key]); // eslint-disable-line react-hooks/exhaustive-deps
  const list = (v: string[]) => (v.length ? v : undefined);
  const set = (label: string) => t("{field} for {name}", { field: t(label), name: owner });
  const count = [p.keepGlyphs?.length, p.decomposeGlyphs?.length, p.decomposeComponents, p.preFilters?.length, p.filters?.length, p.keepOverlaps].filter(Boolean).length;
  const filterHelp = t("One filter per line, as in Glyphs: RoundCorner;20, OffsetCurve;10;10, Transformations;Slant:8 or RemoveOverlap. End a line with ;include:A,B or ;exclude:A,B to choose glyphs.");
  return (
    <details className="fi-names fi-processing" open={count > 0 || warnings.length > 0 ? true : undefined}>
      <summary className="k-caption">{count > 0 ? tn(count, "Glyph processing ({n} set)", "Glyph processing ({n} set)") : t("Glyph processing")}</summary>
      <div className="fi-row">
        {!variable && (
          <label className="info-field" title={t("Only these glyphs are exported, plus .notdef. * and ? work as wildcards, such as a* or *.sc.")}>
            <span className="k-caption muted">{t("Keep Glyphs")}</span>
            <Field label={set("Keep Glyphs")} value={(p.keepGlyphs ?? []).join(", ")} width={200} mono={false} numeric={false} placeholder={t("All glyphs")}
              onCommit={(v) => onChange({ keepGlyphs: list(nameList(v)) })} />
          </label>
        )}
        <label className="info-field" title={t("Components in these glyphs become outlines. * and ? work as wildcards.")}>
          <span className="k-caption muted">{t("Decompose Glyphs")}</span>
          <Field label={set("Decompose Glyphs")} value={(p.decomposeGlyphs ?? []).join(", ")} width={200} mono={false} numeric={false}
            onCommit={(v) => onChange({ decomposeGlyphs: list(nameList(v)) })} />
        </label>
        {variable ? (
          <label className="check-row" title={t("Every component in this variable font becomes outlines (Decompose Components in Variable Font).")}>
            <input type="checkbox" checked={!!p.decomposeComponents} aria-label={t("Decompose all components in {name}", { name: owner })} onChange={(e) => onChange({ decomposeComponents: e.target.checked || undefined })} />
            <span className="k-caption">{t("Decompose all components")}</span>
          </label>
        ) : (
          <>
            <label className="info-field" title={filterHelp}>
              <span className="k-caption muted">{t("PreFilter")}</span>
              <FeatureCodeArea value={(p.preFilters ?? []).join("\n")} label={set("PreFilter")} placeholder={FILTER_EXAMPLES.pre} onCommit={(v) => onChange({ preFilters: list(lineList(v)) })} />
            </label>
            <label className="info-field" title={filterHelp}>
              <span className="k-caption muted">{t("Filter")}</span>
              <FeatureCodeArea value={(p.filters ?? []).join("\n")} label={set("Filter")} placeholder={FILTER_EXAMPLES.post} onCommit={(v) => onChange({ filters: list(lineList(v)) })} />
            </label>
            <label className="check-row" title={t("Untick to keep overlapping outlines in this instance, even when the export removes overlaps. PreFilter runs before overlaps are removed, Filter after.")}>
              <input type="checkbox" checked={!p.keepOverlaps} aria-label={t("Remove overlaps in {name}", { name: owner })} onChange={(e) => onChange({ keepOverlaps: e.target.checked ? undefined : true })} />
              <span className="k-caption">{t("Remove overlaps")}</span>
            </label>
          </>
        )}
      </div>
      {warnings.length > 0 && (
        <ul className="fi-feature-warnings" role="status">
          {warnings.map((w, i) => <li key={i} className="k-caption">{translateMessage(w)}</li>)}
        </ul>
      )}
    </details>
  );
}

/** ttfautohint's own defaults, shown as placeholders (option values, not prose). */
const TTFA_DEFAULTS = { stemWidthMode: "qsq", defaultScript: "latn", fallbackScript: "none" };

/** The gasp flags, in bit order. */
const GASP_FLAGS: [number, string, string][] = [
  [1, "Gridfit", "Snap outlines to the pixel grid (hinting) at these sizes"],
  [2, "Grayscale", "Smooth edges with grayscale at these sizes"],
  [4, "Symmetric gridfit", "ClearType: gridfit in both directions"],
  [8, "Symmetric smoothing", "ClearType: smooth in both directions"],
];

/** Editor for the gasp table ("gasp Table"): one row per size range, with its flags. */
function GaspEditor({ gasp, onApply }: { gasp: GaspRange[]; onApply: (gasp: GaspRange[]) => Promise<boolean> }) {
  const [draft, setDraft] = useState<GaspRange[] | null>(null);
  const rows = draft ?? gasp;
  const set = (i: number, p: Partial<GaspRange>) => setDraft(rows.map((r, j) => (j === i ? { ...r, ...p } : r)));
  const add = () => {
    if (rows.length === 0) { setDraft([{ maxPpem: 8, flags: 2 }, { maxPpem: 65535, flags: 15 }]); return; }
    const before = rows.length > 1 ? rows[rows.length - 2].maxPpem : 0;
    const next = [...rows];
    next.splice(rows.length - 1, 0, { maxPpem: Math.min(before + 8, 65534), flags: 15 });
    setDraft(next);
  };
  return (
    <div className="fi-gasp">
      {rows.length === 0 && <p className="k-caption muted">{t("No gasp table: the autohinter writes its own, and unhinted fonts have none.")}</p>}
      {rows.map((r, i) => (
        <div key={i} className="fi-row fi-gasp-row">
          <label className="info-field" title={t("This range covers sizes up to this many pixels per em. The last range must end at 65535.")}><span className="k-caption muted">{t("Up to (px)")}</span>
            <Field label={t("gasp range {n} size", { n: i + 1 })} value={r.maxPpem} onCommit={(v) => {
              const n = Number(v);
              if (!Number.isInteger(n) || n < 0 || n > 65535) return false;
              set(i, { maxPpem: n });
            }} />
          </label>
          {GASP_FLAGS.map(([bit, en, hintEn]) => (
            <label key={bit} className="check-row" title={t(hintEn)}>
              <input type="checkbox" checked={(r.flags & bit) !== 0} aria-label={t("{flag} for gasp range {n}", { flag: t(en), n: i + 1 })}
                onChange={(e) => set(i, { flags: e.target.checked ? r.flags | bit : r.flags & ~bit })} />
              <span className="k-caption">{t(en)}</span>
            </label>
          ))}
          <button className="icon-btn" aria-label={t("Remove gasp range {n}", { n: i + 1 })} onClick={() => setDraft(rows.filter((_, j) => j !== i))}><CloseIcon size={14} /></button>
        </div>
      ))}
      <div className="fi-actions">
        <button className="btn btn-secondary btn-sm" onClick={add}><AddIcon size={14} /> {t("Add range")}</button>
        <span className="spacer" />
        {draft && (
          <>
            <button className="btn btn-secondary btn-sm" onClick={() => setDraft(null)}>{t("Discard")}</button>
            <button className="btn btn-primary btn-sm" onClick={async () => { if (await onApply(draft)) setDraft(null); }}>{t("Apply gasp table")}</button>
          </>
        )}
      </div>
    </div>
  );
}

/** Numbers typed as a list ("50, 80") or empty; `null` when something isn't a number. */
function numberList(v: string): number[] | null {
  const parts = v.split(/[,\s]+/).map((x) => x.trim()).filter(Boolean);
  const nums = parts.map(Number);
  return nums.every(Number.isFinite) ? nums : null;
}

function Field({ value, onCommit, width = 72, mono = true, label, numeric = true, placeholder }: {
  /** Returning (a promise of) false means the value was refused: the field shows the old one again. */
  value: string | number; onCommit: (v: string) => unknown; width?: number; mono?: boolean; label: string; numeric?: boolean;
  /** Shown when empty; a numeric field with a placeholder accepts empty (meaning automatic). */
  placeholder?: string;
}) {
  const shown = String(value);
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);
  const report = useContext(FieldError);
  const commit = (v: string) => {
    if (v === shown) return;
    // Number fields only take numbers: say so instead of sending garbage.
    if (numeric && v.trim() !== "" && !Number.isFinite(num(v))) {
      setText(shown);
      return report(t("{name}: enter a number.", { name: label }));
    }
    if (numeric && v.trim() === "" && placeholder === undefined) {
      setText(shown);
      return report(t("{name}: enter a number.", { name: label }));
    }
    Promise.resolve(onCommit(v)).then((ok) => { if (ok === false) setText(shown); }, () => setText(shown));
  };
  return (
    <input
      aria-label={label}
      className={`metric-input${mono ? "" : " text-field"}`}
      style={{ width }}
      value={text}
      placeholder={placeholder}
      inputMode={numeric ? "decimal" : undefined}
      onChange={(e) => setText(e.target.value)}
      onBlur={(e) => commit(e.currentTarget.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
        if (e.key === "Escape") { setText(shown); (e.target as HTMLInputElement).blur(); }
      }}
    />
  );
}

interface Props {
  version: number;
  onChanged: (status: DocStatus) => void;
  onError: (e: unknown) => void;
  onGenerateInstances: () => void;
}

/** S10/S11 · Font Info: general info, axes, masters and instances. */
export function FontInfoView({ version, onChanged, onError, onGenerateInstances }: Props) {
  const [info, setInfo] = useState<FontInfo | null>(null);
  const [group, setGroup] = useState<string>(() => { try { return localStorage.getItem("kernest.fontInfoGroup") ?? "general"; } catch { return "general"; } });
  const [settingsFilter, setSettingsFilter] = useState("");
  const [notesPreview, setNotesPreview] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  useEffect(() => { try { localStorage.setItem("kernest.fontInfoGroup", group); } catch { /* a convenience only */ } }, [group]);
  // Filter settings: every section whose labels or values mention the text, across all groups.
  useEffect(() => {
    const root = scrollRef.current;
    if (!root) return;
    const q = settingsFilter.trim().toLowerCase();
    for (const el of root.querySelectorAll<HTMLElement>("[data-group]")) {
      if (!q) { el.hidden = false; continue; }
      const values = [...el.querySelectorAll<HTMLInputElement | HTMLTextAreaElement>("input, textarea")].map((i) => i.value).join(" ");
      el.hidden = !`${el.textContent ?? ""} ${values}`.toLowerCase().includes(q);
    }
  });
  // Drafts while editing axes or instances. A ref mirrors them so commits that land
  // in the same instant (blur, then click Apply) always build on the latest draft.
  const [axes, setAxesState] = useState<Axis[] | null>(null);
  const [instances, setInstState] = useState<Instance[] | null>(null);
  const [variableFonts, setVfState] = useState<VariableFontSetting[] | null>(null);
  const drafts = useRef<{ axes: Axis[] | null; instances: Instance[] | null; variableFonts: VariableFontSetting[] | null }>({ axes: null, instances: null, variableFonts: null });
  const setAxes = (v: Axis[] | null) => { drafts.current.axes = v; setAxesState(v); };
  const setInstances = (v: Instance[] | null) => { drafts.current.instances = v; setInstState(v); };
  const setVariableFonts = (v: VariableFontSetting[] | null) => { drafts.current.variableFonts = v; setVfState(v); };
  const [adding, setAdding] = useState<{ name: string; copyFrom: string; location: Location } | null>(null);
  // The "Add from font…" dialog: a picked source and its per-master compatibility report.
  const [foreign, setForeign] = useState<{ path: string; previews: ForeignMasterPreview[] } | null>(null);
  // A draft of the glyph-order text; null means "show the saved order, not editing".
  const [orderText, setOrderText] = useState<string | null>(null);
  // The axis tag whose avar map editor is expanded (one at a time); null = none.
  const [mapOpen, setMapOpen] = useState<string | null>(null);
  // The axis tag whose STAT value-label editor is expanded (one at a time); null = none.
  const [labelsOpen, setLabelsOpen] = useState<string | null>(null);
  // The master hinting editor that is expanded: "<master id>:zones" or ":stems"; null = none.
  const [hintsOpen, setHintsOpen] = useState<string | null>(null);

  const reload = useCallback(() => backend.fontInfo().then((i) => { setInfo(i); }, onError), [onError]);
  useEffect(() => { reload(); }, [reload, version]);

  const apply = async (task: () => Promise<FontInfo>) => {
    try {
      const next = await task();
      setInfo(next);
      onChanged(next.status);
      return true;
    } catch (e) {
      onError(e);
      return false;
    }
  };

  report = (label) => onError(t("{name}: enter a colour such as #ff8800.", { name: label }));
  if (!info) return <div className="fontinfo"><p className="k-caption muted editor-loading">{t("Loading Font Info…")}</p></div>;

  const axesDraft = axes ?? info.axes;
  const curAxes = () => drafts.current.axes ?? info.axes;
  const curInst = () => drafts.current.instances ?? info.instances;
  const instDraft = instances ?? info.instances;
  const curVf = () => drafts.current.variableFonts ?? info.variableFonts;
  const vfDraft = variableFonts ?? info.variableFonts;
  const locFields = (loc: Location, onChange: (loc: Location) => void, prefix: string) =>
    info.axes.map((a) => {
      // Locations are design-space; the fallback is the mapped design default, not the user one.
      const design = loc[a.tag] ?? designBounds(a)[1];
      return (
        <label key={a.tag} className="info-field">
          <span className="k-caption muted">{a.tag}</span>
          <Field label={`${prefix} ${a.name}`} value={design} onCommit={(v) => onChange({ ...loc, [a.tag]: num(v) })} />
          {hasMap(a) && <span className="k-caption muted axis-user">{t("user {value}", { value: fmtCoord(designToUser(a, design)) })}</span>}
        </label>
      );
    });
  const updateMaster = (m: MasterView, patch: Partial<{ name: string; location: Location; metrics: Metrics }>) =>
    apply(() => backend.updateMaster(m.id, patch.name ?? m.name, patch.location ?? m.location, patch.metrics ?? m.metrics));

  const openForeign = async () => {
    const path = await pickMasterSource();
    if (!path) return;
    try {
      const previews = await backend.previewForeignMasters(path);
      setForeign({ path, previews });
    } catch (e) { onError(e); }
  };
  // Each master is its own undoable step. Show progress as they land; if one fails,
  // refresh so the masters already added are visible, then let the dialog report it.
  const addForeign = async (selection: { masterId: string; name: string; location: Location }[]) => {
    if (!foreign) return;
    try {
      for (const s of selection) {
        const r = await backend.addMasterFrom(foreign.path, s.masterId, s.name, s.location);
        setInfo(r.info);
        onChanged(r.info.status);
      }
      setForeign(null);
    } catch (e) {
      reload();
      throw e;
    }
  };

  return (
    <FieldError.Provider value={onError}>
    <div className="fontinfo">
      <nav className="fi-nav" aria-label={t("Font Info sections")}>
        <label className="search fi-filter">
          <SearchIcon />
          <input placeholder={t("Filter settings")} value={settingsFilter} onChange={(e) => setSettingsFilter(e.target.value)} aria-label={t("Filter settings")} />
        </label>
        <p className="k-overline muted">{t("Font Info")}</p>
        {([
          ["general", t("General"), null],
          ["axes", t("Axes"), info.axes.length],
          ["masters", t("Masters"), info.masters.length],
          ["exports", t("Exports"), info.instances.length + info.variableFonts.length],
          ["features", t("Features"), null],
          ["params", t("Custom parameters"), info.customParameters.length],
          ["other", t("Other"), null],
          ["notes", t("Notes"), null],
        ] as const).map(([id, label, count]) => (
          <button key={id} type="button" className={`list-item${group === id && !settingsFilter.trim() ? " is-selected" : ""}`} aria-current={group === id ? "page" : undefined}
            onClick={() => { setGroup(id); setSettingsFilter(""); scrollRef.current?.scrollTo({ top: 0 }); }}>
            <span>{label}</span>
            {count ? <span className="list-meta">{count}</span> : null}
          </button>
        ))}
      </nav>
      <div className="fontinfo-scroll" ref={scrollRef} data-show={settingsFilter.trim() ? "all" : group}>
        {(info.warnings.length > 0 || info.incompatible > 0) && (
          <div className="fi-warnings" role="status">
            {info.warnings.map((w) => <p key={w} className="notice notice-warning k-label">⚠ {translateMessage(w)}</p>)}
            {info.incompatible > 0 && (
              <p className="notice notice-warning k-label">⚠ {tn(info.incompatible, "{n} glyph can't be interpolated. See the Incompatible filter in Font View.", "{n} glyphs can't be interpolated. See the Incompatible filter in Font View.")}</p>
            )}
          </div>
        )}

        <section className="fi-section" data-group="general" aria-labelledby="fi-general">
          <h2 id="fi-general" className="k-subtitle">{t("General")}</h2>
          <div className="fi-row">
            <label className="info-field"><span className="k-caption muted">{t("Family name")}</span>
              <Field label={t("Family name")} value={info.familyName} width={260} mono={false} numeric={false}
                onCommit={(v) => apply(() => backend.setFontInfo(v, info.unitsPerEm, info.versionMajor, info.versionMinor))} />
            </label>
            <label className="info-field"><span className="k-caption muted">{t("Units per em")}</span>
              <Field label={t("Units per em")} value={info.unitsPerEm} onCommit={(v) => {
                const upm = num(v);
                if (!Number.isInteger(upm) || upm < 16 || upm > 16384) { onError(t("Units per em must be a whole number between 16 and 16384.")); return false; }
                return apply(() => backend.setFontInfo(info.familyName, upm, info.versionMajor, info.versionMinor));
              }} />
            </label>
            <label className="info-field"><span className="k-caption muted">{t("Version")}</span>
              <Field label={t("Version")} value={`${info.versionMajor}.${String(info.versionMinor).padStart(3, "0")}`} numeric={false}
                onCommit={(v) => {
                  const m = v.trim().match(/^(\d+)(?:\.(\d+))?$/);
                  if (!m) { onError(t("Write the version as major.minor, e.g. 1.002.")); return false; }
                  return apply(() => backend.setFontInfo(info.familyName, info.unitsPerEm, Number(m[1]), Number(m[2] ?? 0)));
                }} />
            </label>
          </div>
          <div className="fi-row">
            {([
              ["designer", "Designer", 200],
              ["designerUrl", "Designer URL", 240],
              ["manufacturer", "Manufacturer", 200],
              ["manufacturerUrl", "Manufacturer URL", 240],
              ["copyright", "Copyright", 280],
              ["vendorId", "Vendor ID", 96],
              ["license", "License", 280],
              ["licenseUrl", "License URL", 240],
              ["trademark", "Trademark", 240],
              ["description", "Description", 320],
              ["sampleText", "Sample text", 320],
            ] as const).map(([key, en, width]) => { const label = t(en); return (
              <label key={key} className="info-field"><span className="k-caption muted">{label}</span>
                <Field label={label} value={info.names[key]} width={width} mono={false} numeric={false}
                  onCommit={(v) => apply(() => backend.setFontNames({ ...info.names, [key]: v }))} />
              </label>
            ); })}
          </div>
          <p className="k-caption muted">{t("These names go into the exported font's name table. Leave a field empty to leave it out.")}</p>
        </section>

        <section className="fi-section" data-group="axes" aria-labelledby="fi-axes">
          <div className="fi-head">
            <h2 id="fi-axes" className="k-subtitle">{t("Axes")}</h2>
            <span className="spacer" />
            <select className="select" value="" aria-label={t("Add axis")} onChange={(e) => {
              const p = e.target.value === "custom" ? { name: t("Custom"), tag: "XXXX", min: 0, default: 0, max: 100 } : AXIS_PRESETS.find((a) => a.tag === e.target.value);
              if (p) setAxes([...curAxes(), { ...p, name: t(p.name) }]);
            }}>
              <option value="">{t("Add axis…")}</option>
              {AXIS_PRESETS.filter((p) => !axesDraft.some((a) => a.tag === p.tag)).map((p) => <option key={p.tag} value={p.tag}>{t(p.name)} ({p.tag})</option>)}
              <option value="custom">{t("Custom axis")}</option>
            </select>
          </div>
          {axesDraft.length === 0 ? (
            <p className="k-caption muted">{t("No axes. Add one to make a variable family with several masters.")}</p>
          ) : (
            <table className="fi-table">
              <thead><tr><th>{t("Name")}</th><th>{t("Tag")}</th><th className="num">{t("Minimum")}</th><th className="num">{t("Default")}</th><th className="num">{t("Maximum")}</th><th aria-label={t("Actions")} /></tr></thead>
              <tbody>
                {axesDraft.map((a, i) => {
                  const set = (patch: Partial<Axis>) => setAxes(curAxes().map((x, j) => (j === i ? { ...x, ...patch } : x)));
                  const pts = a.map ?? [];
                  // Points stay sorted by user value; an empty list clears the map (user == design).
                  const setMap = (next: [number, number][]) => {
                    const sorted = [...next].sort((p, q) => p[0] - q[0]);
                    set({ map: sorted.length ? sorted : undefined });
                  };
                  const addPoint = () => {
                    if (pts.length === 0) {
                      // Seed the identity extremes so the axis can be bent at the default right away.
                      const seed: [number, number][] = [[a.min, a.min], [a.default, a.default], [a.max, a.max]];
                      setMap(seed.filter((p, k) => k === 0 || p[0] !== seed[k - 1][0]));
                      return;
                    }
                    // Insert on the current curve at the midpoint of the widest user gap, so no duplicate.
                    let at = 0;
                    let gap = 0;
                    for (let k = 0; k < pts.length - 1; k++) {
                      const g = pts[k + 1][0] - pts[k][0];
                      if (g > gap) { gap = g; at = k; }
                    }
                    if (gap <= 0) return;
                    const u = (pts[at][0] + pts[at + 1][0]) / 2;
                    const next = [...pts];
                    next.splice(at + 1, 0, [u, userToDesign(a, u)]);
                    setMap(next);
                  };
                  const labels = a.labels ?? [];
                  // An empty list clears the labels (the axis has no named values).
                  const setLabels = (next: AxisValue[]) => set({ labels: next.length ? next : undefined });
                  const patchLabel = (j: number, p: Partial<AxisValue>) => setLabels(labels.map((l, k) => (k === j ? { ...l, ...p } : l)));
                  const addLabel = () => setLabels([...labels, { name: "", value: a.default }]);
                  const optNum = (v: string) => (v.trim() === "" ? undefined : num(v));
                  const open = mapOpen === a.tag;
                  const labelsExpanded = labelsOpen === a.tag;
                  return (
                    <Fragment key={i}>
                      <tr>
                        <td><Field label={t("Axis name")} value={a.name} width={160} mono={false} numeric={false} onCommit={(v) => set({ name: v })} /></td>
                        <td><Field label={t("Axis tag")} value={a.tag} width={64} numeric={false} onCommit={(v) => set({ tag: v.trim() })} /></td>
                        <td className="num"><Field label={t("Minimum")} value={a.min} onCommit={(v) => set({ min: num(v) })} /></td>
                        <td className="num"><Field label={t("Default")} value={a.default} onCommit={(v) => set({ default: num(v) })} /></td>
                        <td className="num"><Field label={t("Maximum")} value={a.max} onCommit={(v) => set({ max: num(v) })} /></td>
                        <td className="fi-axis-actions">
                          <button className={`btn btn-sm ${hasMap(a) ? "btn-primary" : "btn-secondary"}`} aria-expanded={open}
                            title={t("Map user values to design values")} onClick={() => setMapOpen(open ? null : a.tag)}>
                            {hasMap(a) ? tn(pts.length, "{n} point", "{n} points") : t("Mapping")}
                          </button>
                          <button className={`btn btn-sm ${labels.length ? "btn-primary" : "btn-secondary"}`} aria-expanded={labelsExpanded}
                            title={t("Name the values on this axis")} onClick={() => setLabelsOpen(labelsExpanded ? null : a.tag)}>
                            {labels.length ? tn(labels.length, "{n} value", "{n} values") : t("Values")}
                          </button>
                          <button className="icon-btn" aria-label={t("Remove {name}", { name: a.name })} title={t("Remove axis")} onClick={() => setAxes(curAxes().filter((_, j) => j !== i))}><CloseIcon size={14} /></button>
                        </td>
                      </tr>
                      {open && (
                        <tr className="fi-axis-map">
                          <td colSpan={6}>
                            <div className="axis-map">
                              <p className="k-caption muted">{t("Apps use user values; masters use design values. Add points to map one to the other.")}</p>
                              <div className="axis-map-points">
                                {pts.map(([u, d], j) => (
                                  <div key={j} className="axis-map-point">
                                    <label className="info-field"><span className="k-caption muted">{t("User")}</span>
                                      <Field label={t("User value")} value={u} onCommit={(v) => setMap(pts.map((p, k): [number, number] => (k === j ? [num(v), p[1]] : p)))} />
                                    </label>
                                    <label className="info-field"><span className="k-caption muted">{t("Design")}</span>
                                      <Field label={t("Design value")} value={d} onCommit={(v) => setMap(pts.map((p, k): [number, number] => (k === j ? [p[0], num(v)] : p)))} />
                                    </label>
                                    <button className="icon-btn" aria-label={t("Remove point")} title={t("Remove point")} onClick={() => setMap(pts.filter((_, k) => k !== j))}><CloseIcon size={14} /></button>
                                  </div>
                                ))}
                              </div>
                              <button className="btn btn-secondary btn-sm" onClick={addPoint}><AddIcon size={14} /> {t("Add point")}</button>
                            </div>
                          </td>
                        </tr>
                      )}
                      {labelsExpanded && (
                        <tr className="fi-axis-map">
                          <td colSpan={6}>
                            <div className="axis-map">
                              <p className="k-caption muted">{t("Apps show these names for the styles at each position on the axis. Values are user values.")}</p>
                              <div className="axis-labels">
                                {labels.map((l, j) => (
                                  <div key={j} className="axis-label">
                                    <label className="info-field"><span className="k-caption muted">{t("Name")}</span>
                                      <Field label={t("Value name")} value={l.name} width={120} mono={false} numeric={false} onCommit={(v) => patchLabel(j, { name: v })} />
                                    </label>
                                    <label className="info-field"><span className="k-caption muted">{t("Value")}</span>
                                      <Field label={t("User value")} value={l.value} onCommit={(v) => patchLabel(j, { value: num(v) })} />
                                    </label>
                                    <label className="info-field"><span className="k-caption muted">{t("Range from")}</span>
                                      <Field label={t("Range from")} value={l.min ?? ""} placeholder={t("none")} onCommit={(v) => patchLabel(j, { min: optNum(v) })} />
                                    </label>
                                    <label className="info-field"><span className="k-caption muted">{t("Range to")}</span>
                                      <Field label={t("Range to")} value={l.max ?? ""} placeholder={t("none")} onCommit={(v) => patchLabel(j, { max: optNum(v) })} />
                                    </label>
                                    <label className="info-field"><span className="k-caption muted">{t("Linked value")}</span>
                                      <Field label={t("Linked value")} value={l.linkedValue ?? ""} placeholder={t("none")} onCommit={(v) => patchLabel(j, { linkedValue: optNum(v) })} />
                                    </label>
                                    <label className="check-row" title={t("Apps may leave this name out of a style name, like Regular.")}>
                                      <input type="checkbox" checked={l.elidable === true} aria-label={t("Elidable")} onChange={(e) => patchLabel(j, { elidable: e.target.checked })} />
                                      <span className="k-caption">{t("Elidable")}</span>
                                    </label>
                                    <button className="icon-btn" aria-label={t("Remove value")} title={t("Remove value")} onClick={() => setLabels(labels.filter((_, k) => k !== j))}><CloseIcon size={14} /></button>
                                  </div>
                                ))}
                              </div>
                              <button className="btn btn-secondary btn-sm" onClick={addLabel}><AddIcon size={14} /> {t("Add value")}</button>
                            </div>
                          </td>
                        </tr>
                      )}
                    </Fragment>
                  );
                })}
              </tbody>
            </table>
          )}
          {axes && (
            <div className="fi-actions">
              <span className="k-caption muted">{t("Axis changes apply to all masters and instances.")}</span>
              <span className="spacer" />
              <button className="btn btn-secondary btn-sm" onClick={() => setAxes(null)}>{t("Discard")}</button>
              <button className="btn btn-primary btn-sm" onClick={async () => {
                const a = drafts.current.axes;
                if (!a) return;
                // Removing an axis also changes masters, instances and layers: say so first.
                const removed = info.axes.filter((old) => !a.some((x) => x.tag === old.tag));
                if (removed.length) {
                  const names = removed.map((x) => x.name).join(", ");
                  const ok = window.confirm(
                    removed.length === 1
                      ? t("Remove {names}?\n\nMasters and instances lose their position on this axis, and intermediate and alternate layers that use it are deleted. You can undo this with Ctrl+Z.", { names })
                      : t("Remove {names}?\n\nMasters and instances lose their position on these axes, and intermediate and alternate layers that use them are deleted. You can undo this with Ctrl+Z.", { names })
                  );
                  if (!ok) return;
                }
                if (await apply(() => backend.setAxes(a))) setAxes(null);
              }}>{t("Apply axes")}</button>
            </div>
          )}
        </section>

        <section className="fi-section" data-group="masters" aria-labelledby="fi-masters">
          <div className="fi-head">
            <h2 id="fi-masters" className="k-subtitle">{t("Masters")}</h2>
            <span className="spacer" />
            <button className="btn btn-secondary btn-sm" onClick={() => setAdding({ name: t("Master {n}", { n: info.masters.length + 1 }), copyFrom: info.defaultMasterId, location: Object.fromEntries(info.axes.map((a) => [a.tag, designBounds(a)[2]])) })}>
              <AddIcon size={14} /> {t("Add master")}
            </button>
            <button className="btn btn-secondary btn-sm" onClick={openForeign}>
              <FolderIcon size={14} /> {t("Add from font…")}
            </button>
          </div>
          {info.masters.map((m) => (
            <div key={m.id} className="fi-master">
              <div className="fi-row">
                <label className="info-field"><span className="k-caption muted">{t("Name")}</span>
                  <Field label={t("Master name")} value={m.name} width={160} mono={false} numeric={false} onCommit={(v) => updateMaster(m, { name: v })} />
                </label>
                {locFields(m.location, (loc) => updateMaster(m, { location: loc }), `${m.name}`)}
                <span className="info-sep" />
                {(["ascender", "capHeight", "xHeight", "descender", "italicAngle"] as const).map((k) => (
                  <label key={k} className="info-field">
                    <span className="k-caption muted">{t({ ascender: "Ascender", capHeight: "Cap height", xHeight: "x-height", descender: "Descender", italicAngle: "Italic angle" }[k])}</span>
                    <Field label={`${m.name} ${k}`} value={m.metrics[k]} width={64} onCommit={(v) => updateMaster(m, { metrics: { ...m.metrics, [k]: num(v) } })} />
                  </label>
                ))}
                <span className="spacer" />
                <button className={`btn btn-sm ${m.alignmentZones.length ? "btn-primary" : "btn-secondary"}`} aria-expanded={hintsOpen === `${m.id}:zones`}
                  title={t("Edges that line up on one pixel row at small sizes")} onClick={() => setHintsOpen(hintsOpen === `${m.id}:zones` ? null : `${m.id}:zones`)}>
                  {m.alignmentZones.length ? tn(m.alignmentZones.length, "{n} zone", "{n} zones") : t("Zones")}
                </button>
                <button className={`btn btn-sm ${Object.values(m.vertical ?? {}).some((v) => v != null) ? "btn-primary" : "btn-secondary"}`} aria-expanded={hintsOpen === `${m.id}:spacing`}
                  title={t("Line spacing and underline values written to the OS/2, hhea and post tables")} onClick={() => setHintsOpen(hintsOpen === `${m.id}:spacing` ? null : `${m.id}:spacing`)}>
                  {t("Line spacing")}
                </button>
                <button className={`btn btn-sm ${m.stems.length ? "btn-primary" : "btn-secondary"}`} aria-expanded={hintsOpen === `${m.id}:stems`}
                  title={t("The common stroke widths, used for hinting")} onClick={() => setHintsOpen(hintsOpen === `${m.id}:stems` ? null : `${m.id}:stems`)}>
                  {m.stems.length ? tn(m.stems.length, "{n} stem", "{n} stems") : t("Stems")}
                </button>
                {m.id === info.defaultMasterId && <span className="pill pill-group" title={t("At the default location of every axis")}>{t("Default")}</span>}
                <button className="icon-btn" aria-label={t("Remove {name}", { name: m.name })} title={info.masters.length === 1 ? t("A font needs at least one master") : t("Remove master")} disabled={info.masters.length === 1}
                  onClick={() => window.confirm(t("Remove the master “{name}” and its outlines and kerning? You can undo this.", { name: m.name })) && apply(() => backend.removeMaster(m.id))}>
                  <CloseIcon size={14} />
                </button>
              </div>
              {hintsOpen === `${m.id}:zones` && (
                <div className="fi-hint-editor">
                  <p className="k-caption muted">{t("Zones keep edges like the baseline and x-height on one pixel row at small sizes. A positive size marks a top edge, a negative size a bottom edge.")}</p>
                  <div className="fi-hint-rows">
                    {m.alignmentZones.map((z, i) => (
                      <div key={i} className="fi-hint-row">
                        <label className="info-field"><span className="k-caption muted">{t("Position")}</span>
                          <Field label={t("Zone position")} value={z.position} onCommit={(v) => apply(() => backend.setAlignmentZones(m.id, m.alignmentZones.map((x, j) => (j === i ? { ...x, position: num(v) } : x))))} />
                        </label>
                        <label className="info-field"><span className="k-caption muted">{t("Size")}</span>
                          <Field label={t("Zone size")} value={z.size} onCommit={(v) => apply(() => backend.setAlignmentZones(m.id, m.alignmentZones.map((x, j) => (j === i ? { ...x, size: num(v) } : x))))} />
                        </label>
                        <button className="icon-btn" aria-label={t("Remove zone")} title={t("Remove zone")}
                          onClick={() => apply(() => backend.setAlignmentZones(m.id, m.alignmentZones.filter((_, j) => j !== i)))}><CloseIcon size={14} /></button>
                      </div>
                    ))}
                  </div>
                  <button className="btn btn-secondary btn-sm" onClick={() => apply(() => backend.setAlignmentZones(m.id, [...m.alignmentZones, { position: 0, size: 10 }]))}>
                    <AddIcon size={14} /> {t("Add zone")}
                  </button>
                </div>
              )}
              {hintsOpen === `${m.id}:spacing` && (
                <div className="fi-hint-editor">
                  <p className="k-caption muted">{t("How far apart lines of text sit, and where the underline goes. Leave a field empty and it's worked out from the ascender and descender on export.")}</p>
                  <div className="fi-hint-rows fi-spacing-rows">
                    {LINE_SPACING.map(({ key, label }) => (
                      <label key={key} className="info-field"><span className="k-caption muted">{t(label)}</span>
                        <Field label={`${m.name} ${t(label)}`} value={m.vertical?.[key] ?? ""} placeholder={t("auto")}
                          onCommit={(v) => apply(() => backend.setVerticalMetrics(m.id, { ...(m.vertical ?? {}), [key]: v.trim() === "" ? null : num(v) }))} />
                      </label>
                    ))}
                  </div>
                  <label className="check-row">
                    <input type="checkbox" checked={info.useTypoMetrics} onChange={(e) => apply(() => backend.setUseTypoMetrics(e.target.checked))} />
                    <span className="k-body">{t("Use Typo Metrics")}</span>
                    <span className="k-caption muted">{t("whole font: apps space lines by the typo values")}</span>
                  </label>
                </div>
              )}
              {hintsOpen === `${m.id}:stems` && (
                <div className="fi-hint-editor">
                  <p className="k-caption muted">{t("Stems are the common stroke widths. PostScript exports use them for hints; TrueType uses the first horizontal stem as its guide width.")}</p>
                  <div className="fi-hint-rows">
                    {m.stems.map((s, i) => (
                      <div key={i} className="fi-hint-row">
                        <label className="info-field"><span className="k-caption muted">{t("Name")}</span>
                          <Field label={t("Stem name")} value={s.name} width={120} mono={false} numeric={false} onCommit={(v) => apply(() => backend.setStems(m.id, m.stems.map((x, j) => (j === i ? { ...x, name: v } : x))))} />
                        </label>
                        <label className="info-field"><span className="k-caption muted">{t("Direction")}</span>
                          <select className="select" value={s.horizontal ? "h" : "v"} aria-label={t("Stem direction")}
                            onChange={(e) => apply(() => backend.setStems(m.id, m.stems.map((x, j) => (j === i ? { ...x, horizontal: e.target.value === "h" } : x))))}>
                            <option value="h">{t("Horizontal")}</option>
                            <option value="v">{t("Vertical")}</option>
                          </select>
                        </label>
                        <label className="info-field"><span className="k-caption muted">{t("Width")}</span>
                          <Field label={t("Stem width")} value={s.value} onCommit={(v) => apply(() => backend.setStems(m.id, m.stems.map((x, j) => (j === i ? { ...x, value: num(v) } : x))))} />
                        </label>
                        <button className="icon-btn" aria-label={t("Remove stem")} title={t("Remove stem")}
                          onClick={() => apply(() => backend.setStems(m.id, m.stems.filter((_, j) => j !== i)))}><CloseIcon size={14} /></button>
                      </div>
                    ))}
                  </div>
                  <button className="btn btn-secondary btn-sm" onClick={() => apply(() => backend.setStems(m.id, [...m.stems, { name: t("Stem {n}", { n: m.stems.length + 1 }), horizontal: true, value: 80 }]))}>
                    <AddIcon size={14} /> {t("Add stem")}
                  </button>
                </div>
              )}
            </div>
          ))}
          <div className="fi-head fi-subhead">
            <h3 className="k-label">{t("Number values")}</h3>
            <span className="k-caption muted">{t("Numbers for feature code: write $name or ${name*2}. Static fonts get each master's value (interpolated for instances); variable fonts get a value that varies.")}</span>
            <span className="spacer" />
            <button className="btn btn-secondary btn-sm"
              onClick={() => {
                let n = 1;
                while (info.numbers[`number${n}`]) n++;
                apply(() => backend.setNumbers({ ...info.numbers, [`number${n}`]: Object.fromEntries(info.masters.map((m) => [m.id, 0])) }));
              }}>
              <AddIcon size={14} /> {t("Add number")}
            </button>
          </div>
          {Object.keys(info.numbers).length > 0 && (
            <div className="fi-numbers" role="table" aria-label={t("Number values")}>
              <div className="fi-numbers-row fi-params-head" role="row" style={{ gridTemplateColumns: `160px repeat(${info.masters.length}, 90px) 32px` }}>
                <span className="k-caption muted" role="columnheader">{t("Name")}</span>
                {info.masters.map((m) => <span key={m.id} className="k-caption muted" role="columnheader">{m.name}</span>)}
                <span />
              </div>
              {Object.entries(info.numbers).map(([name, values]) => {
                const rename = (to: string) => {
                  if (to === name) return;
                  const next: Record<string, Record<string, number>> = {};
                  for (const [k, v] of Object.entries(info.numbers)) next[k === name ? to : k] = v;
                  apply(() => backend.setNumbers(next));
                };
                return (
                  <div key={name} className="fi-numbers-row" role="row" style={{ gridTemplateColumns: `160px repeat(${info.masters.length}, 90px) 32px` }}>
                    <Field label={t("Number name")} value={name} width={150} mono numeric={false} onCommit={rename} />
                    {info.masters.map((m) => (
                      <Field key={m.id} label={t("{name} in {master}", { name, master: m.name })} value={values[m.id] ?? 0} width={80}
                        onCommit={(v) => apply(() => backend.setNumbers({ ...info.numbers, [name]: { ...values, [m.id]: Number(v) } }))} />
                    ))}
                    <button className="icon-btn" aria-label={t("Remove {name}", { name })} title={t("Remove number")}
                      onClick={() => { const next = { ...info.numbers }; delete next[name]; apply(() => backend.setNumbers(next)); }}><CloseIcon size={14} /></button>
                  </div>
                );
              })}
            </div>
          )}
          <div className="fi-head fi-subhead">
            <h3 className="k-label">{t("Virtual masters")}</h3>
            <span className="k-caption muted">{t("A location only glyphs with intermediate layers there use, such as a Crossbar axis that changes A, E, F and H. Other glyphs stay as they are.")}</span>
            <span className="spacer" />
            <button className="btn btn-secondary btn-sm" disabled={info.axes.length === 0}
              onClick={() => apply(() => backend.setVirtualMasters([...info.virtualMasters, { name: t("Virtual master {n}", { n: info.virtualMasters.length + 1 }), location: Object.fromEntries(info.axes.map((a) => [a.tag, designBounds(a)[1]])) }]))}>
              <AddIcon size={14} /> {t("Add virtual master")}
            </button>
          </div>
          {info.virtualMasters.map((vm: VirtualMaster, i: number) => {
            const put = (p: Partial<VirtualMaster>) => apply(() => backend.setVirtualMasters(info.virtualMasters.map((x, j) => (j === i ? { ...x, ...p } : x))));
            return (
              <div key={i} className="fi-row fi-master fi-virtual">
                <label className="info-field"><span className="k-caption muted">{t("Name")}</span>
                  <Field label={t("Virtual master name")} value={vm.name} width={160} mono={false} numeric={false} onCommit={(v) => put({ name: v })} />
                </label>
                {locFields(vm.location, (loc) => put({ location: loc }), vm.name)}
                <span className="spacer" />
                <button className="icon-btn" aria-label={t("Remove {name}", { name: vm.name })} title={t("Remove virtual master")}
                  onClick={() => apply(() => backend.setVirtualMasters(info.virtualMasters.filter((_, j) => j !== i)))}><CloseIcon size={14} /></button>
              </div>
            );
          })}
          {adding && (
            <div className="fi-master fi-adding">
              <div className="fi-row">
                <label className="info-field"><span className="k-caption muted">{t("New master name")}</span>
                  <input className="metric-input text-field" style={{ width: 160 }} value={adding.name} onChange={(e) => setAdding({ ...adding, name: e.target.value })} />
                </label>
                <label className="info-field"><span className="k-caption muted">{t("Copy outlines from")}</span>
                  <select className="select" value={adding.copyFrom} onChange={(e) => setAdding({ ...adding, copyFrom: e.target.value })}>
                    {info.masters.map((m) => <option key={m.id} value={m.id}>{m.name}</option>)}
                  </select>
                </label>
                {locFields(adding.location, (loc) => setAdding({ ...adding, location: loc }), t("New master"))}
                <span className="spacer" />
                <button className="btn btn-secondary btn-sm" onClick={() => setAdding(null)}>{t("Cancel")}</button>
                <button className="btn btn-primary btn-sm" onClick={async () => { if (await apply(() => backend.addMaster(adding.copyFrom, adding.name, adding.location))) setAdding(null); }}>{t("Add")}</button>
              </div>
              {info.axes.length === 0 && <p className="k-caption muted">{t("Tip: add an axis first so masters can sit at different locations.")}</p>}
            </div>
          )}
        </section>

        <section className="fi-section" data-group="exports" aria-labelledby="fi-instances">
          <div className="fi-head">
            <h2 id="fi-instances" className="k-subtitle">{t("Instances")}</h2>
            <span className="spacer" />
            <button className="btn btn-secondary btn-sm" disabled={curInst().length === 0}
              title={t("Interpolate one master per instance and open them as a new unsaved font.")}
              onClick={onGenerateInstances}>
              {t("Generate as New Font")}
            </button>
            <button className="btn btn-secondary btn-sm" disabled={info.axes.length === 0} title={info.axes.length === 0 ? t("Add an axis first") : undefined}
              onClick={() => setInstances([...curInst(), { name: t("Instance {n}", { n: curInst().length + 1 }), location: Object.fromEntries(info.axes.map((a) => [a.tag, designBounds(a)[1]])), export: true }])}>
              <AddIcon size={14} /> {t("Add instance")}
            </button>
          </div>
          {instDraft.length === 0 ? (
            <p className="k-caption muted">{t("No instances. Instances are the named styles (e.g. SemiBold) that get exported.")}</p>
          ) : instDraft.map((it, i) => {
            const patch = (p: Partial<Instance>) => setInstances(curInst().map((x, j) => (j === i ? { ...x, ...p } : x)));
            const link = it.styleLink;
            const kind = link ? (link.bold && link.italic ? "boldItalic" : link.bold ? "bold" : "italic") : "";
            const setKind = (k: string) => patch({
              styleLink: k === "" ? undefined : { bold: k === "bold" || k === "boldItalic", italic: k === "italic" || k === "boldItalic", base: link?.base ?? "" },
            });
            return (
            <div key={i} className="fi-row fi-instance">
              <label className="info-field"><span className="k-caption muted">{t("Name")}</span>
                <Field label={t("Instance name")} value={it.name} width={160} mono={false} numeric={false} onCommit={(v) => patch({ name: v })} />
              </label>
              <label className="info-field" title={t("Export this instance as part of another family, such as Lexend Deca. Empty uses the font's family name.")}><span className="k-caption muted">{t("Family name")}</span>
                <Field label={t("Instance family name")} value={it.familyName ?? ""} width={160} mono={false} numeric={false} placeholder={info.familyName} onCommit={(v) => {
                  const f = v.trim();
                  if ([...f].length > 63) { onError(t("Family name \"{name}\" is too long; use at most 63 characters.", { name: f })); return false; }
                  patch({ familyName: f === "" ? undefined : f });
                }} />
              </label>
              {hasLabels(axesDraft) && (
                <span className="k-caption muted axis-user" title={t("The style name apps show for this instance, from the axis value names.")}>
                  {t("STAT name: {name}", { name: statStyleName(axesDraft, it, instDraft) })}
                </span>
              )}
              {hasLabels(axesDraft) && (
                <fieldset className="fi-elidable" title={t("Marks the axis value name at this instance's location as one apps may leave out, like Regular on the weight axis (Elidable STAT Axis Value Name).")}>
                  <legend className="k-caption muted">{t("Elidable")}</legend>
                  {axesDraft.filter((a) => (a.labels?.length ?? 0) > 0).map((a) => {
                    const on = (it.names?.elidableStatAxes ?? []).includes(a.tag);
                    return (
                      <label key={a.tag} className="check-row">
                        <input type="checkbox" checked={on} aria-label={t("Elidable {axis} name for {name}", { axis: a.name, name: it.name })} onChange={(e) => {
                          const rest = (it.names?.elidableStatAxes ?? []).filter((x) => x !== a.tag);
                          const list = e.target.checked ? [...rest, a.tag] : rest;
                          patch({ names: { ...it.names, elidableStatAxes: list.length ? list : undefined } });
                        }} />
                        <span className="k-caption">{a.name}</span>
                      </label>
                    );
                  })}
                </fieldset>
              )}
              {locFields(it.location, (loc) => patch({ location: loc }), it.name)}
              <label className="info-field" title={t("OS/2 weight class, 100 (Thin) to 900 (Black). Empty picks it from the weight axis.")}><span className="k-caption muted">{t("Weight class")}</span>
                <Field label={t("Weight class")} value={it.weightClass ?? ""} width={88} placeholder={t("Automatic")} onCommit={(v) => {
                  if (v.trim() === "") return patch({ weightClass: undefined });
                  const n = num(v);
                  if (!Number.isInteger(n) || n < 1 || n > 1000) { onError(t("Weight class must be a whole number from 1 to 1000.")); return false; }
                  patch({ weightClass: n });
                }} />
              </label>
              <label className="info-field" title={t("OS/2 width class, 1 (ultra-condensed) to 9 (ultra-expanded). Empty picks it from the width axis.")}><span className="k-caption muted">{t("Width class")}</span>
                <Field label={t("Width class")} value={it.widthClass ?? ""} width={88} placeholder={t("Automatic")} onCommit={(v) => {
                  if (v.trim() === "") return patch({ widthClass: undefined });
                  const n = num(v);
                  if (!Number.isInteger(n) || n < 1 || n > 9) { onError(t("Width class must be a whole number from 1 to 9.")); return false; }
                  patch({ widthClass: n });
                }} />
              </label>
              <label className="info-field" title={t("Links this instance as the bold or italic of another, so apps group them into one family.")}><span className="k-caption muted">{t("Style link")}</span>
                <select className="select" value={kind} aria-label={t("Style link")} onChange={(e) => setKind(e.target.value)}>
                  <option value="">{t("None")}</option>
                  <option value="bold">{t("Bold")}</option>
                  <option value="italic">{t("Italic")}</option>
                  <option value="boldItalic">{t("Bold Italic")}</option>
                </select>
              </label>
              {kind !== "" && (
                <label className="info-field"><span className="k-caption muted">{t("Style of")}</span>
                  <select className="select" value={link?.base ?? ""} aria-label={t("Style of")} onChange={(e) => patch({ styleLink: { bold: link?.bold ?? false, italic: link?.italic ?? false, base: e.target.value } })}>
                    <option value="">{t("This family")}</option>
                    {instDraft.filter((_, j) => j !== i).map((o) => <option key={o.name} value={o.name}>{o.name}</option>)}
                  </select>
                </label>
              )}
              <label className="check-row fi-export" title={t("Export this instance as a static font")}>
                <input type="checkbox" checked={it.export !== false} aria-label={t("Export {name}", { name: it.name })}
                  onChange={(e) => patch({ export: e.target.checked })} />
                <span className="k-caption">{t("Export")}</span>
              </label>
              <label className="check-row fi-export" title={t("Autohint this instance's TrueType fonts when the export turns hinting on (Autohint). Off exports it unhinted.")}>
                <input type="checkbox" checked={it.autohint !== false} aria-label={t("Autohint {name}", { name: it.name })}
                  onChange={(e) => patch({ autohint: e.target.checked ? undefined : false })} />
                <span className="k-caption">{t("Autohint")}</span>
              </label>
              <span className="spacer" />
              <SwapFields rename={it.renameGlyphs} remove={it.removeGlyphs} onChange={(renameGlyphs, removeGlyphs) => patch({ renameGlyphs, removeGlyphs })} />
              <button className="icon-btn" aria-label={t("Remove {name}", { name: it.name })} title={t("Remove instance")} onClick={() => setInstances(curInst().filter((_, j) => j !== i))}><CloseIcon size={14} /></button>
              <NamingFields names={it.names} instance={it.name} onChange={(names) => patch({ names })} />
              <GlyphProcessingFields value={it} rename={it.renameGlyphs} remove={it.removeGlyphs} owner={it.name} variable={false} onChange={(p) => patch(p)} />
              <FeatureCodeFields params={it.featureParameters} owner={it.name} onChange={(featureParameters) => patch({ featureParameters })} />
            </div>
            );
          })}
          {instances && (
            <div className="fi-actions">
              <span className="spacer" />
              <button className="btn btn-secondary btn-sm" onClick={() => setInstances(null)}>{t("Discard")}</button>
              <button className="btn btn-primary btn-sm" onClick={async () => { const list = drafts.current.instances; if (list && await apply(() => backend.setInstances(list))) setInstances(null); }}>{t("Apply instances")}</button>
            </div>
          )}
        </section>

        <section className="fi-section" data-group="exports" aria-labelledby="fi-variable">
          <div className="fi-head">
            <h2 id="fi-variable" className="k-subtitle">{t("Variable fonts")}</h2>
            <span className="spacer" />
            <button className="btn btn-secondary btn-sm" disabled={info.axes.length === 0} title={info.axes.length === 0 ? t("Add an axis first") : undefined}
              onClick={() => { const n = curVf().length + 1; setVariableFonts([...curVf(), { name: t("Variable font {n}", { n }), fileName: `${info.familyName.replace(/\s+/g, "")}-${n}`, axes: [], glyphs: [] }]); }}>
              <AddIcon size={14} /> {t("Add variable font")}
            </button>
          </div>
          <p className="k-caption muted">{t("Extra variable fonts built from this family. Each keeps a range of the axes and a set of glyphs; export writes one file per entry.")}</p>
          {vfDraft.length === 0 ? (
            <p className="k-caption muted">{t("No extra variable fonts. Export builds one variable font covering every axis.")}</p>
          ) : vfDraft.map((vf, i) => {
            const patch = (p: Partial<VariableFontSetting>) => setVariableFonts(curVf().map((x, j) => (j === i ? { ...x, ...p } : x)));
            const setRange = (tag: string, field: "min" | "max", v: string) => {
              const val = v.trim() === "" ? null : num(v);
              const axes = [...(vf.axes ?? [])];
              const idx = axes.findIndex((s) => s.tag === tag);
              const next = { ...(idx >= 0 ? axes[idx] : { tag, min: null, max: null }), [field]: val };
              if (next.min == null && next.max == null) { if (idx >= 0) axes.splice(idx, 1); }
              else if (idx >= 0) axes[idx] = next; else axes.push(next);
              patch({ axes });
            };
            return (
            <div key={i} className="fi-vf">
              <div className="fi-row">
                <label className="info-field"><span className="k-caption muted">{t("Name")}</span>
                  <Field label={t("Variable font name")} value={vf.name} width={160} mono={false} numeric={false} onCommit={(v) => patch({ name: v })} />
                </label>
                <label className="info-field" title={t("The file name without extension, e.g. MyFont-Light.")}><span className="k-caption muted">{t("File name")}</span>
                  <Field label={t("File name")} value={vf.fileName} width={200} mono={false} numeric={false} onCommit={(v) => patch({ fileName: v })} />
                </label>
                <label className="info-field" title={t("A family name of its own for this variable font, so it can be installed next to the static fonts. Empty uses the font's family name.")}><span className="k-caption muted">{t("Family name")}</span>
                  <Field label={t("Variable font family name")} value={vf.familyName ?? ""} width={180} mono={false} numeric={false} placeholder={info.familyName} onCommit={(v) => {
                    const f = v.trim();
                    if ([...f].length > 63) { onError(t("Family name \"{name}\" is too long; use at most 63 characters.", { name: f })); return false; }
                    patch({ familyName: f === "" || f === info.familyName ? undefined : f });
                  }} />
                </label>
                <NameRecordsField records={vf.nameRecords} label={t("Name table entries for {name}", { name: vf.name })} onChange={(nameRecords) => patch({ nameRecords: nameRecords.length ? nameRecords : undefined })} />
                {info.masters.length > 1 && (
                  <label className="info-field" title={t("The master this variable font uses as its default (Variable Font Origin). Apps show the default when no axis is set.")}><span className="k-caption muted">{t("Origin")}</span>
                    <select className="select" value={vf.origin ?? ""} aria-label={t("Origin of {name}", { name: vf.name })} onChange={(e) => patch({ origin: e.target.value === "" ? undefined : e.target.value })}>
                      <option value="">{t("Default master")}</option>
                      {info.masters.map((m) => <option key={m.id} value={m.name}>{m.name}</option>)}
                    </select>
                  </label>
                )}
                <label className="check-row fi-export" title={t("Write a STAT table with the axis value names (Export STAT Table). Turn it off to leave the table out.")}>
                  <input type="checkbox" checked={vf.exportStat !== false} aria-label={t("Export STAT table for {name}", { name: vf.name })}
                    onChange={(e) => patch({ exportStat: e.target.checked ? undefined : false })} />
                  <span className="k-caption">{t("STAT table")}</span>
                </label>
                <span className="spacer" />
                <button className="icon-btn" aria-label={t("Remove {name}", { name: vf.name })} title={t("Remove variable font")} onClick={() => setVariableFonts(curVf().filter((_, j) => j !== i))}><CloseIcon size={14} /></button>
              </div>
              <div className="fi-row">
                {info.axes.map((a) => {
                  const sub = (vf.axes ?? []).find((s) => s.tag === a.tag);
                  return (
                    <div key={a.tag} className="info-field fi-vf-axis">
                      <span className="k-caption muted">{a.name} ({a.tag})</span>
                      <span className="fi-vf-range">
                        <Field label={t("{name} minimum", { name: a.name })} value={sub?.min ?? ""} width={72} placeholder={String(a.min)} onCommit={(v) => setRange(a.tag, "min", v)} />
                        <span className="k-caption muted">–</span>
                        <Field label={t("{name} maximum", { name: a.name })} value={sub?.max ?? ""} width={72} placeholder={String(a.max)} onCommit={(v) => setRange(a.tag, "max", v)} />
                      </span>
                    </div>
                  );
                })}
                <label className="info-field"><span className="k-caption muted">{t("Glyphs")}</span>
                  <Field label={t("Glyphs")} value={(vf.glyphs ?? []).join(", ")} width={240} mono={false} numeric={false}
                    onCommit={(v) => patch({ glyphs: v.split(/[,\s]+/).map((s) => s.trim()).filter(Boolean) })} />
                </label>
                <span className="k-caption muted fi-vf-glyphs-hint">{t("Empty keeps every glyph. * and ? work as wildcards.")}</span>
                <SwapFields rename={vf.renameGlyphs} remove={vf.removeGlyphs} onChange={(renameGlyphs, removeGlyphs) => patch({ renameGlyphs, removeGlyphs })} />
              </div>
              <GlyphProcessingFields value={vf} rename={vf.renameGlyphs} remove={vf.removeGlyphs} owner={vf.name} variable={true} onChange={(p) => patch(p)} />
              <FeatureCodeFields params={vf.featureParameters} owner={vf.name} onChange={(featureParameters) => patch({ featureParameters })} />
              {info.masters.length > 1 && (
                <fieldset className="fi-row fi-vf-masters" title={t("Masters left out of this variable font. Its axes shrink to the masters that remain.")}>
                  <legend className="k-caption muted">{t("Disable masters")}</legend>
                  {info.masters.map((m) => {
                    const off = (vf.disabledMasters ?? []).includes(m.name);
                    return (
                      <label key={m.id} className="check-row">
                        <input type="checkbox" checked={off} aria-label={t("Leave out {name}", { name: m.name })} onChange={(e) => {
                          const rest = (vf.disabledMasters ?? []).filter((n) => n !== m.name);
                          patch({ disabledMasters: e.target.checked ? [...rest, m.name] : rest });
                        }} />
                        {m.name}
                      </label>
                    );
                  })}
                </fieldset>
              )}
            </div>
            );
          })}
          {variableFonts && (
            <div className="fi-actions">
              <span className="spacer" />
              <button className="btn btn-secondary btn-sm" onClick={() => setVariableFonts(null)}>{t("Discard")}</button>
              <button className="btn btn-primary btn-sm" onClick={async () => { const list = drafts.current.variableFonts; if (list && await apply(() => backend.setVariableFonts(list))) setVariableFonts(null); }}>{t("Apply variable fonts")}</button>
            </div>
          )}
        </section>

        <div className="fi-group-wrap" data-group="features"><FeaturesSection version={version} onChanged={onChanged} onError={onError} /></div>

        <section className="fi-section" data-group="other" aria-labelledby="fi-order">
          <div className="fi-head">
            <h2 id="fi-order" className="k-subtitle">{t("Glyph order")}</h2>
            <span className="k-caption muted">{t("The order glyphs export in. One name per line; .notdef is always first.")}</span>
          </div>
          <textarea
            className="notes-area glyph-order-area"
            aria-label={t("Glyph order")}
            spellCheck={false}
            value={orderText ?? info.glyphOrder.join("\n")}
            onChange={(e) => setOrderText(e.target.value)}
          />
          {orderText !== null && (
            <div className="fi-actions">
              <span className="k-caption muted">{t("Every glyph must appear once. The order is saved with the font and used on export.")}</span>
              <span className="spacer" />
              <button className="btn btn-secondary btn-sm" onClick={() => setOrderText(null)}>{t("Discard")}</button>
              <button className="btn btn-primary btn-sm" onClick={async () => {
                const names = orderText.split("\n").map((s) => s.trim()).filter((s) => s.length > 0);
                if (await apply(() => backend.setGlyphOrder(names))) setOrderText(null);
              }}>{t("Apply order")}</button>
            </div>
          )}
        </section>

        <section className="fi-section" data-group="notes" aria-labelledby="fi-notes">
          <div className="fi-head">
            <h2 id="fi-notes" className="k-subtitle">{t("Notes")}</h2>
            <span className="k-caption muted">{t("Saved with the font, never exported. Markdown: # headings, **bold**, _italic_, - lists, [links](https://…).")}</span>
            <span className="spacer" />
            <div className="segmented" role="group" aria-label={t("Notes view")}>
              <button type="button" className={`segment${!notesPreview ? " is-selected" : ""}`} aria-pressed={!notesPreview} onClick={() => setNotesPreview(false)}>{t("Edit")}</button>
              <button type="button" className={`segment${notesPreview ? " is-selected" : ""}`} aria-pressed={notesPreview} onClick={() => setNotesPreview(true)}>{t("Preview")}</button>
            </div>
          </div>
          {notesPreview ? (info.notes.trim() ? <NotesPreview text={info.notes} /> : <p className="k-caption muted">{t("No notes yet.")}</p>) : <textarea
            className="notes-area"
            aria-label={t("Notes")}
            defaultValue={info.notes}
            key={info.notes}
            placeholder={t("Notes to yourself about this font.")}
            onBlur={(e) => { const v = e.currentTarget.value; if (v !== info.notes) apply(() => backend.setNotes(v)); }}
            onKeyDown={(e) => { if (e.key === "Escape") { e.currentTarget.value = info.notes; e.currentTarget.blur(); } }}
          />}
        </section>

        <section className="fi-section" data-group="params" aria-labelledby="fi-params">
          <div className="fi-head">
            <h2 id="fi-params" className="k-subtitle">{t("Custom parameters")}</h2>
            <span className="k-caption muted">{t("Settings from a Glyphs source. They're kept with the font and written back by Save As .glyphs. Kernest applies Export sbix Table, SBIX to SVG and Color Layers to SVG when it exports; it keeps the others without using them.")}</span>
            <span className="spacer" />
            <select className="metric-input fi-param-add" aria-label={t("Add an export switch")} value=""
              onChange={(e) => {
                const [name, value] = e.target.value.split("|");
                if (name) apply(() => backend.setCustomParameters([...info.customParameters, { name, value }]));
              }}>
              <option value="">{t("Add export switch…")}</option>
              <option value="Export sbix Table|0">{t("Export sbix Table (off: leave sbix out)")}</option>
              <option value="SBIX to SVG|128">{t("SBIX to SVG (pixel size)")}</option>
              <option value="Color Layers to SVG|1">{t("Color Layers to SVG")}</option>
            </select>
            <button className="btn btn-secondary btn-sm" onClick={() => apply(() => backend.setCustomParameters([...info.customParameters, { name: t("New parameter"), value: "1" }]))}>
              <AddIcon size={14} /> {t("Add parameter")}
            </button>
          </div>
          {info.customParameters.length === 0 ? (
            <p className="k-caption muted">{t("None. Parameters appear here when you import a Glyphs file that has them.")}</p>
          ) : (
            <div className="fi-params" role="table" aria-label={t("Custom parameters")}>
              <div className="fi-params-row fi-params-head" role="row">
                <span className="k-caption muted" role="columnheader">{t("Property")}</span>
                <span className="k-caption muted" role="columnheader">{t("Value")}</span>
                <span className="k-caption muted" role="columnheader">{t("On")}</span>
                <span />
              </div>
              {info.customParameters.map((p, i) => {
                const save = (patch: Partial<typeof p>) => apply(() => backend.setCustomParameters(info.customParameters.map((x, j) => (j === i ? { ...x, ...patch } : x))));
                return (
                  <div key={`${i}:${p.name}`} className="fi-params-row" role="row">
                    <Field label={t("Parameter name")} value={p.name} width={220} mono={false} numeric={false} onCommit={(v) => save({ name: v })} />
                    <textarea className="metric-input fi-param-value" aria-label={t("{name} value", { name: p.name })} defaultValue={p.value} key={p.value} rows={Math.min(6, p.value.split("\n").length)}
                      onBlur={(e) => { const el = e.currentTarget; const v = el.value; if (v !== p.value) save({ value: v }).then((ok) => { if (!ok) el.value = p.value; }); }} />
                    <input type="checkbox" checked={!p.disabled} aria-label={t("Use {name}", { name: p.name })} onChange={(e) => save({ disabled: !e.target.checked })} />
                    <button className="icon-btn" aria-label={t("Remove {name}", { name: p.name })} title={t("Remove parameter")}
                      onClick={() => apply(() => backend.setCustomParameters(info.customParameters.filter((_, j) => j !== i)))}><CloseIcon size={14} /></button>
                  </div>
                );
              })}
            </div>
          )}
        </section>

        <section className="fi-section" data-group="other" aria-labelledby="fi-hinting">
          <div className="fi-head">
            <h2 id="fi-hinting" className="k-subtitle">{t("Hinting")}</h2>
            <span className="k-caption muted">{t("TrueType autohinting on export (TTF and WOFF2). Turn it on in the Export dialog.")}</span>
          </div>
          <div className="fi-row">
            {([
              ["rangeMin", "Smallest size (px)", "Hinting starts at this many pixels per em"],
              ["rangeMax", "Largest size (px)", "Above this size the font isn't hinted"],
              ["increaseXHeight", "Increase x-height up to (px)", "Rounds the x-height up at small sizes for legibility; 0 is off"],
            ] as const).map(([key, en, hintEn]) => { const label = t(en), hint = t(hintEn); return (
              <label key={key} className="info-field" title={hint}><span className="k-caption muted">{label}</span>
                <Field label={label} value={info.hinting[key]} onCommit={(v) => {
                  const n = num(v);
                  if (!Number.isInteger(n)) { onError(t("{name}: enter a whole number.", { name: label })); return false; }
                  return apply(() => backend.setHinting({ ...info.hinting, [key]: n }));
                }} />
              </label>
            ); })}
            <label className="check-row" title={t("Hint accented letters and other composites as whole glyphs, not through their components")}>
              <input type="checkbox" checked={info.hinting.hintComposites}
                onChange={(e) => apply(() => backend.setHinting({ ...info.hinting, hintComposites: e.target.checked }))} />
              <span className="k-body">{t("Hint composites as whole glyphs")}</span>
            </label>
          </div>
          {(() => {
            const h = info.hinting;
            const setH = (p: Partial<Hinting>) => apply(() => backend.setHinting({ ...h, ...p }));
            const optNum = (key: "hintingLimit" | "fallbackStemWidth", label: string) => (v: string) => {
              if (v.trim() === "") return setH({ [key]: undefined });
              const n = Number(v);
              if (!Number.isInteger(n) || n < 0 || n > 65535) { onError(t("{name}: enter a whole number.", { name: label })); return false; }
              return setH({ [key]: n });
            };
            const text = (key: "stemWidthMode" | "defaultScript" | "fallbackScript" | "xHeightSnappingExceptions") => (v: string) => setH({ [key]: v.trim() === "" ? undefined : v.trim() });
            const more = [h.hintingLimit, h.fallbackStemWidth, h.stemWidthMode, h.defaultScript, h.fallbackScript, h.xHeightSnappingExceptions].filter((x) => x != null).length
              + [h.windowsCompatibility, h.symbol, h.adjustSubglyphs, h.fallbackScaling, h.ttfaTable, h.noInfo].filter(Boolean).length;
            return (
              <details className="fi-processing fi-autohint-more">
                <summary className="k-caption">{more > 0 ? tn(more, "More autohint options ({n} set)", "More autohint options ({n} set)", { n: more }) : t("More autohint options")}</summary>
                <div className="fi-row">
                  <label className="info-field" title={t("Above this size no hinting instructions run at all (-G). 0 means no limit.")}><span className="k-caption muted">{t("Hinting limit (px)")}</span>
                    <Field label={t("Hinting limit (px)")} value={h.hintingLimit ?? ""} placeholder="200" onCommit={optNum("hintingLimit", t("Hinting limit (px)"))} />
                  </label>
                  <label className="info-field" title={t("Stem width for glyphs the autohinter can't measure (-H). Empty uses the default master's first horizontal stem.")}><span className="k-caption muted">{t("Fallback stem width")}</span>
                    <Field label={t("Fallback stem width")} value={h.fallbackStemWidth ?? ""} width={96} placeholder={t("From stems")} onCommit={optNum("fallbackStemWidth", t("Fallback stem width"))} />
                  </label>
                  <label className="info-field" title={t("Three letters for grayscale, GDI ClearType and DirectWrite ClearType: n natural, q quantized, s strong (-a). Kernest's autohinter always uses qsq.")}><span className="k-caption muted">{t("Stem width mode")}</span>
                    <Field label={t("Stem width mode")} value={h.stemWidthMode ?? ""} placeholder={TTFA_DEFAULTS.stemWidthMode} numeric={false} onCommit={text("stemWidthMode")} />
                  </label>
                  <label className="info-field" title={t("The script whose rules hint glyphs no script covers (-D).")}><span className="k-caption muted">{t("Default script")}</span>
                    <Field label={t("Default script")} value={h.defaultScript ?? ""} placeholder={TTFA_DEFAULTS.defaultScript} numeric={false} onCommit={text("defaultScript")} />
                  </label>
                  <label className="info-field" title={t("The script used for glyphs no script rules cover (-f). Empty leaves them unhinted.")}><span className="k-caption muted">{t("Fallback script")}</span>
                    <Field label={t("Fallback script")} value={h.fallbackScript ?? ""} placeholder={TTFA_DEFAULTS.fallbackScript} numeric={false} onCommit={text("fallbackScript")} />
                  </label>
                  <label className="info-field" title={t("Sizes where the x-height isn't snapped, such as 8-12, 15 (-X). Empty lets Kernest pick them from the x-height zone.")}><span className="k-caption muted">{t("X-height snapping exceptions")}</span>
                    <Field label={t("X-height snapping exceptions")} value={h.xHeightSnappingExceptions ?? ""} width={140} placeholder={t("Automatic")} numeric={false} onCommit={text("xHeightSnappingExceptions")} />
                  </label>
                </div>
                <div className="fi-row">
                  {([
                    ["windowsCompatibility", "Windows compatibility", "Adds zones for usWinAscent and usWinDescent so nothing is clipped on Windows (-W)"],
                    ["symbol", "Symbol font", "Hints the font without script rules (-s)"],
                    ["adjustSubglyphs", "Adjust components", "Lets the hinter move the components of composite glyphs (-p)"],
                    ["fallbackScaling", "Fallback scaling", "Scales glyphs of the fallback script instead of hinting them (-S)"],
                    ["ttfaTable", "TTFA table", "Adds a table that records the autohint options used (-t)"],
                    ["noInfo", "No version info", "Leaves the autohinter's details out of the version string (-n)"],
                  ] as const).map(([key, en, hintEn]) => (
                    <label key={key} className="check-row" title={t(hintEn)}>
                      <input type="checkbox" checked={h[key] === true} onChange={(e) => setH({ [key]: e.target.checked || undefined })} />
                      <span className="k-body">{t(en)}</span>
                    </label>
                  ))}
                </div>
              </details>
            );
          })()}
          <h3 className="k-label fi-sub">{t("gasp table")}</h3>
          <p className="k-caption muted">{t("How TrueType exports are rendered at each size range (gasp Table). It replaces the autohinter's own table.")}</p>
          <GaspEditor gasp={info.gasp} onApply={(gasp) => apply(() => backend.setGasp(gasp))} />
          <h3 className="k-label fi-sub">{t("PostScript hints")}</h3>
          <div className="fi-row">
            {([
              ["blueScale", "Blue scale", "How strongly overshoots are kept at small sizes; 0.039625 is the usual value"],
              ["blueShift", "Blue shift", "How far an overshoot must reach past a zone to stay visible"],
              ["blueFuzz", "Blue fuzz", "Extra reach around each zone when matching edges"],
            ] as const).map(([key, en, hintEn]) => { const label = t(en), hint = t(hintEn); return (
              <label key={key} className="info-field" title={hint}><span className="k-caption muted">{label}</span>
                <Field label={label} value={info.psHinting[key]} onCommit={(v) => apply(() => backend.setPsHinting({ ...info.psHinting, [key]: num(v) }))} />
              </label>
            ); })}
          </div>
          <div className="fi-row">
            {([
              ["familyBlues", "Family blues", "The family's top zones and baseline as bottom, top pairs, such as -10, 0, 700, 710. All styles then round them alike at small sizes."],
              ["familyOtherBlues", "Family other blues", "The family's zones below the baseline as bottom, top pairs."],
              ["stemSnapH", "Stem snap H", "Horizontal stem widths for StemSnapH. Empty uses the masters' horizontal stems."],
              ["stemSnapV", "Stem snap V", "Vertical stem widths for StemSnapV. Empty uses the masters' vertical stems."],
            ] as const).map(([key, en, hintEn]) => { const label = t(en); return (
              <label key={key} className="info-field" title={t(hintEn)}><span className="k-caption muted">{label}</span>
                <Field label={label} value={(info.psHinting[key] ?? []).join(", ")} width={150} numeric={false} placeholder={key.startsWith("stem") ? t("From stems") : ""} onCommit={(v) => {
                  const list = numberList(v);
                  if (!list) { onError(t("{name}: enter numbers separated by commas.", { name: label })); return false; }
                  return apply(() => backend.setPsHinting({ ...info.psHinting, [key]: list.length ? list : undefined }));
                }} />
              </label>
            ); })}
          </div>
          <h3 className="k-label fi-sub">{t("Control instructions")}</h3>
          <p className="k-caption muted">{t("TTFAutohint control instructions, applied when exporting TrueType. One instruction per line.")}</p>
          <textarea
            className="notes-area control-area"
            aria-label={t("Control instructions")}
            spellCheck={false}
            defaultValue={info.controlInstructions}
            key={info.controlInstructions}
            placeholder={t("e.g. H point 0 yshift 0.5 @ 12")}
            onBlur={(e) => { const v = e.currentTarget.value; if (v !== info.controlInstructions) apply(() => backend.setControlInstructions(v)); }}
            onKeyDown={(e) => { if (e.key === "Escape") { e.currentTarget.value = info.controlInstructions; e.currentTarget.blur(); } }}
          />
          <p className="k-caption muted">{t("Zones and stems under Masters guide PostScript hinting. For TrueType, ttfautohint ignores manual zones it can't use.")}</p>
        </section>

        <section className="fi-section" data-group="other" aria-labelledby="fi-grid">
          <div className="fi-head">
            <h2 id="fi-grid" className="k-subtitle">{t("Grid")}</h2>
            <span className="k-caption muted">{t("Snap points to a grid while drawing and dragging. Spacing 0 turns the grid off.")}</span>
          </div>
          <div className="fi-row">
            <label className="info-field" title={t("Distance between grid lines, in font units")}>
              <span className="k-caption muted">{t("Spacing")}</span>
              <Field label={t("Spacing")} value={info.grid?.spacing ?? 0} onCommit={(v) => {
                const n = num(v);
                if (!Number.isInteger(n) || n < 0 || n > 65535) { onError(t("{name}: enter a whole number.", { name: t("Spacing") })); return false; }
                return apply(() => backend.setGrid({ spacing: n, subdivision: info.grid?.subdivision || 1 }));
              }} />
            </label>
            <label className="info-field" title={t("Snap steps between two grid lines")}>
              <span className="k-caption muted">{t("Subdivision")}</span>
              <Field label={t("Subdivision")} value={info.grid?.subdivision ?? 1} onCommit={(v) => {
                const n = num(v);
                if (!Number.isInteger(n) || n < 0 || n > 65535) { onError(t("{name}: enter a whole number.", { name: t("Subdivision") })); return false; }
                return apply(() => backend.setGrid({ spacing: info.grid?.spacing ?? 0, subdivision: n }));
              }} />
            </label>
          </div>
        </section>

        <section className="fi-section" data-group="other" aria-labelledby="fi-palettes">
          <div className="fi-head">
            <h2 id="fi-palettes" className="k-subtitle">{t("Colour palettes")}</h2>
            <span className="k-caption muted">{t("Colours for colour layers (COLR and CPAL). Palette 1 is the default; others, such as one for dark backgrounds, have the same number of colours.")}</span>
            <span className="spacer" />
            {info.palettes.length > 0 && (
              <button className="btn btn-secondary btn-sm" onClick={() => apply(() => backend.setPalettes(info.palettes.map((p) => [...p, "#000000ff"])))}>
                <AddIcon size={14} /> {t("Add colour")}
              </button>
            )}
            <button className="btn btn-secondary btn-sm" onClick={() => apply(() => backend.setPalettes(
              info.palettes.length ? [...info.palettes, [...info.palettes[0]]] : [["#e63946ff", "#1d3557ff"]],
            ))}>
              <AddIcon size={14} /> {t("Add palette")}
            </button>
          </div>
          {info.palettes.length === 0 ? (
            <p className="k-caption muted">{t("No palettes. Add one to draw colour glyphs: then use + Colour in the glyph editor.")}</p>
          ) : (
            <table className="fi-table">
              <thead><tr>
                <th>{t("Colour")}</th>
                {info.palettes.map((_, p) => (
                  <th key={p}>
                    {t("Palette {n}", { n: p + 1 })}
                    {p > 0 && (
                      <button className="icon-btn" aria-label={t("Remove palette {n}", { n: p + 1 })} title={t("Remove palette {n}", { n: p + 1 })}
                        onClick={() => apply(() => backend.setPalettes(info.palettes.filter((_, j) => j !== p)))}><CloseIcon size={12} /></button>
                    )}
                  </th>
                ))}
                <th aria-label={t("Actions")} />
              </tr></thead>
              <tbody>
                {info.palettes[0].map((_, c) => (
                  <tr key={c}>
                    <td className="k-caption muted">{c + 1}</td>
                    {info.palettes.map((pal, p) => (
                      <td key={p}>
                        <ColorCell value={pal[c]} label={t("Palette {p}, colour {c}", { p: p + 1, c: c + 1 })}
                          onCommit={(v) => apply(() => backend.setPalettes(info.palettes.map((q, j) => (j === p ? q.map((x, k) => (k === c ? v : x)) : q))))} />
                      </td>
                    ))}
                    <td>
                      <button className="icon-btn" aria-label={t("Remove colour {n}", { n: c + 1 })} title={t("Remove colour {n}", { n: c + 1 })} disabled={info.palettes[0].length === 1}
                        onClick={() => apply(() => backend.setPalettes(info.palettes.map((q) => q.filter((_, k) => k !== c))))}><CloseIcon size={14} /></button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          {info.palettes.length > 0 && info.masters.length > 1 && (
            <div className="fi-master-palettes">
              <h3 className="k-label">{t("Per-master colours")}</h3>
              <p className="k-caption muted">{t("Give a master its own palette to make colour vary between masters in a variable font.")}</p>
              {info.masters.map((m) => {
                const override = info.masterPalettes[m.id];
                return (
                  <div key={m.id} className="fi-master-palette">
                    <label className="check-row">
                      <input type="checkbox" checked={!!override}
                        onChange={(e) => apply(() => backend.setMasterPalettes(m.id, e.target.checked ? info.palettes.map((p) => [...p]) : null))} />
                      <span className="k-body">{m.name}</span>
                    </label>
                    {override && (
                      <table className="fi-table">
                        <thead><tr>
                          <th>{t("Colour")}</th>
                          {override.map((_, p) => <th key={p}>{t("Palette {n}", { n: p + 1 })}</th>)}
                        </tr></thead>
                        <tbody>
                          {override[0].map((_, c) => (
                            <tr key={c}>
                              <td className="k-caption muted">{c + 1}</td>
                              {override.map((pal, p) => (
                                <td key={p}>
                                  <ColorCell value={pal[c]} label={t("{master}, palette {p}, colour {c}", { master: m.name, p: p + 1, c: c + 1 })}
                                    onCommit={(v) => apply(() => backend.setMasterPalettes(m.id, override.map((q, j) => (j === p ? q.map((x, k) => (k === c ? v : x)) : q))))} />
                                </td>
                              ))}
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </section>
      </div>
    </div>
    {foreign && (
      <ForeignMasterDialog path={foreign.path} previews={foreign.previews} axes={info.axes}
        onCancel={() => setForeign(null)} onAdd={addForeign} />
    )}
    </FieldError.Provider>
  );
}
