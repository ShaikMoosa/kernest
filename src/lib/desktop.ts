// Desktop integration: window controls, file dialogs and the recent-fonts list.

import { getCurrentWindow } from "@tauri-apps/api/window";
import { open, save } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { isTauri } from "./backend";

export const windowControls = {
  minimize: () => (isTauri ? getCurrentWindow().minimize() : Promise.resolve()),
  toggleMaximize: () => (isTauri ? getCurrentWindow().toggleMaximize() : Promise.resolve()),
  close: () => (isTauri ? getCurrentWindow().close() : Promise.resolve()),
};

export const REPO_URL = "https://github.com/ShaikMoosa/kernest";
export const HANDBOOK_URL = `${REPO_URL}/blob/main/docs/HANDBOOK.md`;

/** Opens a web page in the default browser (not inside the app window). */
export function openExternal(url: string): Promise<void> {
  if (isTauri) return openUrl(url);
  window.open(url, "_blank", "noopener");
  return Promise.resolve();
}

/** A `.kernest` font is a folder, so Open picks a directory. */
export async function pickFontToOpen(): Promise<string | null> {
  if (!isTauri) return null;
  const picked = await open({ directory: true, multiple: false, title: "Open a .kernest font" });
  return typeof picked === "string" ? picked : null;
}

/**
 * Import picks a file: a .glyphs or .designspace file, or any file inside a
 * .glyphspackage or .ufo folder (such as fontinfo.plist). With `folder`, it picks the
 * .glyphspackage or .ufo folder itself.
 */
export async function pickSourceToImport(folder = false): Promise<string | null> {
  if (!isTauri) return null;
  const picked = folder
    ? await open({ directory: true, multiple: false, title: "Import a .glyphspackage or .ufo folder" })
    : await open({
        multiple: false,
        title: "Import a font source",
        filters: [
          { name: "Font sources", extensions: ["glyphs", "designspace", "plist"] },
          { name: "All files", extensions: ["*"] },
        ],
      });
  return typeof picked === "string" ? picked : null;
}

export async function pickSavePath(familyName: string): Promise<string | null> {
  if (!isTauri) return null;
  return save({
    title: "Save font as",
    defaultPath: `${familyName}.kernest`,
    filters: [{ name: "Kernest font", extensions: ["kernest"] }],
  });
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
  const time = d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  if (days <= 0) return `Today, ${time}`;
  if (days === 1) return "Yesterday";
  return d.toLocaleDateString([], { month: "short", day: "numeric" });
}
