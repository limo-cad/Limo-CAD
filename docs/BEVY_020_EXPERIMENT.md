# Native text input adapter

## Why the full text adapter remains

The new [`TextInput`](https://github.com/bevyengine/bevy/blob/v0.20.0-rc.1/crates/bevy_ui_widgets/src/text_input.rs)
does not replace our document ownership, commit validation, rejected-draft
handling, draft undo/redo, MCP equivalence or event ordering. The three executable
probes in `desktop/tests/bevy_020_widgets.rs` confirm these integration limits:

1. Ordinary typing queues an edit without immediately changing the buffer.
   An owned submit still needs an explicit flush before reading the value.
2. With AltGr represented by Control/Alt/AltGraph, or Option represented by Alt,
   the candidate does not insert the supplied character text. Our adapter needs
   to preserve those input paths. This is a synthetic event probe, not a claim
   of validation with every operating system's keyboard event sequence.
3. Batched IME delivery uses the focus present when the Bevy system runs.
   Changing focus after queuing a commit delivers it to the later field.
   The controller must preserve the original document/control owner and process
   typing, focus changes and model actions in their original order.

The direct `bevy_ui_widgets` dependency supports the probes. The CAD event
router continues to use `EditableText`; CAD fields do not carry `TextInput`,
so a standard widget handler does not also edit them. Passing the probes
records the candidate's current behavior; it does not establish input parity.
