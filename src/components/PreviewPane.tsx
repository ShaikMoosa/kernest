import { useEffect, useMemo, useState } from "react";
import { backend, type FontInfo, type Location, type TextLayout } from "../lib/backend";
import { t } from "../lib/i18n";
import { visualOrder } from "../lib/bidi";
import { PREVIEW_SIZES, clampPreviewHeight, loadPreviewHeight, loadPreviewSettings, placeLine, savePreviewHeight, savePreviewSettings, type PreviewPlace, type PreviewSettings } from "../lib/previewPane";

/**
 * The preview pane docked under the Edit View, with a handle to drag its height
 * (also ↑/↓ on the focused handle). The height is remembered.
 */
export function PreviewDock(props: Props) {
  const [height, setHeight] = useState(loadPreviewHeight);
  const available = (el: Element | null) => (el?.parentElement?.clientHeight ?? 600);
  const startDrag = (e: React.PointerEvent<HTMLDivElement>) => {
    const handle = e.currentTarget;
    const dock = handle.parentElement;
    const y0 = e.clientY, h0 = height;
    handle.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => setHeight(clampPreviewHeight(h0 + (y0 - ev.clientY), available(dock)));
    const up = (ev: PointerEvent) => {
      handle.releasePointerCapture(ev.pointerId);
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", up);
      setHeight((h) => { savePreviewHeight(h); return h; });
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", up);
    e.preventDefault();
  };
  const onKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
    e.preventDefault();
    const step = (e.shiftKey ? 40 : 10) * (e.key === "ArrowUp" ? 1 : -1);
    const next = clampPreviewHeight(height + step, available(e.currentTarget.parentElement));
    setHeight(next);
    savePreviewHeight(next);
  };
  return (
    <div className="preview-dock" style={{ height }}>
      <div className="preview-resizer" role="separator" aria-orientation="horizontal" aria-valuenow={height} tabIndex={0}
        aria-label={t("Preview height")} title={t("Drag to resize the preview")} onPointerDown={startDrag} onKeyDown={onKey} />
      <PreviewPane {...props} />
    </div>
  );
}

interface Props {
  /** Sample text, with /glyphname for glyphs without a character. Empty draws nothing. */
  text: string;
  /** The master being edited: the preview follows it unless another source is picked. */
  masterId: string;
  /** Bumped after every change to the font, so the preview redraws. */
  version: number;
  /** Shows a text field to change the previewed text. */
  onTextChange?: (text: string) => void;
  onOpenGlyph?: (name: string) => void;
  /** Opens the preview in its own window (desktop only). */
  onOpenWindow?: () => void;
  onClose?: () => void;
  /** Extra controls at the end of the bar (the window's pin button). */
  extra?: React.ReactNode;
  /** Shown instead of the glyphs when there is no text to preview. */
  emptyText?: string;
  /** Where the pane is, so the dock and the window remember their own settings. */
  place?: PreviewPlace;
}

const LINE_GAP = 300;

/**
 * SMO-36 · One line of glyphs at a master or an instance: under the Edit View, and
 * in the separate preview window. The source, size and colours are remembered.
 */
