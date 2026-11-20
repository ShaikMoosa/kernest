import { useCallback, useEffect, useMemo, useState } from "react";
import { backend, type DocStatus, type KernPair, type KerningList } from "../lib/backend";
import { AddIcon, CloseIcon, SearchIcon } from "./Icons";
import { t } from "../lib/i18n";

function ValueField({ value, onCommit }: { value: number; onCommit: (v: number) => void }) {
  const shown = String(value);
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);
  return (
    <input className="metric-input kern-value" value={text} aria-label={t("Kerning value")} onChange={(e) => setText(e.target.value)}
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

/** Kerning › Copy keeps the pairs here, so Paste works in another master or font. */
let copiedKerning: { from: string; pairs: KernPair[] } | null = null;

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
  const [showMembers, setShowMembers] = useState(false);
  const [allMasters, setAllMasters] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [copied, setCopied] = useState(copiedKerning);

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
  const members = (side: string, left: boolean) => {
    if (!showMembers || !side.startsWith("@")) return null;
    const list = (left ? data?.leftMembers : data?.rightMembers)?.[side.slice(1)] ?? [];
    return <div className="kern-members k-caption muted">{list.length ? list.join(" ") : t("no glyphs")}</div>;
  };

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
        <h2 className="k-subtitle">{t("Kerning — {master}", { master: masterName })}</h2>
        <span className="spacer" />
        <div className="kern-tools" role="group" aria-label={t("Kerning tools")}>
          <label className="check-row" title={t("List the glyphs in each group under its name")}>
            <input type="checkbox" checked={showMembers} onChange={(e) => setShowMembers(e.target.checked)} />
            <span className="k-body">{t("Show All Glyphs")}</span>
          </label>
          <label className="check-row" title={t("Compress and Clean Up work on every master")}>
            <input type="checkbox" checked={allMasters} onChange={(e) => setAllMasters(e.target.checked)} />
            <span className="k-body">{t("All masters")}</span>
          </label>
          <button className="btn btn-secondary btn-sm" title={t("Removes zero pairs, pairs naming missing glyphs or groups, and exceptions equal to their group")} onClick={() => tidy(false)}>{t("Clean Up")}</button>
          <button className="btn btn-secondary btn-sm" title={t("Moves glyph pairs one step up to their groups (T–m → @T–m → @T–@n); real exceptions stay")} onClick={() => tidy(true)}>{t("Compress")}</button>
          <button className="btn btn-secondary btn-sm" disabled={!data?.pairs.length} onClick={copy}>{t("Copy")}</button>
          <select className="metric-input kern-paste" aria-label={t("Paste kerning")} value="" disabled={!copied}
            onChange={(e) => { if (e.target.value) paste(e.target.value === "overwrite"); }}>
            <option value="">{t("Paste…")}</option>
            <option value="overwrite">{t("Paste (Overwrite)")}</option>
            <option value="keep">{t("Paste (Keep)")}</option>
          </select>
        </div>
        <label className="search kern-search">
          <SearchIcon />
          <input placeholder={t("Filter glyphs or @groups")} value={query} onChange={(e) => setQuery(e.target.value)} />
        </label>
      </div>
      {note && (
        <p className="notice k-caption kern-note" role="status">
          {note} <button type="button" className="link-btn" onClick={() => setNote(null)}>{t("Close")}</button>
        </p>
      )}
      <div className="kern-table-wrap">
        <table className="kern-table">
          <thead>
            <tr><th>{t("Left")}</th><th>{t("Right")}</th><th className="num">{t("Value")}</th><th>{t("Type")}</th><th aria-label={t("Actions")} /></tr>
          </thead>
          <tbody>
            <tr className="add-row">
              <td><input className="metric-input side-input" placeholder={t("glyph or @group")} value={draft.left} onChange={(e) => setDraft({ ...draft, left: e.target.value })} list="kern-groups" /></td>
              <td><input className="metric-input side-input" placeholder={t("glyph or @group")} value={draft.right} onChange={(e) => setDraft({ ...draft, right: e.target.value })} list="kern-groups" /></td>
              <td className="num"><input className="metric-input kern-value" placeholder="0" value={draft.value} onChange={(e) => setDraft({ ...draft, value: e.target.value })} /></td>
              <td colSpan={2}>
                <button className="btn btn-secondary btn-sm" onClick={async () => {
                  const v = Number(draft.value.replace("−", "-"));
                  if (!Number.isFinite(v) || v === 0) return onError(t("Enter a non-zero kerning value."));
                  if (await set(draft.left, draft.right, v)) setDraft({ left: "", right: "", value: "" });
                }}><AddIcon size={14} /> {t("Add pair")}</button>
              </td>
            </tr>
            {shown.map((p) => {
              const group = p.left.startsWith("@") && p.right.startsWith("@");
              return (
                <tr key={`${p.left}|${p.right}`}>
                  <td className={p.left.startsWith("@") ? "side group" : "side"}>{p.left}{members(p.left, true)}</td>
                  <td className={p.right.startsWith("@") ? "side group" : "side"}>{p.right}{members(p.right, false)}</td>
                  <td className="num"><ValueField value={p.value} onCommit={(v) => set(p.left, p.right, v)} /></td>
                  <td><span className={`pill ${group ? "pill-group" : "pill-exception"}`}>{group ? t("group") : t("exception")}</span></td>
                  <td><button className="icon-btn" aria-label={t("Delete {left} {right}", { left: p.left, right: p.right })} title={t("Delete pair")} onClick={() => set(p.left, p.right, 0)}><CloseIcon size={14} /></button></td>
                </tr>
              );
            })}
          </tbody>
        </table>
        {data && shown.length === 0 && (
          <div className="empty">
            <p className="k-subtitle">{data.pairs.length ? t("No pairs match “{query}”", { query }) : t("No kerning pairs in {master}", { master: masterName })}</p>
            <p className="k-caption muted">{t("Open a text tab (Ctrl+T), put the cursor between two glyphs and press Alt+←/→, or add a pair above.")}</p>
          </div>
        )}
        <datalist id="kern-groups">{data?.groups.map((g) => <option key={g} value={`@${g}`} />)}</datalist>
      </div>
      <footer className="statusbar k-caption">
        <span>{t("Shown: {shown} · Pairs: {pairs} (group: {groups}, exceptions: {exceptions})", { shown: shown.length, pairs: data?.pairs.length ?? 0, groups: groupPairs, exceptions: (data?.pairs.length ?? 0) - groupPairs })}</span>
        <span className="spacer" />
        <span>{t("A value of 0 deletes a pair")}</span>
      </footer>
    </div>
  );
}
