import { useEffect, useState } from "react";
import { backend, type Axis, type Location, type TextLayout } from "../lib/backend";
import { t } from "../lib/i18n";

/**
 * S09 · Edit View sidebar: the glyph drawn at any point of the design space, with a
 * slider per axis and the nearest instance. Only shown for fonts with axes and masters.
 */
export function VariationPreview({ glyph, masterId, version }: { glyph: string; masterId: string; version: number }) {
  const [axes, setAxes] = useState<Axis[]>([]);
  const [instances, setInstances] = useState<{ name: string; location: Location }[]>([]);
  const [masters, setMasters] = useState(0);
  const [location, setLocation] = useState<Location>({});
  const [layout, setLayout] = useState<TextLayout | null>(null);
  const [open, setOpen] = useState(true);

  useEffect(() => {
    let live = true;
    backend.fontInfo().then((info) => {
      if (!live) return;
      setAxes(info.axes);
      setInstances(info.instances);
      setMasters(info.masters.length);
      setLocation((old) => Object.fromEntries(info.axes.map((a) => [a.tag, old[a.tag] ?? a.default])));
    }, () => {});
    return () => { live = false; };
  }, [version]);

  const usable = axes.length > 0 && masters > 1;
  useEffect(() => {
    if (!usable || !open) return;
    let live = true;
    const id = setTimeout(() => {
      backend.textLayout(`/${glyph}`, masterId, location).then((l) => { if (live) setLayout(l); }, () => { if (live) setLayout(null); });
    }, 60);
    return () => { live = false; clearTimeout(id); };
  }, [usable, open, glyph, masterId, location, version]);

  if (!usable) return null;
  const item = layout?.lines[0]?.[0];
  const asc = layout?.metrics.ascender ?? 800, desc = layout?.metrics.descender ?? -200;
  const h = asc - desc;
  // The instance closest to the sliders, measured across the axes' ranges.
  const nearest = instances.reduce<{ name: string; d: number } | null>((best, i) => {
    const d = axes.reduce((n, a) => n + (((i.location[a.tag] ?? a.default) - (location[a.tag] ?? a.default)) / (a.max - a.min || 1)) ** 2, 0);
    return !best || d < best.d ? { name: i.name, d } : best;
  }, null);

  return (
    <div className="transform-panel variation-panel">
      <button type="button" className="transform-head" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round"><path d="M6 9l6 6 6-6" /></svg>
        {t("Variation preview")}
      </button>
      {open && (
        <div className="variation-body">
          <div className="variation-art" aria-hidden="true">
            {item?.type === "glyph" && item.svg
              ? <svg viewBox={`${item.width / 2 - h / 2} ${-asc} ${h} ${h}`}><path d={item.svg} transform="scale(1,-1)" /></svg>
              : <span className="k-caption muted">{t("Nothing drawn here yet")}</span>}
          </div>
          {axes.map((a) => (
            <label key={a.tag} className="variation-axis">
              <span className="row">
                <span className="k-caption">{a.name} ({a.tag})</span>
                <span className="spacer" />
                <span className="k-code">{Math.round(location[a.tag] ?? a.default)}</span>
              </span>
              <input type="range" min={a.min} max={a.max} step={1} value={location[a.tag] ?? a.default} aria-label={a.name}
                onChange={(e) => setLocation({ ...location, [a.tag]: Number(e.target.value) })} />
            </label>
          ))}
          {instances.length > 0 && (
            <select className="select" value="" aria-label={t("Go to an instance")}
              onChange={(e) => { const i = instances.find((x) => x.name === e.target.value); if (i) setLocation({ ...location, ...i.location }); }}>
              <option value="">{nearest ? t("Nearest instance: {name}", { name: nearest.name }) : t("Go to an instance")}</option>
              {instances.map((i) => <option key={i.name} value={i.name}>{i.name}</option>)}
            </select>
          )}
        </div>
      )}
    </div>
  );
}
