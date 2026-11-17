import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { listen } from "@tauri-apps/api/event";
import { backend, errorMessage, isTauri, type DocStatus, type FontView, type GlyphSetInfo } from "./lib/backend";
import { classifyPath, forgetRecent, kernestRoot, HANDBOOK_URL, loadRecents, openExternal, pickFontToOpen, pickSavePath, pickSourceToImport, rememberRecent, type RecentFont } from "./lib/desktop";
import { TitleBar, type Menu } from "./components/TitleBar";
import { loadTheme, saveTheme, type ThemePref } from "./lib/theme";
import { StartWindow } from "./components/StartWindow";
import { NewFontDialog } from "./components/NewFontDialog";
import { AddGlyphsDialog } from "./components/AddGlyphsDialog";
import { CompositesDialog } from "./components/CompositesDialog";
import { FontViewScreen } from "./components/FontViewScreen";
import { EditView, sendEditCommand } from "./components/EditView";
import { AddIcon, CloseIcon } from "./components/Icons";
import { TextView } from "./components/TextView";
import { KerningView } from "./components/KerningView";
import { FontInfoView } from "./components/FontInfoView";
import { ExportDialog } from "./components/ExportDialog";
import { ImportNotesDialog } from "./components/ImportNotesDialog";
import { AboutDialog } from "./components/AboutDialog";
import { language, LANGUAGES, saveLanguage, t, tn, translateMessage, type Lang } from "./lib/i18n";

