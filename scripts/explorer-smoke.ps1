param(
    [string]$Executable = 'target\release\convert-anythings.exe',
    [ValidateSet('image', 'pdf', 'document', 'audio', 'video')][string]$Kind = 'image',
    [string]$Fixture,
    [ValidateSet('F8', 'F9')][string]$Shortcut = 'F8',
    [switch]$Cancel
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$Executable = (Resolve-Path -LiteralPath (Join-Path $root $Executable)).Path
$output = Join-Path $root '.test-output'
$case = Join-Path $output ('explorer-' + [guid]::NewGuid().ToString('N'))
New-Item -Path $case -ItemType Directory -Force | Out-Null
$config = $case + '-config'
New-Item -Path $config -ItemType Directory | Out-Null
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
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(Point point);
    [DllImport("user32.dll")] public static extern short GetAsyncKeyState(int key);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, IntPtr processId);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint from, uint to, bool attach);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd, uint msg, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint x, uint y, uint data, UIntPtr extra);
    [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr value);
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int X, Y; }
    public static void Key(byte vk) { keybd_event(vk, 0, 0, UIntPtr.Zero); keybd_event(vk, 0, 2, UIntPtr.Zero); }
    public static void Mouse(uint flags) { mouse_event(flags, 0, 0, 0, UIntPtr.Zero); }
    public static void DragTo(int x, int y) { SetCursorPos(x - 1, y); mouse_event(1, 1, 0, 0, UIntPtr.Zero); }
    public static bool Focus(IntPtr hwnd) {
        if (GetForegroundWindow() == hwnd || SetForegroundWindow(hwnd)) return true;
        uint owner = GetWindowThreadProcessId(GetForegroundWindow(), IntPtr.Zero);
        uint current = GetCurrentThreadId();
        bool attached = owner != 0 && owner != current && AttachThreadInput(current, owner, true);
        try { return SetForegroundWindow(hwnd); }
        finally { if (attached) AttachThreadInput(current, owner, false); }
    }
}
'@
}
[void][ConvertExplorerSmoke]::SetProcessDpiAwarenessContext([IntPtr](-4))

