import { useEffect, useMemo, useState } from "react";
import type { FontView, GlyphSummary } from "../lib/backend";
import { SearchIcon } from "./Icons";

const CATEGORY_ORDER = ["Uppercase", "Lowercase", "Letter", "Number", "Punctuation", "Symbol", "Separator", "Mark", "Other"];

function GlyphCell({ glyph, font, selected, onSelect, onOpen }: { glyph: GlyphSummary; font: FontView; selected: boolean; onSelect: (e: React.MouseEvent) => void; onOpen: () => void }) {
  const { ascender, descender } = font.metrics;
  const h = ascender - descender;
  // Square view box centred on the advance width, so every glyph uses the same scale.
  const x0 = glyph.width / 2 - h / 2;
  const empty = !glyph.svg;
  return (
    <button className={`glyph-cell${selected ? " is-selected" : ""}${empty ? " is-empty" : ""}`} onClick={onSelect} onDoubleClick={onOpen} title={`${glyph.name} — double-click to edit`}>
      <span className="glyph-art">
        {empty ? (
          <span className="glyph-placeholder">{glyph.character?.trim() ? glyph.character : ""}</span>
        ) : (
          <svg viewBox={`${x0} ${-ascender} ${h} ${h}`} aria-hidden="true">
            <path d={glyph.svg} transform="scale(1,-1)" />
          </svg>
        )}
      </span>
      {!glyph.compatible && <span className="glyph-incompatible" title="Masters can't be interpolated (open the glyph to see why)" aria-label="Incompatible masters" />}
      <span className="glyph-label">
        <span className="glyph-name">{glyph.name}</span>
        <span className="glyph-unicode">{glyph.unicode ?? ""}</span>
      </span>
    </button>
  );
}

/** S03 · Font View — grid of glyphs with category sidebar and glyph info. */
export function FontViewScreen({ font, onOpenGlyph, onSelectionChange, onAddGlyphs }: { font: FontView; onOpenGlyph: (name: string) => void; onSelectionChange?: (names: string[]) => void; onAddGlyphs?: () => void }) {
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
          <input placeholder="Search name, Unicode, character" value={query} onChange={(e) => setQuery(e.target.value)} />
        </label>
        <p className="k-overline muted sidebar-heading">Categories</p>
        <ul className="list">
          {[["All", font.glyphs.length] as const, ...counts, ...(incompatibleCount ? [["Incompatible", incompatibleCount] as const] : [])].map(([c, n]) => (
            <li key={c}>
              <button className={`list-item${category === c ? " is-selected" : ""}`} onClick={() => setCategory(c)}>
                <span>{c}</span>
                <span className="list-meta">{n}</span>
              </button>
            </li>
          ))}
        </ul>
        <span className="spacer" />
        <section className="glyph-info" aria-label="Glyph info">
          {one ? (
            <>
              <div className="row">
                <span className="k-body-strong">{one.name}</span>
                <span className="spacer" />
                <span className="k-code muted">{one.unicode ? `U+${one.unicode}` : "unencoded"}</span>
              </div>
              <div className="metric-row">
                <label><span className="k-caption muted">LSB</span><output className="metric">{fmt(one.lsb)}</output></label>
                <label><span className="k-caption muted">Width</span><output className="metric">{fmt(one.width)}</output></label>
                <label><span className="k-caption muted">RSB</span><output className="metric">{fmt(one.rsb)}</output></label>
              </div>
              <p className="k-caption muted">{one.category}{one.export ? "" : " · not exported"}</p>
            </>
          ) : (
            <p className="k-caption muted">{selectedGlyphs.length > 1 ? `${selectedGlyphs.length} glyphs selected` : "Select a glyph to see its metrics."}</p>
          )}
        </section>
      </aside>
      <main className="grid-area">
        <div className="grid-header">
          <h2 className="k-subtitle">{category === "All" ? "All glyphs" : category}</h2>
          <span className="k-caption muted">{shown.length} glyphs</span>
          <span className="spacer" />
          {onAddGlyphs && <button type="button" className="btn btn-secondary btn-sm" onClick={onAddGlyphs}>Add Glyphs…</button>}
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
            <p className="k-subtitle">No glyphs match “{query}”</p>
            <button className="btn btn-secondary" onClick={() => { setQuery(""); setCategory("All"); }}>Clear search & filters</button>
          </div>
        )}
      </main>
      <footer className="statusbar k-caption">
        <span>{selection.size} selected · {shown.length} shown · {font.glyphs.length} total · {drawn} drawn</span>
        <span className="spacer" />
        <span>Master: {font.masters.find((m) => m.id === font.masterId)?.name} · {font.path ?? "Not saved yet"}</span>
      </footer>
    </div>
  );
}
