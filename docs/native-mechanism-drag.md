# Native component dragging

The development Bevy host can drag a component connected to a grounded component
through a movable joint. A plain left-button drag uses the picked body-local
point and a plane parallel to the camera. Ctrl, Shift, Alt and Meta preserve the
existing modified selection gestures. A click remains a selection; movement
must exceed three logical pixels before it becomes a drag.

This uses the existing assembly document and `assembly_preview_mechanism_drag`
solver. Preview coordinates are coalesced while its worker is busy. Releasing
the pointer applies `assembly_apply_joint_motions` once through the existing
mutation/history path. Source solids are unchanged; Undo restores the complete
previous joint state. There is no additional assembly model or solver.

That history behavior is covered by scoped mechanism tests. The known attached
read/history issue described in [transition status](native-transition-status.md)
still prevents a claim of global Undo parity.

The grounded component, disconnected components and rigid-only joint paths
remain selection targets. Sketch support picking, feature/joint editing, motion
previews and motion studies retain priority. Cancellation restores the original
displayed poses under the original document receipt. Key presses, focus loss,
window lifecycle changes, camera navigation input, changed camera/canvas geometry, and changed document ownership
or revision invalidate a pending drag, including a final preview after mouse-up.

The original local implementation was recovered during the September 27 handoff
audit and reconciled with the current controller. The original worktree was
preserved. Feature-gated unit coverage checks point conversion, coalesced release,
camera/canvas invalidation, queued navigation cancellation, support-picker priority, post-release focus loss,
grounded/disabled/rigid rejection, shared solver preview, unchanged source solids,
and exact Undo/Redo. MCP coverage checks read-only preview and the shared atomic
commit command. A `cargo xtask test-mcp native-mechanism` fixture is included for
rendered-canvas drag, consecutive poses, limits, grounded rejection and history.

`native-mechanism-platform --desktop-input` now sets `LIMO_CAD_NATIVE_MECHANISM_INPUT`
so the drag uses the owned XTEST helper instead of a synthetic viewport gesture.
That job has not yet been accepted as a live pass. Pixels, drag responsiveness
on larger mechanisms, platform input behavior and save/reopen behavior still
need that run. The solver uses its existing
12-iteration preview budget and may return a valid constrained pose without
reaching the pointer target. This change does not make the Bevy host the release
interface; the release build remains React.
