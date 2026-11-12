import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { backend, errorMessage, isTauri, type DocStatus, type FontView, type GlyphSetInfo } from "./lib/backend";
import { forgetRecent, loadRecents, pickFontToOpen, pickSavePath, rememberRecent, type RecentFont } from "./lib/desktop";
import { TitleBar, type Menu } from "./components/TitleBar";
import { StartWindow } from "./components/StartWindow";
import { NewFontDialog } from "./components/NewFontDialog";
import { FontViewScreen } from "./components/FontViewScreen";
import { EditView, sendEditCommand } from "./components/EditView";
import { AddIcon, CloseIcon } from "./components/Icons";
import { TextView } from "./components/TextView";
import { KerningView } from "./components/KerningView";
import { FontInfoView } from "./components/FontInfoView";

const later = (label: string, shortcut?: string) => ({ label, shortcut, disabled: true });
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
  const [toast, setToast] = useState<{ kind: "info" | "danger"; text: string } | null>(null);
  const dirtyRef = useRef(false);
  dirtyRef.current = status.dirty;

  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), toast.kind === "danger" ? 8000 : 3000);
    return () => clearTimeout(t);
  }, [toast]);

  const fail = useCallback((e: unknown) => setToast({ kind: "danger", text: errorMessage(e) }), []);

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

  // The Font View is refreshed lazily when it becomes visible after edits.
  useEffect(() => {
    if (font && active === null && fontStale) backend.fontView(font.masterId).then(show, fail);
  }, [active, fontStale, font, show, fail]);

  // Bumped on every change, so text and kerning tabs refresh.
  const [docVersion, setDocVersion] = useState(0);

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
    setVersions((v) => {
      const next = { ...v };
      for (const id of tabsRef.current) if (id.startsWith("g:")) next[id.slice(2)] = (next[id.slice(2)] ?? 0) + 1;
      return next;
    });
  }, [show]);
  const tabsRef = useRef<string[]>([]);
  tabsRef.current = tabs;

  const onStructureChanged = useCallback((s: DocStatus) => {
    setStatus(s);
    refreshAll(font?.masterId).catch(fail);
  }, [refreshAll, font?.masterId, fail]);

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
      if (!e.ctrlKey || e.altKey) return;
      const k = e.key.toLowerCase();
      const inField = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if (k === "n" && !e.shiftKey) showNewFont();
      else if (k === "o" && !e.shiftKey) open();
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
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [showNewFont, open, save, saveAs, closeFont, closeTab, active, undoRedo, newTextTab, openKerning, openInfo, font]);

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
        { label: "Save", shortcut: "Ctrl+S", onSelect: font ? save : undefined, separatorBefore: true },
        { label: "Save As…", shortcut: "Ctrl+Shift+S", onSelect: font ? saveAs : undefined },
        { label: "Close Font", onSelect: font ? closeFont : undefined },
        { ...later("Export…", "Ctrl+E"), separatorBefore: true },
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
        later("Add Glyphs…", "Ctrl+Shift+G"),
        later("Remove Glyph", "Ctrl+Del"),
        { label: "Add Component…", shortcut: "Ctrl+Shift+C", onSelect: inGlyph ? () => sendEditCommand("add-component") : undefined, separatorBefore: true },
      ],
    },
    { label: "Path", items: [later("Correct Path Direction", "Ctrl+Shift+R"), later("Add Extremes")] },
    { label: "View", items: [later("Show Metrics", "Ctrl+Shift+M")] },
    {
      label: "Window",
      items: [
        { label: "New Text Tab", shortcut: "Ctrl+T", onSelect: font ? newTextTab : undefined },
        { label: "Kerning", shortcut: "Ctrl+Shift+K", onSelect: font ? openKerning : undefined },
      ],
    },
    { label: "Help", items: [later("Kernest Handbook", "F1"), later("About Kernest")] },
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
            <button className="btn btn-primary" disabled title="Coming in v1">Export</button>
          </div>
          {active === null && <FontViewScreen key={font.familyName + font.path} font={font} onOpenGlyph={openGlyph} />}
          {tabs.map((id) => (
            <div key={id} className="tab-panel" hidden={active !== id}>
              {id.startsWith("g:") ? (
                <EditView glyph={id.slice(2)} masterId={font.masterId} version={versions[id.slice(2)] ?? 0} active={active === id} glyphs={glyphList} onCommitted={onCommitted} onError={fail} />
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
        />
      )}
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
      {toast && (
        <div className={`toast toast-${toast.kind} k-label`} role="status" onClick={() => setToast(null)}>
          {toast.text}
        </div>
      )}
    </div>
  );
}
