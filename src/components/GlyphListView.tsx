import { useEffect, useMemo, useRef, useState } from "react";
import type { GlyphSummary } from "../lib/backend";
import { t } from "../lib/i18n";

/** A cell edit, routed to the existing metric/group/tag commands by the app. */
export type ListEdit =
  | { kind: "metric"; glyph: string; metric: "lsb" | "rsb" | "width"; value: number }
  | { kind: "groups"; glyph: string; left: string | null; right: string | null }
  | { kind: "rename"; glyph: string; name: string }
  | { kind: "tags"; glyph: string; tags: string[] }
  | { kind: "unicodes"; glyph: string; text: string };

/** HTML5 drag handlers shared by the Font View grid cells and list rows. */
export type DragHandlers = React.HTMLAttributes<HTMLElement>;

type ColumnKey =
  | "glyph" | "name" | "script" | "unicode" | "category" | "subcategory" | "lsb" | "rsb" | "width"
  | "leftGroup" | "rightGroup" | "export" | "tags";

const COLUMNS: { key: ColumnKey; label: string; w: string; edit?: "number" | "text" | "tags" | "name" | "unicodes" }[] = [
  { key: "glyph", label: "Glyph", w: "52px" },
  { key: "name", label: "Name", w: "150px", edit: "name" },
  { key: "unicode", label: "Unicode", w: "90px", edit: "unicodes" },
  { key: "category", label: "Category", w: "96px" },
  { key: "subcategory", label: "Subcategory", w: "96px" },
  { key: "script", label: "Script", w: "80px" },
  { key: "lsb", label: "LSB", w: "60px", edit: "number" },
  { key: "rsb", label: "RSB", w: "60px", edit: "number" },
  { key: "width", label: "Width", w: "60px", edit: "number" },
  { key: "leftGroup", label: "Left group", w: "96px", edit: "text" },
  { key: "rightGroup", label: "Right group", w: "96px", edit: "text" },
  { key: "export", label: "Export", w: "56px" },
  { key: "tags", label: "Tags", w: "minmax(150px, 1fr)", edit: "tags" },
];

const TEMPLATE = COLUMNS.map((c) => c.w).join(" ");
const ROW_H = 30;
const HEAD_H = 32;
const OVERSCAN = 6;

function sortValue(g: GlyphSummary, key: ColumnKey): string | number {
  switch (key) {
    case "lsb": return g.lsb ?? Number.MAX_SAFE_INTEGER;
    case "rsb": return g.rsb ?? Number.MAX_SAFE_INTEGER;
    case "width": return g.width;
    case "export": return g.export ? 1 : 0;
    case "tags": return g.tags.join(", ");
    case "unicode": return g.unicode ?? "";
    case "leftGroup": return g.leftGroup ?? "";
    case "rightGroup": return g.rightGroup ?? "";
    case "glyph": return g.name;
    default: return g[key];
  }
}

/** The glyph drawn small for the Glyph column (its character when it has no outlines yet). */
function ListGlyph({ glyph, metrics }: { glyph: GlyphSummary; metrics: { ascender: number; descender: number } }) {
  const h = metrics.ascender - metrics.descender;
  if (!glyph.svg) return <span className="list-glyph is-empty" aria-hidden="true">{glyph.character?.trim() ?? ""}</span>;
  return (
    <svg className="list-glyph" viewBox={`${glyph.width / 2 - h / 2} ${-metrics.ascender} ${h} ${h}`} aria-hidden="true">
      <path d={glyph.svg} transform="scale(1,-1)" />
    </svg>
  );
}

const fmt = (v: number | null) => (v === null ? "—" : String(Math.round(v)));

