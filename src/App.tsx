import { useCallback, useEffect, useRef, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { backend, errorMessage, isTauri, type DocStatus, type FontView, type GlyphSetInfo } from "./lib/backend";
import { forgetRecent, loadRecents, pickFontToOpen, pickSavePath, rememberRecent, type RecentFont } from "./lib/desktop";
import { TitleBar, type Menu } from "./components/TitleBar";
import { StartWindow } from "./components/StartWindow";
import { NewFontDialog } from "./components/NewFontDialog";
import { FontViewScreen } from "./components/FontViewScreen";
import { EditView } from "./components/EditView";
import { CloseIcon } from "./components/Icons";

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

  // ---------- Glyph tabs ----------

  const openGlyph = useCallback((name: string) => {
    setTabs((t) => (t.includes(name) ? t : [...t, name]));
    setActive(name);
  }, []);

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

  const onCommitted = useCallback((s: DocStatus) => {
    setStatus(s);
    setFontStale(true);
  }, []);

  const undoRedo = useCallback(async (redo: boolean) => {
    if (!font) return;
    try {
      const r = redo ? await backend.redo() : await backend.undo();
      if (!r) return;
      setStatus(r.status);
      setFontStale(true);
      setVersions((v) => ({ ...v, [r.glyph]: (v[r.glyph] ?? 0) + 1 }));
      if (active === null) backend.fontView(font.masterId).then(show, fail);
    } catch (e) {
      fail(e);
    }
  }, [font, active, show, fail]);

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
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [showNewFont, open, save, saveAs, closeFont, closeTab, active, undoRedo]);

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
        { ...later("Font Info…", "Ctrl+I") },
      ],
    },
    {
      label: "Edit",
      items: [
        { label: "Undo", shortcut: "Ctrl+Z", onSelect: status.canUndo ? () => undoRedo(false) : undefined },
        { label: "Redo", shortcut: "Ctrl+Y", onSelect: status.canRedo ? () => undoRedo(true) : undefined },
        { ...later("Cut", "Ctrl+X"), separatorBefore: true },
        later("Copy", "Ctrl+C"),
        later("Paste", "Ctrl+V"),
      ],
    },
    { label: "Glyph", items: [later("Add Glyphs…", "Ctrl+Shift+G"), later("Remove Glyph", "Ctrl+Del")] },
    { label: "Path", items: [later("Correct Path Direction", "Ctrl+Shift+R"), later("Add Extremes")] },
    { label: "View", items: [later("Show Metrics", "Ctrl+Shift+M")] },
    { label: "Window", items: [later("Kerning", "Ctrl+Shift+K")] },
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
              {tabs.map((g) => (
                <div key={g} role="tab" aria-selected={active === g} className={`doc-tab${active === g ? " is-selected" : ""}`} onClick={() => setActive(g)}>
                  <span className="k-label">{g}</span>
                  <button className="tab-close" aria-label={`Close ${g}`} onClick={(e) => { e.stopPropagation(); closeTab(g); }}>
                    <CloseIcon size={12} />
                  </button>
                </div>
              ))}
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
            <button className="btn btn-secondary" disabled title="Coming in v1">Font Info</button>
            <button className="btn btn-primary" disabled title="Coming in v1">Export</button>
          </div>
          {active === null && <FontViewScreen key={font.familyName + font.path} font={font} onOpenGlyph={openGlyph} />}
          {tabs.map((g) => (
            <div key={g} className="tab-panel" hidden={active !== g}>
              <EditView glyph={g} masterId={font.masterId} version={versions[g] ?? 0} active={active === g} onCommitted={onCommitted} onError={fail} />
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
