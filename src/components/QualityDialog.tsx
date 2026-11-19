import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { backend, errorMessage, type CheckSeverity, type ExportedFile, type QualityCheck, type QualityReport } from "../lib/backend";
import { pickQualityReportPath } from "../lib/desktop";
import { t, tn } from "../lib/i18n";
import { CloseIcon } from "./Icons";

const CATEGORIES = [["all", "All"], ["names", "Names & metadata"], ["metrics", "Vertical metrics"], ["outlines", "Outlines"], ["features", "OpenType features"], ["hinting", "Hinting"], ["variable", "Variable font"]] as const;
const SEVERITY_ORDER: Record<CheckSeverity, number> = { ERROR: 0, FAIL: 1, WARN: 2, INFO: 3, PASS: 4, SKIP: 5 };
function category(check: QualityCheck): string {
  const id = check.id.toLowerCase();
  if (/fvar|avar|stat|varfont|axis|axes|interpolation/.test(id)) return "variable";
  if (/hint|gasp|dropout|ppem/.test(id)) return "hinting";
  if (/name|version|copyright|vendor|metadata/.test(id)) return "names";
  if (/metrics|ascent|descent|ascender|descender|linegap|width|hmtx|caret/.test(id)) return "metrics";
  if (/gdef|gpos|gsub|layout|feature|kern|ligature/.test(id)) return "features";
  return "outlines";
}
function explanation(check: QualityCheck) {
  if (check.status === "PASS") return t("No problem was found by this check.");
  if (check.status === "SKIP") return t("This check was skipped because it does not apply or requires network access.");
  if (check.status === "ERROR") return t("This check could not finish. Its result is unknown.");
  if (check.status === "INFO") return t("This check provides information for your review.");
  switch (category(check)) {
    case "names": return t("Review the font names, version and metadata in Font Info.");
    case "metrics": return t("Review the master metrics and glyph widths. Inconsistent values can clip or misalign text.");
    case "features": return t("Review features, anchors and kerning. Check that referenced glyphs exist.");
    case "hinting": return t("Review the hinting settings and inspect the pixel preview at small sizes.");
    case "variable": return t("Review axis ranges, master compatibility and named instances in Font Info.");
    default: return t("Inspect the affected glyphs and exported tables. Empty or malformed outlines can prevent correct display.");
  }
}
const fileName = (path: string) => path.split(/[\\/]/).at(-1) ?? path;
interface Props { files: ExportedFile[]; familyName: string; glyphNames: string[]; onClose: () => void; onFontInfo: () => void; onGlyph: (name: string) => void }

