# Kernest Python API

Kernest can run Python scripts and plug-ins against the open font. This page is the
reference for the `kernest` package they use. The Handbook's
[Scripting and plug-ins](HANDBOOK.md#scripting-and-plug-ins) chapter is the gentle
introduction; this page lists every object and field.

## How a run works

- Kernest starts a Python interpreter **out of process** and hands it a snapshot of
  the font (the same JSON model as a `.kernest` file) plus the current selection.
- Your code runs with a few globals already defined (below). It reads and changes the
  font in place.
- When the code finishes, Kernest takes the font back, checks it is still valid, and
  applies it as **one undo step**. A run that changes nothing leaves no undo entry.
- `print(...)` and errors stream to the console while the script runs.
- A run can be **cancelled** (the Stop button) and is **killed after 60 seconds**, so a
  slow or stuck script can never hang the editor.
- Scripts run with full Python on your own machine. Kernest asks before running a
  script file from outside your Scripts folder.

The interpreter is the embeddable CPython bundled with Kernest (shown in
**Settings › Python**). You can point Kernest at another Python there if you prefer.

## Globals

Every script, macro and plug-in runs with these names available:

| Name | What it is |
|---|---|
| `font` | The open font, a [`Font`](#font). Read and write it freely. |
| `selection` | The raw selection dict (`glyphs`, `layers`, `currentGlyph`, `masterId`). |
| `params` | A filter plug-in's settings from its dialog (a dict). Empty otherwise. |
| `report` | Add drawing instructions — reporter plug-ins only (see [Reporters](#reporters)). |
| `line`, `path`, `text` | Build one drawing instruction each (font units). |

`font` is the friendly way in; `selection` is the same data unchanged. Most scripts
only need `font`.

## Font

| Member | Type | Notes |
|---|---|---|
| `font.glyphs` | keyed view | Glyphs by name, in glyph order. See [Glyphs](#glyphs). |
| `font.masters` | list of dicts | The raw masters (`id`, `name`, …). |
| `font.axes` | list of dicts | The raw design axes. |
| `font.instances` | list of dicts | The raw instances. |
| `font.kerning` | dict | Kerning pairs and groups; created empty if absent. |
| `font.info` | dict | Font info (family name, grid, …); created empty if absent. |
| `font.features` | str | The feature code (read/write). |
| `font.selected_glyphs` | list of str | Names of the selected glyphs. |
| `font.selected_layers` | list of str | The selected layers. |
| `font.current_glyph` | str or None | The glyph open in Edit View. |
| `font.master_id` | str or None | The current master id. |
| `font.data` | dict | The whole font model, for anything not wrapped below. |

Unknown fields are never dropped: the wrappers edit the same dict Kernest saves, so
anything the package does not expose survives a run untouched. Reach it through
`.data` (or `font.data`) when you need it.

A common idiom for "the current master, or the first one":

```python
master = font.master_id or font.masters[0]["id"]
```

## Glyphs

`font.glyphs` is keyed by glyph name and keeps glyph order:

```python
font.glyphs["A"]            # a Glyph
"A" in font.glyphs          # True / False
len(font.glyphs)            # number of glyphs
font.glyphs.keys()          # ["A", "B", ...] in order
for glyph in font.glyphs:   # iterates Glyph objects in order
    ...
```

### Glyph

| Member | Type | Notes |
|---|---|---|
| `glyph.name` | str | The glyph name (read-only). |
| `glyph.layers` | keyed view | Layers by id, or by master id for a master layer. |
| `glyph.unicodes` | list of str | Hex strings such as `"0041"` (read-only). |
| `glyph.locked` | bool | Read/write. Setting `False` removes the flag. |
| `glyph.data` | dict | The raw glyph. |

`glyph.layers["m01"]` finds a layer by its own id, or — for a master layer that has no
separate id — by its master id. `glyph.layers.keys()` lists those keys; iterating
yields every `Layer`.

## Layer

| Member | Type | Notes |
|---|---|---|
| `layer.id` | str or None | The layer's own id, else its master id. |
| `layer.master_id` | str or None | The master this layer belongs to. |
| `layer.width` | number | The advance width (read/write). |
| `layer.shapes` | list of dicts | Every shape, raw. |
| `layer.paths` | live view | The `path` shapes, as [`Path`](#path) objects. |
| `layer.components` | live view | The `component` shapes, as [`Component`](#component) objects. |
| `layer.anchors` | live view | The anchors, as [`Anchor`](#anchor) objects. |
| `layer.metrics_keys` | dict | Metrics keys such as `{"LSB": "=n+10"}`. |
| `layer.data` | dict | The raw layer. |

`paths` and `components` are views over `shapes`: they show only that kind, in order,
and writing through them edits the real shape.

```python
layer = font.glyphs["A"].layers[master]
layer.width = 600
for path in layer.paths:
    for node in path.nodes:
        node.x += 10
```

### Path

| Member | Type | Notes |
|---|---|---|
| `path.closed` | bool | Read/write (default `True`). |
| `path.locked` | bool | Read/write. `False` removes the flag. |
| `path.nodes` | [`NodeList`](#nodelist) | The outline's nodes. |
| `path.data` | dict | The raw path. |

`layer.paths.append()` adds a new closed, empty path and returns it. Pass a dict to
seed it: `layer.paths.append({"nodes": ["0 0 line", "100 0 line"]})`.

### NodeList

The nodes of a path, as [`Node`](#node) objects over the file's compact strings.

```python
nodes = path.nodes
len(nodes)                 # number of nodes
nodes[0]                   # a Node
nodes[-1]                  # the last Node
nodes[1:3]                 # a list of Nodes
for node in nodes: ...     # iterate
nodes.append(x, y, kind="line", smooth=False)   # add a node
nodes.clear()              # remove every node
```

### Node

A point on an outline. Writing a property writes straight back into the path.

| Member | Type | Notes |
|---|---|---|
| `node.x` | number | Read/write. |
| `node.y` | number | Read/write. |
| `node.kind` | str | `"line"`, `"curve"`, `"qcurve"` or `"offcurve"`. Read/write. |
| `node.smooth` | bool | Read/write. |
| `node.super_smooth` | bool | Super-smooth (G2), set in the editor (read-only). Kept when the node moves; setting `smooth` to `False` clears it. |
| `node.on_curve` | bool | `True` unless `kind` is `"offcurve"` (read-only). |

### Component

A reference to another glyph.

| Member | Type | Notes |
|---|---|---|
| `component.base` | str | The base glyph name (read/write). |
| `component.transform` | list of 6 numbers | `[xx, xy, yx, yy, dx, dy]`; default identity. |
| `component.align` | bool | Align to anchors (default `True`); `False` stores the flag. |
| `component.data` | dict | The raw component. |

`layer.components.append({"base": "acutecomb"})` adds a component.

### Anchor

| Member | Type | Notes |
|---|---|---|
| `anchor.name` | str | Read/write. |
| `anchor.x` | number | Read/write. |
| `anchor.y` | number | Read/write. |

```python
layer.anchors["top"]            # by name
layer.anchors[0]                # by index
for a in layer.anchors: ...     # iterate
layer.anchors.append("top", 300, 700)   # add an anchor
```

## Scripts and macros

- **Scripts** are `.py` files in `%APPDATA%\Kernest\Scripts`. Subfolders become
  submenus under the **Script** menu. A `# MenuTitle: Name` comment on the first lines
  sets the menu name; otherwise the file name is used. **Script › Open Scripts Folder**
  opens the folder, **Reload Scripts** rescans it, and **Run Last Script** repeats the
  previous run. Kernest seeds the folder with the two samples below the first time.
- **Macro panel** (**Script › Macro Panel…**, `Ctrl+Shift+P`) is a code box and a
  console. `Ctrl+Enter` runs the code against the open font; the code is remembered
  between sessions. **Run Script File…** runs any `.py` from disk (with a warning if it
  is outside your Scripts folder).

A script changes the font simply by writing to `font`:

```python
font.glyphs["A"].layers[font.master_id].width = 600
```

## Plug-ins

A plug-in is a folder in `%APPDATA%\Kernest\Plugins\<name>` with a `plugin.json` and an
entry script:

```json
{
  "name": "Round Widths",
  "type": "filter",
  "entry": "filter.py",
  "version": "1.0",
  "minKernest": "2.0",
  "description": "Rounds the advance width of the selected glyphs to a step.",
  "parameters": [
    { "name": "step", "label": "Step", "type": "number", "default": 10, "min": 1 }
  ]
}
```

| Field | Meaning |
|---|---|
| `name` | The menu name (required). |
| `type` | `"filter"`, `"reporter"` or `"general"` (required). |
| `entry` | The `.py` file to run, relative to the folder (required). |
| `version`, `minKernest`, `description` | Shown in Settings › Plugins. |
| `parameters` | Optional. Describes the settings dialog for a filter. |

Each parameter has `name`, `label`, `type` (`number`, `string` or `boolean`), and
optional `default`, `min`, `max`. The dialog's values arrive in the script as `params`.
Enable, disable and open plug-ins from **Settings › Plugins**; disabled plug-ins do not
appear in the menus.

### Filters

A filter appears in the **Filter** menu. It runs like a script on the selected glyphs
(or every glyph if none are selected), with `params` from its dialog, and its changes
apply as one undo step.

### Reporters

A reporter does **not** change the font. It draws over the canvas for the current
glyph, and Kernest re-requests it (debounced) whenever the glyph changes. Turn reporters
on and off under **Script › Reporters**.

Call `report(...)` with drawing instructions built by `line`, `path` and `text`.
Coordinates are in **font units** (the same space as the outline); Kernest transforms
them to the screen.

| Helper | Signature |
|---|---|
| `line` | `line(x0, y0, x1, y1, color=None, width=None)` |
| `path` | `path(d, color=None, fill=None)` — `d` is an SVG path data string |
| `text` | `text(x, y, text, size=None, color=None)` — `size` in font units |

`color` is a CSS colour string; `report` accepts items, lists or tuples of items.

```python
report(
    line(0, 0, 500, 700, color="#3dd68c"),
    text(0, 750, "width 500", size=24, color="#3dd68c"),
)
```

## Sample scripts

These two scripts ship with Kernest and are copied into your Scripts folder.

### Report Metrics Keys

Prints the metrics keys set on each glyph's master layer. It only reads, so it leaves
no undo entry.

```python
# MenuTitle: Report Metrics Keys
# Prints the metrics keys (LSB, width, RSB) set on each glyph's master layer.
# A metrics key makes a sidebearing or width follow another glyph, e.g. "=n+10".

master = font.master_id or font.masters[0]["id"]
found = False
for glyph in font.glyphs:
    layer = glyph.layers[master]
    keys = layer.metrics_keys
    if keys:
        found = True
        parts = [f"{k}={v}" for k, v in keys.items() if v]
        print(f"{glyph.name}: {', '.join(parts)}")
if not found:
    print("No metrics keys are set in this font.")
```

### Set Sidebearings to Grid

Snaps the left and right sidebearings of the selected glyphs (or every glyph) to the
font's grid step, shifting the outline and setting the width. One undo step.

```python
# MenuTitle: Set Sidebearings to Grid
# Snaps the left and right sidebearings of the selected glyphs (or every glyph)
# to the font's grid step. The outline is shifted so the left sidebearing lands
# on the grid, and the width is set so the right sidebearing does too.

grid = font.info.get("grid") or {}
spacing = grid.get("spacing") or 0
subdivision = grid.get("subdivision") or 1
step = spacing / subdivision if spacing else 10
if step <= 0:
    step = 10

master = font.master_id or font.masters[0]["id"]
targets = font.selected_glyphs or list(font.glyphs.keys())


def snap(value):
    return round(value / step) * step


changed = 0
for name in targets:
    if name not in font.glyphs:
        continue
    layer = font.glyphs[name].layers[master]
    xs = [node.x for path in layer.paths for node in path.nodes]
    if not xs:
        continue
    minx, maxx = min(xs), max(xs)
    new_lsb = snap(minx)
    new_rsb = snap(layer.width - maxx)
    dx = new_lsb - minx
    for path in layer.paths:
        for node in path.nodes:
            node.x = node.x + dx
    layer.width = new_lsb + (maxx - minx) + new_rsb
    changed += 1

print(f"Snapped sidebearings of {changed} glyph(s) to a {step:g}-unit grid.")
```

## Sample filter plug-in: Round Widths

`plugin.json` is the one shown [above](#plug-ins). The entry script `filter.py`:

```python
# A sample filter plug-in. It runs on the selected glyphs with the parameters
# from the dialog available as `params`.

step = params.get("step", 10) or 10
master = font.master_id or font.masters[0]["id"]
targets = font.selected_glyphs or list(font.glyphs.keys())

changed = 0
for name in targets:
    if name not in font.glyphs:
        continue
    layer = font.glyphs[name].layers[master]
    layer.width = round(layer.width / step) * step
    changed += 1

print(f"Rounded widths of {changed} glyph(s) to {step:g} units.")
```

## Sample reporter plug-in: Show Bounds

`plugin.json`:

```json
{
  "name": "Show Bounds",
  "type": "reporter",
  "entry": "reporter.py",
  "version": "1.0",
  "minKernest": "2.0",
  "description": "Draws the bounding box of the current glyph over the canvas."
}
```

`reporter.py` draws the current glyph's bounding box and its size:

```python
# A sample reporter plug-in. It returns drawing instructions (font units) that
# Kernest draws over the canvas. It does not change the font.

name = font.current_glyph
master = font.master_id or font.masters[0]["id"]
if name and name in font.glyphs:
    layer = font.glyphs[name].layers[master]
    xs = [node.x for path in layer.paths for node in path.nodes]
    ys = [node.y for path in layer.paths for node in path.nodes]
    if xs and ys:
        x0, x1, y0, y1 = min(xs), max(xs), min(ys), max(ys)
        report(
            line(x0, y0, x1, y0, color="#3dd68c"),
            line(x1, y0, x1, y1, color="#3dd68c"),
            line(x1, y1, x0, y1, color="#3dd68c"),
            line(x0, y1, x0, y0, color="#3dd68c"),
            text(x0, y1 + 20, f"{x1 - x0:g} x {y1 - y0:g}", size=24, color="#3dd68c"),
        )
```

## Running the package's tests

The `kernest` package has its own tests. From `crates/kernest-script`:

```sh
PYTHONPATH=python python -m unittest discover -s python/tests
```

CI runs them on every change.