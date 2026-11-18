import { useEffect, useRef, useState } from "react";
import { backend, errorMessage, type FilterRule, type FilterRuleGroup, type GlyphFilter } from "../lib/backend";
import { CloseIcon } from "./Icons";
import { t, tn } from "../lib/i18n";

interface Props {
  /** The filter being edited, or null for a new smart filter. */
  initial: GlyphFilter | null;
  onCancel: () => void;
  onSave: (filter: GlyphFilter) => Promise<void>;
}

type RuleType = FilterRule["type"];

const RULE_LABELS: Record<RuleType, string> = {
  category: "Category",
  subcategory: "Subcategory",
  case: "Case",
  script: "Script",
  hasComponents: "Components",
  hasAnchors: "Anchors",
  hasUnicode: "Unicode",
  compatible: "Compatibility",
  export: "Export",
  metricsKeysOutOfSync: "Metrics keys",
  colorGlyph: "Colour",
  smartGlyph: "Smart glyph",
  nameContains: "Name contains",
  nameStarts: "Name starts with",
  nameEnds: "Name ends with",
  tagsContain: "Tag contains",
};

const CHOICES: Partial<Record<RuleType, string[]>> = {
  category: ["Uppercase", "Lowercase", "Letter", "Number", "Punctuation", "Symbol", "Separator", "Mark", "Other"],
  subcategory: ["Ligature", "Combining", "Decimal", "Fraction", "Superscript", "Subscript", "Dash", "Quote", "Parenthesis", "Bracket", "Space", "Currency", "Math", "Arrow", "Other", ""],
  case: ["Uppercase", "Lowercase"],
  script: ["Latin", "Greek", "Cyrillic", "Arabic", "Hebrew", "Common", "Other"],
};

const BOOL_LABELS: Partial<Record<RuleType, [string, string]>> = {
  hasComponents: ["Has components", "Has no components"],
  hasAnchors: ["Has anchors", "Has no anchors"],
  hasUnicode: ["Has Unicode", "Has no Unicode"],
  compatible: ["Masters compatible", "Masters incompatible"],
  export: ["Exported", "Not exported"],
  metricsKeysOutOfSync: ["Metrics keys out of sync", "Metrics keys in sync"],
  colorGlyph: ["Colour glyph", "Not a colour glyph"],
  smartGlyph: ["Smart glyph", "Not a smart glyph"],
};

const RULE_TYPES = Object.keys(RULE_LABELS) as RuleType[];

function defaultRule(type: RuleType): FilterRule {
  const choices = CHOICES[type];
  if (choices) return { type, value: choices[0] } as FilterRule;
  if (BOOL_LABELS[type]) return { type, value: true } as FilterRule;
  return { type, value: "" } as FilterRule;
}

const emptyGroup = (): FilterRuleGroup => ({ all: true, rules: [defaultRule("category")], groups: [] });

