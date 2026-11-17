"""The Kernest scripting API.

A script runs with a global ``font`` (a :class:`Font`) and ``selection``. The
wrappers read and write the same JSON model Kernest saves in ``.kernest`` files,
mutating it in place so nothing is lost on the way back. Unknown fields survive
untouched.

Example::

    font.glyphs["A"].layers["m01"].width = 600
    for g in font.selected_glyphs:
        print(g, font.glyphs[g].layers["m01"].width)

Reporter plug-ins call :func:`report` with drawing instructions instead of
changing the font.
"""

from __future__ import annotations

__all__ = [
    "Font", "Glyph", "Layer", "Path", "Component", "Node", "Anchor",
    "report", "line", "path", "text",
]


def _num(v):
    """Format a number the way Kernest does: whole numbers without a decimal."""
    f = float(v)
    if f.is_integer() and abs(f) < 1e15:
        return str(int(f))
    return repr(f)


def _parse_node(s):
    parts = s.split()
    x, y = float(parts[0]), float(parts[1])
    kind = parts[2] if len(parts) > 2 else "line"
    smooth = len(parts) > 3 and parts[3] == "smooth"
    return x, y, kind, smooth


def _fmt_node(x, y, kind, smooth):
    s = f"{_num(x)} {_num(y)} {kind}"
    return s + " smooth" if smooth else s


class Node:
    """A point on an outline. Writes back into the path's compact node list."""

    def __init__(self, nodes, index):
        self._nodes = nodes
        self._i = index

    def _read(self):
        return _parse_node(self._nodes[self._i])

    def _write(self, x, y, kind, smooth):
        self._nodes[self._i] = _fmt_node(x, y, kind, smooth)

    @property
    def x(self):
        return self._read()[0]

    @x.setter
    def x(self, v):
        _, y, k, s = self._read()
        self._write(v, y, k, s)

    @property
    def y(self):
        return self._read()[1]

    @y.setter
    def y(self, v):
        x, _, k, s = self._read()
        self._write(x, v, k, s)

    @property
    def kind(self):
        return self._read()[2]

    @kind.setter
    def kind(self, v):
        x, y, _, s = self._read()
        self._write(x, y, v, s)

    @property
    def smooth(self):
        return self._read()[3]

    @smooth.setter
    def smooth(self, v):
        x, y, k, _ = self._read()
        self._write(x, y, k, bool(v))

    @property
    def on_curve(self):
        return self.kind != "offcurve"

    def __repr__(self):
        x, y, k, s = self._read()
        return f"Node({x}, {y}, {k!r}, smooth={s})"


class NodeList:
    """The nodes of a path, as friendly :class:`Node` objects over strings."""

    def __init__(self, raw):
        self._raw = raw

    def __len__(self):
        return len(self._raw)

    def __getitem__(self, i):
        if isinstance(i, slice):
            return [Node(self._raw, j) for j in range(*i.indices(len(self._raw)))]
        if i < 0:
            i += len(self._raw)
        if not 0 <= i < len(self._raw):
            raise IndexError(i)
        return Node(self._raw, i)

    def __iter__(self):
        for i in range(len(self._raw)):
            yield Node(self._raw, i)

    def append(self, x, y, kind="line", smooth=False):
        self._raw.append(_fmt_node(x, y, kind, smooth))

    def clear(self):
        self._raw.clear()


class Path:
    """A closed or open outline (a ``path`` shape)."""

    def __init__(self, data):
        self._d = data

    @property
    def closed(self):
        return self._d.get("closed", True)

    @closed.setter
    def closed(self, v):
        self._d["closed"] = bool(v)

    @property
    def locked(self):
        return self._d.get("locked", False)

    @locked.setter
    def locked(self, v):
        if v:
            self._d["locked"] = True
        else:
            self._d.pop("locked", None)

    @property
    def nodes(self):
        return NodeList(self._d.setdefault("nodes", []))

    @property
    def data(self):
        return self._d


class Component:
    """A reference to another glyph (a ``component`` shape)."""

    def __init__(self, data):
        self._d = data

    @property
    def base(self):
        return self._d["base"]

    @base.setter
    def base(self, v):
        self._d["base"] = v

    @property
    def transform(self):
        return self._d.get("transform", [1, 0, 0, 1, 0, 0])

    @transform.setter
    def transform(self, v):
        self._d["transform"] = list(v)

    @property
    def align(self):
        return self._d.get("align", True)

    @align.setter
    def align(self, v):
        if v:
            self._d.pop("align", None)
        else:
            self._d["align"] = False

    @property
    def data(self):
        return self._d


class _ShapeList:
    """A live view of one kind of shape within a layer's ``shapes`` list."""

    def __init__(self, shapes, kind, cls):
        self._shapes = shapes
        self._kind = kind
        self._cls = cls

    def _items(self):
        return [s for s in self._shapes if s.get("type") == self._kind]

    def __len__(self):
        return len(self._items())

    def __getitem__(self, i):
        items = self._items()
        if isinstance(i, slice):
            return [self._cls(s) for s in items[i]]
        return self._cls(items[i])

    def __iter__(self):
        for s in self._items():
            yield self._cls(s)

    def append(self, data=None):
        d = dict(data) if data else {}
        d["type"] = self._kind
        if self._kind == "path":
            d.setdefault("closed", True)
            d.setdefault("nodes", [])
        self._shapes.append(d)
        return self._cls(d)


class Anchor:
    def __init__(self, data):
        self._d = data

    @property
    def name(self):
        return self._d["name"]

    @name.setter
    def name(self, v):
        self._d["name"] = v

    @property
    def x(self):
        return self._d["x"]

    @x.setter
    def x(self, v):
        self._d["x"] = v

    @property
    def y(self):
        return self._d["y"]

    @y.setter
    def y(self, v):
        self._d["y"] = v


