# Layers and burning

## Layers

Each palette colour is a layer with its own settings (Layers panel):

- **Mode**: Line (follow outlines), Fill (scan back and forth), Fill + Line, Offset Fill, Image.
- **Speed** (mm/s), **Max / Min power** (%), **Passes**, **Line interval** for fills, scan angle, cross-hatch, bi-directional.
- **Fill shapes individually** finishes each shape before moving on — much faster when shapes are far apart.
- **Constant Power Mode** burns the layer at constant power (M3) instead of dynamic power (M4).
- **Overscan** (fills and images): the head runs on past each end of a scan line with the laser off, so it's at full speed while it burns and edges come out even. It's a percentage of the speed: 2.5% at 100 mm/s is 2.5 mm. It never runs off the bed.
- **Kerf offset** (cut layers): the laser cuts away a thin line of material, so parts come out slightly small and holes slightly big. The kerf offset moves cuts outward on outer edges and inward on holes. Measure the width of a test cut and use half of it. A negative value does the opposite (for inlays).
- **Tabs** (cut layers): short places the laser skips so parts stay in the sheet until you snap them out. Choose a number of tabs per shape or one every so many mm, and the tab width. **Skip holes** leaves shapes inside other shapes without tabs, so their scrap falls out.
- **Burn** and **Show** switch a layer's output and visibility. T1 and T2 are tool layers and never burn (T2 holds construction lines).
- **Save Layer Defaults** keeps your settings for new projects.

## Test & Calibrate

**Test & Calibrate** (Laser tab) makes a labelled test design in a new tab, ready to burn:

- **Power × speed grid**: squares (filled, outlined or a gradient picture), each at its own power (across) and speed (down).
- **Cut-through ladder**: squares cut at different speeds and numbers of passes. The ones that drop out cut through.
- **Line interval (LPI)**: filled squares at different intervals. The lowest lines per inch that still looks solid is the fastest.
- **Focus ramp**: a line with a scale. Prop one end of the material up; the thinnest part of the line is in focus.
- **Kerf and fit**: a kerf gauge (ten strips in a frame) and a row of slots for your material's thickness. Push the strips to one side, measure the gap and type it in the window: it works out the kerf offset and can put it on a layer. The slot that fits snugly shows how much to allow for joints.
- **Picture modes strip**: the same gradient in every picture mode.
- **Colour on metal**: squares at different frequencies and pulse widths, for MOPA fiber lasers on stainless steel or titanium.
- **Fine detail ladder**: text from 4 mm down to 0.5 mm, holes from 2 mm to 0.2 mm, and line pairs with gaps down to 0.05 mm.

Each square of a test burns with its **own settings** (shown in Properties when you select it), so a whole grid fits on one layer. The labels are on their own layer, burned lightly: check its settings before burning.

## Materials

The **Materials** library keeps the settings that worked, for each material, thickness and laser.

- After a test, select the square you liked best and press **Save as material…** in Properties. Or save a whole layer's settings with **Save as material…** in the Layers panel.
- A layer's **Material** list applies a saved material to it. **Unlink** keeps the settings but forgets which material they came from.
- **Materials** (Laser tab) lists your library: rename, add notes, apply, delete, and **Export…** or **Import…** it as a file to share.
- The library is kept in BurnCraft's data folder (in the browser: the browser's storage for this site).

## Tabs by hand

**Tabs** (Modify tab, Cutting) puts tabs exactly where you want them: click on a cut outline to add one, click a tab (the small orange marks) to remove it. Tabs placed by hand replace the layer's automatic ones on that outline, and they're used even when the layer's Tabs box is off. The layer's **Tab width** sets their length. Preview shows the gaps.

## Preview

**Preview** draws the job as the laser will run it, with cut distance, travel and estimated time. **Play Preview** (a play button on a screen) animates it. Shapes outside the bed are outlined in red and left out of the job.

The Laser tab keeps the controls apart: **Preview** (Preview, Play Preview, Frame) never burns: Frame only moves the head around the outline with the laser off. **Burn** (Start, Pause, Stop) fires the laser. Start is the only orange button with the "fire" icon.

## Cut order

Layers burn in the order of the Layers list, top first: drag a layer by its dotted handle to move it. The order is saved with the project. Within a layer BurnCraft uses LightBurn-compatible defaults: nearest shape next, inner shapes before the shapes around them, and open lines from whichever end is closer. Change these in Settings > Cut optimization.

## Running a job

- **G-code lasers**: choose the port and speed in the Laser panel and press **Connect**. **Frame** traces the job outline, **Start** burns, **Pause** and **Stop** control it. The Laser panel shows elapsed and remaining time.
- **Start from** (Laser panel, G-code lasers) sets where the job goes:
  - **Absolute coordinates**: where it sits on the bed in BurnCraft (the default).
  - **User origin**: the job's anchor point goes to a point you save. Type it, or press **Here** to use the head's position (connect and home first).
  - **Current position**: the job's anchor point goes where the head is now. Frame brings the head back to that point afterwards, so Start still begins there.
  - **Job anchor** (the nine dots) picks which point of the job that is: a corner, an edge's middle or the centre.
  - BurnCraft still checks that the job stays on the bed. A laser that hasn't been homed doesn't know where its head is on the bed, so with Current position BurnCraft can only check that the job is no bigger than the bed: Frame it first.
- **Save G-code** writes the job as a file named Project_-_Device_-_Time.gcode. With Current position, the file starts from wherever the head is when it runs.
- **Ruida** controllers: enter the controller's IP address; Start sends the job over the network, and Save G-code writes an .rd file for a USB stick.
