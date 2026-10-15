# Sketch dimension interaction requirements

These are required native sketch behaviors, including the cursor-following
behavior requested by Jack. They are acceptance requirements for changes to
the dimension tool.

## Immediate selection and value entry

Selecting the first measurable object must immediately show its dimension
leader and a focused inline value field. The current value must be selected
in full so typing replaces it. A line initially previews its length. Selecting
a second parallel line or straight support-face edge must refine the preview
to spacing; selecting a nonparallel edge must refine it to an angle.

## Required cursor-following leaders

**After the first measurable object is selected, moving the cursor within the
sketch canvas must move the dimension leader and its inline value field.**
A placement click must not be required to make the leader follow the cursor.
Movement must respect the dimension's geometry:

| Measurement | Allowed leader movement |
| --- | --- |
| Line length, including diagonal lines | Perpendicular to the measured line; movement along the line must not shift the leader along that line. |
| Spacing between parallel sketch or support-face edges | Along the parallel edges, perpendicular to the measured separation. |
| Angle | Change the arc radius while retaining the measured angle and placing the value on its bisector. |
| Radius or diameter | Follow the cursor with the leader attached to the measured curve. |

Cursor movement must preserve the typed value or expression, caret/text
selection and keyboard focus. It must not change geometry, create constraints
or add Undo entries. Choosing another measurement object must update the
preview and select the new value for replacement.

## Commit, cancellation and acceptance

Enter must commit the displayed value and leader position in one undoable
operation, including when reusing an existing driving dimension. Escape must
cancel the preview. Enter must also accept the displayed current value without
requiring it to be retyped or changed. An optional canvas placement click must
return focus to the value field, preserving its value, so Enter still commits.
Existing read-only reference dimensions may be
repositioned without changing their measured value.

Manual acceptance must cover horizontal, vertical and diagonal line lengths,
parallel sketch/support edges, angles, radial dimensions, typing while moving
the cursor, existing dimensions, Enter, Escape, Undo/Redo and save/reopen.
Automated checks support this requirement; a maintainer must manually confirm
the experience and manually promote the draft PR. No agent approval.
