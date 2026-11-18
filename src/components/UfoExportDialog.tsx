import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { backend, errorMessage, isTauri, type FontInfo, type UfoExportReport } from "../lib/backend";
import { CloseIcon, FolderIcon } from "./Icons";
import { t, tn, translateMessage } from "../lib/i18n";

const DIR_KEY = "kernest.ufoExportFolder";

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

const fileName = (p: string) => p.split(/[\\/]/).pop() ?? p;

/** S21 · Export UFO: editable sources and a designspace, with master and layer choices. */
export function UfoExportDialog({ fontPath, onClose }: { fontPath: string | null; onClose: () => void }) {
  const [info, setInfo] = useState<FontInfo | null>(null);
  const [picked, setPicked] = useState<string[]>([]);
  const [decomposeSmart, setDecomposeSmart] = useState(false);
  const [colorLayers, setColorLayers] = useState(false);
  const [dir, setDir] = useState(() => loadDir() || (fontPath ? fontPath.replace(/[\\/][^\\/]+$/, "") : ""));
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<UfoExportReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    backend.fontInfo().then((i) => {
      setInfo(i);
      setPicked(i.masters.map((m) => m.id));
    }, (e) => setError(errorMessage(e)));
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && !busy && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, busy]);

  const toggleMaster = (id: string) =>
    setPicked((p) => (p.includes(id) ? p.filter((x) => x !== id) : [...p, id]));

  async function browse() {
    const folder = await open({ directory: true, multiple: false, title: t("Export UFO sources to"), defaultPath: dir || undefined });
    if (typeof folder === "string") setDir(folder);
  }

  async function run() {
    if (!dir.trim()) return setError(t("Choose a folder to export to."));
    setBusy(true);
    setError(null);
    setReport(null);
    try {
      saveDir(dir.trim());
      setReport(await backend.exportUfo(dir.trim(), { masters: picked, decomposeSmart, colorLayers }));
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  const nothing = picked.length === 0;

  return (
    <div className="scrim" role="presentation">
      <div className="dialog export-dialog" role="dialog" aria-modal="true" aria-labelledby="ufo-export-title">
        <header className="dialog-header">
          <h2 id="ufo-export-title" className="k-subtitle">{t("Export UFO · {name}", { name: info?.familyName ?? "" })}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onClose} disabled={busy}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          <fieldset className="field" disabled={busy}>
            <legend className="k-overline muted">{t("Masters")}</legend>
            <span className="k-caption muted">{t("Each chosen master is written as a UFO source.")}</span>
            {(info?.masters ?? []).map((m) => (
              <label key={m.id} className="check-row export-option">
                <input type="checkbox" checked={picked.includes(m.id)} onChange={() => toggleMaster(m.id)} />
                <span className="export-text">
                  <span className="k-body-strong">{m.name}</span>
                  {m.id === info?.defaultMasterId && <span className="k-caption muted">{t("default")}</span>}
                </span>
              </label>
            ))}
          </fieldset>
          <fieldset className="field" disabled={busy}>
            <legend className="k-overline muted">{t("Options")}</legend>
            <label className="check-row export-option">
              <input type="checkbox" checked={decomposeSmart} onChange={(e) => setDecomposeSmart(e.target.checked)} />
              <span className="export-text">
                <span className="k-body-strong">{t("Decompose smart components")}</span>
                <span className="k-caption muted">{t("Flatten smart glyphs to outlines; otherwise they stay plain components.")}</span>
              </span>
            </label>
            <label className="check-row export-option">
              <input type="checkbox" checked={colorLayers} onChange={(e) => setColorLayers(e.target.checked)} />
              <span className="export-text">
                <span className="k-body-strong">{t("Colour layers")}</span>
                <span className="k-caption muted">{t("Write colour layers as named UFO layers.")}</span>
              </span>
            </label>
          </fieldset>
          <label className="field">
            <span className="k-label">{t("Destination folder")}</span>
            <span className="row">
              <input className="text-input dest-input" value={dir} onChange={(e) => setDir(e.target.value)} placeholder="C:\Users\you\Documents\Fonts" disabled={busy} spellCheck={false} />
              <button type="button" className="btn btn-secondary" onClick={browse} disabled={busy || !isTauri}><FolderIcon size={14} /> {t("Browse…")}</button>
            </span>
          </label>
          {busy && <p className="notice export-busy k-label" role="status"><span className="spinner" aria-hidden="true" /> {t("Writing UFO sources…")}</p>}
          {error && <p className="notice notice-danger k-label">{error}</p>}
          {report && (
            <div className="export-report" role="status">
              {report.files.length > 0 && (
                <>
                  <p className="k-label">✓ {tn(report.files.length, "Exported {n} file", "Exported {n} files")}</p>
                  <ul className="export-files">
                    {report.files.map((f) => (
                      <li key={f} title={f}>
                        <span className="pill">{f.endsWith(".designspace") ? t("designspace") : "UFO"}</span>
                        <span className="k-code">{fileName(f)}</span>
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
          <span className="k-caption muted">{t("UFO sources and a designspace, for editing in other font tools.")}</span>
          <span className="spacer" />
          <button type="button" className="btn btn-secondary" onClick={onClose} disabled={busy}>{report ? t("Done") : t("Cancel")}</button>
          <button type="button" className="btn btn-primary" onClick={run} disabled={busy || nothing || !info}>{busy ? t("Exporting…") : t("Export")}</button>
        </footer>
      </div>
    </div>
  );
}