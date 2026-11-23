param(
    [string]$Executable = 'target\release\convert-anythings.exe',
    [switch]$ImagesOnly
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$Executable = (Resolve-Path -LiteralPath (Join-Path $root $Executable)).Path
$case = Join-Path $root ('.test-output\conversion-' + [guid]::NewGuid().ToString('N'))
New-Item -Path $case -ItemType Directory -Force | Out-Null
Add-Type -AssemblyName System.Drawing

function Assert([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Convert-File([string]$InputFile, [string]$Format, [switch]$ExpectFailure, [switch]$NoFfmpeg) {
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $Executable
    $info.Arguments = '--convert --to ' + $Format + ' "' + $InputFile + '"'
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.StandardOutputEncoding = [Text.Encoding]::UTF8
    $info.StandardErrorEncoding = [Text.Encoding]::UTF8
    if ($NoFfmpeg) { $info.EnvironmentVariables['PATH'] = "$env:WINDIR\System32" }
    $process = [Diagnostics.Process]::Start($info)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(125000)) { $process.Kill(); throw "Timed out converting $InputFile to $Format" }
        if ($ExpectFailure) {
            Assert ($process.ExitCode -ne 0) "An unsupported conversion succeeded: $InputFile -> $Format"
            return $stderr.Result.Trim()
        }
        Assert ($process.ExitCode -eq 0) "Conversion failed: $InputFile -> $Format. $($stderr.Result)"
        $output = $stdout.Result.Trim()
        Assert (Test-Path -LiteralPath $output) "Worker claimed success with no output: $output"
        return $output
    } finally { $process.Dispose() }
}

function Get-ImageInfo([string]$Path) {
    $image = [Drawing.Bitmap]::FromFile($Path)
    try {
        return @{ Width = $image.Width; Height = $image.Height; Pixel = $image.GetPixel(0, 0); Format = $image.RawFormat.Guid }
    } finally { $image.Dispose() }
}

$vietnamese = 'Ti' + [char]0x1ebf + 'ng Vi' + [char]0x1ec7 + 't'
$png = Join-Path $case "$vietnamese [mau].png"
$image = New-Object Drawing.Bitmap 52, 36, ([Drawing.Imaging.PixelFormat]::Format32bppArgb)
try {
    $image.SetPixel(20, 20, [Drawing.Color]::DarkCyan)
    $image.Save($png, [Drawing.Imaging.ImageFormat]::Png)
} finally { $image.Dispose() }

$jpg = Convert-File $png jpg
$info = Get-ImageInfo $jpg
Assert ($info.Width -eq 52 -and $info.Height -eq 36) 'JPEG dimensions changed.'
Assert ($info.Pixel.R -ge 245 -and $info.Pixel.G -ge 245 -and $info.Pixel.B -ge 245) 'Transparent pixels did not become white in JPEG.'
Assert ($info.Format -eq [Drawing.Imaging.ImageFormat]::Jpeg.Guid) 'The JPG output has the wrong container.'
$before = (Get-FileHash -LiteralPath $jpg).Hash
$duplicates = Convert-File $png jpg
Assert ($duplicates -like '*(1).jpg') 'A collision did not add the expected suffix.'
Assert ((Get-FileHash -LiteralPath $jpg).Hash -eq $before) 'The original output was overwritten.'

$bmp = Convert-File $png bmp
$tiff = Convert-File $png tiff
Assert ((Get-ImageInfo $bmp).Format -eq [Drawing.Imaging.ImageFormat]::Bmp.Guid) 'BMP container mismatch.'
Assert ((Get-ImageInfo $tiff).Format -eq [Drawing.Imaging.ImageFormat]::Tiff.Guid) 'TIFF container mismatch.'
$fromTiff = Convert-File $tiff png
Assert ((Get-ImageInfo $fromTiff).Pixel.A -eq 0) 'The TIFF to PNG route lost transparency.'
if (Get-Command ffmpeg -ErrorAction SilentlyContinue) {
    $webp = Convert-File $png webp
    $header = [Text.Encoding]::ASCII.GetString(([IO.File]::ReadAllBytes($webp))[8..11])
    Assert ($header -eq 'WEBP') 'The WebP container signature is missing.'
    $fromWebp = Convert-File $webp png
    Assert ((Get-ImageInfo $fromWebp).Pixel.A -eq 0) 'The WebP to PNG route lost transparency.'
    $errorText = Convert-File $webp png -ExpectFailure -NoFfmpeg
    Assert ($errorText -match 'FFmpeg') 'Missing FFmpeg did not report the dependency.'
}
$bad = Join-Path $case 'bad.png'
[IO.File]::WriteAllText($bad, 'broken image data')
Convert-File $bad jpg -ExpectFailure | Out-Null
Assert (-not (Test-Path -LiteralPath (Join-Path $case 'bad.jpg'))) 'A failed conversion published an output.'
Assert (@(Get-ChildItem -LiteralPath $case -Force -Filter '.convert-anythings-*').Count -eq 0) 'Temporary conversion files were left behind.'
Write-Output "Image conversion smoke passed. Fixtures: $case"
