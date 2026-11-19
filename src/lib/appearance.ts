import type { Appearance, CanvasElement } from "./backend";
import type { ThemePref } from "./theme";

export const COLOR_FIELDS: { id: CanvasElement; label: string; token: string }[] = [
  { id: "cornerNode", label: "Corner node", token: "--k-editor-node-corner" },
  { id: "smoothNode", label: "Smooth node", token: "--k-editor-node-smooth" },
  { id: "zones", label: "Hints and zones", token: "--k-editor-hint-color" },
  { id: "stroke", label: "Foreground stroke", token: "--k-editor-outline" },
  { id: "background", label: "Background layer", token: "--k-editor-background-layer" },
  { id: "otherLayers", label: "Components", token: "--k-editor-other-layer" },
  { id: "canvas", label: "Canvas", token: "--k-editor-canvas" },
  { id: "kerningNegative", label: "Kerning negative", token: "--k-editor-kerning-negative" },
  { id: "kerningPositive", label: "Kerning positive", token: "--k-editor-kerning-positive" },
];

export function applyCanvasColors(appearance: Appearance, theme: ThemePref) {
  const media = window.matchMedia("(prefers-color-scheme: dark)");
  const apply = () => {
    const mode = theme === "system" ? (media.matches ? "dark" : "light") : theme;
    for (const field of COLOR_FIELDS) {
      const value = appearance.colors[mode][field.id];
      if (value) document.documentElement.style.setProperty(field.token, value);
      else document.documentElement.style.removeProperty(field.token);
    }
  };
  apply();
  media.addEventListener("change", apply);
  return () => media.removeEventListener("change", apply);
}

export function defaultCanvasColor(theme: "light" | "dark", element: CanvasElement) {
  return getComputedStyle(document.documentElement).getPropertyValue(`--k-default-${theme}-${element}`).trim();
}
