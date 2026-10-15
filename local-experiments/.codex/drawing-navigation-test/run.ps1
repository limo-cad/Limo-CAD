$ErrorActionPreference = 'Stop'
$navRepo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$navDeps = Join-Path $navRepo 'src-tauri/target/debug/deps'
$navRlib = Get-ChildItem -LiteralPath $navDeps -Filter 'libnbcad_interface*.rlib' | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
if (-not $navRlib) { throw 'Build the native interface dependency before running this isolated harness.' }
$navExe = Join-Path $PSScriptRoot 'navigation-tests.exe'
"Using $($navRlib.FullName)" | Set-Content -Encoding UTF8 (Join-Path $PSScriptRoot 'build.log')
& rustc --edition=2024 --test (Join-Path $PSScriptRoot 'harness.rs') -L "dependency=$navDeps" --extern "nbcad_interface=$($navRlib.FullName)" -o $navExe 2>&1 | Tee-Object -Append -FilePath (Join-Path $PSScriptRoot 'build.log')
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
& $navExe --nocapture 2>&1 | Tee-Object -FilePath (Join-Path $PSScriptRoot 'tests.log')
exit $LASTEXITCODE