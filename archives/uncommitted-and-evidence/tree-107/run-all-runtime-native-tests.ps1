# Run a Windows test artifact with the application's common-controls v6 manifest.
# The isolated copy leaves Cargo's executable and the SDK unchanged.
param(
    [Parameter(Mandatory = $true)][string]$TestExecutable,
    [Parameter(Mandatory = $true)][string]$AppExecutable,
    [Parameter(Mandatory = $true)][string]$SdkBin,
    [Parameter(Mandatory = $true)][string]$ManifestTool,
    [Parameter(Mandatory = $true)][string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$testSource = (Resolve-Path -LiteralPath $TestExecutable).ProviderPath
$applicationSource = (Resolve-Path -LiteralPath $AppExecutable).ProviderPath
$sdkDirectory = (Resolve-Path -LiteralPath $SdkBin).ProviderPath
$manifestExecutable = (Resolve-Path -LiteralPath $ManifestTool).ProviderPath
$testDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $testDirectory) {
    throw 'Use a new output directory to preserve previous test artifacts.'
}
$originalHash = (Get-FileHash -LiteralPath $testSource -Algorithm SHA256).Hash
New-Item -ItemType Directory -Path $testDirectory | Out-Null
$testCopy = Join-Path $testDirectory 'native-viewport-tests.exe'
$manifestCopy = Join-Path $testDirectory 'common-controls-v6.manifest'
Copy-Item -LiteralPath $testSource -Destination $testCopy
& $manifestExecutable "-inputresource:$applicationSource;#1" "-out:$manifestCopy"
if ($LASTEXITCODE -ne 0) { throw 'Application manifest extraction failed.' }
& $manifestExecutable '-manifest' $manifestCopy "-outputresource:$testCopy;#1"
if ($LASTEXITCODE -ne 0) { throw 'Test-copy manifest embedding failed.' }

$previousPath = $env:PATH
try {
    $env:PATH = $sdkDirectory + [System.IO.Path]::PathSeparator + $previousPath
    & $testCopy 2>&1 |
        Tee-Object -FilePath (Join-Path $testDirectory 'native-viewport-tests.log')
    $testExitCode = $LASTEXITCODE
} finally {
    $env:PATH = $previousPath
}
if ((Get-FileHash -LiteralPath $testSource -Algorithm SHA256).Hash -ne $originalHash) {
    throw 'The original Cargo test executable changed during verification.'
}
exit $testExitCode
