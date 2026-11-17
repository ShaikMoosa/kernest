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