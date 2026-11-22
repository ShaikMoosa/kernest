// Customisable keyboard shortcuts. The registry lists the actions; the app
// supplies a handler per id and the Settings window lets the user rebind them.
// A shortcut stored in app settings overrides the default here.

export interface ShortcutAction {
  id: string;
  /** English label, passed through t() in the UI. */
  label: string;
  default: string;
  /**
   * "global" always; "font" needs an open font; "glyph" needs an open glyph tab;
   * "text" is handled by the text tab itself (see `findTextShortcut`).
   */
  scope: "global" | "font" | "glyph" | "text";
  /** Fire while typing in a text field (default true). */
  allowInField?: boolean;
  /** Needs a glyph selection or open glyph tab. */
  needsTargets?: boolean;
}

export const SHORTCUT_ACTIONS: ShortcutAction[] = [
  { id: "new-font", label: "New Font", default: "Ctrl+N", scope: "global" },
  { id: "open", label: "Open", default: "Ctrl+O", scope: "global" },
  { id: "import", label: "Import", default: "Ctrl+Shift+O", scope: "global" },
  { id: "save", label: "Save", default: "Ctrl+S", scope: "font" },
  { id: "save-as", label: "Save As", default: "Ctrl+Shift+S", scope: "font" },
  { id: "close", label: "Close Tab or Font", default: "Ctrl+W", scope: "global" },
  { id: "export", label: "Export", default: "Ctrl+E", scope: "font" },
  { id: "font-info", label: "Font Info", default: "Ctrl+I", scope: "font" },
  { id: "undo", label: "Undo", default: "Ctrl+Z", scope: "font", allowInField: false },
  { id: "redo", label: "Redo", default: "Ctrl+Y", scope: "font", allowInField: false },
  { id: "new-text-tab", label: "New Text Tab", default: "Ctrl+T", scope: "font" },
  { id: "kerning", label: "Kerning", default: "Ctrl+Shift+K", scope: "font" },
  { id: "add-glyphs", label: "Add Glyphs", default: "Ctrl+Shift+G", scope: "font" },
  { id: "remove-glyph", label: "Remove Glyph", default: "Ctrl+Del", scope: "font", allowInField: false, needsTargets: true },
  { id: "set-anchors", label: "Set Anchors", default: "Ctrl+U", scope: "font", allowInField: false, needsTargets: true },
  { id: "add-component", label: "Add Component", default: "Ctrl+Shift+C", scope: "glyph" },
  { id: "edit-background", label: "Edit Background", default: "Ctrl+B", scope: "glyph" },
  { id: "selection-to-background", label: "Selection to Background", default: "Ctrl+J", scope: "glyph" },
  { id: "add-to-background", label: "Add to Background", default: "Ctrl+Shift+J", scope: "glyph" },
  { id: "correct-direction", label: "Correct Path Direction", default: "Ctrl+Shift+R", scope: "glyph" },
  { id: "tidy-up", label: "Tidy up Paths", default: "Ctrl+Shift+T", scope: "glyph" },
  { id: "harmonize", label: "Harmonize", default: "Ctrl+Shift+H", scope: "glyph" },
  { id: "align-selection", label: "Align Selection", default: "Ctrl+Alt+Shift+A", scope: "glyph" },
  { id: "repeat-filter", label: "Repeat Last Filter", default: "Ctrl+Shift+F", scope: "font" },
  { id: "show-metrics", label: "Show Metrics", default: "Ctrl+Shift+M", scope: "global" },
  { id: "show-anchors", label: "Show Anchors", default: "Ctrl+Shift+A", scope: "global" },
  { id: "show-guides", label: "Show Guides", default: "Ctrl+Shift+L", scope: "global" },
  { id: "show-background", label: "Show Background", default: "Ctrl+Shift+B", scope: "global" },
  { id: "show-bounding-box", label: "Show Bounding Box", default: "Ctrl+Alt+Shift+B", scope: "global" },
  { id: "macro-panel", label: "Macro Panel", default: "Ctrl+Shift+P", scope: "global" },
  { id: "show-preview", label: "Show Preview", default: "Ctrl+Alt+P", scope: "global" },
  { id: "preview-window", label: "Preview Window", default: "Ctrl+Alt+Shift+P", scope: "font" },
  { id: "settings", label: "Settings", default: "Ctrl+,", scope: "global" },
  // Text tabs (SMO-35). Holding Shift as well takes steps of 10 units.
  { id: "text-kern-decrease", label: "Decrease Kerning", default: "Alt+Left", scope: "text", allowInField: false },
  { id: "text-kern-increase", label: "Increase Kerning", default: "Alt+Right", scope: "text", allowInField: false },
  { id: "text-lsb-increase", label: "Increase Left Sidebearing", default: "Ctrl+Alt+Left", scope: "text", allowInField: false },
  { id: "text-lsb-decrease", label: "Decrease Left Sidebearing", default: "Ctrl+Alt+Right", scope: "text", allowInField: false },
  { id: "text-rsb-increase", label: "Increase Right Sidebearing", default: "Ctrl+Right", scope: "text", allowInField: false },
  { id: "text-rsb-decrease", label: "Decrease Right Sidebearing", default: "Ctrl+Left", scope: "text", allowInField: false },
  { id: "text-shift-right", label: "Move Outline Right", default: "Ctrl+Alt+Up", scope: "text", allowInField: false },
  { id: "text-shift-left", label: "Move Outline Left", default: "Ctrl+Alt+Down", scope: "text", allowInField: false },
  { id: "text-kern-exception", label: "Make or Remove Kerning Exception", default: "Ctrl+Alt+E", scope: "text", allowInField: false },
  { id: "text-ltr", label: "Left to Right Text", default: "Ctrl+Alt+Shift+L", scope: "text" },
  { id: "text-rtl", label: "Right to Left Text", default: "Ctrl+Alt+Shift+R", scope: "text" },
  { id: "text-vertical", label: "Vertical Text", default: "Ctrl+Alt+Shift+V", scope: "text" },
];

