# Central tool-library storage

Implemented September 12, 2026. Desktop storage is independent of saved project
tools. Executable tool numbers/names come directly from project-library snapshots.

## User workflow

Open **Settings → CAM → Central tool library**. The same panel is available
from **Tool Library → Storage settings…**. It displays the complete current
file path, whether it is the default or a custom location, and the tool count.

**Choose folder…** opens the native directory picker, then previews the choice
without changing anything. Review one of these actions:

- **Use this library**: select an existing `cam-tool-library.json`. No merge,
  copy or overwrite occurs.
- **Use empty folder**: select a folder with no library file. The first central
  edit creates an empty/new collection there.
- **Copy current library here**: available only when the destination has no
  library. Copy the current collection, keep the original, then select the new
  location. A destination created by another process is never overwritten.
- **Use default location…**: preview and return to the per-user application
  configuration folder. The previous custom file is retained.

Canceling either the picker or the review does not change the preference.
Existing project tools remain self-contained snapshots: selecting another
central library does not modify them, regenerate paths or change a posted
controller tool call. Imports and publishing remain explicit operations.
Different collections are not automatically reconciled by tool ID; inspect
the source and destination before explicitly replacing a snapshot.

The default is the original per-user `cam-tool-library.json` inside the app's
platform config folder. The panel shows the exact path; nothing is migrated
on upgrade. A small `cam-library-storage.json` preference stays in that config
folder and records the chosen directory. The preference is per device/user,
not embedded in `.limo` projects or the central collection.

**Settings → CAM → Custom posts** manages a separate `cam-posts` directory
under the same OS-user configuration root. It does not follow a relocated
central library. See [private posts](CAM_POSTS.md) for import/profile behavior.

## Failure and concurrency behavior

`desktop/src/cam_library.rs` owns filesystem operations. Native commands run
on background blocking workers so a slow mounted volume does not block the
render/UI thread. Reads and writes validate tool geometry, unique internal
IDs and the allocation counter, with a 16 MB library-size bound.
Legacy tool fields receive Rust's defaults in memory before reaching the UI;
reading alone never rewrites the library. Additional collection/tool metadata
is preserved, and conflict checks still use the original on-disk bytes.

Custom locations must already exist. An unavailable external/shared folder
stops central reads and saves; the app does not recreate the mount path or
silently switch to a local replacement. Project tools remain available.
Choose another folder or restore the mount to recover.

Each editor retains the loaded file path and a content revision. Saving checks
both under an in-process mutex and a cooperating-writer lock file. A changed
file or location rejects the old edit; reload before retrying. The frontend
edits a copy and propagates save failures rather than updating the visible
library optimistically. Location changes notify other windows in the same app
process and discard their stale central-library edit drafts.
Unsaved project-tool drafts remain intact when only central storage changes.

Writes use a synced temporary file in the same directory, followed by atomic
replacement. Initial copies use an atomic no-replace publication; filesystems
without that capability return an error instead of overwriting. If preference
saving fails after a successful copy, the copy remains and the old preference
is retained. A crashed writer can leave `.cam-tool-library.lock`; the app never
removes another writer's lock automatically. Confirm no writer is running
before recovering such a lock.

This is file-based storage, not a collaborative database. Cloud-sync conflict
resolution, distributed transactions, backups and malicious/non-cooperating
writers are outside this implementation.

## Verification

Native tests cover legacy/default reads, custom persistence/reset, copy/use
semantics, no-overwrite behavior, stale file/location edits, corrupt/busy/offline
folders and malformed tools. Browser tests exercise the actual settings and
library shortcut with isolated storage IPC: preview/cancel, copy/use/reset,
failure feedback, fixed footer/scrolling content, stale-editor errors and no
changes to project tools. The test suite does not modify the user's library.
