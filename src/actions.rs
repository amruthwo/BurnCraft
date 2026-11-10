//! Every command in BurnCraft, its label, icon, group and default shortcuts.

use crate::icons::Icon;

macro_rules! actions {
    ($( $id:ident : $label:expr, $icon:ident, $group:expr, $tip:expr, [$($lb:expr),*], [$($cad:expr),*] );* $(;)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum Action { $($id),* }
        impl Action {
            pub const ALL: &'static [Action] = &[$(Action::$id),*];
            pub fn id(self) -> &'static str { match self { $(Action::$id => stringify!($id)),* } }
            pub fn label(self) -> &'static str { match self { $(Action::$id => $label),* } }
            pub fn icon(self) -> Icon { match self { $(Action::$id => Icon::$icon),* } }
            pub fn group(self) -> &'static str { match self { $(Action::$id => $group),* } }
            pub fn tip(self) -> &'static str { match self { $(Action::$id => $tip),* } }
            /// LightBurn-style default shortcuts (as shown in LightBurn's menus).
            pub fn default_lightburn(self) -> &'static [&'static str] { match self { $(Action::$id => &[$($lb),*]),* } }
            /// CAD-style (Fusion 360 / Onshape / SolidWorks) single-letter shortcuts.
            pub fn default_cad(self) -> &'static [&'static str] { match self { $(Action::$id => &[$($cad),*]),* } }
            pub fn from_id(s: &str) -> Option<Action> { Self::ALL.iter().copied().find(|a| a.id() == s) }
        }
    };
}