const CLEAN: DocStatus = { dirty: false, canUndo: false, canRedo: false };

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
  const [about, setAbout] = useState(false);
  const [theme, setThemeState] = useState<ThemePref>(loadTheme);
  // The UI language; changing it re-renders everything (t() reads the current one).
  const [lang, setLangState] = useState<Lang>(language);
  const setLang = (l: Lang) => { saveLanguage(l); setLangState(l); };
  const setTheme = (t: ThemePref) => { saveTheme(t); setThemeState(t); };
  const [showMetrics, setShowMetrics] = useState(() => {
    try { return localStorage.getItem("kernest.showMetrics") !== "0"; } catch { return true; }
  });
  const toggleMetrics = useCallback(() => setShowMetrics((v) => {
    try { localStorage.setItem("kernest.showMetrics", v ? "0" : "1"); } catch { /* not persisted */ }
    return !v;
  }), []);
  const [showAnchors, setShowAnchors] = useState(() => {
    try { return localStorage.getItem("kernest.showAnchors") !== "0"; } catch { return true; }
  });
  const toggleAnchors = useCallback(() => setShowAnchors((v) => {
    try { localStorage.setItem("kernest.showAnchors", v ? "0" : "1"); } catch { /* not persisted */ }
    return !v;
  }), []);
  const [importNotes, setImportNotes] = useState<{ source: string; familyName: string; warnings: string[] } | null>(null);
  const [toast, setToast] = useState<{ kind: "info" | "danger"; text: string } | null>(null);
  const dirtyRef = useRef(false);
  dirtyRef.current = status.dirty;

  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), toast.kind === "danger" ? 8000 : 3000);
    return () => clearTimeout(t);
  }, [toast]);

  const fail = useCallback((e: unknown) => setToast({ kind: "danger", text: errorMessage(e) }), []);
  const openHandbook = useCallback(() => { openExternal(HANDBOOK_URL).catch(fail); }, [fail]);

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
  }, [show]);

  /** Asks before throwing away unsaved changes. Resolves true when it is OK to continue. */
  const confirmDiscard = useCallback(async () => {
    if (!dirtyRef.current) return true;
    const msg = t("You have unsaved changes. Discard them?");
    return isTauri ? ask(msg, { title: "Kernest", kind: "warning", okLabel: t("Discard"), cancelLabel: t("Cancel") }) : window.confirm(msg);
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

  /** Opens a .kernest font or imports a source, from Explorer or a drop on the window. */
  const openAny = useCallback(async (path: string) => {
    const kind = classifyPath(path);
    if (kind === "kernest") return openPath(kernestRoot(path));
    if (kind === "source") return importPath(path);
    fail(t("Kernest opens .kernest fonts and imports .glyphs, .glyphspackage, .ufo and .designspace sources."));
  }, [openPath, importPath, fail]);

  // Fonts from outside: the paths Kernest was started with, paths from a second launch
  // ("Open in Kernest" while it's running), and files dropped on the window.
  const [dropping, setDropping] = useState(false);
  const openAnyRef = useRef(openAny);
  openAnyRef.current = openAny;
  useEffect(() => {
    if (!isTauri) return;
    backend.takeLaunchPaths().then((p) => { if (p[0]) openAnyRef.current(p[0]); }, () => {});
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

  const refreshStatusAfterSave = useCallback(async () => {
    const view = await backend.fontView(font?.masterId);
    show(view);
  }, [font?.masterId, show]);

  const saveAs = useCallback(async () => {
    if (!font) return;
    try {
      const path = await pickSavePath(font.familyName);
      if (!path) return;
      const saved = await backend.saveFont(path);
      setRecents(rememberRecent(saved, font.familyName));
      await refreshStatusAfterSave();
      setToast({ kind: "info", text: t("Saved to {path}", { path: saved }) });
    } catch (e) {
      fail(e);
    }
  }, [font, refreshStatusAfterSave, fail]);

  const save = useCallback(async () => {
    if (!font) return;
    if (!font.path) return saveAs();
    try {
      const saved = await backend.saveFont();
      setRecents(rememberRecent(saved, font.familyName));
      await refreshStatusAfterSave();
      setToast({ kind: "info", text: t("Saved {name}", { name: font.familyName }) });
    } catch (e) {
      fail(e);
    }
  }, [font, saveAs, refreshStatusAfterSave, fail]);

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
  const newTextTab = useCallback(() => openTab(`text:${++textCounter.current}`), [openTab]);
  const openKerning = useCallback(() => openTab("kerning"), [openTab]);

  const closeTab = useCallback((name: string) => {
    setTabs((t) => {
      const i = t.indexOf(name);
      const next = t.filter((x) => x !== name);
      setActive((a) => (a === name ? next[Math.min(i, next.length - 1)] ?? null : a));
      return next;
    });
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
    refreshAll(font?.masterId).catch(fail);
  }, [refreshAll, font?.masterId, fail]);

  const showAddGlyphs = useCallback(() => {
    if (font) backend.glyphSets().then(setAddGlyphSets, fail);
  }, [font, fail]);

  /** The glyphs a Glyph-menu action acts on: the open glyph tab, or the Font View selection. */
  const glyphTargets = active?.startsWith("g:") ? [active.slice(2)] : active === null ? fontSelection : [];

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

  /** Glyph › Create Composites: build glyphs from their parts (Aacute = A + acutecomb). */
  const createComposites = useCallback(async (names: string[]) => {
    const r = await backend.createComposites(names);
    setComposites(null);
    onStructureChanged(r.status);
    const skipped = r.skipped.length ? ` · ${tn(r.skipped.length, "{n} skipped (no recipe, missing parts or already drawn)", "{n} skipped (no recipe, missing parts or already drawn)")}` : "";
    setToast({ kind: "info", text: r.built.length ? `${tn(r.built.length, "Built {n} composite", "Built {n} composites")}${skipped}` : t("Nothing built: no recipes, missing parts, or the glyphs already have outlines.") });
  }, [onStructureChanged]);

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

  const openInfo = useCallback(() => openTab("info"), [openTab]);

  // Windows shortcuts (Ctrl, not Cmd).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "F1") {
        e.preventDefault();
        return openHandbook();
      }
      if (!e.ctrlKey || e.altKey) return;
      const k = e.key.toLowerCase();
      const inField = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if (k === "n" && !e.shiftKey) showNewFont();
      else if (k === "o" && !e.shiftKey) open();
      else if (k === "o") importSource(false);
      else if (k === "s" && e.shiftKey) saveAs();
      else if (k === "s") save();
      else if (k === "w" && !e.shiftKey) {
        if (active !== null) closeTab(active);
        else closeFont();
      } else if (k === "z" && !inField) undoRedo(e.shiftKey);
      else if (k === "y" && !inField) undoRedo(true);
      else if (k === "t" && !e.shiftKey && font) newTextTab();
      else if (k === "k" && e.shiftKey && font) openKerning();
      else if (k === "i" && !e.shiftKey && font) openInfo();
      else if (k === "e" && !e.shiftKey && font) setExporting(true);
      else if (k === "g" && e.shiftKey && font) showAddGlyphs();
      else if (k === "r" && e.shiftKey && active?.startsWith("g:")) sendEditCommand("correct-direction");
      else if (k === "m" && e.shiftKey) toggleMetrics();
      else if (k === "a" && e.shiftKey) toggleAnchors();
      else if (k === "u" && !e.shiftKey && !inField && font && glyphTargets.length) setAnchors(glyphTargets);
      else if (k === "delete" && !inField && font) removeGlyphs(glyphTargets);
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [showNewFont, open, importSource, save, saveAs, closeFont, closeTab, active, undoRedo, newTextTab, openKerning, openInfo, font, showAddGlyphs, removeGlyphs, glyphTargets, toggleMetrics, toggleAnchors, setAnchors, openHandbook]);

  const inGlyph = font !== null && active !== null && active.startsWith("g:");
  const tabLabel = (id: string) => (id.startsWith("g:") ? id.slice(2) : id === "kerning" ? t("Kerning") : id === "info" ? t("Font Info") : t("Text {n}", { n: id.slice(5) }));
  const glyphList = useMemo(() => (font ? font.glyphs.map((g) => ({ name: g.name, width: g.width })) : []), [font]);

  const menus: Menu[] = [
    {
      label: t("File"),
      items: [
        { label: t("New…"), shortcut: "Ctrl+N", onSelect: showNewFont },
        { label: t("Open…"), shortcut: "Ctrl+O", onSelect: open },
        { label: t("Open Sample Font"), onSelect: openSample },
        { label: t("Import…"), shortcut: "Ctrl+Shift+O", onSelect: () => importSource(false), separatorBefore: true },
        { label: t("Import Folder…"), onSelect: () => importSource(true) },
        { label: t("Save"), shortcut: "Ctrl+S", onSelect: font ? save : undefined, separatorBefore: true },
        { label: t("Save As…"), shortcut: "Ctrl+Shift+S", onSelect: font ? saveAs : undefined },
        { label: t("Close Font"), onSelect: font ? closeFont : undefined },
        { label: t("Export…"), shortcut: "Ctrl+E", onSelect: font ? () => setExporting(true) : undefined, separatorBefore: true },
        { label: t("Font Info…"), shortcut: "Ctrl+I", onSelect: font ? openInfo : undefined },
      ],
    },
    {
      label: t("Edit"),
      items: [
        { label: t("Undo"), shortcut: "Ctrl+Z", onSelect: status.canUndo ? () => undoRedo(false) : undefined },
        { label: t("Redo"), shortcut: "Ctrl+Y", onSelect: status.canRedo ? () => undoRedo(true) : undefined },
        { label: t("Cut"), shortcut: "Ctrl+X", onSelect: inGlyph ? () => sendEditCommand("cut") : undefined, separatorBefore: true },
        { label: t("Copy"), shortcut: "Ctrl+C", onSelect: inGlyph ? () => sendEditCommand("copy") : undefined },
        { label: t("Paste"), shortcut: "Ctrl+V", onSelect: inGlyph ? () => sendEditCommand("paste") : undefined },
      ],
    },
    {
      label: t("Glyph"),
      items: [
        { label: t("Add Glyphs…"), shortcut: "Ctrl+Shift+G", onSelect: font ? showAddGlyphs : undefined },
        { label: glyphTargets.length > 1 ? tn(glyphTargets.length, "Remove {n} Glyph", "Remove {n} Glyphs") : t("Remove Glyph"), shortcut: "Ctrl+Del", onSelect: font && glyphTargets.length ? () => removeGlyphs(glyphTargets) : undefined },
        { label: t("Add Component…"), shortcut: "Ctrl+Shift+C", onSelect: inGlyph ? () => sendEditCommand("add-component") : undefined, separatorBefore: true },
        { label: t("Set Anchors"), shortcut: "Ctrl+U", onSelect: font && glyphTargets.length ? () => setAnchors(glyphTargets) : undefined, separatorBefore: true },
        { label: t("Create Composites…"), onSelect: font ? () => setComposites({ initial: glyphTargets.join(" ") }) : undefined },
        { label: t("Update Metrics"), onSelect: font ? updateMetrics : undefined, separatorBefore: true },
      ],
    },
    {
      label: t("Path"),
      items: [
        { label: t("Correct Path Direction"), shortcut: "Ctrl+Shift+R", onSelect: inGlyph ? () => sendEditCommand("correct-direction") : undefined },
        { label: t("Add Extremes"), onSelect: inGlyph ? () => sendEditCommand("add-extremes") : undefined },
        { label: t("Remove Overlap"), onSelect: inGlyph ? () => sendEditCommand("remove-overlap") : undefined, separatorBefore: true },
      ],
    },
    { label: t("View"), items: [
      { label: t("Show Metrics"), shortcut: "Ctrl+Shift+M", onSelect: toggleMetrics, checked: showMetrics },
      { label: t("Show Anchors"), shortcut: "Ctrl+Shift+A", onSelect: toggleAnchors, checked: showAnchors },
      { label: t("Theme: System"), onSelect: () => setTheme("system"), checked: theme === "system", separatorBefore: true },
      { label: t("Theme: Light"), onSelect: () => setTheme("light"), checked: theme === "light" },
      { label: t("Theme: Dark"), onSelect: () => setTheme("dark"), checked: theme === "dark" },
      ...LANGUAGES.map((l, i) => ({ label: `${t("Language")}: ${l.name}`, onSelect: () => setLang(l.id), checked: lang === l.id, separatorBefore: i === 0 })),
    ] },
    {
      label: t("Window"),
      items: [
        { label: t("New Text Tab"), shortcut: "Ctrl+T", onSelect: font ? newTextTab : undefined },
        { label: t("Kerning"), shortcut: "Ctrl+Shift+K", onSelect: font ? openKerning : undefined },
      ],
    },
    {
      label: t("Help"),
      items: [
        { label: t("Kernest Handbook"), shortcut: "F1", onSelect: openHandbook },
        { label: t("About Kernest"), onSelect: () => setAbout(true), separatorBefore: true },
      ],
    },
  ];

  const title = font ? `${font.familyName}${font.path ? ".kernest" : ""}${status.dirty ? ` • ${t("Edited")}` : ""} — Kernest` : "Kernest";

  return (
    <div className="app">
      <TitleBar title={title} menus={menus} />
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
                  <button className="tab-close" aria-label={t("Close {name}", { name: tabLabel(id) })} onClick={(e) => { e.stopPropagation(); closeTab(id); }}>
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
            <button className="btn btn-secondary" onClick={openInfo} title={t("Font Info (Ctrl+I)")}>{t("Font Info")}</button>
            <button className="btn btn-primary" onClick={() => setExporting(true)} title={t("Export fonts (Ctrl+E)")}>{t("Export")}</button>
          </div>
          {active === null && <FontViewScreen key={font.familyName + font.path} font={font} onOpenGlyph={openGlyph} onSelectionChange={setFontSelection} onAddGlyphs={showAddGlyphs} onSetExport={(names, on) => backend.setGlyphExport(names, on).then(onStructureChanged, fail)} />}
          {tabs.map((id) => (
            <div key={id} className="tab-panel" hidden={active !== id}>
              {id.startsWith("g:") ? (
                <EditView glyph={id.slice(2)} masterId={layerOf[id.slice(2)] ?? font.masterId} masterKey={font.masterId} onPickLayer={(key) => setLayerOf((m) => ({ ...m, [id.slice(2)]: key === font.masterId ? undefined : key }))} version={versions[id.slice(2)] ?? 0} active={active === id} glyphs={glyphList} onCommitted={onCommitted} onError={fail} showMetrics={showMetrics} showAnchors={showAnchors} />
              ) : id === "info" ? (
                <FontInfoView version={docVersion} onChanged={onStructureChanged} onError={fail} />
              ) : id === "kerning" ? (
                <KerningView masterId={font.masterId} masterName={font.masters.find((m) => m.id === font.masterId)?.name ?? ""} version={docVersion} onChanged={onCommitted} onError={fail} />
              ) : (
                <TextView masterId={font.masterId} version={docVersion} active={active === id} onChanged={onCommitted} onError={fail} onOpenGlyph={openGlyph} />
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
        />
      )}
      {about && <AboutDialog onClose={() => setAbout(false)} />}
      {dropping && (
        <div className="drop-overlay" aria-hidden="true">
          <div className="drop-card k-subtitle">{t("Drop to open a .kernest font, or import .glyphs, .ufo or .designspace")}</div>
        </div>
      )}
      {importNotes && <ImportNotesDialog {...importNotes} onClose={() => setImportNotes(null)} />}
      {exporting && font && <ExportDialog fontPath={font.path} onClose={() => setExporting(false)} />}
      {glyphSets && (
        <NewFontDialog
          sets={glyphSets}
          onCancel={() => setGlyphSets(null)}
          onCreate={async (name, sets) => {
            loadDocument(await backend.newFont(name, sets));
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
      {toast && (
        <div className={`toast toast-${toast.kind} k-label`} role="status" onClick={() => setToast(null)}>
          {toast.text}
        </div>
      )}
    </div>
  );
}
