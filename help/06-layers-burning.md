# Layers and burning

## Layers

Each palette colour is a layer with its own settings (Layers panel):

- **Mode**: Line (follow outlines), Fill (scan back and forth), Fill + Line, Offset Fill, Image.
- **Speed** (mm/s), **Max / Min power** (%), **Passes**, **Line interval** for fills, scan angle, cross-hatch, bi-directional.
- **Fill shapes individually** finishes each shape before moving on — much faster when shapes are far apart.
- **Constant Power Mode** burns the layer at constant power (M3) instead of dynamic power (M4).
- **Burn** and **Show** switch a layer's output and visibility. T1 and T2 are tool layers and never burn (T2 holds construction lines).
- **Save Layer Defaults** keeps your settings for new projects.

## Preview

**Preview** draws the job as the laser will run it, with cut distance, travel and estimated time. **Play Preview** animates it. Shapes outside the bed are outlined in red and left out of the job.

## Cut order

Layers burn in the order of the Layers list. Within a layer BurnCraft uses LightBurn-compatible defaults: nearest shape next, inner shapes before the shapes around them, and open lines from whichever end is closer. Change these in Settings > Cut optimization.

## Running a job

- **G-code lasers**: choose the port and speed in the Laser panel and press **Connect**. **Frame** traces the job outline, **Start** burns, **Pause** and **Stop** control it. The Laser panel shows elapsed and remaining time.
- **Save G-code** writes the job as a file named Project_-_Device_-_Time.gcode.
- **Ruida** controllers: enter the controller's IP address; Start sends the job over the network, and Save G-code writes an .rd file for a USB stick.
