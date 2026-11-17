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