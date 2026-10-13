$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest



function Parse-Script([string]$Name) {
    $tokens = $null
    $errors = $null
    $ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot $Name), [ref]$tokens, [ref]$errors)
    if ($errors.Count) { throw ($errors | Out-String) }
    return $ast
}
$session = Parse-Script 'native-windows-ime-session.ps1'
$typeSource = $session.FindAll({ param($node)
    $node -is [Management.Automation.Language.StringConstantExpressionAst] -and
        $node.Value.Contains('public static class NativePlatformInput')
}, $true)
if ($typeSource.Count -ne 1) { throw 'Expected the specialized Windows IME helper type' }
Add-Type -TypeDefinition $typeSource[0].Value
$resolver = $session.FindAll({ param($node)
    $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Resolve-OwnedImePaths'
}, $true)
if ($resolver.Count -ne 1) { throw 'Expected the production ownership path resolver' }
. ([scriptblock]::Create($resolver[0].Extent.Text))
function Expect-Rejection([scriptblock]$Action, [string]$Message) {
    try { & $Action | Out-Null } catch {
        if (-not $_.Exception.Message.Contains($Message)) { throw }
        return
    }
    throw "Expected rejection: $Message"
}

$fixture = Join-Path ([IO.Path]::GetTempPath()) ('limo-cad-ime-paths-' + [Guid]::NewGuid().ToString('N'))
$root = Join-Path $fixture 'runner'
$output = Join-Path $root ('[owned] $output ' + [char]0x3042)
$sibling = Join-Path $fixture 'runner-other'
$junction = Join-Path $root 'junction-outside'
$hostFile = Join-Path $root 'native host.exe'
$otherHost = Join-Path $root 'other host.exe'
try {
    [void][IO.Directory]::CreateDirectory($output)
    [void][IO.Directory]::CreateDirectory($sibling)
    [IO.File]::WriteAllText($hostFile, 'owned')
    [IO.File]::WriteAllText($otherHost, 'foreign')
    $canonicalRoot = [NativePlatformInput]::CanonicalPath($root)
    $canonicalOutput = [NativePlatformInput]::CanonicalPath($output)
    $canonicalHost = [NativePlatformInput]::CanonicalPath($hostFile)
    foreach ($runnerPath in @($root, $canonicalRoot, ($root + '\'))) {
        foreach ($outputPath in @($output, $canonicalOutput)) {
            foreach ($expectedHost in @($hostFile, $canonicalHost)) {
                $accepted = Resolve-OwnedImePaths $runnerPath $outputPath $expectedHost $hostFile
                if ($accepted -cne $canonicalOutput) { throw 'Same owned paths resolved differently' }
            }
        }
    }

    $self = (Get-Process -Id $PID).MainModule.FileName
    [void](Resolve-OwnedImePaths $root $canonicalOutput ([NativePlatformInput]::CanonicalPath($self)) $self)
    Expect-Rejection { Resolve-OwnedImePaths $root $root $hostFile $hostFile } 'must be beneath'
    Expect-Rejection { Resolve-OwnedImePaths $root $sibling $hostFile $hostFile } 'must be beneath'
    Expect-Rejection { Resolve-OwnedImePaths $root $output $hostFile $otherHost } 'does not match'
    Expect-Rejection { Resolve-OwnedImePaths $root $output (Join-Path $root 'missing.exe') $hostFile } 'Cannot open owned path'
    [void](New-Item -ItemType Junction -Path $junction -Target $sibling)
    Expect-Rejection { Resolve-OwnedImePaths $root $junction $hostFile $hostFile } 'must be beneath'

    $report = [IO.Path]::Combine($canonicalOutput, 'windows-ime-cleanup.json')
    [IO.File]::WriteAllText($report, '{"status":"finished"}', [Text.UTF8Encoding]::new($false))
    if (([IO.File]::ReadAllText($report) | ConvertFrom-Json).status -ne 'finished') { throw 'Canonical cleanup report could not be read' }
    [IO.File]::Delete($report)
    Write-Output 'PASS: canonical owned paths, current executable, exact directory boundary, foreign executable, missing file, junction escape, cleanup report'
} finally {

    if ([IO.Directory]::Exists($junction)) { [IO.Directory]::Delete($junction) }
    if ([IO.File]::Exists((Join-Path $output 'windows-ime-cleanup.json'))) { [IO.File]::Delete((Join-Path $output 'windows-ime-cleanup.json')) }
    if ([IO.File]::Exists($hostFile)) { [IO.File]::Delete($hostFile) }
    if ([IO.File]::Exists($otherHost)) { [IO.File]::Delete($otherHost) }
    foreach ($directory in @($output, $sibling, $root, $fixture)) {
        if ([IO.Directory]::Exists($directory)) { [IO.Directory]::Delete($directory) }
    }
}
