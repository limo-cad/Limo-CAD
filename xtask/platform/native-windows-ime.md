# Disposable Windows Bevy IME validation

Dispatch the existing `native-host-tests.yml` on the draft feature branch with
`desktop-input=true` and `input-family=windows-ime`. This calls
`windows-native-ime.yml` from that exact branch; GitHub cannot directly dispatch
a new workflow file before it exists on the default branch. It runs only on
a disposable GitHub-hosted Windows 2025 desktop. It first installs the two
Japanese capabilities and requires a successful real stock-control IME test in
the same job. Inventory or zero-key TSF activation alone cannot satisfy this
prerequisite. The ordinary PR workflow never sends these keys.

The driver starts a fresh default native host with its own session
registry. It proves PID, executable path, unique HWND, active session, field
binding, document epoch, and foreground ownership. Only ordinary SendInput
virtual keys choose Japanese/Hiragana and type `haru`. No synthetic IME event
is injected. The fixture requires actual Bevy preedit for `はる`, one commit,
a second composition cancelled without another commit, and an unchanged exact
project before and after closing Rename. Native window captures share the
product Bevy screenshot path.

The opt-in host observer reads the Winit UI thread's current input language,
IMM context, and exact Microsoft TSF profile. A helper's own TSF profile is not
evidence about another thread. These are read-only observations using
[GetKeyboardLayout](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getkeyboardlayout)
and [GetActiveProfile](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfinputprocessorprofilemgr-getactiveprofile).
The observer does not activate a source or initialize a different TSF context.
It exists only under the explicit hosted-runner diagnostic opt-in.

The persistent helper restores the owned host's prior input layout on success,
failure, or EOF. The fixture then verifies the actual Bevy thread's exact prior
profile and layout, rather than accepting helper completion as proof. The
enabled profile set is left as explicitly provisioned for this disposable job.
Clipboard text is restored by the shared platform fixture. Cleanup failures
are failures, and their reports are retained. IMM open/conversion/sentence mode
restoration is not established by the profile/layout comparison.

Until the workflow and original captures have been reviewed, this fixture is
prepared validation, not a Windows Bevy IME pass. It does not prove physical
keyboards, candidate-popup placement/pixels, or monitor DPI transitions.
