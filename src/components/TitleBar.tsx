import { useEffect, useRef, useState } from "react";
import { windowControls } from "../lib/desktop";
import { CaptionIcon } from "./Icons";
import logoUrl from "../assets/kernest-logo.svg";
import { t } from "../lib/i18n";

export interface MenuItem {
  label: string;
  shortcut?: string;
  onSelect?: () => void;
  disabled?: boolean;
  separatorBefore?: boolean;
  /** Shows a check mark (toggles such as View › Show Metrics). */
  checked?: boolean;
  /** A nested submenu (Script folders). */
  submenu?: MenuItem[];
}

export interface Menu {
  label: string;
  items: MenuItem[];
}

/**
 * Windows-style custom title bar: app icon, in-window menu bar, document title
 * and caption buttons. Empty areas drag the window.
 */
export function TitleBar({ title, menus }: { title: string; menus: Menu[] }) {
  const [open, setOpen] = useState<number | null>(null);
  const barRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (open === null) return;
    const onDown = (e: MouseEvent) => {
      if (!barRef.current?.contains(e.target as Node)) setOpen(null);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(null);
    };
    window.addEventListener("mousedown", onDown);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <header className="titlebar" ref={barRef} data-tauri-drag-region onDoubleClick={(e) => e.target === e.currentTarget && windowControls.toggleMaximize()}>
      <img className="titlebar-icon" src={logoUrl} alt="" aria-hidden="true" />
      <nav className="menubar" aria-label={t("Main menu")}>
        {menus.map((m, i) => (
          <div key={m.label} className="menubar-entry">
            <button
              className={`menubar-item${open === i ? " is-open" : ""}`}
              aria-haspopup="menu"
              aria-expanded={open === i}
              onClick={() => setOpen(open === i ? null : i)}
              onMouseEnter={() => open !== null && setOpen(i)}
            >
              {m.label}
            </button>
            {open === i && (
              <div className="menu" role="menu">
                <MenuItems items={m.items} onPick={() => setOpen(null)} />
              </div>
            )}
          </div>
        ))}
      </nav>
      <div className="titlebar-title k-label" data-tauri-drag-region>
        {title}
      </div>
      <div className="captions">
        <button className="caption" aria-label={t("Minimize")} onClick={windowControls.minimize}>
          <CaptionIcon kind="minimize" />
        </button>
        <button className="caption" aria-label={t("Maximize")} onClick={windowControls.toggleMaximize}>
          <CaptionIcon kind="maximize" />
        </button>
        <button className="caption caption-close" aria-label={t("Close")} onClick={windowControls.close}>
          <CaptionIcon kind="close" />
        </button>
      </div>
    </header>
  );
}

/** One menu's items; a submenu opens to the side on hover (Script folders). */
function MenuItems({ items, onPick }: { items: MenuItem[]; onPick: () => void }) {
  const [sub, setSub] = useState<number | null>(null);
  return (
    <>
      {items.map((item, i) => (
        <div key={item.label} className={item.submenu ? "menu-sub-wrap" : undefined} onMouseLeave={() => item.submenu && setSub(null)}>
          {item.separatorBefore && <div className="menu-separator" role="separator" />}
          {item.submenu ? (
            <>
              <button
                role="menuitem"
                aria-haspopup="menu"
                aria-expanded={sub === i}
                className={`menu-item has-submenu${sub === i ? " is-open" : ""}`}
                disabled={item.disabled}
                onMouseEnter={() => setSub(i)}
                onClick={() => setSub(sub === i ? null : i)}
              >
                <span className="menu-check" aria-hidden="true" />
                <span>{item.label}</span>
                <span className="menu-arrow" aria-hidden="true">›</span>
              </button>
              {sub === i && (
                <div className="menu menu-sub" role="menu">
                  <MenuItems items={item.submenu} onPick={onPick} />
                </div>
              )}
            </>
          ) : (
            <button
              role={item.checked === undefined ? "menuitem" : "menuitemcheckbox"}
              aria-checked={item.checked}
              className="menu-item"
              disabled={item.disabled || !item.onSelect}
              onMouseEnter={() => setSub(null)}
              onClick={() => {
                onPick();
                item.onSelect?.();
              }}
            >
              <span className="menu-check" aria-hidden="true">{item.checked ? "✓" : ""}</span>
              <span>{item.label}</span>
              {item.shortcut && <span className="menu-shortcut">{item.shortcut}</span>}
            </button>
          )}
        </div>
      ))}
    </>
  );
}
