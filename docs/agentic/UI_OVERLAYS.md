# UI overlay and flyout invariant

Use this note whenever adding or changing a menu, popover, combobox list,
tooltip, context menu, or dialog in the independent browser interface.
The native desktop uses retained Bevy controls and has no DOM/native compositor.

## The failure we must not repeat

The File menu was an absolutely positioned child of the ribbon. The ribbon
used `overflow-hidden`, and the menu began exactly at the ribbon's bottom edge
(92 px in the standard shell; 122 px after the Drawing workspace row was
added). Clicking File correctly changed React state and created the menu DOM,
but every menu pixel and pointer target was clipped. Fullscreen and windowed
layouts made the symptom easy to misdiagnose as a native Bevy stacking issue.

`z-index` cannot repair this. A descendant cannot paint or receive pointer
events outside an ancestor's overflow clip, regardless of stacking order.

## Required implementation pattern

1. Portal every surface that can extend outside its shell container to
   `document.body`.
2. Position it with fixed viewport coordinates from the trigger's
   `getBoundingClientRect()`; clamp it to the visible window.
3. Recompute placement on open, resize, fullscreen changes, display-scale
   changes, and relevant scrolling.
4. Keep dismissal, focus return, keyboard navigation, and accessibility
   semantics connected to the trigger even though the surface moved in the
   DOM.

Do not work around the problem by removing a deliberate shell clip, adding an
extreme `z-index`, or hard-coding another shell-height offset.

## Flyouts that overflow their own surface

A flyout must be reachable beyond the menu's border box by pointer and keyboard.
`cargo xtask test-mcp contracts` checks real browser hit-testing, dismissal and
focus routing. The former native cut-out islands and their test hooks were
removed with the embedded desktop host. Native menu behavior is checked by the
owned-window fixtures documented under `xtask/platform`.

## Required regression

Test the windowed layout at minimum; also cover fullscreen when the shell
changes there. The test must:

1. Open the surface through its real trigger.
2. Choose an interior point beyond the trigger container's bottom or side.
3. Verify `document.elementFromPoint()` resolves to the surface or one of its
   controls.
4. Click that point and assert the intended action occurred.
5. Repeat after a window resize when placement depends on available space.

Checking only that React state changed, the node exists, or the element looks
correct in fullscreen does
not prove the windowed surface is painted and interactive.

## Responsive command-ribbon policy

This policy applies to Solid Modeling, Sketch, Drawing, Assembly, and CAM.

1. Keep every workflow group visible at ordinary desktop widths. In particular,
   a primary action such as **Select** must not be available only after
   horizontally scrolling the ribbon.
2. When the command strip loses space, move secondary direct buttons into that
   panel's existing flyout before hiding a panel or enabling horizontal scroll.
   Panels without a curated flyout must expose an equivalent generated menu so
   no command becomes unreachable.
3. Measure the usable strip after fixed chrome (the workspace switcher and, in
   Sketch, Finish Sketch). Do not rely on a global viewport breakpoint: restore
   direct buttons as soon as that measured space returns.
4. Horizontal scrolling is the final fallback, only after the localized panel
   labels and one primary action from each group cannot coexist. Center a
   one-command group over its panel label and use the same button width and
   label treatment across groups.