/** S35 · Smart filter rule builder with a live match count, and list-filter renaming. */
export function SmartFilterDialog({ initial, onCancel, onSave }: Props) {
  const isList = initial?.kind === "list";
  const [name, setName] = useState(initial?.name ?? "");
  const [group, setGroup] = useState<FilterRuleGroup>(initial?.rules ?? emptyGroup());
  const [count, setCount] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    ref.current?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  // Live match count, a moment after the rules stop changing.
  useEffect(() => {
    if (isList) return;
    const id = setTimeout(() => {
      backend
        .evaluateFilter({ name: name.trim() || "Preview", kind: "smart", rules: group })
        .then((names) => { setCount(names.length); setError(null); })
        .catch((e) => { setCount(null); setError(errorMessage(e)); });
    }, 250);
    return () => clearTimeout(id);
  }, [group, name, isList]);

  async function submit(e?: React.FormEvent) {
    e?.preventDefault();
    if (!name.trim()) return setError(t("Every filter needs a name."));
    setBusy(true);
    try {
      await onSave({
        name: name.trim(),
        kind: isList ? "list" : "smart",
        ...(isList ? { glyphs: initial?.glyphs ?? [] } : { rules: group }),
      });
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  return (
    <div className="scrim" role="presentation">
      <form className="dialog smart-filter-dialog" role="dialog" aria-modal="true" aria-labelledby="smart-filter-title" onSubmit={submit}>
        <header className="dialog-header">
          <h2 id="smart-filter-title" className="k-subtitle">{isList ? t("List Filter") : initial ? t("Edit Filter") : t("New Filter")}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onCancel}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          <label className="field">
            <span className="k-label">{t("Filter name")}</span>
            <input ref={ref} className="text-input" value={name} onChange={(e) => { setName(e.target.value); setError(null); }} />
          </label>
          {isList ? (
            <p className="k-body muted">{tn(initial?.glyphs?.length ?? 0, "{n} glyph in this list", "{n} glyphs in this list")}</p>
          ) : (
            <GroupEditor group={group} onChange={setGroup} depth={0} />
          )}
          {error && <p className="notice notice-danger k-label">{error}</p>}
        </div>
        <footer className="dialog-footer">
          {!isList && count !== null && <span className="k-caption muted">{tn(count, "Matches {n} glyph", "Matches {n} glyphs")}</span>}
          <span className="spacer" />
          <button type="button" className="btn btn-secondary" onClick={onCancel}>{t("Cancel")}</button>
          <button type="submit" className="btn btn-primary" disabled={busy || !name.trim()}>{t("Save")}</button>
        </footer>
      </form>
    </div>
  );
}

function GroupEditor({ group, onChange, onRemove, depth }: { group: FilterRuleGroup; onChange: (g: FilterRuleGroup) => void; onRemove?: () => void; depth: number }) {
  const patchRule = (i: number, r: FilterRule) => onChange({ ...group, rules: group.rules.map((old, j) => (j === i ? r : old)) });
  return (
    <div className="filter-group">
      <div className="row">
        <select className="select" value={group.all ? "all" : "any"} aria-label={t("Match")} onChange={(e) => onChange({ ...group, all: e.target.value === "all" })}>
          <option value="all">{t("All of the following")}</option>
          <option value="any">{t("Any of the following")}</option>
        </select>
        <span className="spacer" />
        {onRemove && <button type="button" className="btn btn-secondary btn-sm" onClick={onRemove}>{t("Remove Group")}</button>}
      </div>
      {group.rules.map((r, i) => (
        <RuleRow key={i} rule={r} onChange={(next) => patchRule(i, next)} onRemove={() => onChange({ ...group, rules: group.rules.filter((_, j) => j !== i) })} />
      ))}
      {group.groups.map((sub, i) => (
        <GroupEditor
          key={i}
          group={sub}
          depth={depth + 1}
          onChange={(next) => onChange({ ...group, groups: group.groups.map((old, j) => (j === i ? next : old)) })}
          onRemove={() => onChange({ ...group, groups: group.groups.filter((_, j) => j !== i) })}
        />
      ))}
      <div className="row">
        <button type="button" className="btn btn-secondary btn-sm" onClick={() => onChange({ ...group, rules: [...group.rules, defaultRule("category")] })}>{t("Add Rule")}</button>
        {depth < 3 && (
          <button type="button" className="btn btn-secondary btn-sm" onClick={() => onChange({ ...group, groups: [...group.groups, emptyGroup()] })}>{t("Add Group")}</button>
        )}
      </div>
    </div>
  );
}

function RuleRow({ rule, onChange, onRemove }: { rule: FilterRule; onChange: (r: FilterRule) => void; onRemove: () => void }) {
  const choices = CHOICES[rule.type];
  const bools = BOOL_LABELS[rule.type];
  return (
    <div className="filter-rule-row">
      <select className="select" value={rule.type} aria-label={t("Rule")} onChange={(e) => onChange(defaultRule(e.target.value as RuleType))}>
        {RULE_TYPES.map((rt) => <option key={rt} value={rt}>{t(RULE_LABELS[rt])}</option>)}
      </select>
      {choices ? (
        <select className="select" value={rule.value as string} aria-label={t("Value")} onChange={(e) => onChange({ ...rule, value: e.target.value } as FilterRule)}>
          {choices.map((v) => <option key={v} value={v}>{v === "" ? t("None") : t(v)}</option>)}
        </select>
      ) : bools ? (
        <select className="select" value={String(rule.value)} aria-label={t("Value")} onChange={(e) => onChange({ ...rule, value: e.target.value === "true" } as FilterRule)}>
          <option value="true">{t(bools[0])}</option>
          <option value="false">{t(bools[1])}</option>
        </select>
      ) : (
        <input className="text-input" value={rule.value as string} aria-label={t("Value")} onChange={(e) => onChange({ ...rule, value: e.target.value } as FilterRule)} />
      )}
      <button type="button" className="icon-btn" aria-label={t("Remove rule")} onClick={onRemove}><CloseIcon /></button>
    </div>
  );
}