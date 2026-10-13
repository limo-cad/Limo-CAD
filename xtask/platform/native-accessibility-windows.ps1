param([Parameter(Mandatory=$true)][int]$OwnedPid)
$ErrorActionPreference='Stop'
[Console]::InputEncoding=[Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$request=[Console]::In.ReadToEnd() | ConvertFrom-Json
$process=Get-Process -Id $OwnedPid -ErrorAction Stop
$handle=$process.MainWindowHandle
if($handle -eq 0){throw 'Owned application has no native window'}
$root=[Windows.Automation.AutomationElement]::FromHandle($handle)
if($root.Current.ProcessId -ne $OwnedPid){throw 'UIA root belongs to a different process'}
$elements=$root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
if($elements.Count -gt 4096){throw 'Owned accessibility tree exceeds fixture limit'}
$controls=@()
foreach($element in $elements){
    $current=$element.Current
    if($current.ProcessId -ne $OwnedPid){throw 'UIA descendant belongs to a different process'}
    $controls+= [ordered]@{name=$current.Name;role=$current.ControlType.ProgrammaticName;enabled=$current.IsEnabled;
        focusable=$current.IsKeyboardFocusable;patterns=@($element.GetSupportedPatterns() | ForEach-Object {$_.ProgrammaticName})}
}
if($request.operation -ne 'inspect'){
    $targets=@($elements | Where-Object {$_.Current.Name -ceq $request.label -and $_.Current.IsEnabled})
    if($targets.Count -ne 1){throw "Expected one owned enabled UIA control named '$($request.label)', got $($targets.Count)"}
    $target=$targets[0]
    if($target.Current.ProcessId -ne $OwnedPid){throw 'UIA target changed process ownership'}
    switch($request.operation){
        'invoke' {([Windows.Automation.InvokePattern]$target.GetCurrentPattern([Windows.Automation.InvokePattern]::Pattern)).Invoke()}
        'set_value' {([Windows.Automation.ValuePattern]$target.GetCurrentPattern([Windows.Automation.ValuePattern]::Pattern)).SetValue([string]$request.value)}
        default {throw 'Unknown owned UIA operation'}
    }
}
[ordered]@{owned_pid=$OwnedPid;operation=$request.operation;label=$request.label;controls=$controls} | ConvertTo-Json -Depth 5 -Compress
