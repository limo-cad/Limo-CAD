param(
    [Parameter(Mandatory = $true)][string]$Out,
    [switch]$ProvisionJapanese,
    [switch]$ExerciseIme,
    [switch]$DiagnoseProfile
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-DisposableRunner {
    if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows' -or
        $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or
        $env:GITHUB_REPOSITORY_ID -ne '1313334315' -or $env:GITHUB_RUN_ID -notmatch '^\d+$') {
        throw 'Provisioning and real input are allowed only in the explicitly opted-in disposable GitHub-hosted Windows job'
    }
    $runnerRoot = [IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\', '/') + [IO.Path]::DirectorySeparatorChar
    $outputRoot = [IO.Path]::GetFullPath($Out)
    if (-not $outputRoot.StartsWith($runnerRoot, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Mutating probe evidence must be inside RUNNER_TEMP'
    }
}
if ($ProvisionJapanese -or $ExerciseIme -or $DiagnoseProfile) { Assert-DisposableRunner }
if ($DiagnoseProfile -and $ExerciseIme) {
    throw 'Profile diagnosis is a separate zero-key experiment; do not combine it with input'
}
if (-not [IO.Path]::IsPathRooted($Out)) { throw 'Use an absolute fresh evidence directory' }
if ((Test-Path -LiteralPath $Out) -and (Get-ChildItem -LiteralPath $Out -Force | Select-Object -First 1)) {
    throw 'Preserve prior evidence: use an empty output directory'
}
[void](New-Item -ItemType Directory -Path $Out -Force)
$reportPath = Join-Path $Out 'report.json'
$report = [ordered]@{
    schema_version = 1
    status = 'started'
    started_utc = [DateTime]::UtcNow.ToString('o')
    requested = @{ provision_japanese = [bool]$ProvisionJapanese; exercise_ime = [bool]$ExerciseIme; diagnose_profile = [bool]$DiagnoseProfile }
    environment = [ordered]@{
        os_version = [Environment]::OSVersion.VersionString
        powershell = $PSVersionTable.PSVersion.ToString()
        image_os = $env:ImageOS; image_version = $env:ImageVersion
        runner_os = $env:RUNNER_OS; runner_environment = $env:RUNNER_ENVIRONMENT
        repository = $env:GITHUB_REPOSITORY; repository_id = $env:GITHUB_REPOSITORY_ID
        run_id = $env:GITHUB_RUN_ID; sha = $env:GITHUB_SHA
        identity_sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
        session_id = (Get-Process -Id $PID).SessionId
        probe_script_sha256 = (Get-FileHash -LiteralPath $PSCommandPath).Hash
        probe_helper_sha256 = (Get-FileHash -LiteralPath (Join-Path $PSScriptRoot 'windows-ime-probe.cs')).Hash
    }
    provisioning = @()
    restart_needed = $false
    ime = @{ status = 'not-attempted'; native_bevy_validated = $false; candidate_placement = 'not tested' }
    not_proven = @('Bevy IME', 'candidate window ownership/pixels/placement', 'monitor DPI transition', 'physical keyboard')
}
function Save-Report {
    $report | ConvertTo-Json -Depth 24 | Set-Content -LiteralPath $reportPath -Encoding UTF8
}
function Test-JapaneseProfileEnabled($Inventory) {
    $enabled = @($Inventory.profiles | Where-Object {
        $_.type -eq 1 -and $_.language -eq '0411' -and $_.enabled -and
        $_.class_id -eq '03b5835f-f03c-411b-9ce2-aa23e1171e36' -and
        $_.profile_id -eq 'a76c93d9-5523-4e90-aafa-4db112f9ac76'
    })
    return $enabled.Count -eq 1
}
function Error-Detail($Record) {
    return @{ message = $Record.Exception.Message; hresult = $Record.Exception.HResult.ToString('X8'); error_id = $Record.FullyQualifiedErrorId }
}
$capabilityNames = @('Language.Basic~~~ja-JP~0.0.1.0', 'Language.Fonts.Jpan~~~und-JPAN~0.0.1.0')
function Get-Inventory {
    $inventory = [ordered]@{ profiles = @() }
    try {

        $inventory.language_list = @(foreach ($language in (Get-WinUserLanguageList)) {
            @{ language_tag = $language.LanguageTag; english_name = $language.EnglishName; input_method_tips = @($language.InputMethodTips) }
        })
    } catch { $inventory.language_list_error = Error-Detail $_ }
    try { $inventory.profiles = @([WindowsImeProbe]::EnumerateProfiles()) }
    catch { $inventory.profiles_error = Error-Detail $_ }
    try { $inventory.japanese_profile_status = [WindowsImeProbe]::JapaneseProfileStatus() }
    catch { $inventory.japanese_profile_status_error = Error-Detail $_ }
    try { $inventory.desktop = [WindowsImeProbe]::Desktop() }
    catch { $inventory.desktop_error = Error-Detail $_ }
    $inventory.capabilities = @(
        foreach ($name in $capabilityNames) {
            try {
                $capability = @(Get-WindowsCapability -Online -Name $name)
                if ($capability.Count -ne 1) { throw "Expected exactly one capability record for $name; got $($capability.Count)" }
                @{ name = $name; state = $capability[0].State.ToString() }
            } catch { @{ name = $name; state = 'query-failed'; error = (Error-Detail $_) } }
        }
    )
    try {
        $inventory.services = @(Get-CimInstance Win32_Service | Where-Object {
            $_.Name -match '^(TextInputManagementService|TabletInputService|TextInputService|cbdhsvc)(_|$)'
        } | Select-Object Name, DisplayName, State, StartMode, ProcessId)
        $inventory.text_input_processes = @(Get-Process -Name ctfmon, TextInputHost -ErrorAction SilentlyContinue |
            Select-Object Id, ProcessName, SessionId)
    } catch { $inventory.services_error = Error-Detail $_ }
    return $inventory
}
Save-Report
$exitCode = 0
try {
    Add-Type -Path (Join-Path $PSScriptRoot 'windows-ime-probe.cs') -ReferencedAssemblies System.Windows.Forms, System.Drawing
    $report.before = Get-Inventory
    $report.status = 'inventory-complete'
    Save-Report
    if ($ProvisionJapanese) {
        $report.status = 'provisioning-in-progress'; Save-Report



        foreach ($name in $capabilityNames) {
            Assert-DisposableRunner
            $observed = @($report.before.capabilities | Where-Object { $_.name -eq $name })[0]
            if ($observed.state -eq 'query-failed') { throw "Cannot provision an unavailable capability: $name" }
            $attempt = [ordered]@{ capability = $name; state_before = $observed.state; status = 'already-installed'; restart_needed = $false }
            $report.provisioning += $attempt
            if ($observed.state -ne 'Installed') {
                $attempt.status = 'in-progress'; Save-Report
                $watch = [Diagnostics.Stopwatch]::StartNew()
                try {
                    $installed = Add-WindowsCapability -Online -Name $name -LogPath (Join-Path $Out ($name.Split('~')[0] + '.dism.log'))
                    $attempt.restart_needed = [bool]$installed.RestartNeeded
                    $attempt.state_after = (Get-WindowsCapability -Online -Name $name).State.ToString()
                    $attempt.status = 'completed'
                    $report.restart_needed = $report.restart_needed -or $attempt.restart_needed
                    if ($attempt.state_after -ne 'Installed') { throw "Capability did not become Installed: $name" }
                } catch { $attempt.status = 'failed'; $attempt.error = Error-Detail $_; throw }
                finally { $attempt.elapsed_ms = $watch.ElapsedMilliseconds; Save-Report }
                if ($report.restart_needed) { throw 'Capability installation requires restart; the probe will not reboot or sign out this job' }
            }
        }
        Assert-DisposableRunner
        $languages = Get-WinUserLanguageList

        if (-not @($languages | Where-Object LanguageTag -Match '^ja(?:-JP)?$').Count) { $languages.Add('ja-JP') }
        $japanese = @($languages | Where-Object LanguageTag -Match '^ja(?:-JP)?$')
        if ($japanese.Count -ne 1) { throw 'Expected exactly one Japanese user-language entry' }
        $japanese = $japanese[0]
        $tip = '0411:{03B5835F-F03C-411B-9CE2-AA23E1171E36}{A76C93D9-5523-4E90-AAFA-4DB112F9AC76}'
        if ($japanese.InputMethodTips -notcontains $tip) { $japanese.InputMethodTips.Add($tip) }
        $report.profile_update = @{ status = 'in-progress'; language = 'ja-JP'; input_method_tip = $tip }
        Save-Report
        Set-WinUserLanguageList -LanguageList $languages -Force


        Assert-DisposableRunner
        $report.profile_update.tsf_enable = [WindowsImeProbe]::EnableJapaneseProfile()
        Save-Report
        if (-not $report.profile_update.tsf_enable.succeeded) {
            throw 'EnableLanguageProfile failed; inspect its recorded HRESULT; no input was sent'
        }
        if ($report.profile_update.tsf_enable.after.is_enabled -ne $true) {
            throw 'Microsoft Japanese profile remained disabled after EnableLanguageProfile; no input was sent'
        }
        $report.profile_update.status = 'completed'
        $report.status = 'provisioning-complete'
    }
    $report.after = Get-Inventory
    $report.japanese_profile_enabled = Test-JapaneseProfileEnabled $report.after
    Save-Report
    if ($DiagnoseProfile) {
        Assert-DisposableRunner
        if ($report.after.desktop.station -ne 'WinSta0' -or -not $report.after.desktop.user_interactive) {
            throw 'Interactive WinSta0 desktop required for the owned diagnostic control'
        }
        $report.status = 'profile-diagnosis-in-progress'; Save-Report
        $report.profile_diagnosis = [WindowsImeProbe]::DiagnoseProfile()
        Save-Report
        if ($report.profile_diagnosis.status -ne 'profile-diagnosis-complete') { throw 'Profile context diagnosis failed; inspect its observations and cleanup' }
        $report.status = 'profile-diagnosis-complete'
    }
    if ($ExerciseIme) {
        Assert-DisposableRunner
        if ($report.after.desktop.station -ne 'WinSta0' -or -not $report.after.desktop.user_interactive) {
            throw 'Interactive WinSta0 desktop required; no input was sent'
        }
        if ($ProvisionJapanese -and -not $report.japanese_profile_enabled) {





            if (@($report.after.capabilities | Where-Object { $_.state -ne 'Installed' }).Count) {
                throw 'Both Japanese capabilities must be Installed before owned-context activation'
            }
            $report.status = 'stock-control-activation-in-progress'; Save-Report
            $report.activation_prerequisite = [WindowsImeProbe]::DiagnoseProfile()
            Save-Report
            if ($report.activation_prerequisite.status -ne 'profile-diagnosis-complete' -or
                $report.activation_prerequisite.activate_profile_hresult -ne '00000000' -or
                $report.activation_prerequisite.cleanup.status -ne 'restored') {
                throw 'Owned-context activation or exact source restoration failed; no input was sent'
            }
            $report.after_activation = Get-Inventory
            $report.japanese_profile_enabled = Test-JapaneseProfileEnabled $report.after_activation
            Save-Report
        }
        if (-not $report.japanese_profile_enabled) { throw 'Microsoft Japanese IME is not uniquely enabled; no input was sent' }
        $report.status = 'stock-control-ime-in-progress'
        $report.ime = @{ status = 'in-progress'; native_bevy_validated = $false }; Save-Report
        $report.ime = [WindowsImeProbe]::Exercise()
        if ($report.ime.status -ne 'stock-control-ime-feasible') { throw 'Real IME stock-control probe failed; inspect ime evidence' }
        $report.status = 'stock-control-ime-feasible'
    }
} catch {
    $report.status = 'failed'
    $report.error = Error-Detail $_
    $exitCode = 1
} finally {
    $report.finished_utc = [DateTime]::UtcNow.ToString('o')
    Save-Report
}
Write-Output "Windows IME prerequisite report: $reportPath (status=$($report.status)); Bevy and candidate placement remain unvalidated"
exit $exitCode
