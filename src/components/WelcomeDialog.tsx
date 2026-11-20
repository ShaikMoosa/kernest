import { useEffect, useRef, useState } from "react";
import { errorMessage } from "../lib/backend";
import { setShowStartAtLaunch, showStartAtLaunch } from "../lib/desktop";
import { t } from "../lib/i18n";
import { AddIcon, CloseIcon, FolderIcon } from "./Icons";
import logo from "../assets/kernest-logo.svg";

export type StartAction = "new" | "open" | "sample" | "import";
interface Props { onComplete: (action: StartAction | null, experience: "beginner" | "experienced") => Promise<void> }

export function WelcomeDialog({ onComplete }: Props) {
  const [experience, setExperience] = useState<"beginner" | "experienced">("beginner");
  const [action, setAction] = useState<StartAction>("new");
  const [busy, setBusy] = useState(false);
  const [atLaunch, setAtLaunch] = useState(showStartAtLaunch);
  const [error, setError] = useState<string | null>(null);
  const form = useRef<HTMLFormElement>(null);
  useEffect(() => { form.current?.querySelector<HTMLInputElement>('input[type="radio"]')?.focus(); }, []);

  async function complete(choice: StartAction | null) {
    setBusy(true); setError(null);
    try { await onComplete(choice, experience); }
    catch (e) { setError(errorMessage(e)); setBusy(false); }
  }

  return <div className="scrim" role="presentation">
    <form ref={form} className="dialog welcome-dialog" role="dialog" aria-modal="true" aria-labelledby="welcome-title" onSubmit={e => { e.preventDefault(); complete(action); }} onKeyDown={e => {
      if (e.key === "Escape" && !busy) { e.stopPropagation(); complete(null); }
      if (e.key === "Tab") {
        const els = [...(form.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled)') ?? [])].filter(el => el.getClientRects().length);
        if (e.shiftKey && document.activeElement === els[0]) { e.preventDefault(); els.at(-1)?.focus(); }
        else if (!e.shiftKey && document.activeElement === els.at(-1)) { e.preventDefault(); els[0]?.focus(); }
      }
    }}>
      <header className="dialog-header"><h2 id="welcome-title" className="k-subtitle">{t("Welcome to Kernest")}</h2><button type="button" className="icon-btn" aria-label={t("Close")} disabled={busy} onClick={() => complete(null)}><CloseIcon /></button></header>
      <div className="welcome-shell">
        <aside className="welcome-intro">
          <img src={logo} alt={t("Kernest logo")} className="welcome-logo" />
          <h3>{t("Welcome to Kernest")}</h3>
          <p className="welcome-promise">{t("Design, space and ship fonts — from a first glyph to a variable family. Free and open source. Works offline. No account.")}</p>
          <ul className="welcome-features">
            <li><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7"><path d="m4 17-1 4 4-1L20 7l-3-3L4 17ZM14 7l3 3" /></svg>{t("Draw with precise Bézier tools")}</li>
            <li><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7"><path d="M4 12h16M8 8l-4 4 4 4m8-8 4 4-4 4" /></svg>{t("Space and kern in real words")}</li>
            <li><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7"><rect x="4" y="4" width="11" height="11" rx="1" /><path d="M9 9h11v11H9Z" /></svg>{t("Interpolate masters into families")}</li>
            <li><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7"><path d="M12 16V3m-4 4 4-4 4 4M4 13v7h16v-7" /></svg>{t("Export OTF, TTF, WOFF2 and variable fonts")}</li>
          </ul>
        </aside>
        <div className="welcome-main">
          <div className="welcome-content">
            <fieldset className="welcome-experience"><legend className="k-subtitle">{t("What describes you best?")}</legend>
              <div className="welcome-experience-options">
                <label className={`wizard-mode${experience === "beginner" ? " is-selected" : ""}`}><span><input type="radio" name="experience" value="beginner" checked={experience === "beginner"} onChange={() => setExperience("beginner")} />{t("New to type design")}</span><small>{t("Guided tour and extra hints")}</small></label>
                <label className={`wizard-mode${experience === "experienced" ? " is-selected" : ""}`}><span><input type="radio" name="experience" value="experienced" checked={experience === "experienced"} onChange={() => setExperience("experienced")} />{t("I've used other font editors")}</span><small>{t("Skip the tour and start working")}</small></label>
              </div>
            </fieldset>
            <h3 className="k-subtitle">{t("How do you want to start?")}</h3>
            <div className="welcome-choices">
              <button type="button" className={`start-choice${action === "new" ? " is-selected" : ""}`} aria-pressed={action === "new"} onClick={() => setAction("new")}><span className="start-choice-icon"><AddIcon /></span><strong>{t("Start a new font")}</strong><small>{t("Guided setup: name, scripts and masters")}</small></button>
              <button type="button" className={`start-choice${action === "open" ? " is-selected" : ""}`} aria-pressed={action === "open"} onClick={() => setAction("open")}><span className="start-choice-icon"><FolderIcon /></span><strong>{t("Open an existing font")}</strong><small>{t("Open a .kernest font")}</small></button>
              <button type="button" className={`start-choice${action === "sample" ? " is-selected" : ""}`} aria-pressed={action === "sample"} onClick={() => setAction("sample")}><span className="start-choice-icon"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6"><path d="m2 9 10-5 10 5-10 5-10-5Zm4 3v6l6 3 6-3v-6M22 9v8" /></svg></span><strong>{t("Explore a sample font")}</strong><small>{t("Kernest Sample with six short lessons")}</small></button>
              <button type="button" className={`start-choice${action === "import" ? " is-selected" : ""}`} aria-pressed={action === "import"} onClick={() => setAction("import")}><span className="start-choice-icon"><svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6"><path d="M12 3v13m-4-4 4 4 4-4M4 16v5h16v-5" /></svg></span><strong>{t("Import from another app")}</strong><small>{t("Glyphs, UFO, designspace or compiled fonts")}</small></button>
            </div>
            {error && <p role="alert" className="notice notice-danger">{error}</p>}
          </div>
          <footer className="wizard-footer"><label className="check-row" title={t("You can restart the tour from Help at any time.")}><input type="checkbox" checked={atLaunch} onChange={(e) => { setAtLaunch(e.target.checked); setShowStartAtLaunch(e.target.checked); }} /><span className="k-body">{t("Show Start Window at launch")}</span></label><span className="spacer" /><button type="button" className="btn btn-link" disabled={busy} onClick={() => complete(null)}>{t("Skip")}</button><button type="submit" className="btn btn-primary" disabled={busy}>{busy ? t("Opening…") : `${t("Continue")} →`}</button></footer>
        </div>
      </div>
    </form>
  </div>;
}
