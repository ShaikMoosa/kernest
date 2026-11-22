import { useEffect, useState } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { PreviewPane } from "./components/PreviewPane";
import { t } from "./lib/i18n";
import { PREVIEW_EVENTS, type PreviewSource } from "./lib/previewEvents";

/**
 * SMO-36 · The separate preview window (Window › Preview Window). It mirrors the
 * main window's current tab: the text of a text tab, or the preview text of a
 * glyph tab. Other tabs, and a closed font, leave it empty.
 */
export function PreviewWindow() {
  const [src, setSrc] = useState<PreviewSource | null>(null);
  const [pinned, setPinned] = useState(false);

  useEffect(() => {
    let off: (() => void) | undefined;
    let live = true;
    listen<PreviewSource | null>(PREVIEW_EVENTS.source, (e) => setSrc(e.payload)).then((u) => {
      if (live) { off = u; emit(PREVIEW_EVENTS.ready); } else u();
    });
    return () => { live = false; off?.(); };
  }, []);

  useEffect(() => { document.title = src?.title ? `${t("Preview")} · ${src.title}` : t("Preview"); }, [src]);

  const togglePin = () => {
    const next = !pinned;
    getCurrentWindow().setAlwaysOnTop(next).then(() => setPinned(next), () => {});
  };

  return (
    <div className="preview-window">
      <PreviewPane
        place="window"
        text={src?.text ?? ""}
        masterId={src?.masterId ?? ""}
        version={src?.version ?? 0}
        onOpenGlyph={(name) => emit(PREVIEW_EVENTS.openGlyph, name)}
        emptyText={t("Open a text tab or a glyph in the main window to preview it here.")}
        extra={
          <button type="button" className={`btn btn-secondary btn-sm${pinned ? " is-on" : ""}`} aria-pressed={pinned} onClick={togglePin}
            title={t("Keep this window in front of the main window")}>{t("Keep on Top")}</button>
        }
      />
    </div>
  );
}
