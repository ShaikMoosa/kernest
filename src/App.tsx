import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { listen } from "@tauri-apps/api/event";
import { backend, errorMessage, isTauri, type DocStatus, type FontView, type GlyphSetInfo } from "./lib/backend";
import { classifyPath, forgetRecent, kernestRoot, HANDBOOK_URL, loadRecents, openExternal, pickFontToOpen, pickSavePath, pickSourceToImport, rememberRecent, type RecentFont } from "./lib/desktop";
import { TitleBar, type Menu } from "./components/TitleBar";
import { StartWindow } from "./components/StartWindow";
import { NewFontDialog } from "./components/NewFontDialog";
import { AddGlyphsDialog } from "./components/AddGlyphsDialog";
import { FontViewScreen } from "./components/FontViewScreen";
import { EditView, sendEditCommand } from "./components/EditView";
import { AddIcon, CloseIcon } from "./components/Icons";
import { TextView } from "./components/TextView";
import { KerningView } from "./components/KerningView";
import { FontInfoView } from "./components/FontInfoView";
import { ExportDialog } from "./components/ExportDialog";
import { ImportNotesDialog } from "./components/ImportNotesDialog";
import { AboutDialog } from "./components/AboutDialog";

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
  const [showMetrics, setShowMetrics] = useState(() => {
    try { return localStorage.getItem("kernest.showMetrics") !== "0"; } catch { return true; }
  });
  const toggleMetrics = useCallback(() => setShowMetrics((v) => {
    try { localStorage.setItem("kernest.showMetrics", v ? "0" : "1"); } catch { /* not persisted */ }
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
    const msg = "You have unsaved changes. Discard them?";
    return isTauri ? ask(msg, { title: "Kernest", kind: "warning", okLabel: "Discard", cancelLabel: "Cancel" }) : window.confirm(msg);
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
    if (!isTauri) return fail("Opening files needs the desktop app (npm run tauri dev).");
    if (!(await confirmDiscard())) return;
    const path = await pickFontToOpen();
    if (path) await openPath(path, true);
  }, [openPath, confirmDiscard, fail]);

  const importPath = useCallback(async (path: string, confirmed = false) => {
    if (!confirmed && !(await confirmDiscard())) return;
    setToast({ kind: "info", text: "Importing…" });
    try {
      const r = await backend.importFont(path);
      loadDocument(r.view);
      if (r.warnings.length > 0) {
        setToast(null);
        setImportNotes({ source: r.source, familyName: r.view.familyName, warnings: r.warnings });
      } else {
        setToast({ kind: "info", text: `Imported ${r.view.familyName}. Use Save As to keep it as a .kernest font.` });
      }
    } catch (e) {
      fail(e);
    }
  }, [confirmDiscard, loadDocument, fail]);

  const importSource = useCallback(async (folder = false) => {
    if (!isTauri) return fail("Importing fonts needs the desktop app (npm run tauri dev).");
    if (!(await confirmDiscard())) return;
    const path = await pickSourceToImport(folder);
    if (path) await importPath(path, true);
  }, [confirmDiscard, importPath, fail]);

  /** Opens a .kernest font or imports a source, from Explorer or a drop on the window. */
  const openAny = useCallback(async (path: string) => {
    const kind = classifyPath(path);
    if (kind === "kernest") return openPath(kernestRoot(path));
    if (kind === "source") return importPath(path);
    fail("Kernest opens .kernest fonts and imports .glyphs, .glyphspackage, .ufo and .designspace sources.");
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
      setToast({ kind: "info", text: `Saved to ${saved}` });
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
      setToast({ kind: "info", text: `Saved ${font.familyName}` });
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

  /** The glyphs Remove Glyph acts on: the open glyph tab, or the Font View selection. */
  const removalTargets = active?.startsWith("g:") ? [active.slice(2)] : active === null ? fontSelection : [];

  const removeGlyphs = useCallback(async (names: string[]) => {
    if (!font || names.length === 0) return;
    const shown = names.slice(0, 6).join(", ") + (names.length > 6 ? ` and ${names.length - 6} more` : "");
    const msg = `Remove ${names.length === 1 ? "the glyph" : `${names.length} glyphs`} ${shown}?

Components that use ${names.length === 1 ? "it" : "them"} are turned into outlines in other glyphs. You can undo this with Ctrl+Z.`;
    const ok = isTauri ? await ask(msg, { title: "Remove Glyph", kind: "warning", okLabel: "Remove", cancelLabel: "Cancel" }) : window.confirm(msg);
    if (!ok) return;
    try {
      const s = await backend.removeGlyphs(names);
      setFontSelection([]);
      onStructureChanged(s);
      setToast({ kind: "info", text: names.length === 1 ? `Removed ${names[0]}` : `Removed ${names.length} glyphs` });
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
        ? { kind: "danger", text: `Metrics updated; ${r.problems.length} key${r.problems.length === 1 ? "" : "s"} couldn't be applied: ${r.problems.slice(0, 3).join("; ")}` }
        : { kind: "info", text: "Metrics updated" });
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
      else if (k === "delete" && !inField && font) removeGlyphs(removalTargets);
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [showNewFont, open, importSource, save, saveAs, closeFont, closeTab, active, undoRedo, newTextTab, openKerning, openInfo, font, showAddGlyphs, removeGlyphs, removalTargets, toggleMetrics, openHandbook]);

  const inGlyph = font !== null && active !== null && active.startsWith("g:");
  const tabLabel = (id: string) => (id.startsWith("g:") ? id.slice(2) : id === "kerning" ? "Kerning" : id === "info" ? "Font Info" : `Text ${id.slice(5)}`);
  const glyphList = useMemo(() => (font ? font.glyphs.map((g) => ({ name: g.name, width: g.width })) : []), [font]);

  const menus: Menu[] = [
    {
      label: "File",
      items: [
        { label: "New…", shortcut: "Ctrl+N", onSelect: showNewFont },
        { label: "Open…", shortcut: "Ctrl+O", onSelect: open },
        { label: "Open Sample Font", onSelect: openSample },
        { label: "Import…", shortcut: "Ctrl+Shift+O", onSelect: () => importSource(false), separatorBefore: true },
        { label: "Import Folder…", onSelect: () => importSource(true) },
        { label: "Save", shortcut: "Ctrl+S", onSelect: font ? save : undefined, separatorBefore: true },
        { label: "Save As…", shortcut: "Ctrl+Shift+S", onSelect: font ? saveAs : undefined },
        { label: "Close Font", onSelect: font ? closeFont : undefined },
        { label: "Export…", shortcut: "Ctrl+E", onSelect: font ? () => setExporting(true) : undefined, separatorBefore: true },
        { label: "Font Info…", shortcut: "Ctrl+I", onSelect: font ? openInfo : undefined },
      ],
    },
    {
      label: "Edit",
      items: [
        { label: "Undo", shortcut: "Ctrl+Z", onSelect: status.canUndo ? () => undoRedo(false) : undefined },
        { label: "Redo", shortcut: "Ctrl+Y", onSelect: status.canRedo ? () => undoRedo(true) : undefined },
        { label: "Cut", shortcut: "Ctrl+X", onSelect: inGlyph ? () => sendEditCommand("cut") : undefined, separatorBefore: true },
        { label: "Copy", shortcut: "Ctrl+C", onSelect: inGlyph ? () => sendEditCommand("copy") : undefined },
        { label: "Paste", shortcut: "Ctrl+V", onSelect: inGlyph ? () => sendEditCommand("paste") : undefined },
      ],
    },
    {
      label: "Glyph",
      items: [
        { label: "Add Glyphs…", shortcut: "Ctrl+Shift+G", onSelect: font ? showAddGlyphs : undefined },
        { label: removalTargets.length > 1 ? `Remove ${removalTargets.length} Glyphs` : "Remove Glyph", shortcut: "Ctrl+Del", onSelect: font && removalTargets.length ? () => removeGlyphs(removalTargets) : undefined },
        { label: "Add Component…", shortcut: "Ctrl+Shift+C", onSelect: inGlyph ? () => sendEditCommand("add-component") : undefined, separatorBefore: true },
        { label: "Update Metrics", onSelect: font ? updateMetrics : undefined, separatorBefore: true },
      ],
    },
    {
      label: "Path",
      items: [
        { label: "Correct Path Direction", shortcut: "Ctrl+Shift+R", onSelect: inGlyph ? () => sendEditCommand("correct-direction") : undefined },
        { label: "Add Extremes", onSelect: inGlyph ? () => sendEditCommand("add-extremes") : undefined },
        { label: "Remove Overlap", onSelect: inGlyph ? () => sendEditCommand("remove-overlap") : undefined, separatorBefore: true },
      ],
    },
    { label: "View", items: [{ label: "Show Metrics", shortcut: "Ctrl+Shift+M", onSelect: toggleMetrics, checked: showMetrics }] },
    {
      label: "Window",
      items: [
        { label: "New Text Tab", shortcut: "Ctrl+T", onSelect: font ? newTextTab : undefined },
        { label: "Kerning", shortcut: "Ctrl+Shift+K", onSelect: font ? openKerning : undefined },
      ],
    },
    {
      label: "Help",
      items: [
        { label: "Kernest Handbook", shortcut: "F1", onSelect: openHandbook },
        { label: "About Kernest", onSelect: () => setAbout(true), separatorBefore: true },
      ],
    },
  ];

  const title = font ? `${font.familyName}${font.path ? ".kernest" : ""}${status.dirty ? " • Edited" : ""} — Kernest` : "Kernest";

  return (
    <div className="app">
      <TitleBar title={title} menus={menus} />
      {font ? (
        <>
          <div className="commandbar">
            <div className="doc-tabs" role="tablist" aria-label="Open tabs">
              <button role="tab" aria-selected={active === null} className={`doc-tab${active === null ? " is-selected" : ""}`} onClick={() => setActive(null)}>
                <span className="k-label">Font</span>
              </button>
              {tabs.map((id) => (
                <div key={id} role="tab" aria-selected={active === id} className={`doc-tab${active === id ? " is-selected" : ""}${id.startsWith("g:") ? "" : " is-tool"}`} onClick={() => setActive(id)}>
                  <span className="k-label">{tabLabel(id)}</span>
                  <button className="tab-close" aria-label={`Close ${tabLabel(id)}`} onClick={(e) => { e.stopPropagation(); closeTab(id); }}>
                    <CloseIcon size={12} />
                  </button>
                </div>
              ))}
              <button className="icon-btn" title="New Text Tab (Ctrl+T)" aria-label="New Text Tab" onClick={newTextTab}><AddIcon size={14} /></button>
            </div>
            <span className="spacer" />
            <div className="segmented" role="radiogroup" aria-label="Master">
              {font.masters.map((m) => (
                <button key={m.id} role="radio" aria-checked={m.id === font.masterId} className={`segment${m.id === font.masterId ? " is-selected" : ""}`}
                  onClick={() => backend.fontView(m.id).then(show, fail)}>
                  {m.name}
                </button>
              ))}
            </div>
            <button className="btn btn-secondary" onClick={openInfo} title="Font Info (Ctrl+I)">Font Info</button>
            <button className="btn btn-primary" onClick={() => setExporting(true)} title="Export fonts (Ctrl+E)">Export</button>
          </div>
          {active === null && <FontViewScreen key={font.familyName + font.path} font={font} onOpenGlyph={openGlyph} onSelectionChange={setFontSelection} onAddGlyphs={showAddGlyphs} />}
          {tabs.map((id) => (
            <div key={id} className="tab-panel" hidden={active !== id}>
              {id.startsWith("g:") ? (
                <EditView glyph={id.slice(2)} masterId={layerOf[id.slice(2)] ?? font.masterId} masterKey={font.masterId} onPickLayer={(key) => setLayerOf((m) => ({ ...m, [id.slice(2)]: key === font.masterId ? undefined : key }))} version={versions[id.slice(2)] ?? 0} active={active === id} glyphs={glyphList} onCommitted={onCommitted} onError={fail} showMetrics={showMetrics} />
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
          <div className="drop-card k-subtitle">Drop to open a .kernest font, or import .glyphs, .ufo or .designspace</div>
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
          onAdd={async (text, sets) => {
            const r = await backend.addGlyphs(text, sets);
            setAddGlyphSets(null);
            onStructureChanged(r.status);
            const skipped = r.skipped.length ? ` · ${r.skipped.length} already in the font` : "";
            setToast({ kind: "info", text: r.added.length ? `Added ${r.added.length} glyph${r.added.length === 1 ? "" : "s"}${skipped}` : `Nothing added: all ${r.skipped.length} already in the font` });
          }}
        />
      )}
      {toast && (
        <div className={`toast toast-${toast.kind} k-label`} role="status" onClick={() => setToast(null)}>
          {toast.text}
        </div>
      )}
    </div>
  );
}
