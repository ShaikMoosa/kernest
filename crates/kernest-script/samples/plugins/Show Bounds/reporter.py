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