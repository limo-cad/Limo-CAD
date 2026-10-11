# macOS shader-cold startup test

A fresh settings profile alone does not establish cold shader compilation.
Metal also caches shader libraries and compiled GPU functions outside the app.
Use this separate test copy when evaluating first-use shader compilation:

```sh
cargo xtask prepare-cold-start \
  --app "/absolute/path/to/Limo CAD.app" \
  --out "/absolute/existing/parent/new-cold-test" \
  --verify
```

The input must be a signed, thin 64-bit macOS package. Use an optimized release
when comparing release startup. The output directory must not already exist;
preparation never replaces an earlier test, its settings, or its recovery data.
The command requires the Apple command-line developer tools and does not rebuild
or launch CAD.

Open `Limo CAD Cold Start.app` in Finder, leaving it in the prepared folder.
Its settings and session registry start in separate empty directories. Every
top-level launch creates a new private Metal cache before GPU initialization;
child processes inherit only the cache of their current launch. A stale inherited
cache from a different parent does not turn a new launch into a warm launch.
Existing app settings, documents, recovery files, and shader caches are preserved.
The test copy does not register the normal app's URL handlers.

The test-only helper uses Metal's private `MTLSetShaderCachePath` interface and
checks `MTLGetShaderCachePath`, `MTLGetModulesCachePath`, and
`MTLGetGPUArchiverCachePath`. It exits if those paths cannot be verified inside
the new cache. This interface is not a portable production shader-cache feature;
it was verified locally on macOS 26.6.2 with an Apple M4. The normal package
builder and release renderer do not include the helper.

`--verify` launches a small native GPU probe twice through Launch Services,
checks that the two launches start with different empty caches, and verifies
shader execution and newly written private library/function cache data. It does
not open CAD, use the user's profile, or edit a model. Each launch saves a
`cache-setup-<pid>.json` before GPU initialization. `cold-test-ready.json` records
the input/output executable hashes, unchanged compiled-section hashes, resource
checks, helper hash, isolated paths, and GPU verification result. Re-signing
changes the executable's signature; the loaded code, constants, and embedded
shaders are compared separately to the source release.

This is a **shader-cold** test on the host Mac. It does not empty OS or filesystem
memory caches, reset background load, simulate another GPU, or reproduce a clean
macOS installation. Do not describe its timings as measurements from a new
physical machine. Because every launch is shader-cold, use the original package
when measuring normal warm startup or persistent-cache reuse.
