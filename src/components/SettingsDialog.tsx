import { useEffect, useState } from "react";
import { backend, errorMessage, type Appearance, type PluginInfo, type PythonInfo, type Settings, type UpdateInfo } from "../lib/backend";
import { AppearanceSettings } from "./AppearanceSettings";
import { UpdateSettings } from "./UpdateSettings";
import type { ThemePref } from "../lib/theme";
import { SHORTCUT_ACTIONS, effectiveShortcut, shortcutFromEvent } from "../lib/shortcuts";
import { CloseIcon } from "./Icons";
import { t, type Lang } from "../lib/i18n";

interface Props {
  /** The tab to open on (Help › Keyboard Shortcuts, Window › Plugin Manager). */
  initialTab?: Tab;
  settings: Settings;
  theme: ThemePref;
  language: Lang;
  updateInfo: UpdateInfo | null;
  onAppearance: (next: Appearance) => Promise<void>;
  onToggleUpdates: (enabled: boolean) => Promise<void>;
  onCheckUpdates: () => Promise<void>;
  plugins: PluginInfo[];
  onClose: () => void;
  onSetPluginEnabled: (name: string, enabled: boolean) => void;
  onSetShortcut: (action: string, shortcut: string) => void;
  onResetShortcuts: () => void;
  onSetPythonPath: (path: string | null) => void;
  onOpenPluginsFolder: () => void;
  onReloadPlugins: () => void;
}

export type Tab = "appearance" | "updates" | "plugins" | "shortcuts" | "python";

const TYPE_LABEL: Record<string, string> = { filter: "Filter", reporter: "Reporter", general: "General" };

