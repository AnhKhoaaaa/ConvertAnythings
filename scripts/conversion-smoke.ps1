param(
    [string]$Executable = 'target\release\convert-anythings.exe',
    [switch]$ImagesOnly,
    [switch]$DocumentsOnly
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

function New-PdfFixture([string]$Path, [int]$PageCount) {
    $objects = New-Object 'System.Collections.Generic.List[string]'
    $kids = (0..($PageCount - 1) | ForEach-Object { "$(4 + 2 * $_) 0 R" }) -join ' '
    $objects.Add('<< /Type /Catalog /Pages 2 0 R >>')
    $objects.Add("<< /Type /Pages /Kids [$kids] /Count $PageCount >>")
    $objects.Add('<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>')
    for ($index = 0; $index -lt $PageCount; $index++) {
        $contentId = 5 + 2 * $index
        $objects.Add("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 144 72] /Resources << /Font << /F1 3 0 R >> >> /Contents $contentId 0 R >>")
        $commands = "BT /F1 14 Tf 16 40 Td (Page $($index + 1)) Tj ET`n"
        $objects.Add("<< /Length $($commands.Length) >>`nstream`n${commands}endstream")
    }
    $buffer = New-Object Text.StringBuilder
    $positions = New-Object 'System.Collections.Generic.List[int]'
    [void]$buffer.Append("%PDF-1.4`n")
    for ($index = 0; $index -lt $objects.Count; $index++) {
        $positions.Add($buffer.Length)
        [void]$buffer.Append("$($index + 1) 0 obj`n$($objects[$index])`nendobj`n")
    }
    $xref = $buffer.Length
    $entries = $objects.Count + 1
    [void]$buffer.Append("xref`n0 $entries`n0000000000 65535 f `n")
    foreach ($position in $positions) { [void]$buffer.Append(('{0:0000000000} 00000 n ' -f $position) + "`n") }
    [void]$buffer.Append("trailer`n<< /Size $entries /Root 1 0 R >>`nstartxref`n$xref`n%%EOF`n")
    [IO.File]::WriteAllText($Path, $buffer.ToString(), [Text.Encoding]::ASCII)
}

function New-DocxFixture([string]$Path) {
    Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::Open($Path, [IO.Compression.ZipArchiveMode]::Create)
    try {
        $entries = @{
            '[Content_Types].xml' = '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>'
            '_rels/.rels' = '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="doc" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>'
            'word/document.xml' = '<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:rPr><w:b/><w:sz w:val="36"/></w:rPr><w:t>ConvertAnythings Word document</w:t></w:r></w:p><w:tbl><w:tblPr><w:tblW w:w="4800" w:type="dxa"/></w:tblPr><w:tblGrid><w:gridCol w:w="2400"/><w:gridCol w:w="2400"/></w:tblGrid><w:tr><w:tc><w:p><w:r><w:t>Left cell</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Right cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:sectPr><w:pgSz w:w="12240" w:h="15840"/></w:sectPr></w:body></w:document>'
        }
        foreach ($name in $entries.Keys) {
            $writer = New-Object IO.StreamWriter ($archive.CreateEntry($name).Open())
            try { $writer.Write($entries[$name]) } finally { $writer.Dispose() }
        }
    } finally { $archive.Dispose() }
}

$vietnamese = 'Ti' + [char]0x1ebf + 'ng Vi' + [char]0x1ec7 + 't'
if (-not $DocumentsOnly) {
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
}

if ($ImagesOnly) { return }

$single = Join-Path $case 'one-page.pdf'
New-PdfFixture $single 1
$raster = Convert-File $single png
$info = Get-ImageInfo $raster
Assert ($info.Width -eq 300 -and $info.Height -eq 150) 'PDF page does not have the requested 150 DPI resolution.'
Assert ($info.Format -eq [Drawing.Imaging.ImageFormat]::Png.Guid) 'PDF PNG container mismatch.'
$multi = Join-Path $case 'multi-page.pdf'
New-PdfFixture $multi 2
$pages = Convert-File $multi jpg
Assert ((Split-Path -Leaf $pages) -eq 'multi-page-pages') 'Unexpected PDF output directory name.'
Assert (@(Get-ChildItem -LiteralPath $pages -Filter '*.jpg').Count -eq 2) 'A PDF page was skipped.'
Assert ((Get-ImageInfo (Join-Path $pages 'multi-page-002.jpg')).Format -eq [Drawing.Imaging.ImageFormat]::Jpeg.Guid) 'PDF JPEG container mismatch.'
$second = Convert-File $multi png
Assert ($second -like '*(1)') 'A PDF output directory collision was not handled.'
$badPdf = Join-Path $case 'broken.pdf'
[IO.File]::WriteAllText($badPdf, '%PDF-1.4 corrupt')
Convert-File $badPdf png -ExpectFailure | Out-Null
Assert (-not (Test-Path -LiteralPath (Join-Path $case 'broken.png'))) 'A corrupt PDF published a partial image.'
Write-Output "PDF raster tests passed. Fixtures: $case"

$referenceWord = $null
$referenceDocument = $null
$existingWord = @([Diagnostics.Process]::GetProcessesByName('WINWORD'))
try {
    if ($existingWord.Count -eq 0) {
        $referenceWord = New-Object -ComObject Word.Application
        $referenceDocument = $referenceWord.Documents.Add([Type]::Missing, $false, 0, $false)
        $referenceDocument.Content.Text = 'Unrelated unsaved fixture'
    }
    $txt = Join-Path $case 'utf8.txt'
    $utf16 = Join-Path $case 'utf16.txt'
    $docx = Join-Path $case 'layout.docx'
    [IO.File]::WriteAllText($txt, "$vietnamese`r`nConverted from text.", (New-Object Text.UTF8Encoding $false))
    [IO.File]::WriteAllText($utf16, "$vietnamese`r`nConverted from text.", [Text.Encoding]::Unicode)
    New-DocxFixture $docx
    $txtPdf = Convert-File $txt pdf
    $utf16Pdf = Convert-File $utf16 pdf
    $docxPdf = Convert-File $docx pdf
    foreach ($path in @($txtPdf, $utf16Pdf, $docxPdf)) {
        $signature = [Text.Encoding]::ASCII.GetString(([IO.File]::ReadAllBytes($path))[0..4])
        Assert ($signature -eq '%PDF-') "Word PDF signature mismatch: $path"
    }
    $docxRaster = Convert-File $docxPdf png
    $info = Get-ImageInfo $docxRaster
    Assert ($info.Width -eq 1275 -and $info.Height -eq 1650) 'The exported Word page did not retain its Letter layout.'
    $textTool = Get-Command pdftotext -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($textTool) {
        foreach ($path in @($txtPdf, $utf16Pdf, $docxPdf)) {
            $extracted = [IO.Path]::ChangeExtension($path, 'verified.txt')
            $errlog = $extracted + '.log'
            $toolArgs = @('-q', '-enc', 'UTF-8', ('"' + $path + '"'), ('"' + $extracted + '"'))
            $process = Start-Process -FilePath $textTool.Source -ArgumentList $toolArgs -Wait -PassThru -WindowStyle Hidden -RedirectStandardError $errlog
            Assert ($process.ExitCode -eq 0) "Could not extract text from $path"
            $content = [IO.File]::ReadAllText($extracted).Normalize()
            if ($path -eq $docxPdf) {
                Assert ($content.Contains('Left cell') -and $content.Contains('Right cell')) 'The Word table contents went missing.'
            } else {
                Assert ($content.Contains($vietnamese)) 'Vietnamese text did not survive Word PDF export.'
            }
        }
    }
    if ($referenceDocument) {
        Assert ($referenceDocument.Content.Text.Trim() -eq 'Unrelated unsaved fixture') 'Another Word document was affected.'
    }
    foreach ($process in $existingWord) { Assert (-not $process.HasExited) 'An existing Word instance was closed.' }
} finally {
    if ($referenceDocument) { $referenceDocument.Close([ref]0) }
    if ($referenceWord) { $referenceWord.Quit([ref]0) }
}
Assert (@(Get-ChildItem -LiteralPath $case -Force -Filter '.convert-anythings-*').Count -eq 0) 'Document conversion left staging files behind.'
Write-Output "Word PDF tests passed. Fixtures: $case"
