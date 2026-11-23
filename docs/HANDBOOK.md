# Kernest Handbook

Kernest is a font editor for Windows. It works offline, and your fonts are plain files on your disk. This handbook covers everything the app can do today. Press **F1** in the app to open it.

- [Getting started](#getting-started)
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
- [Command line](#command-line)

## Getting started

The first time you open Kernest it shows a **Welcome** window. Pick what describes you best — *New to type design* turns on the guided tour, *I've used other font editors* skips it — then choose how to start: a new font, an existing `.kernest` font, the sample font, or an import from another app. You can reopen the Welcome window later from **Help › Welcome to Kernest**.

The **Start window** lists your recent fonts by file name and folder; type in the search box and use ↑/↓ and Enter to open one. Its side panel links to this Handbook, the keyboard shortcuts and what's new, and shows "Up to date" when update checks are on. Untick **Show at launch** (here or in the Welcome window) and Kernest reopens your most recent font when it starts instead.

### The New Font wizard

**New Font** opens a four-step setup:

1. **Family & metrics** — the family name, units per em, and the vertical proportions (x-height, cap height, ascender, descender).
2. **Scripts & glyph sets** — tick the scripts and glyph sets to start with; overlapping sets are combined. You can add more later.
3. **Masters & axes** — choose *Single style*, *Weight* (Light→Bold), *Weight × Width* (four corners), or *Custom* to define your own axes and masters. Suggested instances are offered; tick the ones to export. One master must sit at the default of every axis.
4. **Review & create** — a summary of everything above. Nothing is created until you press **Create Font**; the new font opens as an unsaved document with empty glyphs, ready to draw.

### Guided tour and checklist

If you chose the guided tour, a short six-step overlay walks the real screens: Font View, Edit View, Text View, kerning, instances and export. Each step points at the control to use; **Next** moves on without doing the work for you. A **Getting started** checklist ticks itself off as you actually create a font, draw a glyph, space glyphs, kern a pair, preview an instance and export. **Show me** jumps to the relevant screen; **End tour** hides the overlay. Restart or reset both from the **Help** menu.

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
- **Export fonts here** and **Check font** on the same right-click menus build the font with the [command line](#command-line) into a `build` folder next to it (Check font then checks the result). A window shows what happened; press a key to close it.
- **Icon and tooltip:** when Kernest saves a `.kernest` folder it draws "Aa" in the font as the folder's icon and sets its tooltip to a summary such as *Kernest Sans · 3 masters · 412 glyphs · 7 instances*. (Explorer has no preview pane for folders, so this is what shows there.) The icon and a `desktop.ini` file live inside the folder and are hidden.

## Font View

The **Font** tab shows every glyph as a cell. Switch between **Grid** and **List** at the top left; in Grid, the slider on the right sets the cell size. Both choices are remembered. The **Actions** menu holds Rename Glyphs… and the batch commands (they run on the selected glyphs), and **Text Preview** in the command bar opens a text tab. The panel at the bottom of the sidebar edits the selected glyph's LSB, width and RSB directly.

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

**List** shows the glyphs as a table: the glyph itself, name, Unicode, category, subcategory, script, LSB, RSB, width, left and right group, export and tags. The selected glyph's properties move to a **Glyph Properties** panel on the right, with a large preview. Click a column heading to sort by it (click again for descending, a third time to return to the font's own order). Double-click a cell to edit it in place — width and sidebearings, groups, tags, export, and the name (which renames the glyph everywhere, see below); Enter or clicking away keeps the change, Escape cancels. Each edit is one undo step. With one glyph selected, the box under the sidebar shows its category, subcategory and script, and lets you set its tags and kerning groups.

### The sidebar: categories, languages, filters

- **Categories** groups glyphs by what they are (Uppercase, Lowercase, Number, Punctuation, Symbol, Separator…).
- **Languages** shows, per language, how many of its characters the font already has (`95 / 574`). Click a language to see the missing characters and **Add Missing** to add them all in one undo step.
- **Filters** holds saved smart filters and lists (below). Click one to show only its glyphs; click again to show all.

### Smart filters

**New Filter…** builds a filter from rules: category, subcategory, case, script, name contains / starts / ends, tag contains, and yes/no rules such as has components, has anchors, has Unicode, masters compatible, exported, metrics keys out of sync, colour glyph, smart glyph. Combine rules with **All** (every rule must match) or **Any** (one is enough), and nest groups to any depth. The dialog shows a live count of matching glyphs as you edit. **Save Selection as List** saves the current selection as a plain list of names instead. Saving a filter switches Font View to it.

### Tags, groups and production names

- **Unicode** (info box, or the list's Unicode column) sets the characters a glyph stands for. Type `0041`, `U+0041` or the character itself; separate several values with commas, or clear the field to leave the glyph unencoded. If a value already belongs to another glyph, Kernest names that glyph and asks whether to move the value over. Undo with Ctrl+Z.
- **Tags** are free labels you put on glyphs (from the list or the info box) to find them again with a "tag contains" filter.
- **Left / right group** set the kerning groups a glyph belongs to.
- **Writing direction** (Glyph Properties) sets a glyph as left to right, right to left or both; **Automatic** takes it from the glyph's script.
- **Vertical metrics** (Glyph Properties) are for vertical typesetting, as in CJK text: **TSB** and **BSB** are the space above and below the drawing, and **Height** is the vertical advance. Changing TSB moves the vertical origin and keeps BSB; changing BSB changes the height. Until a glyph has its own values, it uses the master's typo ascender as its top and typo ascender − typo descender as its height (**Use the default vertical metrics** goes back to those). Once any glyph has vertical metrics, exported fonts get vhea and vmtx tables (VVAR in variable fonts) and OTF files a VORG table. Glyphs files keep them as `vertWidth` and `vertOrigin`, and the vhea values as the vheaVert… parameters. Feature code can use them as `${H:TSB}` and `${H:BSB}`.
- **Production name** (info box) is the name the glyph gets in compiled fonts when you tick **Use production names** in the Export dialog; the working name stays as it is. UFO sources always carry the mapping.
- **Category, Case, Subcategory and Script** (Glyph Properties) are worked out from each glyph's Unicode value and name. Set one to give a glyph your own value, for one glyph or for every selected one at once; **Automatic** goes back to the worked-out value.
  - The values steer Font View's categories and filters, feature code (`${name:category}` and friends) and the automatic features.
  - They also set export's GDEF classes: a glyph with the category **Mark** is a mark, and one with the subcategory **Ligature** is a ligature.
  - **Case** splits letters into upper and lower case; small caps, minor and other count as letters without case.
- **Sort name** (Glyph Properties) is the name the list view sorts the Name column by, for example to keep `f_i` next to `f`.
- **Colour labels** help you track work and don't affect exported fonts. In Glyph Properties:
  - **Colour label** gives the selected glyphs one of Glyphs' twelve colours; **×** removes it.
  - **Layer colour** labels their layer in the shown master instead.
- **Where labels show:**
  - **Grid:** the glyph's colour tints the whole cell, and a layer colour tints the right half.
  - **List:** the **Colour** column shows both.
  - **Sidebar:** **Colours** lists the labels in use; click one to show only those glyphs.
  - **Editor:** the Layers panel shows each layer's colour.
- **Files:** Glyphs files keep labels as `color` on the glyph and the layer, including the RGBA colours Glyphs 3 allows, and UFOs as `public.markColor`. A font with labels saves at format version 21.
- Glyphs files keep these as `category`, `subCategory`, `case`, `script` and `sortName`, and UFOs in the glyph lib under the keys glyphsLib uses. A font with any of them saves at format version 20.

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
| Select | V | Click or drag to select nodes; drag to move them. Shift+V switches to lasso selection and back. |
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
- **Magnetic snapping:** a node or anchor you drag snaps when it comes within a few pixels of:
  - another node's x or y, including handles;
  - a metric line (baseline, x-height, cap height, ascender, descender);
  - the left or right edge of the advance width, or its middle;
  - a guide.

  A red line shows what it snapped to. Hold **Ctrl** while dragging to place it freely. **Settings › Appearance › Snap distance** sets how close it has to come (0 turns snapping off).
- **Selecting:** Ctrl+A selects everything, and double-clicking a node selects its whole path.
- **Super-smooth nodes:** press Alt+Shift+Enter, or Shift+double-click a node, to make the selected nodes super-smooth (G2) or turn them back into smooth nodes. A super-smooth node has a star marker. Kernest keeps it on the line between its handles at the point where the curvature is the same on both sides, so the curve flows through it with no jump in curvature. It moves there whenever its handles change, after the edit is made. Only nodes between two curves can be super-smooth. In a .glyphs file they are saved as smooth nodes at that position.
- **Harmonize** (Path menu, Ctrl+Shift+H) moves the selected smooth nodes, or every smooth node when none are selected, to that same point once. Nodes next to a straight line, at an inflection (where the curve changes direction) or on a flat side are left as they are.
- **Lasso selection:** to select along a free-form shape, hold the Select tool (or right-click it) and choose **Lasso Select**, or press Shift+V; the toolbar icon shows which one is on. Draw around the nodes, anchors and components you want. Where the lasso crosses itself, the parts it loops over twice are left out.
- **Adding and removing:** with either shape, Shift+drag adds to the selection and Ctrl+drag takes away from it. Alt+drag uses the other shape for that one drag.
- **Zoom:** Ctrl+= and Ctrl+- zoom in and out, and Ctrl+0 fits the glyph to the window.
- **Components** (Ctrl+Shift+C): add another glyph as a component. Kernest won't let a glyph contain itself.
- **Component from Selection** (Glyph menu, or right-click with something selected) turns the selected paths and components into a new glyph:
  - A dialog suggests a name such as `_part.H`. Start the name with `_` to keep the new glyph out of exported fonts.
  - The new glyph goes right after the current one, with the shapes from every master and intermediate or alternate layer. A component of it takes their place at the same position.
  - The glyph's masters must have the same paths for this to work.
  - Ctrl+Z undoes it in one step.
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

Use the **Layers** panel at the top of the editor sidebar to switch between the master's own layer and the glyph's extra layers; the buttons under the list add new ones, and the selected layer's own actions (Move…, Ranges…, Paint…, Remove) appear below them.

- **+ Intermediate** adds a brace layer: an extra drawing at its own location on the axes (for example `{wght 550}`), used when a glyph needs a correction between two masters.
  - It starts as the glyph interpolated there, so nothing changes until you edit it.
  - It must stay compatible with the masters (same paths, points and components).
  - **Move…** changes its location; **Remove** deletes it.
- **+ Alternate** adds bracket layers: a second design used wherever the location is inside the axis ranges you give (for example `[wght 600–]` for a simpler `$` at heavy weights).
  - Every master gets a copy to edit, and the alternates must be compatible with each other.
  - A range includes both its minimum and its maximum; leave a box empty for no limit.
  - **Ranges…** changes the ranges for all masters; **Remove** removes the alternate everywhere.
- **In exported fonts:** static fonts get the right design at their location. Variable fonts get intermediate layers as extra sources and swap alternates in with OpenType feature variations.

**Backups.** **+ Backup** saves a copy of the master's drawing (outline, anchors, width, guides and background) as a backup layer, named after the date and time. Backups appear in the Layer menu as **Backup: name**. They aren't exported, interpolated or checked for compatibility; they're kept for reference.

- **Use as master** swaps the backup's drawing with the master's, so the current drawing becomes the backup and nothing is lost.
- **Remove** deletes that backup only.
- Backup layers in Glyphs files (named layers such as "legacy" or a dated copy) are imported as backups and written back by Save As .glyphs.

### Smart components

A smart glyph is a component that stretches: each glyph that uses it sets its own values on the smart glyph's axes. For example, one `_stem` can be the stems of H, I and L at different heights.

1. Open the part glyph (for example `_stem`), click **Smart…** and add axes such as Height from 0 to 100. The master layer is the shape at every axis' minimum.
2. Click **+ Pole** to add the shape at another position, for example Height 100. It appears in the **Layer** menu as "Smart Pole Height 100"; edit it point for point to the other extreme.
3. Use the glyph as a component (**Add Component**). With the component selected, its axis values appear next to X and Y; type a value to reshape it.

Poles must match the master layer point for point. On export, smart components become ordinary outlines. Smart components in Glyphs files import as smart components.

**Smart handles.** Give the smart glyph an anchor named exactly like one of its axes (for example an anchor `Height` on top of `_stem`, in the master layer and in each pole). Wherever the smart glyph is used, that anchor shows as a red diamond on the component. Drag it with the Select tool: it slides along the line between its positions at the axis' minimum and maximum, a label shows the value, and letting go sets it (one undoable step). Each axis with such an anchor gets its own handle.

### Corner, cap and segment components

Small glyphs placed onto an outline, so a serif, an ink trap or a decorative stroke is drawn once and reused everywhere. They stay live: the outline keeps its own points, and Kernest draws the components onto it wherever it's shown or exported. Change the component glyph and every place that uses it follows.

Right-click an on-curve node and choose:

- **Add Corner Component…:** a `_corner.name` glyph replaces the corner at that node. Draw the corner as an open path whose first point lies on the y axis above the origin and whose last point lies on the x axis, the origin being the corner (as in Glyphs; `origin`, `left` and `right` anchors work as in Glyphs too). Kernest turns, shears and slides it to fit the strokes either side of the node exactly as Glyphs does, so corner glyphs made in Glyphs place the same.
- **Add Cap Component…:** a `_cap.name` glyph replaces the segment from the node to the next one, such as the end of a stem. Draw the cap as an open path from its first point to its last, both on the baseline, bulging upwards; Kernest stretches it to fit between the two nodes, pointing out of the shape.
- **Add Segment Component…:** a `_segment.name` glyph replaces the segment from the node to the next one: a wave, a zigzag. Its path runs from its first point to its last (or from its `start` to its `end` anchor), is stretched to the segment's length and, on a curve, bent to follow it.

The **Corner components** panel in the sidebar lists them. Pick another glyph, set **Width %** and **Height %**, line a corner up with the outgoing stroke (◁), the incoming stroke (▷) or halfway (◁▷), flip it (⇋), or for a cap tick **Fit to stroke width** so its height scales with its width. **×** removes one; **Decompose** turns all of the glyph's components into outline, in every master. Markers on the canvas show where they sit.

A component is added to the same node in every master and intermediate layer, and masters whose components differ are reported as incompatible. Corner components in Glyphs files stay live; Save As .glyphs writes Kernest's corner components as Glyphs corner components. Cap and segment components from Glyphs files are reported as left out (they work differently there), and Kernest's own caps and segments are written into the outline in a `.glyphs` copy.

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

### Strokes and pen points

A path can be drawn with a pen instead of being filled: an open stroke for a monoline design, or a closed path drawn as a ring. Select a path (any of its nodes) and use **Stroke** in the editor sidebar.

- **Stroke this path** gives it a 40-unit stroke. The editor then draws the stroke's outline, with the path itself as a thin line through it that you edit as usual.
- **Width:** the pen width in font units.
- **Position:** where the path sits in the stroke: **Left** (the path is the stroke's left edge, as the path runs), **Centre**, **Right**, or any value from −100 to 100.
- **Join:** how corners are drawn: **Miter**, **Round** or **Bevel**.
- **Start cap / End cap** (open paths): **Flat**, **Round**, or **Square** (extends half the width past the end).
- **Fill** (closed paths): fills the inside too, so only the outer edge of the stroke shows.
- **Pen points:** select one node of a stroked path to give it its own **Pen width**; the width changes evenly towards the next node. **Reset** goes back to the stroke's width.
- **Expand Outline** turns every stroked path of the glyph into ordinary outlines, in all masters at once. **Remove stroke** fills the path again.

The stroke settings apply to the same path in every master and intermediate layer, so the masters stay compatible; each master keeps its own width (and pen-point widths), which interpolate. A stroke's outline always has the same structure for the same path, whatever the width, so a thin and a heavy master interpolate in a variable font. Masters whose strokes differ in join, caps, fill, pen points or smooth nodes are reported as incompatible.

Strokes are expanded to outlines when you export (TTF, OTF, WOFF2, variable fonts) and when you export UFO sources. Inner corners can leave small overlaps; static exports remove overlaps, and variable fonts allow them. Save As .glyphs keeps strokes: Glyphs shows the pen width, and Kernest reads all its settings back.

### Brushes

A **brush** is a glyph whose outline is drawn along a path instead of a pen, for example an arrow that follows a curve. Name the glyph `_brush.something`, untick **Export this glyph** so the brush itself stays out of your fonts, and give it a `start` and an `end` anchor: the part of the drawing between them stretches to the path's length and bends to follow it; anything beyond them extends past the path's ends. Without the anchors, the brush's left and right edges at y = 0 stand in.

In the Stroke panel, choose the brush from **Brush** (the list shows every glyph whose name starts with `_brush`). **Pen (no brush)** goes back to a pen. Change the brush glyph and every path drawn with it follows. Brush curves are split a fixed number of times before bending, so masters stay compatible.

### The pixel tool

The **Pixel** tool (X) draws a glyph out of components of a glyph named `pixel`, placed on the grid (Font Info › Grid; the spacing must be 2 or more and sets the pixel size).

- Click a grid cell to add a pixel there, or click a pixel to remove it. Drag to paint (or erase) a run of cells; the whole drag is one undoable step.
- If the font has no `pixel` glyph yet, Kernest offers to create one: a square the size of the grid, not exported.
- Change the `pixel` glyph (a dot, a cross, a diamond) and every pixel glyph changes with it.

### Masks and shape order

A **mask** is a path that cuts away from the shapes drawn before it, like a stencil, without changing their outlines. Use it to notch a stem or trim a serif while keeping both shapes editable.

- Open **Shapes** in the editor sidebar. It lists the layer's paths and components in drawing order, top drawn first.
- Select a path and tick **Mask** to make it a mask: it then cuts every shape above it in the list (the shapes drawn before it), and the editor shows it as a dashed outline with the cut-away area greyed.
- **↑** and **↓** move the selected shape. With **All masters** ticked, the same move happens in every master and intermediate layer, so the glyph stays compatible.
- The Mask setting applies to the same path in every master. Masters with different masks are reported as incompatible.

Masks are applied when you export (fonts and UFO sources); the exported glyph has the cut outline and no mask. Save As .glyphs keeps masks, and Glyphs shows them as its own masks.

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
- **Measure** in the panel makes it a **measurement guide**: wherever it crosses the outline (components included) a small square is drawn, with the distance between each pair of crossings, as with the Measure tool. Unlike the tool, a measurement guide keeps showing its numbers whatever tool is active, as long as guides are shown. A global measurement guide measures every glyph of its master. Measurement guides are kept in Glyphs files (`showMeasurement`).
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

### Images

Place a scan or a sketch behind a glyph to trace it.

- **Glyph › Add Image…** picks a PNG, JPEG or GIF file. The image sits behind the outline at its own size; drag it to move it (unless it's locked).
- The **Image** panel in the editor sidebar sets its position (X, Y), size (W, H, in font units), rotation and lock. **Crop to Layer Bounds** shows only the part inside the glyph's width, from descender to ascender; **Reset Crop** shows all of it again. **Reveal** opens the file's folder; **Replace…** picks another file; **Remove** takes the image away.
- Each master and layer has its own image.
- **View › Show Image** shows or hides every image.

Kernest stores the image's file path, not the picture: relative to the font's folder when the image is inside it, so a folder you move together keeps working. If the file is missing, the editor shows an "Image not found" box in its place. Images are never exported. Save As .glyphs keeps them as Glyphs background images.

### Annotations

Annotations are review notes on the canvas, for yourself or for someone checking the design. Choose the **Annotate** tool (A), then pick a mark in its palette:

- **Text note:** click to place, then type the note in the palette. Its width can be set.
- **Arrow:** points at something; set its angle.
- **Circle:** rings an area; set its diameter.
- **Plus** and **Minus:** mark a spot to make thicker or thinner.

With the select arrow in the palette, click a mark to select it, drag to move it, and press Delete (or **×**) to remove it. **View › Show Annotations** shows or hides them all. Annotations are saved with the layer, are never exported, and come back from Save As .glyphs.

### Editor sidebar

The **Variation preview**, **Layers**, **Transform**, **Hints** and **Pixel preview** panels sit in a sidebar beside the canvas. Variation preview (in fonts with axes and two or more masters) draws the glyph anywhere on the axes as you drag the sliders, and names the nearest instance. The status bar under the canvas shows the layer, how many nodes are selected, the pointer position and the zoom, with − and + buttons. The sidebar is on the right, or on the left in Arabic. Each panel keeps its own collapse arrow, and the sidebar scrolls when the open panels grow taller than the window. **View › Show Sidebar**, or the sidebar button at the end of the editor toolbar, shows or hides the whole sidebar; Kernest remembers the choice between sessions. With the sidebar hidden the canvas gets the full width.

### Preview area and preview window

**View › Show Preview** (Ctrl+Alt+P) opens a preview area under the canvas. It draws one line of text as it will look in the font, and updates as you edit.

- **Text:** the glyph being edited, by default. Type other text around it, such as `nn/a nn` (use `/glyphname` for glyphs without a character). Each glyph tab keeps its own text.
- **Master or instance:** the menu chooses what to draw. **Master being edited** follows the master you're working on; pick another master, or an instance to see the glyphs interpolated there.
- **Size** sets the text size, and **Invert** switches to white on black.
- Drag the line above the area to change its height, or focus it and press ↑ or ↓. Double-click a glyph in the preview to open it.
- The area's height, size, colours and the master or instance are remembered.

**Window › Preview Window** (Ctrl+Alt+Shift+P), or **Open in Window** in the preview area, opens the same preview in a separate window, for example on a second display. It mirrors the current tab: the preview text of a glyph tab, or the text of a text tab. It is empty while another tab, or no font, is open. **Keep on Top** keeps it in front of the main window. The window has its own master or instance, size and colour choices, and it closes when Kernest closes.

### Transform panel

The **Transform** panel in the editor sidebar scales, rotates, skews and mirrors the selected paths and components — or the whole layer when nothing is selected.

- **Scale X %** and **Scale Y %** resize; **Rotate °** turns counter-clockwise; **Skew X °** and **Skew Y °** slant; the two mirror buttons flip left-to-right or top-to-bottom.
- The nine-point picker chooses the origin the transform works around: a corner, an edge midpoint or the centre of the selection's bounding box.
- **All masters** applies the same transform to this layer in every master, so the glyph stays compatible.
- **Apply** performs the transform as one step that Ctrl+Z undoes.

**Aligning.** The row of align buttons lines up the selected paths, nodes, anchors and components:
- **Align left, centre and right** line them up horizontally; **top, middle and bottom** line them up vertically.
- **Distribute horizontally** and **Distribute vertically** leave equal gaps between three or more of them.
- What moves together:
  - a path with all of its nodes selected moves as one shape;
  - otherwise each selected node moves with its handles;
  - each component and anchor moves on its own.
- **to** picks what they line up with:
  - **Selection**: the box around the whole selection.
  - **Glyph width**: for left, centre and right, the left edge, the middle of the advance width or the right edge.
  - **A metric line**: for top, middle and bottom, the baseline, x-height, cap height, ascender or descender. For example, select nodes, choose **x-height** and click **Align top** to put them all on the x-height.
- Each click is one step that Ctrl+Z undoes. A moved component stops following its anchors.

**Boolean operations.** Three more buttons in the same row, also in the Path menu, combine closed paths:
- **Union** merges the selected paths, or every path when none is selected.
- **Subtract** takes the selected paths away from the unselected ones, for example a circle over a stem to cut a hole.
- **Intersect** keeps only the parts where the selected paths overlap the unselected ones.
- With nothing selected, Subtract and Intersect use the frontmost path (the last one drawn) as the selected one.
- A path counts as selected when any of its nodes is. Each side is merged first, and only unselected paths the selected ones reach change: paths elsewhere in the glyph stay as drawn.
- The result keeps curves and follows the usual path direction, so holes run the other way. On a compatible glyph, it applies to every master when the results still interpolate.
- Open, locked and stroked paths, and components, take no part. If open paths were selected, Kernest says so.

**Align Selection** (Path menu, Ctrl+Alt+Shift+A) lines the selected nodes, handles and anchors up on one line:
- It picks a vertical line when they're closer together horizontally, and a horizontal line otherwise.
- The transform origin picks the line: the origin's column gives left, centre or right; its row gives top, middle or bottom.

### Measuring

With the **Measure** tool (L), drag a line across the outline. Kernest marks where the line crosses the outline and shows the distance between each pair of crossings, plus the line's angle — handy for checking stem widths and diagonals. Esc clears the measurement. While editing the background, the measure tool measures the background instead.

### Grid and view toggles

- **Grid:** Font Info › General › Grid has **Spacing** and **Subdivision**. Points snap to a grid of Spacing ÷ Subdivision units while drawing and dragging. Spacing 0 turns the grid off.
- **View menu:** Show Nodes, Show Anchors, Show Guides, Show Background and Show Bounding Box (the rectangle around the outline) each switch their drawing on or off. **Nodes, Metrics, Anchors, Guides, Zones** and **Background** are also toggles at the right of the editor toolbar. Kernest remembers these choices between sessions.
- **Alignment zones** (View › Show Alignment Zones, or **Zones** in the toolbar) shade the master's zones (Font Info › Masters) and mark nodes by height, so overshoots are easy to check:
  - **Gold diamond:** the node sits exactly on a metric line (baseline, x-height, cap height, ascender or descender).
  - **Ring:** the node is inside an alignment zone, as an overshoot should be.
  - **Small warning triangle:** the node is up to 3 units outside a zone or off a metric line, which is usually a slip.
  - Hovering a mark says which it is. Brace layers use their master's zones.

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
  - **Preferred marks:** when a variant of a mark exists, composites use it:
    - a suffixed composite uses suffixed parts, so `aacute.sc` is built from `a.sc` and `acutecomb.sc`;
    - uppercase letters use `.case` marks (`Aacute` uses `acutecomb.case`);
    - `i` and `j` under a top mark become `idotless` and `jdotless` and use `.i` or `.narrow` marks (`imacron` uses `idotless` and `macroncomb.narrow`). Marks below, as in `idotbelow`, keep the dot.
    - Without a variant, the plain mark is used.
- **In exported fonts:** anchors become the OpenType `mark` and `mkmk` features (marks on letters, marks on marks), `curs` for cursive joins, and ligature carets in `GDEF`. Adding a mark by its name (`acutecomb`) also gives it its code point (U+0301), so text typed with combining marks shapes correctly.

## Spacing and kerning

- **Text tabs** (Window › New Text Tab, Ctrl+T): type sample text to see your glyphs together.
  - Click between two glyphs to place the cursor.
  - **Spacing and kerning from the keyboard:** hold Shift as well for steps of 10 units. The status bar shows the new value for a moment, and each key press is one undo step.

    | Keys | Does |
    |---|---|
    | Alt+← / Alt+→ | Kern the pair at the cursor 1 unit tighter / looser |
    | Ctrl+← / Ctrl+→ | Right sidebearing 1 unit smaller / larger |
    | Ctrl+Alt+← / Ctrl+Alt+→ | Left sidebearing 1 unit larger / smaller |
    | Ctrl+Alt+↑ / Ctrl+Alt+↓ | Move the outline 1 unit right / left inside the same width (both sidebearings at once, for monospaced designs) |
    | Ctrl+Alt+E | Make the pair at the cursor an exception with its current value, or remove its exception |

    The sidebearing keys act on the glyph after the cursor (or before it, at the end of a line). A sidebearing linked by a metrics key is left alone, and the status bar says so. With **No kerning** chosen in the Kerning preview menu the kerning keys are off, and with **Kerning, spacing locked** the spacing keys are. Change any of these keys in **Settings › Shortcuts**.
  - **Kerning bars:** each kerned pair shows a bar under the baseline, as wide as the kerning: red where the glyphs move closer, green where they move apart, with the value below it. Exceptions have a dashed bar and a diamond marker. Hover a bar to see the pair it comes from (for example `@A | @V`); click it to open the **Kerning window** with that pair selected.
  - **Measurement line:** tick **Measure** in the toolbar (or press Ctrl+Alt+M) to draw a dashed line across the text at one height, half the x-height at first. For every glyph it shows the distance from the glyph's left edge to where the line first meets the outline, the distances between crossings (stems and counters), and from the last crossing to the right edge. Type a height in the field next to **Measure**, or drag the round grip at the start of the line. The setting and height are remembered. The line measures the current master, so it is hidden in vertical text, in Shape preview and while previewing another location.
  - **Kerning preview** (the menu next to the kerning mode) has three settings, remembered between sessions:
    - **No kerning** sets the text without kerning, also in Shape preview and the Preview strip, and turns the kerning shortcuts and fields off. Use it to judge the spacing alone.
    - **Show kerning** (the default) shows and edits kerning as above.
    - **Kerning, spacing locked** shows kerning but locks the LSB, width and RSB fields, so only kerning can change.
  - The **Features** button previews OpenType features on the sample text (see [Features](#features)).
  - The **Preview** strip at the bottom shows the text in every master at once; the status bar names the pair at the cursor and its kerning. The tab is named after its text.
  - The **Samples** menu lists the strings from **Settings › Sample strings**, where you can add, edit and remove them.
  - **Writing direction:** the three buttons in the toolbar set the tab to **left to right**, **right to left** or **vertical** (Ctrl+Alt+Shift+L, R or V). Each tab keeps its own direction.
    - Right to left, lines start at the right edge. The arrow keys follow the text, so ← moves to the next glyph, and Alt+→ tightens the pair at the cursor. Latin words and numbers inside right-to-left text keep their own order (a simplified bidi algorithm). Glyphs take their direction from their **Writing direction** property, else from their Unicode value or name. Kerning between glyphs of different directions isn't applied.
    - Vertical text runs top to bottom in columns from right to left. It uses each glyph's vertical metrics (see [Glyph properties](#font-view)), and the spacing fields become **TSB**, **Height** and **BSB**. Kerning is horizontal only, so it can't be edited here.
    - With **Shape preview** on, the text is shaped in its direction, run by run, so Arabic joins and reads right to left. Vertical text uses the font's vertical metrics and `vert` features.
    - By default a tab switches to right to left when its text starts with Arabic or Hebrew letters, and back for other letters. Turn this off under **Settings › Appearance › Text tabs follow the writing direction of their text**.
- **Kerning window** (Window › Kerning, Ctrl+Shift+K): every kerning pair for the current master (Left · Value · Right), with a filter box and a row for adding pairs. Click a pair to preview it on the right, where the group members are listed too. **Search options** narrow the filter: group pairs or exceptions, left or right side, or only pairs touching the glyphs selected in Font View. The **−** button deletes the selected pair.
- **Kerning groups:** a glyph has a **left group** (used when it's on the right side of a pair) and a **right group** (used when it's on the left side). Group pairs are written as `@group`. A pair between two specific glyphs is an **exception**, and it overrides the group pair.

### Kerning tools

The Kerning window's **Actions** menu tidies the pairs of the current master, or of every master with **All masters** ticked. Each is one undo step.

- **Compress** moves glyph pairs one step up to their groups, the left side first: `T–m` becomes `@T–m`, and the next Compress makes it `@T–@n`. A pair whose value the group kerning already gives is removed. One that differs from it, such as `T–ö` at −40 where `@T–@o` is −70, stays: it's a real exception.
- **Clean Up** removes pairs that do nothing: zero values, pairs naming a glyph or group that doesn't exist, and exceptions equal to the group pair they fall under.
- **Copy** takes every pair of the master; **Paste (Overwrite)** puts them into another master (or another font), replacing values already there, and **Paste (Keep)** only adds the pairs that are missing.
- **Show All Glyphs** lists the glyphs of each group under its name.
- In hand-written feature code, `@MMK_L_T` stands for the glyphs whose right group is T (used on the left of a pair) and `@MMK_R_o` for those whose left group is o (used on the right), as in Glyphs: `pos @MMK_L_T' -50 @MMK_R_o;`.

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
  - **Elidable** (under each instance, once an axis has values): tick an axis to mark the value name at the instance's position as one apps may leave out of style names, like Regular on the weight axis (Glyphs' Elidable STAT Axis Value Name). It does the same as ticking that value's own elidable box.
  - Removing an axis takes masters and instances off it (their positions on that axis are dropped) and deletes intermediate and alternate layers that use it. Kernest asks first, and Ctrl+Z undoes it.
- **Masters:** each master is a full set of outlines at a location on the axes. Add a master by copying an existing one, then edit its outlines. **Add from font…** brings a master in from another font (a compiled font, or a Kernest, Glyphs, UFO or designspace source): pick the source master and its location, and Kernest reports which glyphs are compatible and which differ before anything is added. Each master has **Zones** and **Stems** editors: zones keep edges like the baseline and x-height on one pixel row at small sizes (a positive size marks a top edge, a negative size a bottom edge); stems are the common stroke widths — PostScript exports use them for hints, and TrueType uses the first horizontal stem as its guide width.
- **Instances:** named styles such as SemiBold at a location between masters. They become static fonts on export; untick **Export** on an instance to leave it out.
  - **Family name** exports the instance as part of another family, so one source can make several families (for example Lexend, Lexend Deca and Lexend Exa). Leave it empty to use the font's family name. Instance names only need to be unique within a family, so each family can have its own Bold. Family names in other languages, from Glyphs files, are kept and written to the font's name table.
  - **Weight class** and **Width class** set the numbers apps use to sort and match styles (weight 1–1000, width 1–9). Leave them empty (**Automatic**) to take them from the instance's position on the Weight or Width axis, or from its name.
  - **Style link** marks an instance as the Bold, Italic or Bold Italic of another instance (or of the family). Exported fonts then carry the right style-map names, so apps group Regular and Bold as one family.
  - **Naming** (folded under each instance) overrides the names export derives, as Glyphs' naming parameters do: **PostScript name** (nameID 6 and the CFF font name; ASCII, no spaces), **Full name** (nameID 4), **Compatible full name** (nameID 18), **Style map family** (nameID 1, for style linking) and **File name** (the exported file, without extension; characters Windows forbids become `_`). **Name table entries** add records one per line, in Glyphs' syntax: `0; © 2026 Me` writes a Windows record, `9 1; Ada` a Mac one, and `16 3 1 0x407; Familie` gives the platform, encoding and language IDs (decimal, octal with a leading 0, or hexadecimal). IDs 1, 2, 3, 5 and 6 come from the names above, so they can't be entries. Entries in the font's custom parameters apply to every export; an instance's entry with the same IDs replaces them. Glyphs files keep all of these in both directions.
  - **Generate as New Font** turns the instances into masters of a brand-new font, opened in its own window: one master per instance at its location, with the outlines interpolated. Handy for turning a variable design into a static family you can then edit per style.
- **Variable fonts:** by default export builds one variable font covering every axis. **Add variable font** defines extra variable fonts from the same family: each keeps a name, a file name, a range of each axis (empty ends use the axis range) and a set of glyphs (empty keeps every glyph). Export writes one variable file per entry, so a family can ship e.g. a full Weight×Width font plus a lighter Text-only cut.
  - **Family name** gives the variable font a family of its own (Glyphs' Variable Font Family Name), so it can be installed next to the static fonts; empty uses the font's family name. **Name table entries** work as they do for instances. Glyphs 2 files that name the variable font with the font's Variable Font Family Name and Variable Font File Name parameters get one variable font entry with those names.
  - **Disable masters** leaves the ticked masters out of that variable font (Glyphs' Disable Masters). Each axis then spans only the masters that remain: without a Thin master at the light end, the weight axis starts at the next master. Named instances the remaining masters can't reach are left out, and the export says which. If the default master is left out, the nearest remaining master becomes the default. Instances can carry the same setting in Glyphs files, and they're then interpolated without those masters.
  - **Origin** picks the master this variable font uses as its default (Glyphs' Variable Font Origin). Its location becomes the default value of every axis, so apps show that master when no axis is set; **Default master** keeps the font's own. A master left out by Disable masters, or outside the variable font's axis ranges, can't be the origin: the export says so and keeps the default.
  - **STAT table** writes the axis value names into that variable font (Glyphs' Export STAT Table). Untick it to leave the STAT table out.

- **Virtual masters** (Font Info › Masters): a named location that only glyphs with intermediate layers there use. Use it for an axis that changes only a few glyphs, such as a Crossbar axis that raises the bar of A, E, F and H: add the axis, add a virtual master at its far end, then give those glyphs an intermediate layer at that location. Every other glyph stays as drawn along that axis. Locations must lie inside each axis' range. Save As .glyphs writes them as Glyphs' Virtual Master parameter.
- **Glyph swaps:** each instance and each variable font can swap glyph drawings when it's exported. Use this, for example, for a style whose default `g` is the single-storey form.
  - **Rename Glyphs** takes pairs such as `dollar=dollar.alt`. The exported `dollar` gets the drawing of `dollar.alt`, and the other way round. Unicode values stay with the names. Kerning groups, kerning and the rules in your feature code follow the drawings. If the second glyph doesn't exist, the first one is simply renamed.
  - **Remove Glyphs** lists glyphs to leave out of that export, such as the alternates you swapped in. `*` and `?` work as wildcards, so `*.ss01` removes every `.ss01` glyph.
  - Removing a glyph decomposes the components that use it and drops its kerning. Rules in your feature code that need it are removed too, along with classes, lookups and features left empty. Generated features are built from the glyphs that remain.
  - Save As .glyphs writes both as Glyphs parameters.
- **Glyph processing:** each instance has a **Glyph processing** fold with more of Glyphs' parameters:
  - **Keep Glyphs** exports only the glyphs it lists, plus `.notdef`. Wildcards work, such as `A*, a*`.
  - **Decompose Glyphs** turns the components of the listed glyphs into outlines.
  - **PreFilter** and **Filter** run filters on that instance's outlines, one per line, written as in Glyphs:
    - `RoundCorner;20` rounds corners.
    - `OffsetCurve;10;10` offsets outlines horizontally and vertically; add `;1` to make a stroke instead.
    - `Transformations;Slant:8` slants outlines. It also takes `OffsetX`, `OffsetY`, `Scale`, `ScaleX` and `ScaleY`, joined with `;`.
    - `RemoveOverlap` removes overlaps.
    - End a line with `;include:A,B` or `;exclude:A,B` to choose the glyphs a filter works on.
    - PreFilter runs before overlaps are removed, Filter after.
  - Untick **Remove overlaps** to keep that instance's overlaps even when the export removes them.
  - A variable font's fold has **Decompose Glyphs** and **Decompose all components** (Glyphs' Decompose Components in Variable Font). Its **Glyphs** list takes wildcards too.
  - Under the fields, a note lists anything an export would warn about: patterns that match no glyph, filters Kernest doesn't have, and numbers it can't read. The export goes on without them.
  - Glyphs files bring these parameters in and Save As .glyphs writes them back. A font that uses them saves at format version 24.
- **Export-specific feature code:** each instance and each variable font has a **Feature code** fold with Glyphs' feature parameters. They change the feature code for that export only. They apply in order, to the generated features and to your own code:
  - **Replace Feature** gives a feature new code: write its tag, a semicolon, then the code, such as `liga; sub f i by f_i.alt;`. If the font has no such feature, it's added.
  - **Remove Features** leaves features out, such as `liga, dlig`. A removed feature is also taken out of `aalt`.
  - **Replace Prefix** and **Remove Prefixes** work on the prefixes that Glyphs files bring in. A prefix is the code under a `# Prefix: name` line. Write the name, a semicolon and the new code.
  - **Replace Class** gives a class new glyphs, such as `Uppercase; A B C`. **Remove Classes** removes class definitions.
  - Untick **On** to keep a parameter without applying it. When a parameter names something the code doesn't have, a note appears under the parameters and in the export report. The export goes on without that parameter.
  - These parameters come from Glyphs files and go back to them. A font that uses them saves at format version 23.

A font with masks, annotations, images, interpolation curves, virtual masters or glyph swaps saves at format version 14.

**Number values** (Font Info › Masters) are named numbers with a value per master, such as a `padding` of 24 in Regular and 48 in Bold. Feature code uses them as `$padding` or `${padding*2}` (see [Features](#features)). **Add number** adds one; rename it or remove it in its row. They come from and go back to Glyphs files.

**File › Compare with Font…** opens a report comparing the open font with another font: family info, metrics, axes, masters, glyphs and kerning, side by side, with the differences listed. Use it to check what changed between two versions or against a reference font.

Switch the master you're editing with the buttons at the top right. Previews interpolate live. Kernest checks that every glyph can be interpolated, meaning the same number of paths, nodes and components in each master, and lists the glyphs that can't be in the **Incompatible** category of Font View.

**Custom parameters** (Font Info) lists the settings from a Glyphs source that Kernest doesn't use itself, with their values exactly as Glyphs writes them (`1`, `"text"`, `(a, b)`, `{key = value;}`). They're kept with the font and written back by Save As .glyphs. Kernest applies three of them when it exports (Export sbix Table, SBIX to SVG and Color Layers to SVG; see [Bitmaps and SVG](#bitmaps-and-svg)) and keeps the others without using them. Edit a value, switch one off with **On**, add one with **Add parameter**, or remove it; a value Glyphs couldn't read is refused. A font with custom parameters, backup layers, strokes or corner, cap and segment components saves at format version 13.

**Line spacing** (Font Info › Masters, on each master) sets how far apart lines of text sit and where the underline goes: typo, hhea and win ascender, descender and line gap, plus underline thickness and position. Leave a field empty and it's worked out from the ascender and descender on export. Win ascent, win descent, line gaps and underline thickness can't be negative. **Use Typo Metrics** applies to the whole font: apps then space lines by the typo values. A font that sets any of these saves at format version 12.

### Interpolation curves

Normally every node moves in a straight line from one master to the next. With the **Interpolation curves** tool (I), a node can follow a curve instead, so a turning stroke or a swinging tail doesn't shrink or kink halfway.

- Edit the lower master of the two (for example Light, when Light and Bold sit on the Weight axis). Each node shows where it is here (blue), in the next master up the axis (purple) and in the next master down (yellow), with a dotted line for its path.
- **Drag** the knob in the middle of a node's line to bend the path through that point.
- **Click** a node to choose its curve in the panel: **Linear** (the straight line), **Intermediate point** (through a point halfway), **Handles** (a curve with two handles), or **Rotation** (a turn around a centre; **Center** puts the centre halfway between the two positions, and the direction can be **Auto**, the shorter way, **Clockwise** or **Counterclockwise**).
- With more than one axis, pick the axis in the panel. A curve only works when there's a compatible master further up that axis with the same position on every other axis.

Curves don't change the masters. When you export, Kernest adds intermediate layers a quarter, half and three quarters of the way between the two masters, with each curved node on its curve and every other point interpolated as usual. Static instances and variable fonts both follow these layers. Places that already have an intermediate layer keep it. Save As .glyphs keeps curves in Kernest's own layer data.

### Interpolation warnings

A glyph can be compatible and still morph badly between masters. Kernest compares every master and intermediate layer with the default master and warns when:

- **a path runs the other way round:** it turns inside out halfway;
- **a closed path starts at a different point:** its points slide round the outline;
- **two similar paths are in a different order:** they swap places halfway;
- **a smooth point kinks:** its handles are straight in both masters but bend between them.

These glyphs get an amber dot and are listed in the **Interpolation warnings** category of Font View; the smart filter rule **Interpolation** finds them too. In the glyph editor, a banner lists each warning. **Fix order, direction and start points** lines those up with the default master in one undoable step. Kinks need fixing by hand: give the handles the same length ratio in every master. Exporting a variable font lists the glyphs that still have warnings, without stopping the export.

## Names and notes

Font Info › **General** holds the names that go into the exported font's name table: designer and designer URL, manufacturer and manufacturer URL, copyright, vendor ID, license and license URL, trademark, description and sample text. Leave a field empty to leave it out of the export. The family name stays at the top of Font Info.

Font Info › **Notes** is a free text box for remarks to yourself about the font. Notes are saved in the `.kernest` file and undo with Ctrl+Z, but they are never exported into the font. Switch to **Preview** to read them as Markdown: `#` headings, `**bold**`, `_italic_`, `-` lists and `[links](https://…)`.

Font Info lists its sections on the left (General, Axes, Masters, Exports, Features, Custom parameters, Other, Notes) and shows one at a time. **Filter settings** at the top searches every section at once.

## Features

OpenType features are the substitutions and positioning rules that run when text is shaped: ligatures, stylistic alternates, mark placement and so on. Font Info › **Features** is where they come from.

- **Automatic features:** Kernest reads your glyph names and offers a switch for each feature it can build: `liga` from ligature names like `f_i`, `ss01`…`ss20` from suffixes like `a.ss01`, and `aalt` gathering the alternates. For joining scripts, positional forms build `init`, `medi`, `fina` and `isol` (`beh-ar.init`, `beh-ar.fina`…) and, for Syriac, `med2`, `fin2` and `fin3`, under the right script, so Arabic letters join without hand-written code. `ccmp` swaps `i` and `j` for `idotless` and `jdotless` before a top mark (any mark with a `_top` anchor). Turn a switch on and the feature is generated for you; the **Generated code** disclosure shows exactly what will be compiled. More features come from Glyphs' naming conventions:
  - **Fractions:** `.numr` and `.dnom` figures (or `.numerator` and `.denominator`) make `numr` and `dnom`. With a `fraction` glyph too, `frac` turns `1/2` into a numerator, the fraction slash and a denominator. Fraction ligatures named like `one_fraction_two.afrc` make `afrc`.
  - **Figures:** `.tf` makes `tnum` and `.osf` makes `onum`. `.lf` makes `lnum`, and `.pf` makes `pnum`. `.tosf` is reached from both `.tf` and `.osf`, so `onum` and `tnum` together give tabular oldstyle figures. Each of these features also leads back to your default figures.
  - **Ordinals and zero:** `ordn` puts `ordfeminine` and `ordmasculine` after figures, makes `numero` from `N o period`, and uses `.ordn` alternates. `zero.zero` (or `zero.slash`) makes `zero`, and `.sinf` makes `sinf`.
  - **Ligatures:** `f_f_i.dlig`, `c_t.hlig` and `x_x.rlig` make `dlig`, `hlig` and `rlig` from their parts.
  - **Other suffixes:** `.titl`, `.swsh`, `.cswh`, `.ornm` and `.hist` make the features of the same name. `.pc` makes `pcap` (lowercase) and `c2pc` (capitals).
  - **Capital spacing:** a number value named `cpsp` in Font Info › Masters makes `cpsp`, which adds that much space on each side of every capital. In a variable font, the spacing varies with the value.
- **Your own code:** the editor below the switches is for hand-written feature code, with line numbers. **Apply features** compiles it; errors are listed with their line and column, and clicking an error jumps to that line. When it compiles cleanly you'll see "Compiled with no errors." A hand-written feature replaces the generated one with the same tag; your code is used as-is. **File › Import Features…** merges a `.fea` file in, showing what's new and what it replaces first (see [Import](#features-1)).
- **In exported fonts:** generated and hand-written features are compiled together into GSUB/GPOS, alongside the mark and kerning features Kernest builds from anchors and kerning.
- **Tokens and conditions** (as in Glyphs). The **Tokens and conditions** disclosure under the editor lists them, and **Alt+click** on a token shows what it stands for:
  - `$padding` and `${padding*2}` use the **number values** in Font Info › Masters (+, −, ×, ÷ and brackets work). A static font gets its master's value, interpolated for instances between masters; a variable font gets a value that varies, so `pos @Uppercase <$padding 0 ${padding*2} 0>;` spaces capitals more in bold.
  - `${period:LSB}` reads a glyph's own measurements: `width`, `LSB`, `RSB` or an anchor position such as `${b:anchors.top.x}` (components count towards the sidebearings). They work in expressions too, such as `${period:LSB*2}` or `${a.alt:width/2+10}`, and vary per master like number values, so `pos A <${period:LSB} 0 ${period:LSB*2} 0>;` follows period's spacing in every weight. `TSB` and `BSB` give the space above and below the drawing in vertical typesetting (see Vertical metrics).
  - `$[case == smallCaps]` is the list of glyphs that match, for example `@SmallCaps = [$[case == smallCaps]];`. Compare `name`, `category`, `subCategory`, `case` (upper, lower, smallCaps, minor, noCase), `script`, `direction` (LTR, RTL), `unicode`, `countOfUnicodes`, `tags`, `export`, `hasComponents`, `leftKerningGroup`, `rightKerningGroup`, `layer.width` and `layer.anchors.top.y` with `==`, `!=`, `<`, `>`, `<=`, `>=`, `BEGINSWITH`, `ENDSWITH`, `CONTAINS`, `LIKE` (with `*` and `?`) and `IN`, and combine with `AND`, `OR`, `NOT` and brackets.
  - `standalone lookup demo { … } demo;` inside a feature defines a lookup that only other rules call, such as `sub A acutecomb' lookup demo;`. The feature doesn't run it on its own, and it keeps its place in the lookup order. A lookup can also close with `} ;`, leaving out its name.
  - `#ifdef VARIABLE` … `#endif` keeps code for variable fonts only; `#ifndef VARIABLE` for static fonts; `#else` for the other kind.
  - `condition 600 < wght < 900;` inside a feature: the rules after it (up to the next `condition` or the end of the feature) apply only in that part of the design space. Variable fonts get a feature variation; static fonts keep or drop the rules at their location. Values are axis values as in Font Info › Axes.
  - `pos @Uppercase 10 (wdth:80) 20;` gives a value that changes across the design space: 10 at the default and 20 at a width of 80 (master locations).
  - `lookupflag;` resets the lookup flags, and `delete x;` is written as `sub x by NULL;`.
- **Preview in text tabs:** the **Features** button in a text tab lists every available feature with a checkbox, plus **Shape preview**. With Shape preview on, the sample text is shaped with the ticked features (using the same engine as browsers), so you can see `fi` become a ligature or `a` become its `ss01` alternate. Spacing edits are paused while Shape preview is on; turn it off to edit kerning again. `aalt` starts unticked, as in other apps, because it only lists alternates.

## Export

**File › Export › Font…** (Ctrl+E) builds fonts with [fontc](https://github.com/googlefonts/fontc):

- **Variable font** (`Family-VF.ttf`): built from all the masters. It needs at least two masters, an axis, a master at the default location, and glyphs that can be interpolated. If Font Info › **Variable fonts** defines extra cuts, export writes one variable file per entry, each with its own axis ranges and glyph set.
- **Static fonts** (`Family-Style.ttf`): one per exported instance, or one per master if there are no instances. An instance with its own **Family name** uses that family in its file name and names, and one with a **File name** under Naming uses that instead. If two fonts would be written to the same file (names are compared ignoring case, as Windows does), export stops before writing anything and names both, so no font silently replaces another. Instances between masters are interpolated by Kernest first. Each static font's weight class (used by apps to sort and match styles) comes from its **Weight class** in Font Info, or from its position on the Weight axis, or from its style name (Thin, Light, Bold…) if there's no Weight axis; its width class comes from **Width class** or a Width axis. Style-linked instances carry the style-map names that group them with their base family.

Choose the formats:

- **TTF:** TrueType outlines, for desktop apps.
- **OTF:** PostScript outlines: CFF for static fonts and CFF2 for the variable font (`Family-VF.otf`), with intermediate layers and alternates just as in the TrueType variable font. OTF exports carry the master's zones, stems and blue controls in the font's Private DICT, plus automatic stem hints and your manual glyph hints.
- **WOFF2:** compressed web fonts, for static and variable fonts.

And what happens before building:

- **Remove overlaps** merges overlapping contours in static fonts. Variable fonts keep them, because their masters must stay compatible.
- **Apply metrics keys** re-applies every metrics key, so sidebearings are up to date.
- **Autohint** adds TrueType hinting instructions to TTF and WOFF2 files, static and variable, using the settings in Font Info › Hinting, including any control instructions. Hinting makes text crisper at small sizes on Windows. OTF files aren't affected. Kernest hints with [tilvisan](https://github.com/simoncozens/tilvisan), a Rust port of ttfautohint.
- **Use production names** renames glyphs in compiled fonts to their production names (set per glyph in Font View); without it, compiled fonts keep the working names. UFO sources always carry the working → production mapping.

Kerning and kerning groups are compiled into GPOS, and your feature code — generated and hand-written (see [Features](#features)) — is included. Glyphs that aren't set to export are left out. Line spacing values brought in from a Glyphs or UFO source are written to the OS/2, hhea and post tables; named instances in a variable font get PostScript names; hinted fonts are marked to round sizes to whole pixels.

Exporting the same source twice gives the same tables. To get byte-identical files, also set the `SOURCE_DATE_EPOCH` environment variable (seconds since 1970) before starting Kernest or `kernest-cli`; the creation and modification dates in the `head` table then use it instead of the current time.

When the export finishes, the window lists anything that went wrong (a format that couldn't be built, or a font exported without hinting or colour) and, separately, things worth a look in fonts that were built: metrics keys that couldn't be applied, and glyphs that may not interpolate cleanly.

### Trying fonts in other apps

The Export window can also put the font where other apps see it. Each builds its own fonts, without changing the export settings:

- **Test Install** builds static TTFs and loads them for this Windows session only. Other apps see them once they refresh their font list, and they go away when Kernest quits, so an app never keeps an old copy. Test Install again replaces them.
- **Install for Me** builds static TTFs and installs them for your Windows account (no admin rights), where they stay. Restart apps that were already open.
- **Web Preview** builds WOFF2 fonts and opens a page on `http://localhost:4123` with sample text, a waterfall and the CSS to use them. Use Web Preview again after changes; an open page reloads the fonts by itself. Nothing leaves your computer.
- **Quick destinations** fill in the destination folder: your own fonts folder, the Adobe Fonts folder that Adobe apps read (writing there needs admin rights) and the web preview folder.
- File › Export also has **Test Install**, **Web Preview** and **Export All Presets** (every saved preset, each to its own folder).

### Presets

The left rail of the Export window holds **presets**: named bundles of formats, options, chosen instances and destination folder. **Save preset** stores the current choices under a name; picking a preset restores them; **Remove preset** deletes it. Presets are saved inside the `.kernest` font (so they travel with it) and are shared with the [command line](#command-line) (`kernest-cli export … --preset name`). A font that uses presets saves at format version 11; older files without presets keep their own version.

### Check font

After exporting, **Check font** runs [Fontspector](https://github.com/fonttools/fontspector) over the files you just wrote and opens a report: **FAIL / WARN / PASS / SKIP / ERROR** per check, grouped by category (names, vertical metrics, outlines, features, hinting, variable font), with a plain explanation and the original checker output under *Technical details*. Filter by category or severity, switch between the *OpenType and general quality* and *OpenType specification* profiles, or check a single exported file. **Save report…** writes the report as JSON. Checks run entirely on your computer; no font data is sent anywhere, and network-dependent checks are skipped. **Open Font Info** / **Show glyph** jump to the place to fix a problem.

### Editable sources

- **File › Export › UFO…** writes editable sources instead of compiled fonts: one `.ufo` per chosen master, plus a `.designspace` tying them together, for editing in other font tools. Guides (per glyph and per master) and backgrounds (in the `public.background` layer) are included; strokes are written as the outlines they draw. Tick the masters to write (the default one is marked), and choose whether to **decompose smart components** (flatten smart glyphs to outlines; otherwise they stay plain components) and write **colour layers** as named UFO layers.
- **File › Save As .glyphs…** writes a Glyphs 3 `.glyphs` copy of the open font, including guides, backgrounds, glyph tags, notes and locks, line spacing values, colour palettes and colour layers, backup layers, strokes, custom parameters and variable font settings (their file names and glyph lists; axis ranges have no Glyphs equivalent). An opaque solid colour becomes a palette colour layer; see-through colours and gradients become full-colour layers. This is a copy out: the working document stays a `.kernest` font, since some things have no Glyphs equivalent (a radial gradient's separate focus point, smart filters, export presets).

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

### Bitmaps and SVG

Some apps show colour glyphs from pictures instead of COLR layers. The **Bitmaps and SVG** panel in the glyph editor's sidebar adds them:

- **Add Bitmap…** adds a PNG for one pixel size (an `sbix` strike, used by Apple apps and some emoji fonts). Set the **Pixel size** first; each image pixel covers units-per-em ÷ pixel-size font units. Add one picture per size you want.
- **Add SVG…** adds an SVG drawing (the `SVG ` table, used by Adobe apps and Firefox). It's scaled to the height of the em; X, Y and H place and size it.
- Click a picture in the list to show it on the canvas, and set its position there. Like placed images, pictures and SVG files are referenced by path, relative to the font file, so keep them in a folder next to the font.
- The outline stays the fallback wherever pictures aren't supported.

On export the switches are custom parameters (Font Info › Custom parameters › **Add export switch…**), as in Glyphs:

- **Export sbix Table** — `0` leaves the bitmaps out.
- **SBIX to SVG** — a pixel size: glyphs without an SVG drawing get one made from their bitmap at that size (or the next larger one).
- **Color Layers to SVG** — `1` also writes SVG documents drawn from the colour layers, for apps that read SVG but not COLR.

Glyphs files keep bitmaps as `iColor` layers and SVG drawings as `svg` layers, in both directions. A font with bitmaps, SVG drawings or number values saves at format version 15; a font whose instances have family names of their own saves at version 16, one with vertical metrics or writing directions at version 17, one with instance naming overrides or variable font family names at version 18, and one with super-smooth nodes at version 19.

## Import

**File › Import…** (Ctrl+Shift+O) opens a font from another editor as a new, unsaved Kernest font. Kernest never changes the original file.

- **Glyphs:** `.glyphs` (Glyphs 2 and 3) and `.glyphspackage`.
- **UFO:** a single `.ufo` (one master), or a `.designspace` with its UFO sources.
- **Compiled fonts:** `.otf`, `.ttf` and `.woff2`. Kernest draws the outlines back out of the built font and brings over metrics and kerning. A variable font gives the default master and each axis extreme; intermediate masters and named instances beyond these are approximations. Compiled fonts have no editable components or feature code, so outlines arrive as paths and features are left out.

`.glyphspackage` and `.ufo` sources are folders. Use **File › Import Folder…** to pick the folder, or pick any file inside it with Import….

Kernest brings over the axes, masters, outlines, components, anchors, kerning, instances and features, plus brace and bracket layers, guides (per master and per glyph, locked or not), background drawings, and line spacing values (typo, hhea and win ascender/descender/line gap, underline, and **Use Typo Metrics**). From Glyphs files it also brings metrics keys (glyph-wide and per layer), glyph tags, notes and locks, alignment zones and stems, the hinting settings in a **TTFAutohint options** custom parameter, and colour: **Color Palettes**, palette colour layers and full-colour layers with fills and gradients. From UFOs, backgrounds come from the `public.background` layer. From designspaces, sparse sources become intermediate layers and rules become alternates. If anything is left out, a dialog lists it.

Kerning pairs that point at a group with no glyphs in it are left out (they can't do anything). A pair with the value 0 is kept when one side is a glyph, because it cancels the group kerning for that glyph.

More from Glyphs files:

- **Right-to-left kerning** joins the font's kerning, as fontc and glyphsLib compile it; a right-to-left pair wins over a left-to-right pair with the same sides.
- **Variable Font Settings** become entries in Font Info › Variable fonts: the name, the `fileName` parameter and the glyphs kept by **Keep Glyphs** / **Remove Glyphs** (wildcards such as `*.sc` work). Other parameters of a setting are listed as left out.
- **Backup layers** (extra named layers of a master) are kept as backups (see [Edit View](#intermediate-and-alternate-layers)).
- **Custom parameters** Kernest has no setting for (fsType, panose, isFixedPitch…) are kept in Font Info › Custom parameters.

To see or change them, use **Line spacing** on a master in Font Info › Masters.

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

**Window › Settings…** (Ctrl+,) has these tabs, besides Appearance and Updates:

- **Sample strings:** the texts in the Samples menu of text tabs.
- **Plugins:** enable, disable and open plug-ins.
- **Shortcuts:** every keyboard action, its current shortcut, **Change** to record a new
  one and **Reset** to restore the defaults. Search by name or key, or show only the ones
  you changed (**Customized**). Your choices are saved and used everywhere, including the menus.
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

### Appearance

**Window › Settings… › Appearance** tunes the editor beyond the theme:

- **Handle size** (Small / Medium / Large) scales the node and anchor handles on the canvas.
- **Text View width** (in 1/1000 em) sets how wide a line of text may run before it wraps in the Text View.
- **Text tabs follow the writing direction of their text** switches a text tab to right to left when its text starts with right-to-left letters, and back (on by default).
- **Canvas colours** gives a Light and a Dark value for each drawing element — corner nodes, smooth nodes, hints and zones, foreground stroke, background layer, components, canvas, and negative/positive kerning. Changes apply immediately; **Restore standard appearance** resets them.

### Updates

**Window › Settings… › Updates** controls update checking. It is **off by default**: Kernest sends no update requests. When you turn it on, Kernest reads the latest stable release from GitHub at most once a day and shows a notice with what's new. **Help › Check for Updates…** checks once, right now, even with automatic checks off. **Check now** in Settings does the same.

The notice offers **Later** and **Download & install**. Download & install fetches the setup program for your PC from the GitHub release, checks it against the release's `SHA256SUMS.txt` (it refuses a file that doesn't match), asks about unsaved changes, then runs it and closes Kernest. Nothing is downloaded until you click it.

## Menus

Every command is in the menu bar: File, Edit, Glyph, Path, Filter, View, Script, Window and Help. **Help › Search Menus…** (Alt+Q) finds any command by name: type a few letters and press Enter. A few worth knowing:

- **File:** Open Recent, Revert to Saved, Generate Instances, Exit.
- **Edit:** Select All, Deselect All and Invert Selection (nodes in the glyph editor, glyphs in Font View); Find… jumps to the Font View search; Find and Replace… renames glyphs.
- **Glyph:** New Glyph…, Duplicate Glyph (a copy named `a.001` right after the glyph), Decompose Components.
- **View:** tabs (Next and Previous Tab with Ctrl+Tab and Ctrl+Shift+Tab, Reopen Closed Tab), Zoom In, Zoom Out, Zoom to Active Layer, Actual Size and Full Screen (F11).
- **Window:** Font View, Plugin Manager. **Help:** Keyboard Shortcuts, What's New, Check for Updates…, Report a Problem on GitHub….

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
| Select / Lasso Select tool | V / Shift+V |
| Harmonize / Super-smooth nodes | Ctrl+Shift+H / Alt+Shift+Enter |
| Align Selection | Ctrl+Alt+Shift+A |
| Show Metrics | Ctrl+Shift+M |
| Show Guides / Show Background | Ctrl+Shift+L / Ctrl+Shift+B |
| Show Bounding Box | Ctrl+Alt+Shift+B |
| Edit Background | Ctrl+B |
| Selection to Background / Add to Background | Ctrl+J / Ctrl+Shift+J |
| New Text Tab / Kerning | Ctrl+T / Ctrl+Shift+K |
| Text tab: kern the pair at the cursor | Alt+← / Alt+→ (Shift: 10 units) |
| Text tab: right / left sidebearing | Ctrl+←/→ / Ctrl+Alt+←/→ (Shift: 10 units) |
| Text tab: move the outline inside its width | Ctrl+Alt+↑ / Ctrl+Alt+↓ |
| Text tab: make or remove a kerning exception | Ctrl+Alt+E |
| Text tab: left to right / right to left / vertical | Ctrl+Alt+Shift+L / R / V |
| Text tab: show the measurement line | Ctrl+Alt+M |
| Font Info / Export | Ctrl+I / Ctrl+E |
| Macro Panel / Settings | Ctrl+Shift+P / Ctrl+, |
| Show Preview / Preview Window | Ctrl+Alt+P / Ctrl+Alt+Shift+P |
| Handbook | F1 |

Most of these can be changed in **Settings › Shortcuts**; the menus then show your
shortcut. Undo and Redo also work as Ctrl+Z and Ctrl+Shift+Z.

## Command line

The installer puts `kernest-cli.exe` next to the app. It uses the same engine as the editor, so builds and checks match what you see in the app. Add the install folder to your `PATH`, or call it by full path.

- `kernest-cli export <font> [--format ttf,otf,woff2,variable] [--production-names] [--out dir] [--preset name]` — build fonts. `<font>` is a `.kernest` package or any importable source. Without `--preset`, `--format` picks the formats (default `ttf`); `variable` adds the variable font, and `--production-names` writes production names (such as `uni0628`) into the fonts. `--preset` uses a preset saved in the font (see [Export › Presets](#presets)); `--out` overrides its folder. Problems print as `FAIL:` and set exit code `1`; notes about fonts that were built print as `WARN:` and don't.
- `kernest-cli check <dir|file> [--profile universal|opentype]` — run the same offline Fontspector checks as **Check font**. Exit code `0` = no failures, `1` = at least one FAIL, `2` = a check error or bad input. Warnings alone do not fail the build.
- `kernest-cli convert <in> --to kernest|glyphs|ufo [--out path]` — convert between formats without overwriting an existing output.
- `kernest-cli info <font>` — print the family, glyph count, masters, axes, instances (at their axis values, in the same units as the axes) and presets.

Add `--json` to any command for machine-readable output (handy in CI). The command line is English-only; the app interface is translated.
