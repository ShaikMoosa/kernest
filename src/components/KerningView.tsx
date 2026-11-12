import { useCallback, useEffect, useMemo, useState } from "react";
import { backend, type DocStatus, type KerningList } from "../lib/backend";
import { AddIcon, CloseIcon, SearchIcon } from "./Icons";

function ValueField({ value, onCommit }: { value: number; onCommit: (v: number) => void }) {
  const shown = String(value);
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);
  return (
    <input className="metric-input kern-value" value={text} aria-label="Kerning value" onChange={(e) => setText(e.target.value)}
      onBlur={() => {
        const v = Number(text.trim().replace("−", "-") || "0");
        if (Number.isFinite(v) && text !== shown) onCommit(v);
        else setText(shown);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
        if (e.key === "Escape") { setText(shown); (e.target as HTMLInputElement).blur(); }
      }} />
  );
}

interface Props {
  masterId: string;
  masterName: string;
  version: number;
  onChanged: (status: DocStatus) => void;
  onError: (e: unknown) => void;
}

/** S07 · Kerning window: every pair in the current master. */
export function KerningView({ masterId, masterName, version, onChanged, onError }: Props) {
  const [data, setData] = useState<KerningList | null>(null);
  const [query, setQuery] = useState("");
  const [draft, setDraft] = useState({ left: "", right: "", value: "" });

  const reload = useCallback(() => backend.kerningList(masterId).then(setData, onError), [masterId, onError]);
  useEffect(() => { reload(); }, [reload, version]);

  const set = async (left: string, right: string, value: number) => {
    try {
      onChanged(await backend.setKerningPair(masterId, left, right, value));
      await reload();
      return true;
    } catch (e) {
      onError(e);
      return false;
    }
  };

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    const pairs = data?.pairs ?? [];
    return q ? pairs.filter((p) => p.left.toLowerCase().includes(q) || p.right.toLowerCase().includes(q)) : pairs;
  }, [data, query]);
  const groupPairs = data?.pairs.filter((p) => p.left.startsWith("@") && p.right.startsWith("@")).length ?? 0;

  return (
    <div className="kerningview">
      <div className="toolbar">
        <h2 className="k-subtitle">Kerning — {masterName}</h2>
        <span className="spacer" />
        <label className="search kern-search">
          <SearchIcon />
          <input placeholder="Filter glyphs or @groups" value={query} onChange={(e) => setQuery(e.target.value)} />
        </label>
      </div>
      <div className="kern-table-wrap">
        <table className="kern-table">
          <thead>
            <tr><th>Left</th><th>Right</th><th className="num">Value</th><th>Type</th><th aria-label="Actions" /></tr>
          </thead>
          <tbody>
            <tr className="add-row">
              <td><input className="metric-input side-input" placeholder="glyph or @group" value={draft.left} onChange={(e) => setDraft({ ...draft, left: e.target.value })} list="kern-groups" /></td>
              <td><input className="metric-input side-input" placeholder="glyph or @group" value={draft.right} onChange={(e) => setDraft({ ...draft, right: e.target.value })} list="kern-groups" /></td>
              <td className="num"><input className="metric-input kern-value" placeholder="0" value={draft.value} onChange={(e) => setDraft({ ...draft, value: e.target.value })} /></td>
              <td colSpan={2}>
                <button className="btn btn-secondary btn-sm" onClick={async () => {
                  const v = Number(draft.value.replace("−", "-"));
                  if (!Number.isFinite(v) || v === 0) return onError("Enter a non-zero kerning value.");
                  if (await set(draft.left, draft.right, v)) setDraft({ left: "", right: "", value: "" });
                }}><AddIcon size={14} /> Add pair</button>
              </td>
            </tr>
            {shown.map((p) => {
              const group = p.left.startsWith("@") && p.right.startsWith("@");
              return (
                <tr key={`${p.left}|${p.right}`}>
                  <td className={p.left.startsWith("@") ? "side group" : "side"}>{p.left}</td>
                  <td className={p.right.startsWith("@") ? "side group" : "side"}>{p.right}</td>
                  <td className="num"><ValueField value={p.value} onCommit={(v) => set(p.left, p.right, v)} /></td>
                  <td><span className={`pill ${group ? "pill-group" : "pill-exception"}`}>{group ? "group" : "exception"}</span></td>
                  <td><button className="icon-btn" aria-label={`Delete ${p.left} ${p.right}`} title="Delete pair" onClick={() => set(p.left, p.right, 0)}><CloseIcon size={14} /></button></td>
                </tr>
              );
            })}
          </tbody>
        </table>
        {data && shown.length === 0 && (
          <div className="empty">
            <p className="k-subtitle">{data.pairs.length ? `No pairs match “${query}”` : `No kerning pairs in ${masterName}`}</p>
            <p className="k-caption muted">Open a text tab (Ctrl+T), put the cursor between two glyphs and press Alt+←/→, or add a pair above.</p>
          </div>
        )}
        <datalist id="kern-groups">{data?.groups.map((g) => <option key={g} value={`@${g}`} />)}</datalist>
      </div>
      <footer className="statusbar k-caption">
        <span>{shown.length} shown · {data?.pairs.length ?? 0} pairs ({groupPairs} group, {(data?.pairs.length ?? 0) - groupPairs} exceptions)</span>
        <span className="spacer" />
        <span>A value of 0 deletes a pair</span>
      </footer>
    </div>
  );
}
