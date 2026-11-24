import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { emit, listen } from "@tauri-apps/api/event";
import { PREVIEW_EVENTS, type PreviewSource } from "./lib/previewEvents";
import { defaultPreviewText } from "./lib/previewPane";
import { backend, errorMessage, initialSettings, isTauri, type Appearance, type Milestone, type Onboarding, type UpdateInfo, type BatchOperation, type ClipGlyph, type CompareView, type DocStatus, type FeatureImportPreview, type FilterName, type FilterParams, type FontView, type GlyphSetInfo, type KeepSide, type MetricsSourceInfo, type OutputLine, type PasteMode, type PluginInfo, type ScriptEntry, type Settings, type SvgPlacement, type UndoStep } from "./lib/backend";
import { classifyPath, forgetRecent, kernestRoot, HANDBOOK_URL, REPO_URL, loadRecents, openExternal, pickCompareSource, pickFeaFile, pickFontToOpen, pickGlyphsSavePath, pickMetricsSource, pickSavePath, pickScript, pickSourceToImport, pickSvgFiles, rememberRecent, showStartAtLaunch, type RecentFont } from "./lib/desktop";
import { TitleBar, type Menu, type MenuItem } from "./components/TitleBar";
import { SHORTCUT_ACTIONS, effectiveShortcut, matchShortcut, parseShortcut } from "./lib/shortcuts";
import { loadTheme, saveTheme, type ThemePref } from "./lib/theme";
import { applyCanvasColors } from "./lib/appearance";
import { StartWindow } from "./components/StartWindow";
import { MenuSearch } from "./components/MenuSearch";
import type { Tab as SettingsTab } from "./components/SettingsDialog";
import { NewFontDialog } from "./components/NewFontDialog";
import { WelcomeDialog, type StartAction } from "./components/WelcomeDialog";
import { GuidedTour } from "./components/GuidedTour";
import { AddGlyphsDialog } from "./components/AddGlyphsDialog";
import { CompositesDialog } from "./components/CompositesDialog";
import { PasteSpecialDialog } from "./components/PasteSpecialDialog";
import { RenameDialog } from "./components/RenameDialog";
import { FeatureLinesDialog } from "./components/FeatureLinesDialog";
import { FontViewScreen } from "./components/FontViewScreen";
import { EditView, sendEditCommand } from "./components/EditView";
import { AddIcon, CloseIcon } from "./components/Icons";
import { TextView } from "./components/TextView";
import { KerningView } from "./components/KerningView";
import { FontInfoView } from "./components/FontInfoView";
import { ExportDialog } from "./components/ExportDialog";
import { UfoExportDialog } from "./components/UfoExportDialog";
import { FilterDialog } from "./components/FilterDialog";
import { SvgImportDialog } from "./components/SvgImportDialog";
import { MetricsImportDialog } from "./components/MetricsImportDialog";
import { FeatureImportDialog } from "./components/FeatureImportDialog";
import { ImportNotesDialog } from "./components/ImportNotesDialog";
import { AboutDialog } from "./components/AboutDialog";
import { ScriptDialog } from "./components/ScriptDialog";
import { PluginFilterDialog } from "./components/PluginFilterDialog";
import { SettingsDialog } from "./components/SettingsDialog";
import { CompareDialog } from "./components/CompareDialog";
import { language, LANGUAGES, saveLanguage, t, tn, translateMessage, type Lang } from "./lib/i18n";
import { copyGlyphsToClipboard } from "./lib/glyphClipboard";
import { formatGlyphNames, NAME_FORMATS, type NameFormat } from "./lib/glyphNames";

const CLEAN: DocStatus = { dirty: false, canUndo: false, canRedo: false };

/** "Undo Edit Outlines in “a”", "Undo Change Metrics in 5 glyphs", or plain "Undo". */
function undoLabel(redo: boolean, step: UndoStep | null | undefined): string {
  if (!step) return redo ? t("Redo") : t("Undo");
  const action = t(step.action);
  if (step.glyph) return redo ? t("Redo {action} in “{name}”", { action, name: step.glyph }) : t("Undo {action} in “{name}”", { action, name: step.glyph });
  if (step.glyphs > 1) return redo ? tn(step.glyphs, "Redo {action} in {n} glyph", "Redo {action} in {n} glyphs", { action }) : tn(step.glyphs, "Undo {action} in {n} glyph", "Undo {action} in {n} glyphs", { action });
  return redo ? t("Redo {action}", { action }) : t("Undo {action}", { action });
}

const REPORTERS_KEY = "kernest.reporters";
const loadReporters = (): string[] => {
  try { const v = JSON.parse(localStorage.getItem(REPORTERS_KEY) ?? "[]"); return Array.isArray(v) ? v.filter((x) => typeof x === "string") : []; } catch { return []; }
};
const saveReporters = (v: string[]) => { try { localStorage.setItem(REPORTERS_KEY, JSON.stringify(v)); } catch { /* not persisted */ } };
const LAST_SCRIPT_KEY = "kernest.lastScript";
const loadLastScript = (): string | null => { try { return localStorage.getItem(LAST_SCRIPT_KEY); } catch { return null; } };
const saveLastScript = (p: string) => { try { localStorage.setItem(LAST_SCRIPT_KEY, p); } catch { /* not persisted */ } };

/** Builds the Script menu's folder submenus from the discovered scripts. */
function buildScriptMenu(scripts: ScriptEntry[], run: (e: ScriptEntry) => void): MenuItem[] {
  const root: MenuItem[] = [];
  const folders = new Map<string, MenuItem>();
  const ensureFolder = (path: string): MenuItem[] => {
    if (!path) return root;
    const existing = folders.get(path);
    if (existing) return existing.submenu!;
    const parts = path.split("/");
    const parent = ensureFolder(parts.slice(0, -1).join("/"));
    const item: MenuItem = { label: parts[parts.length - 1], submenu: [] };
    parent.push(item);
    folders.set(path, item);
    return item.submenu!;
  };
  for (const s of scripts) ensureFolder(s.folder).push({ label: s.name, onSelect: () => run(s) });
  if (root.length) root[0].separatorBefore = true;
  return root;
}

/** A View-menu toggle remembered across sessions ("0" = off, "1" = on). */
function useViewToggle(key: string, defaultOn: boolean): [boolean, () => void] {
  const [on, setOn] = useState(() => {
    try {
      const raw = localStorage.getItem(key);
      return raw === null ? defaultOn : raw !== "0";
    } catch { return defaultOn; }
  });
  const toggle = useCallback(() => setOn((v) => {
    try { localStorage.setItem(key, v ? "0" : "1"); } catch { /* not persisted */ }
    return !v;
  }), [key]);
  return [on, toggle];
}

