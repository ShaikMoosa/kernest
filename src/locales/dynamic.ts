/**
 * UI strings that reach t() through a variable (data shown in the UI), so the
 * translation check knows about them.
 */
export const DYNAMIC_KEYS = [
  // Glyph sets (from the font engine)
  "Latin Basic (ASCII)",
  "Latin-1 Supplement",
  "General Punctuation",
  "Cyrillic Basic",
  // Glyph categories (Font View)
  "All",
  "Incompatible",
  "Uppercase",
  "Lowercase",
  "Letter",
  "Number",
  "Punctuation",
  "Symbol",
  "Separator",
  "Mark",
  "Other",
  // Exported file kinds
  "variable",
  "static",
  // Edit View tools
  "Select (V)",
  "Pen (P)",
  "Rectangle (F)",
  "Ellipse (E)",
  "Measure (L)",
  "Hand (H) · or hold Space",
  // Transform panel origin picker
  "Top left",
  "Top",
  "Top right",
  "Left",
  "Center",
  "Right",
  "Bottom left",
  "Bottom",
  "Bottom right",
  // Text View kerning modes
  "Group",
  "Kern the two glyphs' kerning groups (or the glyphs, if they have none)",
  "Left exception",
  "Kern this left glyph against the right group",
  "Right exception",
  "Kern the left group against this right glyph",
  "Exception",
  "Kern exactly these two glyphs",
  // Font Info › Hinting
  "Smallest size (px)",
  "Hinting starts at this many pixels per em",
  "Largest size (px)",
  "Above this size the font isn't hinted",
  "Increase x-height up to (px)",
  "Rounds the x-height up at small sizes for legibility; 0 is off",
  // Axis presets
  "Weight",
  "Optical size",
  "Slant",
  // Master metrics
  "Italic angle",
];
