param(
    [string]$Executable = 'target\release\convert-anythings.exe',
    [switch]$ProofOnly
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$Executable = (Resolve-Path -LiteralPath (Join-Path $root $Executable)).Path
$output = Join-Path $root '.test-output'
$case = Join-Path $output ('explorer-' + [guid]::NewGuid().ToString('N'))
New-Item -Path $case -ItemType Directory -Force | Out-Null
$trace = Join-Path $output ((Split-Path -Leaf $case) + '.log')

Add-Type -AssemblyName System.Drawing, UIAutomationClient, UIAutomationTypes
if (-not ('ConvertExplorerSmoke' -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class ConvertExplorerSmoke {
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool GetCursorPos(out Point point);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern IntPtr FindWindow(string cls, string title);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd, uint msg, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint x, uint y, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int X, Y; }
    public static void Key(byte vk) { keybd_event(vk, 0, 0, UIntPtr.Zero); keybd_event(vk, 0, 2, UIntPtr.Zero); }
    public static void Mouse(uint flags) { mouse_event(flags, 0, 0, 0, UIntPtr.Zero); }
}
'@
}
[void][ConvertExplorerSmoke]::SetProcessDpiAwarenessContext([IntPtr](-4))

function Wait-For([scriptblock]$Find, [string]$Failure) {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    while ($timer.Elapsed.TotalSeconds -lt 8) {
        $result = & $Find
        if ($result) { return $result }
        Start-Sleep -Milliseconds 75
    }
    throw $Failure
}

function Get-Trace([string]$Pattern) {
    if (Test-Path -LiteralPath $trace) {
        Get-Content -LiteralPath $trace | Where-Object { $_ -match $Pattern } | Select-Object -Last 1
    }
}

$file = Join-Path $case 'fixture.bmp'
$bitmap = New-Object Drawing.Bitmap 52, 36
try {
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try { $graphics.Clear([Drawing.Color]::DarkCyan) } finally { $graphics.Dispose() }
    $bitmap.Save($file, [Drawing.Imaging.ImageFormat]::Bmp)
} finally { $bitmap.Dispose() }

$originalMouse = New-Object ConvertExplorerSmoke+Point
[void][ConvertExplorerSmoke]::GetCursorPos([ref]$originalMouse)
$previousTrace = $env:CONVERTANYTHINGS_TRACE
$previousConfig = $env:CONVERTANYTHINGS_CONFIG_DIR
$previousSmoke = $env:CONVERTANYTHINGS_SMOKE
$env:CONVERTANYTHINGS_TRACE = $trace
$env:CONVERTANYTHINGS_CONFIG_DIR = $case
$env:CONVERTANYTHINGS_SMOKE = '1'
$app = $null
$explorer = $null
$mouseDown = $false
$hostWindow = [IntPtr]::Zero

try {
    if ([ConvertExplorerSmoke]::FindWindow('ConvertAnythings.Controller', 'ConvertAnythings') -ne [IntPtr]::Zero) {
        throw 'Close the existing ConvertAnythings instance before running the smoke test.'
    }
    $app = Start-Process -FilePath $Executable -WindowStyle Hidden -PassThru
    Wait-For { Get-Trace '^ready$' } 'The application did not signal readiness.' | Out-Null
    $hostWindow = [ConvertExplorerSmoke]::FindWindow('ConvertAnythings.Controller', 'ConvertAnythings')
    if ($hostWindow -eq [IntPtr]::Zero -or [ConvertExplorerSmoke]::IsWindowVisible($hostWindow)) {
        throw 'The application controller must exist and stay hidden.'
    }

    $shell = New-Object -ComObject Shell.Application
    $shell.Explore($case)
    $explorer = Wait-For {
        foreach ($window in $shell.Windows()) {
            try { if ($window.Document.Folder.Self.Path -eq $case) { $window; break } } catch { }
        }
    } 'Explorer did not open the fixture folder.'
    $handle = [IntPtr]([int64]$explorer.HWND)
    [void][ConvertExplorerSmoke]::SetForegroundWindow($handle)
    $element = [Windows.Automation.AutomationElement]::FromHandle($handle)
    $item = Wait-For {
        foreach ($name in @('fixture.bmp', 'fixture')) {
            $condition = New-Object Windows.Automation.PropertyCondition ([Windows.Automation.AutomationElement]::NameProperty), $name
            $candidate = $element.FindFirst([Windows.Automation.TreeScope]::Descendants, $condition)
            if ($candidate -and -not $candidate.Current.BoundingRectangle.IsEmpty) { $candidate; break }
        }
    } 'Explorer accessibility did not find the fixture file.'
    $rect = $item.Current.BoundingRectangle
    $x = [int]($rect.Left + $rect.Width / 4)
    $y = [int]($rect.Top + $rect.Height / 2)
    [void][ConvertExplorerSmoke]::SetCursorPos($x, $y)
    [ConvertExplorerSmoke]::Mouse(2)
    $mouseDown = $true
    Start-Sleep -Milliseconds 160
    [void][ConvertExplorerSmoke]::SetCursorPos(($x + 38), ($y + 6))
    Start-Sleep -Milliseconds 180
    [ConvertExplorerSmoke]::Key(0x77)
    $shown = Wait-For { Get-Trace '^overlay-open ' } 'F8 did not open the overlay during the Explorer drag.'
    $values = $shown.Split(' ')
    $left, $top, $size = [int]$values[1], [int]$values[2], [int]$values[3]
    if ($ProofOnly) {
        $dropX = $left + [int]($size * 0.60)
        $dropY = $top + [int]($size * 0.48)
    } else {
        # A BMP's first output is JPG at twelve o'clock.
        $dropX = $left + [int]($size * 0.50)
        $dropY = $top + [int]($size * 0.16)
    }
    [void][ConvertExplorerSmoke]::SetCursorPos($dropX, $dropY)
    Wait-For { Get-Trace '^drag-enter .*fixture\.bmp$' } 'Explorer did not deliver the dragged BMP to the overlay.' | Out-Null
    if (-not $ProofOnly) {
        $image = New-Object Drawing.Bitmap $size, $size
        try {
            $graphics = [Drawing.Graphics]::FromImage($image)
            try { $graphics.CopyFromScreen($left, $top, 0, 0, $image.Size) } finally { $graphics.Dispose() }
            $image.Save((Join-Path $output 'radial-menu.png'), [Drawing.Imaging.ImageFormat]::Png)
        } finally { $image.Dispose() }
    }
    [ConvertExplorerSmoke]::Mouse(4)
    $mouseDown = $false
    Wait-For { Get-Trace '^drop .*fixture\.bmp(?:\s|$)' } 'The mouse release did not deliver an accepted drop.' | Out-Null
    if (-not $ProofOnly) {
        Wait-For { Test-Path -LiteralPath (Join-Path $case 'fixture.jpg') } 'The radial menu did not produce the JPG.' | Out-Null
        $converted = [Drawing.Image]::FromFile((Join-Path $case 'fixture.jpg'))
        try { if ($converted.Width -ne 52 -or $converted.Height -ne 36) { throw 'Unexpected JPG dimensions.' } }
        finally { $converted.Dispose() }
    }
    if (-not (Test-Path -LiteralPath $file)) { throw 'The source file was moved or removed.' }
    Get-Content -LiteralPath $trace
} finally {
    if ($mouseDown) {
        [ConvertExplorerSmoke]::Key(0x1b)
        [ConvertExplorerSmoke]::Mouse(4)
    }
    if ($hostWindow -ne [IntPtr]::Zero) {
        [void][ConvertExplorerSmoke]::PostMessage($hostWindow, 0x10, [IntPtr]::Zero, [IntPtr]::Zero)
    }
    if ($app -and -not $app.WaitForExit(4000)) { Stop-Process -Id $app.Id -Force }
    if ($explorer) { try { $explorer.Quit() } catch { } }
    [void][ConvertExplorerSmoke]::SetCursorPos($originalMouse.X, $originalMouse.Y)
    $env:CONVERTANYTHINGS_TRACE = $previousTrace
    $env:CONVERTANYTHINGS_CONFIG_DIR = $previousConfig
    $env:CONVERTANYTHINGS_SMOKE = $previousSmoke
}