export interface ParsedShortcut {
  ctrl: boolean;
  shift: boolean;
  alt: boolean;
  key: string;
}

const KEY_ALIASES: Record<string, string> = {
  del: "delete",
  esc: "escape",
  return: "enter",
  space: " ",
  comma: ",",
  period: ".",
  plus: "+",
  minus: "-",
  left: "arrowleft",
  right: "arrowright",
  up: "arrowup",
  down: "arrowdown",
};

/** Parses "Ctrl+Shift+T" into modifiers and a key; null if it has no key. */
export function parseShortcut(text: string): ParsedShortcut | null {
  const parts = text.split("+").map((p) => p.trim()).filter(Boolean);
  if (parts.length === 0) return null;
  let ctrl = false, shift = false, alt = false;
  let key = "";
  for (const p of parts) {
    const low = p.toLowerCase();
    if (low === "ctrl" || low === "control") ctrl = true;
    else if (low === "shift") shift = true;
    else if (low === "alt") alt = true;
    else key = KEY_ALIASES[low] ?? low;
  }
  if (!key) return null;
  return { ctrl, shift, alt, key };
}

/** True when a key event matches a parsed shortcut (Cmd is never used on Windows). */
export function matchShortcut(e: KeyboardEvent, s: ParsedShortcut): boolean {
  return e.ctrlKey === s.ctrl && e.shiftKey === s.shift && e.altKey === s.alt && !e.metaKey && e.key.toLowerCase() === s.key;
}

/** The key parts of a key event that shortcut matching reads. */
export type KeyLike = Pick<KeyboardEvent, "ctrlKey" | "altKey" | "shiftKey" | "metaKey" | "key">;

/**
 * The text-tab action a key press triggers, and its step: 1 unit, or 10 with
 * Shift held as well. A binding that itself uses Shift always steps by 1.
 */
export function findTextShortcut(e: KeyLike, overrides: Record<string, string>): { id: string; step: number } | null {
  for (const a of SHORTCUT_ACTIONS) {
    if (a.scope !== "text") continue;
    const s = parseShortcut(effectiveShortcut(a, overrides));
    if (!s || e.ctrlKey !== s.ctrl || e.altKey !== s.alt || e.metaKey || e.key.toLowerCase() !== s.key) continue;
    if (s.shift) {
      if (e.shiftKey) return { id: a.id, step: 1 };
      continue;
    }
    return { id: a.id, step: e.shiftKey ? 10 : 1 };
  }
  return null;
}

/** The shortcut text for an action, honouring a stored override. */
export function effectiveShortcut(action: ShortcutAction, overrides: Record<string, string>): string {
  return overrides[action.id] ?? action.default;
}

/** Renders a key event as shortcut text ("Ctrl+Shift+T"), for the rebinding UI. */
export function shortcutFromEvent(e: KeyboardEvent): string | null {
  const key = e.key;
  if (["Control", "Shift", "Alt", "Meta"].includes(key)) return null;
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  const named: Record<string, string> = {
    " ": "Space", ",": "Comma", ".": "Period", "+": "Plus", "-": "Minus",
    Delete: "Del", Escape: "Esc", Enter: "Enter", ArrowUp: "Up", ArrowDown: "Down",
    ArrowLeft: "Left", ArrowRight: "Right",
  };
  const k = named[key] ?? (key.length === 1 ? key.toUpperCase() : key);
  parts.push(k);
  // A bare letter without Ctrl is not a useful app shortcut; require a modifier.
  if (!e.ctrlKey && !e.altKey) return null;
  return parts.join("+");
}