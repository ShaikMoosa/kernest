import { useEffect, useRef, useState } from "react";
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

/** Font View header menu: runs one edit over every selected glyph. */
export function BatchMenu({ count, onRun }: { count: number; onRun: (operation: string) => void }) {
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
      <button
        type="button"
        className="btn btn-secondary btn-sm"
        disabled={count === 0}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        {t("Batch")}
      </button>
      {open && (
        <div className="batch-menu" role="menu" aria-label={t("Batch")}>
          {OPERATIONS.map((op) => (
            <button
              key={op.id}
              type="button"
              role="menuitem"
              className="batch-item"
              onClick={() => {
                setOpen(false);
                onRun(op.id);
              }}
            >
              {t(op.label)}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}