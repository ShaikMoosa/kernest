import { createContext, Fragment, useCallback, useContext, useEffect, useRef, useState } from "react";
import { backend, type Axis, type AxisValue, type DocStatus, type FontInfo, type ForeignMasterPreview, type Instance, type Location, type MasterView, type Metrics, type VariableFontSetting } from "../lib/backend";
import { designBounds, designToUser, hasMap, userToDesign } from "../lib/axisMap";
import { hasLabels, statStyleName } from "../lib/statName";
import { pickMasterSource } from "../lib/desktop";
import { AddIcon, CloseIcon, FolderIcon } from "./Icons";
import { FeaturesSection } from "./FeaturesSection";
import { ForeignMasterDialog } from "./ForeignMasterDialog";
import { t, tn, translateMessage } from "../lib/i18n";

const AXIS_PRESETS: Axis[] = [
  { name: "Weight", tag: "wght", min: 100, default: 400, max: 900 },
  { name: "Width", tag: "wdth", min: 75, default: 100, max: 125 },
  { name: "Optical size", tag: "opsz", min: 8, default: 14, max: 72 },
  { name: "Slant", tag: "slnt", min: -12, default: 0, max: 0 },
];

const num = (s: string) => Number(s.trim().replace("−", "-"));

/** Design/user readouts round to 2 decimals so interpolated values stay readable. */
const fmtCoord = (v: number) => String(Math.round(v * 100) / 100);

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
  onGenerateInstances: () => void;
}

