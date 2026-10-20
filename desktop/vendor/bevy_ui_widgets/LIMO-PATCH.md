# Preserve the dialog press origin for large first movements

This is the published `bevy_ui_widgets 0.20.0` source from Bevy commit
`b3bdd75becaffe9a16677cf5bae5bd6d198f052b`. The normalized registry manifest and
original MIT/Apache licenses are retained. Original registry archive SHA256:
`6442cd402df6a54f0e51ed8c1bd004c8f9c6715d0f8045e33c68ea9aebb1ce16`.

The only upstream source change is in `src/dialog.rs`: initialize
`DialogDragState` on `PointerPress` rather than `PointerDragStart`.
`PointerDragStart.pointer.position` is the location of the first movement,
whereas `PointerDrag.distance` is measured from the original press. Mixing
those origins made a first movement from x=575 to x=275 clamp -300 to -259;
a subsequent move from x=275 to x=15 clamped -260 to +1. Large movements could
therefore stop early or move in the wrong direction.

Bevy still owns picking, press/drag/release, dialog translation and its edge
clamp. There is no application drag recognizer, replay queue or extra capture
state. Limo's existing whole-dialog bounds and resize adapter are unchanged.
The patch is selected only by the desktop workspace's `[patch.crates-io]`.
Remove it when the pinned upstream dialog component retains the press origin;
recheck the regression before refreshing the vendored source.

Regression: `large_first_title_motion_retains_press_origin_and_repeated_edges` in
`desktop/src/native_viewport/interface_shell/dialogs/tests.rs`, through actual
Bevy window input, picking and dialog handling at 100% and 125% UI scale.
Native diagnostic evidence is retained separately from final live acceptance.
