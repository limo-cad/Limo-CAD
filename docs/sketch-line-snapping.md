# Sketch line snapping requirements

A line being drawn near 0°, 90°, 180° or 270° must preview and commit the
exact horizontal or vertical direction. This includes a free endpoint and
an endpoint attached to a finite sketch or support-face edge. When an edge
is acquired, use the exact intersection of that edge and the inferred axis;
retain both the alignment and edge relation after dimension changes.

Snapping must release promptly as the pointer leaves alignment. Capture is
limited to 3° on either side of an axis and seven logical screen pixels
perpendicular to it, at every zoom level. Both limits apply, including when
only the line length is locked. Do not extend a finite edge to create a snap.
Exact endpoint snaps retain priority. A typed angle remains exact, and
holding Ctrl temporarily suppresses inferred alignment and edge attachment.

Maintainer acceptance must cover drawing in both directions on both axes,
short and long lines, several zoom levels, deliberate shallow diagonals,
edge attachment, typed lengths and angles, Ctrl, dimension changes and
Undo/Redo. Automated checks do not replace manual confirmation in the app.
The maintainer must manually confirm the experience and manually promote
the draft PR. No agent approval.
