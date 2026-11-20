import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { backend, errorMessage, isTauri, type DocStatus, type ExportDestination, type ExportFormats, type ExportPreset, type ExportReport, type FontInfo } from "../lib/backend";
import { openExternal } from "../lib/desktop";
import { CloseIcon, FolderIcon } from "./Icons";
import { QualityDialog } from "./QualityDialog";
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
const DEFAULT_FORMATS: ExportFormats = { ttf: true, otf: false, woff2: false, removeOverlaps: true, applyMetricsKeys: true, autohint: true, productionNames: false };

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
export function ExportDialog({ fontPath, version, onChanged, onClose, onExported, onFontInfo, onGlyph }: { fontPath: string | null; version: number; onChanged: (status: DocStatus) => void; onClose: () => void; onExported?: () => void; onFontInfo: () => void; onGlyph: (name: string) => void }) {
  const [info, setInfo] = useState<FontInfo | null>(null);
  const [variable, setVariable] = useState(true);
  const [statics, setStatics] = useState(true);
  const [formats, setFormats] = useState<ExportFormats>(loadFormats);
  const setFormat = (k: keyof ExportFormats, v: boolean) => setFormats((f) => ({ ...f, [k]: v }));
  const [dir, setDir] = useState(() => loadDir() || (fontPath ? fontPath.replace(/[\\/][^\\/]+$/, "") : ""));
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<ExportReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [selectedPreset, setSelectedPreset] = useState("");
  const [presetName, setPresetName] = useState("");
  const [allInstances, setAllInstances] = useState(true);
  const [selectedInstances, setSelectedInstances] = useState<string[]>([]);
  const [dirEdited, setDirEdited] = useState(true);
  const [checking, setChecking] = useState(false);
  const [destinations, setDestinations] = useState<ExportDestination[]>([]);
  const [installNote, setInstallNote] = useState<string | null>(null);
  useEffect(() => { backend.exportDestinations().then(setDestinations, () => setDestinations([])); }, []);

  useEffect(() => {
    let cancelled = false;
    backend.fontInfo().then(result => { if (!cancelled) setInfo(result); }, e => { if (!cancelled) setError(errorMessage(e)); });
    return () => { cancelled = true; };
  }, [version]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !busy && !checking && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, busy, checking]);

  function choosePreset(name: string) {
    setSelectedPreset(name); setPresetName(name); setError(null);
    const preset = info?.exportPresets.find(p => p.name === name);
    if (!preset) return;
    const { variable, statics, ...options } = preset.options;
    setVariable(variable); setStatics(statics);
    setFormats({ ...options, ttf: preset.formats.includes("ttf"), otf: preset.formats.includes("otf"), woff2: preset.formats.includes("woff2") });
    setSelectedInstances(preset.instances); setAllInstances(preset.instances.length === 0);
    const parent = fontPath?.replace(/[\\/][^\\/]+$/, "");
    const folder = preset.folder || "build";
    setDir(/^(?:[A-Za-z]:[\\/]|[\\/]{2}|\/)/.test(folder) ? folder : parent ? `${parent}/${folder}` : "");
    setDirEdited(false);
  }

  async function savePreset(remove = false) {
    if (!info) return;
    const name = presetName.trim();
    if (!remove && !name) return setError(t("Enter a preset name."));
    if (!remove && name !== selectedPreset && info.exportPresets.some(p => p.name === name)) return setError(t("Preset name \"{name}\" is used twice.", { name }));
    if (!remove && statics && !allInstances && selectedInstances.length === 0) return setError(t("Choose at least one instance."));
    setBusy(true); setError(null);
    try {
      const preset: ExportPreset = {
        name, formats: (["ttf", "otf", "woff2"] as const).filter(f => formats[f]), instances: allInstances ? [] : selectedInstances,
        options: { variable: wantVariable, statics, removeOverlaps: formats.removeOverlaps, applyMetricsKeys: formats.applyMetricsKeys, autohint: formats.autohint, productionNames: formats.productionNames },
        folder: !dirEdited ? info.exportPresets.find(p => p.name === selectedPreset)?.folder ?? dir.trim() : dir.trim(),
      };
      const list = info.exportPresets.filter(p => p.name !== selectedPreset);
      if (!remove) list.push(preset);
      const result = await backend.setExportPresets(list);
      setInfo(result); onChanged(result.status);
      setSelectedPreset(remove ? "" : name); setPresetName(remove ? "" : name);
    } catch (e) { setError(errorMessage(e)); }
    finally { setBusy(false); }
  }

  const canVariable = !!info && info.masters.length > 1 && info.axes.length > 0;
  const variableNote = !info
    ? ""
    : !canVariable
      ? t("Needs at least two masters and an axis (Font Info).")
      : info.incompatible > 0
        ? tn(info.incompatible, "{n} glyph is incompatible and will block the variable font.", "{n} glyphs are incompatible and will block the variable font.")
        : `${tn(info.masters.length, "{n} master", "{n} masters")} · ${info.axes.map((a) => a.tag).join(", ")}`;
  const exportedInstances = info?.instances.filter((i) => i.export !== false) ?? [];
  const staticStyles = !allInstances ? selectedInstances : exportedInstances.length ? exportedInstances.map((i) => i.name) : (info?.masters.map((m) => m.name) ?? []);
  const wantVariable = variable && canVariable;
  const noFormat = !formats.ttf && !formats.otf && !formats.woff2;
  const nothing = (!wantVariable && !statics) || noFormat || (statics && !allInstances && selectedInstances.length === 0);
  // Every style in every chosen format, for the button ("Export 4 fonts").
  const fileCount = ((statics ? staticStyles.length : 0) + (wantVariable ? 1 : 0)) * (["ttf", "otf", "woff2"] as const).filter((f) => formats[f]).length;

  async function browse() {
    const picked = await open({ directory: true, multiple: false, title: t("Export fonts to"), defaultPath: dir || undefined });
    if (typeof picked === "string") { setDir(picked); setDirEdited(true); }
  }

  async function run() {
    if (!dir.trim()) return setError(t("Choose a folder to export to."));
    setBusy(true);
    setError(null);
    setReport(null);
    try {
      saveDir(dir.trim());
      saveFormats(formats);
      const result = await backend.exportFont(wantVariable, statics, dir.trim(), formats, allInstances ? [] : selectedInstances);
      setReport(result);
      if (result.files.length) onExported?.();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  /** Test Install, Install for Me or the web preview: their own builds, not the Export settings. */
  async function trial(kind: "test" | "user" | "web") {
    setBusy(true);
    setError(null);
    setReport(null);
    setInstallNote(null);
    try {
      const instances = allInstances ? [] : selectedInstances;
      if (kind === "web") {
        const url = await backend.webPreview();
        await openExternal(url);
        setInstallNote(t("The web preview is open at {url}. Use Web Preview again after changes; the page reloads the fonts by itself.", { url }));
      } else {
        const r = kind === "test" ? await backend.testInstall(instances) : await backend.installForUser(instances);
        setReport(r.report);
        setInstallNote(kind === "test"
          ? t("Loaded for this session: {styles}. Other apps see them after they refresh their font list; they go away when Kernest quits.", { styles: r.installed.join(", ") })
          : t("Installed for you: {styles}. Restart apps that were already open to see them.", { styles: r.installed.join(", ") }));
      }
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  const destLabel = (d: ExportDestination) => d.id === "user-fonts" ? t("Your fonts folder (Install for Me also registers them with Windows)") : d.id === "adobe" ? t("Adobe apps (needs admin rights)") : t("Web preview folder");

  if (checking && report && info) return <QualityDialog files={report.files} familyName={info.familyName} glyphNames={info.glyphOrder} onClose={() => setChecking(false)} onFontInfo={onFontInfo} onGlyph={onGlyph} />;

  return (
    <div className="scrim" role="presentation">
      <div className="dialog export-dialog" role="dialog" aria-modal="true" aria-labelledby="export-title">
        <header className="dialog-header">
          <h2 id="export-title" className="k-subtitle">{t("Export {name}", { name: info?.familyName ?? "" })}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onClose} disabled={busy}><CloseIcon /></button>
        </header>
        <div className="export-layout">
        <aside className="export-presets" aria-label={t("Export presets")}>
          <h3 className="k-subtitle">{t("Presets")}</h3>
          <label className="field"><span className="k-caption muted">{t("Saved preset")}</span><select value={selectedPreset} disabled={busy} aria-label={t("Saved preset")} onChange={e => choosePreset(e.target.value)}><option value="">{t("Custom settings")}</option>{info?.exportPresets.map(p => <option key={p.name} value={p.name}>{p.name}</option>)}</select></label>
          <div className="preset-list">{info?.exportPresets.map(p => <button key={p.name} type="button" disabled={busy} className={selectedPreset === p.name ? "is-selected" : ""} onClick={() => choosePreset(p.name)}><strong>{p.name}</strong><small>{p.formats.map(f => f.toUpperCase()).join(" + ")}{p.options.variable ? ` · ${t("Variable font")}` : ""}</small></button>)}</div>
          <label className="field"><span className="k-caption muted">{t("Preset name")}</span><input value={presetName} maxLength={64} disabled={busy} aria-label={t("Preset name")} onChange={e => setPresetName(e.target.value)} /></label>
          <button type="button" className="btn btn-secondary btn-sm" disabled={busy || !info || nothing} onClick={() => savePreset()}>{t("Save preset")}</button>
          <button type="button" className="btn btn-link" disabled={busy || !selectedPreset} onClick={() => savePreset(true)}>{t("Remove preset")}</button>
          <p className="k-caption muted">{t("Presets are saved in this font and shared with the command line. Save preset keeps your current choices.")}</p>
        </aside>
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
          {statics && !!info?.instances.length && <fieldset className="field export-instance-picks" disabled={busy}><legend className="k-label">{t("Instances")}</legend><label className="check-row"><input type="checkbox" checked={allInstances} onChange={e => { setAllInstances(e.target.checked); if (!e.target.checked && !selectedInstances.length) setSelectedInstances(exportedInstances.map(i => i.name)); }} />{t("All exported instances")}</label>{!allInstances && info.instances.map(i => <label key={i.name} className="check-row"><input type="checkbox" checked={selectedInstances.includes(i.name)} aria-label={t("Export instance {name}", { name: i.name })} onChange={() => setSelectedInstances(names => names.includes(i.name) ? names.filter(n => n !== i.name) : [...names, i.name])} />{i.name}</label>)}</fieldset>}
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
            <label className="check-row"><input type="checkbox" checked={formats.productionNames} onChange={(e) => setFormat("productionNames", e.target.checked)} /> <span className="k-body">{t("Use production names")}</span> <span className="k-caption muted">{t("the glyph names written into the font file")}</span></label>
          </fieldset>
          <label className="field">
            <span className="k-label">{t("Destination folder")}</span>
            <span className="row">
              <input className="text-input dest-input" value={dir} onChange={(e) => { setDir(e.target.value); setDirEdited(true); }} placeholder="C:\Users\you\Documents\Fonts" disabled={busy} spellCheck={false} />
              <button type="button" className="btn btn-secondary" onClick={browse} disabled={busy || !isTauri}><FolderIcon size={14} /> {t("Browse…")}</button>
            </span>
          </label>
          {destinations.length > 0 && (
            <div className="field">
              <span className="k-overline muted">{t("Quick destinations")}</span>
              <div className="dest-list">
                {destinations.map((d) => (
                  <button key={d.id} type="button" className={`dest-item${dir.trim() === d.path ? " is-selected" : ""}`} disabled={busy}
                    onClick={() => { setDir(d.path); setDirEdited(true); }}>
                    <span className="k-code">{d.path}</span>
                    <small className="k-caption muted">{destLabel(d)}{d.exists ? "" : ` · ${t("created on export")}`}</small>
                  </button>
                ))}
              </div>
            </div>
          )}
          <details className="field cache-tips">
            <summary className="k-caption">{t("Trying fonts in other apps")}</summary>
            <ol className="k-caption">
              <li>{t("Test Install loads the fonts for this session only, so apps never keep an old copy.")}</li>
              <li>{t("Install for Me puts them in your own fonts folder; restart the app you test in.")}</li>
              <li>{t("Adobe apps also read fonts from the Adobe Fonts folder (needs admin rights).")}</li>
              <li>{t("Browsers: press Ctrl+F5 to reload without the cache.")}</li>
            </ol>
          </details>
          {installNote && <p className="notice k-label" role="status">{installNote}</p>}
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
              {(report.warnings ?? []).map((p) => <p key={p} className="notice notice-warning k-label">{translateMessage(p)}</p>)}
            </div>
          )}
        </div>
        </div>
        <footer className="dialog-footer">
          <span className="k-caption muted">{t("Compiled with fontc · CFF and WOFF2 written by Kernest")}</span>
          <span className="spacer" />
          <button type="button" className="btn btn-secondary" disabled={busy || !info || !isTauri} title={t("Builds static TTFs and loads them until Kernest quits")} onClick={() => trial("test")}>{t("Test Install")}</button>
          <button type="button" className="btn btn-secondary" disabled={busy || !info || !isTauri} title={t("Installs static TTFs for your Windows account")} onClick={() => trial("user")}>{t("Install for Me")}</button>
          <button type="button" className="btn btn-secondary" disabled={busy || !info || !isTauri} title={t("Opens the fonts as web fonts in your browser")} onClick={() => trial("web")}>{t("Web Preview")}</button>
          <button type="button" className="btn btn-secondary" disabled={busy || !report?.files.length} onClick={() => setChecking(true)}>{t("Check font")}</button>
          <button type="button" className="btn btn-secondary" onClick={onClose} disabled={busy}>{report ? t("Done") : t("Cancel")}</button>
          <button type="button" className="btn btn-primary" onClick={run} disabled={busy || nothing || !info}>{busy ? t("Exporting…") : nothing ? t("Export") : tn(fileCount, "Export {n} font", "Export {n} fonts")}</button>
        </footer>
      </div>
    </div>
  );
}
