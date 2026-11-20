import { useState } from "react";
import { DEFAULT_SAMPLES, loadSamples, saveSamples } from "../lib/samples";
import { AddIcon, CloseIcon } from "./Icons";
import { t } from "../lib/i18n";

/** Settings › Sample strings: the texts in the text tab's Samples menu. */
export function SampleStringsSettings() {
  const [list, setList] = useState<string[]>(loadSamples);
  const update = (next: string[]) => { setList(next); saveSamples(next); };
  return (
    <>
      <div className="settings-row">
        <span className="k-caption muted">{t("Shown in the Samples menu of text tabs. Type /glyphname for glyphs without a character.")}</span>
        <span className="spacer" />
        <button type="button" className="btn btn-secondary" onClick={() => update(DEFAULT_SAMPLES)}>{t("Reset All")}</button>
      </div>
      <ul className="settings-list sample-list">
        {list.map((text, i) => (
          <li key={i} className="settings-item sample-item">
            <input className="text-input" value={text} aria-label={t("Sample string {n}", { n: i + 1 })} spellCheck={false}
              onChange={(e) => setList(list.map((s, j) => (j === i ? e.target.value : s)))}
              onBlur={() => saveSamples(list)} />
            <button type="button" className="icon-btn" aria-label={t("Remove sample string {n}", { n: i + 1 })} onClick={() => update(list.filter((_, j) => j !== i))}>
              <CloseIcon size={14} />
            </button>
          </li>
        ))}
      </ul>
      <div className="settings-row">
        <button type="button" className="btn btn-secondary" onClick={() => setList([...list, ""])}><AddIcon size={14} /> {t("Add sample")}</button>
      </div>
    </>
  );
}
