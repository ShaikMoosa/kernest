import { useEffect } from "react";
import type { CompareView, FieldDiff } from "../lib/backend";
import { CloseIcon } from "./Icons";
import { t } from "../lib/i18n";

interface Props {
  report: CompareView;
  currentFamily: string;
  onClose: () => void;
}

const infoLabel = (key: string): string => {
  const labels: Record<string, string> = {
    familyName: t("Family name"),
    unitsPerEm: t("Units per em"),
    version: t("Version"),
    designer: t("Designer"),
    designerUrl: t("Designer URL"),
    manufacturer: t("Manufacturer"),
    manufacturerUrl: t("Manufacturer URL"),
    copyright: t("Copyright"),
    license: t("License"),
    licenseUrl: t("License URL"),
    vendorId: t("Vendor ID"),
    description: t("Description"),
    trademark: t("Trademark"),
    sampleText: t("Sample text"),
  };
  return labels[key] ?? key;
};

const metricLabel = (key: string): string => {
  const labels: Record<string, string> = {
    ascender: t("Ascender"),
    capHeight: t("Cap height"),
    xHeight: t("x-height"),
    descender: t("Descender"),
    italicAngle: t("Italic angle"),
  };
  return labels[key] ?? key;
};

function FieldTable({ rows, label }: { rows: FieldDiff[]; label: (key: string) => string }) {
  return (
    <table className="fi-table cmp-table">
      <thead>
        <tr><th>{t("Field")}</th><th>{t("This font")}</th><th>{t("Other font")}</th></tr>
      </thead>
      <tbody>
        {rows.map((d) => (
          <tr key={d.field}>
            <td className="k-label">{label(d.field)}</td>
            <td>{d.current || "—"}</td>
            <td>{d.other || "—"}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Empty() {
  return <p className="k-caption muted cmp-empty">{t("No differences.")}</p>;
}

/**
 * S42 · The compare-fonts report window. Read-only: it lists what differs between the
 * open font and the picked one — Font Info, master metrics, glyph outlines and widths,
 * and kerning — without changing either font.
 */
export function CompareDialog({ report, currentFamily, onClose }: Props) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const onlyIn = (side?: "current" | "other") =>
    side === "current" ? t("Only in this font") : side === "other" ? t("Only in the other font") : "";

  return (
    <div className="scrim" role="presentation">
      <div className="dialog compare-dialog" role="dialog" aria-modal="true" aria-labelledby="compare-title">
        <header className="dialog-header">
          <h2 id="compare-title" className="k-subtitle">{t("Compare Fonts")}</h2>
          <button type="button" className="icon-btn" aria-label={t("Close")} onClick={onClose}><CloseIcon /></button>
        </header>
        <div className="dialog-body">
          <p className="k-caption muted">
            {t("Compared with {name}", { name: currentFamily })} · {report.fileName}
            {report.familyName ? ` (${report.familyName})` : ""}
          </p>

          <section className="cmp-section" aria-labelledby="cmp-info">
            <h3 id="cmp-info" className="k-label">{t("Font Info")} <span className="muted">({report.info.length})</span></h3>
            {report.info.length ? <FieldTable rows={report.info} label={infoLabel} /> : <Empty />}
          </section>

          <section className="cmp-section" aria-labelledby="cmp-masters">
            <h3 id="cmp-masters" className="k-label">{t("Masters")} <span className="muted">({report.masters.length})</span></h3>
            {report.masters.length === 0 && <Empty />}
            {report.masters.map((m) => (
              <div key={m.name} className="cmp-master">
                <span className="k-label cmp-master-name">{m.name}</span>
                {m.onlyIn ? <span className="pill pill-group">{onlyIn(m.onlyIn)}</span>
                  : m.metrics.length ? <FieldTable rows={m.metrics} label={metricLabel} /> : <Empty />}
              </div>
            ))}
          </section>

          <section className="cmp-section" aria-labelledby="cmp-glyphs">
            <h3 id="cmp-glyphs" className="k-label">{t("Glyphs")} <span className="muted">({report.glyphs.length})</span></h3>
            {report.glyphs.length === 0 ? <Empty /> : (
              <table className="fi-table cmp-table">
                <thead>
                  <tr>
                    <th rowSpan={2}>{t("Glyph")}</th>
                    <th colSpan={3}>{t("This font")}</th>
                    <th colSpan={3}>{t("Other font")}</th>
                  </tr>
                  <tr>
                    <th className="num">{t("Paths")}</th><th className="num">{t("Nodes")}</th><th className="num">{t("Width")}</th>
                    <th className="num">{t("Paths")}</th><th className="num">{t("Nodes")}</th><th className="num">{t("Width")}</th>
                  </tr>
                </thead>
                <tbody>
                  {report.glyphs.map((g) => (
                    <tr key={g.name}>
                      <td className="k-label cmp-side">{g.name}</td>
                      {g.onlyIn === "other" ? <td colSpan={3} className="muted">{onlyIn("other")}</td> : (
                        <>
                          <td className="num">{g.paths?.[0]}</td><td className="num">{g.nodes?.[0]}</td><td className="num">{g.width?.[0]}</td>
                        </>
                      )}
                      {g.onlyIn === "current" ? <td colSpan={3} className="muted">{onlyIn("current")}</td> : (
                        <>
                          <td className="num">{g.paths?.[1]}</td><td className="num">{g.nodes?.[1]}</td><td className="num">{g.width?.[1]}</td>
                        </>
                      )}
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </section>

          <section className="cmp-section" aria-labelledby="cmp-kerning">
            <h3 id="cmp-kerning" className="k-label">{t("Kerning")} <span className="muted">({report.kerning.length})</span></h3>
            {report.kerning.length === 0 ? <Empty /> : (
              <table className="fi-table cmp-table">
                <thead>
                  <tr><th>{t("Left")}</th><th>{t("Right")}</th><th className="num">{t("This font")}</th><th className="num">{t("Other font")}</th></tr>
                </thead>
                <tbody>
                  {report.kerning.map((k) => (
                    <tr key={`${k.left}|${k.right}`}>
                      <td className="cmp-side">{k.left}</td>
                      <td className="cmp-side">{k.right}</td>
                      <td className="num">{k.current ?? "—"}</td>
                      <td className="num">{k.other ?? "—"}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </section>
        </div>
        <footer className="dialog-footer">
          <span className="spacer" />
          <button type="button" className="btn btn-secondary" onClick={onClose}>{t("Close")}</button>
        </footer>
      </div>
    </div>
  );
}