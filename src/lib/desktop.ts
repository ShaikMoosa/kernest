// Desktop integration: window controls, file dialogs and the recent-fonts list.

import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { isTauri } from "./backend";
import { language, t } from "./i18n";

export const windowControls = {
  minimize: () => (isTauri ? getCurrentWindow().minimize() : Promise.resolve()),
  toggleMaximize: () => (isTauri ? getCurrentWindow().toggleMaximize() : Promise.resolve()),
  close: () => (isTauri ? getCurrentWindow().close() : Promise.resolve()),
};

/** What a path dropped on the window or passed by Explorer is. */
export function classifyPath(path: string): "kernest" | "source" | "compiled" | null {
  const parts = path.toLowerCase().split(/[\\/]/).filter(Boolean);
  if (parts.some((p) => p.endsWith(".kernest"))) return "kernest";
  const last = parts[parts.length - 1] ?? "";
  if (last.endsWith(".glyphs") || last.endsWith(".designspace")) return "source";
  if (parts.some((p) => p.endsWith(".ufo") || p.endsWith(".glyphspackage"))) return "source";
  if (last.endsWith(".otf") || last.endsWith(".ttf") || last.endsWith(".woff2")) return "compiled";
  return null;
}

/** The .kernest folder a path is in (or is), e.g. for a file dropped from inside it. */
export function kernestRoot(path: string): string {
  const m = /^(.*?\.kernest)(?:[\\/]|$)/i.exec(path);
  return m ? m[1] : path;
}

export const REPO_URL = "https://github.com/ShaikMoosa/kernest";
export const HANDBOOK_URL = `${REPO_URL}/blob/main/docs/HANDBOOK.md`;

/** Opens a web page in the default browser (not inside the app window). */
export function openExternal(url: string): Promise<void> {
  if (isTauri) return openUrl(url);
  window.open(url, "_blank", "noopener");
  return Promise.resolve();
}

export async function pickQualityReportPath(familyName: string): Promise<string | null> {
  if (!isTauri) return null;
  return save({ title: t("Save font check report"), defaultPath: `${familyName.replace(/[<>:"/\\|?*\x00-\x1f]/g, "_")}-checks.json`, filters: [{ name: "JSON", extensions: ["json"] }] });
}

/** A `.kernest` font is a folder, so Open picks a directory. */
export async function pickFontToOpen(): Promise<string | null> {
  if (!isTauri) return null;
  const picked = await open({ directory: true, multiple: false, title: t("Open a .kernest font") });
  return typeof picked === "string" ? picked : null;
}

/**
 * Import picks a file: a .glyphs or .designspace file, a compiled .otf/.ttf/.woff2
 * font, or any file inside a .glyphspackage or .ufo folder (such as fontinfo.plist).
 * With `folder`, it picks the .glyphspackage or .ufo folder itself.
 */
export async function pickSourceToImport(folder = false): Promise<string | null> {
  if (!isTauri) return null;
  const picked = folder
    ? await open({ directory: true, multiple: false, title: t("Import a .glyphspackage or .ufo folder") })
    : await open({
        multiple: false,
        title: t("Import a font"),
        filters: [
          { name: t("Font sources"), extensions: ["glyphs", "designspace", "plist"] },
          { name: t("Compiled fonts"), extensions: ["otf", "ttf", "woff2"] },
          { name: t("All files"), extensions: ["*"] },
        ],
      });
  return typeof picked === "string" ? picked : null;
}

/** Picks one SVG file per glyph to import (File › Import SVG…). */
export async function pickSvgFiles(): Promise<string[]> {
  if (!isTauri) return [];
  const picked = await open({
    multiple: true,
    title: t("Import SVG outlines"),
    filters: [
      { name: t("SVG drawing"), extensions: ["svg"] },
      { name: t("All files"), extensions: ["*"] },
    ],
  });
  return Array.isArray(picked) ? picked : typeof picked === "string" ? [picked] : [];
}

/** Picks an AFM file or another font to import metrics from (File › Import Metrics…). */
export async function pickMetricsSource(): Promise<string | null> {
  if (!isTauri) return null;
  const picked = await open({
    multiple: false,
    title: t("Import metrics from"),
    filters: [
      { name: t("AFM metrics"), extensions: ["afm"] },
      { name: t("Kernest font"), extensions: ["kernest"] },
      { name: t("Font sources"), extensions: ["glyphs", "designspace", "plist"] },
      { name: t("Compiled fonts"), extensions: ["otf", "ttf", "woff2"] },
      { name: t("All files"), extensions: ["*"] },
    ],
  });
  return typeof picked === "string" ? picked : null;
}

