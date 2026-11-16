import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { backend, errorMessage, isTauri, type ExportFormats, type ExportReport, type FontInfo } from "../lib/backend";
import { CloseIcon, FolderIcon } from "./Icons";
import { t, tn, translateMessage } from "../lib/i18n";

const DIR_KEY = "kernest.exportFolder";

function loadDir(): string {
  try {
    return localStorage.getItem(DIR_KEY) ?? "";
  } catch {
    return "";
  }
}

function saveDir(dir: string) {
  try {
    localStorage.setItem(DIR_KEY, dir);
  } catch {
    // The folder is a convenience only.
  }
}

const FORMATS_KEY = "kernest.exportFormats";
const DEFAULT_FORMATS: ExportFormats = { ttf: true, otf: false, woff2: false, removeOverlaps: true, applyMetricsKeys: true, autohint: true };

function loadFormats(): ExportFormats {
  try {
    return { ...DEFAULT_FORMATS, ...JSON.parse(localStorage.getItem(FORMATS_KEY) ?? "{}") };
  } catch {
    return DEFAULT_FORMATS;
  }
}

function saveFormats(f: ExportFormats) {
  try {
    localStorage.setItem(FORMATS_KEY, JSON.stringify(f));
  } catch {
    // A convenience only.
  }
}

const kb = (n: number) => (n < 1024 ? `${n} B` : `${(n / 1024).toFixed(n < 10240 ? 1 : 0)} KB`);
const fileName = (p: string) => p.split(/[\\/]/).pop() ?? p;

