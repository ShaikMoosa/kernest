import { useEffect, useMemo, useState } from "react";
import type { FontView, GlyphSummary } from "../lib/backend";
import { SearchIcon } from "./Icons";
import { t, tn } from "../lib/i18n";
import { ColorGlyph } from "./ColorGlyph";

const CATEGORY_ORDER = ["Uppercase", "Lowercase", "Letter", "Number", "Punctuation", "Symbol", "Separator", "Mark", "Other"];

function GlyphCell({ glyph, font, selected, onSelect, onOpen }: { glyph: GlyphSummary; font: FontView; selected: boolean; onSelect: (e: React.MouseEvent) => void; onOpen: () => void }) {
  const { ascender, descender } = font.metrics;
  const h = ascender - descender;
  // Square view box centred on the advance width, so every glyph uses the same scale.
  const x0 = glyph.width / 2 - h / 2;
  const empty = !glyph.svg;
  return (
    <button className={`glyph-cell${selected ? " is-selected" : ""}${empty ? " is-empty" : ""}`} onClick={onSelect} onDoubleClick={onOpen} title={t("{name} — double-click to edit", { name: glyph.name })}>
      <span className="glyph-art">
        {empty ? (
          <span className="glyph-placeholder">{glyph.character?.trim() ? glyph.character : ""}</span>
        ) : (
          <svg viewBox={`${x0} ${-ascender} ${h} ${h}`} aria-hidden="true">
            {glyph.color?.length
              ? <ColorGlyph layers={glyph.color} palette={font.palette ?? []} />
              : <path d={glyph.svg} transform="scale(1,-1)" />}
          </svg>
        )}
      </span>
      {!glyph.compatible && <span className="glyph-incompatible" title={t("Masters can't be interpolated (open the glyph to see why)")} aria-label={t("Incompatible masters")} />}
      {glyph.locked && (
        <span className="glyph-locked" title={t("This glyph is locked. Unlock it to edit.")} aria-label={t("Locked")}>
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" aria-hidden="true">
            <rect x="5" y="11" width="14" height="9" rx="2" /><path d="M8 11V7a4 4 0 0 1 8 0v4" />
          </svg>
        </span>
      )}
      <span className="glyph-label">
        <span className="glyph-name">{glyph.name}</span>
        <span className="glyph-unicode">{glyph.unicode ?? ""}</span>
      </span>
    </button>
  );
}

