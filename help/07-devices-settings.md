# Devices and settings

## Setting up your laser

Open **Settings** (gear icon, top right):

- **Controller**: G-code controllers (GRBL, GRBL-M3, GRBL-LPC, FluidNC, Smoothieware, Marlin, RepRap/Duet, LinuxCNC, custom G-code), K40 lasers with the M2 Nano board, galvo lasers with an EZCad2 / JCZ board, and Ruida DSP controllers. Trocen, TopWisdom, EZCad3 and BSL galvo controllers are listed but not supported yet.
- **Work area**, **Max S value** (GRBL $30, usually 1000; Marlin 255), travel speed and dynamic power (M4).
- **Import LightBurn device (.lbdev)**: reads the laser's name, work area and controller from a LightBurn device profile. Opening or dropping an .lbdev file does the same.

## K40 lasers (M2 Nano board)

Choose **K40 (Lihuiyu M2 Nano)** as the controller and pick your **Board** (printed on the board, usually M2). The work area switches to the K40's usual 325 × 220 mm.

- BurnCraft talks to the board over USB directly; K40 Whisperer and LaserDRW don't need to be running (close them first).
- **Home** before the first job: jobs start from the back-left corner. **Frame** traces the job's outline with the laser off. **Stop** cancels a job.
- The board only switches the laser on and off: set the power with the knob on the machine. Each layer's speed is used.
- **Unlock rail** (Laser panel) lets you push the head by hand.
- **Save G-code** writes an **.egv** file, the format K40 Whisperer and LaserDRW use.
- **Linux**: the packages let your user use the laser. Installed from the tarball? Copy `70-burncraft-k40.rules` to `/etc/udev/rules.d/`, run `sudo udevadm control --reload`, and plug the laser in again.
- **Windows**: install the **WinUSB** driver for the laser once, with Zadig (choose the "USB-EPP/I2C... CH341A" device, USB ID 1A86 5512). This replaces the LaserDRW driver.

This support comes from the open-source K40 Whisperer and hasn't been tested on every board; try a slow, low-power job first.

## Galvo lasers (EZCad2 / JCZ)

Choose **EZCad2 / JCZ** as the controller for fiber, UV and CO2 galvo markers with a JCZ "LMC" board (the kind EZCad2 drives; USB ID 9588:9899). Close EZCad2 first.

- **Laser source**: Fiber, UV or CO2 galvo. Power and frequency are sent the way each kind expects.
- **Lens field**: the size marked on the lens (e.g. 110 mm). The work area becomes that square, with the field's middle in the middle.
- **Correction file**: pick the machine's `.cor` file from its EZCad folder (it corrects the lens's distortion). The lens field is read from the file too.
- **Orientation**: if Frame traces the outline turned or mirrored, change Flip X, Flip Y or Swap X/Y.
- **Frame** traces the job's outline with the red pointer for about 20 seconds (Stop ends it sooner). **Start** marks the job. **Stop** stops at once.
- Each layer's **speed**, **power** and **frequency** (in Hz, e.g. 30000 for 30 kHz) are used. Travel speed, laser on/off/corner delays and MOPA pulse width are in Settings.
- **Windows**: install the WinUSB driver for the board once, with Zadig. **Linux**: the packages allow your user to use it (tarball users copy `70-burncraft-k40.rules` as described for the K40).

This support follows the open-source MeerK40t project's galvo driver and **hasn't been tested on a real machine yet**: start with a small test at low power, and keep a hand near the emergency stop.

You can also **import EZCad2 projects (.ezd)**: shapes, text, pictures and hatches come in, and each EZCad pen becomes a layer with its speed, power, passes and frequency. Hatched areas become Fill layers. Exporting to .ezd isn't possible: the format is undocumented.

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
