# Images and tracing

## Bringing images in

- **Import** (Ctrl+I) adds PNG, JPG, BMP, GIF, WebP or TIFF pictures, or another project.
- **Paste**: a picture copied in a browser or another app pastes straight in (Ctrl+V), centred on the mouse. Pasted SVG becomes outlines and pasted text becomes a Text shape.
- Images go on an Image layer. Adjust brightness, contrast and gamma in the Image tab, and invert.

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
