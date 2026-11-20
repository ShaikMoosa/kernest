import { useEffect, useMemo, useRef, useState } from "react";
import { errorMessage, type Axis, type GlyphSetInfo, type Metrics, type NewFontSetup } from "../lib/backend";
import { familyLayout, suggestedInstances, type FamilyMode } from "../lib/onboarding";
import { CloseIcon } from "./Icons";
import { t, tn } from "../lib/i18n";

interface Props {
  sets: GlyphSetInfo[];
  onCancel: () => void;
  onCreate: (setup: NewFontSetup) => Promise<void>;
}

const STEP_LABELS = ["Family & metrics", "Scripts & glyph sets", "Masters & axes", "Review & create"];

export function NewFontDialog({ sets, onCancel, onCreate }: Props) {
  const [step, setStep] = useState(0);
  const [name, setName] = useState("Untitled Sans");
  const [upm, setUpm] = useState(1000);
  const [metrics, setMetrics] = useState<Metrics>({ ascender: 800, capHeight: 700, xHeight: 500, descender: -200, italicAngle: 0 });
  const [chosen, setChosen] = useState<string[]>(["latin-basic"]);
  const [mode, setMode] = useState<FamilyMode>("single");
  const [layout, setLayout] = useState(() => familyLayout("single"));
  const [selected, setSelected] = useState<string[]>(["Regular"]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const form = useRef<HTMLFormElement>(null);
  const suggestions = useMemo(() => suggestedInstances(layout.axes), [layout.axes]);
  const instances = suggestions.filter(i => selected.includes(i.name));
  const chosenSets = sets.filter(s => chosen.includes(s.id));
  const total = chosenSets.reduce((n, s) => n + s.count, 0);
  const setsSummary = chosenSets.length <= 2 && chosenSets.length > 0
    ? `${chosenSets.map(s => t(s.name)).join(" + ")} · ${tn(total, "{n} glyph", "{n} glyphs")}`
    : `${tn(chosen.length, "{n} glyph set", "{n} glyph sets")} · ${tn(total, "{n} glyph", "{n} glyphs")}`;

  useEffect(() => {
    form.current?.querySelector<HTMLInputElement>("input")?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !busy) { e.stopPropagation(); onCancel(); }
      if (e.key !== "Tab") return;
      const focusable = [...(form.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled)') ?? [])].filter(el => el.getClientRects().length);
      const first = focusable[0], last = focusable.at(-1);
      if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last?.focus(); }
      if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first?.focus(); }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onCancel, busy]);

  function selectMode(next: FamilyMode) {
    setMode(next);
    const nextLayout = next === "custom" ? layout : familyLayout(next);
    setLayout(nextLayout);
    setSelected(suggestedInstances(nextLayout.axes).filter(i => !["Thin", "ExtraLight", "ExtraBold", "Black"].includes(i.name)).map(i => i.name));
    setError(null);
  }

  function updateAxis(index: number, key: keyof Axis, value: string | number) {
    setLayout(old => {
      const previous = old.axes[index];
      const axis = { ...previous, [key]: value };
      if (key === "min" && mode !== "custom") axis.default = Number(value);
      return { axes: old.axes.map((a, i) => i === index ? axis : a), masters: old.masters.map(m => {
        const location = { ...m.location };
        const before = location[previous.tag];
        if (key === "tag") { delete location[previous.tag]; location[axis.tag] = before; }
        else if (mode !== "custom") {
          if (before === previous.min) location[axis.tag] = axis.min;
          else if (before === previous.max) location[axis.tag] = axis.max;
        }
        return { ...m, location };
      }) };
    });
  }

  function validateStep(): string | null {
    if (!name.trim()) return t("Enter a family name.");
    if (upm < 16 || upm > 16384) return t("Units per em must be between 16 and 16384.");
    const invalidMetrics = metrics.descender > 0
      || metrics.xHeight <= 0
      || metrics.xHeight > metrics.capHeight || metrics.capHeight > metrics.ascender;
    if (invalidMetrics) return t("Use descender ≤ 0 < x-height ≤ cap height ≤ ascender.");
    if (step >= 2 && layout.axes.some(a => !a.name.trim() || a.tag.length !== 4 || a.min > a.default || a.default > a.max)) return t("Give every axis a name, a four-character tag and an ordered range.");
    return null;
  }

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    const invalid = validateStep();
    if (invalid) { setError(invalid); return; }
    setError(null);
    if (step < 3) { setStep(step + 1); return; }
    setBusy(true);
    try { await onCreate({ familyName: name.trim(), unitsPerEm: upm, metrics, glyphSets: chosen, axes: layout.axes, masters: layout.masters, instances }); }
    catch (err) { setError(errorMessage(err)); setBusy(false); }
  }

  const numberField = (label: string, value: number, change: (n: number) => void, min?: number, max?: number) => (
    <label className="field"><span className="k-caption muted">{t(label)}</span><input type="number" required value={value} min={min} max={max} aria-label={t(label)} onChange={e => change(Number(e.target.value))} /></label>
  );

  return (
    <div className="scrim" role="presentation">
      <form ref={form} className="dialog wizard-dialog" role="dialog" aria-modal="true" aria-labelledby="new-font-title" onSubmit={submit}>
        <header className="dialog-header"><h2 id="new-font-title" className="k-subtitle">{t("New Font — Setup")}</h2><button type="button" className="icon-btn" aria-label={t("Close")} disabled={busy} onClick={onCancel}><CloseIcon /></button></header>
        <div className="wizard-shell">
          <nav className="wizard-steps" aria-label={t("Font setup steps")}>
            {STEP_LABELS.map((label, i) => <button key={label} type="button" className={`wizard-step${step === i ? " is-current" : ""}`} aria-current={step === i ? "step" : undefined} disabled={busy || i > step} onClick={() => { setStep(i); setError(null); }}>
              <span className={`wizard-step-number${i < step ? " is-done" : ""}`}>{i < step ? <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="m5 12 4 4L19 6" /></svg> : i + 1}</span>
              <span><strong>{t(label)}</strong>{i === 0 && step > 0 && <small>{name} · {upm} UPM</small>}{i === 1 && step > 1 && <small>{setsSummary}</small>}{i === 2 && step > 2 && <small>{tn(layout.masters.length, "{n} master", "{n} masters")} · {tn(instances.length, "{n} instance", "{n} instances")}</small>}</span>
            </button>)}
          </nav>
          <div className="wizard-main">
            <div className="wizard-content">
              <h3 className="k-title" tabIndex={-1}>{t(STEP_LABELS[step])}</h3>
              {step === 0 && <>
                <p className="muted">{t("Choose a family name and the vertical proportions of your font.")}</p>
                <label className="field"><span className="k-label">{t("Family name")}</span><input value={name} required maxLength={64} aria-label={t("Family name")} onChange={e => setName(e.target.value)} /></label>
                <div className="wizard-metrics">
                  {numberField("Units per em", upm, setUpm, 16, 16384)}
                  {numberField("x-height", metrics.xHeight, n => setMetrics(m => ({ ...m, xHeight: n })))}
                  {numberField("Cap height", metrics.capHeight, n => setMetrics(m => ({ ...m, capHeight: n })))}
                  {numberField("Ascender", metrics.ascender, n => setMetrics(m => ({ ...m, ascender: n })))}
                  {numberField("Descender", metrics.descender, n => setMetrics(m => ({ ...m, descender: n })))}
                </div>
                <p className="notice k-caption">{t("Nothing is created until you finish the review. You can change these values later in Font Info.")}</p>
              </>}
              {step === 1 && <>
                <p className="muted">{t("Pick the scripts and glyph sets you want to start drawing. You can add more later.")}</p>
                <fieldset className="wizard-sets"><legend className="sr-only">{t("Start with glyph sets")}</legend>{sets.map(s => <label key={s.id} className="check-row"><input type="checkbox" checked={chosen.includes(s.id)} onChange={() => setChosen(c => c.includes(s.id) ? c.filter(x => x !== s.id) : [...c, s.id])} /><span>{t(s.name)}</span><span className="spacer" /><span className="k-caption muted">{tn(s.count, "{n} glyph", "{n} glyphs")}</span></label>)}</fieldset>
                <section className="wizard-sample" aria-label={t("Preview")}>
                  <h4 className="k-label muted">{t("Preview")}</h4>
                  <p dir="auto">{chosenSets.map(s => s.sample).join(" ") || <span className="muted k-body">{t("Pick a glyph set to see its characters.")}</span>}</p>
                </section>
                <p className="k-caption muted">{tn(total, "Total: up to {n} glyph; overlapping sets are combined.", "Total: up to {n} glyphs; overlapping sets are combined.")}</p>
              </>}
              {step === 2 && <>
                <p className="muted">{t("How will this family vary? You can change everything later in Font Info.")}</p>
                <div className="wizard-modes">{([["single", "Single style", "1 master"], ["weight", "Weight", "Light to Bold · 2 masters"], ["weight-width", "Weight × Width", "4 masters"], ["custom", "Custom", "Define axes yourself"]] as const).map(([id, label, caption]) => <label key={id} className={`wizard-mode${mode === id ? " is-selected" : ""}`}><span><input type="radio" name="family-mode" value={id} checked={mode === id} onChange={() => selectMode(id)} />{t(label)}</span><small>{t(caption)}</small></label>)}</div>
                <div className="wizard-designspace">
                  <div>
                    {layout.axes.length > 0 && <h4 className="k-label">{tn(layout.axes.length, "Axis", "Axes")}</h4>}
                    {layout.axes.map((axis, i) => <div className="wizard-axis" key={i}>
                      <label className="field"><span className="k-caption muted">{t("Name")}</span><input aria-label={t("Axis name")} value={axis.name} required onChange={e => updateAxis(i, "name", e.target.value)} /></label>
                      <label className="field"><span className="k-caption muted">{t("Tag")}</span><input aria-label={t("Axis tag")} className="k-code" value={axis.tag} required minLength={4} maxLength={4} onChange={e => updateAxis(i, "tag", e.target.value)} /></label>
                      {numberField("Min", axis.min, n => updateAxis(i, "min", n))}{numberField("Default", axis.default, n => updateAxis(i, "default", n))}{numberField("Max", axis.max, n => updateAxis(i, "max", n))}
                      {mode === "custom" && <button className="icon-btn" type="button" aria-label={t("Remove axis")} onClick={() => setLayout(l => ({ axes: l.axes.filter((_, n) => i !== n), masters: l.masters.map(m => ({ ...m, location: Object.fromEntries(Object.entries(m.location).filter(([tag]) => tag !== axis.tag)) })) }))}><CloseIcon size={12} /></button>}
                    </div>)}
                    {mode === "custom" && <button type="button" className="btn btn-secondary btn-sm" onClick={() => setLayout(l => { const tag = `AX${String(l.axes.length + 1).padStart(2, "0")}`; return { axes: [...l.axes, { name: "Custom", tag, min: 0, default: 0, max: 1 }], masters: l.masters.map(m => ({ ...m, location: { ...m.location, [tag]: 0 } })) }; })}>{t("Add axis")}</button>}
                    <h4 className="k-label">{t("Masters")}</h4>
                    <div className="wizard-masters">{layout.masters.map((m, i) => <div key={i} className="wizard-master">
                      <input aria-label={t("Master name")} value={m.name} required onChange={e => setLayout(l => ({ ...l, masters: l.masters.map((old, n) => n === i ? { ...old, name: e.target.value } : old) }))} />
                      {layout.axes.map(a => <label key={a.tag} className="wizard-location"><span className="k-caption muted">{a.tag}</span><input type="number" step="any" min={a.min} max={a.max} aria-label={`${m.name} ${a.tag}`} value={m.location[a.tag] ?? a.default} onChange={e => { setMode("custom"); setLayout(l => ({ ...l, masters: l.masters.map((old, n) => i === n ? { ...old, location: { ...old.location, [a.tag]: Number(e.target.value) } } : old) })); }} /></label>)}
                      {mode === "custom" && <button type="button" className="icon-btn" disabled={layout.masters.length === 1} aria-label={t("Remove master")} onClick={() => setLayout(l => ({ ...l, masters: l.masters.filter((_, n) => n !== i) }))}><CloseIcon size={12} /></button>}
                    </div>)}</div>
                    {mode === "custom" && <button type="button" className="btn btn-secondary btn-sm" onClick={() => setLayout(l => ({ ...l, masters: [...l.masters, { name: `Master ${l.masters.length + 1}`, location: Object.fromEntries(l.axes.map(a => [a.tag, a.default])) }] }))}>{t("Add master")}</button>}
                  </div>
                  {layout.axes.length > 0 && <aside className="wizard-preview" aria-label={t("Master locations")}>
                    <h4 className="k-label">{t("Designspace")}</h4>
                    {layout.axes.length === 1 ? (() => {
                      const a = layout.axes[0];
                      const x = (v: number) => 12 + (v - a.min) / (a.max - a.min || 1) * 216;
                      return <svg viewBox="0 0 240 56" role="img" aria-label={t("Master locations")}>
                        <path d="M12 20H228" stroke="currentColor" opacity="0.25" />
                        {instances.map(i => <circle key={i.name} className="wizard-dot-instance" cx={x(i.location[a.tag] ?? a.default)} cy="20" r="5"><title>{i.name}</title></circle>)}
                        {layout.masters.map((m, i) => <circle key={i} className="wizard-dot-master" cx={x(m.location[a.tag] ?? a.default)} cy="20" r="7"><title>{m.name}</title></circle>)}
                        <text x="12" y="48" className="wizard-dot-label">{a.min}</text>
                        <text x="228" y="48" textAnchor="end" className="wizard-dot-label">{a.max}</text>
                      </svg>;
                    })() : <svg viewBox="0 0 240 120" role="img" aria-label={t("Master locations")}><path d="M16 92H224 M16 92V16" fill="none" stroke="currentColor" opacity="0.25" />{[...instances.map(i => ({ name: i.name, location: i.location, master: false })), ...layout.masters.map(m => ({ ...m, master: true }))].map((m, i) => { const x = layout.axes[0], y = layout.axes[1]; return <circle key={i} className={m.master ? "wizard-dot-master" : "wizard-dot-instance"} cx={16 + ((m.location[x.tag] ?? x.default) - x.min) / (x.max - x.min || 1) * 208} cy={y ? 92 - ((m.location[y.tag] ?? y.default) - y.min) / (y.max - y.min || 1) * 76 : 54} r={m.master ? 6 : 4}><title>{m.name}</title></circle>; })}</svg>}
                    {instances.length > 0 && <ul className="wizard-instance-names">{[instances[0], instances[Math.floor((instances.length - 1) / 2)], instances.at(-1)!].filter((v, n, all) => all.indexOf(v) === n).map(i => <li key={i.name} style={{ fontWeight: Math.max(100, Math.min(900, i.location.wght ?? 400)) }}>{name} {i.name}</li>)}</ul>}
                    <p className="k-caption muted">{t("Filled dots are masters, open dots are instances. One master must sit at the default of every axis.")}</p>
                  </aside>}
                </div>
                <h4 className="k-label">{t("Suggested instances")}</h4>
                <div className="wizard-instances">{suggestions.map(i => <label key={i.name} className={`wizard-instance${selected.includes(i.name) ? " is-selected" : ""}`}><input type="checkbox" checked={selected.includes(i.name)} onChange={() => setSelected(s => s.includes(i.name) ? s.filter(n => n !== i.name) : [...s, i.name])} />{i.name}</label>)}</div>
              </>}
              {step === 3 && <>
                <p className="muted">{t("Review your starting point. The font opens as an unsaved document.")}</p>
                <dl className="wizard-review"><dt>{t("Family name")}</dt><dd>{name}</dd><dt>{t("Units per em")}</dt><dd>{upm}</dd><dt>{t("Glyph sets")}</dt><dd>{sets.filter(s => chosen.includes(s.id)).map(s => t(s.name)).join(", ") || t("None")}</dd><dt>{t("Masters")}</dt><dd>{layout.masters.map(m => m.name).join(", ")}</dd><dt>{t("Axes")}</dt><dd>{layout.axes.map(a => `${a.name} (${a.tag} ${a.min}–${a.max})`).join(", ") || t("None")}</dd><dt>{t("Instances")}</dt><dd>{instances.map(i => i.name).join(", ") || t("One static font per master")}</dd></dl>
                <p className="notice k-caption">{t("Glyphs start empty. The guided tour helps you draw, space, kern and export your first font.")}</p>
              </>}
              {error && <p role="alert" className="notice notice-danger k-label">{error}</p>}
            </div>
            <footer className="wizard-footer"><span className="k-caption muted">{t("Step {step} of 4", { step: step + 1 })}</span><span className="spacer" />{step > 0 && <button type="button" className="btn btn-secondary" disabled={busy} onClick={() => { setStep(step - 1); setError(null); }}>← {t("Back")}</button>}<button type="submit" className="btn btn-primary" disabled={busy}>{busy ? t("Creating font…") : step === 3 ? t("Create Font") : step === 2 ? `${t("Next: Review")} →` : `${t("Next")} →`}</button></footer>
          </div>
        </div>
      </form>
    </div>
  );
}
