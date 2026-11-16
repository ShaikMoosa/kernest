/** The colour theme: follow Windows (System), or always Light or Dark. */
export type ThemePref = "system" | "light" | "dark";

const KEY = "kernest.theme";

export function loadTheme(): ThemePref {
  try {
    const v = localStorage.getItem(KEY);
    return v === "light" || v === "dark" ? v : "system";
  } catch {
    return "system";
  }
}

/** Applies a theme to the page: `data-theme` is set only for an explicit choice, so
 * "system" follows Windows through `prefers-color-scheme`. */
export function applyTheme(pref: ThemePref) {
  const root = document.documentElement;
  if (pref === "system") delete root.dataset.theme;
  else root.dataset.theme = pref;
}

export function saveTheme(pref: ThemePref) {
  applyTheme(pref);
  try {
    if (pref === "system") localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, pref);
  } catch {
    // The theme still applies for this session.
  }
}
