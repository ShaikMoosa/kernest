import { useEffect, useMemo, useState } from "react";
import { baseName, openExternal, relativeDate, REPO_URL, setShowStartAtLaunch, shortFolder, showStartAtLaunch, type RecentFont } from "../lib/desktop";
import type { UpdateInfo } from "../lib/backend";
import { ArrowUpRightIcon, SearchIcon, CloseIcon } from "./Icons";
import logoUrl from "../assets/kernest-logo.svg";
import { t } from "../lib/i18n";

interface Props {
  recents: RecentFont[];
  onNew: () => void;
  onOpen: () => void;
  onOpenRecent: (path: string) => void;
  onForgetRecent: (path: string) => void;
  onSample: () => void;
  onImport: () => void;
  /** The last update check, when update checks are on (null: nothing to show). */
  updateInfo: UpdateInfo | null;
  /** Opens the Handbook, at a section when given. */
  onHandbook: (section?: string) => void;
  onShortcuts: () => void;
  onWhatsNew: () => void;
}

/** S01 · Start Window */
export function StartWindow({ recents, onNew, onOpen, onOpenRecent, onForgetRecent, onSample, onImport, updateInfo, onHandbook, onShortcuts, onWhatsNew }: Props) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [atLaunch, setAtLaunch] = useState(showStartAtLaunch);
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? recents.filter((r) => r.familyName.toLowerCase().includes(q) || r.path.toLowerCase().includes(q)) : recents;
  }, [recents, query]);
  useEffect(() => { setActive(0); }, [query]);

  const updateBadge = updateInfo && !updateInfo.error
    ? updateInfo.available
      ? <span className="pill pill-group">{t("Kernest {version} is available", { version: updateInfo.latestVersion ?? "" })}</span>
      : <span className="pill pill-success">{t("Up to date")}</span>
    : null;

  return (
    <div className="start">
      <section className="start-main">
        <div className="row">
          <h1 className="k-title">{t("Recent fonts")}</h1>
          <span className="spacer" />
          <button className="btn btn-secondary" onClick={onImport} title={t("Import a .glyphs, .glyphspackage, .ufo or .designspace source")}>{t("Import…")}</button>
          <button className="btn btn-secondary" onClick={onOpen}>{t("Open…")}</button>
          <button className="btn btn-primary" onClick={onNew}>{t("New Font")}</button>
        </div>
        {recents.length > 0 && (
          <label className="search">
            <SearchIcon />
            <input placeholder={t("Search recent fonts")} value={query} onChange={(e) => setQuery(e.target.value)}
              aria-label={t("Search recent fonts")}
              onKeyDown={(e) => {
                // Arrow keys move through the list, Enter opens the highlighted font.
                if (e.key === "ArrowDown") { e.preventDefault(); setActive((i) => Math.min(i + 1, shown.length - 1)); }
                else if (e.key === "ArrowUp") { e.preventDefault(); setActive((i) => Math.max(i - 1, 0)); }
                else if (e.key === "Enter" && shown[active]) { e.preventDefault(); onOpenRecent(shown[active].path); }
              }} />
          </label>
        )}
        <ul className="recent-list">
          {shown.map((r, i) => {
            const file = baseName(r.path);
            return (
              <li key={r.path}>
                <button className={`recent-item${i === active ? " is-active" : ""}`} onClick={() => onOpenRecent(r.path)}
                  onMouseEnter={() => setActive(i)} title={r.path}>
                  <span className="recent-thumb">Hamb</span>
                  <span className="recent-text">
                    <span className="k-body-strong recent-name">{file}</span>
                    <span className="k-caption muted">{file.replace(/\.[^.]+$/, "") === r.familyName ? shortFolder(r.path) : `${r.familyName} · ${shortFolder(r.path)}`}</span>
                  </span>
                  <span className="k-caption muted">{relativeDate(r.openedAt)}</span>
                </button>
                <button className="icon-btn recent-forget" aria-label={t("Remove {name} from recent fonts", { name: r.familyName })} onClick={() => onForgetRecent(r.path)}>
                  <CloseIcon size={14} />
                </button>
              </li>
            );
          })}
        </ul>
        {recents.length === 0 && (
          <div className="empty">
            <div className="empty-art">Aa</div>
            <p className="k-subtitle">{t("No recent fonts")}</p>
            <p className="k-body muted">{t("Create your first font or open an existing .kernest font.")}</p>
            <div className="row">
              <button className="btn btn-primary" onClick={onNew}>{t("New Font")}</button>
              <button className="btn btn-secondary" onClick={onOpen}>{t("Open…")}</button>
            </div>
            <button className="btn btn-link" onClick={() => onHandbook("getting-started")}>↗ {t("Getting started")}</button>
            <button className="btn btn-link" onClick={onSample}>{t("Explore the sample font")}</button>
          </div>
        )}
        {recents.length > 0 && shown.length === 0 && <p className="k-body muted">{t("No recent fonts match “{query}”.", { query })}</p>}
      </section>
      <aside className="start-side">
        <img className="logo" src={logoUrl} alt={t("Kernest logo")} />
        <h2 className="k-title">Kernest</h2>
        <p className="k-caption muted">{t("Version {version} · open source · offline", { version: __APP_VERSION__ })}</p>
        {updateBadge && <div>{updateBadge}</div>}
        <div className="divider" />
        <nav className="start-links" aria-label={t("Help")}>
          <button className="link" onClick={() => onHandbook("getting-started")}><ArrowUpRightIcon /> {t("Getting started")}</button>
          <button className="link" onClick={() => onHandbook()}><ArrowUpRightIcon /> {t("Kernest Handbook")}</button>
          <button className="link" onClick={onShortcuts}><ArrowUpRightIcon /> {t("Keyboard shortcuts")}</button>
          <button className="link" onClick={onWhatsNew}><ArrowUpRightIcon /> {t("What’s new")}</button>
          <button className="link" onClick={onSample}><ArrowUpRightIcon /> {t("Explore the sample font")}</button>
          <a className="link" href={REPO_URL} onClick={(e) => { e.preventDefault(); openExternal(REPO_URL); }}>
            <ArrowUpRightIcon /> {t("Project on GitHub")}
          </a>
        </nav>
        <span className="spacer" />
        <label className="check-row">
          <input type="checkbox" checked={atLaunch} onChange={(e) => { setAtLaunch(e.target.checked); setShowStartAtLaunch(e.target.checked); }} />
          <span className="k-body" title={t("When this is off, Kernest reopens your most recent font when it starts.")}>{t("Show at launch")}</span>
        </label>
      </aside>
    </div>
  );
}
