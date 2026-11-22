// Events between the main window and the separate preview window (SMO-36).

/** What the preview window draws: the main window's current tab. */
export interface PreviewSource {
  text: string;
  masterId: string;
  /** Bumped by the main window after each change to the font. */
  version: number;
  /** The tab's name, for the window title. */
  title: string;
}

export const PREVIEW_EVENTS = {
  /** Main → preview: the current source, or null when there is nothing to show. */
  source: "kernest://preview-source",
  /** Preview → main: the window is listening and wants the current source. */
  ready: "kernest://preview-ready",
  /** Preview → main: a glyph was double-clicked. */
  openGlyph: "kernest://preview-open-glyph",
} as const;

/** True when this page is the preview window rather than the main window. */
export function isPreviewWindow(search: string): boolean {
  return new URLSearchParams(search).get("window") === "preview";
}