export function PreviewPane({ text, masterId, version, onTextChange, onOpenGlyph, onOpenWindow, onClose, extra, emptyText, place = "pane" }: Props) {
  const [settings, setSettings] = useState<PreviewSettings>(() => loadPreviewSettings(place));
  const update = (patch: Partial<PreviewSettings>) => setSettings((s) => { const next = { ...s, ...patch }; savePreviewSettings(next, place); return next; });
  const [info, setInfo] = useState<FontInfo | null>(null);
  const [layout, setLayout] = useState<TextLayout | null>(null);
  useEffect(() => { backend.fontInfo().then(setInfo, () => setInfo(null)); }, [version]);

  // A picked master or instance that no longer exists falls back to the edited master.
  const source = settings.source;
  const target = useMemo((): { master: string; location?: Location } => {
    if (source.startsWith("m:") && info?.masters.some((m) => m.id === source.slice(2))) return { master: source.slice(2) };
    if (source.startsWith("i:")) {
      const inst = info?.instances.find((i) => i.name === source.slice(2));
      if (inst) return { master: masterId, location: inst.location };
    }
    return { master: masterId };
  }, [source, info, masterId]);

  useEffect(() => {
    if (!text.trim()) { setLayout(null); return; }
    let live = true;
    const id = setTimeout(() => {
      backend.textLayout(text, target.master, target.location).then((l) => { if (live) setLayout(l); }, () => { if (live) setLayout(null); });
    }, 40);
    return () => { live = false; clearTimeout(id); };
  }, [text, target, version]);

  const sc = settings.size / 1000;
  const asc = layout?.metrics.ascender ?? 800;
  const desc = layout?.metrics.descender ?? -200;
  const drawn = useMemo(() => {
    if (!layout) return { paths: [] as { key: string; d: string; x: number; name: string }[], width: 0 };
    const paths: { key: string; d: string; x: number; name: string }[] = [];
    let offset = 0;
    layout.lines.forEach((line, li) => {
      const items = line.map((it) => (it.type === "glyph" ? { width: it.width, kern: it.kern, rtl: it.rtl } : { width: 500, kern: 0, rtl: it.rtl }));
      // The paragraph direction follows the first glyph, as in the text tab.
      const order = visualOrder(line.map((it) => it.rtl), line.find((it) => it.type === "glyph")?.rtl ?? false);
      const placed = placeLine(items, order, sc);
      line.forEach((it, i) => {
        if (it.type === "glyph" && it.svg) paths.push({ key: `${li}:${i}`, d: it.svg, x: offset + placed.x[i], name: it.name });
      });
      offset += placed.width + LINE_GAP * sc;
    });
    return { paths, width: Math.max(0, offset - LINE_GAP * sc) };
  }, [layout, sc]);

  const height = Math.ceil((asc - desc) * sc) + 8;
  const sourceLabel = (v: string) => (v === "" ? t("Master being edited") : v.startsWith("m:") ? info?.masters.find((m) => m.id === v.slice(2))?.name ?? v : t("Instance: {name}", { name: v.slice(2) }));

  return (
    <section className={`preview-pane${settings.invert ? " is-inverted" : ""}`} aria-label={t("Preview")}>
      <div className="preview-pane-bar">
        {onTextChange && (
          <input className="text-input preview-pane-text" value={text} dir="auto" spellCheck={false} aria-label={t("Preview text")}
            placeholder={t("Type text, or /glyphname")} onChange={(e) => onTextChange(e.target.value)} />
        )}
        <select className="select" aria-label={t("Preview master or instance")} value={source} title={sourceLabel(source)} onChange={(e) => update({ source: e.target.value })}>
          <option value="">{t("Master being edited")}</option>
          {(info?.masters.length ?? 0) > 1 && <optgroup label={t("Masters")}>
            {info?.masters.map((m) => <option key={m.id} value={`m:${m.id}`}>{m.name}</option>)}
          </optgroup>}
          {(info?.instances.length ?? 0) > 0 && <optgroup label={t("Instances")}>
            {info?.instances.map((i) => <option key={i.name} value={`i:${i.name}`}>{i.name}</option>)}
          </optgroup>}
        </select>
        <select className="select" aria-label={t("Preview size")} value={settings.size} onChange={(e) => update({ size: Number(e.target.value) })}>
          {(PREVIEW_SIZES.includes(settings.size) ? PREVIEW_SIZES : [...PREVIEW_SIZES, settings.size].sort((a, b) => a - b)).map((s) => <option key={s} value={s}>{s} px</option>)}
        </select>
        <button type="button" className={`btn btn-secondary btn-sm${settings.invert ? " is-on" : ""}`} aria-pressed={settings.invert}
          title={t("White on black")} onClick={() => update({ invert: !settings.invert })}>{t("Invert")}</button>
        <span className="spacer" />
        {extra}
        {onOpenWindow && <button type="button" className="btn btn-secondary btn-sm" onClick={onOpenWindow} title={t("Show the preview in its own window, for example on a second display")}>{t("Open in Window")}</button>}
        {onClose && (
          <button type="button" className="icon-btn" aria-label={t("Hide preview")} title={t("Hide preview")} onClick={onClose}>
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round"><path d="M6 6l12 12M18 6L6 18" /></svg>
          </button>
        )}
      </div>
      <div className="preview-pane-body">
        {drawn.paths.length === 0 ? (
          <p className="k-caption muted preview-pane-empty">{text.trim() ? t("Nothing drawn here yet") : emptyText ?? t("Type text to preview it.")}</p>
        ) : (
          <svg width={Math.max(1, drawn.width) + 32} height={height} aria-label={t("Preview of {text}", { text })}
            onDoubleClick={(e) => { const n = (e.target as Element).getAttribute("data-glyph"); if (n) onOpenGlyph?.(n); }}>
            {drawn.paths.map((p) => (
              <path key={p.key} d={p.d} data-glyph={p.name} transform={`translate(${16 + p.x} ${asc * sc + 4}) scale(${sc} ${-sc})`}>
                <title>{p.name}</title>
              </path>
            ))}
          </svg>
        )}
      </div>
    </section>
  );
}
