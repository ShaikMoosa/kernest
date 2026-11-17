import { useCallback, useEffect, useRef, useState } from "react";
import { backend, type DocStatus, type FeaturesView } from "../lib/backend";
import { t, translateMessage } from "../lib/i18n";

/** Must match the line-height of .fea-gutter and .fea-area in app.css. */
const LINE_HEIGHT = 20;

interface Props {
  version: number;
  onChanged: (status: DocStatus) => void;
  onError: (e: unknown) => void;
}

/** Font Info · Features: automatic switches, generated code, and the hand-written editor. */
export function FeaturesSection({ version, onChanged, onError }: Props) {
  const [view, setView] = useState<FeaturesView | null>(null);
  const [unavailable, setUnavailable] = useState<string | null>(null);
  // Draft while editing, kept in a ref so a toggle landing mid-edit doesn't lose it.
  const [draft, setDraftState] = useState<string | null>(null);
  const draftRef = useRef<string | null>(null);
  const setDraft = (v: string | null) => { draftRef.current = v; setDraftState(v); };
  const areaRef = useRef<HTMLTextAreaElement>(null);
  const gutterRef = useRef<HTMLDivElement>(null);

  const reload = useCallback(() => backend.features().then(
    (v) => { setView(v); setUnavailable(null); },
    (e) => setUnavailable(e instanceof Error ? e.message : String(e)),
  ), []);
  useEffect(() => { reload(); }, [reload, version]);

  const apply = async (task: () => Promise<FeaturesView>) => {
    try {
      const next = await task();
      setView(next);
      onChanged(next.status);
      return true;
    } catch (e) {
      onError(e);
      return false;
    }
  };

  const head = (
    <div className="fi-head">
      <h2 id="fi-features" className="k-subtitle">{t("Features")}</h2>
      <span className="k-caption muted">{t("OpenType layout code. Code you write here replaces the generated feature with the same tag.")}</span>
    </div>
  );
  if (unavailable) return <section className="fi-section" aria-labelledby="fi-features">{head}<p className="k-caption muted">{translateMessage(unavailable)}</p></section>;
  if (!view) return <section className="fi-section" aria-labelledby="fi-features">{head}<p className="k-caption muted">{t("Loading…")}</p></section>;

  const text = draft ?? view.text;
  const lineCount = text.split("\n").length;
  const jump = (line: number) => {
    const el = areaRef.current;
    if (!el) return;
    const ls = text.split("\n");
    let idx = 0;
    for (let i = 0; i < line - 1 && i < ls.length; i++) idx += ls[i].length + 1;
    el.focus();
    el.setSelectionRange(idx, idx + (ls[line - 1]?.length ?? 0));
    el.scrollTop = Math.max(0, (line - 1) * LINE_HEIGHT - el.clientHeight / 2);
    if (gutterRef.current) gutterRef.current.scrollTop = el.scrollTop;
  };
  const toggle = (tag: string, on: boolean) => {
    const next = on ? [...view.automatic, tag] : view.automatic.filter((x) => x !== tag);
    apply(() => backend.setAutomaticFeatures(next));
  };

  return (
    <section className="fi-section" aria-labelledby="fi-features">
      {head}
      {view.available.length === 0 && view.handwritten.length === 0 ? (
        <p className="k-caption muted">{t("No features yet. Name glyph alternates like a.ss01 or ligatures like f_i and they appear here.")}</p>
      ) : (
        <>
          {view.available.length > 0 && (
            <div className="fea-auto">
              {view.available.map((tag) => (
                <label key={tag} className="check-row" title={t("Generate this feature from the glyph names")}>
                  <input type="checkbox" checked={view.automatic.includes(tag)} onChange={(e) => toggle(tag, e.target.checked)} />
                  <span className="k-body fea-tag">{tag}</span>
                </label>
              ))}
            </div>
          )}
          {view.handwritten.length > 0 && (
            <p className="k-caption muted">{t("Written by hand: {tags}. Your code is used as-is.", { tags: view.handwritten.join(", ") })}</p>
          )}
          {view.generated !== "" && (
            <details className="fea-generated-wrap">
              <summary className="k-caption">{t("Generated code")}</summary>
              <pre className="fea-generated" dir="ltr">{view.generated}</pre>
            </details>
          )}
        </>
      )}
      <div className="fea-editor" dir="ltr">
        <div className="fea-gutter" ref={gutterRef} aria-hidden="true">
          {Array.from({ length: lineCount }, (_, i) => <div key={i}>{i + 1}</div>)}
        </div>
        <textarea
          ref={areaRef}
          className="fea-area"
          aria-label={t("Feature code")}
          spellCheck={false}
          wrap="off"
          value={text}
          placeholder={"feature ss01 {\n  sub a by a.ss01;\n} ss01;"}
          onChange={(e) => setDraft(e.target.value)}
          onScroll={(e) => { if (gutterRef.current) gutterRef.current.scrollTop = e.currentTarget.scrollTop; }}
        />
      </div>
      {view.diagnostics.length > 0 ? (
        <ul className="fea-diags">
          {view.diagnostics.map((d, i) => (
            <li key={i}>
              <button type="button" className="fea-diag" onClick={() => jump(d.line)}>
                <span className="fea-diag-loc">{t("Line {line}, column {column}", { line: d.line, column: d.column })}</span>
                <span>{translateMessage(d.message)}</span>
              </button>
            </li>
          ))}
        </ul>
      ) : draft === null && text.trim() !== "" ? (
        <p className="k-caption fea-ok" role="status">{t("Compiled with no errors.")}</p>
      ) : null}
      {draft !== null && (
        <div className="fi-actions">
          <span className="spacer" />
          <button className="btn btn-secondary btn-sm" onClick={() => setDraft(null)}>{t("Discard")}</button>
          <button className="btn btn-primary btn-sm" onClick={async () => { const v = draftRef.current; if (v !== null && await apply(() => backend.setFeatures(v))) setDraft(null); }}>{t("Apply features")}</button>
        </div>
      )}
    </section>
  );
}