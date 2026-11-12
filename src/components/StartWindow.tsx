import { useMemo, useState } from "react";
import { relativeDate, type RecentFont } from "../lib/desktop";
import { ExternalIcon, SearchIcon, CloseIcon } from "./Icons";

interface Props {
  recents: RecentFont[];
  onNew: () => void;
  onOpen: () => void;
  onOpenRecent: (path: string) => void;
  onForgetRecent: (path: string) => void;
  onSample: () => void;
}

/** S01 · Start Window */
export function StartWindow({ recents, onNew, onOpen, onOpenRecent, onForgetRecent, onSample }: Props) {
  const [query, setQuery] = useState("");
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return q ? recents.filter((r) => r.familyName.toLowerCase().includes(q) || r.path.toLowerCase().includes(q)) : recents;
  }, [recents, query]);

  return (
    <div className="start">
      <section className="start-main">
        <div className="row">
          <h1 className="k-title">Recent fonts</h1>
          <span className="spacer" />
          <button className="btn btn-secondary" onClick={onOpen}>Open…</button>
          <button className="btn btn-primary" onClick={onNew}>New Font</button>
        </div>
        {recents.length > 0 && (
          <label className="search">
            <SearchIcon />
            <input placeholder="Search recent fonts" value={query} onChange={(e) => setQuery(e.target.value)} />
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
              <button className="icon-btn recent-forget" aria-label={`Remove ${r.familyName} from recent fonts`} onClick={() => onForgetRecent(r.path)}>
                <CloseIcon size={14} />
              </button>
            </li>
          ))}
        </ul>
        {recents.length === 0 && (
          <div className="empty">
            <div className="empty-art">Aa</div>
            <p className="k-subtitle">No recent fonts</p>
            <p className="k-body muted">Create your first font or open an existing .kernest font.</p>
            <button className="btn btn-link" onClick={onSample}>Explore the sample font</button>
          </div>
        )}
        {recents.length > 0 && shown.length === 0 && <p className="k-body muted">No recent fonts match “{query}”.</p>}
      </section>
      <aside className="start-side">
        <div className="logo">K</div>
        <h2 className="k-title">Kernest</h2>
        <p className="k-caption muted">Version {__APP_VERSION__} · open source · offline</p>
        <div className="divider" />
        <button className="link" onClick={onSample}>Explore the sample font</button>
        <a className="link" href="https://github.com/ShaikMoosa/kernest" target="_blank" rel="noreferrer">
          Project on GitHub <ExternalIcon />
        </a>
      </aside>
    </div>
  );
}