actions! {
    New: "New", New, "File", "Start a new, empty project", ["Ctrl+N"], ["Ctrl+N"];
    Open: "Open", Open, "File", "Open a BurnCraft or LightBurn project (.lbrn2, .lbrn)", ["Ctrl+O"], ["Ctrl+O"];
    Import: "Import", Import, "File", "Bring in an image (PNG, JPG, BMP...) or another project", ["Ctrl+I"], ["Ctrl+I"];
    Save: "Save", Save, "File", "Save the project (LightBurn-compatible .lbrn2)", ["Ctrl+S"], ["Ctrl+S"];
    SaveAs: "Save As", SaveAs, "File", "Save the project under a new name", ["Ctrl+Shift+S"], ["Ctrl+Shift+S"];
    ExportGcode: "Save G-code", Export, "File", "Write the laser job as a G-code file", ["Alt+X"], ["Alt+X"];
    Undo: "Undo", Undo, "Edit", "Undo the last change", ["Ctrl+Z"], ["Ctrl+Z"];
    Redo: "Redo", Redo, "Edit", "Redo what you undid", ["Ctrl+Shift+Z"], ["Ctrl+Y", "Ctrl+Shift+Z"];
    Copy: "Copy", Copy, "Edit", "Copy the selection", ["Ctrl+C"], ["Ctrl+C"];
    Cut: "Cut", Cut, "Edit", "Cut the selection", ["Ctrl+X"], ["Ctrl+X"];
    Paste: "Paste", Paste, "Edit", "Paste (offset a little)", ["Ctrl+V"], ["Ctrl+V"];
    PasteInPlace: "Paste in Place", Paste, "Edit", "Paste exactly where it was copied", ["Alt+V"], ["Ctrl+Shift+V"];
    Duplicate: "Duplicate", Duplicate, "Edit", "Duplicate the selection in place", ["Ctrl+D"], ["Ctrl+D"];
    Delete: "Delete", Delete, "Edit", "Delete the selection", ["Delete", "Backspace"], ["Delete", "Backspace"];
    SelectAll: "Select All", SelectAll, "Edit", "Select every shape", ["Ctrl+A"], ["Ctrl+A"];
    ToolSelect: "Select", Select, "Sketch", "Select and move shapes. Drag left-to-right to pick shapes fully inside, right-to-left to pick anything touched", ["Escape"], ["Escape"];
    ToolPan: "Pan", Pan, "Sketch", "Drag the view. Middle- or right-drag pans with any tool; Space + drag too", [], ["Shift+P"];
    ToolLine: "Line", Line, "Sketch", "Draw lines: click points, double-click or Enter to finish, click the first point to close", ["Ctrl+L"], ["L"];
    ToolRect: "Rectangle", Rect, "Sketch", "Draw a rectangle. Shift = square, Ctrl = from centre", ["Ctrl+R"], ["R"];
    ToolEllipse: "Ellipse", Ellipse, "Sketch", "Draw an ellipse. Shift = circle, Ctrl = from centre", ["Ctrl+E"], ["C"];
    ToolPolygon: "Polygon", Polygon, "Sketch", "Draw a regular polygon (sides in Properties)", [], ["Shift+G"];
    ToolText: "Text", Text, "Sketch", "Click to place text", ["Ctrl+T"], ["T"];
    ToolNodes: "Edit Nodes", Nodes, "Sketch", "Drag points of a path; double-click a point to delete it", ["Ctrl+`"], ["N"];
    ToolMeasure: "Measure", Measure, "Sketch", "Measure a distance and angle", [], ["M"];
    Group: "Group", Group, "Modify", "Group the selection", ["Ctrl+G"], ["Ctrl+G"];
    Ungroup: "Ungroup", Ungroup, "Modify", "Split a group into its parts", ["Ctrl+U"], ["Ctrl+Shift+G"];
    FlipH: "Flip Horizontal", FlipH, "Modify", "Mirror the selection left-right", ["Ctrl+Shift+H", "H"], ["Ctrl+Shift+H"];
    FlipV: "Flip Vertical", FlipV, "Modify", "Mirror the selection top-bottom", ["Ctrl+Shift+V", "V"], ["Ctrl+Alt+V"];
    RotateCW: "Rotate 90° CW", RotateCw, "Modify", "Rotate the selection clockwise", ["."], ["."];
    RotateCCW: "Rotate 90° CCW", RotateCcw, "Modify", "Rotate the selection counter-clockwise", [","], [","];
    ConvertToPath: "Convert to Path", ToPath, "Modify", "Turn rectangles, ellipses and text into editable paths", ["Ctrl+Shift+C"], ["Ctrl+Shift+C"];
    Offset: "Offset", Offset, "Modify", "Grow or shrink closed shapes by a distance", ["Alt+O"], ["O"];
    Weld: "Weld", Weld, "Modify", "Merge overlapping shapes into one outline", ["Ctrl+W"], ["Ctrl+W"];
    BoolUnion: "Union", Union, "Modify", "Keep the area of either shape (A + B)", ["Alt++"], ["Alt+U"];
    BoolSubtract: "Subtract", Subtract, "Modify", "Cut the last-selected shape out of the first (A − B)", ["Alt+-"], ["Alt+S"];
    BoolIntersect: "Intersect", Intersect, "Modify", "Keep only the overlap (A and B)", ["Alt+8"], ["Alt+I"];
    BreakApart: "Break Apart", BreakApart, "Modify", "Split a shape into its separate outlines", ["Alt+B"], ["Alt+B"];
    GridArray: "Grid Array", Array, "Modify", "Make rows and columns of copies", [], ["Shift+A"];
    AlignLeft: "Align Left", AlignLeft, "Arrange", "Line up left edges", ["Alt+Left"], ["Alt+Left"];
    AlignRight: "Align Right", AlignRight, "Arrange", "Line up right edges", ["Alt+Right"], ["Alt+Right"];
    AlignTop: "Align Top", AlignTop, "Arrange", "Line up top edges", ["Alt+Up"], ["Alt+Up"];
    AlignBottom: "Align Bottom", AlignBottom, "Arrange", "Line up bottom edges", ["Alt+Down"], ["Alt+Down"];
    AlignHCenter: "Align Centres (H)", AlignHCenter, "Arrange", "Line up horizontal centres", ["Alt+PgDown"], ["Alt+PgDown"];
    AlignVCenter: "Align Centres (V)", AlignVCenter, "Arrange", "Line up vertical centres", ["Alt+PgUp"], ["Alt+PgUp"];
    DistributeH: "Distribute H", DistributeH, "Arrange", "Even horizontal spacing", [], [];
    DistributeV: "Distribute V", DistributeV, "Arrange", "Even vertical spacing", [], [];
    PageCenter: "Centre on Bed", PageCenter, "Arrange", "Move the selection to the middle of the work area", ["P"], ["Shift+C"];
    PushFront: "Bring to Front", Front, "Arrange", "Draw (and burn) this first", ["Ctrl+PgUp"], ["Ctrl+PgUp"];
    PushBack: "Send to Back", Back, "Arrange", "Draw (and burn) this last", ["Ctrl+PgDown"], ["Ctrl+PgDown"];
    ZoomIn: "Zoom In", ZoomIn, "View", "Zoom in", ["Ctrl+="], ["Ctrl+="];
    ZoomOut: "Zoom Out", ZoomOut, "View", "Zoom out", ["Ctrl+-"], ["Ctrl+-"];
    ZoomPage: "Fit Bed", ZoomFit, "View", "Show the whole work area", ["Ctrl+0"], ["F"];
    ZoomSelection: "Fit Selection", ZoomSel, "View", "Zoom to the selection", ["Ctrl+Shift+A"], ["Shift+F"];
    ToggleFilled: "Filled View", Filled, "View", "Show closed shapes filled", [], [];
    ToggleGrid: "Grid", Grid, "View", "Show the grid", [], ["G"];
    Preview: "Preview", Preview, "Laser", "Show the laser path and time estimate", ["Alt+P"], ["Alt+P"];
    TraceImage: "Trace Image", Trace, "Image", "Turn an image into vector outlines", ["Alt+T"], ["Alt+T"];
    TraceMcp: "AI Outline (MCP)", Mcp, "Image", "Ask an MCP tool (for example an AI model) to find outlines in the image", [], [];
    ImageInvert: "Invert", Invert, "Image", "Swap light and dark", [], [];
    ImageReset: "Reset Adjust", Reset, "Image", "Reset brightness, contrast and gamma", [], [];
    LaserConnect: "Connect", Connect, "Laser", "Connect to a GRBL laser over USB/serial", [], [];
    LaserHome: "Home", Home, "Laser", "Send the laser to its home position ($H)", [], [];
    LaserFrame: "Frame", Frame, "Laser", "Trace the job outline with the laser off", [], [];
    LaserStart: "Start", Start, "Laser", "Start burning", ["Alt+Shift+S"], ["Alt+Shift+S"];
    LaserPause: "Pause", Pause, "Laser", "Pause / resume", [], [];
    LaserStop: "Stop", Stop, "Laser", "Stop now", ["Ctrl+Cancel"], [];
    ToolPen: "Pen / Curve", Pen, "Sketch", "Click for sharp points, click-and-drag for smooth curves. Enter or double-click finishes, click the first point to close", [], ["B"];
    ToolArc: "Arc", Arc, "Sketch", "Draw arcs: 3-point, centre, or tangent (choose in the options bar)", [], ["A"];
    ToolStar: "Star", Star, "Sketch", "Draw a star (points and depth in the options bar). Ctrl = from centre", [], [];
    ToolTrim: "Trim", Trim, "Sketch", "Click a piece of a line to cut it back to the nearest crossings", [], ["Shift+T"];
    ToolFillet: "Fillet / Chamfer", Fillet, "Sketch", "Click corners to round (fillet) or bevel (chamfer) them; set size in the options bar", [], ["Shift+R"];
    ToolDimension: "Dimension", Dimension, "Sketch", "Show and change lengths, arc and fillet radii, diameters, offsets and sizes", [], ["D"];
    ToolRelate: "Relations", Relate, "Sketch", "Pick nodes, lines or circles and tie them together: horizontal, vertical, parallel, perpendicular, tangent, equal, midpoint, coincident, concentric", [], ["Shift+J"];
    ToggleConstruction: "Construction", Construction, "Sketch", "Turn the selection into construction lines (for sketching, snaps and mirror lines; never burned) or back. With nothing selected, new drawing goes on the construction layer", [], ["Q"];
    JoinPaths: "Join / Close", Join, "Sketch", "Join open lines and curves whose ends touch (within 0.05 mm) into one continuous path, and close paths whose ends meet — like LightBurn's Auto-Join", ["Alt+J"], ["Alt+J"];
    ToolMirrorLine: "Mirror Line", MirrorLine, "Modify", "Mirror the selection about a line you click: any straight edge or a construction line", [], ["Shift+M"];
    EditArray: "Edit Array", Array, "Modify", "Change the number of copies, spacing or angle of the array the selected shape belongs to", [], [];
    PathPattern: "Pattern on Path", PathPattern, "Modify", "Copy the selection evenly along a curve (select the curve last)", [], [];
    AlignToPath: "Align to Path", AlignPath, "Modify", "Place the selected shapes along a curve in their left-to-right order (select the curve last)", [], [];
    Barcode: "Barcode / QR", Barcode, "Sketch", "Make QR codes, Data Matrix and 1D barcodes (Code 128, EAN-13, UPC-A, Code 39)", [], [];
    NodeSmooth: "Smooth Points", NodeSmooth, "Nodes", "Make the selected points smooth curves (S while editing points)", [], [];
    NodeCorner: "Corner Points", NodeCorner, "Nodes", "Make the selected points sharp corners (C while editing points)", [], [];
    NodeLine: "Make Lines", NodeLine, "Nodes", "Straighten segments between selected points (L while editing points)", [], [];
    NodeCurve: "Make Curves", NodeCurve, "Nodes", "Turn segments between selected points into bendable curves", [], [];
    NodeInsert: "Insert Points", NodeInsert, "Nodes", "Add a point in the middle of each selected segment (M while editing points)", [], [];
    NodeBreak: "Break at Points", NodeBreak, "Nodes", "Split the path at the selected points (B while editing points)", [], [];
    CircularArray: "Circular Array", CircArray, "Modify", "Copy the selection around a centre point", [], [];
    ToolWand: "Magic Wand", Wand, "Image", "Click an image to select similar colours. Shift adds, Alt subtracts", [], ["W"];
    ToolLasso: "Lasso", Lasso, "Image", "Draw around part of an image to select it; Magnetic snaps to edges. Shift adds, Alt subtracts", [], ["Shift+L"];
    SelectionToPath: "Selection to Outline", SelOutline, "Image", "Turn the image selection into a vector outline", [], [];
    ClearSelection: "Clear Selection", Reset, "Image", "Drop the image selection", [], [];
    PreviewPlay: "Play Preview", Start, "Laser", "Animate the laser path in the preview", [], [];
    InvertBed: "Invert Bed", Invert, "View", "Swap light and dark on the bed, to check designs for dark materials", [], [];
    SaveLayerDefaults: "Save Layer Defaults", Layers, "Laser", "Use this project's layer settings for every new project", [], [];
    VariableText: "Variable Text", VarText, "Laser", "Fill text from a .csv / .txt file, a serial number or the date, a new row each job", [], [];
    Camera: "Camera", Camera, "Laser", "Show the laser's camera picture on the bed, and line it up", [], [];
    Rotary: "Rotary", Rotary, "Laser", "Burn around cylinders with a roller or chuck rotary", [], [];
    Nest: "Nest", Nest, "Arrange", "Move and rotate the selected shapes to fit tightly on the work area or another shape", [], [];
    NewTab: "New Tab", NewTab, "File", "Open an empty project in a new tab", ["Ctrl+Shift+N"], ["Ctrl+Shift+N"];
    DuplicateTab: "Duplicate Tab", DuplicateTab, "File", "Copy this project into a new tab", [], [];
    CloseTab: "Close Tab", CloseTab, "File", "Close this project tab", ["Ctrl+F4"], ["Ctrl+F4"];
    CommandSearch: "Search Commands", Search, "App", "Find any command by name", ["Ctrl+K"], ["S", "Ctrl+K"];
    Shortcuts: "Keyboard Shortcuts", Keyboard, "App", "Change any shortcut", [], ["Ctrl+Alt+K"];
    Settings: "Settings", Settings, "App", "Theme, size, units, device", [], [];
    Notes: "Notes", Notes, "App", "Project notes", ["Ctrl+Alt+N"], ["Ctrl+Alt+N"];
    Help: "Help", Help, "App", "How to use BurnCraft: drawing, sketching, layers, burning, devices", ["F1"], ["F1"];
    WelcomeTour: "Welcome Tour", Tour, "App", "A short guided tour of the window (also in Settings)", [], [];
    Exit: "Exit", Exit, "File", "Close BurnCraft", ["Ctrl+Q"], ["Ctrl+Q"];
}
