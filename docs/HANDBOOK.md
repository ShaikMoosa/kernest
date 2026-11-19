# Kernest Handbook

Kernest is a font editor for Windows. It works offline, and your fonts are plain files on your disk. This handbook covers everything the app can do today. Press **F1** in the app to open it.

- [Fonts and files](#fonts-and-files)
- [Font View](#font-view)
- [Edit View](#edit-view)
- [Guides and background](#guides-and-background)
- [Spacing and kerning](#spacing-and-kerning)
- [Masters, axes and instances](#masters-axes-and-instances)
- [Export](#export)
- [Colour fonts](#colour-fonts)
- [Import](#import)
- [Scripting and plug-ins](#scripting-and-plug-ins)
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
  - Compiled `.otf`, `.ttf` and `.woff2` fonts dropped on the window are imported too.
- **One window at a time:** if Kernest is already running, the font opens in that window, which asks first if there are unsaved changes.

## Font View

The **Font** tab shows every glyph as a cell. Switch between **Grid** and **List** at the top right; in Grid, the slider next to it sets the cell size. Both choices are remembered.

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

### List view

**List** shows the glyphs as a table: name, Unicode, category, subcategory, LSB, RSB, width, left and right group, export and tags. Click a column heading to sort by it (click again for descending, a third time to return to the font's own order). Double-click a cell to edit it in place — width and sidebearings, groups, tags, export, and the name (which renames the glyph everywhere, see below); Enter or clicking away keeps the change, Escape cancels. Each edit is one undo step. With one glyph selected, the box under the sidebar shows its category, subcategory and script, and lets you set its tags and kerning groups.

### The sidebar: categories, languages, filters

- **Categories** groups glyphs by what they are (Uppercase, Lowercase, Number, Punctuation, Symbol, Separator…).
- **Languages** shows, per language, how many of its characters the font already has (`95 / 574`). Click a language to see the missing characters and **Add Missing** to add them all in one undo step.
- **Filters** holds saved smart filters and lists (below). Click one to show only its glyphs; click again to show all.

### Smart filters

**New Filter…** builds a filter from rules: category, subcategory, case, script, name contains / starts / ends, tag contains, and yes/no rules such as has components, has anchors, has Unicode, masters compatible, exported, metrics keys out of sync, colour glyph, smart glyph. Combine rules with **All** (every rule must match) or **Any** (one is enough), and nest groups to any depth. The dialog shows a live count of matching glyphs as you edit. **Save Selection as List** saves the current selection as a plain list of names instead. Saving a filter switches Font View to it.

### Tags, groups and production names

- **Tags** are free labels you put on glyphs (from the list or the info box) to find them again with a "tag contains" filter.
- **Left / right group** set the kerning groups a glyph belongs to.
- **Production name** (info box) is the name the glyph gets in compiled fonts when you tick **Use production names** in the Export dialog; the working name stays as it is. UFO sources always carry the mapping.

### Rename and batch commands

- **Rename Glyphs…** (Glyph menu, the Font View button, or double-clicking a name in List) renames many glyphs at once: find and replace (plain text or a regular expression), add or remove a suffix, over the selection or the whole font. A preview shows old → new before you commit. Components, kerning, metrics keys, lists and feature code that name a renamed glyph are updated too; if feature code changes, Kernest shows you the rewritten lines. One undo step.
- **Batch** (Font View button, needs a selection) applies a command to every selected glyph at once: Correct Path Direction, Remove Overlap, Tidy up Paths, Round Coordinates, Decompose Components, Update Metrics, Set Anchors, Create Composites. One undo step for the whole batch.

### Copy and paste glyphs

With glyphs selected, **Edit › Copy Glyphs** copies them with all their layers; **Edit › Copy Glyph Names** copies just the names in six formats (space-separated, comma-separated, one per line, slashed `/a/b`, a Python list, or a `U+0041` list). **Edit › Paste Special…** pastes copied glyphs three ways: over the glyphs with the same names, into the selected glyphs in order (keeping their names and Unicodes), or as components inside the selected glyphs (adding any missing base glyphs first). Locked glyphs refuse a paste. Each paste is one undo step.

### Glyph order

The order glyphs appear in Font View is the order they export in. Drag a cell (or a list row) onto another to move it before that glyph — one undo step. Dragging is paused while a column sort is active, since the sort rearranges the rows. You can also edit the order as text in **Font Info › Glyph order**: one name per line, every glyph exactly once; `.notdef` always stays first. Apply or discard the draft; applying is one undo step.

## Edit View

Each glyph opens in its own tab. The toolbar has these tools, each with a one-letter shortcut:

| Tool | Key | What it does |
|---|---|---|
| Select | V | Click or drag to select nodes; drag to move them. |
| Pen | P | Click to add corner points, drag to add curve points. Click the first point to close the path. |
| Rectangle | F | Drag to draw a rectangle. |
| Ellipse | Shift+E | Drag to draw an ellipse. |
| Knife | E | Drag a line across the outline to cut paths where it crosses. |
| Measure | L | Drag a line across the outline to measure distances and angles. |
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

### Path tools and filters

The **Path** menu and the node right-click menu reshape existing outlines. Each change is one undoable step.

- **Knife (E):** drag a line across the outline. Every path the line *crosses* is cut at the crossings and split into separate closed paths; a line that stays inside a shape does nothing. Press Esc to cancel mid-drag.
- **Open Corner:** splits the selected corner node into two, opening the path there.
- **Reconnect Nodes / Connect Nodes:** joins the open ends back together (Reconnect merges a corner you opened; Connect draws a segment between two selected end points).
- **Close Open Paths:** closes every open path in the layer.
- **Make Node First:** rotates a closed path so the selected node becomes its first point (useful for matching point order between masters).
- **Reverse Selected Contours:** flips the direction of the selected paths.
- **Tidy up Paths (Ctrl+Shift+T):** removes redundant points, such as a node that sits exactly on a straight line between its neighbours.
- **Round Coordinates:** rounds every point to whole font units.
- **Correct Path Direction (Ctrl+Shift+R)** and **Add Extremes** are described under Edit View above; they work across all compatible masters.

The **Filter** menu opens a dialog with a live preview drawn over the glyph before you commit:

- **Round Corners…:** rounds every corner by a radius you set.
- **Offset Curve…:** grows or shrinks the outline by a horizontal and vertical offset (make a stroke, or thin a shape).
- **Transformations…:** moves, scales, rotates or skews by exact amounts.
- Each dialog has an **All masters** switch and, when several glyphs are selected in the Font View, a **Selected glyphs (n)** switch so one filter run edits them all as a single undo step.
- **Repeat Last Filter (Ctrl+Shift+F):** re-applies the last filter and its settings to the current target.
- Locked glyphs are skipped: the dialog shows a notice and disables Apply until you unlock them.

### Locks and focus

- **Lock a path:** right-click an edge › **Lock Path** (or **Unlock Path**). A locked path can't be selected, moved or edited; the knife and filters leave it alone.
- **Lock a glyph:** **Glyph › Locked** toggles a lock on the whole glyph. A locked glyph shows a badge, refuses edits with a short notice, and is marked in the Font View. Unlock it the same way.
- **Focus on Selected Path:** dims every other path so you can work on one contour without distraction. **Clear Focus** (or Esc) restores them. Focus is a view aid only — it changes nothing in the saved font.
- Locks are saved in the `.kernest` file. A font that uses any lock saves at format version 6; older files without locks keep opening and saving at their own version.

### Hints and pixel preview

- **Hints panel:** the collapsible **Hints** panel in the editor sidebar lists the glyph's hints. With no manual hints, Kernest derives them automatically from the master's stems, draws them faintly on the canvas and shows their count in the panel. **+ Horizontal**, **+ Vertical** and **+ Ghost** add hints; each has a direction, position and width. A width of 0 makes a *ghost hint* that marks a single edge — the button beside it picks which edge (top/bottom for horizontal hints, right/left for vertical ones). Manual hints replace the automatic ones for that glyph, and hint bands are drawn across the advance on the canvas. Every change is one undoable step.
- **Pixel preview:** the collapsible **Pixel preview** panel in the editor sidebar shows text at real pixel sizes, with **Hinted** and **Unhinted** columns side by side. Type a test string (it starts with the current glyph's character) and comma-separated sizes (12, 16 and 24 by default; 1–512 pixels). The hinted side uses the same TrueType hinting as the export, so zone and stem changes show up right away; the unhinted side stays the same. If hinting can't be applied — for example a mistake in the control instructions — the panel says why and the hinted side falls back to the unhinted outlines.

## Guides and background

### Guides

A guide is a straight reference line to align stems, serifs or whole glyphs against. A guide belongs either to the layer you're editing or to the master: a **global** guide is drawn in every glyph of that master, so one baseline or cap-height guide serves the whole font. Global guides are drawn in a different colour from local ones.

- **Add:** right-click the canvas and choose **Add Horizontal Guide** or **Add Vertical Guide**, or drag from the strip at the left or top edge of the canvas into the glyph. Dropping back on the strip cancels.
- **Move:** drag a guide to slide it. While you drag a node near a guide, the node snaps onto it.
- **Edit:** double-click a guide to open its panel: position (X, Y), angle, a name, and a lock. A locked guide can't be dragged until you unlock it.
- **Make global / Make local** in the panel moves the guide between the layer and the master, keeping its position, angle, name and lock.
- **Delete** removes the selected guide.
- **Show or hide:** View › Show Guides (Ctrl+Shift+L).

Guides are saved in the `.kernest` file, undo with Ctrl+Z, and are never exported into the font.

### Background

Each layer can hold a background drawing, shown tinted behind the outline. Use it to trace a sketch or keep an earlier version of the glyph while you rework the outline.

- **Edit › Selection to Background** (Ctrl+J) moves the selected paths out of the outline and into the background, replacing what was there. **Add to Background** (Ctrl+Shift+J) also moves them, but keeps the existing background and adds to it. With nothing selected, the whole layer is used.
- **Swap with Background** exchanges the outline and the background; **Clear Background** removes the background.
- **Edit Background** (Ctrl+B) lets you draw on the background directly: the pen, rectangle and ellipse tools edit the background while the outline stays untouched. Press Ctrl+B again to return to the outline. The sidebearing fields are paused while you edit the background.
- **Show or hide:** View › Show Background (Ctrl+Shift+B).

The background is saved in the `.kernest` file and never exported into the font.

### Editor sidebar

The **Transform**, **Hints** and **Pixel preview** panels sit in a sidebar beside the canvas — on the right, or on the left in Arabic. Each panel keeps its own collapse arrow, and the sidebar scrolls when the open panels grow taller than the window. **View › Show Sidebar**, or the sidebar button at the end of the editor toolbar, shows or hides the whole sidebar; Kernest remembers the choice between sessions. With the sidebar hidden the canvas gets the full width.

### Transform panel

The **Transform** panel in the editor sidebar scales, rotates, skews and mirrors the selected paths and components — or the whole layer when nothing is selected.

- **Scale X %** and **Scale Y %** resize; **Rotate °** turns counter-clockwise; **Skew X °** and **Skew Y °** slant; the two mirror buttons flip left-to-right or top-to-bottom.
- The nine-point picker chooses the origin the transform works around: a corner, an edge midpoint or the centre of the selection's bounding box.
- **All masters** applies the same transform to this layer in every master, so the glyph stays compatible.
- **Apply** performs the transform as one step that Ctrl+Z undoes.

### Measuring

With the **Measure** tool (L), drag a line across the outline. Kernest marks where the line crosses the outline and shows the distance between each pair of crossings, plus the line's angle — handy for checking stem widths and diagonals. Esc clears the measurement. While editing the background, the measure tool measures the background instead.

### Grid and view toggles

- **Grid:** Font Info › General › Grid has **Spacing** and **Subdivision**. Points snap to a grid of Spacing ÷ Subdivision units while drawing and dragging. Spacing 0 turns the grid off.
- **View menu:** Show Nodes, Show Anchors, Show Guides, Show Background and Show Bounding Box (the rectangle around the outline) each switch their drawing on or off. Kernest remembers these choices between sessions.

## Anchors and composites

An anchor is a named point that tells other glyphs where to sit: a mark above a letter, a second mark above the first, or the join between two cursive letters. Anchors are drawn as small diamonds with their name beside them.

- **Show or hide:** **View › Show Anchors** (Ctrl+Shift+A).
- **Add:** right-click the canvas and choose **Add Anchor**. **Rename** it in the panel at the bottom; **drag** it or nudge with the arrow keys (Shift for ×10); **Delete** removes it. Every anchor edit undoes with Ctrl+Z.
- **Glyph › Set Anchors** (Ctrl+U) puts sensible anchors on the selected glyphs (or the open glyph) in every master at once:
  - letters get `top` at cap height or x-height and `bottom` at the baseline, plus `ogonek` where the name calls for it;
  - marks get `_top` at the bottom of their outline and `top` at its top, so they hang from the letter's `top`; marks that sit below use `_bottom` and `bottom`;
  - ligatures get `caret_1`, `caret_2`… between their parts, and cursive letters get `entry` and `exit`.
  - Glyphs that take no anchors are left alone.
- **Components that align:** select a component and use the lock button beside X and Y. Closed (green) means the component is placed by its anchors and follows them when they move; open (grey) means you placed it by hand and it stays put. A mark placed by anchors offers its own `top` as a target, so a second mark stacks on the first instead of on the letter.
- **Glyph › Create Composites…** builds glyphs from their parts by name: `Aacute` becomes `A` + `acutecomb`, placed by anchors in every master. Glyphs that already have outlines keep them. **Add Glyphs** has the same option as "Build composites from their parts", so typing `Aacute acutecomb` draws the composite and its mark in one step.
- **In exported fonts:** anchors become the OpenType `mark` and `mkmk` features (marks on letters, marks on marks), `curs` for cursive joins, and ligature carets in `GDEF`. Adding a mark by its name (`acutecomb`) also gives it its code point (U+0301), so text typed with combining marks shapes correctly.

## Spacing and kerning

- **Text tabs** (Window › New Text Tab, Ctrl+T): type sample text to see your glyphs together.
  - Click between two glyphs to place the cursor.
  - **Alt+Left** and **Alt+Right** change the kerning there by 10 units, or by 1 unit with Shift held too.
  - The **Features** button previews OpenType features on the sample text (see [Features](#features)).
- **Kerning window** (Window › Kerning, Ctrl+Shift+K): every kerning pair for the current master, with a filter box and a row for adding pairs.
- **Kerning groups:** a glyph has a **left group** (used when it's on the right side of a pair) and a **right group** (used when it's on the left side). Group pairs are written as `@group`. A pair between two specific glyphs is an **exception**, and it overrides the group pair.

## Masters, axes and instances

Open **Font Info** (Ctrl+I) to set:

- **Family:** the family name, units per em and version.
- **Hinting:** the settings for autohinting on export (see [Export](#export)):
  - **Smallest size** and **Largest size:** the range of text sizes, in pixels per em, that gets hinting instructions (8 to 50 by default).
  - **Increase x-height up to:** at sizes up to this one, the x-height is rounded up so lowercase letters stay legible (14 by default; 0 turns it off).
  - **Hint composites as whole glyphs:** hints accented letters and other composites as one shape, instead of through their components.
  - **PostScript hints:** **Blue scale**, **Blue shift** and **Blue fuzz** control how OTF exports keep overshoots near zones; the defaults are the usual PostScript values.
  - **Control instructions:** TTFAutohint control instructions applied when exporting TrueType, one per line (for example `H point 0 yshift 0.5 @ 12`).
  - Zones and stems under Masters guide PostScript hinting. For TrueType, ttfautohint ignores manual zones it can't use.
- **Axes:** for example Weight (`wght`) from 100 to 900, with a default value.
  - **Mapping** (avar): apps use *user* values (the numbers in a font menu or CSS), masters use *design* values. Add points to map one to the other, for example a Weight axis whose user 400–700 covers design 400–900. User values must rise strictly and design values never fall; points stay inside the axis range. With no points, user and design values are the same.
  - **Values** (STAT): names for the styles at each position on the axis, such as Regular at 400 or Bold at 700. Apps show these names. Each value can carry a range (from–to) it covers and link to another axis's value; instances take their style name from these names.
  - Removing an axis takes masters and instances off it (their positions on that axis are dropped) and deletes intermediate and alternate layers that use it. Kernest asks first, and Ctrl+Z undoes it.
- **Masters:** each master is a full set of outlines at a location on the axes. Add a master by copying an existing one, then edit its outlines. **Add from font…** brings a master in from another font (a compiled font, or a Kernest, Glyphs, UFO or designspace source): pick the source master and its location, and Kernest reports which glyphs are compatible and which differ before anything is added. Each master has **Zones** and **Stems** editors: zones keep edges like the baseline and x-height on one pixel row at small sizes (a positive size marks a top edge, a negative size a bottom edge); stems are the common stroke widths — PostScript exports use them for hints, and TrueType uses the first horizontal stem as its guide width.
- **Instances:** named styles such as SemiBold at a location between masters. They become static fonts on export; untick **Export** on an instance to leave it out.
  - **Weight class** and **Width class** set the numbers apps use to sort and match styles (weight 1–1000, width 1–9). Leave them empty (**Automatic**) to take them from the instance's position on the Weight or Width axis, or from its name.
  - **Style link** marks an instance as the Bold, Italic or Bold Italic of another instance (or of the family). Exported fonts then carry the right style-map names, so apps group Regular and Bold as one family.
  - **Generate as New Font** turns the instances into masters of a brand-new font, opened in its own window: one master per instance at its location, with the outlines interpolated. Handy for turning a variable design into a static family you can then edit per style.
- **Variable fonts:** by default export builds one variable font covering every axis. **Add variable font** defines extra variable fonts from the same family: each keeps a name, a file name, a range of each axis (empty ends use the axis range) and a set of glyphs (empty keeps every glyph). Export writes one variable file per entry, so a family can ship e.g. a full Weight×Width font plus a lighter Text-only cut.

**File › Compare with Font…** opens a report comparing the open font with another font: family info, metrics, axes, masters, glyphs and kerning, side by side, with the differences listed. Use it to check what changed between two versions or against a reference font.

Switch the master you're editing with the buttons at the top right. Previews interpolate live. Kernest checks that every glyph can be interpolated, meaning the same number of paths, nodes and components in each master, and lists the glyphs that can't be in the **Incompatible** category of Font View.

## Names and notes

Font Info › **General** holds the names that go into the exported font's name table: designer and designer URL, manufacturer and manufacturer URL, copyright, vendor ID, license and license URL, trademark, description and sample text. Leave a field empty to leave it out of the export. The family name stays at the top of Font Info.

Font Info › **Notes** is a free text box for remarks to yourself about the font. Notes are saved in the `.kernest` file and undo with Ctrl+Z, but they are never exported into the font.

## Features

OpenType features are the substitutions and positioning rules that run when text is shaped: ligatures, stylistic alternates, mark placement and so on. Font Info › **Features** is where they come from.

- **Automatic features:** Kernest reads your glyph names and offers a switch for each feature it can build: `liga` from ligature names like `f_i`, `ss01`…`ss20` from suffixes like `a.ss01`, and `aalt` gathering the alternates. Turn a switch on and the feature is generated for you; the **Generated code** disclosure shows exactly what will be compiled.
- **Your own code:** the editor below the switches is for hand-written feature code, with line numbers. **Apply features** compiles it; errors are listed with their line and column, and clicking an error jumps to that line. When it compiles cleanly you'll see "Compiled with no errors." A hand-written feature replaces the generated one with the same tag; your code is used as-is. **File › Import Features…** merges a `.fea` file in, showing what's new and what it replaces first (see [Import](#features-1)).
- **In exported fonts:** generated and hand-written features are compiled together into GSUB/GPOS, alongside the mark and kerning features Kernest builds from anchors and kerning.
- **Preview in text tabs:** the **Features** button in a text tab lists every available feature with a checkbox, plus **Shape preview**. With Shape preview on, the sample text is shaped with the ticked features (using the same engine as browsers), so you can see `fi` become a ligature or `a` become its `ss01` alternate. Spacing edits are paused while Shape preview is on; turn it off to edit kerning again.

## Export

**File › Export › Font…** (Ctrl+E) builds fonts with [fontc](https://github.com/googlefonts/fontc):

- **Variable font** (`Family-VF.ttf`): built from all the masters. It needs at least two masters, an axis, a master at the default location, and glyphs that can be interpolated. If Font Info › **Variable fonts** defines extra cuts, export writes one variable file per entry, each with its own axis ranges and glyph set.
- **Static fonts** (`Family-Style.ttf`): one per exported instance, or one per master if there are no instances. Instances between masters are interpolated by Kernest first. Each static font's weight class (used by apps to sort and match styles) comes from its **Weight class** in Font Info, or from its position on the Weight axis, or from its style name (Thin, Light, Bold…) if there's no Weight axis; its width class comes from **Width class** or a Width axis. Style-linked instances carry the style-map names that group them with their base family.

Choose the formats:

- **TTF:** TrueType outlines, for desktop apps.
- **OTF:** PostScript outlines: CFF for static fonts and CFF2 for the variable font (`Family-VF.otf`), with intermediate layers and alternates just as in the TrueType variable font. OTF exports carry the master's zones, stems and blue controls in the font's Private DICT, plus automatic stem hints and your manual glyph hints.
- **WOFF2:** compressed web fonts, for static and variable fonts.

And what happens before building:

- **Remove overlaps** merges overlapping contours in static fonts. Variable fonts keep them, because their masters must stay compatible.
- **Apply metrics keys** re-applies every metrics key, so sidebearings are up to date.
- **Autohint** adds TrueType hinting instructions to TTF and WOFF2 files, static and variable, using the settings in Font Info › Hinting, including any control instructions. Hinting makes text crisper at small sizes on Windows. OTF files aren't affected. Kernest hints with [tilvisan](https://github.com/simoncozens/tilvisan), a Rust port of ttfautohint.
- **Use production names** renames glyphs in compiled fonts to their production names (set per glyph in Font View); without it, compiled fonts keep the working names. UFO sources always carry the working → production mapping.

Kerning and kerning groups are compiled into GPOS, and your feature code — generated and hand-written (see [Features](#features)) — is included. Glyphs that aren't set to export are left out.

### Editable sources

- **File › Export › UFO…** writes editable sources instead of compiled fonts: one `.ufo` per chosen master, plus a `.designspace` tying them together, for editing in other font tools. Tick the masters to write (the default one is marked), and choose whether to **decompose smart components** (flatten smart glyphs to outlines; otherwise they stay plain components) and write **colour layers** as named UFO layers.
- **File › Save As .glyphs…** writes a Glyphs 3 `.glyphs` copy of the open font. This is a copy out: the working document stays a `.kernest` font, since Glyphs cannot hold everything Kernest keeps (colour layers, guides, notes).

## Colour fonts

Kernest makes colour fonts in the **COLRv1** format: glyphs built from coloured layers, each filled with a palette colour or a gradient. Windows 11, current browsers and design apps show them in colour. The glyph's normal outline is the black-and-white fallback for apps that don't support colour.

1. **Palettes:** in Font Info › Colour palettes, choose **Add palette**, then set its colours with the swatches or as `#rrggbb` / `#rrggbbaa` (the last two digits are the opacity). Extra palettes, for example one for dark backgrounds, have the same number of colours as palette 1, which is the default.
2. **Colour layers:** in the glyph editor, **+ Colour** adds a colour layer in every master. The first one starts as a copy of the glyph's outline; later ones start empty. Pick a colour layer in the **Layer** list and draw it like any other layer. Layers stack from Colour 1 at the bottom upwards; **↓** and **↑** change the order.
3. **Paint…** chooses how the layer is filled:
   - **Solid colour:** a palette colour, with an opacity.
   - **Linear gradient:** from a start point to an end point, in font units.
   - **Radial gradient:** outwards from a centre, up to a radius.
   - Gradients go through two or more colour stops, each with a palette colour, a position and an opacity.

By default a colour layer's paint is the same in every master. In the **Paint…** dialog the **Master** list lets you give one master its own paint — gradient points, stops and opacities — while the others keep theirs; **All masters** sets every master at once. Colour layers must match point for point across masters, like the outlines, and they interpolate in variable fonts.

For colour that changes between masters, Font Info › **Per-master colours** gives a master its own palette: tick the master, then edit its colours. On export Kernest stacks the masters' colours as cross-fading layers, so a variable font shows each master's palette exactly at its location and blends between them.

On export, every colour layer becomes a glyph of its own (`O.color0`, `O.color1`, …), and Kernest writes the `COLR` and `CPAL` tables into TTF, OTF and WOFF2 files, static and variable. Glyphs whose layers are all solid, opaque colours also get a COLRv0 record, so Windows 10 and older apps can show them in colour too. In variable fonts, gradient points, stop positions and opacities that differ between masters are written as COLRv1 variation deltas, so they interpolate along with the outlines.

## Import

**File › Import…** (Ctrl+Shift+O) opens a font from another editor as a new, unsaved Kernest font. Kernest never changes the original file.

- **Glyphs:** `.glyphs` (Glyphs 2 and 3) and `.glyphspackage`.
- **UFO:** a single `.ufo` (one master), or a `.designspace` with its UFO sources.
- **Compiled fonts:** `.otf`, `.ttf` and `.woff2`. Kernest draws the outlines back out of the built font and brings over metrics and kerning. A variable font gives the default master and each axis extreme; intermediate masters and named instances beyond these are approximations. Compiled fonts have no editable components or feature code, so outlines arrive as paths and features are left out.

`.glyphspackage` and `.ufo` sources are folders. Use **File › Import Folder…** to pick the folder, or pick any file inside it with Import….

Kernest brings over the axes, masters, outlines, components, anchors, kerning, instances and features, plus brace and bracket layers and, from Glyphs files, metrics keys (glyph-wide and per layer), alignment zones and stems, and the hinting settings in a **TTFAutohint options** custom parameter. From designspaces, sparse sources become intermediate layers and rules become alternates. Right-to-left kerning isn't supported yet. If anything is left out, a dialog lists it.

### SVG outlines

**File › Import SVG…** imports one SVG file per glyph, matched by file name: `A.svg` goes to glyph `A`, and files with no matching glyph are left out. **Edit › Paste SVG…** (in a glyph tab) pastes SVG text copied from Figma, Illustrator or another vector editor into the current glyph. Both offer three placements:

- **Fit cap height:** scales the drawing to the master's cap height and sits it on the baseline.
- **Keep size at origin:** keeps the drawing's size and moves its bottom-left corner to the origin.
- **Keep as is:** only flips the vertical axis; the coordinates stay as they are.

Strokes, text and images are left out; convert them to filled outlines in your vector editor first. The glyph gets symmetric sidebearings from the drawing's bounds, and each import is one undo step.

### Metrics

**File › Import Metrics…** brings widths, sidebearings and kerning from an AFM file or another font — a `.kernest`, Glyphs, UFO or compiled font, choosing which master to read. Tick what to import (widths and sidebearings, kerning, or both) and which sidebearing to keep: the outline slides so the source's left or right sidebearing stays fixed while the imported width is applied. Kerning pairs are merged over the current ones; pairs pointing at glyphs this font doesn't have are left out. Locked glyphs are skipped. One undo step.

### Features

**File › Import Features…** merges a `.fea` file into the hand-written feature code (see [Features](#features)). Before applying, a dialog lists what's new and what replaces existing features, plus any problems in the merged code. A feature block in the file replaces the current block with the same tag; new tags are appended; language systems, lookups and comments outside feature blocks are kept, and repeated `languagesystem` lines are dropped. One undo step.

## Scripting and plug-ins

Kernest can run Python scripts and plug-ins against the open font. Scripts run in a
separate process, so a slow or stuck one can be stopped and can never hang the editor.
Each run that changes the font is **one undo step**; a run that only reads (or prints)
leaves no undo entry. `print(...)` and errors appear in a console while the script runs.

Kernest comes with a Python interpreter bundled, so scripts work out of the box.
**Settings › Python** shows its version and path, and lets you point Kernest at another
Python if you prefer.

### The Script menu

- **Macro Panel…** (Ctrl+Shift+P) opens a code box and a console. Type Python and press
  **Ctrl+Enter** to run it against the open font. The code is remembered next time.
- **Run Last Script** repeats the previous run.
- **Run Script File…** runs any `.py` file from disk. Because a script can change your
  font and run any code on your computer, Kernest asks first if the file is outside your
  Scripts folder.
- Your scripts appear as menu items. **Open Scripts Folder** opens
  `%APPDATA%\Kernest\Scripts`, and **Reload Scripts** rescans it after you add or edit
  files. Folders become submenus. A `# MenuTitle: Name` comment at the top of a file sets
  its menu name. Kernest puts two sample scripts there the first time: *Report Metrics
  Keys* (prints the metrics keys in use) and *Set Sidebearings to Grid* (snaps
  sidebearings to the grid).

### Plug-ins

Plug-ins live in `%APPDATA%\Kernest\Plugins`, one folder each with a `plugin.json` and a
Python file. **Settings › Plugins** lists them, turns them on or off, and opens the
folder. Kernest seeds two samples:

- **Filters** appear in the **Filter** menu and run on the selected glyphs (or every
  glyph if none are selected). A filter can ask for settings in a small dialog first —
  the sample *Round Widths* asks for a step and rounds each width to it. Like any script,
  a filter's changes are one undo step.
- **Reporters** draw over the canvas for the current glyph without changing the font.
  Turn them on and off under **Script › Reporters**; Kernest redraws them as you edit.
  The sample *Show Bounds* draws the glyph's bounding box and its size.

### Settings

**Window › Settings…** (Ctrl+,) has three tabs:

- **Plugins:** enable, disable and open plug-ins.
- **Shortcuts:** every keyboard action, its current shortcut, **Change** to record a new
  one and **Reset** to restore the defaults. Your choices are saved and used everywhere,
  including the menus.
- **Python:** the interpreter Kernest runs, its version and path.

The full scripting reference — every object and field, the plug-in format and the sample
code — is in [docs/PYTHON_API.md](PYTHON_API.md).

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
| Set Anchors | Ctrl+U |
| Show Anchors | Ctrl+Shift+A |
| Correct Path Direction | Ctrl+Shift+R |
| Tidy up Paths | Ctrl+Shift+T |
| Repeat Last Filter | Ctrl+Shift+F |
| Knife / Ellipse tool | E / Shift+E |
| Show Metrics | Ctrl+Shift+M |
| Show Guides / Show Background | Ctrl+Shift+L / Ctrl+Shift+B |
| Show Bounding Box | Ctrl+Alt+Shift+B |
| Edit Background | Ctrl+B |
| Selection to Background / Add to Background | Ctrl+J / Ctrl+Shift+J |
| New Text Tab / Kerning | Ctrl+T / Ctrl+Shift+K |
| Font Info / Export | Ctrl+I / Ctrl+E |
| Macro Panel / Settings | Ctrl+Shift+P / Ctrl+, |
| Handbook | F1 |

Most of these can be changed in **Settings › Shortcuts**; the menus then show your
shortcut. Undo and Redo also work as Ctrl+Z and Ctrl+Shift+Z.