function Wait-For([scriptblock]$Find, [string]$Failure, [int]$Seconds = 8) {
    $timer = [Diagnostics.Stopwatch]::StartNew()
    while ($timer.Elapsed.TotalSeconds -lt $Seconds) {
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

$fileName = 'fixture.bmp'
if ($Fixture) {
    $source = (Resolve-Path -LiteralPath $Fixture).Path
    $fileName = 'fixture' + [IO.Path]::GetExtension($source)
    Copy-Item -LiteralPath $source -Destination (Join-Path $case $fileName)
} elseif ($Kind -eq 'image') {
    $bitmap = New-Object Drawing.Bitmap 52, 36
    try {
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        try { $graphics.Clear([Drawing.Color]::DarkCyan) } finally { $graphics.Dispose() }
        $bitmap.Save((Join-Path $case $fileName), [Drawing.Imaging.ImageFormat]::Bmp)
    } finally { $bitmap.Dispose() }
} else { throw 'Provide the fixture file for this conversion family.' }
$file = Join-Path $case $fileName
$fileRegex = [regex]::Escape($fileName)
$outputExtension = @{ image = 'jpg'; pdf = 'jpg'; document = 'pdf'; audio = 'mp3'; video = 'webm' }[$Kind]
$convertedPath = Join-Path $case ('fixture.' + $outputExtension)
[IO.File]::WriteAllText((Join-Path $config 'hotkey.txt'), $Shortcut, [Text.Encoding]::ASCII)

$originalMouse = New-Object ConvertExplorerSmoke+Point
[void][ConvertExplorerSmoke]::GetCursorPos([ref]$originalMouse)
$previousTrace = $env:CONVERTANYTHINGS_TRACE
$previousConfig = $env:CONVERTANYTHINGS_CONFIG_DIR
$env:CONVERTANYTHINGS_TRACE = $trace
$env:CONVERTANYTHINGS_CONFIG_DIR = $config
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
    if (-not (Get-Trace "^hotkey $Shortcut$")) { throw "The app did not register $Shortcut." }
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
    if (-not [ConvertExplorerSmoke]::Focus($handle)) { throw 'Could not foreground the fixture Explorer window.' }
    try { $explorer.Document.CurrentViewMode = 3 } catch { }
    $element = [Windows.Automation.AutomationElement]::FromHandle($handle)
    $item = Wait-For {
        foreach ($name in @($fileName, 'fixture')) {
            $condition = New-Object Windows.Automation.PropertyCondition ([Windows.Automation.AutomationElement]::NameProperty), $name
            $candidate = $element.FindFirst([Windows.Automation.TreeScope]::Descendants, $condition)
            if ($candidate -and -not $candidate.Current.BoundingRectangle.IsEmpty) { $candidate; break }
        }
    } 'Explorer accessibility did not find the fixture file.'
    $rect = $item.Current.BoundingRectangle
    try { $item.SetFocus() } catch { }
    $x = [int]($rect.Left + [Math]::Min(36, $rect.Width / 2))
    $y = [int]($rect.Top + $rect.Height / 2)
    [void][ConvertExplorerSmoke]::SetCursorPos($x, $y)
    $explorer.Document.SelectItem($explorer.Document.Folder.ParseName($fileName), 29)
    $selected = $explorer.Document.SelectedItems()
    if ($selected.Count -ne 1 -or $selected.Item(0).Path -ine $file) {
        throw "Explorer was not focused on the fixture before dragging at $x,$y ($($item.Current.ClassName), $rect)."
    }
    [ConvertExplorerSmoke]::Mouse(2)
    $mouseDown = $true
    Start-Sleep -Milliseconds 160
    [ConvertExplorerSmoke]::DragTo(($x + 16), ($y + 12))
    Start-Sleep -Milliseconds 80
    [ConvertExplorerSmoke]::DragTo(($x + 40), ($y + 35))
    Start-Sleep -Milliseconds 180
    $hotkey = if ($Shortcut -eq 'F9') { 0x78 } else { 0x77 }
    [ConvertExplorerSmoke]::Key($hotkey)
    $shown = Wait-For { Get-Trace '^overlay-open ' } "$Shortcut did not open the overlay during the Explorer drag."
    $values = $shown.Split(' ')
    $left, $top, $size = [int]$values[1], [int]$values[2], [int]$values[3]
    $dropX = $left + [int]($size * 0.50)
    if ($Kind -eq 'video') {
        # Select the Video group before moving to the outer WebM sector.
        [ConvertExplorerSmoke]::DragTo($dropX, ($top + [int]($size * 0.24)))
        Start-Sleep -Milliseconds 160
    }
    $dropY = $top + [int]($size * 0.13)
    [ConvertExplorerSmoke]::DragTo($dropX, $dropY)
    try {
        Wait-For { Get-Trace ("^drag-enter .*$fileRegex$") } 'Explorer did not deliver the dragged file to the overlay.' | Out-Null
    } catch {
        $cursor = New-Object ConvertExplorerSmoke+Point
        [void][ConvertExplorerSmoke]::GetCursorPos([ref]$cursor)
        $hit = [ConvertExplorerSmoke]::WindowFromPoint($cursor)
        $wheel = [ConvertExplorerSmoke]::FindWindow('ConvertAnythings.Wheel', 'ConvertAnythings')
        $pressed = [ConvertExplorerSmoke]::GetAsyncKeyState(1) -lt 0
        throw "$_ Start=$x,$y cursor=$($cursor.X),$($cursor.Y) rect=$rect buttonDown=$pressed hitWheel=$($hit -eq $wheel -and $wheel -ne [IntPtr]::Zero)"
    }
    Wait-For { Get-Trace '^overlay-redraw$' } 'The overlay did not draw choices.' | Out-Null
    Start-Sleep -Milliseconds 80
    if (-not $Cancel) {
        $capture = New-Object Drawing.Bitmap $size, $size
        try {
            $graphics = [Drawing.Graphics]::FromImage($capture)
            try { $graphics.CopyFromScreen($left, $top, 0, 0, $capture.Size) } finally { $graphics.Dispose() }
            $capture.Save((Join-Path $output "radial-menu-$Kind.png"), [Drawing.Imaging.ImageFormat]::Png)
        } finally { $capture.Dispose() }
    }
    if ($Cancel) { [ConvertExplorerSmoke]::Key(0x1b) }
    [ConvertExplorerSmoke]::Mouse(4)
    $mouseDown = $false
    if ($Cancel) {
        Wait-For { Get-Trace '^drag-leave$' } 'Escape did not cancel the active native drag.' | Out-Null
        Wait-For { Get-Trace '^overlay-closed$' } 'Escape did not close the overlay.' | Out-Null
        if (Get-Trace '^drop ') { throw 'A cancelled drag queued a conversion.' }
    } else {
        Wait-For { Get-Trace ("^drop .*$fileRegex -> $outputExtension$") } 'The mouse release did not deliver an accepted drop.' | Out-Null
        $job = Wait-For { Get-Trace '^job-(ok|error) ' } 'The radial menu conversion did not finish.' 30
        if ($job -notmatch '^job-ok ' -or -not (Test-Path -LiteralPath $convertedPath)) { throw $job }
        if ($Kind -eq 'image' -and -not $Fixture) {
            $converted = [Drawing.Image]::FromFile($convertedPath)
            try { if ($converted.Width -ne 52 -or $converted.Height -ne 36) { throw 'Unexpected JPG dimensions.' } }
            finally { $converted.Dispose() }
        }
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
}
