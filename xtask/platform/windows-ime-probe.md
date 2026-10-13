# Disposable Windows IME prerequisite probe

Dispatch the existing `Native desktop host tests` (`native-host-tests.yml`) on the
reviewed branch with `ime-probe-only=true`. The first run only records OS/image,
runner identity, current-user languages, registered/enabled TSF profiles, Japanese
capability state, text-input services/processes, and interactive desktop identity.
It requires no native CAD build. Inspect the uploaded `report.json`, including
per-query errors; `inventory-complete` does not mean an IME is ready.

```powershell
gh workflow run native-host-tests.yml --repo limo-cad/Limo-CAD --ref feat/bevy-interface -f ime-probe-only=true
```

All IME inputs default to false for dispatch and reusable calls. Ordinary
push, pull request, dispatch, and reusable invocations retain the existing native
and optional package jobs. Probe-only skips all four of those jobs, including
when `native-packages=true`, and runs only the Windows probe by default. Setting
`ime-probe-macos=true` selects the [macOS probe](macos-ime-probe.md) instead, with
its own concurrency group so Windows provisioning can continue. The provisioning
and exercise inputs have no effect unless `ime-probe-only=true`.
Probe-only runs use a separate concurrency group, so dispatching the inventory
does not cancel native or package checks already running on the same branch.

