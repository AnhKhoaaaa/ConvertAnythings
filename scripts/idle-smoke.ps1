param(
    [string]$Executable = 'target\release\convert-anythings.exe',
    [ValidateRange(5, 120)][int]$IdleSeconds = 30,
    [ValidateRange(2, 50)][int]$Cycles = 20
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$Executable = (Resolve-Path -LiteralPath (Join-Path $root $Executable)).Path
$output = Join-Path $root '.test-output'
$case = Join-Path $output ('idle-' + [guid]::NewGuid().ToString('N'))
New-Item -Path $case -ItemType Directory -Force | Out-Null
$trace = Join-Path $case 'events.log'

Add-Type -AssemblyName System.Windows.Forms
if (-not ('ConvertIdleSmoke' -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ConvertIdleSmoke {
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out Point point);
    [DllImport("user32.dll")] public static extern IntPtr FindWindow(string cls, string title);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd, uint msg, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int X, Y; }
    public static void Hotkey() { keybd_event(0x77, 0, 0, UIntPtr.Zero); keybd_event(0x77, 0, 2, UIntPtr.Zero); }
}
'@
}
[void][ConvertIdleSmoke]::SetProcessDpiAwarenessContext([IntPtr](-4))

function Find-Events([string]$Pattern) {
    if (Test-Path -LiteralPath $trace) {
        Get-Content -LiteralPath $trace | Where-Object { $_ -match $Pattern }
    }
}

function Wait-Events([string]$Pattern, [int]$Count) {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while ($clock.Elapsed.TotalSeconds -lt 5) {
        $lines = @(Find-Events $Pattern)
        if ($lines.Count -ge $Count) { return $lines[-1] }
        Start-Sleep -Milliseconds 35
    }
    throw "Expected $Count occurrences of $Pattern in $trace"
}

function Measure-Waiting([Diagnostics.Process]$Process) {
    $initialRedraws = @(Find-Events '^overlay-(open |redraw$)').Count
    $Process.Refresh()
    $initialCPU = $Process.TotalProcessorTime.TotalMilliseconds
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $gpuQuerySucceeded = $false
    $gpuMeasurements = @()
    try {
        $samples = (Get-Counter '\GPU Engine(*)\Utilization Percentage' -SampleInterval 1 -MaxSamples 5).CounterSamples
        $gpuMeasurements = @($samples | Where-Object { $_.InstanceName -like "pid_$($Process.Id)_*" } | ForEach-Object CookedValue)
        $gpuQuerySucceeded = $true
    } catch { }
    $remaining = [int]([Math]::Max(0, $IdleSeconds * 1000 - $clock.ElapsedMilliseconds))
    Start-Sleep -Milliseconds $remaining
    $Process.Refresh()
    $elapsed = $clock.Elapsed.TotalMilliseconds
    $cpu = ($Process.TotalProcessorTime.TotalMilliseconds - $initialCPU) / $elapsed / [Environment]::ProcessorCount * 100
    $privateWorkingSet = $null
    try {
        $counters = (Get-Counter -Counter '\Process(convert-anythings*)\ID Process', '\Process(convert-anythings*)\Working Set - Private').CounterSamples
        $instance = $counters | Where-Object { $_.Path.EndsWith('\id process') -and $_.CookedValue -eq $Process.Id } | Select-Object -First 1
        if ($instance) {
            $privateWorkingSet = ($counters | Where-Object { $_.InstanceName -eq $instance.InstanceName -and $_.Path.EndsWith('\working set - private') } | Select-Object -First 1).CookedValue
        }
    } catch { }
    $draws = @(Find-Events '^overlay-(open |redraw$)').Count - $initialRedraws
    if ($draws -ne 0) { throw 'The hidden app drew frames while it was supposed to be idle.' }
    [pscustomobject]@{
        Seconds = [Math]::Round($elapsed / 1000, 2)
        WorkingSetMiB = [Math]::Round($Process.WorkingSet64 / 1MB, 2)
        PrivateWorkingSetMiB = $(if ($null -ne $privateWorkingSet) { [Math]::Round($privateWorkingSet / 1MB, 2) })
        CpuPercent = [Math]::Round($cpu, 4)
        GpuCounterAvailable = $gpuQuerySucceeded
        GpuSamplesForProcess = $gpuMeasurements.Count
        GpuPeakPercent = $(if ($gpuMeasurements.Count -gt 0) { [Math]::Round(($gpuMeasurements | Measure-Object -Maximum).Maximum, 4) })
        OverlayDraws = $draws
    }
}

