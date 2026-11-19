import { useEffect, useState } from "react";
import { errorMessage, type Milestone, type Onboarding } from "../lib/backend";
import { t } from "../lib/i18n";

const STEPS = [
  { title: "Double-click a glyph to start drawing", text: "Every glyph lives in Font View. Open a glyph to work on its outline.", target: '.tab-panel:not([hidden]) .glyph-cell[title^="n —"], .tab-panel:not([hidden]) .glyph-cell', view: "Font View" },
  { title: "Draw your first glyph", text: "Choose the Rectangle or Pen tool and draw on the canvas. Use the selection tool to move points.", target: ".tab-panel:not([hidden]) .editor .toolbar", view: "Edit View" },
  { title: "Check spacing in real words", text: "Type a test string, click a glyph and change its sidebearings or width in the bottom bar.", target: ".tab-panel:not([hidden]) .textview .text-input", view: "Text View" },
  { title: "Kern a pair", text: "Enter two glyph names, such as A and V, and a kerning value. Add the pair to see it in text.", target: ".tab-panel:not([hidden]) .kern-table .add-row", view: "Kerning" },
  { title: "Add an instance and preview it", text: "Set up a named instance in Font Info. Then choose it in Text View's preview selector.", target: ".tab-panel:not([hidden]) #fi-instances", view: "Instances" },
  { title: "Export your first font", text: "Choose a format and destination, then export. Check font reports any problems in the exported files.", target: ".export-dialog .dialog-footer", view: "Export" },
];
const CHECKLIST: { id: Milestone; label: string; step: number }[] = [
  { id: "created", label: "Create or open a font", step: 0 },
  { id: "drawn", label: "Draw your first glyph", step: 1 },
  { id: "spaced", label: "Space glyphs in Text View", step: 2 },
  { id: "kerned", label: "Kern a pair", step: 3 },
  { id: "instance", label: "Add an instance and preview it", step: 4 },
  { id: "exported", label: "Export your font", step: 5 },
];
interface Box { left: number; top: number; width: number; height: number; viewportWidth: number; viewportHeight: number }

export function GuidedTour({ onboarding, onChange, onNavigate }: { onboarding: Onboarding; onChange: (next: Onboarding) => Promise<void>; onNavigate: (step: number) => void }) {
  const step = STEPS[onboarding.tourStep];
  const [box, setBox] = useState<Box | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let observed: Element | null = null;
    const resize = new ResizeObserver(() => measure());
    const measure = () => {
      const element = document.querySelector(step.target);
      if (element !== observed) { resize.disconnect(); if (element) resize.observe(element); observed = element; }
      if (!element) { setBox(null); return; }
      let r = element.getBoundingClientRect();
      if (r.top > window.innerHeight
        || r.bottom < 0) { element.scrollIntoView({ block: "nearest" }); r = element.getBoundingClientRect(); }
      const next = { left: r.left, top: r.top, width: r.width, height: r.height, viewportWidth: window.innerWidth, viewportHeight: window.innerHeight };
      setBox(old => old && Object.keys(next).every(key => old[key as keyof Box] === next[key as keyof Box]) ? old : next);
    };
    measure();
    const changes = new MutationObserver(measure);
    changes.observe(document.body, { childList: true, subtree: true });
    window.addEventListener("resize", measure);
    window.addEventListener("scroll", measure, true);
    return () => { changes.disconnect(); resize.disconnect(); window.removeEventListener("resize", measure); window.removeEventListener("scroll", measure, true); };
  }, [step]);

  const go = async (next: number) => {
    setBusy(true); setError(null);
    try { onNavigate(next); await onChange({ ...onboarding, tourStep: next }); }
    catch (e) { setError(errorMessage(e)); }
    finally { setBusy(false); }
  };
  const finish = async () => {
    setBusy(true);
    try { await onChange({ ...onboarding, tourEnabled: false }); }
    catch (e) { setError(errorMessage(e)); setBusy(false); }
  };
  const rtl = document.documentElement.dir === "rtl";
  const vw = box?.viewportWidth ?? window.innerWidth, vh = box?.viewportHeight ?? window.innerHeight;
  const left = box && !rtl && box.left + box.width + 352 < vw ? box.left + box.width + 16 : box && rtl && box.left > 352 ? box.left - 336 : rtl ? 16 : vw - 336;
  const top = box && box.height < 160 ? Math.max(16, Math.min(box.top + box.height + 16, vh - 280)) : Math.max(16, vh - 310);
  return <div className="tour-layer">
    {box && box.width > 0 && <div className="tour-focus" aria-hidden="true" style={{ left: box.left - 4, top: box.top - 4, width: box.width + 8, height: box.height + 8 }} />}
    <aside className="tour-checklist" aria-labelledby="checklist-title">
      <header><h3 id="checklist-title" className="k-subtitle">{t("Getting started")}</h3><span className="k-caption">{onboarding.completed.length} / 6</span></header>
      <progress max={6} value={onboarding.completed.length} aria-label={t("Getting-started progress")} />
      <ul>{CHECKLIST.map(item => {
        const done = onboarding.completed.includes(item.id);
        return <li key={item.id} className={done ? "is-complete" : ""}><span className="checklist-mark" aria-label={done ? t("Completed") : t("Not completed")}>{done && <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="m5 12 4 4L19 6" /></svg>}</span><span>{t(item.label)}</span>{!done && <button type="button" className="btn btn-link" disabled={busy} onClick={() => go(item.step)}>{t("Show me")}</button>}</li>;
      })}</ul>
    </aside>
    <section className="tour-card" aria-labelledby="tour-title" style={{ left, top }}>
      <p className="k-caption muted">{t("Step {step} of 6 · {view}", { step: onboarding.tourStep + 1, view: t(step.view) })}</p>
      <h3 id="tour-title" className="k-subtitle">{t(step.title)}</h3>
      <p>{t(step.text)}</p>
      <div className="tour-dots" aria-hidden="true">{STEPS.map((_, i) => <span key={i} className={i === onboarding.tourStep ? "is-current" : ""} />)}</div>
      {error && <p role="alert" className="notice notice-danger">{error}</p>}
      <footer><button type="button" className="btn btn-link" disabled={busy} onClick={finish}>{t("End tour")}</button><span className="spacer" />{!box && <button type="button" className="btn btn-secondary btn-sm" disabled={busy} onClick={() => go(onboarding.tourStep)}>{t("Show me")}</button>}<button type="button" className="btn btn-primary btn-sm" disabled={busy} onClick={() => onboarding.tourStep === 5 ? finish() : go(onboarding.tourStep + 1)}>{onboarding.tourStep === 5 ? t("Finish tour") : t("Next")}</button></footer>
    </section>
  </div>;
}
