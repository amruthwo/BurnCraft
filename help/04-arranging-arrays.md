# Arranging, arrays and patterns

## Selecting and moving

- Click to select; Shift or Ctrl adds. Drag left-to-right to select shapes fully inside the box, right-to-left to select anything touched.
- Drag the handles to scale, the round handle above to rotate (Shift snaps to 15°). Arrow keys nudge by 1 mm (Shift: 10 mm).
- Type exact position, size and angle in **Properties**.

## Arrange

Align, distribute, centre on the bed, bring to front or send to back, group and ungroup — in the Arrange tab.

## Nest

**Arrange > Nest** moves and turns the selected shapes so they fit tightly together, to save material.

- **Sheet**: pack onto the **work area**, or into **another shape** — an offcut you traced, or a rectangle you drew. Select that shape last and choose *Last selected shape*. It stays where it is, and its holes are avoided.
- **Spacing**: the smallest gap between two parts. **Margin**: the smallest gap from the sheet's edge. Allow for the laser's kerf.
- **Rotations**: none, 90° steps, or every so many degrees. More angles can pack tighter but take longer.
- **Mirroring**: allows flipping parts over (off by default — leave it off if the material has a good side).
- **Holes**: lets small parts go inside the holes of bigger ones (off by default).
- **Copies**: nests that many of each part. **Fill the sheet** keeps adding copies until nothing more fits.
- **Precision**: *Fine* follows the outlines more closely; it's slower.
- **Time limit**: BurnCraft tries many orders and keeps the best layout it finds. **Stop** keeps the best layout found so far.

The result is previewed on the bed with how much of the sheet is used. **Apply** moves the shapes (one Undo puts them back); **Cancel** leaves everything as it was. Grouped shapes move together as one part. Parts that don't fit are left where they were.

## Mirror

- **Flip Horizontal / Vertical** mirror the selection in place.
- **Mirror Line**: select shapes, then click any straight edge or construction line to make mirrored copies about it. Or, like LightBurn, select the shapes and then the line last before pressing Mirror Line.

## Arrays and patterns

- **Grid Array**: columns, rows and spacing (centre to centre or padding between edges), with the total size shown.
- **Circular Array**: copies around a centre — the bed centre, below the selection, or the last selected shape.
- **Pattern on Path**: copies along the last selected curve (number of copies or spacing, rotate and scale copies), following LightBurn's Copy Along Path.
- **Align to Path**: moves the selected shapes onto a curve in their left-to-right order.

Arrays stay editable: select any copy and press the **Edit array** button under the selection, double-click a copy, or use Edit Array in the Modify tab or Properties.

## Offset

**Offset** grows or shrinks outlines (outward, inward or both; round, bevel or sharp corners). Offsets of circles stay true circles, and the offset distance shows as a dimension you can change later.

## Boolean operations

Weld, Union, Subtract and Intersect combine closed shapes (Modify > Combine).