/** S04 · Virtualised, sortable glyph table; only the visible rows are rendered. */
export function GlyphListView({ glyphs, metrics, selection, onSelect, onOpen, onEdit, onSetExport, dragProps, dragName, dropName }: {
  glyphs: GlyphSummary[];
  /** Vertical metrics, to draw every glyph at the same scale in the Glyph column. */
  metrics: { ascender: number; descender: number };
  selection: Set<string>;
  onSelect: (g: GlyphSummary, e: React.MouseEvent) => void;
  onOpen: (name: string) => void;
  onEdit: (edit: ListEdit) => void;
  onSetExport: (names: string[], exportGlyph: boolean) => void;
  dragProps?: (name: string) => DragHandlers;
  dragName?: string | null;
  dropName?: string | null;
}) {
  const [sort, setSort] = useState<{ key: ColumnKey; dir: 1 | -1 } | null>(null);
  // A column sort rearranges the rows, so dragging would fight it: disable drag while sorted.
  const rowDrag = sort ? null : dragProps;
  const [editing, setEditing] = useState<{ name: string; key: ColumnKey } | null>(null);
  const [draft, setDraft] = useState("");
  const [scrollTop, setScrollTop] = useState(0);
  const [boxH, setBoxH] = useState(480);
  const boxRef = useRef<HTMLDivElement>(null);
  const cancelled = useRef(false);

  useEffect(() => {
    const el = boxRef.current;
    if (!el) return;
    const measure = () => setBoxH(el.clientHeight);
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const sorted = useMemo(() => {
    if (!sort) return glyphs;
    const { key, dir } = sort;
    return [...glyphs].sort((a, b) => {
      const [x, y] = [sortValue(a, key), sortValue(b, key)];
      const c = typeof x === "number" && typeof y === "number" ? x - y : String(x).localeCompare(String(y));
      return c * dir;
    });
  }, [glyphs, sort]);

  // Window of rows around the scroll position; the rest of the height is a spacer.
  const first = Math.max(0, Math.floor(Math.max(0, scrollTop - HEAD_H) / ROW_H) - OVERSCAN);
  const last = Math.min(sorted.length, Math.ceil((scrollTop + boxH) / ROW_H) + OVERSCAN);

  function startEdit(g: GlyphSummary, key: ColumnKey) {
    const col = COLUMNS.find((c) => c.key === key)!;
    if (!col.edit || key === "glyph") return;
    setEditing({ name: g.name, key });
    setDraft(
      col.edit === "number" ? String(g[key] ?? "")
        : key === "tags" ? g.tags.join(", ")
        : key === "unicode" ? g.unicodes.join(", ")
        : String(g[key] ?? ""),
    );
  }

  function commit() {
    if (cancelled.current) { cancelled.current = false; setEditing(null); return; }
    if (!editing) return;
    const g = glyphs.find((x) => x.name === editing.name);
    const col = COLUMNS.find((c) => c.key === editing.key)!;
    setEditing(null);
    if (!g || !col.edit) return;
    if (col.edit === "number") {
      const v = Number(draft);
      if (draft.trim() !== "" && Number.isFinite(v)) {
        onEdit({ kind: "metric", glyph: g.name, metric: editing.key as "lsb" | "rsb" | "width", value: v });
      }
    } else if (col.edit === "tags") {
      onEdit({ kind: "tags", glyph: g.name, tags: draft.split(",") });
    } else if (col.edit === "unicodes") {
      if (draft.trim() !== g.unicodes.join(", ")) onEdit({ kind: "unicodes", glyph: g.name, text: draft });
    } else if (col.edit === "name") {
      const next = draft.trim();
      if (next && next !== g.name) onEdit({ kind: "rename", glyph: g.name, name: next });
    } else {
      const side = draft.trim() || null;
      onEdit({
        kind: "groups",
        glyph: g.name,
        left: editing.key === "leftGroup" ? side : g.leftGroup,
        right: editing.key === "rightGroup" ? side : g.rightGroup,
      });
    }
  }

  function cellText(g: GlyphSummary, key: ColumnKey): string {
    switch (key) {
      case "unicode": return g.unicode ? `U+${g.unicode}${g.unicodes.length > 1 ? ` +${g.unicodes.length - 1}` : ""}` : "—";
      case "category": return t(g.category);
      case "subcategory": return g.subcategory ? t(g.subcategory) : "—";
      case "lsb": return fmt(g.lsb);
      case "rsb": return fmt(g.rsb);
      case "width": return fmt(g.width);
      case "leftGroup": return g.leftGroup ?? "—";
      case "rightGroup": return g.rightGroup ?? "—";
      case "tags": return g.tags.join(", ");
      case "script": return g.script && g.script !== "Common" ? t(g.script) : "—";
      default: return g.name;
    }
  }

  return (
    <div
      className="glyph-list"
      role="table"
      aria-label={t("Glyph list")}
      onKeyDown={(e) => {
        if (e.key === "Enter" && selection.size && !editing) {
          e.preventDefault();
          for (const g of glyphs) if (selection.has(g.name)) onOpen(g.name);
        }
      }}
    >
      <div className="glyph-list-body" ref={boxRef} onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}>
        <div className="glyph-list-head" role="row" style={{ gridTemplateColumns: TEMPLATE }}>
          {COLUMNS.map((c) => c.key === "glyph" ? <span key={c.key} role="columnheader" className="list-th">{t(c.label)}</span> : (
            <button
              key={c.key}
              type="button"
              role="columnheader"
              className="list-th"
              aria-sort={sort?.key === c.key ? (sort.dir === 1 ? "ascending" : "descending") : "none"}
              onClick={() => setSort((s) => (s?.key === c.key ? (s.dir === 1 ? { key: c.key, dir: -1 } : null) : { key: c.key, dir: 1 }))}
              title={t("Sort by {column}", { column: t(c.label) })}
            >
              {t(c.label)}{sort?.key === c.key ? (sort.dir === 1 ? " ↑" : " ↓") : ""}
            </button>
          ))}
        </div>
        <div style={{ height: sorted.length * ROW_H, position: "relative" }}>
          {sorted.slice(first, last).map((g, i) => (
            <div
              key={g.name}
              role="row"
              className={`glyph-list-row${selection.has(g.name) ? " is-selected" : ""}${rowDrag && dragName === g.name ? " is-dragging" : ""}${rowDrag && dropName === g.name && dragName !== g.name ? " is-drop" : ""}`}
              style={{ top: (first + i) * ROW_H, gridTemplateColumns: TEMPLATE }}
              {...(rowDrag ? rowDrag(g.name) : {})}
              onClick={(e) => onSelect(g, e)}
              onDoubleClick={() => onOpen(g.name)}
              title={g.locked ? t("This glyph is locked. Unlock it to edit.") : undefined}
            >
              {COLUMNS.map((c) => {
                const isEditing = editing?.name === g.name && editing.key === c.key;
                return (
                  <div
                    key={c.key}
                    role="cell"
                    className={`glyph-list-cell${c.edit === "number" ? " num" : ""}${c.key === "name" ? " name" : ""}`}
                    onDoubleClick={(e) => {
                      if (c.edit) { e.stopPropagation(); startEdit(g, c.key); }
                    }}
                  >
                    {isEditing ? (
                      <input
                        className="text-input list-edit"
                        autoFocus
                        value={draft}
                        aria-label={t(c.label)}
                        onChange={(e) => setDraft(e.target.value)}
                        onBlur={commit}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") e.currentTarget.blur();
                          if (e.key === "Escape") { cancelled.current = true; e.currentTarget.blur(); }
                        }}
                      />
                    ) : c.key === "glyph" ? (
                      <ListGlyph glyph={g} metrics={metrics} />
                    ) : c.key === "export" ? (
                      <input
                        type="checkbox"
                        checked={g.export}
                        aria-label={t("Export {name}", { name: g.name })}
                        onClick={(e) => e.stopPropagation()}
                        onChange={(e) => onSetExport([g.name], e.target.checked)}
                      />
                    ) : (
                      cellText(g, c.key)
                    )}
                  </div>
                );
              })}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}