/** Picks another font to copy masters from (Font Info › Masters › Add from font…). */
/** Glyph › Add Image…: a PNG, JPEG or GIF to place on the layer. */
export async function pickImage(): Promise<string | null> {
  if (!isTauri) return null;
  const picked = await open({
    multiple: false,
    title: t("Add Image"),
    filters: [{ name: t("Images"), extensions: ["png", "jpg", "jpeg", "gif"] }],
  });
  return typeof picked === "string" ? picked : null;
}

export async function pickMasterSource(): Promise<string | null> {
  if (!isTauri) return null;
  const picked = await open({
    multiple: false,
    title: t("Add masters from"),
    filters: [
      { name: t("Kernest font"), extensions: ["kernest"] },
      { name: t("Font sources"), extensions: ["glyphs", "designspace", "plist"] },
      { name: t("Compiled fonts"), extensions: ["otf", "ttf", "woff2"] },
      { name: t("All files"), extensions: ["*"] },
    ],
  });
  return typeof picked === "string" ? picked : null;
}

/** Picks another font to compare against the open one (File › Compare with Font…). */
export async function pickCompareSource(): Promise<string | null> {
  if (!isTauri) return null;
  const picked = await open({
    multiple: false,
    title: t("Compare with font"),
    filters: [
      { name: t("Kernest font"), extensions: ["kernest"] },
      { name: t("Font sources"), extensions: ["glyphs", "designspace", "plist"] },
      { name: t("Compiled fonts"), extensions: ["otf", "ttf", "woff2"] },
      { name: t("All files"), extensions: ["*"] },
    ],
  });
  return typeof picked === "string" ? picked : null;
}

/** Picks a .fea file to merge into the feature code (File › Import Features…). */
export async function pickFeaFile(): Promise<string | null> {
  if (!isTauri) return null;
  const picked = await open({
    multiple: false,
    title: t("Import features from"),
    filters: [
      { name: t("OpenType features"), extensions: ["fea"] },
      { name: t("All files"), extensions: ["*"] },
    ],
  });
  return typeof picked === "string" ? picked : null;
}

export async function pickSavePath(familyName: string): Promise<string | null> {
  if (!isTauri) return null;
  return save({
    title: t("Save font as"),
    defaultPath: `${familyName}.kernest`,
    filters: [{ name: t("Kernest font"), extensions: ["kernest"] }],
  });
}

/** Picks where to write a Glyphs 3 `.glyphs` copy of the open font. */
export async function pickGlyphsSavePath(familyName: string): Promise<string | null> {
  if (!isTauri) return null;
  return save({
    title: t("Save as .glyphs"),
    defaultPath: `${familyName}.glyphs`,
    filters: [{ name: t("Glyphs font"), extensions: ["glyphs"] }],
  });
}

/** Picks a Python script to run once (Script › Run Script File…). */
export async function pickScript(): Promise<string | null> {
  if (!isTauri) return null;
  const picked = await open({
    multiple: false,
    title: t("Run a Python script"),
    filters: [
      { name: t("Python script"), extensions: ["py"] },
      { name: t("All files"), extensions: ["*"] },
    ],
  });
  return typeof picked === "string" ? picked : null;
}

export interface RecentFont {
  path: string;
  familyName: string;
  openedAt: number;
}

const RECENTS_KEY = "kernest.recentFonts";
const MAX_RECENTS = 12;

export function loadRecents(): RecentFont[] {
  try {
    const raw = localStorage.getItem(RECENTS_KEY);
    const list = raw ? (JSON.parse(raw) as RecentFont[]) : [];
    return Array.isArray(list) ? list.filter((r) => typeof r.path === "string") : [];
  } catch {
    return [];
  }
}

export function rememberRecent(path: string, familyName: string): RecentFont[] {
  const next = [{ path, familyName, openedAt: Date.now() }, ...loadRecents().filter((r) => r.path !== path)].slice(0, MAX_RECENTS);
  try {
    localStorage.setItem(RECENTS_KEY, JSON.stringify(next));
  } catch {
    // Storage can be unavailable; the list is a convenience only.
  }
  return next;
}

export function forgetRecent(path: string): RecentFont[] {
  const next = loadRecents().filter((r) => r.path !== path);
  try {
    localStorage.setItem(RECENTS_KEY, JSON.stringify(next));
  } catch {
    // ignore
  }
  return next;
}

export function relativeDate(ts: number): string {
  const d = new Date(ts);
  const now = new Date();
  const days = Math.floor((now.setHours(0, 0, 0, 0) - new Date(ts).setHours(0, 0, 0, 0)) / 86_400_000);
  // Dates in the UI language, with Latin digits (as in the rest of Kernest).
  const locale = `${language()}-u-nu-latn`;
  const time = d.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" });
  if (days <= 0) return t("Today, {time}", { time });
  if (days === 1) return t("Yesterday");
  return d.toLocaleDateString(locale, { month: "short", day: "numeric" });
}