/** S10/S11 · Font Info: general info, axes, masters and instances. */
export function FontInfoView({ version, onChanged, onError, onGenerateInstances }: Props) {
  const [info, setInfo] = useState<FontInfo | null>(null);
  // Drafts while editing axes or instances. A ref mirrors them so commits that land
  // in the same instant (blur, then click Apply) always build on the latest draft.
  const [axes, setAxesState] = useState<Axis[] | null>(null);
  const [instances, setInstState] = useState<Instance[] | null>(null);
  const [variableFonts, setVfState] = useState<VariableFontSetting[] | null>(null);
  const drafts = useRef<{ axes: Axis[] | null; instances: Instance[] | null; variableFonts: VariableFontSetting[] | null }>({ axes: null, instances: null, variableFonts: null });
  const setAxes = (v: Axis[] | null) => { drafts.current.axes = v; setAxesState(v); };
  const setInstances = (v: Instance[] | null) => { drafts.current.instances = v; setInstState(v); };
  const setVariableFonts = (v: VariableFontSetting[] | null) => { drafts.current.variableFonts = v; setVfState(v); };
  const [adding, setAdding] = useState<{ name: string; copyFrom: string; location: Location } | null>(null);
  // The "Add from font…" dialog: a picked source and its per-master compatibility report.
  const [foreign, setForeign] = useState<{ path: string; previews: ForeignMasterPreview[] } | null>(null);
  // A draft of the glyph-order text; null means "show the saved order, not editing".
  const [orderText, setOrderText] = useState<string | null>(null);
  // The axis tag whose avar map editor is expanded (one at a time); null = none.
  const [mapOpen, setMapOpen] = useState<string | null>(null);
  // The axis tag whose STAT value-label editor is expanded (one at a time); null = none.
  const [labelsOpen, setLabelsOpen] = useState<string | null>(null);
  // The master hinting editor that is expanded: "<master id>:zones" or ":stems"; null = none.
  const [hintsOpen, setHintsOpen] = useState<string | null>(null);

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
  const curVf = () => drafts.current.variableFonts ?? info.variableFonts;
  const vfDraft = variableFonts ?? info.variableFonts;
  const locFields = (loc: Location, onChange: (loc: Location) => void, prefix: string) =>
    info.axes.map((a) => {
      // Locations are design-space; the fallback is the mapped design default, not the user one.
      const design = loc[a.tag] ?? designBounds(a)[1];
      return (
        <label key={a.tag} className="info-field">
          <span className="k-caption muted">{a.tag}</span>
          <Field label={`${prefix} ${a.name}`} value={design} onCommit={(v) => onChange({ ...loc, [a.tag]: num(v) })} />
          {hasMap(a) && <span className="k-caption muted axis-user">{t("user {value}", { value: fmtCoord(designToUser(a, design)) })}</span>}
        </label>
      );
    });
  const updateMaster = (m: MasterView, patch: Partial<{ name: string; location: Location; metrics: Metrics }>) =>
    apply(() => backend.updateMaster(m.id, patch.name ?? m.name, patch.location ?? m.location, patch.metrics ?? m.metrics));

  const openForeign = async () => {
    const path = await pickMasterSource();
    if (!path) return;
    try {
      const previews = await backend.previewForeignMasters(path);
      setForeign({ path, previews });
    } catch (e) { onError(e); }
  };
  // Each master is its own undoable step. Show progress as they land; if one fails,
  // refresh so the masters already added are visible, then let the dialog report it.
  const addForeign = async (selection: { masterId: string; name: string; location: Location }[]) => {
    if (!foreign) return;
    try {
      for (const s of selection) {
        const r = await backend.addMasterFrom(foreign.path, s.masterId, s.name, s.location);
        setInfo(r.info);
        onChanged(r.info.status);
      }
      setForeign(null);
    } catch (e) {
      reload();
      throw e;
    }
  };

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
                  const pts = a.map ?? [];
                  // Points stay sorted by user value; an empty list clears the map (user == design).
                  const setMap = (next: [number, number][]) => {
                    const sorted = [...next].sort((p, q) => p[0] - q[0]);
                    set({ map: sorted.length ? sorted : undefined });
                  };
                  const addPoint = () => {
                    if (pts.length === 0) {
                      // Seed the identity extremes so the axis can be bent at the default right away.
                      const seed: [number, number][] = [[a.min, a.min], [a.default, a.default], [a.max, a.max]];
                      setMap(seed.filter((p, k) => k === 0 || p[0] !== seed[k - 1][0]));
                      return;
                    }
                    // Insert on the current curve at the midpoint of the widest user gap, so no duplicate.
                    let at = 0;
                    let gap = 0;
                    for (let k = 0; k < pts.length - 1; k++) {
                      const g = pts[k + 1][0] - pts[k][0];
                      if (g > gap) { gap = g; at = k; }
                    }
                    if (gap <= 0) return;
                    const u = (pts[at][0] + pts[at + 1][0]) / 2;
                    const next = [...pts];
                    next.splice(at + 1, 0, [u, userToDesign(a, u)]);
                    setMap(next);
                  };
                  const labels = a.labels ?? [];
                  // An empty list clears the labels (the axis has no named values).
                  const setLabels = (next: AxisValue[]) => set({ labels: next.length ? next : undefined });
                  const patchLabel = (j: number, p: Partial<AxisValue>) => setLabels(labels.map((l, k) => (k === j ? { ...l, ...p } : l)));
                  const addLabel = () => setLabels([...labels, { name: "", value: a.default }]);
                  const optNum = (v: string) => (v.trim() === "" ? undefined : num(v));
                  const open = mapOpen === a.tag;
                  const labelsExpanded = labelsOpen === a.tag;
                  return (
                    <Fragment key={i}>
                      <tr>
                        <td><Field label={t("Axis name")} value={a.name} width={160} mono={false} numeric={false} onCommit={(v) => set({ name: v })} /></td>
                        <td><Field label={t("Axis tag")} value={a.tag} width={64} numeric={false} onCommit={(v) => set({ tag: v.trim() })} /></td>
                        <td className="num"><Field label={t("Minimum")} value={a.min} onCommit={(v) => set({ min: num(v) })} /></td>
                        <td className="num"><Field label={t("Default")} value={a.default} onCommit={(v) => set({ default: num(v) })} /></td>
                        <td className="num"><Field label={t("Maximum")} value={a.max} onCommit={(v) => set({ max: num(v) })} /></td>
                        <td className="fi-axis-actions">
                          <button className={`btn btn-sm ${hasMap(a) ? "btn-primary" : "btn-secondary"}`} aria-expanded={open}
                            title={t("Map user values to design values")} onClick={() => setMapOpen(open ? null : a.tag)}>
                            {hasMap(a) ? tn(pts.length, "{n} point", "{n} points") : t("Mapping")}
                          </button>
                          <button className={`btn btn-sm ${labels.length ? "btn-primary" : "btn-secondary"}`} aria-expanded={labelsExpanded}
                            title={t("Name the values on this axis")} onClick={() => setLabelsOpen(labelsExpanded ? null : a.tag)}>
                            {labels.length ? tn(labels.length, "{n} value", "{n} values") : t("Values")}
                          </button>
                          <button className="icon-btn" aria-label={t("Remove {name}", { name: a.name })} title={t("Remove axis")} onClick={() => setAxes(curAxes().filter((_, j) => j !== i))}><CloseIcon size={14} /></button>
                        </td>
                      </tr>
                      {open && (
                        <tr className="fi-axis-map">
                          <td colSpan={6}>
                            <div className="axis-map">
                              <p className="k-caption muted">{t("Apps use user values; masters use design values. Add points to map one to the other.")}</p>
                              <div className="axis-map-points">
                                {pts.map(([u, d], j) => (
                                  <div key={j} className="axis-map-point">
                                    <label className="info-field"><span className="k-caption muted">{t("User")}</span>
                                      <Field label={t("User value")} value={u} onCommit={(v) => setMap(pts.map((p, k): [number, number] => (k === j ? [num(v), p[1]] : p)))} />
                                    </label>
                                    <label className="info-field"><span className="k-caption muted">{t("Design")}</span>
                                      <Field label={t("Design value")} value={d} onCommit={(v) => setMap(pts.map((p, k): [number, number] => (k === j ? [p[0], num(v)] : p)))} />
                                    </label>
                                    <button className="icon-btn" aria-label={t("Remove point")} title={t("Remove point")} onClick={() => setMap(pts.filter((_, k) => k !== j))}><CloseIcon size={14} /></button>
                                  </div>
                                ))}
                              </div>
                              <button className="btn btn-secondary btn-sm" onClick={addPoint}><AddIcon size={14} /> {t("Add point")}</button>
                            </div>
                          </td>
                        </tr>
                      )}
                      {labelsExpanded && (
                        <tr className="fi-axis-map">
                          <td colSpan={6}>
                            <div className="axis-map">
                              <p className="k-caption muted">{t("Apps show these names for the styles at each position on the axis. Values are user values.")}</p>
                              <div className="axis-labels">
                                {labels.map((l, j) => (
                                  <div key={j} className="axis-label">
                                    <label className="info-field"><span className="k-caption muted">{t("Name")}</span>
                                      <Field label={t("Value name")} value={l.name} width={120} mono={false} numeric={false} onCommit={(v) => patchLabel(j, { name: v })} />
                                    </label>
                                    <label className="info-field"><span className="k-caption muted">{t("Value")}</span>
                                      <Field label={t("User value")} value={l.value} onCommit={(v) => patchLabel(j, { value: num(v) })} />
                                    </label>
                                    <label className="info-field"><span className="k-caption muted">{t("Range from")}</span>
                                      <Field label={t("Range from")} value={l.min ?? ""} placeholder={t("none")} onCommit={(v) => patchLabel(j, { min: optNum(v) })} />
                                    </label>
                                    <label className="info-field"><span className="k-caption muted">{t("Range to")}</span>
                                      <Field label={t("Range to")} value={l.max ?? ""} placeholder={t("none")} onCommit={(v) => patchLabel(j, { max: optNum(v) })} />
                                    </label>
                                    <label className="info-field"><span className="k-caption muted">{t("Linked value")}</span>
                                      <Field label={t("Linked value")} value={l.linkedValue ?? ""} placeholder={t("none")} onCommit={(v) => patchLabel(j, { linkedValue: optNum(v) })} />
                                    </label>
                                    <label className="check-row" title={t("Apps may leave this name out of a style name, like Regular.")}>
                                      <input type="checkbox" checked={l.elidable === true} aria-label={t("Elidable")} onChange={(e) => patchLabel(j, { elidable: e.target.checked })} />
                                      <span className="k-caption">{t("Elidable")}</span>
                                    </label>
                                    <button className="icon-btn" aria-label={t("Remove value")} title={t("Remove value")} onClick={() => setLabels(labels.filter((_, k) => k !== j))}><CloseIcon size={14} /></button>
                                  </div>
                                ))}
                              </div>
                              <button className="btn btn-secondary btn-sm" onClick={addLabel}><AddIcon size={14} /> {t("Add value")}</button>
                            </div>
                          </td>
                        </tr>
                      )}
                    </Fragment>
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
            <button className="btn btn-secondary btn-sm" onClick={() => setAdding({ name: t("Master {n}", { n: info.masters.length + 1 }), copyFrom: info.defaultMasterId, location: Object.fromEntries(info.axes.map((a) => [a.tag, designBounds(a)[2]])) })}>
              <AddIcon size={14} /> {t("Add master")}
            </button>
            <button className="btn btn-secondary btn-sm" onClick={openForeign}>
              <FolderIcon size={14} /> {t("Add from font…")}
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
                <button className={`btn btn-sm ${m.alignmentZones.length ? "btn-primary" : "btn-secondary"}`} aria-expanded={hintsOpen === `${m.id}:zones`}
                  title={t("Edges that line up on one pixel row at small sizes")} onClick={() => setHintsOpen(hintsOpen === `${m.id}:zones` ? null : `${m.id}:zones`)}>
                  {m.alignmentZones.length ? tn(m.alignmentZones.length, "{n} zone", "{n} zones") : t("Zones")}
                </button>
                <button className={`btn btn-sm ${m.stems.length ? "btn-primary" : "btn-secondary"}`} aria-expanded={hintsOpen === `${m.id}:stems`}
                  title={t("The common stroke widths, used for hinting")} onClick={() => setHintsOpen(hintsOpen === `${m.id}:stems` ? null : `${m.id}:stems`)}>
                  {m.stems.length ? tn(m.stems.length, "{n} stem", "{n} stems") : t("Stems")}
                </button>
                {m.id === info.defaultMasterId && <span className="pill pill-group" title={t("At the default location of every axis")}>{t("Default")}</span>}
                <button className="icon-btn" aria-label={t("Remove {name}", { name: m.name })} title={info.masters.length === 1 ? t("A font needs at least one master") : t("Remove master")} disabled={info.masters.length === 1}
                  onClick={() => window.confirm(t("Remove the master “{name}” and its outlines and kerning? You can undo this.", { name: m.name })) && apply(() => backend.removeMaster(m.id))}>
                  <CloseIcon size={14} />
                </button>
              </div>
              {hintsOpen === `${m.id}:zones` && (
                <div className="fi-hint-editor">
                  <p className="k-caption muted">{t("Zones keep edges like the baseline and x-height on one pixel row at small sizes. A positive size marks a top edge, a negative size a bottom edge.")}</p>
                  <div className="fi-hint-rows">
                    {m.alignmentZones.map((z, i) => (
                      <div key={i} className="fi-hint-row">
                        <label className="info-field"><span className="k-caption muted">{t("Position")}</span>
                          <Field label={t("Zone position")} value={z.position} onCommit={(v) => apply(() => backend.setAlignmentZones(m.id, m.alignmentZones.map((x, j) => (j === i ? { ...x, position: num(v) } : x))))} />
                        </label>
                        <label className="info-field"><span className="k-caption muted">{t("Size")}</span>
                          <Field label={t("Zone size")} value={z.size} onCommit={(v) => apply(() => backend.setAlignmentZones(m.id, m.alignmentZones.map((x, j) => (j === i ? { ...x, size: num(v) } : x))))} />
                        </label>
                        <button className="icon-btn" aria-label={t("Remove zone")} title={t("Remove zone")}
                          onClick={() => apply(() => backend.setAlignmentZones(m.id, m.alignmentZones.filter((_, j) => j !== i)))}><CloseIcon size={14} /></button>
                      </div>
                    ))}
                  </div>
                  <button className="btn btn-secondary btn-sm" onClick={() => apply(() => backend.setAlignmentZones(m.id, [...m.alignmentZones, { position: 0, size: 10 }]))}>
                    <AddIcon size={14} /> {t("Add zone")}
                  </button>
                </div>
              )}
              {hintsOpen === `${m.id}:stems` && (
                <div className="fi-hint-editor">
                  <p className="k-caption muted">{t("Stems are the common stroke widths. PostScript exports use them for hints; TrueType uses the first horizontal stem as its guide width.")}</p>
                  <div className="fi-hint-rows">
                    {m.stems.map((s, i) => (
                      <div key={i} className="fi-hint-row">
                        <label className="info-field"><span className="k-caption muted">{t("Name")}</span>
                          <Field label={t("Stem name")} value={s.name} width={120} mono={false} numeric={false} onCommit={(v) => apply(() => backend.setStems(m.id, m.stems.map((x, j) => (j === i ? { ...x, name: v } : x))))} />
                        </label>
                        <label className="info-field"><span className="k-caption muted">{t("Direction")}</span>
                          <select className="select" value={s.horizontal ? "h" : "v"} aria-label={t("Stem direction")}
                            onChange={(e) => apply(() => backend.setStems(m.id, m.stems.map((x, j) => (j === i ? { ...x, horizontal: e.target.value === "h" } : x))))}>
                            <option value="h">{t("Horizontal")}</option>
                            <option value="v">{t("Vertical")}</option>
                          </select>
                        </label>
                        <label className="info-field"><span className="k-caption muted">{t("Width")}</span>
                          <Field label={t("Stem width")} value={s.value} onCommit={(v) => apply(() => backend.setStems(m.id, m.stems.map((x, j) => (j === i ? { ...x, value: num(v) } : x))))} />
                        </label>
                        <button className="icon-btn" aria-label={t("Remove stem")} title={t("Remove stem")}
                          onClick={() => apply(() => backend.setStems(m.id, m.stems.filter((_, j) => j !== i)))}><CloseIcon size={14} /></button>
                      </div>
                    ))}
                  </div>
                  <button className="btn btn-secondary btn-sm" onClick={() => apply(() => backend.setStems(m.id, [...m.stems, { name: t("Stem {n}", { n: m.stems.length + 1 }), horizontal: true, value: 80 }]))}>
                    <AddIcon size={14} /> {t("Add stem")}
                  </button>
                </div>
              )}
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
            <button className="btn btn-secondary btn-sm" disabled={curInst().length === 0}
              title={t("Interpolate one master per instance and open them as a new unsaved font.")}
              onClick={onGenerateInstances}>
              {t("Generate as New Font")}
            </button>
            <button className="btn btn-secondary btn-sm" disabled={info.axes.length === 0} title={info.axes.length === 0 ? t("Add an axis first") : undefined}
              onClick={() => setInstances([...curInst(), { name: t("Instance {n}", { n: curInst().length + 1 }), location: Object.fromEntries(info.axes.map((a) => [a.tag, designBounds(a)[1]])), export: true }])}>
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
              {hasLabels(axesDraft) && (
                <span className="k-caption muted axis-user" title={t("The style name apps show for this instance, from the axis value names.")}>
                  {t("STAT name: {name}", { name: statStyleName(axesDraft, it) })}
                </span>
              )}
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

        <section className="fi-section" aria-labelledby="fi-variable">
          <div className="fi-head">
            <h2 id="fi-variable" className="k-subtitle">{t("Variable fonts")}</h2>
            <span className="spacer" />
            <button className="btn btn-secondary btn-sm" disabled={info.axes.length === 0} title={info.axes.length === 0 ? t("Add an axis first") : undefined}
              onClick={() => { const n = curVf().length + 1; setVariableFonts([...curVf(), { name: t("Variable font {n}", { n }), fileName: `${info.familyName.replace(/\s+/g, "")}-${n}`, axes: [], glyphs: [] }]); }}>
              <AddIcon size={14} /> {t("Add variable font")}
            </button>
          </div>
          <p className="k-caption muted">{t("Extra variable fonts built from this family. Each keeps a range of the axes and a set of glyphs; export writes one file per entry.")}</p>
          {vfDraft.length === 0 ? (
            <p className="k-caption muted">{t("No extra variable fonts. Export builds one variable font covering every axis.")}</p>
          ) : vfDraft.map((vf, i) => {
            const patch = (p: Partial<VariableFontSetting>) => setVariableFonts(curVf().map((x, j) => (j === i ? { ...x, ...p } : x)));
            const setRange = (tag: string, field: "min" | "max", v: string) => {
              const val = v.trim() === "" ? null : num(v);
              const axes = [...(vf.axes ?? [])];
              const idx = axes.findIndex((s) => s.tag === tag);
              const next = { ...(idx >= 0 ? axes[idx] : { tag, min: null, max: null }), [field]: val };
              if (next.min == null && next.max == null) { if (idx >= 0) axes.splice(idx, 1); }
              else if (idx >= 0) axes[idx] = next; else axes.push(next);
              patch({ axes });
            };
            return (
            <div key={i} className="fi-vf">
              <div className="fi-row">
                <label className="info-field"><span className="k-caption muted">{t("Name")}</span>
                  <Field label={t("Variable font name")} value={vf.name} width={160} mono={false} numeric={false} onCommit={(v) => patch({ name: v })} />
                </label>
                <label className="info-field" title={t("The file name without extension, e.g. MyFont-Light.")}><span className="k-caption muted">{t("File name")}</span>
                  <Field label={t("File name")} value={vf.fileName} width={200} mono={false} numeric={false} onCommit={(v) => patch({ fileName: v })} />
                </label>
                <span className="spacer" />
                <button className="icon-btn" aria-label={t("Remove {name}", { name: vf.name })} title={t("Remove variable font")} onClick={() => setVariableFonts(curVf().filter((_, j) => j !== i))}><CloseIcon size={14} /></button>
              </div>
              <div className="fi-row">
                {info.axes.map((a) => {
                  const sub = (vf.axes ?? []).find((s) => s.tag === a.tag);
                  return (
                    <div key={a.tag} className="info-field fi-vf-axis">
                      <span className="k-caption muted">{a.name} ({a.tag})</span>
                      <span className="fi-vf-range">
                        <Field label={t("{name} minimum", { name: a.name })} value={sub?.min ?? ""} width={72} placeholder={String(a.min)} onCommit={(v) => setRange(a.tag, "min", v)} />
                        <span className="k-caption muted">–</span>
                        <Field label={t("{name} maximum", { name: a.name })} value={sub?.max ?? ""} width={72} placeholder={String(a.max)} onCommit={(v) => setRange(a.tag, "max", v)} />
                      </span>
                    </div>
                  );
                })}
                <label className="info-field"><span className="k-caption muted">{t("Glyphs")}</span>
                  <Field label={t("Glyphs")} value={(vf.glyphs ?? []).join(", ")} width={240} mono={false} numeric={false}
                    onCommit={(v) => patch({ glyphs: v.split(/[,\s]+/).map((s) => s.trim()).filter(Boolean) })} />
                </label>
                <span className="k-caption muted fi-vf-glyphs-hint">{t("Empty keeps every glyph.")}</span>
              </div>
            </div>
            );
          })}
          {variableFonts && (
            <div className="fi-actions">
              <span className="spacer" />
              <button className="btn btn-secondary btn-sm" onClick={() => setVariableFonts(null)}>{t("Discard")}</button>
              <button className="btn btn-primary btn-sm" onClick={async () => { const list = drafts.current.variableFonts; if (list && await apply(() => backend.setVariableFonts(list))) setVariableFonts(null); }}>{t("Apply variable fonts")}</button>
            </div>
          )}
        </section>

        <FeaturesSection version={version} onChanged={onChanged} onError={onError} />

        <section className="fi-section" aria-labelledby="fi-order">
          <div className="fi-head">
            <h2 id="fi-order" className="k-subtitle">{t("Glyph order")}</h2>
            <span className="k-caption muted">{t("The order glyphs export in. One name per line; .notdef is always first.")}</span>
          </div>
          <textarea
            className="notes-area glyph-order-area"
            aria-label={t("Glyph order")}
            spellCheck={false}
            value={orderText ?? info.glyphOrder.join("\n")}
            onChange={(e) => setOrderText(e.target.value)}
          />
          {orderText !== null && (
            <div className="fi-actions">
              <span className="k-caption muted">{t("Every glyph must appear once. The order is saved with the font and used on export.")}</span>
              <span className="spacer" />
              <button className="btn btn-secondary btn-sm" onClick={() => setOrderText(null)}>{t("Discard")}</button>
              <button className="btn btn-primary btn-sm" onClick={async () => {
                const names = orderText.split("\n").map((s) => s.trim()).filter((s) => s.length > 0);
                if (await apply(() => backend.setGlyphOrder(names))) setOrderText(null);
              }}>{t("Apply order")}</button>
            </div>
          )}
        </section>

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
          <h3 className="k-label fi-sub">{t("PostScript hints")}</h3>
          <div className="fi-row">
            {([
              ["blueScale", "Blue scale", "How strongly overshoots are kept at small sizes; 0.039625 is the usual value"],
              ["blueShift", "Blue shift", "How far an overshoot must reach past a zone to stay visible"],
              ["blueFuzz", "Blue fuzz", "Extra reach around each zone when matching edges"],
            ] as const).map(([key, en, hintEn]) => { const label = t(en), hint = t(hintEn); return (
              <label key={key} className="info-field" title={hint}><span className="k-caption muted">{label}</span>
                <Field label={label} value={info.psHinting[key]} onCommit={(v) => apply(() => backend.setPsHinting({ ...info.psHinting, [key]: num(v) }))} />
              </label>
            ); })}
          </div>
          <h3 className="k-label fi-sub">{t("Control instructions")}</h3>
          <p className="k-caption muted">{t("TTFAutohint control instructions, applied when exporting TrueType. One instruction per line.")}</p>
          <textarea
            className="notes-area control-area"
            aria-label={t("Control instructions")}
            spellCheck={false}
            defaultValue={info.controlInstructions}
            key={info.controlInstructions}
            placeholder={t("e.g. H point 0 yshift 0.5 @ 12")}
            onBlur={(e) => { const v = e.currentTarget.value; if (v !== info.controlInstructions) apply(() => backend.setControlInstructions(v)); }}
            onKeyDown={(e) => { if (e.key === "Escape") { e.currentTarget.value = info.controlInstructions; e.currentTarget.blur(); } }}
          />
          <p className="k-caption muted">{t("Zones and stems under Masters guide PostScript hinting. For TrueType, ttfautohint ignores manual zones it can't use.")}</p>
        </section>

        <section className="fi-section" aria-labelledby="fi-grid">
          <div className="fi-head">
            <h2 id="fi-grid" className="k-subtitle">{t("Grid")}</h2>
            <span className="k-caption muted">{t("Snap points to a grid while drawing and dragging. Spacing 0 turns the grid off.")}</span>
          </div>
          <div className="fi-row">
            <label className="info-field" title={t("Distance between grid lines, in font units")}>
              <span className="k-caption muted">{t("Spacing")}</span>
              <Field label={t("Spacing")} value={info.grid?.spacing ?? 0} onCommit={(v) => {
                const n = num(v);
                if (!Number.isInteger(n) || n < 0 || n > 65535) { onError(t("{name}: enter a whole number.", { name: t("Spacing") })); return false; }
                return apply(() => backend.setGrid({ spacing: n, subdivision: info.grid?.subdivision || 1 }));
              }} />
            </label>
            <label className="info-field" title={t("Snap steps between two grid lines")}>
              <span className="k-caption muted">{t("Subdivision")}</span>
              <Field label={t("Subdivision")} value={info.grid?.subdivision ?? 1} onCommit={(v) => {
                const n = num(v);
                if (!Number.isInteger(n) || n < 0 || n > 65535) { onError(t("{name}: enter a whole number.", { name: t("Subdivision") })); return false; }
                return apply(() => backend.setGrid({ spacing: info.grid?.spacing ?? 0, subdivision: n }));
              }} />
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
          {info.palettes.length > 0 && info.masters.length > 1 && (
            <div className="fi-master-palettes">
              <h3 className="k-label">{t("Per-master colours")}</h3>
              <p className="k-caption muted">{t("Give a master its own palette to make colour vary between masters in a variable font.")}</p>
              {info.masters.map((m) => {
                const override = info.masterPalettes[m.id];
                return (
                  <div key={m.id} className="fi-master-palette">
                    <label className="check-row">
                      <input type="checkbox" checked={!!override}
                        onChange={(e) => apply(() => backend.setMasterPalettes(m.id, e.target.checked ? info.palettes.map((p) => [...p]) : null))} />
                      <span className="k-body">{m.name}</span>
                    </label>
                    {override && (
                      <table className="fi-table">
                        <thead><tr>
                          <th>{t("Colour")}</th>
                          {override.map((_, p) => <th key={p}>{t("Palette {n}", { n: p + 1 })}</th>)}
                        </tr></thead>
                        <tbody>
                          {override[0].map((_, c) => (
                            <tr key={c}>
                              <td className="k-caption muted">{c + 1}</td>
                              {override.map((pal, p) => (
                                <td key={p}>
                                  <ColorCell value={pal[c]} label={t("{master}, palette {p}, colour {c}", { master: m.name, p: p + 1, c: c + 1 })}
                                    onCommit={(v) => apply(() => backend.setMasterPalettes(m.id, override.map((q, j) => (j === p ? q.map((x, k) => (k === c ? v : x)) : q))))} />
                                </td>
                              ))}
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </section>
      </div>
    </div>
    {foreign && (
      <ForeignMasterDialog path={foreign.path} previews={foreign.previews} axes={info.axes}
        onCancel={() => setForeign(null)} onAdd={addForeign} />
    )}
    </FieldError.Provider>
  );
}
