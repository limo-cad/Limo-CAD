# Native center annotations

The Drawing workspace's existing **More dimensions** menu offers **Center Mark** and **Centerline**. This native Centerline increment supports two circular centers in one view. The sidebar names that method explicitly. Centerline between straight edges, automatic symmetry axes, and bolt-circle authoring remain separate parity work; their saved records are preserved.

Center targets require complete projected circles and respect the view's hidden-line setting. Coincident centers follow the existing React visibility/radius preference. New references retain the exact body, occurrence, edge key, topology signature, and projected model diagnostics. A first center is disposable; switching owner, revision, view, or projection retires it. A pair of translated occurrences of the same source edge is valid.

Select a saved center stroke to edit **Extension (paper mm)**, reset, or delete it. Selected endpoint grips preview extension along the outward direction and commit through the existing history transaction on release. Cancel restores the saved document. Native painting and export use the same pure center extent helpers; broken references keep the existing selectable `!` marker. Form edits retain loaded associations and other document metadata.

## Focused verification

Set `LIMO_CAD_NATIVE_CENTERS_ONLY=1` for the existing `xtask test-mcp native-drawing-authoring` command, retaining its normal disposable blank-session arguments. This uses the established real OCCT rectangle/three-boss fixture and its 24 saved annotation variants. It performs published-control creation in both centerline orders, duplicate/cancel/reset checks, extension edit/delete, exact Undo/Redo, and archive preservation. It retains nine new target/created/edited PNGs, exact before/after models, and the actual projection.

The focused path sends no OS mouse or keyboard input. The fixture's report explicitly leaves physical center picking, extension-grip dragging, and Linux/macOS center pixels unproven. Original PNGs still require visual review.

For real Linux input, `xtask test-mcp native-centers-platform --desktop-input --server <native-host> --out <fresh-absolute-directory>` owns a fresh host and verifies its private Xvfb display before launch. It runs the same control checks, then actual XTEST center picks and extension-grip drags for a mark and both circle-pair orders. Duplicate staging, held Escape, measured paper-space extension, exact Undo/Redo/delete and archive checks retain helper receipts and nine additional captures. This path is for disposable CI desktops; its results and original images must pass before claiming physical-input coverage. It does not establish Windows/macOS, Wayland or monitor-transition behavior.

Automatic view captions now clear the actual center strokes and rings, including their width. Native paint and export use the same paper-space clearance helper; neighboring views and sheets without center annotations keep their existing caption placement.

Native unit regressions live in `drawing_authoring/center/{tests,history_tests}.rs` and `drawing_annotations/center_tests.rs`. The default native desktop build includes this controller; `cargo check --locked --manifest-path desktop/Cargo.toml --all-targets` also compiles its regressions.