if ([ConvertIdleSmoke]::FindWindow('ConvertAnythings.Controller', 'ConvertAnythings') -ne [IntPtr]::Zero) {
    throw 'Close the existing ConvertAnythings instance before measuring the test instance.'
}
$beforeTrace = $env:CONVERTANYTHINGS_TRACE
$beforeConfig = $env:CONVERTANYTHINGS_CONFIG_DIR
$env:CONVERTANYTHINGS_TRACE = $trace
$env:CONVERTANYTHINGS_CONFIG_DIR = $case
$app = $null
$window = [IntPtr]::Zero
$mouse = New-Object ConvertIdleSmoke+Point
[void][ConvertIdleSmoke]::GetCursorPos([ref]$mouse)

try {
    $app = Start-Process -FilePath $Executable -WindowStyle Hidden -PassThru
    Wait-Events '^hotkey F8$' 1 | Out-Null
    Wait-Events '^ready$' 1 | Out-Null
    $window = [ConvertIdleSmoke]::FindWindow('ConvertAnythings.Controller', 'ConvertAnythings')
    $before = Measure-Waiting $app

    $screens = [Windows.Forms.Screen]::AllScreens
    $placements = for ($index = 0; $index -lt $Cycles; $index++) {
        $screen = $screens[$index % $screens.Count]
        $area = $screen.WorkingArea
        $point = if (($index % 2) -eq 0) { @(($area.Left + 2), ($area.Top + 2)) } else { @(($area.Right - 2), ($area.Bottom - 2)) }
        if (-not [ConvertIdleSmoke]::SetCursorPos($point[0], $point[1])) { throw 'Could not move to the screen edge.' }
        [ConvertIdleSmoke]::Hotkey()
        $line = (Wait-Events '^overlay-open ' ($index + 1)).Split(' ')
        $left, $top, $size, $dpi = [int]$line[1], [int]$line[2], [int]$line[3], [int]$line[4]
        if ($left -lt $area.Left -or $top -lt $area.Top -or $left + $size -gt $area.Right -or $top + $size -gt $area.Bottom) {
            throw "Overlay outside work area on $($screen.DeviceName): $line"
        }
        Wait-Events '^menu-latency-us ' ($index + 1) | Out-Null
        [ConvertIdleSmoke]::Hotkey()
        Wait-Events '^overlay-closed$' ($index + 1) | Out-Null
        [pscustomobject]@{ Screen = $screen.DeviceName; Dpi = $dpi; Pixels = $size }
    }
    $after = Measure-Waiting $app
    $latency = @(Find-Events '^menu-latency-us ' | ForEach-Object { [double]($_.Split(' ')[1]) / 1000 })
    $sorted = @($latency | Sort-Object)
    $report = [pscustomobject]@{
        Cycles = $Cycles
        ExecutableBytes = (Get-Item -LiteralPath $Executable).Length
        Displays = @($placements | Sort-Object -Property Screen, Dpi -Unique)
        Before = $before
        After = $after
        FirstDrawMs = [Math]::Round($latency[0], 2)
        P50DrawMs = [Math]::Round($sorted[[int]([Math]::Ceiling($sorted.Count * 0.50) - 1)], 2)
        P95DrawMs = [Math]::Round($sorted[[int]([Math]::Ceiling($sorted.Count * 0.95) - 1)], 2)
    }
    $report | ConvertTo-Json -Depth 5 | Tee-Object -FilePath (Join-Path $output 'idle-report.json')
} finally {
    if ($window -ne [IntPtr]::Zero) {
        [void][ConvertIdleSmoke]::PostMessage($window, 0x10, [IntPtr]::Zero, [IntPtr]::Zero)
    }
    if ($app -and -not $app.WaitForExit(4000)) { Stop-Process -Id $app.Id -Force }
    [void][ConvertIdleSmoke]::SetCursorPos($mouse.X, $mouse.Y)
    $env:CONVERTANYTHINGS_TRACE = $beforeTrace
    $env:CONVERTANYTHINGS_CONFIG_DIR = $beforeConfig
}
