import { useEffect, useMemo, useRef, useState } from "react";
import type { Menu, MenuItem } from "./TitleBar";
import { SearchIcon } from "./Icons";
import { t } from "../lib/i18n";

interface Entry {
  path: string;
  label: string;
  shortcut?: string;
  onSelect: () => void;
}

function flatten(items: MenuItem[], prefix: string, out: Entry[]) {
  for (const it of items) {
    if (it.submenu) {
      flatten(it.submenu, `${prefix} › ${it.label}`, out);
    } else if (it.onSelect && !it.disabled) {
      out.push({ path: prefix, label: it.label, shortcut: it.shortcut, onSelect: it.onSelect });
    }
  }
}

/** Help › Search Menus (Alt+Q): find any menu command by name and run it. */
export function MenuSearch({ menus, onClose }: { menus: Menu[]; onClose: () => void }) {
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const entries = useMemo(() => {
    const out: Entry[] = [];
    for (const m of menus) flatten(m.items, m.label, out);
    return out;
  }, [menus]);
  const shown = useMemo(() => {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    const list = words.length
      ? entries.filter((e) => {
          const text = `${e.path} ${e.label}`.toLowerCase();
          return words.every((w) => text.includes(w));
        })
      : entries;
    return list.slice(0, 40);
  }, [entries, query]);
  useEffect(() => { inputRef.current?.focus(); }, []);
  useEffect(() => { setIndex(0); }, [query]);

  const run = (e: Entry | undefined) => {
    if (!e) return;
    onClose();
    // After the palette closes, so dialogs the command opens get the focus.
    setTimeout(e.onSelect, 0);
  };

  return (
    <div className="scrim" role="presentation" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="dialog menu-search" role="dialog" aria-modal="true" aria-label={t("Search menus")}>
        <label className="search menu-search-input">
          <SearchIcon />
          <input ref={inputRef} value={query} placeholder={t("Search menus…")} aria-label={t("Search menus")}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") { e.preventDefault(); onClose(); }
              else if (e.key === "ArrowDown") { e.preventDefault(); setIndex((i) => Math.min(i + 1, shown.length - 1)); }
              else if (e.key === "ArrowUp") { e.preventDefault(); setIndex((i) => Math.max(i - 1, 0)); }
              else if (e.key === "Enter") { e.preventDefault(); run(shown[index]); }
            }} />
        </label>
        <ul className="menu-search-list" role="listbox" aria-label={t("Menu commands")}>
          {shown.map((e, i) => (
            <li key={`${i}:${e.path}/${e.label}`} role="option" aria-selected={i === index}
              className={`menu-search-item${i === index ? " is-active" : ""}`}
              onMouseEnter={() => setIndex(i)} onClick={() => run(e)}>
              <span className="k-body">{e.label}</span>
              <span className="k-caption muted">{e.path}</span>
              <span className="spacer" />
              {e.shortcut && <span className="k-caption muted menu-search-key">{e.shortcut}</span>}
            </li>
          ))}
          {shown.length === 0 && <li className="k-caption muted menu-search-empty">{t("No menu command matches “{query}”", { query })}</li>}
        </ul>
      </div>
    </div>
  );
}
