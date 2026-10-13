param(
    [string]$BambuStudio = 'C:/Program Files/Bambu Studio/bambu-studio.exe',
    [string]$OrcaSlicer = 'C:/Program Files/OrcaSlicer/orca-slicer.exe'
)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
Push-Location $taskRoot
try {
    $taskResults = Join-Path $taskRoot 'target/slicer-acceptance'
    New-Item -ItemType Directory -Force -Path $taskResults | Out-Null
    $env:LIMO_CAD_3MF_FIXTURE = Join-Path $taskRoot 'target/multipart-acceptance.3mf'
    if (Test-Path -LiteralPath $env:LIMO_CAD_3MF_FIXTURE) { Remove-Item -LiteralPath $env:LIMO_CAD_3MF_FIXTURE }
    & cargo test --locked -p limo-cad-native-engine --features native-occt --lib host::print_layout_tests::native_named_layout_export_preserves_repeats_and_nested_poses -- --exact
    if ($LASTEXITCODE -ne 0 -or !(Test-Path -LiteralPath $env:LIMO_CAD_3MF_FIXTURE)) { throw 'Native host fixture generation failed' }
    foreach ($taskSlicer in @(@{name='bambu'; exe=$BambuStudio}, @{name='orca'; exe=$OrcaSlicer})) {
        if (!(Test-Path -LiteralPath $taskSlicer.exe)) { throw "Missing slicer: $($taskSlicer.exe)" }
        $taskLog = Join-Path $taskResults '00000.log'
        $taskNamedLog = Join-Path $taskResults "$($taskSlicer.name).log"
        foreach ($taskOldLog in @($taskLog, $taskNamedLog)) {
            if (Test-Path -LiteralPath $taskOldLog) { Remove-Item -LiteralPath $taskOldLog }
        }
        $taskOutput = Join-Path $taskResults "$($taskSlicer.name)-roundtrip.3mf"
        if (Test-Path -LiteralPath $taskOutput) { Remove-Item -LiteralPath $taskOutput }
        $taskProcess = Start-Process -FilePath $taskSlicer.exe -ArgumentList @(
            '--debug', '3', '--arrange', '0', '--export-3mf', "`"$taskOutput`"", "`"$env:LIMO_CAD_3MF_FIXTURE`""
        ) -WorkingDirectory $taskResults -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $taskResults "$($taskSlicer.name)-stdout.log") -RedirectStandardError (Join-Path $taskResults "$($taskSlicer.name)-stderr.log")
        $null = $taskProcess.Handle
        if (!$taskProcess.WaitForExit(60000)) { $taskProcess.Kill(); throw "$($taskSlicer.name) timed out" }
        $taskProcess.WaitForExit()
        if ($taskProcess.ExitCode -ne 0 -or !(Test-Path -LiteralPath $taskOutput)) { throw "$($taskSlicer.name) import/export failed" }
        if (Test-Path -LiteralPath $taskLog) { Copy-Item -LiteralPath $taskLog -Destination $taskNamedLog -Force }
    }
    $env:LIMO_CAD_SLICER_RESULTS = $taskResults
    & cargo test --locked -p limo-cad-native-engine --features native-occt --lib host::print_layout_tests::installed_slicer_roundtrips_preserve_native_named_layout -- --ignored --exact
    if ($LASTEXITCODE -ne 0) { throw 'Slicer roundtrip validation failed' }
} finally {
    Remove-Item Env:LIMO_CAD_3MF_FIXTURE -ErrorAction SilentlyContinue
    Remove-Item Env:LIMO_CAD_SLICER_RESULTS -ErrorAction SilentlyContinue
    Pop-Location
}
