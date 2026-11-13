import { useEffect } from "react";
import logoUrl from "../assets/kernest-logo.svg";
import { HANDBOOK_URL, openExternal, REPO_URL } from "../lib/desktop";
import { CloseIcon } from "./Icons";

/** Help › About Kernest. */
export function AboutDialog({ onClose }: { onClose: () => void }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => (e.key === "Escape" || e.key === "Enter") && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  const link = (url: string, label: string) => (
    <a className="link" href={url} onClick={(e) => { e.preventDefault(); openExternal(url); }}>{label}</a>
  );
  return (
    <div className="scrim" role="presentation" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="dialog about-dialog" role="dialog" aria-modal="true" aria-labelledby="about-title">
        <header className="dialog-header">
          <span className="spacer" />
          <button type="button" className="icon-btn" aria-label="Close" onClick={onClose}><CloseIcon /></button>
        </header>
        <div className="dialog-body about-body">
          <img className="about-logo" src={logoUrl} alt="Kernest logo" />
          <h2 id="about-title" className="k-title">Kernest</h2>
          <p className="k-body">A font editor for Windows. Open source, works offline.</p>
          <p className="k-caption muted">Version {__APP_VERSION__} · Apache License 2.0</p>
          <p className="k-caption muted">Fonts are compiled with fontc; Glyphs and UFO sources are read with glyphs-reader and norad.</p>
          <p className="about-links">{link(HANDBOOK_URL, "Handbook")} · {link(REPO_URL, "Source code")} · {link(`${REPO_URL}/releases`, "Releases")}</p>
        </div>
      </div>
    </div>
  );
}
