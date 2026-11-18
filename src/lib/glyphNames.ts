export type NameFormat = "space" | "comma" | "lines" | "slash" | "python" | "unicode";

export const NAME_FORMATS: { id: NameFormat; label: string }[] = [
  { id: "space", label: "Space-separated" },
  { id: "comma", label: "Comma-separated" },
  { id: "lines", label: "One per line" },
  { id: "slash", label: "Slashed (/a/b)" },
  { id: "python", label: "Python list" },
  { id: "unicode", label: "Unicode list" },
];

/** Glyph names as text in the chosen layout; unicode is the hex codepoint string ("0041"). */
export function formatGlyphNames(glyphs: { name: string; unicode: string | null }[], format: NameFormat): string {
  const names = glyphs.map((g) => g.name);
  switch (format) {
    case "space":
      return names.join(" ");
    case "comma":
      return names.join(", ");
    case "lines":
      return names.join("\n");
    case "slash":
      return names.map((n) => `/${n}`).join("");
    case "python":
      return `[${names.map((n) => `'${n}'`).join(", ")}]`;
    case "unicode":
      return glyphs.map((g) => g.unicode).filter((u): u is string => !!u).map((u) => `U+${u}`).join(" ");
  }
}