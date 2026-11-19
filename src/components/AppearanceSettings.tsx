import { useEffect, useState } from "react";
import { errorMessage, type Appearance } from "../lib/backend";
import { COLOR_FIELDS, defaultCanvasColor } from "../lib/appearance";
import { LANGUAGES, t, type Lang } from "../lib/i18n";
import type { ThemePref } from "../lib/theme";

export function AppearanceSettings({ appearance, theme, language, onChange }: { appearance: Appearance; theme: ThemePref; language: Lang; onChange: (next: Appearance) => Promise<void> }) {
  const [width, setWidth] = useState(String(appearance.textWidth));
  const [error, setError] = useState<string | null>(null);
  useEffect(() => setWidth(String(appearance.textWidth)), [appearance.textWidth]);
  const save = async (next: Appearance) => {
    try { await onChange(next); setError(null); }
    catch (e) { setError(errorMessage(e)); }
  };
  const saveWidth = () => {
    const n = Number(width);
    if (!Number.isInteger(n) || n < 1000 || n > 100000) { setError(t("Text View width must be between 1000 and 100000.")); return; }
    if (n !== appearance.textWidth) save({ ...appearance, textWidth: n });
  };
  return <section className="appearance-settings" aria-labelledby="appearance-title">
    <h3 id="appearance-title" className="k-title">{t("Appearance")}</h3>
    <div className="appearance-top">
      <label className="field"><span className="k-label">{t("Theme")}</span><select aria-label={t("Theme")} value={theme} onChange={e => save({ ...appearance, theme: e.target.value as ThemePref })}><option value="system">{t("System")}</option><option value="light">{t("Light")}</option><option value="dark">{t("Dark")}</option></select></label>
      <label className="field"><span className="k-label">{t("Language")}</span><select aria-label={t("Language")} value={language} onChange={e => save({ ...appearance, language: e.target.value as Lang })}>{LANGUAGES.map(l => <option key={l.id} value={l.id}>{l.name}</option>)}</select></label>
      <label className="field"><span className="k-caption muted">{t("Text View width (1/1000 em)")}</span><input type="number" min="1000" max="100000" step="1000" aria-label={t("Text View width (1/1000 em)")} value={width} onChange={e => setWidth(e.target.value)} onBlur={saveWidth} onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); saveWidth(); } }} /></label>
      <div className="field"><span className="k-caption muted">{t("Handle size")}</span><div className="segmented" role="group" aria-label={t("Handle size")}>{(["small", "medium", "large"] as const).map((size, i) => <button key={size} type="button" aria-pressed={appearance.handleSize === size} className={`segment${appearance.handleSize === size ? " is-selected" : ""}`} onClick={() => save({ ...appearance, handleSize: size })}>{t(["Small", "Medium", "Large"][i])}</button>)}</div></div>
    </div>
    <h4 className="k-label">{t("Canvas colours")}</h4>
    <table className="appearance-colors"><thead><tr><th scope="col">{t("Element")}</th><th scope="col">{t("Light")}</th><th scope="col">{t("Dark")}</th></tr></thead><tbody>{COLOR_FIELDS.map(field => <tr key={field.id}><th scope="row">{t(field.label)}</th>{(["light", "dark"] as const).map(mode => <td key={mode}><input type="color" aria-label={t("{element} — {theme}", { element: t(field.label), theme: t(mode === "light" ? "Light" : "Dark") })} value={appearance.colors[mode][field.id] ?? defaultCanvasColor(mode, field.id)} onChange={e => save({ ...appearance, colors: { ...appearance.colors, [mode]: { ...appearance.colors[mode], [field.id]: e.target.value } } })} /></td>)}</tr>)}</tbody></table>
    <div><button type="button" className="btn btn-secondary" onClick={() => save({ ...appearance, handleSize: "medium", textWidth: 16000, colors: { light: {}, dark: {} } })}>{t("Restore standard appearance")}</button></div>
    <p className="k-caption muted">{t("Changes apply immediately. Light and dark colours are stored separately.")}</p>
    {error && <p role="alert" className="notice notice-danger">{error}</p>}
  </section>;
}