class _AnchorList:
    def __init__(self, raw):
        self._raw = raw

    def __len__(self):
        return len(self._raw)

    def __getitem__(self, i):
        if isinstance(i, str):
            for a in self._raw:
                if a["name"] == i:
                    return Anchor(a)
            raise KeyError(i)
        return Anchor(self._raw[i])

    def __iter__(self):
        for a in self._raw:
            yield Anchor(a)

    def append(self, name, x, y):
        d = {"name": name, "x": x, "y": y}
        self._raw.append(d)
        return Anchor(d)


class Layer:
    """One layer of a glyph: width, paths, components and anchors."""

    def __init__(self, data):
        self._d = data

    @property
    def id(self):
        return self._d.get("id") or self._d.get("masterId")

    @property
    def master_id(self):
        return self._d.get("masterId")

    @property
    def width(self):
        return self._d.get("width", 0)

    @width.setter
    def width(self, v):
        self._d["width"] = v

    @property
    def shapes(self):
        return self._d.setdefault("shapes", [])

    @property
    def paths(self):
        return _ShapeList(self.shapes, "path", Path)

    @property
    def components(self):
        return _ShapeList(self.shapes, "component", Component)

    @property
    def anchors(self):
        return _AnchorList(self._d.setdefault("anchors", []))

    @property
    def metrics_keys(self):
        return self._d.setdefault("metricsKeys", {})

    @property
    def data(self):
        return self._d


class _Layers:
    """Layers keyed by id; a master layer is found by its master id."""

    def __init__(self, glyph):
        self._g = glyph

    def _find(self, key):
        for l in self._g.get("layers", []):
            lid = l.get("id")
            if lid is not None:
                if lid == key:
                    return l
            elif l.get("masterId") == key:
                return l
        return None

    def __getitem__(self, key):
        l = self._find(key)
        if l is None:
            raise KeyError(key)
        return Layer(l)

    def __contains__(self, key):
        return self._find(key) is not None

    def __iter__(self):
        for l in self._g.get("layers", []):
            yield Layer(l)

    def __len__(self):
        return len(self._g.get("layers", []))

    def keys(self):
        return [l.get("id") or l.get("masterId") for l in self._g.get("layers", [])]


class Glyph:
    def __init__(self, data):
        self._d = data

    @property
    def name(self):
        return self._d["name"]

    @property
    def layers(self):
        return _Layers(self._d)

    @property
    def unicodes(self):
        return self._d.get("unicodes", [])

    @property
    def locked(self):
        return self._d.get("locked", False)

    @locked.setter
    def locked(self, v):
        if v:
            self._d["locked"] = True
        else:
            self._d.pop("locked", None)

    @property
    def data(self):
        return self._d


class _Glyphs:
    """Glyphs keyed by name, in glyph order."""

    def __init__(self, font):
        self._f = font

    def _find(self, name):
        for g in self._f.get("glyphs", []):
            if g["name"] == name:
                return g
        return None

    def __getitem__(self, name):
        g = self._find(name)
        if g is None:
            raise KeyError(name)
        return Glyph(g)

    def __contains__(self, name):
        return self._find(name) is not None

    def __iter__(self):
        for g in self._f.get("glyphs", []):
            yield Glyph(g)

    def __len__(self):
        return len(self._f.get("glyphs", []))

    def keys(self):
        return [g["name"] for g in self._f.get("glyphs", [])]


class Font:
    """The whole font source, plus the editor selection."""

    def __init__(self, data, selection=None):
        self._d = data
        self._sel = selection or {}

    @property
    def glyphs(self):
        return _Glyphs(self._d)

    @property
    def masters(self):
        return self._d.get("masters", [])

    @property
    def axes(self):
        return self._d.get("axes", [])

    @property
    def instances(self):
        return self._d.get("instances", [])

    @property
    def kerning(self):
        return self._d.setdefault("kerning", {})

    @property
    def info(self):
        return self._d.setdefault("info", {})

    @property
    def features(self):
        return self._d.get("features", "")

    @features.setter
    def features(self, v):
        self._d["features"] = v

    @property
    def selected_glyphs(self):
        return list(self._sel.get("glyphs", []))

    @property
    def selected_layers(self):
        return list(self._sel.get("layers", []))

    @property
    def current_glyph(self):
        return self._sel.get("currentGlyph")

    @property
    def master_id(self):
        return self._sel.get("masterId")

    @property
    def data(self):
        return self._d


# ---- Reporter drawing instructions ----

_draw = []


def line(x0, y0, x1, y1, color=None, width=None):
    d = {"kind": "line", "x0": x0, "y0": y0, "x1": x1, "y1": y1}
    if color is not None:
        d["color"] = color
    if width is not None:
        d["width"] = width
    return d


def path(d, color=None, fill=None):
    item = {"kind": "path", "d": d}
    if color is not None:
        item["color"] = color
    if fill is not None:
        item["fill"] = fill
    return item


def text(x, y, text, size=None, color=None):
    d = {"kind": "text", "x": x, "y": y, "text": text}
    if size is not None:
        d["size"] = size
    if color is not None:
        d["color"] = color
    return d


def report(*items):
    """Add drawing instructions for a reporter plug-in (font units)."""
    for it in items:
        if isinstance(it, (list, tuple)):
            _draw.extend(it)
        else:
            _draw.append(it)


def _take_draw():
    global _draw
    out = _draw
    _draw = []
    return out