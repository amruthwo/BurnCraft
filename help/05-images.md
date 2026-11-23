# Images and tracing

## Bringing images in

- **Import** (Ctrl+I) adds SVG, DXF or PDF drawings, PNG, JPG, BMP, GIF, WebP or TIFF pictures, EZCad2 files, or another project. You can also drop files on the window, or copy files in your file manager and paste them (Ctrl+V).
- **SVG**: sizes and units are kept (an SVG made 50 mm wide comes in 50 mm wide), and so are curves, rotations and pictures inside it. Each colour goes on the layer with the nearest colour. The SVG's page sits at the bed's top-left corner; if the drawing would land off the bed, it's put in the middle. Text in an SVG is left out (BurnCraft says so): convert text to paths in your drawing program (Inkscape: Path → Object to Path) before saving. Pictures must be embedded in the SVG: pictures that are only linked to as separate files are skipped, so that an SVG from elsewhere can never pull files from your computer into a design (in Inkscape, Extensions → Images → Embed Images).
- **DXF** (from CAD programs): lines, polylines with arcs, arcs, circles, ellipses, splines and blocks come in at their real size, using the units the drawing says (or millimetres if it doesn't say, and BurnCraft tells you). Pieces whose ends meet are joined into closed shapes. Each colour goes on the nearest layer; a DXF layer named like a BurnCraft layer (C00–C29) goes back on that layer. Text and dimensions are left out (explode them into lines first), and drawings or pictures in other files are never read. Binary DXF isn't supported: save as text (ASCII) DXF.
- **PDF**: the drawing on one page (BurnCraft asks which page if there are several), with its curves, colours and pictures. Text is left out: convert it to outlines (curves) first. A page that is only a picture (a scan) comes in as a picture: trace it to get outlines.
- **DWG** files can't be opened: save the drawing as DXF in your CAD program.
- **Paste**: a picture copied in a browser or another app pastes straight in (Ctrl+V), centred on the mouse. Pasted SVG becomes shapes the same way, and pasted text becomes a Text shape.
- **Copy** puts the selected shapes on the clipboard as SVG too, in their layer colours and real size, so programs that accept pasted SVG text can use them. **Copy as Picture** (Ctrl+Alt+C) copies the selection as a picture instead, for documents and chats; pasting that picture back into BurnCraft gives the shapes again.
- Images go on an Image layer. Adjust brightness, contrast and gamma in the Image tab, and invert.

## How a picture burns

Choose the **Dithering** for a picture in Properties. The canvas shows the picture exactly as it will burn:

- **Threshold**: dark parts burn, light parts don't. For logos and line art.
- **Grayscale**: the power follows the darkness. Smooth shading, where the material shows it.
- **Floyd–Steinberg**, **Jarvis**, **Stucki**, **Sierra** and **Atkinson** spread dots to show shades. Stucki is often the best for photos on wood; Atkinson is lighter and keeps detail, good for faces and sketches.
- **Halftone**: a screen of round dots, like newspaper printing. Set **Lines per inch** (fewer gives bigger dots) and the **Screen angle** (45° is usual).
- **3D slice (depth)**: depth engraving. The picture burns in several **Depth steps**, each at the layer's max power, and each step burns only the places dark enough for it, so black is burned by every step and goes deepest. This is mainly for fiber and CO2 lasers on materials that carve away (wood, stone, metal); a diode laser on wood mostly darkens. Try a small test first.

Each picture is burned at the layer's **line interval**: BurnCraft resamples it to exactly one dot per line before dithering, so the dots land where the laser burns. Properties shows the picture's own resolution and the burn resolution, and warns when the picture has far fewer dots than it burns at (it will look soft or blocky: use a bigger picture, burn it smaller, or use a wider interval).

## Masking a picture

Select a picture and a closed shape (a rectangle, ellipse, closed path or text) on top of it, then in the Image tab choose **Keep Inside** to keep only the part inside the shape, or **Keep Outside** to cut the shape's area out. The parts left out become see-through and never burn. The shape stays, so you can delete it or burn it as an outline.

## Tracing

**Trace Image** turns a picture into outlines, with a live preview. Choose the precise tracer (sharp, follows pixels) or the smooth tracer (VTracer curves), and set threshold, ignore-smaller-than, smoothing and optimising.

## Selections

- **Magic Wand**: click a colour to select similar pixels (tolerance, contiguous). Shift adds, Alt subtracts.
- **Lasso**: drag around an area; magnetic mode snaps to edges.
- **Clean up the edges** with the sliders in the options bar; the dashed outline updates as you drag:
  - **Smooth** rounds off ragged, noisy edges (a bigger number smooths more).
  - **Grow** makes the selection bigger (+) or smaller (−), in pixels.
  - **Specks** drops stray islands and fills pinholes smaller than this many pixels across.
  - **Curves** turns pixel steps in the outline into smooth curves.
  - **Reset** goes back to the default clean-up; **Raw** shows exactly the selected pixels.
- **Selection to Outline** turns the selection into a path that matches the dashed outline exactly.

## Camera

**Camera** (Laser tab) shows a picture of the bed under your design, so you can place it on the real material.

1. Choose a source: a **USB camera** (Linux), a **network camera** (the address of a JPEG snapshot, from an IP camera or a phone camera app), or a **Photo** of the bed taken from the camera's position.
2. Press **Align: click 4 points** and click the four corners of the work area in the picture: front-left, front-right, back-right, back-left. If the corners can't be seen, burn four marks at known positions, tick **The points aren't the corners**, and type their positions.
3. **Show on the bed** and **Opacity** control the overlay. The alignment is saved for next time.

The picture is straightened with a perspective correction. For a fish-eye camera, lower **Lens bend** below 0 until the bed's straight edges look straight on the canvas (raise it above 0 for the opposite, pincushion, bend), then align the four points again.

## Variable text

Variable text burns a different name, number or line each job.

1. Select a text shape and tick **Variable text** in Properties.
2. Open **Variable Text** (Laser tab, or **Rows…** next to the checkbox) and load a **.csv** or **.txt** file. A .txt is one value per line; a .csv can have column names in its first row.
3. Put placeholders in the text: `%0`, `%1`… for columns, or `{Name}` using a column name; `{serial}` for a serial number, `{row}`, `{date}`, `{time}`. The buttons in the window add them for you.
4. Pick the row to burn. With **Move to the next row after each job** ticked, each Start moves on to the next row and serial number.

The rows are saved in the project. Opening a .csv or .txt file loads it as rows too.

## AI outlines

**AI Outline (MCP)** asks your own AI tool for outlines. Pick it in Settings: Claude, any OpenAI-compatible service (OpenAI, Ollama, LM Studio, OpenRouter) or any MCP server. BurnCraft can also serve its tools to AI assistants itself (`burncraft --mcp`).
