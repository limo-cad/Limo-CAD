# Associative sketch Projection

While editing a sketch, open **Edit → Project**. Choose a curve from an earlier
finished sketch to preview its projection, then choose **Confirm** to add the
read-only reference. **Cancel** or Escape clears the preview without changing
the sketch or its Undo history. Use **References** to inspect or
remove an association. Undo and Redo operate on a whole projection batch.

References follow the source feature and entity IDs when their geometry changes.
They use the current source and target plane bases. Source lines, arcs, circles,
and fit-point splines are supported; circular curves require parallel planes.
Splines use the existing sampled profile representation, bounded to 2–250 fit
points and 999 segments per projected curve. Solid-body edges and
oblique circular-to-ellipse conversion are outside this first implementation.

Explicitly projected curves can form a closed profile by themselves, close a
region together with drawn geometry, and form holes inside another profile.
Automatic support-face boundaries still cannot form a selectable profile alone.
Both participate in the existing external reference constraints and add no
authored sketch entities or independent DOF.
Remove constraints using a reference before removing it. Deleting a source leaves
an identifiable broken reference for repair; it does not silently reuse cached
geometry or bind another curve. A broken reference blocks that sketch's downstream
profile catalog until the source is restored or the reference is removed.
Deleting an entire source feature is rejected while any sketch still projects it;
remove those associations first so replay cannot retain a stale downstream body.

The picker is scoped to the current document, sketch, engine revision and picker
generation. Changing the document or sketch retires its choices. MCP uses the same
commands: `sketch_projection_sources`, `sketch_project`, `sketch_projections`, and
`sketch_projection_remove`. Source discovery returns at most 512 choices and each
target can retain at most 128 references. Source display names are shortened to
80 Unicode characters; owning feature/entity IDs remain visible and authoritative.

Project model schema 17 stores source identities and the ID high-water mark. It
reads older projects; older applications reject the new schema rather than
discarding associations. The native composite snapshot remains CBOR v1.

The shared project-format foundation was merged in PR #516. The
[native evidence and provenance](media/sketch-projection/README.md) record the
qualified rendering, Linux keyboard traversal and minimum-window locale cases.
OS pointer delivery, other-platform OS input and whole-app layout/performance
remain unqualified. The broad UI/MCP survey remains paused.
