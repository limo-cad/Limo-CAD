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

Native callers capture the paired manager state under the existing document
owner and revision fences. Native history, tab eviction/restore, `.limo`
Save/Open and session publication use complete snapshots. Active sketch editing
state, derived results, worker identities and UI state are excluded.

## Authoritative manager integration

`SketchManager` owns domain intent alongside its model. Its complete snapshot
export retains every known or unknown domain payload. Ordinary model-only export
rejects a document containing domain intent, preventing older history/save paths
from silently discarding it. Documents without domain intent keep their existing
model export behavior.

`prepare_load_project_snapshot` validates known thermal payloads before preparing
the existing model replay transaction. Only the matching kernel commit replaces
both model and domain intent. Cancellation and stale commits retain the live
pair. Explicit New or a legacy project load creates an empty domain set in the
candidate and clears the old set only on commit.

`replace_project_intents` validates the complete replacement before mutation,
returns false for a no-op, and rejects during an active sketch or pending solid
or project transaction. Snapshot capture also rejects pending solid work.
Unknown payloads remain opaque; a caller editing one domain must preserve all
other entries unless their removal was explicitly requested. A document with
any domain intent is not blank for automatic script playback.

## Native and MCP integration

Native mutation and history adapters keep the complete snapshot inside the
document owner/revision fence. File Save stages model and intent together in a
separate archive candidate; Open and cold-tab restore replace the pair only
after a successful owned transaction. Failed or stale preparation retains the
previous document.

Native publication writes a typed capsule and commit record, then acknowledges
the matching receipt in its heartbeat. MCP attachment reads that complete
capsule and verifies the document route and receipt. The public
`cad_load_project_snapshot` tool supports complete loads.

Read-only model projections support geometry exports and print metadata without
copying domain payloads. They must not replace complete snapshots for Save,
history or publication. Legacy model-only export still rejects domain-bearing
documents; `cad_script` and script export retain that restriction until portable
replay support is integrated.

This integration adds no visible domain controls. Unknown domain bytes remain
opaque. Publication commits provide process-failure consistency; power-loss
durability depends on the filesystem.
