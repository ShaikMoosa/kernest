import { useEffect, useRef, useState } from "react";
import { windowControls } from "../lib/desktop";
import { CaptionIcon } from "./Icons";
import logoUrl from "../assets/kernest-logo.svg";

export interface MenuItem {
  label: string;
  shortcut?: string;
  onSelect?: () => void;
  disabled?: boolean;
  separatorBefore?: boolean;
  /** Shows a check mark (toggles such as View › Show Metrics). */
  checked?: boolean;
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
      <nav className="menubar" aria-label="Main menu">
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
                {m.items.map((item) => (
                  <div key={item.label}>
                    {item.separatorBefore && <div className="menu-separator" role="separator" />}
                    <button
                      role={item.checked === undefined ? "menuitem" : "menuitemcheckbox"}
                      aria-checked={item.checked}
                      className="menu-item"
                      disabled={item.disabled || !item.onSelect}
                      onClick={() => {
                        setOpen(null);
                        item.onSelect?.();
                      }}
                    >
                      <span className="menu-check" aria-hidden="true">{item.checked ? "✓" : ""}</span>
                      <span>{item.label}</span>
                      {item.shortcut && <span className="menu-shortcut">{item.shortcut}</span>}
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        ))}
      </nav>
      <div className="titlebar-title k-label" data-tauri-drag-region>
        {title}
      </div>
      <div className="captions">
        <button className="caption" aria-label="Minimize" onClick={windowControls.minimize}>
          <CaptionIcon kind="minimize" />
        </button>
        <button className="caption" aria-label="Maximize" onClick={windowControls.toggleMaximize}>
          <CaptionIcon kind="maximize" />
        </button>
        <button className="caption caption-close" aria-label="Close" onClick={windowControls.close}>
          <CaptionIcon kind="close" />
        </button>
      </div>
    </header>
  );
}
