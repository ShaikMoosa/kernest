import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import { backend, type Axis, type DocStatus, type FontInfo, type Instance, type Location, type MasterView, type Metrics } from "../lib/backend";
import { AddIcon, CloseIcon } from "./Icons";
import { FeaturesSection } from "./FeaturesSection";
import { t, tn, translateMessage } from "../lib/i18n";

const AXIS_PRESETS: Axis[] = [
  { name: "Weight", tag: "wght", min: 100, default: 400, max: 900 },
  { name: "Width", tag: "wdth", min: 75, default: 100, max: 125 },
  { name: "Optical size", tag: "opsz", min: 8, default: 14, max: 72 },
  { name: "Slant", tag: "slnt", min: -12, default: 0, max: 0 },
];

const num = (s: string) => Number(s.trim().replace("−", "-"));

/** One palette colour: a swatch that opens the colour picker, and the #rrggbbaa text. */
function ColorCell({ value, label, onCommit }: { value: string; label: string; onCommit: (v: string) => unknown }) {
  const ref = useRef<HTMLInputElement>(null);
  const alpha = value.length === 9 ? value.slice(7) : "ff";
  // The native "change" event fires once the picker closes (React's onChange fires while dragging).
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const done = () => onCommit(el.value + alpha);
    el.addEventListener("change", done);
    return () => el.removeEventListener("change", done);
  }, [alpha, onCommit]);
  return (
    <span className="color-cell">
      <input ref={ref} type="color" className="color-swatch" aria-label={label} defaultValue={value.slice(0, 7)} key={value} />
      <Field label={label} value={value} width={96} numeric={false} onCommit={(v) => {
        const hex = v.trim().toLowerCase();
        if (!/^#([0-9a-f]{6}|[0-9a-f]{8})$/.test(hex)) { report(label); return false; }
        return onCommit(hex);
      }} />
    </span>
  );
}

let report: (label: string) => void = () => {};

/** Where number fields report input that isn't a number. */
const FieldError = createContext<(message: string) => void>(() => {});

/** A text/number input that commits on Enter or blur and reverts on Escape. */
function Field({ value, onCommit, width = 72, mono = true, label, numeric = true, placeholder }: {
  /** Returning (a promise of) false means the value was refused: the field shows the old one again. */
  value: string | number; onCommit: (v: string) => unknown; width?: number; mono?: boolean; label: string; numeric?: boolean;
  /** Shown when empty; a numeric field with a placeholder accepts empty (meaning automatic). */
  placeholder?: string;
}) {
  const shown = String(value);
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);
  const report = useContext(FieldError);
  const commit = (v: string) => {
    if (v === shown) return;
    // Number fields only take numbers: say so instead of sending garbage.
    if (numeric && v.trim() !== "" && !Number.isFinite(num(v))) {
      setText(shown);
      return report(t("{name}: enter a number.", { name: label }));
    }
    if (numeric && v.trim() === "" && placeholder === undefined) {
      setText(shown);
      return report(t("{name}: enter a number.", { name: label }));
    }
    Promise.resolve(onCommit(v)).then((ok) => { if (ok === false) setText(shown); }, () => setText(shown));
  };
  return (
    <input
      aria-label={label}
      className={`metric-input${mono ? "" : " text-field"}`}
      style={{ width }}
      value={text}
      placeholder={placeholder}
      inputMode={numeric ? "decimal" : undefined}
      onChange={(e) => setText(e.target.value)}
      onBlur={(e) => commit(e.currentTarget.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
        if (e.key === "Escape") { setText(shown); (e.target as HTMLInputElement).blur(); }
      }}
    />
  );
}

interface Props {
  version: number;
  onChanged: (status: DocStatus) => void;
  onError: (e: unknown) => void;
}

