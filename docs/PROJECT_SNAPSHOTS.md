# Composite project snapshots

`ProjectSnapshot` is a CBOR v1 envelope for exact model JSON text plus named
opaque domain payloads. Its CDDL is [project-snapshot.cddl](project-snapshot.cddl).
The model stays in its existing schema; the envelope does not silently rewrite
or reinterpret geometry. Domain entries use CBOR byte strings and safe names
under `intent/`. Unknown domain bytes are retained.

`ProjectArchive::snapshot` captures the model and all live intent entries;
`with_snapshot` prepares a separate candidate containing that pair. The source
archive remains unchanged on success or failure. Images, recipes and other
ancillary entries remain attached to the archive. Original ZIP bytes and staged
intent buffers are shared immutably between archive candidates, avoiding copies
of those buffers during candidate creation.

The snapshot validates envelope/header shape, unique safe names, count and byte
bounds. It does not validate CAD or domain semantics. A host must prepare all
known domains and commit their state together under its existing document owner
and revision fence. An unsupported domain must not be silently edited or lost.

Limits: 64 live intents, 256 MiB encoded snapshot, recursion depth 16. Capture
bounds aggregate model and intent bytes before reading another payload; encoded
field overhead also counts against the writer limit. Container staging and
archive-size limits remain applicable and preparation can fail without mutation.

This is the shared snapshot boundary, not completed native history or recovery
integration. Native mutation receipts, Undo/Redo tickets, Save/Open and session
publication still need to adopt it. Results, worker identities and UI state do
not belong in persistent domain intent. Live native qualification remains open.
