import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import { backend, type Axis, type DocStatus, type FontInfo, type Instance, type Location, type MasterView, type Metrics } from "../lib/backend";
import { AddIcon, CloseIcon } from "./Icons";

const AXIS_PRESETS: Axis[] = [
  { name: "Weight", tag: "wght", min: 100, default: 400, max: 900 },
  { name: "Width", tag: "wdth", min: 75, default: 100, max: 125 },
  { name: "Optical size", tag: "opsz", min: 8, default: 14, max: 72 },
  { name: "Slant", tag: "slnt", min: -12, default: 0, max: 0 },
];

const num = (s: string) => Number(s.trim().replace("−", "-"));

/** Where number fields report input that isn't a number. */
const FieldError = createContext<(message: string) => void>(() => {});

/** A text/number input that commits on Enter or blur and reverts on Escape. */
function Field({ value, onCommit, width = 72, mono = true, label, numeric = true }: {
  value: string | number; onCommit: (v: string) => void; width?: number; mono?: boolean; label: string; numeric?: boolean;
}) {
  const shown = String(value);
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);
  const report = useContext(FieldError);
  const commit = (v: string) => {
    if (v === shown) return;
    // Number fields only take numbers: say so instead of sending garbage.
    if (numeric && (v.trim() === "" || !Number.isFinite(num(v)))) {
      setText(shown);
      return report(`${label}: enter a number.`);
    }
    onCommit(v);
  };
  return (
    <input
      aria-label={label}
      className={`metric-input${mono ? "" : " text-field"}`}
      style={{ width }}
      value={text}
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

  if (!info) return <div className="fontinfo"><p className="k-caption muted editor-loading">Loading Font Info…</p></div>;

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
            {info.warnings.map((w) => <p key={w} className="notice notice-warning k-label">⚠ {w}</p>)}
            {info.incompatible > 0 && (
              <p className="notice notice-warning k-label">⚠ {info.incompatible} glyph{info.incompatible === 1 ? "" : "s"} can't be interpolated. See the Incompatible filter in Font View.</p>
            )}
          </div>
        )}

        <section className="fi-section" aria-labelledby="fi-general">
          <h2 id="fi-general" className="k-subtitle">General</h2>
          <div className="fi-row">
            <label className="info-field"><span className="k-caption muted">Family name</span>
              <Field label="Family name" value={info.familyName} width={260} mono={false} numeric={false}
                onCommit={(v) => apply(() => backend.setFontInfo(v, info.unitsPerEm, info.versionMajor, info.versionMinor))} />
            </label>
            <label className="info-field"><span className="k-caption muted">Units per em</span>
              <Field label="Units per em" value={info.unitsPerEm} onCommit={(v) => {
                const upm = num(v);
                if (!Number.isInteger(upm) || upm < 16 || upm > 16384) return onError("Units per em must be a whole number between 16 and 16384.");
                apply(() => backend.setFontInfo(info.familyName, upm, info.versionMajor, info.versionMinor));
              }} />
            </label>
            <label className="info-field"><span className="k-caption muted">Version</span>
              <Field label="Version" value={`${info.versionMajor}.${String(info.versionMinor).padStart(3, "0")}`} numeric={false}
                onCommit={(v) => {
                  const m = v.trim().match(/^(\d+)(?:\.(\d+))?$/);
                  if (!m) return onError("Write the version as major.minor, e.g. 1.002.");
                  apply(() => backend.setFontInfo(info.familyName, info.unitsPerEm, Number(m[1]), Number(m[2] ?? 0)));
                }} />
            </label>
          </div>
        </section>

        <section className="fi-section" aria-labelledby="fi-axes">
          <div className="fi-head">
            <h2 id="fi-axes" className="k-subtitle">Axes</h2>
            <span className="spacer" />
            <select className="select" value="" aria-label="Add axis" onChange={(e) => {
              const p = e.target.value === "custom" ? { name: "Custom", tag: "XXXX", min: 0, default: 0, max: 100 } : AXIS_PRESETS.find((a) => a.tag === e.target.value);
              if (p) setAxes([...curAxes(), { ...p }]);
            }}>
              <option value="">Add axis…</option>
              {AXIS_PRESETS.filter((p) => !axesDraft.some((a) => a.tag === p.tag)).map((p) => <option key={p.tag} value={p.tag}>{p.name} ({p.tag})</option>)}
              <option value="custom">Custom axis</option>
            </select>
          </div>
          {axesDraft.length === 0 ? (
            <p className="k-caption muted">No axes. Add one to make a variable family with several masters.</p>
          ) : (
            <table className="fi-table">
              <thead><tr><th>Name</th><th>Tag</th><th className="num">Minimum</th><th className="num">Default</th><th className="num">Maximum</th><th aria-label="Actions" /></tr></thead>
              <tbody>
                {axesDraft.map((a, i) => {
                  const set = (patch: Partial<Axis>) => setAxes(curAxes().map((x, j) => (j === i ? { ...x, ...patch } : x)));
                  return (
                    <tr key={i}>
                      <td><Field label="Axis name" value={a.name} width={160} mono={false} numeric={false} onCommit={(v) => set({ name: v })} /></td>
                      <td><Field label="Axis tag" value={a.tag} width={64} numeric={false} onCommit={(v) => set({ tag: v.trim() })} /></td>
                      <td className="num"><Field label="Minimum" value={a.min} onCommit={(v) => set({ min: num(v) })} /></td>
                      <td className="num"><Field label="Default" value={a.default} onCommit={(v) => set({ default: num(v) })} /></td>
                      <td className="num"><Field label="Maximum" value={a.max} onCommit={(v) => set({ max: num(v) })} /></td>
                      <td><button className="icon-btn" aria-label={`Remove ${a.name}`} title="Remove axis" onClick={() => setAxes(curAxes().filter((_, j) => j !== i))}><CloseIcon size={14} /></button></td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
          {axes && (
            <div className="fi-actions">
              <span className="k-caption muted">Axis changes apply to all masters and instances.</span>
              <span className="spacer" />
              <button className="btn btn-secondary btn-sm" onClick={() => setAxes(null)}>Discard</button>
              <button className="btn btn-primary btn-sm" onClick={async () => {
                const a = drafts.current.axes;
                if (!a) return;
                // Removing an axis also changes masters, instances and layers: say so first.
                const removed = info.axes.filter((old) => !a.some((x) => x.tag === old.tag));
                if (removed.length) {
                  const names = removed.map((x) => x.name).join(", ");
                  const ok = window.confirm(
                    `Remove ${names}?\n\nMasters and instances lose their position on ${removed.length === 1 ? "this axis" : "these axes"}, and intermediate and alternate layers that use ${removed.length === 1 ? "it" : "them"} are deleted. You can undo this with Ctrl+Z.`
                  );
                  if (!ok) return;
                }
                if (await apply(() => backend.setAxes(a))) setAxes(null);
              }}>Apply axes</button>
            </div>
          )}
        </section>

        <section className="fi-section" aria-labelledby="fi-masters">
          <div className="fi-head">
            <h2 id="fi-masters" className="k-subtitle">Masters</h2>
            <span className="spacer" />
            <button className="btn btn-secondary btn-sm" onClick={() => setAdding({ name: `Master ${info.masters.length + 1}`, copyFrom: info.defaultMasterId, location: Object.fromEntries(info.axes.map((a) => [a.tag, a.max])) })}>
              <AddIcon size={14} /> Add master
            </button>
          </div>
          {info.masters.map((m) => (
            <div key={m.id} className="fi-master">
              <div className="fi-row">
                <label className="info-field"><span className="k-caption muted">Name</span>
                  <Field label="Master name" value={m.name} width={160} mono={false} numeric={false} onCommit={(v) => updateMaster(m, { name: v })} />
                </label>
                {locFields(m.location, (loc) => updateMaster(m, { location: loc }), `${m.name}`)}
                <span className="info-sep" />
                {(["ascender", "capHeight", "xHeight", "descender", "italicAngle"] as const).map((k) => (
                  <label key={k} className="info-field">
                    <span className="k-caption muted">{{ ascender: "Ascender", capHeight: "Cap height", xHeight: "x-height", descender: "Descender", italicAngle: "Italic angle" }[k]}</span>
                    <Field label={`${m.name} ${k}`} value={m.metrics[k]} width={64} onCommit={(v) => updateMaster(m, { metrics: { ...m.metrics, [k]: num(v) } })} />
                  </label>
                ))}
                <span className="spacer" />
                {m.id === info.defaultMasterId && <span className="pill pill-group" title="At the default location of every axis">Default</span>}
                <button className="icon-btn" aria-label={`Remove ${m.name}`} title={info.masters.length === 1 ? "A font needs at least one master" : "Remove master"} disabled={info.masters.length === 1}
                  onClick={() => window.confirm(`Remove the master "${m.name}" and its outlines and kerning? You can undo this.`) && apply(() => backend.removeMaster(m.id))}>
                  <CloseIcon size={14} />
                </button>
              </div>
            </div>
          ))}
          {adding && (
            <div className="fi-master fi-adding">
              <div className="fi-row">
                <label className="info-field"><span className="k-caption muted">New master name</span>
                  <input className="metric-input text-field" style={{ width: 160 }} value={adding.name} onChange={(e) => setAdding({ ...adding, name: e.target.value })} />
                </label>
                <label className="info-field"><span className="k-caption muted">Copy outlines from</span>
                  <select className="select" value={adding.copyFrom} onChange={(e) => setAdding({ ...adding, copyFrom: e.target.value })}>
                    {info.masters.map((m) => <option key={m.id} value={m.id}>{m.name}</option>)}
                  </select>
                </label>
                {locFields(adding.location, (loc) => setAdding({ ...adding, location: loc }), "New master")}
                <span className="spacer" />
                <button className="btn btn-secondary btn-sm" onClick={() => setAdding(null)}>Cancel</button>
                <button className="btn btn-primary btn-sm" onClick={async () => { if (await apply(() => backend.addMaster(adding.copyFrom, adding.name, adding.location))) setAdding(null); }}>Add</button>
              </div>
              {info.axes.length === 0 && <p className="k-caption muted">Tip: add an axis first so masters can sit at different locations.</p>}
            </div>
          )}
        </section>

        <section className="fi-section" aria-labelledby="fi-instances">
          <div className="fi-head">
            <h2 id="fi-instances" className="k-subtitle">Instances</h2>
            <span className="spacer" />
            <button className="btn btn-secondary btn-sm" disabled={info.axes.length === 0} title={info.axes.length === 0 ? "Add an axis first" : undefined}
              onClick={() => setInstances([...curInst(), { name: `Instance ${curInst().length + 1}`, location: Object.fromEntries(info.axes.map((a) => [a.tag, a.default])), export: true }])}>
              <AddIcon size={14} /> Add instance
            </button>
          </div>
          {instDraft.length === 0 ? (
            <p className="k-caption muted">No instances. Instances are the named styles (e.g. SemiBold) that get exported.</p>
          ) : instDraft.map((it, i) => (
            <div key={i} className="fi-row fi-instance">
              <label className="info-field"><span className="k-caption muted">Name</span>
                <Field label="Instance name" value={it.name} width={160} mono={false} numeric={false} onCommit={(v) => setInstances(curInst().map((x, j) => (j === i ? { ...x, name: v } : x)))} />
              </label>
              {locFields(it.location, (loc) => setInstances(curInst().map((x, j) => (j === i ? { ...x, location: loc } : x))), it.name)}
              <label className="check-row fi-export" title="Export this instance as a static font">
                <input type="checkbox" checked={it.export !== false} aria-label={`Export ${it.name}`}
                  onChange={(e) => setInstances(curInst().map((x, j) => (j === i ? { ...x, export: e.target.checked } : x)))} />
                <span className="k-caption">Export</span>
              </label>
              <span className="spacer" />
              <button className="icon-btn" aria-label={`Remove ${it.name}`} title="Remove instance" onClick={() => setInstances(curInst().filter((_, j) => j !== i))}><CloseIcon size={14} /></button>
            </div>
          ))}
          {instances && (
            <div className="fi-actions">
              <span className="spacer" />
              <button className="btn btn-secondary btn-sm" onClick={() => setInstances(null)}>Discard</button>
              <button className="btn btn-primary btn-sm" onClick={async () => { const list = drafts.current.instances; if (list && await apply(() => backend.setInstances(list))) setInstances(null); }}>Apply instances</button>
            </div>
          )}
        </section>
      </div>
    </div>
    </FieldError.Provider>
  );
}
