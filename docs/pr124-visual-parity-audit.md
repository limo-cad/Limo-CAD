# PR124 visual parity and sketch snapping audit

Reference: the supplied noBS CAD app from `cam-pr180-followups`, source commit
`391a025c2ec7047bc7c369e972cd7a4799f604b2`.
Stacked base: PR124 (`feat/bevy-interface`), commit
`b36fabeaee3c4a745ff3c8b64686e85217086814`.

## Requested fixes

| Area | Finding in PR124 | Result |
| --- | --- | --- |
| Workspace switcher and menu | Generic outline icons and different captions | Port the reference's original shaded cube and milling-machine artwork in both appearances; use its FileText icon for Drawing, compact/wide captions, current-workspace check, and sketch badge. Drawing and Manufacture are disabled during a sketch. |
| Sketch Palette / orientation dial | Both panels occupy the top-right viewport | Retire the dial and axis labels during sketch editing, matching the reference; restore them when sketch editing ends. Navigation controls remain available. |
| Drawing acquisition | Only Line presents a snap marker; other tools use fixed engine defaults | Share cursor acquisition across every creation tool, including polygon center placement. Origin and point capture use 14 logical pixels; grid capture uses 7. The grid interval follows the visible 1–2–5 lattice at the current zoom. |
| Sketch tool groups | Dimension and Select are combined into other groups; dividers are missing | Restore DRAW, EDIT, DIMENSION, REPEAT, CONSTRAIN and SELECT, with vertical dividers and centered inline captions/chevrons. Compact layouts retain group menus for secondary commands. |
| Additional chrome | Palette and browser typography, borders, rows and active-sketch presentation differ | Match palette width/spacing/title/footer, Flat View icon, browser heading/divider/units badge, plane labels, active sketch emphasis and automatic folder expansion, plus history caption treatment. |
| Feature panels used in a combined workflow | Missing header icons, larger titles, plain field labels and full-width footer buttons | Use the reference's Lucide header artwork, 12 px titles, tracked 10 px uppercase field labels, directional header wash, header/footer dividers, compact right-aligned Cancel/OK buttons, regular Cancel text, 40% disabled submit opacity and 20 px right inset. |

## Launch diagnosis

The two initial temporary-app reports at 00:05 and 00:06 on October 6 were a
missing `LC_RPATH` for bundled OCCT libraries and an AppKit registration abort
during a restricted GUI launch. The desktop executable now links its relative
`@executable_path/../Frameworks` search path directly; the package step retains
its existing verification and repair as well. Test GUI launches use an isolated
profile with normal WindowServer access.

The local test runner also encountered missing-library and invalid-signature
failures while using rewritten staging libraries. Its corrected configuration
loads the signed bundled libraries. The delivered app verifies both its own
signature and the bundled library signatures before startup checks.

## Interaction safeguards

Preview and click use the same acquisition query. The ordered creation command
carries its viewport snap distances so another caller cannot alter those distances
between preview and commit. The host scopes and restores this runtime context;
it does not persist viewport zoom in the project or invalidate geometry on a
preview. Calls without viewport context retain their existing engine defaults.

Typed circles and rectangles, and line inference, keep the raw direction hint
used by the engine preview. A nearby snap target therefore cannot make a typed
circle jump when clicked. Snap off preserves cursor coordinates. Ctrl suppresses
relational snapping while preserving the reference's grid behavior. Creation
remains one undo step.

## Validation

- Shared vector audit: all 108 declared/available icons pass, including provenance
  and external-reference/executable-content checks.
- Regression coverage exercises all 13 primitive creation variants against
  origin, grid and existing-point acquisition, preview immutability, committed
  geometry, Snap/Ctrl switches and undo. Polygon uses the same acquisition path.
- Additional coverage checks scoped-context restoration after success/error,
  midpoint-line grid spacing, typed-circle preview/commit agreement, dial
  retirement/restoration, compact/wide workspace layouts, shaped group captions
  and active-sketch folder expansion without overriding a later manual collapse.
- Live native-interface checks verify rectangle and circle origin/grid snapping,
  Snap off, preview markers, and undo using an isolated test document.

## Combined workflow

The isolated native document follows this sequence through the actual retained
controls and viewport handlers:

1. Snap a 30 × 20 mm rectangle to the origin and grid, then add a typed Ø6 mm
   circle at (10, 10) in the same sketch.
2. Finish the sketch, pick its material region and extrude 8 mm. Check a single
   30 × 20 × 8 mm body with the circular hole and no scene errors.
3. Pick an exterior edge, apply a 1 mm fillet, undo it and redo it. Compare the
   resulting geometry summaries exactly.
4. Edit Extrude1 from history to 10 mm and check the downstream fillet replay.
5. Save, close and reopen the project, comparing its geometry and ordered
   Sketch1 / Extrude1 / Fillet1 history.
6. Reopen Sketch1, edit the circle's existing dimension to Ø8 mm and finish.
   Check the updated hole and replayed extrusion/fillet, then save again.
7. Switch through Drawing and Manufacture and return to Solid Modeling,
   checking that the model remains intact.

Feature-panel GPU captures use the production panel builder and typed forms
against this saved model. Extrude, Solid Fillet and Hole captures were reviewed
for icon/label alignment, field readability, borders and footer layout. The Mac
was locked during this follow-up: these captures validate the production panels,
while a fresh side-by-side OS-window comparison remains unavailable until unlock.
The combined interaction checks do not claim GPU presentation while locked.

To reproduce without opening an OS window, build
`bevy-ui-lab` with `dev-ui-lab`, set `LIMO_CAD_FEATURE_LAB_MODEL` to the JSON
string returned by `cad_project_model`, and set `LIMO_CAD_FEATURE_LAB` to
`extrude`, `fillet` or `hole`. Pass the output PNG path as the first argument.

The reference uses WebKit text while the native interface uses Bevy text shaping;
minor font-metric differences remain. This audit covers the requested chrome
and sketch interactions rather than certifying pixel-identical rendering of
every native workspace.
