import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { backend, type DocStatus, type KernPair, type KerningList, type TextLayout } from "../lib/backend";
import { AddIcon, ChevronDownIcon, CloseIcon, SearchIcon } from "./Icons";
import { t, tn } from "../lib/i18n";

function ValueField({ value, onCommit }: { value: number; onCommit: (v: number) => void }) {
  const shown = String(value);
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);
  return (
    <input className={`metric-input kern-value${value > 0 ? " is-positive" : ""}`} value={text} aria-label={t("Kerning value")} onChange={(e) => setText(e.target.value)}
      onClick={(e) => e.stopPropagation()}
      onBlur={(e) => {
        const raw = e.currentTarget.value;
        const v = Number(raw.trim().replace("−", "-") || "0");
        if (Number.isFinite(v) && raw !== shown) onCommit(v);
        else setText(shown);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
        if (e.key === "Escape") { setText(shown); (e.target as HTMLInputElement).blur(); }
      }} />
  );
}

/** The selected pair drawn large, with the first glyph of each group standing in for it. */
function PairPreview({ masterId, left, right, version }: { masterId: string; left: string | null; right: string | null; version: number }) {
  const [layout, setLayout] = useState<TextLayout | null>(null);
  useEffect(() => {
    if (!left || !right) { setLayout(null); return; }
    let live = true;
    backend.textLayout(`/${left} /${right}`.replace(/ /g, ""), masterId).then((l) => { if (live) setLayout(l); }, () => { if (live) setLayout(null); });
    return () => { live = false; };
  }, [masterId, left, right, version]);
  const items = layout?.lines[0] ?? [];
  const asc = layout?.metrics.ascender ?? 800, desc = layout?.metrics.descender ?? -200;
  let x = 0;
  const paths = items.map((it, i) => {
    const at = x;
    x += it.type === "glyph" ? it.width + it.kern : 500;
    return it.type === "glyph" && it.svg ? <path key={i} d={it.svg} transform={`translate(${at} 0) scale(1 -1)`} /> : null;
  });
  return (
    <div className="kern-preview" aria-hidden="true">
      {layout && x > 0 ? <svg viewBox={`${-60} ${-asc} ${x + 120} ${asc - desc}`}>{paths}</svg> : <span className="k-caption muted">{t("Select a pair to preview it")}</span>}
    </div>
  );
}

/** Kerning › Copy keeps the pairs here, so Paste works in another master or font. */
let copiedKerning: { from: string; pairs: KernPair[] } | null = null;

type Match = "contains" | "exact";
interface SearchOptions { match: Match; groups: boolean; exceptions: boolean; left: boolean; right: boolean; selected: boolean }
const DEFAULT_OPTIONS: SearchOptions = { match: "contains", groups: true, exceptions: true, left: true, right: true, selected: false };

interface Props {
  masterId: string;
  masterName: string;
  version: number;
  onChanged: (status: DocStatus) => void;
  onError: (e: unknown) => void;
  /** Glyphs selected in the Font View, for "Only glyphs selected in Font View". */
  fontSelection?: string[];
  /** Opens a text tab, where Alt+←/→ kerns at the cursor. */
  onOpenText?: () => void;
  /** A pair to select, from a click on a kerning bar in a text tab; `seq` changes on each click. */
  focus?: { left: string; right: string; seq: number } | null;
}