/** S44 · Settings: plug-ins, keyboard shortcuts and the Python runtime. */
export function SettingsDialog({ settings, theme, language, updateInfo, onAppearance, onToggleUpdates, onCheckUpdates, plugins, onClose, onSetPluginEnabled, onSetShortcut, onResetShortcuts, onSetPythonPath, onOpenPluginsFolder, onReloadPlugins, initialTab }: Props) {
  const [tab, setTab] = useState<Tab>(initialTab ?? "appearance");
  const [recording, setRecording] = useState<string | null>(null);
  const [python, setPython] = useState<PythonInfo | null>(null);
  const [pythonPath, setPythonPath] = useState(settings.pythonPath ?? "");
  const [pythonError, setPythonError] = useState<string | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape" && !recording) onClose(); };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose, recording]);

  // While rebinding, capture the next combination and keep it from reaching the app.
  useEffect(() => {
    if (!recording) return;
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopImmediatePropagation();
      if (e.key === "Escape") { setRecording(null); return; }
      const text = shortcutFromEvent(e);
      if (text) { onSetShortcut(recording, text); setRecording(null); }
    };
    window.addEventListener("keydown", onKey, { capture: true });
    return () => window.removeEventListener("keydown", onKey, { capture: true });
  }, [recording, onSetShortcut]);

  useEffect(() => {
    let cancelled = false;
    backend.pythonInfo().then((info) => { if (!cancelled) { setPython(info); setPythonError(null); } }, (e) => { if (!cancelled) setPythonError(errorMessage(e)); });
    return () => { cancelled = true; };
  }, [settings.pythonPath]);

  const tabs: { id: Tab; label: string }[] = [
    { id: "appearance", label: t("Appearance") },
    { id: "updates", label: t("Updates") },
    { id: "plugins", label: t("Plugins") },
    { id: "shortcuts", label: t("Shortcuts") },
    { id: "python", label: t("Python") },
  ];

  return (
    <div className="scrim" role="presentation">
      <div className="dialog dialog-md settings-dialog" role="dialog" aria-modal="true" aria-labelledby="settings-title">
        <header className="dialog-header">
          <h2 id="settings-title" className="k-subtitle">{t("Settings")}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onClose}><CloseIcon /></button>
        </header>
        <div className="settings-layout">
        <div className="settings-tabs" role="tablist" aria-orientation="vertical" aria-label={t("Settings")} onKeyDown={e => {
          const current = tabs.findIndex(tb => tb.id === tab);
          const index = e.key === "ArrowDown" ? (current + 1) % tabs.length : e.key === "ArrowUp" ? (current + tabs.length - 1) % tabs.length : e.key === "Home" ? 0 : e.key === "End" ? tabs.length - 1 : -1;
          if (index >= 0) { e.preventDefault(); setTab(tabs[index].id); document.getElementById(`settings-tab-${tabs[index].id}`)?.focus(); }
        }}>
          {tabs.map((tb) => (
            <button key={tb.id} id={`settings-tab-${tb.id}`} role="tab" aria-selected={tab === tb.id} aria-controls="settings-panel" tabIndex={tab === tb.id ? 0 : -1} className={`segment${tab === tb.id ? " is-selected" : ""}`} onClick={() => setTab(tb.id)}>{tb.label}</button>
          ))}
        </div>
        <div id="settings-panel" role="tabpanel" aria-labelledby={`settings-tab-${tab}`} className="dialog-body settings-body">
          {tab === "appearance" && <AppearanceSettings appearance={settings.appearance} theme={theme} language={language} onChange={onAppearance} />}
          {tab === "updates" && <UpdateSettings enabled={settings.updates.enabled} info={updateInfo} onToggle={onToggleUpdates} onCheck={onCheckUpdates} />}
          {tab === "plugins" && (
            <>
              <div className="settings-row">
                <span className="k-caption muted">{t("Plug-ins live in the Plugins folder. Disable one to hide it from the menus.")}</span>
                <span className="spacer" />
                <button type="button" className="btn btn-secondary" onClick={onReloadPlugins}>{t("Reload")}</button>
                <button type="button" className="btn btn-secondary" onClick={onOpenPluginsFolder}>{t("Open Plugins Folder")}</button>
              </div>
              {plugins.length === 0 && <p className="k-caption muted">{t("No plug-ins installed.")}</p>}
              <ul className="settings-list">
                {plugins.map((p) => (
                  <li key={p.id} className="settings-item">
                    <label className="check-row">
                      <input type="checkbox" checked={p.enabled} onChange={(e) => onSetPluginEnabled(p.name, e.target.checked)} />
                      <span className="k-label">{p.name}</span>
                    </label>
                    <span className="settings-meta k-caption muted">
                      {t(TYPE_LABEL[p.type] ?? p.type)}{p.version ? ` · ${p.version}` : ""}
                    </span>
                    {p.description && <span className="settings-desc k-caption muted">{p.description}</span>}
                  </li>
                ))}
              </ul>
            </>
          )}
          {tab === "shortcuts" && (
            <>
              <div className="settings-row">
                <span className="k-caption muted">{t("Click Change, then press the keys. Escape cancels.")}</span>
                <span className="spacer" />
                <button type="button" className="btn btn-secondary" onClick={onResetShortcuts}>{t("Reset All")}</button>
              </div>
              <ul className="settings-list">
                {SHORTCUT_ACTIONS.map((a) => {
                  const current = effectiveShortcut(a, settings.shortcuts);
                  const overridden = settings.shortcuts[a.id] !== undefined;
                  return (
                    <li key={a.id} className="settings-item shortcut-item">
                      <span className="k-label">{t(a.label)}</span>
                      <span className="spacer" />
                      {recording === a.id ? (
                        <span className="shortcut-recording k-label">{t("Press keys…")}</span>
                      ) : (
                        <kbd className="shortcut-keys">{current}</kbd>
                      )}
                      <button type="button" className="btn btn-secondary" onClick={() => setRecording(recording === a.id ? null : a.id)}>
                        {recording === a.id ? t("Cancel") : t("Change")}
                      </button>
                      {overridden && <button type="button" className="btn btn-secondary" onClick={() => onSetShortcut(a.id, "")}>{t("Reset")}</button>}
                    </li>
                  );
                })}
              </ul>
            </>
          )}
          {tab === "python" && (
            <>
              <p className="k-caption muted">{t("Scripts and plug-ins run in Python. Kernest bundles its own; you can point it at another install.")}</p>
              <div className="settings-python-info">
                <div className="settings-row"><span className="k-label">{t("Version")}</span><span className="k-caption">{python?.version ?? t("Not found")}</span></div>
                <div className="settings-row"><span className="k-label">{t("Path")}</span><span className="k-caption settings-path">{python?.path ?? "—"}</span></div>
                {python?.bundled && <span className="k-caption muted">{t("Using the bundled Python.")}</span>}
                {pythonError && <p className="notice notice-danger k-label">{pythonError}</p>}
              </div>
              <label className="field">
                <span className="k-label">{t("Custom Python path")}</span>
                <input className="text-input" value={pythonPath} aria-label={t("Custom Python path")} placeholder={t("Leave empty to use the bundled Python")} onChange={(e) => setPythonPath(e.target.value)} />
              </label>
              <div className="settings-row">
                <span className="spacer" />
                <button type="button" className="btn btn-secondary" onClick={() => { setPythonPath(""); onSetPythonPath(null); }}>{t("Use Bundled")}</button>
                <button type="button" className="btn btn-primary" onClick={() => onSetPythonPath(pythonPath.trim() || null)}>{t("Apply")}</button>
              </div>
            </>
          )}
        </div>
        </div>
        <footer className="dialog-footer">
          <span className="spacer" />
          <button type="button" className="btn btn-secondary" onClick={onClose}>{t("Close")}</button>
        </footer>
      </div>
    </div>
  );
}