export default function App() {
  const [font, setFont] = useState<FontView | null>(null);
  const [status, setStatus] = useState<DocStatus>(CLEAN);
  const [tabs, setTabs] = useState<string[]>([]);
  const [active, setActive] = useState<string | null>(null); // null = Font tab
  const [versions, setVersions] = useState<Record<string, number>>({});
  const [fontStale, setFontStale] = useState(false);
  const [recents, setRecents] = useState<RecentFont[]>(() => loadRecents());
  const [glyphSets, setGlyphSets] = useState<GlyphSetInfo[] | null>(null);
  const [addGlyphSets, setAddGlyphSets] = useState<GlyphSetInfo[] | null>(null);
  const [fontSelection, setFontSelection] = useState<string[]>([]);
  const [exporting, setExporting] = useState(false);
  const [exportingUfo, setExportingUfo] = useState(false);
  const [about, setAbout] = useState(false);
  const [theme, setThemeState] = useState<ThemePref>(loadTheme);
  // The UI language; changing it re-renders everything (t() reads the current one).
  const [lang, setLangState] = useState<Lang>(language);
  const setLang = (l: Lang) => { changeAppearance({ ...settings.appearance, language: l }).catch(fail); };
  const setTheme = (theme: ThemePref) => { changeAppearance({ ...settings.appearance, theme }).catch(fail); };
  const [showMetrics, toggleMetrics] = useViewToggle("kernest.showMetrics", true);
  const [showAnchors, toggleAnchors] = useViewToggle("kernest.showAnchors", true);
  const [showNodes, toggleNodes] = useViewToggle("kernest.showNodes", true);
  const [showGuides, toggleGuides] = useViewToggle("kernest.showGuides", true);
  const [showBackground, toggleBackground] = useViewToggle("kernest.showBackground", true);
  const [showImage, toggleImage] = useViewToggle("kernest.showImage", true);
  const [showAnnotations, toggleAnnotations] = useViewToggle("kernest.showAnnotations", true);
  const [showBoundingBox, toggleBoundingBox] = useViewToggle("kernest.showBoundingBox", false);
  const [showZones, toggleZones] = useViewToggle("kernest.showZones", true);
  const [showSidebar, toggleSidebar] = useViewToggle("kernest.showSidebar", true);
  const [showPreview, togglePreview] = useViewToggle("kernest.showPreview", false);
  const [importNotes, setImportNotes] = useState<{ source: string; familyName: string; warnings: string[] } | null>(null);
  const [toast, setToast] = useState<{ kind: "info" | "danger"; text: string } | null>(null);
  // ---------- Scripts, plug-ins and settings (phase 6) ----------
  const [scripts, setScripts] = useState<ScriptEntry[]>([]);
  const [plugins, setPlugins] = useState<PluginInfo[]>([]);
  const [settings, setSettings] = useState<Settings>(initialSettings);
  const [settingsReady, setSettingsReady] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [dismissedUpdate, setDismissedUpdate] = useState<string | null>(null);
  const [updating, setUpdating] = useState(false);
  // Help › Check for Updates… shows its result even when automatic checks are off.
  const [manualUpdateCheck, setManualUpdateCheck] = useState(false);
  useEffect(() => applyCanvasColors(settings.appearance, theme), [settings.appearance, theme]);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [compare, setCompare] = useState<CompareView | null>(null);
  const [scriptDialog, setScriptDialog] = useState<{ mode: "macro" | "script"; name?: string } | null>(null);
  const [scriptOutput, setScriptOutput] = useState<OutputLine[]>([]);
  const [scriptRunning, setScriptRunning] = useState(false);
  const [scriptError, setScriptError] = useState<string | null>(null);
  const [pluginFilter, setPluginFilter] = useState<PluginInfo | null>(null);
  const [enabledReporters, setEnabledReporters] = useState<string[]>(loadReporters);
  const lastScript = useRef<string | null>(loadLastScript());
  const dirtyRef = useRef(false);
  dirtyRef.current = status.dirty;

  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), toast.kind === "danger" ? 8000 : 3000);
    return () => clearTimeout(t);
  }, [toast]);

  // Scripts, plug-ins and settings load once; script output streams in live.
  useEffect(() => {
    backend.listScripts().then(setScripts, () => {});
    backend.listPlugins().then(setPlugins, () => {});
    backend.getSettings().then(s => {
      setSettings(s);
      if (s.appearance.theme) { saveTheme(s.appearance.theme); setThemeState(s.appearance.theme); }
      if (s.appearance.language) { saveLanguage(s.appearance.language); setLangState(s.appearance.language); }
      setSettingsReady(true);
    }, e => setToast({ kind: "danger", text: errorMessage(e) }));
    const off = backend.onScriptOutput((line) => setScriptOutput((prev) => [...prev, line]));
    return () => { off.then((f) => f()); };
  }, []);

  const fail = useCallback((e: unknown) => setToast({ kind: "danger", text: errorMessage(e) }), []);
  const openHandbook = useCallback(() => { openExternal(HANDBOOK_URL).catch(fail); }, [fail]);
  const changeAppearance = useCallback(async (appearance: Appearance) => {
    const next = await backend.setAppearance(appearance);
    setSettings(s => ({ ...s, appearance: next.appearance }));
    if (appearance.theme) { saveTheme(appearance.theme); setThemeState(appearance.theme); }
    if (appearance.language) { saveLanguage(appearance.language); setLangState(appearance.language); }
  }, []);
  const changeOnboarding = useCallback(async (onboarding: Onboarding) => {
    const next = await backend.setOnboarding(onboarding);
    setSettings(s => ({ ...s, onboarding: next.onboarding }));
  }, []);
  const checkForUpdates = useCallback(async () => {
    const info = await backend.checkUpdates();
    setUpdateInfo(info);
    setSettings(s => ({ ...s, updates: { ...s.updates, lastChecked: info.checkedAt, latestVersion: info.latestVersion, error: info.error } }));
  }, []);
  const toggleUpdates = useCallback(async (enabled: boolean) => {
    const next = await backend.setUpdateCheck(enabled);
    setSettings(s => ({ ...s, updates: next.updates }));
  }, []);
  useEffect(() => {
    if (!settingsReady || !settings.updates.enabled) { setUpdateInfo(null); return; }
    checkForUpdates().catch(fail);
    const timer = setInterval(() => checkForUpdates().catch(fail), 3600000);
    return () => clearInterval(timer);
  }, [settingsReady, settings.updates.enabled, checkForUpdates, fail]);
  const completedMilestones = useRef(settings.onboarding.completed);
  completedMilestones.current = settings.onboarding.completed;
  const recordMilestone = useCallback((milestone: Milestone) => {
    if (completedMilestones.current.includes(milestone)) return;
    completedMilestones.current = [...completedMilestones.current, milestone];
    backend.recordMilestone(milestone).then(s => setSettings(current => ({ ...current, onboarding: { ...current.onboarding, completed: s.onboarding.completed } })), fail);
  }, [fail]);

  const show = useCallback((view: FontView) => {
    setFont(view);
    setStatus(view.status);
    setFontStale(false);
  }, []);

  const loadDocument = useCallback((view: FontView) => {
    show(view);
    setTabs([]);
    setActive(null);
    setVersions({});
    recordMilestone("created");
  }, [show, recordMilestone]);

  /** Asks before throwing away unsaved changes. Resolves true when it is OK to continue. */
  // S50 · "Save changes to …?" before closing, opening or replacing an edited font.
  const [savePrompt, setSavePrompt] = useState<{ name: string; resolve: (choice: "save" | "discard" | "cancel") => void } | null>(null);
  const saveRef = useRef<() => Promise<boolean>>(async () => false);
  const fontNameRef = useRef("");
  const confirmDiscard = useCallback(async () => {
    if (!dirtyRef.current) return true;
    const choice = await new Promise<"save" | "discard" | "cancel">((resolve) => setSavePrompt({ name: fontNameRef.current, resolve }));
    setSavePrompt(null);
    if (choice === "save") return saveRef.current();
    return choice === "discard";
  }, []);

  // Closing the window (caption button, Alt+F4) also asks first.
  useEffect(() => {
    if (!isTauri) return;
    const win = getCurrentWindow();
    const unlisten = win.onCloseRequested(async (e) => {
      if (!(await confirmDiscard())) e.preventDefault();
    });
    return () => { unlisten.then((f) => f()); };
  }, [confirmDiscard]);

  const showNewFont = useCallback(async () => {
    if (await confirmDiscard()) backend.glyphSets().then(setGlyphSets, fail);
  }, [confirmDiscard, fail]);

  const openPath = useCallback(async (path: string, confirmed = false) => {
    if (!confirmed && !(await confirmDiscard())) return;
    try {
      const view = await backend.openFont(path);
      loadDocument(view);
      setRecents(rememberRecent(view.path ?? path, view.familyName));
    } catch (e) {
      fail(e);
    }
  }, [confirmDiscard, loadDocument, fail]);

  const open = useCallback(async () => {
    if (!isTauri) return fail(t("Opening files needs the desktop app (npm run tauri dev)."));
    if (!(await confirmDiscard())) return;
    const path = await pickFontToOpen();
    if (path) await openPath(path, true);
  }, [openPath, confirmDiscard, fail]);

  const importPath = useCallback(async (path: string, confirmed = false) => {
    if (!confirmed && !(await confirmDiscard())) return;
    setToast({ kind: "info", text: t("Importing…") });
    try {
      const r = await backend.importFont(path);
      loadDocument(r.view);
      if (r.warnings.length > 0) {
        setToast(null);
        setImportNotes({ source: r.source, familyName: r.view.familyName, warnings: r.warnings });
      } else {
        setToast({ kind: "info", text: t("Imported {name}. Use Save As to keep it as a .kernest font.", { name: r.view.familyName }) });
      }
    } catch (e) {
      fail(e);
    }
  }, [confirmDiscard, loadDocument, fail]);

  const importSource = useCallback(async (folder = false) => {
    if (!isTauri) return fail(t("Importing fonts needs the desktop app (npm run tauri dev)."));
    if (!(await confirmDiscard())) return;
    const path = await pickSourceToImport(folder);
    if (path) await importPath(path, true);
  }, [confirmDiscard, importPath, fail]);

  /** Opens a .kernest font or imports a source or compiled font, from Explorer or a drop on the window. */
  const openAny = useCallback(async (path: string) => {
    const kind = classifyPath(path);
    if (kind === "kernest") return openPath(kernestRoot(path));
    if (kind === "source" || kind === "compiled") return importPath(path);
    fail(t("Kernest opens .kernest fonts and imports .glyphs, .glyphspackage, .ufo, .designspace, .otf, .ttf and .woff2 fonts."));
  }, [openPath, importPath, fail]);

  // Fonts from outside: the paths Kernest was started with, paths from a second launch
  // ("Open in Kernest" while it's running), and files dropped on the window.
  const [dropping, setDropping] = useState(false);
  const openAnyRef = useRef(openAny);
  openAnyRef.current = openAny;
  useEffect(() => {
    if (!isTauri) return;
    backend.takeLaunchPaths().then((p) => {
      if (p[0]) openAnyRef.current(p[0]);
      // Start window › Show at launch turned off: reopen the most recent font instead.
      else if (!showStartAtLaunch()) { const last = loadRecents()[0]; if (last) openAnyRef.current(last.path); }
    }, () => {});
    const offOpen = listen<string[]>("open-paths", (e) => { if (e.payload[0]) openAnyRef.current(e.payload[0]); });
    const offDrop = getCurrentWebview().onDragDropEvent((e) => {
      const p = e.payload;
      if (p.type === "enter" || p.type === "over") setDropping(true);
      else if (p.type === "leave") setDropping(false);
      else if (p.type === "drop") {
        setDropping(false);
        if (p.paths[0]) openAnyRef.current(p.paths[0]);
      }
    });
    return () => {
      offOpen.then((f) => f());
      offDrop.then((f) => f());
    };
  }, []);

  const openSample = useCallback(async () => {
    if (await confirmDiscard()) backend.openSample().then(loadDocument, fail);
  }, [confirmDiscard, loadDocument, fail]);

  const finishWelcome = useCallback(async (action: StartAction | null, experience: "beginner" | "experienced") => {
    await changeOnboarding({ ...settings.onboarding, welcomeDone: true, experience, tourEnabled: action !== null && experience === "beginner", tourStep: 0 });
    if (action === "new") await showNewFont();
    if (action === "open") await open();
    if (action === "sample") await openSample();
    if (action === "import") await importSource(false);
  }, [settings.onboarding, changeOnboarding, showNewFont, open, openSample, importSource]);

  const refreshStatusAfterSave = useCallback(async () => {
    const view = await backend.fontView(font?.masterId);
    show(view);
  }, [font?.masterId, show]);

  const saveAs = useCallback(async (): Promise<boolean> => {
    if (!font) return false;
    try {
      const path = await pickSavePath(font.familyName);
      if (!path) return false;
      const saved = await backend.saveFont(path);
      setRecents(rememberRecent(saved, font.familyName));
      await refreshStatusAfterSave();
      setToast({ kind: "info", text: t("Saved to {path}", { path: saved }) });
      return true;
    } catch (e) {
      fail(e);
      return false;
    }
  }, [font, refreshStatusAfterSave, fail]);

  const saveAsGlyphs = useCallback(async () => {
    if (!font) return;
    try {
      const path = await pickGlyphsSavePath(font.familyName);
      if (!path) return;
      const saved = await backend.saveGlyphs(path);
      setToast({ kind: "info", text: t("Saved to {path}", { path: saved }) });
    } catch (e) {
      fail(e);
    }
  }, [font, fail]);

  const save = useCallback(async (): Promise<boolean> => {
    if (!font) return false;
    if (!font.path) return saveAs();
    try {
      const saved = await backend.saveFont();
      setRecents(rememberRecent(saved, font.familyName));
      await refreshStatusAfterSave();
      setToast({ kind: "info", text: t("Saved {name}", { name: font.familyName }) });
      return true;
    } catch (e) {
      fail(e);
      return false;
    }
  }, [font, saveAs, refreshStatusAfterSave, fail]);
  saveRef.current = save;
  fontNameRef.current = font?.familyName ?? "";

  const closeFont = useCallback(async () => {
    if (!(await confirmDiscard())) return;
    await backend.closeFont().catch(() => {});
    setFont(null);
    setStatus(CLEAN);
    setTabs([]);
    setActive(null);
  }, [confirmDiscard]);

  // ---------- Tabs ----------
  // Tab ids: "g:<glyph>" for Edit View, "text:<n>" for text views, "kerning" for the kerning window.

  const openTab = useCallback((id: string) => {
    setTabs((t) => (t.includes(id) ? t : [...t, id]));
    setActive(id);
  }, []);
  const openGlyph = useCallback((name: string) => openTab(`g:${name}`), [openTab]);
  const textCounter = useRef(0);
  // Text tabs are named after their sample text.
  const [textTitles, setTextTitles] = useState<Record<string, string>>({});
  const titleSetters = useRef(new Map<string, (title: string) => void>());
  const textTitleSetter = (id: string) => {
    let f = titleSetters.current.get(id);
    if (!f) { f = (title: string) => setTextTitles((m) => (m[id] === title ? m : { ...m, [id]: title })); titleSetters.current.set(id, f); }
    return f;
  };
  const newTextTab = useCallback(() => openTab(`text:${++textCounter.current}`), [openTab]);
  const openKerning = useCallback(() => openTab("kerning"), [openTab]);
  // A click on a kerning bar in a text tab opens the Kerning window on that pair.
  const [kerningFocus, setKerningFocus] = useState<{ left: string; right: string; seq: number } | null>(null);
  const openKerningPair = useCallback((left: string, right: string) => {
    setKerningFocus((f) => ({ left, right, seq: (f?.seq ?? 0) + 1 }));
    openTab("kerning");
  }, [openTab]);
  // SMO-36: the preview pane under glyph tabs (text kept per glyph) and the preview window.
  const [previewTexts, setPreviewTexts] = useState<Record<string, string>>({});
  const openPreviewWindow = useCallback(() => {
    if (!isTauri) return fail(t("The preview window needs the desktop app (npm run tauri dev)."));
    backend.openPreviewWindow().catch(fail);
  }, [fail]);
  const navigateTour = useCallback((step: number) => {
    if (!font) return;
    if (step !== 5) setExporting(false);
    if (step === 0) setActive(null);
    if (step === 1) {
      const glyph = font.glyphs.find(g => g.name === "n") ?? font.glyphs.find(g => g.name !== "space");
      if (glyph) openGlyph(glyph.name);
      else setToast({ kind: "info", text: t("Add a glyph before starting to draw.") });
    }
    if (step === 2) { const text = tabs.find(id => id.startsWith("text:")); if (text) setActive(text); else newTextTab(); }
    if (step === 3) openKerning();
    if (step === 4) openTab("info");
    if (step === 5) setExporting(true);
  }, [font, tabs, openGlyph, newTextTab, openKerning, openTab]);

  const closeTab = useCallback((name: string) => {
    setTabs((t) => {
      const i = t.indexOf(name);
      const next = t.filter((x) => x !== name);
      setActive((a) => (a === name ? next[Math.min(i, next.length - 1)] ?? null : a));
      return next;
    });
  }, []);

  // Closed tabs, newest last, for View › Reopen Closed Tab.
  const closedTabs = useRef<string[]>([]);
  const closeTabRemembered = useCallback((id: string) => {
    closedTabs.current = [...closedTabs.current.filter((x) => x !== id), id].slice(-20);
    closeTab(id);
  }, [closeTab]);
  const [menuSearch, setMenuSearch] = useState(false);
  const [settingsTab, setSettingsTab] = useState<SettingsTab | undefined>(undefined);
  // Keys for menu commands defined later in the render (Alt+Q, F11, Ctrl+Tab).
  const menuKeys = useRef<{ search: () => void; fullScreen: () => void; cycle: (dir: 1 | -1) => void }>({ search: () => {}, fullScreen: () => {}, cycle: () => {} });
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.altKey && !e.ctrlKey && !e.shiftKey && e.key.toLowerCase() === "q") { e.preventDefault(); menuKeys.current.search(); }
      else if (e.key === "F11") { e.preventDefault(); menuKeys.current.fullScreen(); }
      else if (e.ctrlKey && !e.altKey && e.key === "Tab") { e.preventDefault(); menuKeys.current.cycle(e.shiftKey ? -1 : 1); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Glyph tabs whose glyph no longer exists (removed, or an undone add) are closed.
  useEffect(() => {
    if (!font) return;
    const names = new Set(font.glyphs.map((g) => g.name));
    const gone = tabs.filter((t) => t.startsWith("g:") && !names.has(t.slice(2)));
    for (const t of gone) closeTab(t);
  }, [font, tabs, closeTab]);

  // The Font View is refreshed lazily when it becomes visible after edits.
  useEffect(() => {
    if (font && active === null && fontStale) backend.fontView(font.masterId).then(show, fail);
  }, [active, fontStale, font, show, fail]);

  // Bumped on every change, so text and kerning tabs refresh.
  const [docVersion, setDocVersion] = useState(0);
  // SMO-36: the preview window mirrors the current tab: a text tab's text, or a glyph
  // tab's preview text. Other tabs, and no font, send null so the window empties.
  const previewSource = useMemo((): PreviewSource | null => {
    if (!font || active === null) return null;
    if (active.startsWith("g:")) {
      const g = active.slice(2);
      return { text: previewTexts[g] ?? defaultPreviewText(g), masterId: font.masterId, version: docVersion * 100000 + (versions[g] ?? 0), title: g };
    }
    if (active.startsWith("text:")) {
      const text = textTitles[active] ?? "";
      return { text, masterId: font.masterId, version: docVersion, title: text };
    }
    return null;
  }, [font, active, previewTexts, textTitles, docVersion, versions]);
  const previewSourceRef = useRef(previewSource);
  previewSourceRef.current = previewSource;
  useEffect(() => { if (isTauri) emit(PREVIEW_EVENTS.source, previewSource).catch(() => {}); }, [previewSource]);
  useEffect(() => {
    if (!isTauri) return;
    const offs = [
      listen(PREVIEW_EVENTS.ready, () => { emit(PREVIEW_EVENTS.source, previewSourceRef.current).catch(() => {}); }),
      listen<string>(PREVIEW_EVENTS.openGlyph, (e) => { openGlyph(e.payload); getCurrentWindow().setFocus().catch(() => {}); }),
    ];
    return () => { offs.forEach((p) => p.then((f) => f())); };
  }, [openGlyph]);
  // Glyph tabs showing a brace or bracket layer instead of the current master's layer.
  const [layerOf, setLayerOf] = useState<Record<string, string | undefined>>({});
  useEffect(() => setLayerOf({}), [font?.masterId]);

  const onCommitted = useCallback((s: DocStatus) => {
    setStatus(s);
    setFontStale(true);
    setDocVersion((v) => v + 1);
  }, []);

  /**
   * After a structural change (axes, masters, info) everything may differ: refresh the
   * font (keeping the current master if it still exists) and every open tab.
   */
  const refreshAll = useCallback(async (masterId?: string) => {
    const view = await backend.fontView(masterId).catch(() => backend.fontView());
    show(view);
    setDocVersion((v) => v + 1);
    // Every open glyph tab reloads (its outlines may have changed with the masters).
    // Tabs of glyphs that no longer exist are closed instead (see the effect above).
    const names = new Set(view.glyphs.map((g) => g.name));
    setVersions((v) => {
      const next = { ...v };
      for (const id of tabsRef.current) if (id.startsWith("g:") && names.has(id.slice(2))) next[id.slice(2)] = (next[id.slice(2)] ?? 0) + 1;
      return next;
    });
  }, [show]);
  const tabsRef = useRef<string[]>([]);
  tabsRef.current = tabs;

  const onStructureChanged = useCallback((s: DocStatus) => {
    setStatus(s);
    // Returned so callers that activate freshly saved data (filters) wait for the
    // refreshed font prop before selecting it.
    return refreshAll(font?.masterId).catch(fail);
  }, [refreshAll, font?.masterId, fail]);

  const showAddGlyphs = useCallback(() => {
    if (font) backend.glyphSets().then(setAddGlyphSets, fail);
  }, [font, fail]);

  /** The glyphs a Glyph-menu action acts on: the open glyph tab, or the Font View selection. */
  const glyphTargets = active?.startsWith("g:") ? [active.slice(2)] : active === null ? fontSelection : [];

  // ---------- Filters ----------
  const [filterDialog, setFilterDialog] = useState<FilterName | null>(null);
  // The last applied filter, remembered across sessions for Repeat Last Filter.
  const lastFilter = useRef<{ name: FilterName; params: FilterParams; allMasters: boolean } | null>(
    (() => { try { return JSON.parse(localStorage.getItem("kernest.lastFilter") ?? "null"); } catch { return null; } })()
  );
  /** The glyph a filter dialog previews: the open tab, else the first selected or drawn glyph. */
  const filterGlyph = active?.startsWith("g:") ? active.slice(2) : fontSelection[0] ?? font?.glyphs[0]?.name ?? "";

  const applyFilter = useCallback(async (name: FilterName, params: FilterParams, glyphs: string[], allMasters: boolean) => {
    if (!font || !filterGlyph) return;
    const masterId = layerOf[filterGlyph] ?? font.masterId;
    const r = await backend.runFilter(filterGlyph, masterId, glyphs, name, params, allMasters);
    lastFilter.current = { name, params, allMasters };
    localStorage.setItem("kernest.lastFilter", JSON.stringify(lastFilter.current));
    setFilterDialog(null);
    onStructureChanged(r.layer.status);
    const notes = (r.warnings ?? []).map((w) => translateMessage(w));
    if (r.changed.length) notes.unshift(tn(r.changed.length, "Filter applied to {n} glyph.", "Filter applied to {n} glyphs."));
    if (notes.length) setToast({ kind: "info", text: notes.join(" ") });
  }, [font, filterGlyph, layerOf, onStructureChanged]);

  /** Filter › Repeat Last Filter: the last settings over the current scope. */
  const repeatFilter = useCallback(() => {
    const last = lastFilter.current;
    if (!last || !font) return;
    const glyphs = active === null ? fontSelection : [];
    applyFilter(last.name, last.params, glyphs, last.allMasters).catch(fail);
  }, [font, active, fontSelection, applyFilter, fail]);

  // ---------- Import SVG outlines (S23) ----------
  const [svgDialog, setSvgDialog] = useState<{ mode: "files" | "paste"; files?: string[]; glyph?: string } | null>(null);

  const importSvgFiles = useCallback(async () => {
    if (!font) return;
    const files = await pickSvgFiles();
    if (files.length) setSvgDialog({ mode: "files", files });
  }, [font]);

  const pasteSvg = useCallback(() => {
    if (active?.startsWith("g:")) setSvgDialog({ mode: "paste", glyph: active.slice(2) });
  }, [active]);

  const applySvg = useCallback(async (placement: SvgPlacement, svgText: string | null) => {
    if (!font || !svgDialog) return;
    if (svgDialog.mode === "files") {
      const r = await backend.importSvgFiles(svgDialog.files ?? [], font.masterId, placement);
      setSvgDialog(null);
      onStructureChanged(r.status);
      const notes = r.warnings.map((w) => translateMessage(w));
      if (r.changed.length) notes.unshift(tn(r.changed.length, "Imported SVG into {n} glyph.", "Imported SVG into {n} glyphs."));
      if (notes.length) setToast({ kind: "info", text: notes.join(" ") });
    } else {
      const glyph = svgDialog.glyph ?? "";
      const r = await backend.importSvgText(glyph, layerOf[glyph] ?? font.masterId, svgText ?? "", placement);
      setSvgDialog(null);
      onStructureChanged(r.status);
      const notes = r.warnings.map((w) => translateMessage(w));
      if (notes.length) setToast({ kind: "info", text: notes.join(" ") });
    }
  }, [font, svgDialog, layerOf, onStructureChanged]);

  // ---------- Import metrics (S24) ----------
  const [metricsDialog, setMetricsDialog] = useState<{ path: string; info: MetricsSourceInfo } | null>(null);

  const importMetrics = useCallback(async () => {
    if (!font) return;
    const path = await pickMetricsSource();
    if (!path) return;
    try {
      const info = await backend.readMetricsSource(path);
      setMetricsDialog({ path, info });
    } catch (e) {
      fail(e);
    }
  }, [font, fail]);

  const applyMetrics = useCallback(async (sourceMasterId: string, keep: KeepSide, widths: boolean, kerning: boolean) => {
    if (!font || !metricsDialog) return;
    const r = await backend.importMetrics(metricsDialog.path, font.masterId, sourceMasterId, keep, widths, kerning);
    setMetricsDialog(null);
    onStructureChanged(r.status);
    const notes = r.warnings.map((w) => translateMessage(w));
    if (r.changed.length) notes.unshift(tn(r.changed.length, "Imported metrics for {n} glyph.", "Imported metrics for {n} glyphs."));
    if (notes.length) setToast({ kind: "info", text: notes.join(" ") });
  }, [font, metricsDialog, onStructureChanged]);

  // ---------- Import features (.fea) ----------
  const [featureDialog, setFeatureDialog] = useState<{ path: string; preview: FeatureImportPreview } | null>(null);

  const importFeatures = useCallback(async () => {
    if (!font) return;
    const path = await pickFeaFile();
    if (!path) return;
    try {
      const preview = await backend.previewFeatureImport(path);
      setFeatureDialog({ path, preview });
    } catch (e) {
      fail(e);
    }
  }, [font, fail]);

  // S42 · Compare the open font with another one; the report changes nothing.
  const compareFont = useCallback(async () => {
    if (!font) return;
    const path = await pickCompareSource();
    if (!path) return;
    try {
      setCompare(await backend.compareFonts(path));
    } catch (e) {
      fail(e);
    }
  }, [font, fail]);

  // S43 · Expand the instances into interpolated masters in a new unsaved document.
  const generateInstances = useCallback(async () => {
    if (!font) return;
    if (!(await confirmDiscard())) return;
    try {
      const view = await backend.generateInstances();
      loadDocument(view);
      setToast({ kind: "info", text: t("Generated {n} masters as a new font.", { n: view.masters.length }) });
    } catch (e) {
      fail(e);
    }
  }, [font, confirmDiscard, loadDocument, fail]);

  const applyFeatureImport = useCallback(async () => {
    if (!font || !featureDialog) return;
    const r = await backend.importFeatures(featureDialog.path);
    setFeatureDialog(null);
    onCommitted(r.status);
    setToast({ kind: "info", text: t("Imported features.") });
  }, [font, featureDialog, onCommitted]);

  // ---------- Scripts, plug-ins and settings ----------

  /** Runs a macro or script file against the open font; one run is one undo step. */
  const doRun = useCallback(async (opts: { code?: string; path?: string }) => {
    if (!font) return;
    setScriptRunning(true);
    setScriptError(null);
    const current = active?.startsWith("g:") ? active.slice(2) : filterGlyph || undefined;
    try {
      const r = await backend.runScript({ ...opts, current, masterId: font.masterId, glyphs: glyphTargets });
      if (r.error) setScriptError(translateMessage(r.error));
      if (r.applied) onStructureChanged((await backend.fontView(font.masterId)).status);
    } catch (e) {
      setScriptError(errorMessage(e));
    } finally {
      setScriptRunning(false);
    }
  }, [font, active, filterGlyph, glyphTargets, onStructureChanged]);

  const runScriptPath = useCallback((path: string, name?: string) => {
    setScriptOutput([]);
    setScriptError(null);
    setScriptDialog({ mode: "script", name });
    lastScript.current = path;
    saveLastScript(path);
    doRun({ path });
  }, [doRun]);

  const openMacro = useCallback(() => {
    setScriptOutput([]);
    setScriptError(null);
    setScriptDialog({ mode: "macro" });
  }, []);

  const runLastScript = useCallback(() => {
    if (lastScript.current) runScriptPath(lastScript.current);
  }, [runScriptPath]);

  /** Script › Run Script File…: an ad-hoc file outside the Scripts folder, so warn first. */
  const runScriptFile = useCallback(async () => {
    if (!isTauri) return fail(t("Running scripts needs the desktop app (npm run tauri dev)."));
    const path = await pickScript();
    if (!path) return;
    const ok = await ask(
      t("This script is outside your Scripts folder. It can change your font and run any code on your computer. Run it?"),
      { title: t("Run Script"), kind: "warning", okLabel: t("Run"), cancelLabel: t("Cancel") },
    );
    if (ok) runScriptPath(path);
  }, [fail, runScriptPath]);

  const cancelScriptRun = useCallback(() => { backend.cancelScript().catch(() => {}); }, []);

  const toggleReporter = useCallback((name: string) => {
    setEnabledReporters((prev) => {
      const next = prev.includes(name) ? prev.filter((n) => n !== name) : [...prev, name];
      saveReporters(next);
      return next;
    });
  }, []);

  /** A filter plug-in runs over the chosen glyphs as one undo step; errors stay in the dialog. */
  const applyPluginFilter = useCallback(async (plugin: PluginInfo, params: Record<string, number | string | boolean>, glyphs: string[]) => {
    if (!font) return;
    const current = active?.startsWith("g:") ? active.slice(2) : filterGlyph || undefined;
    const r = await backend.runFilterPlugin({ name: plugin.name, params, current, masterId: font.masterId, glyphs });
    if (!r.ok) throw new Error(r.error ? translateMessage(r.error) : t("The plug-in failed."));
    setPluginFilter(null);
    onStructureChanged((await backend.fontView(font.masterId)).status);
    setToast({ kind: "info", text: t("Plug-in applied.") });
  }, [font, active, filterGlyph, onStructureChanged]);

  const reloadScripts = useCallback(() => { backend.listScripts().then(setScripts, fail); }, [fail]);
  const reloadPlugins = useCallback(() => { backend.listPlugins().then(setPlugins, fail); }, [fail]);
  const openScriptsFolder = useCallback(() => { backend.openScriptsFolder().catch(fail); }, [fail]);
  const openPluginsFolder = useCallback(() => { backend.openPluginsFolder().catch(fail); }, [fail]);

  const setPluginEnabled = useCallback((name: string, enabled: boolean) => {
    backend.setPluginEnabled(name, enabled).then((s) => { setSettings(s); reloadPlugins(); }, fail);
  }, [fail, reloadPlugins]);
  const setShortcut = useCallback((action: string, shortcut: string) => {
    backend.setShortcut(action, shortcut).then(setSettings, fail);
  }, [fail]);
  const resetShortcuts = useCallback(() => { backend.resetShortcuts().then(setSettings, fail); }, [fail]);
  const setPythonPath = useCallback((path: string | null) => { backend.setPythonPath(path).then(setSettings, fail); }, [fail]);

  const removeGlyphs = useCallback(async (names: string[]) => {
    if (!font || names.length === 0) return;
    const shown = names.slice(0, 6).join(", ") + (names.length > 6 ? ` ${tn(names.length - 6, "and {n} more", "and {n} more")}` : "");
    const msg = names.length === 1
      ? t("Remove the glyph {names}?\n\nComponents that use it are turned into outlines in other glyphs. You can undo this with Ctrl+Z.", { names: shown })
      : tn(names.length, "Remove {n} glyph: {names}?\n\nComponents that use them are turned into outlines in other glyphs. You can undo this with Ctrl+Z.", "Remove {n} glyphs: {names}?\n\nComponents that use them are turned into outlines in other glyphs. You can undo this with Ctrl+Z.", { names: shown });
    const ok = isTauri ? await ask(msg, { title: t("Remove Glyph"), kind: "warning", okLabel: t("Remove"), cancelLabel: t("Cancel") }) : window.confirm(msg);
    if (!ok) return;
    try {
      const s = await backend.removeGlyphs(names);
      setFontSelection([]);
      onStructureChanged(s);
      setToast({ kind: "info", text: names.length === 1 ? t("Removed {name}", { name: names[0] }) : tn(names.length, "Removed {n} glyph", "Removed {n} glyphs") });
    } catch (e) {
      fail(e);
    }
  }, [font, onStructureChanged, fail]);

  /** Glyph › Update Metrics: re-applies every metrics key in the font. */
  const updateMetrics = useCallback(async () => {
    try {
      const r = await backend.updateMetrics();
      onStructureChanged(r.status);
      setToast(r.problems.length
        ? { kind: "danger", text: tn(r.problems.length, "Metrics updated; {n} key couldn't be applied: {problems}", "Metrics updated; {n} keys couldn't be applied: {problems}", { problems: r.problems.slice(0, 3).map((p) => translateMessage(p)).join("; ") }) }
        : { kind: "info", text: t("Metrics updated") });
    } catch (e) {
      fail(e);
    }
  }, [onStructureChanged, fail]);

  /** Glyph › Set Anchors: default anchors on the targeted glyphs, in every master. */
  const setAnchors = useCallback(async (names: string[]) => {
    if (!font || names.length === 0) return;
    try {
      const r = await backend.setDefaultAnchors(names);
      onStructureChanged(r.status);
      setToast({ kind: "info", text: r.updated ? tn(r.updated, "Set anchors on {n} glyph", "Set anchors on {n} glyphs") : t("These glyphs take no default anchors.") });
    } catch (e) {
      fail(e);
    }
  }, [font, onStructureChanged, fail]);

  const [composites, setComposites] = useState<{ initial: string } | null>(null);

  // ---------- Batch rename ----------
  const [renameOpen, setRenameOpen] = useState(false);
  const [featureLines, setFeatureLines] = useState<string[] | null>(null);

  /** Renames glyphs and updates every reference; warns about changed feature lines. */
  const applyRename = useCallback(async (renames: Record<string, string>) => {
    const r = await backend.renameGlyphs(renames);
    onStructureChanged(r.status);
    if (r.changedFeatureLines.length) setFeatureLines(r.changedFeatureLines);
    else setToast({ kind: "info", text: tn(Object.keys(renames).length, "Renamed {n} glyph", "Renamed {n} glyphs") });
  }, [onStructureChanged]);

  /** Unicode values typed in Font View. A value another glyph already uses is moved
   *  over only after asking. */
  const setUnicodes = useCallback(async (glyph: string, text: string) => {
    try {
      onStructureChanged(await backend.setGlyphUnicodes(glyph, text, false));
    } catch (e) {
      const raw = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
      if (!raw.includes("already belongs to")) { fail(e); throw e; }
      const msg = `${translateMessage(raw)} ${t("Move it to {glyph}?", { glyph })}`;
      const ok = isTauri ? await ask(msg, { title: t("Unicode"), kind: "warning", okLabel: t("Move"), cancelLabel: t("Cancel") }) : window.confirm(msg);
      if (!ok) throw e;
      await backend.setGlyphUnicodes(glyph, text, true).then(onStructureChanged, (e2) => { fail(e2); throw e2; });
    }
  }, [onStructureChanged, fail]);

  /** A single name edited in the list view goes through the same rename engine. */
  const renameOne = useCallback((glyph: string, name: string) => {
    applyRename({ [glyph]: name }).catch(fail);
  }, [applyRename, fail]);

  /** Glyph › Locked: a locked glyph refuses edits and is skipped by filters. */
  const toggleGlyphLocked = useCallback(async (names: string[]) => {
    if (!font || names.length === 0) return;
    const lock = !names.every((n) => font.glyphs.find((g) => g.name === n)?.locked);
    try {
      let s: DocStatus | null = null;
      for (const n of names) s = await backend.setGlyphLocked(n, lock);
      if (s) onStructureChanged(s);
      setToast({ kind: "info", text: lock ? tn(names.length, "Locked {n} glyph", "Locked {n} glyphs") : tn(names.length, "Unlocked {n} glyph", "Unlocked {n} glyphs") });
    } catch (e) {
      fail(e);
    }
  }, [font, onStructureChanged, fail]);

  /** Glyph › Create Composites: build glyphs from their parts (Aacute = A + acutecomb). */
  const createComposites = useCallback(async (names: string[]) => {
    const r = await backend.createComposites(names);
    setComposites(null);
    onStructureChanged(r.status);
    const skipped = r.skipped.length ? ` · ${tn(r.skipped.length, "{n} skipped (no recipe, missing parts or already drawn)", "{n} skipped (no recipe, missing parts or already drawn)")}` : "";
    setToast({ kind: "info", text: r.built.length ? `${tn(r.built.length, "Built {n} composite", "Built {n} composites")}${skipped}` : t("Nothing built: no recipes, missing parts, or the glyphs already have outlines.") });
  }, [onStructureChanged]);

  /** Font View › Batch: one edit over the whole selection, in a single undo step. */
  const runBatch = useCallback(async (operation: string) => {
    if (!font || fontSelection.length === 0) return;
    const names = fontSelection;
    try {
      if (operation === "set-anchors") {
        await setAnchors(names);
        return;
      }
      if (operation === "create-composite") {
        await createComposites(names);
        return;
      }
      if (operation === "update-metrics") {
        const r = await backend.updateMetricsFor(names);
        onStructureChanged(r.status);
        setToast(r.problems.length
          ? { kind: "danger", text: tn(r.problems.length, "Metrics updated; {n} key couldn't be applied: {problems}", "Metrics updated; {n} keys couldn't be applied: {problems}", { problems: r.problems.slice(0, 3).map((p) => translateMessage(p)).join("; ") }) }
          : { kind: "info", text: t("Metrics updated") });
        return;
      }
      const r = await backend.batchPathOperation(names, font.masterId, operation as BatchOperation);
      onStructureChanged(r.status);
      setToast({ kind: "info", text: tn(names.length, "Applied to {n} glyph", "Applied to {n} glyphs") });
    } catch (e) {
      fail(e);
    }
  }, [font, fontSelection, setAnchors, createComposites, onStructureChanged, fail]);

  // ---------- Copy & Paste Special ----------
  const [pasteOpen, setPasteOpen] = useState(false);

  /** The glyphs the copy actions act on, in font order. */
  const targetGlyphs = useMemo(() => {
    if (!font) return [];
    const names = new Set(glyphTargets);
    return font.glyphs.filter((g) => names.has(g.name));
  }, [font, glyphTargets]);

  const copyGlyphs = useCallback(async () => {
    if (targetGlyphs.length === 0) return;
    try {
      const n = await copyGlyphsToClipboard(targetGlyphs.map((g) => g.name));
      setToast({ kind: "info", text: tn(n, "Copied {n} glyph", "Copied {n} glyphs") });
    } catch (e) {
      fail(e);
    }
  }, [targetGlyphs, fail]);

  const copyNames = useCallback(async (format: NameFormat) => {
    if (targetGlyphs.length === 0) return;
    try {
      await navigator.clipboard.writeText(formatGlyphNames(targetGlyphs, format));
      setToast({ kind: "info", text: tn(targetGlyphs.length, "Copied {n} name", "Copied {n} names") });
    } catch (e) {
      fail(e);
    }
  }, [targetGlyphs, fail]);

  const pasteSpecial = useCallback(async (mode: PasteMode, glyphs: ClipGlyph[]) => {
    const r = await backend.pasteSpecial(mode, glyphTargets, glyphs);
    setPasteOpen(false);
    onStructureChanged(r.status);
    setToast({ kind: "info", text: tn(glyphs.length, "Pasted {n} glyph", "Pasted {n} glyphs") });
  }, [glyphTargets, onStructureChanged]);

  /** Font View drag (or Font Info text): a new working glyph order, in one undo step. */
  const reorderGlyphs = useCallback(async (order: string[]) => {
    try {
      const info = await backend.setGlyphOrder(order);
      onStructureChanged(info.status);
    } catch (e) {
      fail(e);
    }
  }, [onStructureChanged, fail]);

  const undoRedo = useCallback(async (redo: boolean) => {
    if (!font) return;
    try {
      const r = redo ? await backend.redo() : await backend.undo();
      if (!r) return;
      setStatus(r.status);
      if (r.kind === "font") return refreshAll(font.masterId);
      setFontStale(true);
      setDocVersion((v) => v + 1);
      const g = r.glyph;
      if (r.kind === "layer" && g) setVersions((v) => ({ ...v, [g]: (v[g] ?? 0) + 1 }));
      if (active === null) backend.fontView(font.masterId).then(show, fail);
    } catch (e) {
      fail(e);
    }
  }, [font, active, show, fail, refreshAll]);

  // Undo history in use (SMO-83): the glyph's own in its Edit View tab, the font's elsewhere.
  const undoGlyph = font && active?.startsWith("g:") ? active.slice(2) : null;
  useEffect(() => {
    if (!font) return;
    backend.setUndoContext(undoGlyph).then(setStatus, () => {});
  }, [font, undoGlyph]);

  const openInfo = useCallback(() => openTab("info"), [openTab]);

  const inGlyph = font !== null && active !== null && active.startsWith("g:");

  // One handler per shortcut action id; the registry below decides when each fires.
  const shortcutHandlers = useMemo<Record<string, () => void>>(() => ({
    "new-font": showNewFont,
    "open": open,
    "import": () => importSource(false),
    "save": save,
    "save-as": saveAs,
    "close": () => { if (active !== null) closeTabRemembered(active); else closeFont(); },
    "export": () => setExporting(true),
    "font-info": openInfo,
    "undo": () => undoRedo(false),
    "redo": () => undoRedo(true),
    "new-text-tab": newTextTab,
    "kerning": openKerning,
    "add-glyphs": showAddGlyphs,
    "remove-glyph": () => removeGlyphs(glyphTargets),
    "set-anchors": () => setAnchors(glyphTargets),
    "add-component": () => sendEditCommand("add-component"),
    "edit-background": () => sendEditCommand("edit-background"),
    "selection-to-background": () => sendEditCommand("to-background"),
    "add-to-background": () => sendEditCommand("add-to-background"),
    "correct-direction": () => sendEditCommand("correct-direction"),
    "tidy-up": () => sendEditCommand("tidy-up"),
    "harmonize": () => sendEditCommand("harmonize"),
    "align-selection": () => sendEditCommand("align-selection"),
    "repeat-filter": repeatFilter,
    "show-metrics": toggleMetrics,
    "show-anchors": toggleAnchors,
    "show-guides": toggleGuides,
    "show-background": toggleBackground,
    "show-bounding-box": toggleBoundingBox,
    "macro-panel": openMacro,
    "show-preview": togglePreview,
    "preview-window": openPreviewWindow,
    "settings": () => setSettingsOpen(true),
  }), [showNewFont, open, importSource, save, saveAs, active, closeTabRemembered, closeFont, openInfo, undoRedo, newTextTab, openKerning, showAddGlyphs, removeGlyphs, glyphTargets, setAnchors, repeatFilter, toggleMetrics, toggleAnchors, toggleGuides, toggleBackground, toggleBoundingBox, showSidebar, toggleSidebar, openMacro, togglePreview, openPreviewWindow]);

  // Windows shortcuts (Ctrl, not Cmd), honouring any rebinding stored in settings.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "F1") {
        e.preventDefault();
        return openHandbook();
      }
      // Ctrl+Shift+Z is the familiar redo, alongside the listed Ctrl+Y.
      if (e.ctrlKey && e.shiftKey && !e.altKey && e.key.toLowerCase() === "z") {
        e.preventDefault();
        return undoRedo(true);
      }
      const inField = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      for (const a of SHORTCUT_ACTIONS) {
        const s = parseShortcut(effectiveShortcut(a, settings.shortcuts));
        if (!s || !matchShortcut(e, s)) continue;
        // Text-tab shortcuts are handled by the text tab.
        if (a.scope === "text") continue;
        if (a.scope === "font" && !font) continue;
        if (a.scope === "glyph" && !inGlyph) continue;
        if (a.needsTargets && glyphTargets.length === 0) continue;
        if (a.allowInField === false && inField) continue;
        const fn = shortcutHandlers[a.id];
        if (!fn) continue;
        e.preventDefault();
        fn();
        return;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [shortcutHandlers, settings.shortcuts, font, inGlyph, glyphTargets, openHandbook, undoRedo]);

  const tabLabel = (id: string) => (id.startsWith("g:") ? id.slice(2) : id === "kerning" ? t("Kerning") : id === "info" ? t("Font Info")
    : textTitles[id] ? (textTitles[id].length > 18 ? `${textTitles[id].slice(0, 17)}…` : textTitles[id]) : t("Text {n}", { n: id.slice(5) }));
  const glyphList = useMemo(() => (font ? font.glyphs.map((g) => ({ name: g.name, width: g.width })) : []), [font]);

  /** The shortcut shown in a menu, honouring any rebinding stored in settings. */
  const shortcutText = (id: string) => {
    const a = SHORTCUT_ACTIONS.find((x) => x.id === id);
    return a ? effectiveShortcut(a, settings.shortcuts) : undefined;
  };
  const pluginFilters = plugins.filter((p) => p.type === "filter" && p.enabled);
  const reporterPlugins = plugins.filter((p) => p.type === "reporter" && p.enabled);

  // ---------- Menu bar extras (S46, S47) ----------
  const fontViewCommand = (cmd: string) => window.dispatchEvent(new CustomEvent("kernest:fontview", { detail: cmd }));
  const selectCommand = (cmd: "select-all" | "deselect-all" | "invert-selection") =>
    inGlyph ? () => sendEditCommand(cmd) : font && active === null ? () => fontViewCommand(cmd) : undefined;
  const reopenClosedTab = () => {
    const id = closedTabs.current.pop();
    if (id) openTab(id);
  };
  const cycleTab = (dir: 1 | -1) => {
    if (!font) return;
    const order: (string | null)[] = [null, ...tabs];
    const i = Math.max(0, order.indexOf(active));
    setActive(order[(i + dir + order.length) % order.length]);
  };
  const revertToSaved = async () => {
    if (!font?.path) return;
    if (status.dirty && !(await confirmDiscard())) return;
    await openPath(font.path, true);
  };
  const duplicateGlyphs = async (names: string[]) => {
    try {
      const r = await backend.duplicateGlyphs(names);
      onStructureChanged(r.status);
      setToast({ kind: "info", text: t("Duplicated as {names}", { names: r.added.join(", ") }) });
    } catch (e) {
      fail(e);
    }
  };
  const decomposeTargets = async () => {
    if (!font || !glyphTargets.length) return;
    try {
      const r = await backend.batchPathOperation(glyphTargets, font.masterId, "decompose");
      onStructureChanged(r.status);
    } catch (e) {
      fail(e);
    }
  };
  const exportAllPresets = async () => {
    try {
      const info = await backend.fontInfo();
      if (!info.exportPresets.length) return setToast({ kind: "info", text: t("Save an export preset in the Export dialog first.") });
      let files = 0;
      const problems: string[] = [];
      for (const p of info.exportPresets) {
        const r = await backend.exportPreset(p.name);
        files += r.files.length;
        problems.push(...r.problems.map((x) => `${p.name}: ${translateMessage(x)}`));
      }
      setToast(problems.length
        ? { kind: "danger", text: problems.slice(0, 2).join("; ") }
        : { kind: "info", text: tn(files, "Exported {n} font with every preset", "Exported {n} fonts with every preset") });
    } catch (e) {
      fail(e);
    }
  };
  const toggleFullScreen = async () => {
    if (!isTauri) return;
    const w = getCurrentWindow();
    await w.setFullscreen(!(await w.isFullscreen()));
  };
  const openSettingsAt = (tab?: SettingsTab) => { setSettingsTab(tab); setSettingsOpen(true); };
  const checkUpdatesNow = async () => {
    try {
      const info = await backend.checkUpdates(true);
      setUpdateInfo(info);
      setManualUpdateCheck(true);
      setDismissedUpdate(null);
      setToast(info.error
        ? { kind: "danger", text: translateMessage(info.error) }
        : info.available
          ? { kind: "info", text: t("Kernest {version} is available.", { version: info.latestVersion ?? "" }) }
          : { kind: "info", text: t("Kernest {version} is up to date.", { version: info.currentVersion }) });
    } catch (e) {
      fail(e);
    }
  };
  menuKeys.current = { search: () => setMenuSearch(true), fullScreen: () => { toggleFullScreen().catch(fail); }, cycle: cycleTab };

  const menus: Menu[] = [
    {
      label: t("File"),
      items: [
        { label: t("New…"), shortcut: shortcutText("new-font"), onSelect: showNewFont },
        { label: t("New from Glyph Sets…"), onSelect: showNewFont },
        { label: t("Open…"), shortcut: shortcutText("open"), onSelect: open },
        { label: t("Open Recent"), submenu: recents.length
          ? recents.filter((r, i) => recents.findIndex((x) => x.path.replace(/\\/g, "/").toLowerCase() === r.path.replace(/\\/g, "/").toLowerCase()) === i).slice(0, 10).map((r) => ({ label: `${r.familyName} — ${r.path.split(/[\\/]/).pop()}`, onSelect: () => { openPath(r.path).catch(fail); } }))
          : [{ label: t("No recent fonts") }] },
        { label: t("Open Sample Font"), onSelect: openSample },
        { label: t("Import…"), shortcut: shortcutText("import"), onSelect: () => importSource(false), separatorBefore: true },
        { label: t("Import Folder…"), onSelect: () => importSource(true) },
        { label: t("Import SVG…"), onSelect: font ? importSvgFiles : undefined },
        { label: t("Import Metrics…"), onSelect: font ? importMetrics : undefined },
        { label: t("Import Features…"), onSelect: font ? importFeatures : undefined },
        { label: t("Save"), shortcut: shortcutText("save"), onSelect: font ? save : undefined, separatorBefore: true },
        { label: t("Save As…"), shortcut: shortcutText("save-as"), onSelect: font ? saveAs : undefined },
        { label: t("Save As .glyphs…"), onSelect: font ? saveAsGlyphs : undefined },
        { label: t("Revert to Saved"), onSelect: font?.path ? () => { revertToSaved().catch(fail); } : undefined },
        { label: t("Close Font"), onSelect: font ? closeFont : undefined },
        { label: t("Export"), separatorBefore: true, submenu: [
          { label: t("Font…"), shortcut: shortcutText("export"), onSelect: font ? () => setExporting(true) : undefined },
          { label: t("UFO…"), onSelect: font ? () => setExportingUfo(true) : undefined },
          { label: t("Export All Presets"), onSelect: font ? () => { exportAllPresets(); } : undefined, separatorBefore: true },
          { label: t("Test Install"), onSelect: font ? () => backend.testInstall().then((r) => setToast({ kind: "info", text: t("Loaded for this session: {styles}", { styles: r.installed.join(", ") }) }), fail) : undefined, separatorBefore: true },
          { label: t("Web Preview"), onSelect: font ? () => backend.webPreview().then((url) => openExternal(url), fail) : undefined },
        ] },
        { label: t("Generate Instances"), onSelect: font ? generateInstances : undefined },
        { label: t("Font Info…"), shortcut: shortcutText("font-info"), onSelect: font ? openInfo : undefined, separatorBefore: true },
        { label: t("Compare with Font…"), onSelect: font ? compareFont : undefined },
        { label: t("Settings…"), shortcut: shortcutText("settings"), onSelect: () => openSettingsAt() },
        { label: t("Exit"), shortcut: "Alt+F4", onSelect: isTauri ? () => { getCurrentWindow().close().catch(fail); } : undefined, separatorBefore: true },
      ],
    },
    {
      label: t("Edit"),
      items: [
        { label: undoLabel(false, status.undo), shortcut: shortcutText("undo"), onSelect: status.canUndo ? () => undoRedo(false) : undefined },
        { label: undoLabel(true, status.redo), shortcut: shortcutText("redo"), onSelect: status.canRedo ? () => undoRedo(true) : undefined },
        { label: t("Cut"), shortcut: "Ctrl+X", onSelect: inGlyph ? () => sendEditCommand("cut") : undefined, separatorBefore: true },
        { label: t("Copy"), shortcut: "Ctrl+C", onSelect: inGlyph ? () => sendEditCommand("copy") : undefined },
        { label: t("Paste"), shortcut: "Ctrl+V", onSelect: inGlyph ? () => sendEditCommand("paste") : undefined },
        { label: t("Paste SVG…"), onSelect: inGlyph ? pasteSvg : undefined },
        { label: t("Copy Glyphs"), onSelect: font && glyphTargets.length ? copyGlyphs : undefined, separatorBefore: true },
        { label: t("Copy Glyph Names"), submenu: NAME_FORMATS.map((f) => ({ label: t(f.label), onSelect: font && glyphTargets.length ? () => copyNames(f.id) : undefined })) },
        { label: t("Paste Special…"), onSelect: font ? () => setPasteOpen(true) : undefined },
        { label: t("Select All"), onSelect: selectCommand("select-all"), separatorBefore: true },
        { label: t("Deselect All"), onSelect: selectCommand("deselect-all") },
        { label: t("Invert Selection"), onSelect: selectCommand("invert-selection") },
        { label: t("Find…"), onSelect: font ? () => { setActive(null); setTimeout(() => fontViewCommand("find"), 50); } : undefined, separatorBefore: true },
        { label: t("Find and Replace…"), onSelect: font ? () => setRenameOpen(true) : undefined },
        { label: t("Edit Background"), shortcut: shortcutText("edit-background"), onSelect: inGlyph ? () => sendEditCommand("edit-background") : undefined, separatorBefore: true },
        { label: t("Selection to Background"), shortcut: shortcutText("selection-to-background"), onSelect: inGlyph ? () => sendEditCommand("to-background") : undefined },
        { label: t("Add to Background"), shortcut: shortcutText("add-to-background"), onSelect: inGlyph ? () => sendEditCommand("add-to-background") : undefined },
        { label: t("Swap with Background"), onSelect: inGlyph ? () => sendEditCommand("swap-background") : undefined },
        { label: t("Clear Background"), onSelect: inGlyph ? () => sendEditCommand("clear-background") : undefined },
      ],
    },
    {
      label: t("Glyph"),
      items: [
        { label: t("Add Glyphs…"), shortcut: shortcutText("add-glyphs"), onSelect: font ? showAddGlyphs : undefined },
        { label: t("New Glyph…"), onSelect: font ? showAddGlyphs : undefined },
        { label: t("Duplicate Glyph"), onSelect: font && glyphTargets.length ? () => { duplicateGlyphs(glyphTargets); } : undefined },
        { label: t("Add Image…"), onSelect: active?.startsWith("g:") ? () => window.dispatchEvent(new CustomEvent("kernest:add-image")) : undefined },
        { label: glyphTargets.length > 1 ? tn(glyphTargets.length, "Remove {n} Glyph", "Remove {n} Glyphs") : t("Remove Glyph"), shortcut: shortcutText("remove-glyph"), onSelect: font && glyphTargets.length ? () => removeGlyphs(glyphTargets) : undefined },
        { label: t("Rename Glyphs…"), onSelect: font ? () => setRenameOpen(true) : undefined },
        { label: t("Add Component…"), shortcut: shortcutText("add-component"), onSelect: inGlyph ? () => sendEditCommand("add-component") : undefined, separatorBefore: true },
        { label: t("Component from Selection…"), onSelect: inGlyph ? () => sendEditCommand("component-from-selection") : undefined },
        { label: t("Set Anchors"), shortcut: shortcutText("set-anchors"), onSelect: font && glyphTargets.length ? () => setAnchors(glyphTargets) : undefined, separatorBefore: true },
        { label: t("Create Composites…"), onSelect: font ? () => setComposites({ initial: glyphTargets.join(" ") }) : undefined },
        { label: t("Decompose Components"), onSelect: font && glyphTargets.length ? () => { decomposeTargets(); } : undefined },
        { label: t("Locked"), onSelect: font && glyphTargets.length ? () => toggleGlyphLocked(glyphTargets) : undefined, checked: glyphTargets.length > 0 && glyphTargets.every((n) => font?.glyphs.find((g) => g.name === n)?.locked), separatorBefore: true },
        { label: t("Update Metrics"), onSelect: font ? updateMetrics : undefined, separatorBefore: true },
      ],
    },
    {
      label: t("Path"),
      items: [
        { label: t("Open Corner"), onSelect: inGlyph ? () => sendEditCommand("open-corner") : undefined },
        { label: t("Reconnect Nodes"), onSelect: inGlyph ? () => sendEditCommand("reconnect-nodes") : undefined },
        { label: t("Connect Nodes"), onSelect: inGlyph ? () => sendEditCommand("connect-nodes") : undefined },
        { label: t("Close Open Paths"), onSelect: inGlyph ? () => sendEditCommand("close-open-paths") : undefined },
        { label: t("Make Node First"), onSelect: inGlyph ? () => sendEditCommand("make-node-first") : undefined },
        { label: t("Reverse Selected Contours"), onSelect: inGlyph ? () => sendEditCommand("reverse-contours") : undefined },
        { label: t("Correct Path Direction"), shortcut: shortcutText("correct-direction"), onSelect: inGlyph ? () => sendEditCommand("correct-direction") : undefined, separatorBefore: true },
        { label: t("Add Extremes"), onSelect: inGlyph ? () => sendEditCommand("add-extremes") : undefined },
        { label: t("Remove Overlap"), onSelect: inGlyph ? () => sendEditCommand("remove-overlap") : undefined },
        { label: t("Union"), onSelect: inGlyph ? () => sendEditCommand("union") : undefined },
        { label: t("Subtract"), onSelect: inGlyph ? () => sendEditCommand("subtract") : undefined },
        { label: t("Intersect"), onSelect: inGlyph ? () => sendEditCommand("intersect") : undefined },
        { label: t("Tidy up Paths"), shortcut: shortcutText("tidy-up"), onSelect: inGlyph ? () => sendEditCommand("tidy-up") : undefined, separatorBefore: true },
        { label: t("Align Selection"), shortcut: shortcutText("align-selection"), onSelect: inGlyph ? () => sendEditCommand("align-selection") : undefined },
        { label: t("Harmonize"), shortcut: shortcutText("harmonize"), onSelect: inGlyph ? () => sendEditCommand("harmonize") : undefined },
        { label: t("Super-smooth Nodes"), shortcut: "Alt+Shift+Enter", onSelect: inGlyph ? () => sendEditCommand("super-smooth") : undefined },
        { label: t("Round Coordinates"), onSelect: inGlyph ? () => sendEditCommand("round-coordinates") : undefined },
        { label: t("Transformations…"), onSelect: font && filterGlyph ? () => setFilterDialog("transformations") : undefined, separatorBefore: true },
        { label: t("Focus on Selected Path"), onSelect: inGlyph ? () => sendEditCommand("focus-path") : undefined, separatorBefore: true },
        { label: t("Clear Focus"), onSelect: inGlyph ? () => sendEditCommand("clear-focus") : undefined },
      ],
    },
    {
      label: t("Filter"),
      items: [
        { label: t("Round Corners…"), onSelect: font && filterGlyph ? () => setFilterDialog("round-corners") : undefined },
        { label: t("Offset Curve…"), onSelect: font && filterGlyph ? () => setFilterDialog("offset-curve") : undefined },
        { label: t("Transformations…"), onSelect: font && filterGlyph ? () => setFilterDialog("transformations") : undefined },
        { label: t("Shape Order…"), onSelect: inGlyph ? () => sendEditCommand("shape-order") : undefined },
        ...pluginFilters.map((p, i) => ({ label: p.name, onSelect: font && filterGlyph ? () => setPluginFilter(p) : undefined, separatorBefore: i === 0 })),
        { label: t("Repeat Last Filter"), shortcut: shortcutText("repeat-filter"), onSelect: font && lastFilter.current ? repeatFilter : undefined, separatorBefore: true },
      ],
    },
    { label: t("View"), items: [
      { label: t("New Tab"), shortcut: shortcutText("new-text-tab"), onSelect: font ? newTextTab : undefined },
      { label: t("Close Tab"), shortcut: shortcutText("close"), onSelect: active !== null ? () => closeTabRemembered(active) : undefined },
      { label: t("Reopen Closed Tab"), onSelect: font ? reopenClosedTab : undefined },
      { label: t("Next Tab"), shortcut: "Ctrl+Tab", onSelect: font ? () => cycleTab(1) : undefined },
      { label: t("Previous Tab"), shortcut: "Ctrl+Shift+Tab", onSelect: font ? () => cycleTab(-1) : undefined },
      { label: t("Zoom In"), onSelect: inGlyph ? () => sendEditCommand("zoom-in") : undefined, separatorBefore: true },
      { label: t("Zoom Out"), onSelect: inGlyph ? () => sendEditCommand("zoom-out") : undefined },
      { label: t("Zoom to Active Layer"), onSelect: inGlyph ? () => sendEditCommand("zoom-fit") : undefined },
      { label: t("Actual Size"), onSelect: inGlyph ? () => sendEditCommand("zoom-actual") : undefined },
      { label: t("Full Screen"), shortcut: "F11", onSelect: isTauri ? () => { toggleFullScreen().catch(fail); } : undefined },
      { label: t("Show Nodes"), onSelect: toggleNodes, checked: showNodes, separatorBefore: true },
      { label: t("Show Metrics"), shortcut: shortcutText("show-metrics"), onSelect: toggleMetrics, checked: showMetrics },
      { label: t("Show Anchors"), shortcut: shortcutText("show-anchors"), onSelect: toggleAnchors, checked: showAnchors },
      { label: t("Show Guides"), shortcut: shortcutText("show-guides"), onSelect: toggleGuides, checked: showGuides },
      { label: t("Show Background"), shortcut: shortcutText("show-background"), onSelect: toggleBackground, checked: showBackground },
      { label: t("Show Image"), onSelect: toggleImage, checked: showImage },
      { label: t("Show Annotations"), onSelect: toggleAnnotations, checked: showAnnotations },
      { label: t("Show Bounding Box"), shortcut: shortcutText("show-bounding-box"), onSelect: toggleBoundingBox, checked: showBoundingBox },
      { label: t("Show Alignment Zones"), onSelect: toggleZones, checked: showZones },
      { label: t("Show Sidebar"), onSelect: toggleSidebar, checked: showSidebar },
      { label: t("Show Preview"), shortcut: shortcutText("show-preview"), onSelect: togglePreview, checked: showPreview },
      { label: t("Theme: System"), onSelect: () => setTheme("system"), checked: theme === "system", separatorBefore: true },
      { label: t("Theme: Light"), onSelect: () => setTheme("light"), checked: theme === "light" },
      { label: t("Theme: Dark"), onSelect: () => setTheme("dark"), checked: theme === "dark" },
      ...LANGUAGES.map((l, i) => ({ label: `${t("Language")}: ${l.name}`, onSelect: () => setLang(l.id), checked: lang === l.id, separatorBefore: i === 0 })),
    ] },
    {
      label: t("Window"),
      items: [
        { label: t("Font View"), onSelect: font ? () => setActive(null) : undefined },
        { label: t("New Text Tab"), shortcut: shortcutText("new-text-tab"), onSelect: font ? newTextTab : undefined },
        { label: t("Kerning"), shortcut: shortcutText("kerning"), onSelect: font ? openKerning : undefined },
        { label: t("Preview Window"), shortcut: shortcutText("preview-window"), onSelect: isTauri ? openPreviewWindow : undefined },
        { label: t("Macro Panel"), shortcut: shortcutText("macro-panel"), onSelect: openMacro },
        { label: t("Plugin Manager"), onSelect: () => openSettingsAt("plugins"), separatorBefore: true },
        { label: t("Settings…"), shortcut: shortcutText("settings"), onSelect: () => openSettingsAt() },
      ],
    },
    {
      label: t("Script"),
      items: [
        { label: t("Macro Panel…"), shortcut: shortcutText("macro-panel"), onSelect: openMacro },
        { label: t("Run Last Script"), onSelect: lastScript.current ? runLastScript : undefined },
        { label: t("Run Script File…"), onSelect: runScriptFile },
        { label: t("Reload Scripts"), onSelect: reloadScripts, separatorBefore: true },
        { label: t("Open Scripts Folder"), onSelect: openScriptsFolder },
        ...buildScriptMenu(scripts, (e) => runScriptPath(e.path, e.name)),
        ...(reporterPlugins.length ? [{
          label: t("Reporters"),
          separatorBefore: true,
          submenu: reporterPlugins.map((p) => ({ label: p.name, checked: enabledReporters.includes(p.name), onSelect: () => toggleReporter(p.name) })),
        }] : []),
      ],
    },
    {
      label: t("Help"),
      items: [
        { label: t("Search Menus…"), shortcut: "Alt+Q", onSelect: () => setMenuSearch(true) },
        { label: t("Kernest Handbook"), shortcut: "F1", onSelect: openHandbook, separatorBefore: true },
        { label: t("Keyboard Shortcuts"), onSelect: () => openSettingsAt("shortcuts") },
        { label: t("Welcome to Kernest"), onSelect: () => changeOnboarding({ ...settings.onboarding, welcomeDone: false }).catch(fail) },
        { label: t("Restart guided tour"), onSelect: font ? () => changeOnboarding({ ...settings.onboarding, tourEnabled: true, tourStep: 0 }).then(() => navigateTour(0)).catch(fail) : undefined },
        { label: t("Reset getting-started checklist"), onSelect: () => changeOnboarding({ ...settings.onboarding, completed: [], tourEnabled: true, tourStep: 0 }).then(() => navigateTour(0)).catch(fail) },
        { label: t("What's New"), onSelect: () => { openExternal(`${REPO_URL}/releases/latest`).catch(fail); }, separatorBefore: true },
        { label: t("Check for Updates…"), onSelect: () => { checkUpdatesNow(); } },
        { label: t("Report a Problem on GitHub…"), onSelect: () => { openExternal(`${REPO_URL}/issues/new`).catch(fail); } },
        { label: t("About Kernest"), onSelect: () => setAbout(true), separatorBefore: true },
      ],
    },
  ];

  const title = font ? `${font.familyName}${font.path ? ".kernest" : ""}${status.dirty ? ` • ${t("Edited")}` : ""} — Kernest` : "Kernest";

  return (
    <div className="app">
      <TitleBar title={title} menus={menus} />
      {updateInfo?.available && updateInfo.latestVersion !== dismissedUpdate && (settings.updates.enabled || manualUpdateCheck) && (
        <div className="update-notice" role="status">
          <svg className="update-icon" width="18" height="18" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3l8 9h-5v9H9v-9H4z" fill="currentColor" /></svg>
          <span className="update-text">
            <strong className="k-label">{t("Kernest {version} is available", { version: updateInfo.latestVersion ?? "" })}</strong>
            <span className="k-caption muted">
              {updateInfo.notes ? `${t("New:")} ${updateInfo.notes} · ` : ""}
              <button type="button" className="link-btn" onClick={() => openExternal(`${REPO_URL}/releases/latest`).catch(fail)}>{t("View release notes on GitHub")}</button>
            </span>
          </span>
          <button type="button" className="btn btn-secondary btn-sm" disabled={updating} onClick={() => setDismissedUpdate(updateInfo.latestVersion)}>{t("Later")}</button>
          <button type="button" className="btn btn-primary btn-sm" disabled={updating || !updateInfo.latestVersion}
            title={t("Downloads the setup program, checks it against the release's checksums, installs it and restarts Kernest")}
            onClick={async () => {
              if (!updateInfo.latestVersion || !(await confirmDiscard())) return;
              setUpdating(true);
              try {
                await backend.installUpdate(updateInfo.latestVersion);
              } catch (e) {
                fail(e);
                setUpdating(false);
              }
            }}>{updating ? t("Downloading…") : t("Download & install")}</button>
        </div>
      )}
      {font ? (
        <>
          <div className="commandbar">
            <div className="doc-tabs" role="tablist" aria-label={t("Open tabs")}>
              <button role="tab" aria-selected={active === null} className={`doc-tab${active === null ? " is-selected" : ""}`} onClick={() => setActive(null)}>
                <span className="k-label">{t("Font")}</span>
              </button>
              {tabs.map((id) => (
                <div key={id} role="tab" aria-selected={active === id} className={`doc-tab${active === id ? " is-selected" : ""}${id.startsWith("g:") ? "" : " is-tool"}`} onClick={() => setActive(id)}>
                  <span className="k-label">{tabLabel(id)}</span>
                  <button className="tab-close" aria-label={t("Close {name}", { name: tabLabel(id) })} onClick={(e) => { e.stopPropagation(); closeTabRemembered(id); }}>
                    <CloseIcon size={12} />
                  </button>
                </div>
              ))}
              <button className="icon-btn" title={t("New Text Tab (Ctrl+T)")} aria-label={t("New Text Tab")} onClick={newTextTab}><AddIcon size={14} /></button>
            </div>
            <span className="spacer" />
            <div className="segmented" role="radiogroup" aria-label={t("Master")}>
              {font.masters.map((m) => (
                <button key={m.id} role="radio" aria-checked={m.id === font.masterId} className={`segment${m.id === font.masterId ? " is-selected" : ""}`}
                  onClick={() => backend.fontView(m.id).then(show, fail)}>
                  {m.name}
                </button>
              ))}
            </div>
            <button className="btn btn-secondary" title={t("Text Preview (Ctrl+T)")}
              onClick={() => { const text = tabs.find((id) => id.startsWith("text:")); if (text) setActive(text); else newTextTab(); }}>{t("Text Preview")}</button>
            <button className="btn btn-secondary" onClick={openInfo} title={t("Font Info (Ctrl+I)")}>{t("Font Info")}</button>
            <button className="btn btn-primary" onClick={() => setExporting(true)} title={t("Export fonts (Ctrl+E)")}>{t("Export")}</button>
          </div>
          {active === null && <FontViewScreen key={font.familyName + font.path} font={font} onOpenGlyph={openGlyph} onSelectionChange={setFontSelection} onAddGlyphs={showAddGlyphs} onSetExport={(names, on) => backend.setGlyphExport(names, on).then(onStructureChanged, fail)} onSetFilters={(filters) => backend.setFilters(filters).then(onStructureChanged, fail)} onSetTags={(glyph, tags) => backend.setGlyphTags(glyph, tags).then(onStructureChanged, fail)} onSetProductionName={(glyph, name) => backend.setProductionName(glyph, name).then(onStructureChanged, fail)} onSetDirection={(glyph, direction) => backend.setWritingDirection(glyph, direction).then(onStructureChanged, fail)} onSetColorLabel={(names, layer, label) => backend.setColorLabel(names, layer, label).then(onStructureChanged, (e) => { fail(e); throw e; })} onSetGlyphProperty={(names, property, value) => backend.setGlyphProperty(names, property, value).then(onStructureChanged, (e) => { fail(e); throw e; })} onSetVertical={(glyph, which, value) => backend.setVerticalMetric(glyph, font.masterId, which, value).then(onStructureChanged, fail)} onSetUnicodes={setUnicodes} onSetGroups={(glyph, left, right) => backend.setGlyphGroups(glyph, left, right).then(onStructureChanged, fail)} onRenameGlyphs={() => setRenameOpen(true)} onBatch={runBatch} onReorder={reorderGlyphs} onRenameGlyph={renameOne} onSetMetric={(glyph, metric, value) => backend.setMetric(glyph, font.masterId, metric, value).then((r) => onStructureChanged(r.status), fail)} onAddMissing={(language) => backend.addMissing(language).then((r) => onStructureChanged(r.status), fail)}
            onAddNamed={(name) => backend.addGlyphs(name, []).then((r) => { onStructureChanged(r.status); setToast({ kind: "info", text: r.added.length ? tn(r.added.length, "Added {n} glyph", "Added {n} glyphs") : t("{name} is already in the font", { name }) }); }, fail)} />}
          {tabs.map((id) => (
            <div key={id} className="tab-panel" hidden={active !== id}>
              {id.startsWith("g:") ? (
                <EditView glyph={id.slice(2)} masterId={layerOf[id.slice(2)] ?? font.masterId} masterKey={font.masterId} onPickLayer={(key) => setLayerOf((m) => ({ ...m, [id.slice(2)]: key === font.masterId ? undefined : key }))} version={versions[id.slice(2)] ?? 0} active={active === id} glyphs={glyphList} onCommitted={onCommitted} onStructureChanged={onStructureChanged} onError={fail} onWarning={(text) => setToast({ kind: "info", text })} showMetrics={showMetrics} showAnchors={showAnchors} showNodes={showNodes} showGuides={showGuides} showBackground={showBackground} showImage={showImage} showAnnotations={showAnnotations} showBoundingBox={showBoundingBox} showZones={showZones} showSidebar={showSidebar} onToggleSidebar={toggleSidebar} onToggleView={(v) => ({ nodes: toggleNodes, metrics: toggleMetrics, anchors: toggleAnchors, guides: toggleGuides, background: toggleBackground, zones: toggleZones })[v]()} handleSize={settings.appearance.handleSize} snapDistance={settings.appearance.snapDistance ?? 6} onDrawn={() => recordMilestone("drawn")} reporters={enabledReporters}
                  showPreview={showPreview} onTogglePreview={togglePreview} previewText={previewTexts[id.slice(2)]} onPreviewText={(text) => setPreviewTexts((m) => ({ ...m, [id.slice(2)]: text }))}
                  onOpenPreviewWindow={isTauri ? openPreviewWindow : undefined} onOpenGlyph={openGlyph} />
              ) : id === "info" ? (
                <FontInfoView version={docVersion} onChanged={onStructureChanged} onError={fail} onGenerateInstances={generateInstances} />
              ) : id === "kerning" ? (
                <KerningView masterId={font.masterId} masterName={font.masters.find((m) => m.id === font.masterId)?.name ?? ""} version={docVersion} onChanged={s => { onCommitted(s); recordMilestone("kerned"); }} onError={fail} fontSelection={fontSelection} onOpenText={newTextTab} focus={kerningFocus} />
              ) : (
                <TextView masterId={font.masterId} version={docVersion} active={active === id} onChanged={onCommitted} onError={fail} onOpenGlyph={openGlyph} onOpenKerning={openKerningPair} shortcuts={settings.shortcuts} textWidth={settings.appearance.textWidth} autoDirection={settings.appearance.autoDirection} onMilestone={recordMilestone} onTitle={textTitleSetter(id)} />
              )}
            </div>
          ))}
        </>
      ) : (
        <StartWindow
          recents={recents}
          onNew={showNewFont}
          onOpen={open}
          onOpenRecent={openPath}
          onForgetRecent={(p) => setRecents(forgetRecent(p))}
          onSample={openSample}
          onImport={() => importSource(false)}
          updateInfo={settings.updates.enabled || manualUpdateCheck ? updateInfo : null}
          onHandbook={(section) => { openExternal(section ? `${HANDBOOK_URL}#${section}` : HANDBOOK_URL).catch(fail); }}
          onShortcuts={() => openSettingsAt("shortcuts")}
          onWhatsNew={() => { openExternal(`${REPO_URL}/releases/latest`).catch(fail); }}
        />
      )}
      {about && <AboutDialog onClose={() => setAbout(false)} />}
      {savePrompt && (
        <div className="scrim" role="presentation">
          <div className="dialog save-prompt" role="alertdialog" aria-modal="true" aria-labelledby="save-prompt-title" aria-describedby="save-prompt-text"
            onKeyDown={(e) => { if (e.key === "Escape") { e.preventDefault(); savePrompt.resolve("cancel"); } }}>
            <div className="save-prompt-body">
              <span className="save-prompt-icon" aria-hidden="true">
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round"><path d="M5 4h11l3 3v13H5z" /><path d="M8 4v5h7V4M8 20v-6h8v6" /></svg>
              </span>
              <div>
                <h2 id="save-prompt-title" className="k-subtitle">{t("Save changes to “{name}”?", { name: savePrompt.name })}</h2>
                <p id="save-prompt-text" className="k-body muted">{t("Your changes will be lost if you don't save them.")}</p>
              </div>
            </div>
            <footer className="dialog-footer">
              <button type="button" className="btn btn-secondary" onClick={() => savePrompt.resolve("discard")}>{t("Don't Save")}</button>
              <span className="spacer" />
              <button type="button" className="btn btn-secondary" onClick={() => savePrompt.resolve("cancel")}>{t("Cancel")}</button>
              <button type="button" className="btn btn-primary" autoFocus onClick={() => savePrompt.resolve("save")}>{t("Save")}</button>
            </footer>
          </div>
        </div>
      )}
      {dropping && (
        <div className="drop-overlay" aria-hidden="true">
          <div className="drop-card k-subtitle">{t("Drop to open a .kernest font, or import .glyphs, .ufo or .designspace")}</div>
        </div>
      )}
      {importNotes && <ImportNotesDialog {...importNotes} onClose={() => setImportNotes(null)} />}
      {exporting && font && <ExportDialog fontPath={font.path} version={docVersion} onChanged={onStructureChanged} onClose={() => setExporting(false)} onExported={() => recordMilestone("exported")} onFontInfo={() => { setExporting(false); openTab("info"); }} onGlyph={name => { setExporting(false); openGlyph(name); }} />}
      {exportingUfo && font && <UfoExportDialog fontPath={font.path} onClose={() => setExportingUfo(false)} />}
      {filterDialog && font && filterGlyph && (
        <FilterDialog
          name={filterDialog}
          glyph={filterGlyph}
          masterId={layerOf[filterGlyph] ?? font.masterId}
          selection={active === null ? fontSelection : []}
          onCancel={() => setFilterDialog(null)}
          onApply={applyFilter}
        />
      )}
      {svgDialog && font && (
        <SvgImportDialog
          mode={svgDialog.mode}
          files={svgDialog.files}
          glyph={svgDialog.glyph}
          onCancel={() => setSvgDialog(null)}
          onApply={applySvg}
        />
      )}
      {metricsDialog && font && (
        <MetricsImportDialog
          path={metricsDialog.path}
          info={metricsDialog.info}
          onCancel={() => setMetricsDialog(null)}
          onApply={applyMetrics}
        />
      )}
      {featureDialog && font && (
        <FeatureImportDialog
          path={featureDialog.path}
          preview={featureDialog.preview}
          onCancel={() => setFeatureDialog(null)}
          onApply={applyFeatureImport}
        />
      )}
      {settingsReady && !settings.onboarding.welcomeDone && <WelcomeDialog onComplete={finishWelcome} />}
      {settingsReady && settings.onboarding.welcomeDone && settings.onboarding.tourEnabled && font && !glyphSets && !settingsOpen && (!exporting || settings.onboarding.tourStep === 5) && <GuidedTour onboarding={settings.onboarding} onChange={changeOnboarding} onNavigate={navigateTour} />}
      {glyphSets && (
        <NewFontDialog
          sets={glyphSets}
          onCancel={() => setGlyphSets(null)}
          onCreate={async setup => {
            loadDocument(await backend.newFont(setup));
            setGlyphSets(null);
          }}
        />
      )}
      {addGlyphSets && (
        <AddGlyphsDialog
          sets={addGlyphSets}
          onCancel={() => setAddGlyphSets(null)}
          onAdd={async (text, sets, recipes) => {
            const r = await backend.addGlyphs(text, sets);
            let built = 0;
            let s = r.status;
            if (recipes && r.added.length) {
              const c = await backend.createComposites(r.added);
              built = c.built.length;
              s = c.status;
            }
            setAddGlyphSets(null);
            onStructureChanged(s);
            const skipped = r.skipped.length ? ` · ${tn(r.skipped.length, "{n} already in the font", "{n} already in the font")}` : "";
            const compositesNote = built ? ` · ${tn(built, "built {n} composite", "built {n} composites")}` : "";
            setToast({ kind: "info", text: r.added.length ? `${tn(r.added.length, "Added {n} glyph", "Added {n} glyphs")}${skipped}${compositesNote}` : tn(r.skipped.length, "Nothing added: all {n} already in the font", "Nothing added: all {n} already in the font") });
          }}
        />
      )}
      {composites && (
        <CompositesDialog initial={composites.initial} onCancel={() => setComposites(null)} onCreate={createComposites} />
      )}
      {renameOpen && font && (
        <RenameDialog
          allNames={font.glyphs.map((g) => g.name)}
          selection={fontSelection}
          onCancel={() => setRenameOpen(false)}
          onRename={applyRename}
        />
      )}
      {pasteOpen && font && (
        <PasteSpecialDialog targets={glyphTargets} onCancel={() => setPasteOpen(false)} onPaste={pasteSpecial} />
      )}
      {featureLines && <FeatureLinesDialog lines={featureLines} onClose={() => setFeatureLines(null)} />}
      {scriptDialog && (
        <ScriptDialog
          mode={scriptDialog.mode}
          name={scriptDialog.name}
          output={scriptOutput}
          running={scriptRunning}
          error={scriptError}
          canRun={font !== null}
          onRun={(code) => doRun({ code })}
          onCancelRun={cancelScriptRun}
          onClose={() => setScriptDialog(null)}
        />
      )}
      {pluginFilter && font && (
        <PluginFilterDialog
          plugin={pluginFilter}
          selection={active === null ? fontSelection : []}
          onCancel={() => setPluginFilter(null)}
          onApply={applyPluginFilter}
        />
      )}
      {menuSearch && <MenuSearch menus={menus} onClose={() => setMenuSearch(false)} />}
      {settingsOpen && (
        <SettingsDialog
          initialTab={settingsTab}
          settings={settings}
          theme={theme}
          language={lang}
          updateInfo={updateInfo}
          onAppearance={changeAppearance}
          onToggleUpdates={toggleUpdates}
          onCheckUpdates={checkForUpdates}
          plugins={plugins}
          onClose={() => { setSettingsOpen(false); setSettingsTab(undefined); }}
          onSetPluginEnabled={setPluginEnabled}
          onSetShortcut={setShortcut}
          onResetShortcuts={resetShortcuts}
          onSetPythonPath={setPythonPath}
          onOpenPluginsFolder={openPluginsFolder}
          onReloadPlugins={reloadPlugins}
        />
      )}
      {compare && (
        <CompareDialog report={compare} currentFamily={font?.familyName ?? ""} onClose={() => setCompare(null)} />
      )}
      {toast && (
        <div className={`toast toast-${toast.kind} k-label`} role="status" onClick={() => setToast(null)}>
          {toast.text}
        </div>
      )}
    </div>
  );
}
