import { useEffect, useMemo, useRef, useState } from "react";
import { errorMessage } from "../lib/backend";
import { CloseIcon } from "./Icons";
import { t, tn } from "../lib/i18n";

interface Props {
  /** Every glyph name in the font, in glyph order. */
  allNames: string[];
  /** The current Font View selection. */
  selection: string[];
  onCancel: () => void;
  onRename: (renames: Record<string, string>) => Promise<void>;
}

type Mode = "replace" | "add" | "remove";

const validName = (n: string) =>
  n === ".notdef" || n === ".null" || (/^[A-Za-z_][A-Za-z0-9_.-]*$/.test(n) && n.length <= 63);

/** Glyph › Rename Glyphs… — find/replace (plain or regex) or add/remove a suffix,
 * over the selection or the whole font. The pattern runs here; the backend
 * receives the old→new map and updates every reference. */
export function RenameDialog({ allNames, selection, onCancel, onRename }: Props) {
  const [mode, setMode] = useState<Mode>("replace");
  const [scope, setScope] = useState<"selection" | "all">(selection.length ? "selection" : "all");
  const [find, setFind] = useState("");
  const [replace, setReplace] = useState("");
  const [regex, setRegex] = useState(false);
  const [suffix, setSuffix] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    ref.current?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  const targets = scope === "selection" ? selection : allNames;

  const { renames, badRegex, invalid, clash } = useMemo(() => {
    const map: Record<string, string> = {};
    let re: RegExp | null = null;
    let bad = false;
    if (mode === "replace" && regex && find) {
      try {
        re = new RegExp(find, "g");
      } catch {
        bad = true;
      }
    }
    if (!bad) {
      for (const name of targets) {
        let next = name;
        if (mode === "replace") {
          if (!find) continue;
          next = re ? name.replace(re, replace) : name.split(find).join(replace);
        } else if (mode === "add") {
          if (!suffix) continue;
          next = name + suffix;
        } else {
          if (!suffix || !name.endsWith(suffix)) continue;
          next = name.slice(0, name.length - suffix.length);
        }
        if (next !== name) map[name] = next;
      }
    }
    const invalid = Object.values(map).some((n) => !validName(n));
    const seen = new Set<string>();
    let clash = false;
    for (const n of allNames.map((n) => map[n] ?? n)) {
      if (seen.has(n)) {
        clash = true;
        break;
      }
      seen.add(n);
    }
    return { renames: map, badRegex: bad, invalid, clash };
  }, [mode, targets, find, replace, regex, suffix, allNames]);

  const entries = Object.entries(renames);
  const problem = badRegex || invalid || clash;
  const canApply = entries.length > 0 && !problem && !busy;

  async function submit(e?: React.FormEvent) {
    e?.preventDefault();
    if (!canApply) return;
    setBusy(true);
    try {
      await onRename(renames);
      onCancel();
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  const modeLabel: Record<Mode, string> = {
    replace: t("Find and replace"),
    add: t("Add suffix"),
    remove: t("Remove suffix"),
  };

  return (
    <div className="scrim" role="presentation">
      <form className="dialog rename-dialog" role="dialog" aria-modal="true" aria-labelledby="rename-title" onSubmit={submit}>
        <header className="dialog-header">
          <h2 id="rename-title" className="k-subtitle">{t("Rename Glyphs")}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onCancel}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          <div className="segmented" role="radiogroup" aria-label={t("Rename method")}>
            {(["replace", "add", "remove"] as const).map((k) => (
              <button key={k} type="button" role="radio" aria-checked={mode === k} className={`segment${mode === k ? " is-selected" : ""}`} onClick={() => { setMode(k); setError(null); }}>
                {modeLabel[k]}
              </button>
            ))}
          </div>
          {selection.length > 0 && (
            <div className="segmented" role="radiogroup" aria-label={t("Rename scope")}>
              <button type="button" role="radio" aria-checked={scope === "selection"} className={`segment${scope === "selection" ? " is-selected" : ""}`} onClick={() => setScope("selection")}>
                {tn(selection.length, "Selected ({n})", "Selected ({n})")}
              </button>
              <button type="button" role="radio" aria-checked={scope === "all"} className={`segment${scope === "all" ? " is-selected" : ""}`} onClick={() => setScope("all")}>
                {t("All glyphs")}
              </button>
            </div>
          )}
          {mode === "replace" ? (
            <>
              <label className="field">
                <span className="k-label">{t("Find")}</span>
                <input ref={ref} className="text-input" type="text" value={find} spellCheck={false} onChange={(e) => { setFind(e.target.value); setError(null); }} />
              </label>
              <label className="field">
                <span className="k-label">{t("Replace with")}</span>
                <input className="text-input" type="text" value={replace} spellCheck={false} onChange={(e) => { setReplace(e.target.value); setError(null); }} />
              </label>
              <label className="check-row">
                <input type="checkbox" checked={regex} onChange={(e) => { setRegex(e.target.checked); setError(null); }} />
                <span className="k-body">{t("Regular expression")}</span>
                <span className="k-caption muted">{t("capture groups work in the replacement ($1, $2)")}</span>
              </label>
            </>
          ) : (
            <label className="field">
              <span className="k-label">{mode === "add" ? t("Suffix to add") : t("Suffix to remove")}</span>
              <input ref={ref} className="text-input" type="text" value={suffix} spellCheck={false} onChange={(e) => { setSuffix(e.target.value); setError(null); }} />
            </label>
          )}
          <div className="rename-preview" dir="ltr">
            {entries.length === 0 ? (
              <p className="k-caption muted">{t("No glyphs change.")}</p>
            ) : (
              <>
                {entries.slice(0, 100).map(([old, next]) => (
                  <div key={old} className={`rename-row${validName(next) ? "" : " is-invalid"}`}>
                    <span className="rename-old">{old}</span>
                    <span className="rename-arrow">→</span>
                    <span className="rename-new">{next}</span>
                  </div>
                ))}
                {entries.length > 100 && <p className="k-caption muted">{tn(entries.length - 100, "+{n} more", "+{n} more")}</p>}
              </>
            )}
          </div>
          {badRegex && <p className="notice notice-danger k-label">{t("That regular expression isn’t valid.")}</p>}
          {!badRegex && (invalid || clash) && <p className="notice notice-danger k-label">{t("Some new names aren’t valid, or two glyphs would end up with the same name.")}</p>}
          {error && <p className="notice notice-danger k-label">{error}</p>}
        </div>
        <footer className="dialog-footer">
          <span className="k-caption muted">
            {entries.length ? tn(entries.length, "Renames {n} glyph", "Renames {n} glyphs") : t("References in components, kerning, metrics keys and features follow.")}
          </span>
          <span className="spacer" />
          <button type="button" className="btn btn-secondary" onClick={onCancel}>{t("Cancel")}</button>
          <button type="submit" className="btn btn-primary" disabled={!canApply}>{t("Rename")}</button>
        </footer>
      </form>
    </div>
  );
}