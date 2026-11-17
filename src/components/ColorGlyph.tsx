import { useId } from "react";
import type { ColorLayerView, Paint } from "../lib/backend";

/** A palette colour as CSS, with extra opacity (missing colours show as grey). */
function css(palette: string[], index: number, alpha = 1): { color: string; opacity: number } {
  return { color: palette[index] ?? "#888888", opacity: alpha };
}

/**
 * Draws colour layers (bottom to top) in font units, y up: put it inside an SVG whose
 * view box is in font units, like the glyph outlines. Gradients use font-unit
 * coordinates too.
 */
export function ColorGlyph({ layers, palette, opacity = 1, highlight }: {
  layers: ColorLayerView[];
  palette: string[];
  /** Opacity of the whole drawing. */
  opacity?: number;
  /** A layer drawn at full strength while the others are faint (the one being edited). */
  highlight?: string;
}) {
  const uid = useId().replace(/:/g, "");
  return (
    <g opacity={opacity} transform="scale(1,-1)">
      <defs>
        {layers.map((l, i) => <PaintDef key={i} id={`${uid}-${i}`} paint={l.paint} palette={palette} />)}
      </defs>
      {layers.map((l, i) => (
        <path
          key={l.key}
          d={l.svg}
          fill={l.paint.type === "solid" ? css(palette, l.paint.color).color : `url(#${uid}-${i})`}
          fillOpacity={l.paint.type === "solid" ? l.paint.alpha : 1}
          opacity={highlight && l.key !== highlight ? 0.3 : 1}
        />
      ))}
    </g>
  );
}

function PaintDef({ id, paint, palette }: { id: string; paint: Paint; palette: string[] }) {
  if (paint.type === "solid") return null;
  const stops = [...paint.stops].sort((a, b) => a.offset - b.offset).map((s, i) => {
    const c = css(palette, s.color, s.alpha);
    return <stop key={i} offset={s.offset} stopColor={c.color} stopOpacity={c.opacity} />;
  });
  // The drawing is flipped (scale(1,-1)), so y coordinates are negated here.
  if (paint.type === "linear") {
    return (
      <linearGradient id={id} gradientUnits="userSpaceOnUse" x1={paint.start[0]} y1={-paint.start[1]} x2={paint.end[0]} y2={-paint.end[1]}>
        {stops}
      </linearGradient>
    );
  }
  const f = paint.focus ?? paint.center;
  return (
    <radialGradient id={id} gradientUnits="userSpaceOnUse" cx={paint.center[0]} cy={-paint.center[1]} r={paint.radius} fx={f[0]} fy={-f[1]}>
      {stops}
    </radialGradient>
  );
}
