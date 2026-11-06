# Troubleshooting

**Shapes outside the bed won't burn.** Anything over the edge is left out so the head can't crash. Move it inside the bed, or set the right work area in Settings.

**Two arcs or lines don't make a closed shape.** Draw the second one starting on the first one's end (it snaps and joins), or select both and press Join / Close (Alt+J).

**A relation won't apply.** It conflicts with another relation or driving dimension — nothing is moved. Remove a relation (Relations tool: click its tag) or make a dimension driven.

**Ctrl+V pastes the wrong thing.** BurnCraft pastes whatever was copied last: its own shapes, or a picture, SVG or text from another app.

**The laser doesn't connect.** Check the port and speed (115200 for GRBL, 250000 for Marlin). On Linux your user may need to be in the dialout group.

**Big projects feel slow.** BurnCraft caches outlines and draws only what's visible; turning off the filled view (View tab) helps with very large fills.

**Windows warns about the program.** It isn't code-signed yet; choose More info > Run anyway.
