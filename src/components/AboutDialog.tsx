import { useEffect } from "react";
import logoUrl from "../assets/kernest-logo.svg";
import { HANDBOOK_URL, openExternal, REPO_URL } from "../lib/desktop";
import { CloseIcon } from "./Icons";
import { t } from "../lib/i18n";

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
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onClose}><CloseIcon /></button>
        </header>
        <div className="dialog-body about-body">
          <img className="about-logo" src={logoUrl} alt={t("Kernest logo")} />
          <h2 id="about-title" className="k-title">Kernest</h2>
          <p className="k-body">{t("A font editor for Windows. Open source, works offline.")}</p>
          <p className="k-caption muted">{t("Version {version} · Apache License 2.0", { version: __APP_VERSION__ })}</p>
          <p className="k-caption muted">{t("Fonts are compiled with fontc and autohinted with tilvisan; Glyphs and UFO sources are read with glyphs-reader and norad.")}</p>
          <p className="about-links">{link(HANDBOOK_URL, t("Handbook"))} · {link(REPO_URL, t("Source code"))} · {link(`${REPO_URL}/releases`, t("Releases"))}</p>
        </div>
      </div>
    </div>
  );
}
