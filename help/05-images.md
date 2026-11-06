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

## AI outlines

**AI Outline (MCP)** asks your own AI tool for outlines. Pick it in Settings: Claude, any OpenAI-compatible service (OpenAI, Ollama, LM Studio, OpenRouter) or any MCP server. BurnCraft can also serve its tools to AI assistants itself (`burncraft --mcp`).
