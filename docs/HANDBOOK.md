# Kernest Handbook

Kernest is a font editor for Windows. It works offline, and your fonts are plain files on your disk. This handbook covers everything the app can do today. Press **F1** in the app to open it.

- [Fonts and files](#fonts-and-files)
- [Font View](#font-view)
- [Edit View](#edit-view)
- [Spacing and kerning](#spacing-and-kerning)
- [Masters, axes and instances](#masters-axes-and-instances)
- [Export](#export)
- [Colour fonts](#colour-fonts)
- [Import](#import)
- [Theme and language](#theme-and-language)
- [Keyboard shortcuts](#keyboard-shortcuts)

## Fonts and files

A Kernest font is a **`.kernest` folder**. Inside it, `font.json` holds the font's info, masters, kerning and features, and the `glyphs` folder holds one readable JSON file per glyph. That makes fonts easy to keep in Git and to compare between versions.

- **New font** (Ctrl+N): choose a family name and the glyph sets to start with.
- **Open** (Ctrl+O): pick the `.kernest` folder. Fonts you've opened recently are listed on the start window.
- **Save** (Ctrl+S) and **Save As** (Ctrl+Shift+S): the title bar shows **Edited** while there are unsaved changes, and Kernest asks before closing a font with unsaved changes.
- **Open Sample Font**: a ready-made font to try things out with.

### Opening fonts from Windows

- **Right-click a `.kernest` folder › Open in Kernest.** A `.kernest` font is a folder, so double-clicking it opens it in Explorer, as with any folder. (On Windows 11, look under **Show more options** if it isn't on the short menu.)
- **Drag and drop** a `.kernest` folder onto the Kernest window or its taskbar button.
- **Font sources** open the same ways and are imported:
  - right-click a `.ufo` or `.glyphspackage` folder › **Open in Kernest**;
  - double-click a `.glyphs` or `.designspace` file, or use **Open with › Kernest**. Kernest only becomes their default app if no other app has claimed them.
- **One window at a time:** if Kernest is already running, the font opens in that window, which asks first if there are unsaved changes.

## Font View

The **Font** tab shows every glyph as a cell.

- **Find glyphs:** filter by category on the left, or search by name, Unicode value (`0041` or `U+0041`) or the character itself.
- **Select:** click a glyph. Ctrl+click and Shift+click select more than one.
- **Open:** double-click a glyph, or press Enter, to edit it in its own tab.
- **Add glyphs** (Glyph › Add Glyphs…, Ctrl+Shift+G, or the **Add Glyphs…** button):
  - type glyph names (`A.alt`, `a.sc`), characters (`Ж`, `€`) or code points (`U+20AC`, `uni0416`), separated by spaces, commas or new lines;
  - tick glyph sets to add whole ranges at once;
  - glyphs already in the font are skipped;
  - new glyphs get a layer in every master, and a suffixed glyph such as `a.sc` starts with the width of `a`.
- **Export or not:** with glyphs selected, untick **Export this glyph** in the panel on the left to leave them out of compiled fonts, for example parts used only as components. Font View then shows "not exported".
- **Remove glyphs** (Glyph › Remove Glyph, Ctrl+Delete): removes the selected glyphs, or the glyph whose tab is open.
  - Components that use a removed glyph are turned into outlines in the other glyphs, so nothing changes shape.
  - Kerning pairs that name a removed glyph are removed too.
  - You can undo it with Ctrl+Z.

## Edit View

Each glyph opens in its own tab. The toolbar has these tools, each with a one-letter shortcut:

| Tool | Key | What it does |
|---|---|---|
| Select | V | Click or drag to select nodes; drag to move them. |
| Pen | P | Click to add corner points, drag to add curve points. Click the first point to close the path. |
| Rectangle | F | Drag to draw a rectangle. |
| Ellipse | E | Drag to draw an ellipse. |
| Hand | H or hold Space | Drag to move around the canvas. |

- **Moving points:**
  - arrow keys nudge the selection by 1 unit, Shift+arrows by 10;
  - Enter switches the selected nodes between smooth and corner;
  - Delete removes them.
- **Selecting:** Ctrl+A selects everything, and double-clicking a node selects its whole path.
- **Zoom:** Ctrl+= and Ctrl+- zoom in and out, and Ctrl+0 fits the glyph to the window.
- **Components** (Ctrl+Shift+C): add another glyph as a component. Kernest won't let a glyph contain itself.
- **Clipboard:** Ctrl+X, Ctrl+C and Ctrl+V cut, copy and paste paths and components, including between glyph tabs.
- **Sidebearings and width:** edit the left sidebearing, advance width and right sidebearing in the panel at the bottom.
- **Path › Correct Path Direction** (Ctrl+Shift+R): makes outer contours run counter-clockwise and counters (holes) clockwise, which is what the compiler expects. It also orders contours left to right, with each counter right after its outline.
- **Path › Add Extremes:** adds points where curves reach their horizontal and vertical extremes, on cubic and quadratic (TrueType) curves. It works on the selected paths, or on all paths if none is selected.
- **Every master at once:** when a glyph's masters are compatible, both path commands work out the change on the master you're editing and make the same change in every master and intermediate layer, so the glyph stays compatible.
- **Path › Remove Overlap:** merges overlapping or touching contours of the layer you're editing into one outline (only that layer, so do it in every master to keep them compatible, or let export do it).
- **Metrics keys:** type a key instead of a number in the LSB, Width or RSB field to make it follow another glyph:
  - `=n` uses `n`'s value on the same side, and `=|H` uses the opposite side (H's right sidebearing for a left sidebearing);
  - add arithmetic such as `=o+10` or `=n*1.1`, or give a fixed value with `=40`;
  - the key shows next to the field name, and × removes it; typing a plain number replaces it;
  - **Glyph › Update Metrics** re-applies every key in the font, and export applies them too.
- **View › Show Metrics** (Ctrl+Shift+M): shows or hides the metric lines (ascender, cap height, x-height, baseline, descender) and the advance box.

Every edit can be undone with **Ctrl+Z** and redone with **Ctrl+Y**.

### Intermediate and alternate layers

Use the **Layer** menu in the Edit View toolbar to switch between the master's own layer and the glyph's extra layers.

- **+ Intermediate** adds a brace layer: an extra drawing at its own location on the axes (for example `{wght 550}`), used when a glyph needs a correction between two masters.
  - It starts as the glyph interpolated there, so nothing changes until you edit it.
  - It must stay compatible with the masters (same paths, points and components).
  - **Move…** changes its location; **Remove** deletes it.
- **+ Alternate** adds bracket layers: a second design used wherever the location is inside the axis ranges you give (for example `[wght 600–]` for a simpler `$` at heavy weights).
  - Every master gets a copy to edit, and the alternates must be compatible with each other.
  - A range includes both its minimum and its maximum; leave a box empty for no limit.
  - **Ranges…** changes the ranges for all masters; **Remove** removes the alternate everywhere.
- **In exported fonts:** static fonts get the right design at their location. Variable fonts get intermediate layers as extra sources and swap alternates in with OpenType feature variations.

### Smart components

A smart glyph is a component that stretches: each glyph that uses it sets its own values on the smart glyph's axes. For example, one `_stem` can be the stems of H, I and L at different heights.

1. Open the part glyph (for example `_stem`), click **Smart…** and add axes such as Height from 0 to 100. The master layer is the shape at every axis' minimum.
2. Click **+ Pole** to add the shape at another position, for example Height 100. It appears in the **Layer** menu as "Smart Pole Height 100"; edit it point for point to the other extreme.
3. Use the glyph as a component (**Add Component**). With the component selected, its axis values appear next to X and Y; type a value to reshape it.

Poles must match the master layer point for point. On export, smart components become ordinary outlines. Smart components in Glyphs files import as smart components.

## Spacing and kerning

- **Text tabs** (Window › New Text Tab, Ctrl+T): type sample text to see your glyphs together.
  - Click between two glyphs to place the cursor.
  - **Alt+Left** and **Alt+Right** change the kerning there by 10 units, or by 1 unit with Shift held too.
- **Kerning window** (Window › Kerning, Ctrl+Shift+K): every kerning pair for the current master, with a filter box and a row for adding pairs.
- **Kerning groups:** a glyph has a **left group** (used when it's on the right side of a pair) and a **right group** (used when it's on the left side). Group pairs are written as `@group`. A pair between two specific glyphs is an **exception**, and it overrides the group pair.

## Masters, axes and instances

Open **Font Info** (Ctrl+I) to set:

- **Family:** the family name, units per em and version.
- **Hinting:** the settings for autohinting on export (see [Export](#export)):
  - **Smallest size** and **Largest size:** the range of text sizes, in pixels per em, that gets hinting instructions (8 to 50 by default).
  - **Increase x-height up to:** at sizes up to this one, the x-height is rounded up so lowercase letters stay legible (14 by default; 0 turns it off).
  - **Hint composites as whole glyphs:** hints accented letters and other composites as one shape, instead of through their components.
- **Axes:** for example Weight (`wght`) from 100 to 900, with a default value.
  - Removing an axis takes masters and instances off it (their positions on that axis are dropped) and deletes intermediate and alternate layers that use it. Kernest asks first, and Ctrl+Z undoes it.
- **Masters:** each master is a full set of outlines at a location on the axes. Add a master by copying an existing one, then edit its outlines.
- **Instances:** named styles such as SemiBold at a location between masters. They become static fonts on export; untick **Export** on an instance to leave it out.

Switch the master you're editing with the buttons at the top right. Previews interpolate live. Kernest checks that every glyph can be interpolated, meaning the same number of paths, nodes and components in each master, and lists the glyphs that can't be in the **Incompatible** category of Font View.

## Export

**File › Export** (Ctrl+E) builds fonts with [fontc](https://github.com/googlefonts/fontc):

- **Variable font** (`Family-VF.ttf`): built from all the masters. It needs at least two masters, an axis, a master at the default location, and glyphs that can be interpolated.
- **Static fonts** (`Family-Style.ttf`): one per exported instance, or one per master if there are no instances. Instances between masters are interpolated by Kernest first. Each static font's weight class (used by apps to sort and match styles) comes from its position on the Weight axis, or from its style name (Thin, Light, Bold…) if there's no Weight axis; its width class comes from a Width axis.

Choose the formats:

- **TTF:** TrueType outlines, for desktop apps.
- **OTF:** PostScript outlines: CFF for static fonts and CFF2 for the variable font (`Family-VF.otf`), with intermediate layers and alternates just as in the TrueType variable font.
- **WOFF2:** compressed web fonts, for static and variable fonts.

And what happens before building:

- **Remove overlaps** merges overlapping contours in static fonts. Variable fonts keep them, because their masters must stay compatible.
- **Apply metrics keys** re-applies every metrics key, so sidebearings are up to date.
- **Autohint** adds TrueType hinting instructions to TTF and WOFF2 files, static and variable, using the settings in Font Info › Hinting. Hinting makes text crisper at small sizes on Windows. OTF files aren't affected. Kernest hints with [tilvisan](https://github.com/simoncozens/tilvisan), a Rust port of ttfautohint.

Kerning and kerning groups are compiled into GPOS, and your feature code is included. Glyphs that aren't set to export are left out.

## Colour fonts

Kernest makes colour fonts in the **COLRv1** format: glyphs built from coloured layers, each filled with a palette colour or a gradient. Windows 11, current browsers and design apps show them in colour. The glyph's normal outline is the black-and-white fallback for apps that don't support colour.

1. **Palettes:** in Font Info › Colour palettes, choose **Add palette**, then set its colours with the swatches or as `#rrggbb` / `#rrggbbaa` (the last two digits are the opacity). Extra palettes, for example one for dark backgrounds, have the same number of colours as palette 1, which is the default.
2. **Colour layers:** in the glyph editor, **+ Colour** adds a colour layer in every master. The first one starts as a copy of the glyph's outline; later ones start empty. Pick a colour layer in the **Layer** list and draw it like any other layer. Layers stack from Colour 1 at the bottom upwards; **↓** and **↑** change the order.
3. **Paint…** chooses how the layer is filled:
   - **Solid colour:** a palette colour, with an opacity.
   - **Linear gradient:** from a start point to an end point, in font units.
   - **Radial gradient:** outwards from a centre, up to a radius.
   - Gradients go through two or more colour stops, each with a palette colour, a position and an opacity.

The paint of a colour layer is the same in every master, but its drawing can differ: colour layers must match point for point across masters, like the outlines, and they interpolate in variable fonts.

On export, every colour layer becomes a glyph of its own (`O.color0`, `O.color1`, …), and Kernest writes the `COLR` and `CPAL` tables into TTF, OTF and WOFF2 files, static and variable. Glyphs whose layers are all solid, opaque colours also get a COLRv0 record, so Windows 10 and older apps can show them in colour too.

## Import

**File › Import…** (Ctrl+Shift+O) opens a font from another editor as a new, unsaved Kernest font. Kernest never changes the original file.

- **Glyphs:** `.glyphs` (Glyphs 2 and 3) and `.glyphspackage`.
- **UFO:** a single `.ufo` (one master), or a `.designspace` with its UFO sources.

`.glyphspackage` and `.ufo` sources are folders. Use **File › Import Folder…** to pick the folder, or pick any file inside it with Import….

Kernest brings over the axes, masters, outlines, components, anchors, kerning, instances and features, plus brace and bracket layers and, from Glyphs files, metrics keys (glyph-wide and per layer) and the hinting settings in a **TTFAutohint options** custom parameter. From designspaces, sparse sources become intermediate layers and rules become alternates. Right-to-left kerning isn't supported yet. If anything is left out, a dialog lists it.

## Theme and language

- **Theme:** View › Theme follows Windows (System), or is always Light or Dark.
- **Language:** View › Language switches the interface between English, Español and العربية (Arabic) straight away, with no restart. The first time, Kernest uses Windows' language if it has it.
  - In Arabic the interface reads right to left. The glyph canvas, text previews, coordinates and glyph names stay left to right.
  - Messages from the font engine, such as export problems, are translated too.
  - The Handbook is in English.
- The installer can also be run in English, Spanish or Arabic.

## Keyboard shortcuts

| Action | Shortcut |
|---|---|
| New / Open / Import | Ctrl+N / Ctrl+O / Ctrl+Shift+O |
| Save / Save As | Ctrl+S / Ctrl+Shift+S |
| Close tab (or font) | Ctrl+W |
| Undo / Redo | Ctrl+Z / Ctrl+Y (or Ctrl+Shift+Z) |
| Add Glyphs / Remove Glyph | Ctrl+Shift+G / Ctrl+Delete |
| Add Component | Ctrl+Shift+C |
| Correct Path Direction | Ctrl+Shift+R |
| Show Metrics | Ctrl+Shift+M |
| New Text Tab / Kerning | Ctrl+T / Ctrl+Shift+K |
| Font Info / Export | Ctrl+I / Ctrl+E |
| Handbook | F1 |
