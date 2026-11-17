import { useEffect, useRef, useState } from "react";
import type { OutputLine } from "../lib/backend";
import { CloseIcon } from "./Icons";
import { t } from "../lib/i18n";

interface Props {
  /** "macro" shows the code box; "script" runs a file and only shows the console. */
  mode: "macro" | "script";
  name?: string;
  output: OutputLine[];
  running: boolean;
  error: string | null;
  canRun: boolean;
  onRun: (code: string) => void;
  onCancelRun: () => void;
  onClose: () => void;
}

const MACRO_KEY = "kernest.macro";
const loadCode = () => {
  try { return localStorage.getItem(MACRO_KEY) ?? ""; } catch { return ""; }
};

/**
 * S45 · The macro panel and script console. A macro runs against the open font
 * with Ctrl+Enter; a menu script runs on open. Output streams in live and the
 * run can be cancelled. One run is one undo step.
 */
export function ScriptDialog({ mode, name, output, running, error, canRun, onRun, onCancelRun, onClose }: Props) {
  const [code, setCode] = useState(loadCode);
  const consoleRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      if (mode === "macro" && e.ctrlKey && e.key === "Enter") {
        e.preventDefault();
        if (canRun && !running && code.trim()) run();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  useEffect(() => {
    const el = consoleRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [output, error, running]);

  function run() {
    try { localStorage.setItem(MACRO_KEY, code); } catch { /* the macro still runs */ }
    onRun(code);
  }

  const title = mode === "macro" ? t("Macro Panel") : name ?? t("Script");

  return (
    <div className="scrim" role="presentation">
      <div className="dialog dialog-md script-dialog" role="dialog" aria-modal="true" aria-labelledby="script-title">
        <header className="dialog-header">
          <h2 id="script-title" className="k-subtitle">{title}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onClose}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          {mode === "macro" && (
            <label className="field">
              <span className="k-label">{t("Python code")}</span>
              <textarea
                className="text-input macro-code"
                value={code}
                spellCheck={false}
                aria-label={t("Python code")}
                placeholder={t("font.glyphs['A'].layers[font.master_id].width = 600")}
                onChange={(e) => setCode(e.target.value)}
              />
            </label>
          )}
          <div className="script-console-wrap">
            <span className="k-label">{t("Console")}</span>
            <div className="script-console" ref={consoleRef} role="log" aria-live="polite" aria-label={t("Console")}>
              {output.length === 0 && !error && !running && <span className="muted k-caption">{t("No output yet.")}</span>}
              {output.map((line, i) => (
                <div key={i} className={`console-line${line.stream === "stderr" ? " is-stderr" : ""}`}>{line.text}</div>
              ))}
              {running && <div className="console-line is-status">{t("Running…")}</div>}
            </div>
          </div>
          {error && <p className="notice notice-danger k-label script-error">{error}</p>}
          {!canRun && <p className="notice notice-warning k-label">{t("Open a font to run scripts.")}</p>}
        </div>
        <footer className="dialog-footer">
          {mode === "macro" && <span className="k-caption muted">{t("Ctrl+Enter runs")}</span>}
          <span className="spacer" />
          {running ? (
            <button type="button" className="btn btn-secondary" onClick={onCancelRun}>{t("Stop")}</button>
          ) : (
            mode === "macro" && (
              <button type="button" className="btn btn-primary" disabled={!canRun || !code.trim()} onClick={run}>{t("Run")}</button>
            )
          )}
          <button type="button" className="btn btn-secondary" onClick={onClose}>{t("Close")}</button>
        </footer>
      </div>
    </div>
  );
}