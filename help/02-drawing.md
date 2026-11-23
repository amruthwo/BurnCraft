# Drawing and sketching

## Shapes

- **Rectangle**, **Ellipse**, **Polygon**, **Star**: drag to draw. Shift keeps it square or round; Ctrl draws from the centre. Set corner radius, sides or points in the options bar under the ribbon.
- **Line**: click to add points. Shift locks the segment to horizontal, 45° or vertical. Double-click or Enter finishes; click the first point, or press **C**, to close the shape.
- **Arcs in a line**: rest the cursor on the last point you placed for a moment — the next segment becomes a tangent arc. Rest again to switch back.
- **Pen / Curve**: click for sharp points, click-and-drag for smooth curves. Press **C** to close the shape.
- **Arc**: three-point, centre, or tangent (continue from the end of a line).
- **Text**: click to place, then type in Properties. Any installed font; bold, italic, case, spacing and welding. In the font list, ↑ / ↓ try each font on the bed, Enter keeps it and Escape goes back. Fonts without a real bold or italic get a simulated one (marked "simulated").
- **Barcode / QR**: QR codes, Data Matrix and common 1-D barcodes.

## Closed shapes

Lines and arcs you draw onto the end of another open line join into one path, and close when the ends meet — like a CAD sketch. To join existing pieces, select them and use **Join / Close** (Alt+J); ends within 0.05 mm are joined.

## Editing

- **Edit Nodes**: drag points and curve handles. Shift-click adds to the selection, Alt-drag lassos, scroll over a point to grow the selection. Keys: S smooth, C corner, L make lines, U make curves, I insert, B break, D delete.
- **Trim**: click the piece of a line to cut away, up to where it crosses other lines.
- **Fillet / Chamfer**: click corners to round or bevel them.
- **Convert to Path** turns rectangles, ellipses and text into editable paths; **Break Apart** splits a path into its pieces.

## Construction lines

Press **Q** (or Sketch > Construction) to turn the selection into construction geometry: dashed, used for snapping, relations and mirror lines, and never burned. With nothing selected, Q switches new drawing to construction lines.

## Text on a path or a circle

- **Text on Path** (Modify tab): select a text and a line or curve. The text runs along the curve and stays editable: change the words, font and size as usual. The curve is copied into the text, so you can delete it or keep it.
- **Text on Circle**: select a text. It goes around a circle, over the top.
- In Properties: **Radius** (circle), **Shift along** (move the text along the curve), **Inside** (circle: letters inside it along the bottom) or **Below the curve**, **Flip** (the other way round), and **Straighten** to put it back on a line.

## Warp

Select shapes or text, then **Warp** (Modify tab). Choose **Arch**, **Arc**, **Bulge**, **Wave / flag**, **Rise** or **Fisheye** and drag the handle (or the Amount slider), or **Perspective** (drag the four corners) or **Mesh** (drag a 3 × 3 grid of handles). The canvas shows the result live; **Apply** keeps it. Warped shapes become paths; text becomes outlines, so the words can't be edited afterwards (Undo brings them back). Pictures aren't warped.

## Skew and rotating around a point

- **Skew**: in Properties, type the angles and press **Apply**, or hold **Alt** and drag a side's middle handle on the canvas.
- **Rotate around**: in Properties, pick which point of the selection rotating turns around (a corner, an edge's middle or the centre). The rotate handle, the Rotate box and Rotate 90° all use it; a small cross marks the point when it isn't the centre.

## Bolden, thin and stencil bridges

**Bolden / Thin** and **Stencil Bridges** (Modify tab) open one window:

- **Bolden / Thin**: every edge of the selected shapes or text moves out by the amount (in, when it's negative). Good for text that's too thin to engrave, or for a little clearance.
- **Stencil bridges**: for shapes cut out of a sheet as a stencil. A strip of the bridge width is left through every hole (the middle of an O, A, B…), so the piece inside stays joined to the sheet.
- Both turn text into outlines. Open lines and pictures are left as they are.

## Generators

**Generators** (Sketch tab, Create) makes ready-to-cut designs on the current layer:

- **Finger-joint box**: width, depth and height (inside or outside sizes), material thickness, finger width and an open or closed top. The parts (bottom, front, back, sides, top) are laid out side by side. Set the **kerf offset** (half the cut width: Test & Calibrate → Kerf and fit measures it) so the joints fit snugly, and tick **Tabs** to hold the parts in the sheet; both go on the layer.
- **Living hinge**: columns of slits that let a sheet of wood or acrylic bend. Choose the area (or take it from the selection), the slit length, the bridge between slits and the spacing between columns.
- **Gear**: a spur gear with proper (involute) teeth: the number of teeth, the module (gears that mesh need the same module), the pressure angle (20° is standard) and the bore.
- **Jigsaw puzzle**: rows and columns of pieces with round tabs. Take the size from a selected picture to put the puzzle right over it; **Shuffle** changes which way the tabs point.
