# Privacy redactions in retained evidence

The [redaction manifest](privacy-redactions-20261020.json) identifies retained
logs and qualification records whose developer home directories, local account
names, local drive roots or private project label were replaced with placeholders.
`<local-root>` stands for an original drive root; the remaining artifact tail is
retained, with private project labels replaced by `model`. The manifest records the
source revision and hashes of both versions. Original copies remain private.

These edits preserve measurements, tested revisions, results and the paused
qualification state. They do not add coverage or establish a new test pass.
Historical Git commits retain the original text; this cleanup changes the
current files and guards against accidental reintroduction.