/** S14 · Export: variable and static fonts as TTF, OTF and WOFF2. */
export function ExportDialog({ fontPath, onClose }: { fontPath: string | null; onClose: () => void }) {
  const [info, setInfo] = useState<FontInfo | null>(null);
  const [variable, setVariable] = useState(true);
  const [statics, setStatics] = useState(true);
  const [formats, setFormats] = useState<ExportFormats>(loadFormats);
  const setFormat = (k: keyof ExportFormats, v: boolean) => setFormats((f) => ({ ...f, [k]: v }));
  const [dir, setDir] = useState(() => loadDir() || (fontPath ? fontPath.replace(/[\\/][^\\/]+$/, "") : ""));
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<ExportReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    backend.fontInfo().then(setInfo, (e) => setError(errorMessage(e)));
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !busy && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, busy]);

  const canVariable = !!info && info.masters.length > 1 && info.axes.length > 0;
  const variableNote = !info
    ? ""
    : !canVariable
      ? t("Needs at least two masters and an axis (Font Info).")
      : info.incompatible > 0
        ? tn(info.incompatible, "{n} glyph is incompatible and will block the variable font.", "{n} glyphs are incompatible and will block the variable font.")
        : `${tn(info.masters.length, "{n} master", "{n} masters")} · ${info.axes.map((a) => a.tag).join(", ")}`;
  const exportedInstances = info?.instances.filter((i) => i.export !== false) ?? [];
  const staticStyles = exportedInstances.length ? exportedInstances.map((i) => i.name) : (info?.masters.map((m) => m.name) ?? []);
  const wantVariable = variable && canVariable;
  const noFormat = !formats.ttf && !formats.otf && !formats.woff2;
  const nothing = (!wantVariable && !statics) || noFormat;

  async function browse() {
    const picked = await open({ directory: true, multiple: false, title: t("Export fonts to"), defaultPath: dir || undefined });
    if (typeof picked === "string") setDir(picked);
  }

  async function run() {
    if (!dir.trim()) return setError(t("Choose a folder to export to."));
    setBusy(true);
    setError(null);
    setReport(null);
    try {
      saveDir(dir.trim());
      saveFormats(formats);
      setReport(await backend.exportFont(wantVariable, statics, dir.trim(), formats));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="scrim" role="presentation">
      <div className="dialog export-dialog" role="dialog" aria-modal="true" aria-labelledby="export-title">
        <header className="dialog-header">
          <h2 id="export-title" className="k-subtitle">{t("Export {name}", { name: info?.familyName ?? "" })}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onClose} disabled={busy}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          <fieldset className="field" disabled={busy}>
            <legend className="k-overline muted">{t("Fonts")}</legend>
            <label className="check-row export-option">
              <input type="checkbox" checked={wantVariable} disabled={!canVariable} onChange={(e) => setVariable(e.target.checked)} />
              <span className="export-text">
                <span className="k-body-strong">{t("Variable font")}</span>
                <span className="k-caption muted">{variableNote}</span>
              </span>
            </label>
            <label className="check-row export-option">
              <input type="checkbox" checked={statics} onChange={(e) => setStatics(e.target.checked)} />
              <span className="export-text">
                <span className="k-body-strong">{t("Static fonts")}</span>
                <span className="k-caption muted">
                  {tn(staticStyles.length, "{n} file: {styles}", "{n} files: {styles}", { styles: staticStyles.join(", ") })}
                  {exportedInstances.length ? ` ${t("(instances)")}` : ` ${t("(one per master)")}`}
                </span>
              </span>
            </label>
          </fieldset>
          <fieldset className="field" disabled={busy}>
            <legend className="k-overline muted">{t("Formats")}</legend>
            <div className="export-formats">
              <label className="check-row"><input type="checkbox" checked={formats.ttf} onChange={(e) => setFormat("ttf", e.target.checked)} /> <span className="k-body">TTF</span> <span className="k-caption muted">{t("TrueType, for desktop apps")}</span></label>
              <label className="check-row"><input type="checkbox" checked={formats.otf} onChange={(e) => setFormat("otf", e.target.checked)} /> <span className="k-body">OTF</span> <span className="k-caption muted">{t("PostScript outlines (CFF, and CFF2 for variable)")}</span></label>
              <label className="check-row"><input type="checkbox" checked={formats.woff2} onChange={(e) => setFormat("woff2", e.target.checked)} /> <span className="k-body">WOFF2</span> <span className="k-caption muted">{t("compressed web fonts")}</span></label>
            </div>
          </fieldset>
          <fieldset className="field" disabled={busy}>
            <legend className="k-overline muted">{t("Before building")}</legend>
            <label className="check-row"><input type="checkbox" checked={formats.removeOverlaps} onChange={(e) => setFormat("removeOverlaps", e.target.checked)} /> <span className="k-body">{t("Remove overlaps")}</span> <span className="k-caption muted">{t("in static fonts (variable fonts keep them)")}</span></label>
            <label className="check-row"><input type="checkbox" checked={formats.applyMetricsKeys} onChange={(e) => setFormat("applyMetricsKeys", e.target.checked)} /> <span className="k-body">{t("Apply metrics keys")}</span> <span className="k-caption muted">{t("such as =n on sidebearings")}</span></label>
            <label className="check-row"><input type="checkbox" checked={formats.autohint} disabled={!formats.ttf && !formats.woff2} onChange={(e) => setFormat("autohint", e.target.checked)} /> <span className="k-body">{t("Autohint")}</span> <span className="k-caption muted">{t("sharper TTF and WOFF2 text on Windows (settings in Font Info)")}</span></label>
          </fieldset>
          <label className="field">
            <span className="k-label">{t("Destination folder")}</span>
            <span className="row">
              <input className="text-input dest-input" value={dir} onChange={(e) => setDir(e.target.value)} placeholder="C:\Users\you\Documents\Fonts" disabled={busy} spellCheck={false} />
              <button type="button" className="btn btn-secondary" onClick={browse} disabled={busy || !isTauri}><FolderIcon size={14} /> {t("Browse…")}</button>
            </span>
          </label>
          {busy && <p className="notice export-busy k-label" role="status"><span className="spinner" aria-hidden="true" /> {t("Compiling fonts… this can take a little while for large fonts.")}</p>}
          {error && <p className="notice notice-danger k-label">{error}</p>}
          {report && (
            <div className="export-report" role="status">
              {report.files.length > 0 && (
                <>
                  <p className="k-label">✓ {tn(report.files.length, "Exported {n} font", "Exported {n} fonts")}</p>
                  <ul className="export-files">
                    {report.files.map((f) => (
                      <li key={f.path} title={f.path}>
                        <span className={`pill ${f.kind === "variable" ? "pill-group" : "pill-static"}`}>{t(f.kind)}</span>
                        <span className="pill">{f.format.toUpperCase()}</span>
                        <span className="k-code">{fileName(f.path)}</span>
                        <span className="spacer" />
                        <span className="k-caption muted">{kb(f.bytes)}</span>
                      </li>
                    ))}
                  </ul>
                </>
              )}
              {report.problems.map((p) => <p key={p} className="notice notice-warning k-label">⚠ {translateMessage(p)}</p>)}
            </div>
          )}
        </div>
        <footer className="dialog-footer">
          <span className="k-caption muted">{t("Compiled with fontc · CFF and WOFF2 written by Kernest")}</span>
          <span className="spacer" />
          <button type="button" className="btn btn-secondary" onClick={onClose} disabled={busy}>{report ? t("Done") : t("Cancel")}</button>
          <button type="button" className="btn btn-primary" onClick={run} disabled={busy || nothing || !info}>{busy ? t("Exporting…") : t("Export")}</button>
        </footer>
      </div>
    </div>
  );
}
