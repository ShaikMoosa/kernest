# ADR 0001: Per-glyph undo history

- Status: accepted
- Date: 2026-11-24
- Tickets: SMO-51 (decision), SMO-83 (implementation)

## Context

Kernest had one undo/redo stack for the whole font. Undoing in a glyph's tab could
revert an edit made in another glyph, which surprises designers who come from Glyphs,
where every glyph has its own undo history (Glyphs Handbook 3.6.3).

## Decision

`kernest_core::edit::History` keeps:

- one undo/redo history per glyph, for layer edits (outlines, anchors, components,
  metrics, hints, guides, background), capped at 200 steps each;
- one font history for everything else (Font Info, kerning and groups, features,
  axes, masters, exports, adding, removing, renaming and reordering glyphs), capped
  at 500 steps.

The frontend sets the undo context (`set_undo_context`): a glyph while its Edit View
tab is in front, the font everywhere else (Font View, Font Info, text tabs).
Undo, Redo and the Edit menu labels use the history of that context.

Commands keep recording through `apply`, `apply_kerning`, `apply_groups` and
`apply_font`. `apply_font` compares the fonts in place: when only glyph layers
changed, the change is split into per-glyph layer steps; otherwise it is a font step.
Layer steps also go on the font history as one group when the font context is in use
or the action touched several glyphs, so a Font View batch undoes in one step while
each glyph can still undo its own part.

Restores are guarded: a layer is only put back while it is still as the step left it,
so a group undo never overwrites a glyph edited since, and a glyph step that was
already undone through its group is skipped. A font step restores the font but keeps
layers edited after it. A rename moves the glyphs' histories; removing a glyph drops
its history.

## Consequences

- Undo in a glyph never changes another glyph, except through the explicit group undo
  of a multi-glyph action from Font View.
- Edits made in a glyph tab don't appear in Font View's history, as in Glyphs.
- Undo labels come from the change itself ("Edit Outlines", "Change Metrics",
  "Font Info"…), so commands don't need to name their steps.
- The browser mock backend keeps a single history.
