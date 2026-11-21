import { useEffect, useState } from "react";
import { errorMessage, type Appearance } from "../lib/backend";
import { COLOR_FIELDS, defaultCanvasColor } from "../lib/appearance";
import { LANGUAGES, t, type Lang } from "../lib/i18n";
import type { ThemePref } from "../lib/theme";

export function AppearanceSettings({ appearance, theme, language, onChange }: { appearance: Appearance; theme: ThemePref; language: Lang; onChange: (next: Appearance) => Promise<void> }) {
  const [width, setWidth] = useState(String(appearance.textWidth));
  const [snap, setSnap] = useState(String(appearance.snapDistance ?? 6));
  useEffect(() => setSnap(String(appearance.snapDistance ?? 6)), [appearance.snapDistance]);
  const saveSnap = () => {
    const n = Number(snap);
    if (!Number.isInteger(n) || n < 0 || n > 30) { setError(t("Snap distance must be between 0 and 30 pixels.")); return; }
    if (n !== (appearance.snapDistance ?? 6)) save({ ...appearance, snapDistance: n });
  };
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
      <label className="field" title={t("How close a dragged node has to come to another node, a metric line or a guide to snap to it. 0 turns snapping off; holding Ctrl while dragging turns it off for that drag.")}><span className="k-caption muted">{t("Snap distance (px)")}</span><input type="number" min="0" max="30" step="1" aria-label={t("Snap distance (px)")} value={snap} onChange={e => setSnap(e.target.value)} onBlur={saveSnap} onKeyDown={e => { if (e.key === "Enter") { e.preventDefault(); saveSnap(); } }} /></label>
      <div className="field"><span className="k-caption muted">{t("Handle size")}</span><div className="segmented" role="group" aria-label={t("Handle size")}>{(["small", "medium", "large"] as const).map((size, i) => <button key={size} type="button" aria-pressed={appearance.handleSize === size} className={`segment${appearance.handleSize === size ? " is-selected" : ""}`} onClick={() => save({ ...appearance, handleSize: size })}>{t(["Small", "Medium", "Large"][i])}</button>)}</div></div>
    </div>
    <label className="check-row" title={t("Switch a text tab to right to left when its text starts with Arabic or Hebrew letters, and back to left to right for other letters")}><input type="checkbox" checked={appearance.autoDirection} onChange={e => save({ ...appearance, autoDirection: e.target.checked })} /><span className="k-body">{t("Text tabs follow the writing direction of their text")}</span></label>
    <h4 className="k-label">{t("Canvas colours")}</h4>
    <table className="appearance-colors"><thead><tr><th scope="col">{t("Element")}</th><th scope="col">{t("Light")}</th><th scope="col">{t("Dark")}</th></tr></thead><tbody>{COLOR_FIELDS.map(field => <tr key={field.id}><th scope="row">{t(field.label)}</th>{(["light", "dark"] as const).map(mode => <td key={mode}><input type="color" aria-label={t("{element} — {theme}", { element: t(field.label), theme: t(mode === "light" ? "Light" : "Dark") })} value={appearance.colors[mode][field.id] ?? defaultCanvasColor(mode, field.id)} onChange={e => save({ ...appearance, colors: { ...appearance.colors, [mode]: { ...appearance.colors[mode], [field.id]: e.target.value } } })} /></td>)}</tr>)}</tbody></table>
    <div><button type="button" className="btn btn-secondary" onClick={() => save({ ...appearance, handleSize: "medium", textWidth: 16000, autoDirection: true, snapDistance: 6, colors: { light: {}, dark: {} } })}>{t("Restore standard appearance")}</button></div>
    <p className="k-caption muted">{t("Changes apply immediately. Light and dark colours are stored separately.")}</p>
    {error && <p role="alert" className="notice notice-danger">{error}</p>}
  </section>;
}
