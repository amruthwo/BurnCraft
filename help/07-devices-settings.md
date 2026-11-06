# Devices and settings

## Setting up your laser

Open **Settings** (gear icon, top right):

- **Controller**: G-code controllers (GRBL, GRBL-M3, GRBL-LPC, FluidNC, Smoothieware, Marlin, RepRap/Duet, LinuxCNC, custom G-code) and Ruida DSP controllers. Trocen, TopWisdom and galvo controllers (EZCad, BSL) are listed but not supported yet.
- **Work area**, **Max S value** (GRBL $30, usually 1000; Marlin 255), travel speed and dynamic power (M4).
- **Import LightBurn device (.lbdev)**: reads the laser's name, work area and controller from a LightBurn device profile. Opening or dropping an .lbdev file does the same.

## Other settings

- Theme, interface size, units (mm or inches), grid spacing.
- Ribbon labels and the compact two-row ribbon.
- Cut optimization (see Layers and burning).
- AI provider for AI Outline.
- **Replay Welcome Tour** shows the tour again.

## Keyboard shortcuts

**Keyboard Shortcuts** (View tab) lets you change any shortcut, choose CAD-style or LightBurn-style defaults, and export or import your set. Conflicts are shown as you type.

## Files

- BurnCraft saves LightBurn-compatible **.lbrn2** projects, and opens .lbrn2 and older .lbrn files.
- Sketch relations, editable offsets and arrays are stored as extra attributes that LightBurn ignores.
- Settings and your layer defaults are kept in your user settings folder (%APPDATA%\BurnCraft on Windows).