/** S07 · Kerning window: every pair in the current master. */
export function KerningView({ masterId, masterName, version, onChanged, onError, fontSelection = [], onOpenText, focus }: Props) {
  const [data, setData] = useState<KerningList | null>(null);
  const [query, setQuery] = useState("");
  const [draft, setDraft] = useState({ left: "", right: "", value: "" });
  const [showMembers, setShowMembers] = useState(false);
  const [allMasters, setAllMasters] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [copied, setCopied] = useState(copiedKerning);
  const [options, setOptions] = useState<SearchOptions>(DEFAULT_OPTIONS);
  const [selected, setSelected] = useState<string | null>(null);
  const [menu, setMenu] = useState(false);
  const tableRef = useRef<HTMLTableElement>(null);
  // Select the pair a text tab asked for, clearing any search that would hide it.
  useEffect(() => {
    if (!focus) return;
    setQuery("");
    setOptions(DEFAULT_OPTIONS);
    setSelected(`${focus.left}|${focus.right}`);
  }, [focus]);
  useEffect(() => {
    tableRef.current?.querySelector("tr.is-selected")?.scrollIntoView({ block: "nearest" });
  }, [selected, data]);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => { if (!menuRef.current?.contains(e.target as Node)) setMenu(false); };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") setMenu(false); };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", esc);
    return () => { document.removeEventListener("mousedown", close); document.removeEventListener("keydown", esc); };
  }, [menu]);

  const reload = useCallback(() => backend.kerningList(masterId).then(setData, onError), [masterId, onError]);
  useEffect(() => { reload(); }, [reload, version]);

  const tidy = async (compress: boolean) => {
    try {
      const r = await backend.tidyKerning(masterId, allMasters, compress);
      onChanged(r.status);
      await reload();
      const { moved, removed, kept } = r.tidy;
      setNote(compress
        ? t("Compress: {moved} moved to groups, {removed} removed as the same as their group, {kept} kept as exceptions.", { moved, removed, kept })
        : t("Clean Up: {removed} pairs removed (zero, naming nothing, or the same as their group).", { removed }));
    } catch (e) {
      onError(e);
    }
  };
  const copy = () => {
    copiedKerning = { from: masterName, pairs: (data?.pairs ?? []).map((p) => ({ ...p })) };
    setCopied(copiedKerning);
    const text = copiedKerning.pairs.map((p) => `${p.left}\t${p.right}\t${p.value}`).join("\n");
    navigator.clipboard?.writeText(text).catch(() => {});
    setNote(t("Copied {n} pairs from {master}.", { n: copiedKerning.pairs.length, master: masterName }));
  };
  const paste = async (overwrite: boolean) => {
    if (!copiedKerning) return;
    try {
      onChanged(await backend.pasteKerning(masterId, copiedKerning.pairs, overwrite));
      await reload();
      setNote(overwrite
        ? t("Pasted {n} pairs from {master}, replacing values already here.", { n: copiedKerning.pairs.length, master: copiedKerning.from })
        : t("Pasted pairs from {master}, keeping values already here.", { master: copiedKerning.from }));
    } catch (e) {
      onError(e);
    }
  };
  const membersOf = (side: string, left: boolean) => (left ? data?.leftMembers : data?.rightMembers)?.[side.slice(1)] ?? [];
  const members = (side: string, left: boolean) => {
    if (!showMembers || !side.startsWith("@")) return null;
    const list = membersOf(side, left);
    return <div className="kern-members k-caption muted">{list.length ? list.join(" ") : t("no glyphs")}</div>;
  };

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

  const isGroupPair = (p: KernPair) => p.left.startsWith("@") && p.right.startsWith("@");
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    const picked = new Set(fontSelection);
    const sideMatches = (side: string, left: boolean) => {
      if (options.selected) {
        const glyphs = side.startsWith("@") ? membersOf(side, left) : [side];
        if (!glyphs.some((g) => picked.has(g))) return false;
      }
      if (!q) return true;
      const s = side.toLowerCase();
      return options.match === "exact" ? s === q || s === `@${q}` : s.includes(q);
    };
    return (data?.pairs ?? []).filter((p) => {
      if (isGroupPair(p) ? !options.groups : !options.exceptions) return false;
      if (!q && !options.selected) return true;
      return (options.left && sideMatches(p.left, true)) || (options.right && sideMatches(p.right, false));
    });
    // membersOf reads data, which is a dependency already.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [data, query, options, fontSelection]);
  const groupPairs = data?.pairs.filter(isGroupPair).length ?? 0;
  const pairKey = (p: KernPair) => `${p.left}|${p.right}`;
  const current = shown.find((p) => pairKey(p) === selected) ?? null;
  // The preview draws the first glyph of a group in place of the group.
  const stand = (side: string, left: boolean) => (side.startsWith("@") ? membersOf(side, left)[0] ?? null : side);

  return (
    <div className="kerningview">
      <div className="toolbar">
        <h2 className="k-subtitle">{t("Kerning — {master}", { master: masterName })}</h2>
        <span className="spacer" />
        <label className="search kern-search">
          <SearchIcon />
          <input placeholder={t("Filter glyphs or @groups")} value={query} onChange={(e) => setQuery(e.target.value)} aria-label={t("Filter glyphs or @groups")} />
        </label>
      </div>
      {note && (
        <p className="notice k-caption kern-note" role="status">
          {note} <button type="button" className="link-btn" onClick={() => setNote(null)}>{t("Close")}</button>
        </p>
      )}
      <div className="kern-body">
        <div className="kern-main">
          <div className="kern-table-wrap">
            <table className="kern-table" ref={tableRef}>
              <thead>
                <tr><th>{t("Left")}</th><th className="num">{t("Value")}</th><th>{t("Right")}</th><th>{t("Type")}</th><th aria-label={t("Actions")} /></tr>
              </thead>
              <tbody>
                <tr className="add-row">
                  <td><input className="metric-input side-input" placeholder={t("glyph or @group")} value={draft.left} onChange={(e) => setDraft({ ...draft, left: e.target.value })} list="kern-groups" aria-label={t("Left")} /></td>
                  <td className="num"><input className="metric-input kern-value" placeholder="0" value={draft.value} onChange={(e) => setDraft({ ...draft, value: e.target.value })} aria-label={t("Kerning value")} /></td>
                  <td><input className="metric-input side-input" placeholder={t("glyph or @group")} value={draft.right} onChange={(e) => setDraft({ ...draft, right: e.target.value })} list="kern-groups" aria-label={t("Right")} /></td>
                  <td colSpan={2}>
                    <button className="btn btn-secondary btn-sm" onClick={async () => {
                      const v = Number(draft.value.replace("−", "-"));
                      if (!Number.isFinite(v) || v === 0) return onError(t("Enter a non-zero kerning value."));
                      if (await set(draft.left, draft.right, v)) { setSelected(`${draft.left}|${draft.right}`); setDraft({ left: "", right: "", value: "" }); }
                    }}><AddIcon size={14} /> {t("Add pair")}</button>
                  </td>
                </tr>
                {shown.map((p) => {
                  const group = isGroupPair(p);
                  const key = pairKey(p);
                  return (
                    <tr key={key} className={selected === key ? "is-selected" : undefined} onClick={() => setSelected(key)} aria-selected={selected === key}>
                      <td className={p.left.startsWith("@") ? "side group" : "side glyph"}>{p.left}{members(p.left, true)}</td>
                      <td className="num"><ValueField value={p.value} onCommit={(v) => set(p.left, p.right, v)} /></td>
                      <td className={p.right.startsWith("@") ? "side group" : "side glyph"}>{p.right}{members(p.right, false)}</td>
                      <td><span className={`pill ${group ? "pill-group" : "pill-exception"}`}>{group ? t("group") : t("exception")}</span></td>
                      <td><button className="icon-btn kern-delete" aria-label={t("Delete {left} {right}", { left: p.left, right: p.right })} title={t("Delete pair")} onClick={(e) => { e.stopPropagation(); set(p.left, p.right, 0); }}><CloseIcon size={14} /></button></td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
            {data && shown.length === 0 && (
              <div className="empty">
                <p className="k-subtitle">{data.pairs.length ? t("No pairs match “{query}”", { query }) : t("No kerning pairs in {master}", { master: masterName })}</p>
                <p className="k-caption muted">{t("Open a text tab (Ctrl+T), put the cursor between two glyphs and press Alt+←/→, or add a pair above.")}</p>
                {!data.pairs.length && (
                  <div className="row">
                    {onOpenText && <button type="button" className="btn btn-primary" onClick={onOpenText}>{t("Open a Text Tab")}</button>}
                    {copied && <button type="button" className="btn btn-secondary" onClick={() => paste(false)}>{t("Paste pairs from {master}", { master: copied.from })}</button>}
                  </div>
                )}
              </div>
            )}
            <datalist id="kern-groups">{data?.groups.map((g) => <option key={g} value={`@${g}`} />)}</datalist>
          </div>
          <footer className="kern-footer">
            <button type="button" className="btn btn-secondary btn-sm" disabled={!current} title={t("Delete pair")} aria-label={t("Delete the selected pair")}
              onClick={() => { if (current) set(current.left, current.right, 0).then(() => setSelected(null)); }}>−</button>
            <div className="batch-wrap kern-actions" ref={menuRef}>
              <button type="button" className="btn btn-secondary btn-sm" aria-haspopup="menu" aria-expanded={menu} onClick={() => setMenu((m) => !m)}>{t("Actions")} <ChevronDownIcon /></button>
              {menu && (
                <div className="batch-menu kern-actions-menu" role="menu" aria-label={t("Actions")}>
                  <button type="button" role="menuitem" className="batch-item" title={t("Removes zero pairs, pairs naming missing glyphs or groups, and exceptions equal to their group")} onClick={() => { setMenu(false); tidy(false); }}>{t("Clean Up")}</button>
                  <button type="button" role="menuitem" className="batch-item" title={t("Moves glyph pairs one step up to their groups (T–m → @T–m → @T–@n); real exceptions stay")} onClick={() => { setMenu(false); tidy(true); }}>{t("Compress")}</button>
                  <label className="check-row batch-check" title={t("Compress and Clean Up work on every master")}>
                    <input type="checkbox" checked={allMasters} onChange={(e) => setAllMasters(e.target.checked)} />
                    <span>{t("All masters")}</span>
                  </label>
                  <div className="batch-separator" role="separator" />
                  <button type="button" role="menuitem" className="batch-item" disabled={!data?.pairs.length} onClick={() => { setMenu(false); copy(); }}>{t("Copy")}</button>
                  <button type="button" role="menuitem" className="batch-item" disabled={!copied} onClick={() => { setMenu(false); paste(true); }}>{t("Paste (Overwrite)")}</button>
                  <button type="button" role="menuitem" className="batch-item" disabled={!copied} onClick={() => { setMenu(false); paste(false); }}>{t("Paste (Keep)")}</button>
                  <div className="batch-separator" role="separator" />
                  <label className="check-row batch-check" title={t("List the glyphs in each group under its name")}>
                    <input type="checkbox" checked={showMembers} onChange={(e) => setShowMembers(e.target.checked)} />
                    <span>{t("Show All Glyphs")}</span>
                  </label>
                </div>
              )}
            </div>
            <span className="spacer" />
            <span className="k-caption muted">
              {current ? `${t("1 selected")} · ` : ""}
              {t("{pairs} pairs ({groups} group, {exceptions} exceptions)", { pairs: data?.pairs.length ?? 0, groups: groupPairs, exceptions: (data?.pairs.length ?? 0) - groupPairs })}
              {shown.length !== (data?.pairs.length ?? 0) ? ` · ${tn(shown.length, "{n} shown", "{n} shown")}` : ""}
              {" · "}{t("A value of 0 deletes a pair")}
            </span>
          </footer>
        </div>
        <aside className="kern-side" aria-label={t("Pair details")}>
          <PairPreview masterId={masterId} version={version} left={current ? stand(current.left, true) : null} right={current ? stand(current.right, false) : null} />
          <p className="k-overline muted">{t("Search options")}</p>
          <select className="select kern-match" value={options.match} onChange={(e) => setOptions({ ...options, match: e.target.value as Match })} aria-label={t("How the filter matches")}>
            <option value="contains">{t("Pair contains")}</option>
            <option value="exact">{t("Pair side is exactly")}</option>
          </select>
          <label className="check-row"><input type="checkbox" checked={options.groups} onChange={(e) => setOptions({ ...options, groups: e.target.checked })} /><span className="k-body">{t("Is a group")}</span></label>
          <label className="check-row"><input type="checkbox" checked={options.exceptions} onChange={(e) => setOptions({ ...options, exceptions: e.target.checked })} /><span className="k-body">{t("Is not a group")}</span></label>
          <label className="check-row"><input type="checkbox" checked={options.left} onChange={(e) => setOptions({ ...options, left: e.target.checked })} /><span className="k-body">{t("Search left side")}</span></label>
          <label className="check-row"><input type="checkbox" checked={options.right} onChange={(e) => setOptions({ ...options, right: e.target.checked })} /><span className="k-body">{t("Search right side")}</span></label>
          <label className="check-row" title={fontSelection.length ? undefined : t("Select glyphs in the Font tab first")}>
            <input type="checkbox" checked={options.selected} disabled={!fontSelection.length && !options.selected} onChange={(e) => setOptions({ ...options, selected: e.target.checked })} />
            <span className="k-body">{t("Only glyphs selected in Font View")}</span>
          </label>
          <button type="button" className="btn btn-link kern-reset" onClick={() => setOptions(DEFAULT_OPTIONS)}>{t("Reset options")}</button>
          {current && ([[current.left, true], [current.right, false]] as const).filter(([side]) => side.startsWith("@")).map(([side, left]) => (
            <div key={`${side}:${left}`} className="kern-group-members">
              <p className="k-caption muted">{left ? t("Group {name} (left side)", { name: side }) : t("Group {name} (right side)", { name: side })}</p>
              <div className="kern-chips">{membersOf(side, left).map((g) => <span key={g} className="pill pill-static">{g}</span>)}</div>
            </div>
          ))}
        </aside>
      </div>
    </div>
  );
}