This uses a workflow already registered on the default branch, so the reviewed
feature-branch revision can be selected with `--ref` without merging the Bevy
transition. [GitHub dispatch requirement](https://docs.github.com/en/actions/how-tos/manage-workflow-runs/manually-run-a-workflow).

`ime-provision-japanese=true` additionally installs the discovered Japanese Basic and
Jpan font capabilities through DISM and enables the Microsoft Japanese profile
for the disposable runner user. It does not use the client-only `Install-Language`
cmdlet, change display/system locale, configure services, reboot, or sign out.
Installation attempts are written before starting; results retain elapsed time,
HRESULTs, state changes, DISM logs, and `RestartNeeded`. A required restart fails
the probe and leaves that environment prerequisite unresolved.
Provisioning has a 35-minute step limit within a 40-minute job, leaving time for
evidence upload. In run `36337710379`, Japanese Basic installed without a restart
in 14 minutes 20 seconds; the original 15-minute limit then canceled the font
installation after 35 seconds, before profile activation or IME input. That
timeout established neither IME feasibility nor an IME failure. Both installed
capabilities and the actual stock-control composition assertions remain required.

`ime-profile-diagnosis=true` selects a separate **zero-key** experiment
(`-DiagnoseProfile` in the script). Use it with `ime-probe-only=true` and leave
exercise and the macOS selector false. By default it does not install capabilities.
It may follow explicitly requested `ime-provision-japanese=true` in the same job,
retaining before/after installation evidence, but it rejects composition input.
The registered Microsoft Japanese profile alone permits diagnosis; it does not
prove working IME input.

```powershell
gh workflow run native-host-tests.yml --repo limo-cad/Limo-CAD --ref feat/bevy-interface -f ime-probe-only=true -f ime-profile-diagnosis=true
```

Run `36348814600` failed before activation because the probe's ThreadMgr CLSID
contained `ab9c` instead of the SDK's `ab9e`; no source changed and no keys ran.
With that literal corrected, run `36349699758` completed the zero-key diagnosis.
The owned TSF thread/document activated successfully. Exact profile activation
returned `S_OK` and modern enablement changed to true, but Japanese language
selection returned `80004005`, and the active source remained US English after
message pumping. Both Japanese capabilities were absent. Cleanup restored the
prior active source/language and balanced the thread activation. This does not
prove composition or attribute the language failure to missing capabilities.
The next controlled comparison provisions those same two capabilities before
the identical no-key diagnosis:

```powershell
gh workflow run native-host-tests.yml --repo limo-cad/Limo-CAD --ref feat/bevy-interface -f ime-probe-only=true -f ime-provision-japanese=true -f ime-profile-diagnosis=true
```

The diagnostic retains exact legacy enablement, modern profile flags, active
profile, current language, HRESULTs and OS thread ID before showing an owned
stock textbox, after showing it, after explicitly activating that thread's TSF
manager, and after pumping its message loop. It then requests Japanese on that
same thread and calls exact `ActivateProfile` with `TF_IPPMF_ENABLEPROFILE`.
It does not use process/session activation or `DONTCARECURRENTINPUTLANGUAGE`,
whose documented behavior can defer activation until a future language switch.
The latter is not evidence that a source is currently active.

The prior exact active source and language must be readable before either source
change is attempted. Cleanup restores and verifies both, including on errors,
and balances the diagnostic's thread-manager activation. Current-user profile
registration may be enabled on the disposable VM; no developer settings change.
Every mutation remains guarded before COM/window creation, and the entire method
sends **zero keys**. `profile-diagnosis-complete` means observations were collected
and source cleanup succeeded, even if activation returned a failure HRESULT.
It does not mean Japanese became enabled, or any preedit/commit/cancel reached a
control. Existing composition-input enablement and focus guards are unchanged.

Run `36340954448` installed both capabilities without a restart in about 29
minutes. The language list contained the Japanese TIP, but TSF still reported
that profile disabled; no input was attempted. Provisioning now also calls
`ITfInputProcessorProfiles::EnableLanguageProfile` for that exact current-user
profile and checks `IsEnabledLanguageProfile` before the existing unique-enabled
profile guard. It does not change the default profile or activate another user's
input method. The helper recognizes Windows' canonical `ja` language tag.

Run `36343149647` still stopped before input: the legacy enabled query was true
while the modern enumerated profile had flags `0`. The probe incorrectly skipped
the enable call in that case and reported `S_OK` anyway. Provisioning now always
calls the documented current-user enable API once and retains its actual result,
including failure, before checking availability. Inventory and enablement evidence
also record exact `GetProfile`, `IsEnabledLanguageProfile`, and current-language
results, so disagreement between enumeration, direct queries, and activation is
visible. The SDK layouts and flag values match the probe. These APIs do not
document the observed disagreement as normal; it remains unresolved until the
disposable retry. Neither a legacy true value nor a successful enable call
bypasses the unique enabled modern-profile guard or the owned UI thread's exact
active-profile check. No input or Bevy success has been established by these runs.

`ime-exercise=true` separately opts into ordinary virtual-key SendInput on a fresh
owned stock WinForms textbox. The helper verifies the foreground/focus and reads
the exact active Microsoft Japanese profile on its own UI thread. It selects
Hiragana, types ASCII `haru`, requires received `WM_IME_COMPOSITION/GCS_COMPSTR`
preedit `はる`, commits once with Enter, then checks a second composition cancels.
It records the actual IMM preedit/result strings, message sequence, profile,
key actions, owned HWND/PID, and final text. It never posts IME messages, injects
Unicode packets, or synthesizes Bevy events. A 20-second owned-window deadline
bounds this check. Missing profile/focus/composition is a failing result.

Provisioned zero-key run `36350094873` activated the exact modern Japanese
profile inside an owned TSF document, then restored the prior US source. Both
capabilities were installed. This established a stronger prerequisite than the
earlier legacy enable call, but sent no keys. A provisioned exercise now reuses
that bounded zero-key activation step only when the modern inventory still
reports disabled. It requires successful exact activation and source restoration,
then re-reads the unique modern enabled profile before allowing input. Exercise
without explicit provisioning retains the original enabled-profile requirement.
The stock control still independently checks its own active profile and actual
composition; prerequisite success cannot substitute for those assertions.

After reviewing the inventory, explicitly request provisioning and the separate
stock-control exercise on a fresh disposable runner:

```powershell
gh workflow run native-host-tests.yml --repo limo-cad/Limo-CAD --ref feat/bevy-interface -f ime-probe-only=true -f ime-provision-japanese=true -f ime-exercise=true
```

Provisioning and input require repository ID `1313334315` on an explicitly
opted-in GitHub-hosted Windows job with evidence beneath `RUNNER_TEMP`. The VM is
discarded by GitHub afterward. The inventory-only script may be run locally:

```powershell
powershell.exe -NoProfile -File xtask/platform/windows-ime-probe.ps1 -Out C:\absolute\fresh-evidence
```

Local capability queries may record elevation errors. Do not elevate or enable
the mutation/input switches on a developer machine just to satisfy the probe.
For syntax/compile validation, parse the PowerShell file and compile the C# with
`Add-Type -ReferencedAssemblies System.Windows.Forms,System.Drawing`; compiling
does not invoke `Exercise` or open a window.

**A stock-control success is environment feasibility only.** It does not validate
the Bevy text adapter, candidate popup ownership/placement/pixels, DPI transitions,
or physical keyboard hardware. Those remain separate checks. The workflow always
uploads its evidence, including failed/in-progress attempts. It performs no CAD
build, does not select a user document, and does not change the release host.

Primary references:

- [Server-supported Add-WindowsCapability](https://learn.microsoft.com/en-us/powershell/module/dism/add-windowscapability?view=windowsserver2025-ps)
- [LanguagePackManagement is client-only](https://learn.microsoft.com/en-us/powershell/module/languagepackmanagement/?view=windowsserver2025-ps)
- [Current-user language and input methods](https://learn.microsoft.com/en-us/powershell/module/international/set-winuserlanguagelist?view=windowsserver2025-ps)
- [Enable the exact current-user TSF profile](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfinputprocessorprofiles-enablelanguageprofile)
- [Verify TSF profile enablement](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfinputprocessorprofiles-isenabledlanguageprofile)
- [Query the exact modern TSF profile](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfinputprocessorprofilemgr-getprofile)
- [Activate the calling thread's TSF manager](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfthreadmgr-activate)
- [Change the current input language; missing thread-manager error](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfinputprocessorprofiles-changecurrentlanguage)
- [Exact profile activation and deferred-activation flags](https://learn.microsoft.com/en-us/windows/win32/api/msctf/nf-msctf-itfinputprocessorprofilemgr-activateprofile)
- [Microsoft Windows SDK profile layouts and flags](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/RecompiledIdlHeaders/um/msctf.h)
- [Japanese IME keys and `haru` example](https://learn.microsoft.com/en-us/globalization/input/japanese-ime)
- [IMM composition strings](https://learn.microsoft.com/en-us/windows/win32/api/imm/nf-imm-immgetcompositionstringw)
