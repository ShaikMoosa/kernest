import { useEffect, useState } from "react";
import { errorMessage, type PluginInfo } from "../lib/backend";
import { CloseIcon } from "./Icons";
import { t, tn } from "../lib/i18n";

interface Props {
  plugin: PluginInfo;
  /** Glyphs selected in the Font View; empty when nothing is selected. */
  selection: string[];
  onCancel: () => void;
  onApply: (plugin: PluginInfo, params: Record<string, number | string | boolean>, glyphs: string[]) => Promise<void>;
}

type Values = Record<string, string | boolean>;

const initialValues = (plugin: PluginInfo): Values => {
  const v: Values = {};
  for (const p of plugin.parameters) {
    v[p.name] = p.type === "boolean" ? Boolean(p.default) : String(p.default ?? "");
  }
  return v;
};

/**
 * S17 · A filter plug-in's settings, described by its plugin.json. Apply runs it
 * over the chosen glyphs as one undoable step.
 */
export function PluginFilterDialog({ plugin, selection, onCancel, onApply }: Props) {
  const [v, setV] = useState<Values>(() => initialValues(plugin));
  const [useSelection, setUseSelection] = useState(selection.length > 0);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  function params(): Record<string, number | string | boolean> | null {
    const out: Record<string, number | string | boolean> = {};
    for (const p of plugin.parameters) {
      const raw = v[p.name];
      if (p.type === "boolean") {
        out[p.name] = Boolean(raw);
      } else if (p.type === "number") {
        const s = String(raw ?? "").trim();
        const n = Number(s);
        if (s === "" || !Number.isFinite(n)) return null;
        if (p.min !== undefined && n < p.min) return null;
        if (p.max !== undefined && n > p.max) return null;
        out[p.name] = n;
      } else {
        out[p.name] = String(raw ?? "");
      }
    }
    return out;
  }

  const p = params();

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!p) return setError(t("Enter valid numbers in every field."));
    setError(null);
    setBusy(true);
    try {
      await onApply(plugin, p, useSelection ? selection : []);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  }

  return (
    <div className="scrim" role="presentation">
      <form className="dialog dialog-sm filter-dialog" role="dialog" aria-modal="true" aria-labelledby="plugin-filter-title" onSubmit={submit}>
        <header className="dialog-header">
          <h2 id="plugin-filter-title" className="k-subtitle">{plugin.name}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onCancel}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          {plugin.description && <p className="k-caption muted">{plugin.description}</p>}
          {plugin.parameters.map((param) => {
            const label = param.label || param.name;
            if (param.type === "boolean") {
              return (
                <label className="check-row" key={param.name}>
                  <input type="checkbox" checked={Boolean(v[param.name])} onChange={(e) => setV({ ...v, [param.name]: e.target.checked })} />
                  <span className="k-label">{label}</span>
                </label>
              );
            }
            return (
              <label className="field" key={param.name}>
                <span className="k-label">{label}</span>
                <input
                  className="text-input"
                  inputMode={param.type === "number" ? "decimal" : undefined}
                  value={String(v[param.name] ?? "")}
                  aria-label={label}
                  onChange={(e) => setV({ ...v, [param.name]: e.target.value })}
                />
              </label>
            );
          })}
          {plugin.parameters.length === 0 && <p className="k-caption muted">{t("This plug-in takes no settings.")}</p>}
          <div className="filter-scope">
            {selection.length > 0 && (
              <label className="check-row">
                <input type="checkbox" checked={useSelection} onChange={(e) => setUseSelection(e.target.checked)} />
                <span className="k-label">{tn(selection.length, "Selected glyph ({n})", "Selected glyphs ({n})")}</span>
              </label>
            )}
          </div>
          {error && <p className="notice notice-danger k-label">{error}</p>}
        </div>
        <footer className="dialog-footer">
          <span className="spacer" />
          <button type="button" className="btn btn-secondary" onClick={onCancel}>{t("Cancel")}</button>
          <button type="submit" className="btn btn-primary" disabled={busy || !p}>{t("Apply")}</button>
        </footer>
      </form>
    </div>
  );
}