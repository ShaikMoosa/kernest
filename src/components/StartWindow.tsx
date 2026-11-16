import { useMemo, useState } from "react";
import { openExternal, relativeDate, REPO_URL, type RecentFont } from "../lib/desktop";
import { ExternalIcon, SearchIcon, CloseIcon } from "./Icons";
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
}

/** S01 · Start Window */
export function StartWindow({ recents, onNew, onOpen, onOpenRecent, onForgetRecent, onSample, onImport }: Props) {
  const [query, setQuery] = useState("");
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? recents.filter((r) => r.familyName.toLowerCase().includes(q) || r.path.toLowerCase().includes(q)) : recents;
  }, [recents, query]);

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
            <input placeholder={t("Search recent fonts")} value={query} onChange={(e) => setQuery(e.target.value)} />
          </label>
        )}
        <ul className="recent-list">
          {shown.map((r) => (
            <li key={r.path}>
              <button className="recent-item" onClick={() => onOpenRecent(r.path)}>
                <span className="recent-thumb">Hamb</span>
                <span className="recent-text">
                  <span className="k-body-strong">{r.familyName}</span>
                  <span className="k-caption muted">{r.path}</span>
                </span>
                <span className="k-caption muted">{relativeDate(r.openedAt)}</span>
              </button>
              <button className="icon-btn recent-forget" aria-label={t("Remove {name} from recent fonts", { name: r.familyName })} onClick={() => onForgetRecent(r.path)}>
                <CloseIcon size={14} />
              </button>
            </li>
          ))}
        </ul>
        {recents.length === 0 && (
          <div className="empty">
            <div className="empty-art">Aa</div>
            <p className="k-subtitle">{t("No recent fonts")}</p>
            <p className="k-body muted">{t("Create your first font or open an existing .kernest font.")}</p>
            <button className="btn btn-link" onClick={onSample}>{t("Explore the sample font")}</button>
          </div>
        )}
        {recents.length > 0 && shown.length === 0 && <p className="k-body muted">{t("No recent fonts match “{query}”.", { query })}</p>}
      </section>
      <aside className="start-side">
        <img className="logo" src={logoUrl} alt={t("Kernest logo")} />
        <h2 className="k-title">Kernest</h2>
        <p className="k-caption muted">{t("Version {version} · open source · offline", { version: __APP_VERSION__ })}</p>
        <div className="divider" />
        <button className="link" onClick={onSample}>{t("Explore the sample font")}</button>
        <a className="link" href={REPO_URL} onClick={(e) => { e.preventDefault(); openExternal(REPO_URL); }}>
          {t("Project on GitHub")} <ExternalIcon />
        </a>
      </aside>
    </div>
  );
}
