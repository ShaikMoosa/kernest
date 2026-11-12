import { useCallback, useEffect, useState } from "react";
import { backend, errorMessage, type FontView, type GlyphSetInfo } from "./lib/backend";
import { forgetRecent, loadRecents, pickFontToOpen, pickSavePath, rememberRecent, type RecentFont } from "./lib/desktop";
import { TitleBar, type Menu } from "./components/TitleBar";
import { StartWindow } from "./components/StartWindow";
import { NewFontDialog } from "./components/NewFontDialog";
import { FontViewScreen } from "./components/FontViewScreen";

const later = (label: string, shortcut?: string) => ({ label, shortcut, disabled: true });

export default function App() {
  const [font, setFont] = useState<FontView | null>(null);
  const [recents, setRecents] = useState<RecentFont[]>(() => loadRecents());
  const [glyphSets, setGlyphSets] = useState<GlyphSetInfo[] | null>(null);
  const [toast, setToast] = useState<{ kind: "info" | "danger"; text: string } | null>(null);

  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), toast.kind === "danger" ? 8000 : 3000);
    return () => clearTimeout(t);
  }, [toast]);

  const fail = (e: unknown) => setToast({ kind: "danger", text: errorMessage(e) });

  const showNewFont = useCallback(() => {
    backend.glyphSets().then(setGlyphSets, fail);
  }, []);

  const openPath = useCallback(async (path: string) => {
    try {
      const view = await backend.openFont(path);
      setFont(view);
      setRecents(rememberRecent(view.path ?? path, view.familyName));
    } catch (e) {
      fail(e);
    }
  }, []);

  const open = useCallback(async () => {
    const path = await pickFontToOpen();
    if (path) await openPath(path);
    else if (!("__TAURI_INTERNALS__" in window)) fail("Opening files needs the desktop app (npm run tauri dev).");
  }, [openPath]);

  const openSample = useCallback(() => backend.openSample().then(setFont, fail), []);

  const saveAs = useCallback(async () => {
    if (!font) return;
    try {
      const path = await pickSavePath(font.familyName);
      if (!path) return;
      const saved = await backend.saveFont(path);
      setFont({ ...font, path: saved });
      setRecents(rememberRecent(saved, font.familyName));
      setToast({ kind: "info", text: `Saved to ${saved}` });
    } catch (e) {
      fail(e);
    }
  }, [font]);

  const save = useCallback(async () => {
    if (!font) return;
    if (!font.path) return saveAs();
    try {
      const saved = await backend.saveFont();
      setToast({ kind: "info", text: `Saved ${font.familyName}` });
      setRecents(rememberRecent(saved, font.familyName));
    } catch (e) {
      fail(e);
    }
  }, [font, saveAs]);

  const closeFont = useCallback(() => setFont(null), []);

  // Windows shortcuts (Ctrl, not Cmd).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.ctrlKey || e.altKey) return;
      const k = e.key.toLowerCase();
      if (k === "n" && !e.shiftKey) showNewFont();
      else if (k === "o" && !e.shiftKey) open();
      else if (k === "s" && e.shiftKey) saveAs();
      else if (k === "s") save();
      else if (k === "w" && !e.shiftKey) closeFont();
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [showNewFont, open, save, saveAs, closeFont]);

  const menus: Menu[] = [
    {
      label: "File",
      items: [
        { label: "New…", shortcut: "Ctrl+N", onSelect: showNewFont },
        { label: "Open…", shortcut: "Ctrl+O", onSelect: open },
        { label: "Open Sample Font", onSelect: openSample },
        { label: "Save", shortcut: "Ctrl+S", onSelect: font ? save : undefined, separatorBefore: true },
        { label: "Save As…", shortcut: "Ctrl+Shift+S", onSelect: font ? saveAs : undefined },
        { label: "Close", shortcut: "Ctrl+W", onSelect: font ? closeFont : undefined },
        { ...later("Export…", "Ctrl+E"), separatorBefore: true },
        { ...later("Font Info…", "Ctrl+I") },
      ],
    },
    { label: "Edit", items: [later("Undo", "Ctrl+Z"), later("Redo", "Ctrl+Y"), { ...later("Cut", "Ctrl+X"), separatorBefore: true }, later("Copy", "Ctrl+C"), later("Paste", "Ctrl+V"), later("Select All", "Ctrl+A")] },
    { label: "Glyph", items: [later("Add Glyphs…", "Ctrl+Shift+G"), later("Remove Glyph", "Ctrl+Del")] },
    { label: "Path", items: [later("Correct Path Direction", "Ctrl+Shift+R"), later("Add Extremes")] },
    { label: "View", items: [later("Show Metrics", "Ctrl+Shift+M")] },
    { label: "Window", items: [later("Kerning", "Ctrl+Shift+K")] },
    { label: "Help", items: [later("Kernest Handbook", "F1"), later("About Kernest")] },
  ];

  const title = font ? `${font.familyName}${font.path ? ".kernest" : ""} — Kernest` : "Kernest";

  return (
    <div className="app">
      <TitleBar title={title} menus={menus} />
      {font ? (
        <>
          <div className="commandbar">
            <div className="doc-tab is-selected">
              <span className="k-label">Font</span>
            </div>
            <span className="spacer" />
            <div className="segmented" role="radiogroup" aria-label="Master">
              {font.masters.map((m) => (
                <button key={m.id} role="radio" aria-checked={m.id === font.masterId} className={`segment${m.id === font.masterId ? " is-selected" : ""}`}
                  onClick={() => backend.fontView(m.id).then(setFont, fail)}>
                  {m.name}
                </button>
              ))}
            </div>
            <button className="btn btn-secondary" disabled title="Coming in v1">Font Info</button>
            <button className="btn btn-primary" disabled title="Coming in v1">Export</button>
          </div>
          <FontViewScreen key={font.familyName + font.path} font={font} />
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
            setFont(await backend.newFont(name, sets));
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