/** S10/S11 · Font Info: general info, axes, masters and instances. */
export function FontInfoView({ version, onChanged, onError }: Props) {
  const [info, setInfo] = useState<FontInfo | null>(null);
  // Drafts while editing axes or instances. A ref mirrors them so commits that land
  // in the same instant (blur, then click Apply) always build on the latest draft.
  const [axes, setAxesState] = useState<Axis[] | null>(null);
  const [instances, setInstState] = useState<Instance[] | null>(null);
  const drafts = useRef<{ axes: Axis[] | null; instances: Instance[] | null }>({ axes: null, instances: null });
  const setAxes = (v: Axis[] | null) => { drafts.current.axes = v; setAxesState(v); };
  const setInstances = (v: Instance[] | null) => { drafts.current.instances = v; setInstState(v); };
  const [adding, setAdding] = useState<{ name: string; copyFrom: string; location: Location } | null>(null);

  const reload = useCallback(() => backend.fontInfo().then((i) => { setInfo(i); }, onError), [onError]);
  useEffect(() => { reload(); }, [reload, version]);

  const apply = async (task: () => Promise<FontInfo>) => {
    try {
      const next = await task();
      setInfo(next);
      onChanged(next.status);
      return true;
    } catch (e) {
      onError(e);
      return false;
    }
  };

  report = (label) => onError(t("{name}: enter a colour such as #ff8800.", { name: label }));
  if (!info) return <div className="fontinfo"><p className="k-caption muted editor-loading">{t("Loading Font Info…")}</p></div>;

  const axesDraft = axes ?? info.axes;
  const curAxes = () => drafts.current.axes ?? info.axes;
  const curInst = () => drafts.current.instances ?? info.instances;
  const instDraft = instances ?? info.instances;
  const locFields = (loc: Location, onChange: (loc: Location) => void, prefix: string) =>
    info.axes.map((a) => (
      <label key={a.tag} className="info-field">
        <span className="k-caption muted">{a.tag}</span>
        <Field label={`${prefix} ${a.name}`} value={loc[a.tag] ?? a.default} onCommit={(v) => onChange({ ...loc, [a.tag]: num(v) })} />
      </label>
    ));
  const updateMaster = (m: MasterView, patch: Partial<{ name: string; location: Location; metrics: Metrics }>) =>
    apply(() => backend.updateMaster(m.id, patch.name ?? m.name, patch.location ?? m.location, patch.metrics ?? m.metrics));

  return (
    <FieldError.Provider value={onError}>
    <div className="fontinfo">
      <div className="fontinfo-scroll">
        {(info.warnings.length > 0 || info.incompatible > 0) && (
          <div className="fi-warnings" role="status">
            {info.warnings.map((w) => <p key={w} className="notice notice-warning k-label">⚠ {translateMessage(w)}</p>)}
            {info.incompatible > 0 && (
              <p className="notice notice-warning k-label">⚠ {tn(info.incompatible, "{n} glyph can't be interpolated. See the Incompatible filter in Font View.", "{n} glyphs can't be interpolated. See the Incompatible filter in Font View.")}</p>
            )}
          </div>
        )}

        <section className="fi-section" aria-labelledby="fi-general">
          <h2 id="fi-general" className="k-subtitle">{t("General")}</h2>
          <div className="fi-row">
            <label className="info-field"><span className="k-caption muted">{t("Family name")}</span>
              <Field label={t("Family name")} value={info.familyName} width={260} mono={false} numeric={false}
                onCommit={(v) => apply(() => backend.setFontInfo(v, info.unitsPerEm, info.versionMajor, info.versionMinor))} />
            </label>
            <label className="info-field"><span className="k-caption muted">{t("Units per em")}</span>
              <Field label={t("Units per em")} value={info.unitsPerEm} onCommit={(v) => {
                const upm = num(v);
                if (!Number.isInteger(upm) || upm < 16 || upm > 16384) { onError(t("Units per em must be a whole number between 16 and 16384.")); return false; }
                return apply(() => backend.setFontInfo(info.familyName, upm, info.versionMajor, info.versionMinor));
              }} />
            </label>
            <label className="info-field"><span className="k-caption muted">{t("Version")}</span>
              <Field label={t("Version")} value={`${info.versionMajor}.${String(info.versionMinor).padStart(3, "0")}`} numeric={false}
                onCommit={(v) => {
                  const m = v.trim().match(/^(\d+)(?:\.(\d+))?$/);
                  if (!m) { onError(t("Write the version as major.minor, e.g. 1.002.")); return false; }
                  return apply(() => backend.setFontInfo(info.familyName, info.unitsPerEm, Number(m[1]), Number(m[2] ?? 0)));
                }} />
            </label>
          </div>
          <div className="fi-row">
            {([
              ["designer", "Designer", 200],
              ["designerUrl", "Designer URL", 240],
              ["manufacturer", "Manufacturer", 200],
              ["manufacturerUrl", "Manufacturer URL", 240],
              ["copyright", "Copyright", 280],
              ["vendorId", "Vendor ID", 96],
              ["license", "License", 280],
              ["licenseUrl", "License URL", 240],
              ["trademark", "Trademark", 240],
              ["description", "Description", 320],
              ["sampleText", "Sample text", 320],
            ] as const).map(([key, en, width]) => { const label = t(en); return (
              <label key={key} className="info-field"><span className="k-caption muted">{label}</span>
                <Field label={label} value={info.names[key]} width={width} mono={false} numeric={false}
                  onCommit={(v) => apply(() => backend.setFontNames({ ...info.names, [key]: v }))} />
              </label>
            ); })}
          </div>
          <p className="k-caption muted">{t("These names go into the exported font's name table. Leave a field empty to leave it out.")}</p>
        </section>

        <section className="fi-section" aria-labelledby="fi-axes">
          <div className="fi-head">
            <h2 id="fi-axes" className="k-subtitle">{t("Axes")}</h2>
            <span className="spacer" />
            <select className="select" value="" aria-label={t("Add axis")} onChange={(e) => {
              const p = e.target.value === "custom" ? { name: t("Custom"), tag: "XXXX", min: 0, default: 0, max: 100 } : AXIS_PRESETS.find((a) => a.tag === e.target.value);
              if (p) setAxes([...curAxes(), { ...p, name: t(p.name) }]);
            }}>
              <option value="">{t("Add axis…")}</option>
              {AXIS_PRESETS.filter((p) => !axesDraft.some((a) => a.tag === p.tag)).map((p) => <option key={p.tag} value={p.tag}>{t(p.name)} ({p.tag})</option>)}
              <option value="custom">{t("Custom axis")}</option>
            </select>
          </div>
          {axesDraft.length === 0 ? (
            <p className="k-caption muted">{t("No axes. Add one to make a variable family with several masters.")}</p>
          ) : (
            <table className="fi-table">
              <thead><tr><th>{t("Name")}</th><th>{t("Tag")}</th><th className="num">{t("Minimum")}</th><th className="num">{t("Default")}</th><th className="num">{t("Maximum")}</th><th aria-label={t("Actions")} /></tr></thead>
              <tbody>
                {axesDraft.map((a, i) => {
                  const set = (patch: Partial<Axis>) => setAxes(curAxes().map((x, j) => (j === i ? { ...x, ...patch } : x)));
                  return (
                    <tr key={i}>
                      <td><Field label={t("Axis name")} value={a.name} width={160} mono={false} numeric={false} onCommit={(v) => set({ name: v })} /></td>
                      <td><Field label={t("Axis tag")} value={a.tag} width={64} numeric={false} onCommit={(v) => set({ tag: v.trim() })} /></td>
                      <td className="num"><Field label={t("Minimum")} value={a.min} onCommit={(v) => set({ min: num(v) })} /></td>
                      <td className="num"><Field label={t("Default")} value={a.default} onCommit={(v) => set({ default: num(v) })} /></td>
                      <td className="num"><Field label={t("Maximum")} value={a.max} onCommit={(v) => set({ max: num(v) })} /></td>
                      <td><button className="icon-btn" aria-label={t("Remove {name}", { name: a.name })} title={t("Remove axis")} onClick={() => setAxes(curAxes().filter((_, j) => j !== i))}><CloseIcon size={14} /></button></td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
          {axes && (
            <div className="fi-actions">
              <span className="k-caption muted">{t("Axis changes apply to all masters and instances.")}</span>
              <span className="spacer" />
              <button className="btn btn-secondary btn-sm" onClick={() => setAxes(null)}>{t("Discard")}</button>
              <button className="btn btn-primary btn-sm" onClick={async () => {
                const a = drafts.current.axes;
                if (!a) return;
                // Removing an axis also changes masters, instances and layers: say so first.
                const removed = info.axes.filter((old) => !a.some((x) => x.tag === old.tag));
                if (removed.length) {
                  const names = removed.map((x) => x.name).join(", ");
                  const ok = window.confirm(
                    removed.length === 1
                      ? t("Remove {names}?\n\nMasters and instances lose their position on this axis, and intermediate and alternate layers that use it are deleted. You can undo this with Ctrl+Z.", { names })
                      : t("Remove {names}?\n\nMasters and instances lose their position on these axes, and intermediate and alternate layers that use them are deleted. You can undo this with Ctrl+Z.", { names })
                  );
                  if (!ok) return;
                }
                if (await apply(() => backend.setAxes(a))) setAxes(null);
              }}>{t("Apply axes")}</button>
            </div>
          )}
        </section>

        <section className="fi-section" aria-labelledby="fi-masters">
          <div className="fi-head">
            <h2 id="fi-masters" className="k-subtitle">{t("Masters")}</h2>
            <span className="spacer" />
            <button className="btn btn-secondary btn-sm" onClick={() => setAdding({ name: t("Master {n}", { n: info.masters.length + 1 }), copyFrom: info.defaultMasterId, location: Object.fromEntries(info.axes.map((a) => [a.tag, a.max])) })}>
              <AddIcon size={14} /> {t("Add master")}
            </button>
          </div>
          {info.masters.map((m) => (
            <div key={m.id} className="fi-master">
              <div className="fi-row">
                <label className="info-field"><span className="k-caption muted">{t("Name")}</span>
                  <Field label={t("Master name")} value={m.name} width={160} mono={false} numeric={false} onCommit={(v) => updateMaster(m, { name: v })} />
                </label>
                {locFields(m.location, (loc) => updateMaster(m, { location: loc }), `${m.name}`)}
                <span className="info-sep" />
                {(["ascender", "capHeight", "xHeight", "descender", "italicAngle"] as const).map((k) => (
                  <label key={k} className="info-field">
                    <span className="k-caption muted">{t({ ascender: "Ascender", capHeight: "Cap height", xHeight: "x-height", descender: "Descender", italicAngle: "Italic angle" }[k])}</span>
                    <Field label={`${m.name} ${k}`} value={m.metrics[k]} width={64} onCommit={(v) => updateMaster(m, { metrics: { ...m.metrics, [k]: num(v) } })} />
                  </label>
                ))}
                <span className="spacer" />
                {m.id === info.defaultMasterId && <span className="pill pill-group" title={t("At the default location of every axis")}>{t("Default")}</span>}
                <button className="icon-btn" aria-label={t("Remove {name}", { name: m.name })} title={info.masters.length === 1 ? t("A font needs at least one master") : t("Remove master")} disabled={info.masters.length === 1}
                  onClick={() => window.confirm(t("Remove the master “{name}” and its outlines and kerning? You can undo this.", { name: m.name })) && apply(() => backend.removeMaster(m.id))}>
                  <CloseIcon size={14} />
                </button>
              </div>
            </div>
          ))}
          {adding && (
            <div className="fi-master fi-adding">
              <div className="fi-row">
                <label className="info-field"><span className="k-caption muted">{t("New master name")}</span>
                  <input className="metric-input text-field" style={{ width: 160 }} value={adding.name} onChange={(e) => setAdding({ ...adding, name: e.target.value })} />
                </label>
                <label className="info-field"><span className="k-caption muted">{t("Copy outlines from")}</span>
                  <select className="select" value={adding.copyFrom} onChange={(e) => setAdding({ ...adding, copyFrom: e.target.value })}>
                    {info.masters.map((m) => <option key={m.id} value={m.id}>{m.name}</option>)}
                  </select>
                </label>
                {locFields(adding.location, (loc) => setAdding({ ...adding, location: loc }), t("New master"))}
                <span className="spacer" />
                <button className="btn btn-secondary btn-sm" onClick={() => setAdding(null)}>{t("Cancel")}</button>
                <button className="btn btn-primary btn-sm" onClick={async () => { if (await apply(() => backend.addMaster(adding.copyFrom, adding.name, adding.location))) setAdding(null); }}>{t("Add")}</button>
              </div>
              {info.axes.length === 0 && <p className="k-caption muted">{t("Tip: add an axis first so masters can sit at different locations.")}</p>}
            </div>
          )}
        </section>

        <section className="fi-section" aria-labelledby="fi-instances">
          <div className="fi-head">
            <h2 id="fi-instances" className="k-subtitle">{t("Instances")}</h2>
            <span className="spacer" />
            <button className="btn btn-secondary btn-sm" disabled={info.axes.length === 0} title={info.axes.length === 0 ? t("Add an axis first") : undefined}
              onClick={() => setInstances([...curInst(), { name: t("Instance {n}", { n: curInst().length + 1 }), location: Object.fromEntries(info.axes.map((a) => [a.tag, a.default])), export: true }])}>
              <AddIcon size={14} /> {t("Add instance")}
            </button>
          </div>
          {instDraft.length === 0 ? (
            <p className="k-caption muted">{t("No instances. Instances are the named styles (e.g. SemiBold) that get exported.")}</p>
          ) : instDraft.map((it, i) => {
            const patch = (p: Partial<Instance>) => setInstances(curInst().map((x, j) => (j === i ? { ...x, ...p } : x)));
            const link = it.styleLink;
            const kind = link ? (link.bold && link.italic ? "boldItalic" : link.bold ? "bold" : "italic") : "";
            const setKind = (k: string) => patch({
              styleLink: k === "" ? undefined : { bold: k === "bold" || k === "boldItalic", italic: k === "italic" || k === "boldItalic", base: link?.base ?? "" },
            });
            return (
            <div key={i} className="fi-row fi-instance">
              <label className="info-field"><span className="k-caption muted">{t("Name")}</span>
                <Field label={t("Instance name")} value={it.name} width={160} mono={false} numeric={false} onCommit={(v) => patch({ name: v })} />
              </label>
              {locFields(it.location, (loc) => patch({ location: loc }), it.name)}
              <label className="info-field" title={t("OS/2 weight class, 100 (Thin) to 900 (Black). Empty picks it from the weight axis.")}><span className="k-caption muted">{t("Weight class")}</span>
                <Field label={t("Weight class")} value={it.weightClass ?? ""} width={88} placeholder={t("Automatic")} onCommit={(v) => {
                  if (v.trim() === "") return patch({ weightClass: undefined });
                  const n = num(v);
                  if (!Number.isInteger(n) || n < 1 || n > 1000) { onError(t("Weight class must be a whole number from 1 to 1000.")); return false; }
                  patch({ weightClass: n });
                }} />
              </label>
              <label className="info-field" title={t("OS/2 width class, 1 (ultra-condensed) to 9 (ultra-expanded). Empty picks it from the width axis.")}><span className="k-caption muted">{t("Width class")}</span>
                <Field label={t("Width class")} value={it.widthClass ?? ""} width={88} placeholder={t("Automatic")} onCommit={(v) => {
                  if (v.trim() === "") return patch({ widthClass: undefined });
                  const n = num(v);
                  if (!Number.isInteger(n) || n < 1 || n > 9) { onError(t("Width class must be a whole number from 1 to 9.")); return false; }
                  patch({ widthClass: n });
                }} />
              </label>
              <label className="info-field" title={t("Links this instance as the bold or italic of another, so apps group them into one family.")}><span className="k-caption muted">{t("Style link")}</span>
                <select className="select" value={kind} aria-label={t("Style link")} onChange={(e) => setKind(e.target.value)}>
                  <option value="">{t("None")}</option>
                  <option value="bold">{t("Bold")}</option>
                  <option value="italic">{t("Italic")}</option>
                  <option value="boldItalic">{t("Bold Italic")}</option>
                </select>
              </label>
              {kind !== "" && (
                <label className="info-field"><span className="k-caption muted">{t("Style of")}</span>
                  <select className="select" value={link?.base ?? ""} aria-label={t("Style of")} onChange={(e) => patch({ styleLink: { bold: link?.bold ?? false, italic: link?.italic ?? false, base: e.target.value } })}>
                    <option value="">{t("This family")}</option>
                    {instDraft.filter((_, j) => j !== i).map((o) => <option key={o.name} value={o.name}>{o.name}</option>)}
                  </select>
                </label>
              )}
              <label className="check-row fi-export" title={t("Export this instance as a static font")}>
                <input type="checkbox" checked={it.export !== false} aria-label={t("Export {name}", { name: it.name })}
                  onChange={(e) => patch({ export: e.target.checked })} />
                <span className="k-caption">{t("Export")}</span>
              </label>
              <span className="spacer" />
              <button className="icon-btn" aria-label={t("Remove {name}", { name: it.name })} title={t("Remove instance")} onClick={() => setInstances(curInst().filter((_, j) => j !== i))}><CloseIcon size={14} /></button>
            </div>
            );
          })}
          {instances && (
            <div className="fi-actions">
              <span className="spacer" />
              <button className="btn btn-secondary btn-sm" onClick={() => setInstances(null)}>{t("Discard")}</button>
              <button className="btn btn-primary btn-sm" onClick={async () => { const list = drafts.current.instances; if (list && await apply(() => backend.setInstances(list))) setInstances(null); }}>{t("Apply instances")}</button>
            </div>
          )}
        </section>

        <FeaturesSection version={version} onChanged={onChanged} onError={onError} />

        <section className="fi-section" aria-labelledby="fi-notes">
          <div className="fi-head">
            <h2 id="fi-notes" className="k-subtitle">{t("Notes")}</h2>
            <span className="k-caption muted">{t("Saved with the font, never exported.")}</span>
          </div>
          <textarea
            className="notes-area"
            aria-label={t("Notes")}
            defaultValue={info.notes}
            key={info.notes}
            placeholder={t("Notes to yourself about this font.")}
            onBlur={(e) => { const v = e.currentTarget.value; if (v !== info.notes) apply(() => backend.setNotes(v)); }}
            onKeyDown={(e) => { if (e.key === "Escape") { e.currentTarget.value = info.notes; e.currentTarget.blur(); } }}
          />
        </section>

        <section className="fi-section" aria-labelledby="fi-hinting">
          <div className="fi-head">
            <h2 id="fi-hinting" className="k-subtitle">{t("Hinting")}</h2>
            <span className="k-caption muted">{t("TrueType autohinting on export (TTF and WOFF2). Turn it on in the Export dialog.")}</span>
          </div>
          <div className="fi-row">
            {([
              ["rangeMin", "Smallest size (px)", "Hinting starts at this many pixels per em"],
              ["rangeMax", "Largest size (px)", "Above this size the font isn't hinted"],
              ["increaseXHeight", "Increase x-height up to (px)", "Rounds the x-height up at small sizes for legibility; 0 is off"],
            ] as const).map(([key, en, hintEn]) => { const label = t(en), hint = t(hintEn); return (
              <label key={key} className="info-field" title={hint}><span className="k-caption muted">{label}</span>
                <Field label={label} value={info.hinting[key]} onCommit={(v) => {
                  const n = num(v);
                  if (!Number.isInteger(n)) { onError(t("{name}: enter a whole number.", { name: label })); return false; }
                  return apply(() => backend.setHinting({ ...info.hinting, [key]: n }));
                }} />
              </label>
            ); })}
            <label className="check-row" title={t("Hint accented letters and other composites as whole glyphs, not through their components")}>
              <input type="checkbox" checked={info.hinting.hintComposites}
                onChange={(e) => apply(() => backend.setHinting({ ...info.hinting, hintComposites: e.target.checked }))} />
              <span className="k-body">{t("Hint composites as whole glyphs")}</span>
            </label>
          </div>
        </section>

        <section className="fi-section" aria-labelledby="fi-palettes">
          <div className="fi-head">
            <h2 id="fi-palettes" className="k-subtitle">{t("Colour palettes")}</h2>
            <span className="k-caption muted">{t("Colours for colour layers (COLR and CPAL). Palette 1 is the default; others, such as one for dark backgrounds, have the same number of colours.")}</span>
            <span className="spacer" />
            {info.palettes.length > 0 && (
              <button className="btn btn-secondary btn-sm" onClick={() => apply(() => backend.setPalettes(info.palettes.map((p) => [...p, "#000000ff"])))}>
                <AddIcon size={14} /> {t("Add colour")}
              </button>
            )}
            <button className="btn btn-secondary btn-sm" onClick={() => apply(() => backend.setPalettes(
              info.palettes.length ? [...info.palettes, [...info.palettes[0]]] : [["#e63946ff", "#1d3557ff"]],
            ))}>
              <AddIcon size={14} /> {t("Add palette")}
            </button>
          </div>
          {info.palettes.length === 0 ? (
            <p className="k-caption muted">{t("No palettes. Add one to draw colour glyphs: then use + Colour in the glyph editor.")}</p>
          ) : (
            <table className="fi-table">
              <thead><tr>
                <th>{t("Colour")}</th>
                {info.palettes.map((_, p) => (
                  <th key={p}>
                    {t("Palette {n}", { n: p + 1 })}
                    {p > 0 && (
                      <button className="icon-btn" aria-label={t("Remove palette {n}", { n: p + 1 })} title={t("Remove palette {n}", { n: p + 1 })}
                        onClick={() => apply(() => backend.setPalettes(info.palettes.filter((_, j) => j !== p)))}><CloseIcon size={12} /></button>
                    )}
                  </th>
                ))}
                <th aria-label={t("Actions")} />
              </tr></thead>
              <tbody>
                {info.palettes[0].map((_, c) => (
                  <tr key={c}>
                    <td className="k-caption muted">{c + 1}</td>
                    {info.palettes.map((pal, p) => (
                      <td key={p}>
                        <ColorCell value={pal[c]} label={t("Palette {p}, colour {c}", { p: p + 1, c: c + 1 })}
                          onCommit={(v) => apply(() => backend.setPalettes(info.palettes.map((q, j) => (j === p ? q.map((x, k) => (k === c ? v : x)) : q))))} />
                      </td>
                    ))}
                    <td>
                      <button className="icon-btn" aria-label={t("Remove colour {n}", { n: c + 1 })} title={t("Remove colour {n}", { n: c + 1 })} disabled={info.palettes[0].length === 1}
                        onClick={() => apply(() => backend.setPalettes(info.palettes.map((q) => q.filter((_, k) => k !== c))))}><CloseIcon size={14} /></button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </section>
      </div>
    </div>
    </FieldError.Provider>
  );
}
