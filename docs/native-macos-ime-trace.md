# Bounded native macOS IME diagnostic

The existing `Native desktop host tests` workflow accepts `ime-native-macos=true`
and `ime-profile-diagnosis=true` together. This builds the live host with
`dev-native-ime-trace` and runs the same stock AppKit prerequisite and native
Japanese input scenario. It does not add retries, change delays or keys, or
force additional focus/source/IME state transitions.

Both the compile feature and the `LIMO_CAD_NATIVE_IME_TRACE=1` runtime opt-in are
required. The runtime also requires the existing Japanese IME opt-in, macOS,
the named repository, and a disposable GitHub-hosted Actions run. Without those
guards the feature build disables Bevy's LogPlugin. Ordinary `dev-bevy-host`
builds do not include this logger.

The `native-input-macos` artifact's Japanese scenario directory retains:

- `ime-trace.json`: requested diagnostic scope and byte limit.
- `host-stderr.log`: ordinary host stderr plus the bounded trace. The trace
  begins with `LIMO_CAD_NATIVE_IME_TRACE enabled`. An absent marker does not prove
  that Winit received no callbacks.
- `host-stdout.jsonl`: the owned fixture's MCP replies, including initialization
  metadata, alongside the existing field snapshots and native captures.

The formatter allows only Winit 0.30.13's macOS `TraceGuard` event callsites and
the `winit::Window::set_ime_allowed` span. These report static callback names,
module paths, and the setter's boolean. The trace is capped at 1 MiB including
its enabled/truncated markers; ordinary non-tracing stderr is not covered by
that cap. A truncation marker means later callbacks are unknown.

Winit's `trace!(target = module_path, ...)` stores `target` as an event field;
its metadata target remains `winit::platform_impl::macos::util`. The filter
deliberately selects that metadata, retaining surrounding AppKit callbacks so
their ordering remains visible. It does not enable general Winit/Bevy payload
logging, even if `RUST_LOG` would allow more events. Span-creation logging is
required to retain the IME setter's boolean.

For literal keys with no native preedit, compare the latest `set_ime_allowed`
call with `keyDown:`, `setMarkedText:selectedRange:replacementRange:`,
`insertText:replacementRange:`, and `doCommandBySelector:` scopes. A public
setter call is not a read of Winit's private backend state. Missing marked-text
callbacks alone do not establish why AppKit declined composition. Logging can
affect scheduling, so a passing diagnostic run does not explain an intermittent
failure or establish that it is fixed. Keep all existing prerequisite, focus,
source, native field ownership, and actual IME-event checks.