/** S03 · Font View — grid of glyphs with category sidebar and glyph info. */
export function FontViewScreen({ font, onOpenGlyph, onSelectionChange, onAddGlyphs, onSetExport }: { font: FontView; onOpenGlyph: (name: string) => void; onSelectionChange?: (names: string[]) => void; onAddGlyphs?: () => void; onSetExport?: (names: string[], exportGlyph: boolean) => void }) {
  const [category, setCategory] = useState<string>("All");
  const [query, setQuery] = useState("");
  const [selection, setSelection] = useState<Set<string>>(new Set());

  // Keep the selection to glyphs that still exist, and tell the app (for Remove Glyph).
  useEffect(() => {
    const names = new Set(font.glyphs.map((g) => g.name));
    const kept = [...selection].filter((n) => names.has(n));
    if (kept.length !== selection.size) setSelection(new Set(kept));
    onSelectionChange?.(font.glyphs.filter((g) => selection.has(g.name)).map((g) => g.name));
  }, [selection, font.glyphs, onSelectionChange]);

  const incompatibleCount = useMemo(() => font.glyphs.filter((g) => !g.compatible).length, [font.glyphs]);
  const counts = useMemo(() => {
    const m = new Map<string, number>();
    for (const g of font.glyphs) m.set(g.category, (m.get(g.category) ?? 0) + 1);
    return CATEGORY_ORDER.filter((c) => m.has(c)).map((c) => [c, m.get(c)!] as const);
  }, [font.glyphs]);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return font.glyphs.filter((g) => {
      if (category === "Incompatible") { if (g.compatible) return false; }
      else if (category !== "All" && g.category !== category) return false;
      if (!q) return true;
      return g.name.toLowerCase().includes(q) || g.unicode?.toLowerCase() === q.replace(/^u\+/, "") || g.character === query.trim();
    });
  }, [font.glyphs, category, query]);

  const drawn = font.glyphs.filter((g) => g.svg).length;
  const selectedGlyphs = font.glyphs.filter((g) => selection.has(g.name));
  const one = selectedGlyphs.length === 1 ? selectedGlyphs[0] : null;

  function select(g: GlyphSummary, e: React.MouseEvent) {
    setSelection((prev) => {
      if (e.ctrlKey) {
        const next = new Set(prev);
        if (next.has(g.name)) next.delete(g.name);
        else next.add(g.name);
        return next;
      }
      if (e.shiftKey && prev.size > 0) {
        const names = shown.map((x) => x.name);
        const last = [...prev].pop()!;
        const [a, b] = [names.indexOf(last), names.indexOf(g.name)].sort((x, y) => x - y);
        if (a >= 0) return new Set([...prev, ...names.slice(a, b + 1)]);
      }
      return new Set([g.name]);
    });
  }

  const fmt = (v: number | null) => (v === null ? "—" : String(Math.round(v)));

  return (
    <div className="fontview">
      <aside className="sidebar">
        <label className="search">
          <SearchIcon />
          <input placeholder={t("Search name, Unicode, character")} value={query} onChange={(e) => setQuery(e.target.value)} />
        </label>
        <p className="k-overline muted sidebar-heading">{t("Categories")}</p>
        <ul className="list">
          {[["All", font.glyphs.length] as const, ...counts, ...(incompatibleCount ? [["Incompatible", incompatibleCount] as const] : [])].map(([c, n]) => (
            <li key={c}>
              <button className={`list-item${category === c ? " is-selected" : ""}`} onClick={() => setCategory(c)}>
                <span>{t(c)}</span>
                <span className="list-meta">{n}</span>
              </button>
            </li>
          ))}
        </ul>
        <span className="spacer" />
        <section className="glyph-info" aria-label={t("Glyph info")}>
          {one ? (
            <>
              <div className="row">
                <span className="k-body-strong">{one.name}</span>
                <span className="spacer" />
                <span className="k-code muted">{one.unicode ? `U+${one.unicode}` : t("unencoded")}</span>
              </div>
              <div className="metric-row">
                <label><span className="k-caption muted">{t("LSB")}</span><output className="metric">{fmt(one.lsb)}</output></label>
                <label><span className="k-caption muted">{t("Width")}</span><output className="metric">{fmt(one.width)}</output></label>
                <label><span className="k-caption muted">{t("RSB")}</span><output className="metric">{fmt(one.rsb)}</output></label>
              </div>
              <p className="k-caption muted">{t(one.category)}{one.export ? "" : ` · ${t("not exported")}`}</p>
            </>
          ) : (
            <p className="k-caption muted">{selectedGlyphs.length > 1 ? tn(selectedGlyphs.length, "{n} glyph selected", "{n} glyphs selected") : t("Select a glyph to see its metrics.")}</p>
          )}
          {selectedGlyphs.length > 0 && onSetExport && (
            <label className="check-row" title={t("Glyphs that aren't exported (such as parts used only as components) are left out of fonts")}>
              <input
                type="checkbox"
                checked={selectedGlyphs.every((g) => g.export)}
                ref={(el) => { if (el) el.indeterminate = selectedGlyphs.some((g) => g.export) && !selectedGlyphs.every((g) => g.export); }}
                onChange={(e) => onSetExport(selectedGlyphs.map((g) => g.name), e.target.checked)}
                aria-label={t("Export the selected glyphs")}
              />
              <span className="k-caption">{selectedGlyphs.length > 1 ? t("Export these glyphs") : t("Export this glyph")}</span>
            </label>
          )}
        </section>
      </aside>
      <main className="grid-area">
        <div className="grid-header">
          <h2 className="k-subtitle">{category === "All" ? t("All glyphs") : t(category)}</h2>
          <span className="k-caption muted">{tn(shown.length, "{n} glyph", "{n} glyphs")}</span>
          <span className="spacer" />
          {onAddGlyphs && <button type="button" className="btn btn-secondary btn-sm" onClick={onAddGlyphs}>{t("Add Glyphs…")}</button>}
        </div>
        {shown.length ? (
          <div className="glyph-grid" role="list" onKeyDown={(e) => {
            // Enter opens the selected glyphs in Edit View tabs.
            if (e.key === "Enter" && selection.size) {
              e.preventDefault();
              for (const g of font.glyphs) if (selection.has(g.name)) onOpenGlyph(g.name);
            }
          }}>
            {shown.map((g) => (
              <GlyphCell key={g.name} glyph={g} font={font} selected={selection.has(g.name)} onSelect={(e) => select(g, e)} onOpen={() => onOpenGlyph(g.name)} />
            ))}
          </div>
        ) : (
          <div className="empty">
            <p className="k-subtitle">{t("No glyphs match “{query}”", { query })}</p>
            <button className="btn btn-secondary" onClick={() => { setQuery(""); setCategory("All"); }}>{t("Clear search & filters")}</button>
          </div>
        )}
      </main>
      <footer className="statusbar k-caption">
        <span>{t("Selected: {selected} · Shown: {shown} · Total: {total} · Drawn: {drawn}", { selected: selection.size, shown: shown.length, total: font.glyphs.length, drawn })}</span>
        <span className="spacer" />
        <span>{t("Master: {master}", { master: font.masters.find((m) => m.id === font.masterId)?.name ?? "" })} · {font.path ?? t("Not saved yet")}</span>
      </footer>
    </div>
  );
}
