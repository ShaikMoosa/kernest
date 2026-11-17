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