export function QualityDialog({ files, familyName, glyphNames, onClose, onFontInfo, onGlyph }: Props) {
  const [profile, setProfile] = useState<"universal" | "opentype">("universal");
  const [target, setTarget] = useState("");
  const [group, setGroup] = useState("all");
  const [status, setStatus] = useState<CheckSeverity | "all">("all");
  const [report, setReport] = useState<QualityReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const sequence = useRef(0);
  const dialog = useRef<HTMLDivElement>(null);
  const run = useCallback(async () => {
    const request = ++sequence.current;
    setBusy(true); setError(null); setReport(null); setSaved(null);
    try {
      const result = await backend.checkFonts(files.filter(f => !target || f.path === target).map(f => f.path), profile);
      if (request === sequence.current) setReport(result);
    } catch (e) { if (request === sequence.current) setError(errorMessage(e)); }
    finally { if (request === sequence.current) setBusy(false); }
  }, [files, target, profile]);
  useEffect(() => { run(); return () => { sequence.current++; }; }, [run]);
  useEffect(() => { dialog.current?.focus(); }, []);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.stopImmediatePropagation(); if (!busy) onClose(); }
      if (e.key === "Tab") {
        const elements = [...(dialog.current?.querySelectorAll<HTMLElement>('button:not(:disabled), select:not(:disabled), summary') ?? [])].filter(el => el.getClientRects().length);
        if (e.shiftKey && document.activeElement === elements[0]) { e.preventDefault(); elements.at(-1)?.focus(); }
        else if (!e.shiftKey && document.activeElement === elements.at(-1)) { e.preventDefault(); elements[0]?.focus(); }
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [busy, onClose]);
  const visible = useMemo(() => (report?.checks ?? []).filter(c => (group === "all" || category(c) === group) && (status === "all" || c.status === status)).sort((a, b) => SEVERITY_ORDER[a.status] - SEVERITY_ORDER[b.status]), [report, group, status]);
  const save = async () => {
    if (!report) return;
    try { const path = await pickQualityReportPath(familyName); if (path) setSaved(await backend.saveQualityReport(path, report)); }
    catch (e) { setError(errorMessage(e)); }
  };
  return <div className="scrim quality-scrim" role="presentation"><div ref={dialog} tabIndex={-1} className="dialog quality-dialog" role="dialog" aria-modal="true" aria-labelledby="quality-title">
    <header className="dialog-header"><h2 id="quality-title" className="k-subtitle">{t("Check Font — {name}", { name: familyName })}</h2><button type="button" className="icon-btn" aria-label={t("Close")} onClick={onClose} disabled={busy}><CloseIcon /></button></header>
    <div className="quality-toolbar">
      <select aria-label={t("Check profile")} value={profile} disabled={busy} onChange={e => setProfile(e.target.value as typeof profile)}><option value="universal">{t("OpenType and general quality")}</option><option value="opentype">{t("OpenType specification")}</option></select>
      <select aria-label={t("Check target")} value={target} disabled={busy} onChange={e => setTarget(e.target.value)}><option value="">{tn(files.length, "All {n} exported font", "All {n} exported fonts")}</option>{files.map(f => <option key={f.path} value={f.path}>{fileName(f.path)}</option>)}</select>
      <span className="spacer" />
      {report && (["FAIL", "WARN", "PASS", "SKIP", "ERROR"] as const).map(level => <button key={level} type="button" className={`quality-badge quality-${level.toLowerCase()}`} aria-pressed={status === level} onClick={() => setStatus(s => s === level ? "all" : level)}>{t(level)} {report[{ FAIL: "failed", WARN: "warnings", PASS: "passed", SKIP: "skipped", ERROR: "errors" }[level] as "failed"]}</button>)}
      <button type="button" className="btn btn-secondary btn-sm" disabled={!report || busy} onClick={save}>{t("Save report…")}</button><button type="button" className="btn btn-primary btn-sm" disabled={busy} onClick={run}>{busy ? t("Checking…") : t("Run checks")}</button>
    </div>
    <div className="quality-layout">
      <nav className="quality-sidebar" aria-label={t("Check categories")}>{CATEGORIES.map(([id, label]) => <button key={id} type="button" className={group === id ? "is-selected" : ""} aria-pressed={group === id} onClick={() => { setGroup(id); setStatus("all"); }}><span>{t(label)}</span><small>{report?.checks.filter(c => id === "all" || category(c) === id).length ?? "—"}</small></button>)}</nav>
      <main className="quality-main">
        <p className="notice k-caption">{t("Checks run locally with Fontspector. No font data is sent anywhere.")}</p>
        {busy && <p role="status" className="notice"><span className="spinner" aria-hidden="true" /> {t("Checking exported fonts…")}</p>}
        {error && <p role="alert" className="notice notice-danger">{error}</p>}
        {saved && <p role="status" className="notice">{t("Saved to {path}", { path: saved })}</p>}
        {report && <div className="quality-checks" aria-live="polite">{visible.map((check, i) => {
          const glyph = (check.glyphs ?? []).find(g => glyphNames.includes(g));
          return <article key={`${check.id}-${check.file}-${i}`} className={`quality-row quality-row-${check.status.toLowerCase()}`}>
            <span className={`quality-badge quality-${check.status.toLowerCase()}`}>{t(check.status)}</span>
            <div className="quality-description"><h3 className="k-label">{t(check.title)}</h3><p className="k-caption muted">{explanation(check)}</p>{check.file && <p className="k-caption k-code">{fileName(check.file)}</p>}
              {check.messages.length > 0 && <details><summary className="k-caption">{t("Technical details (original checker output)")}</summary><pre>{check.messages.map(m => `${m.code ? `[${m.code}] ` : ""}${m.message}`).join("\n")}</pre></details>}
            </div>
            {(check.status === "FAIL" || check.status === "WARN") && (glyph ? <button type="button" className="btn btn-secondary btn-sm" onClick={() => onGlyph(glyph)}>{t("Show glyph")}</button> : <button type="button" className="btn btn-secondary btn-sm" onClick={onFontInfo}>{t("Open Font Info")}</button>)}
          </article>;
        })}{visible.length === 0 && <p className="muted">{t("No checks match these filters.")}</p>}</div>}
      </main>
    </div>
  </div></div>;
}
