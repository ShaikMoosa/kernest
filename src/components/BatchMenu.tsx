import { useEffect, useRef, useState } from "react";
import { ChevronDownIcon } from "./Icons";
import { t } from "../lib/i18n";

const OPERATIONS: { id: string; label: string }[] = [
  { id: "correct-direction", label: "Correct Path Direction" },
  { id: "remove-overlap", label: "Remove Overlap" },
  { id: "tidy-up", label: "Tidy up Paths" },
  { id: "round-coordinates", label: "Round Coordinates" },
  { id: "decompose", label: "Decompose Components" },
  { id: "update-metrics", label: "Update Metrics" },
  { id: "set-anchors", label: "Set Anchors" },
  { id: "create-composite", label: "Create Composites" },
];

/**
 * Font View header "Actions" menu: Rename Glyphs… and the batch commands, which run one
 * edit over every selected glyph (they're unavailable until glyphs are selected).
 */
export function BatchMenu({ count, onRun, onRename }: { count: number; onRun?: (operation: string) => void; onRename?: () => void }) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", key);
    };
  }, [open]);
  return (
    <div className="batch-wrap" ref={ref}>
      <button type="button" className="btn btn-secondary btn-sm" aria-haspopup="menu" aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        {t("Actions")} <ChevronDownIcon />
      </button>
      {open && (
        <div className="batch-menu" role="menu" aria-label={t("Actions")}>
          {onRename && (
            <button type="button" role="menuitem" className="batch-item" onClick={() => { setOpen(false); onRename(); }}>
              {t("Rename Glyphs…")}
            </button>
          )}
          {onRun && (
            <>
              {onRename && <div className="batch-separator" role="separator" />}
              <p className="k-overline muted batch-heading">{count ? t("Selected glyphs") : t("Select glyphs to run these")}</p>
              {OPERATIONS.map((op) => (
                <button key={op.id} type="button" role="menuitem" className="batch-item" disabled={count === 0}
                  onClick={() => { setOpen(false); onRun(op.id); }}>
                  {t(op.label)}
                </button>
              ))}
            </>
          )}
        </div>
      )}
    </div>
  );
}
