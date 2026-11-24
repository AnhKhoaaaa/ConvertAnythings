param(
    [string]$Executable = 'target\release\convert-anythings.exe',
    [switch]$ImagesOnly,
    [switch]$DocumentsOnly,
    [switch]$MediaOnly
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

function Convert-File([string]$InputFile, [string]$Format, [switch]$ExpectFailure, [switch]$NoFfmpeg, [string]$FallbackDir = '') {
    return Convert-Files @($InputFile) $Format -ExpectFailure:$ExpectFailure -NoFfmpeg:$NoFfmpeg -FallbackDir $FallbackDir
}

function Convert-Files([string[]]$InputFiles, [string]$Format, [switch]$ExpectFailure, [switch]$NoFfmpeg, [string]$FallbackDir = '') {
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $Executable
    $quoted = ($InputFiles | ForEach-Object { '"' + $_ + '"' }) -join ' '
    $info.Arguments = '--convert --to ' + $Format + ' ' + $quoted
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $info.StandardOutputEncoding = [Text.Encoding]::UTF8
    $info.StandardErrorEncoding = [Text.Encoding]::UTF8
    if ($NoFfmpeg) {
        $info.EnvironmentVariables['PATH'] = "$env:WINDIR\System32"
        $info.EnvironmentVariables['CONVERTANYTHINGS_STRICT_PATH'] = '1'
    }
    if ($FallbackDir) {
        $info.EnvironmentVariables['CONVERTANYTHINGS_FALLBACK_DIR'] = $FallbackDir
    }
    $process = [Diagnostics.Process]::Start($info)
    try {
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(125000)) { $process.Kill(); throw "Timed out converting $($InputFiles -join ', ') to $Format" }
        if ($ExpectFailure) {
            Assert ($process.ExitCode -ne 0) "An unsupported conversion succeeded: $($InputFiles -join ', ') -> $Format"
            return $stderr.Result.Trim()
        }
        Assert ($process.ExitCode -eq 0) "Conversion failed: $($InputFiles -join ', ') -> $Format. $($stderr.Result)"
        $outputs = @($stdout.Result -split "\r?\n" | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne '' })
        Assert ($outputs.Count -ge 1) "Worker claimed success with no output."
        foreach ($output in $outputs) {
            Assert (Test-Path -LiteralPath $output) "Worker claimed success with no output: $output"
        }
        if ($outputs.Count -eq 1) { return $outputs[0] }
        return $outputs
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
if (-not $DocumentsOnly -and -not $MediaOnly) {
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
    $avif = Convert-File $png avif
    $ftyp = [Text.Encoding]::ASCII.GetString(([IO.File]::ReadAllBytes($avif))[4..7])
    Assert ($ftyp -eq 'ftyp') 'The AVIF container signature is missing.'
    $ico = Convert-File $png ico
    $icoBytes = [IO.File]::ReadAllBytes($ico)
    Assert ($icoBytes[0] -eq 0 -and $icoBytes[1] -eq 0 -and $icoBytes[2] -eq 1 -and $icoBytes[3] -eq 0) 'The ICO header signature is invalid.'
    $errorText = Convert-File $webp png -ExpectFailure -NoFfmpeg
    Assert ($errorText -match 'FFmpeg') 'Missing FFmpeg did not report the dependency.'
}
$singleImgPdf = Convert-File $png pdf
$sigPdf = [Text.Encoding]::ASCII.GetString(([IO.File]::ReadAllBytes($singleImgPdf))[0..4])
Assert ($sigPdf -eq '%PDF-') 'PNG to PDF signature mismatch.'
$mergedImgPdf = Convert-Files @($png, $jpg) pdf
$mergedPages = Convert-File $mergedImgPdf png
Assert ((Get-ChildItem -LiteralPath $mergedPages -Filter '*.png').Count -eq 2) 'Merged image PDF did not contain 2 pages.'
$compressedImgs = Convert-Files @($png, $jpg) compress
Assert ($compressedImgs.Count -eq 2) 'Batch image compress did not return 2 outputs.'
foreach ($cImg in $compressedImgs) {
    Assert ([IO.Path]::GetFileNameWithoutExtension($cImg) -like '*-compressed*') "Compressed image missing -compressed suffix: $cImg"
    $cInfo = Get-ImageInfo $cImg
    Assert ($cInfo.Width -eq 52 -and $cInfo.Height -eq 36) "Compressed image dimensions changed: $cImg"
}
$bad = Join-Path $case 'bad.png'
[IO.File]::WriteAllText($bad, 'broken image data')
Convert-File $bad jpg -ExpectFailure | Out-Null
Assert (-not (Test-Path -LiteralPath (Join-Path $case 'bad.jpg'))) 'A failed conversion published an output.'
Assert (@(Get-ChildItem -LiteralPath $case -Force -Filter '.convert-anythings-*').Count -eq 0) 'Temporary conversion files were left behind.'
Write-Output "Image conversion smoke passed. Fixtures: $case"
}

if ($ImagesOnly) { return }

if (-not $MediaOnly) {
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
$mergedPdf = Convert-Files @($single, $multi) merge-pdf
Assert ([IO.Path]::GetFileName($mergedPdf) -like '*-merged.pdf') "Merged PDF missing -merged suffix: $mergedPdf"
$mergedPdfPages = Convert-File $mergedPdf png
Assert ((Get-ChildItem -LiteralPath $mergedPdfPages -Filter '*.png').Count -eq 3) 'Merged PDF did not contain 3 pages.'
$splitPdfDir = Convert-File $multi split-pdf
Assert ((Split-Path -Leaf $splitPdfDir) -eq 'multi-page-split') "Unexpected split PDF dir: $splitPdfDir"
$splitPdfs = @(Get-ChildItem -LiteralPath $splitPdfDir -Filter '*.pdf')
Assert ($splitPdfs.Count -eq 2) 'Split PDF did not create 2 files.'
foreach ($sp in $splitPdfs) {
    $spSig = [Text.Encoding]::ASCII.GetString(([IO.File]::ReadAllBytes($sp.FullName))[0..4])
    Assert ($spSig -eq '%PDF-') "Split PDF invalid header: $($sp.FullName)"
}
$compressedPdf = Convert-File $multi compress
Assert ([IO.Path]::GetFileName($compressedPdf) -like '*-compressed.pdf') "Compressed PDF missing suffix: $compressedPdf"
$cmpPdfSig = [Text.Encoding]::ASCII.GetString(([IO.File]::ReadAllBytes($compressedPdf))[0..4])
Assert ($cmpPdfSig -eq '%PDF-') 'Compressed PDF signature mismatch.'
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
    if (Get-Command pdftotext -CommandType Application -ErrorAction SilentlyContinue) {
        $pdfTxt = Convert-File $single txt
        $extractedText = [IO.File]::ReadAllText($pdfTxt)
        Assert ($extractedText.Contains('Page 1')) 'PDF to TXT did not contain expected text.'

        $pdfDocx = Convert-File $single docx
        Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
        $zip = [IO.Compression.ZipFile]::OpenRead($pdfDocx)
        try {
            $docEntry = $zip.GetEntry('word/document.xml')
            Assert ($null -ne $docEntry) 'PDF to DOCX did not produce word/document.xml.'
            $reader = New-Object IO.StreamReader ($docEntry.Open())
            try {
                Assert ($reader.ReadToEnd().Contains('Page 1')) 'PDF to DOCX did not contain extracted text.'
            } finally { $reader.Dispose() }
        } finally { $zip.Dispose() }
    }

    $mdPath = Join-Path $case 'readme.md'
    [IO.File]::WriteAllText($mdPath, "# ConvertAnythings`n`nHello **Markdown** and ``code``!`n`n- Item 1`n", [Text.Encoding]::UTF8)
    $htmlOut = Convert-File $mdPath html
    $htmlText = [IO.File]::ReadAllText($htmlOut)
    Assert ($htmlText.Contains('<h1>ConvertAnythings</h1>') -and $htmlText.Contains('<strong>Markdown</strong>')) 'Markdown to HTML output mismatch.'

    $mdDocxOut = Convert-File $mdPath docx
    Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
    $mdZip = [IO.Compression.ZipFile]::OpenRead($mdDocxOut)
    try {
        $mdDocEntry = $mdZip.GetEntry('word/document.xml')
        Assert ($null -ne $mdDocEntry) 'Markdown to DOCX did not produce word/document.xml.'
        $mdReader = New-Object IO.StreamReader ($mdDocEntry.Open())
        try {
            $mdXml = $mdReader.ReadToEnd()
            Assert ($mdXml.Contains('ConvertAnythings') -and $mdXml.Contains('Markdown') -and $mdXml.Contains('<w:b/>')) 'Markdown to DOCX content mismatch.'
        } finally { $mdReader.Dispose() }
    } finally { $mdZip.Dispose() }

    $mdPdfOut = Convert-File $mdPath pdf
    $mdPdfSig = [Text.Encoding]::ASCII.GetString(([IO.File]::ReadAllBytes($mdPdfOut))[0..4])
    Assert ($mdPdfSig -eq '%PDF-') 'Markdown to PDF signature mismatch.'
    if (Get-Command pdftotext -CommandType Application -ErrorAction SilentlyContinue) {
        $mdPdfTxt = [IO.Path]::ChangeExtension($mdPdfOut, 'verified.txt')
        & pdftotext -q -enc UTF-8 $mdPdfOut $mdPdfTxt
        Assert ($LASTEXITCODE -eq 0) 'Could not extract text from Markdown PDF.'
        $mdPdfText = [IO.File]::ReadAllText($mdPdfTxt)
        Assert ($mdPdfText.Contains('ConvertAnythings') -and $mdPdfText.Contains('Markdown')) 'Markdown PDF did not preserve text.'
    }

    $roDir = Join-Path $case 'readonly-src'
    $fbDir = Join-Path $case 'fallback-downloads'
    New-Item -Path $roDir -ItemType Directory -Force | Out-Null
    New-Item -Path $fbDir -ItemType Directory -Force | Out-Null
    $roMd = Join-Path $roDir 'locked.md'
    [IO.File]::WriteAllText($roMd, "# Locked`n`nFallback test", [Text.Encoding]::UTF8)
    $acl = Get-Acl -LiteralPath $roDir
    $denyRule = New-Object Security.AccessControl.FileSystemAccessRule(
        "$env:USERDOMAIN\$env:USERNAME",
        'CreateFiles,CreateDirectories',
        'None',
        'None',
        'Deny'
    )
    $acl.AddAccessRule($denyRule)
    Set-Acl -LiteralPath $roDir -AclObject $acl
    try {
        $fbOut = Convert-File $roMd html -FallbackDir $fbDir
        Assert ((Split-Path -Parent $fbOut) -eq $fbDir) "Read-only fallback did not write to fallback dir: $fbOut"
    } finally {
        $acl.RemoveAccessRule($denyRule) | Out-Null
        Set-Acl -LiteralPath $roDir -AclObject $acl
    }

    $tsCode = Join-Path $case 'script.ts'
    [IO.File]::WriteAllText($tsCode, 'export const v: number = 1;', [Text.Encoding]::UTF8)
    Convert-File $tsCode mp4 -ExpectFailure | Out-Null

    $fakeVideo = Join-Path $case 'misnamed.mp4'
    [IO.File]::WriteAllBytes($fakeVideo, [IO.File]::ReadAllBytes($raster))
    $sniffErr = Convert-File $fakeVideo webm -ExpectFailure
    Assert ($sniffErr -match 'PNG' -and $sniffErr -match 'MP4') "Magic-byte sniff did not report misnamed PNG: $sniffErr"
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
}

if ($DocumentsOnly) { return }

$ffmpeg = (Get-Command ffmpeg -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
$ffprobe = (Get-Command ffprobe -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source

function Write-MediaFixture([string[]]$Options) {
    & $ffmpeg -hide_banner -nostdin -loglevel error @Options
    Assert ($LASTEXITCODE -eq 0) 'FFmpeg could not generate a test fixture.'
}

function Assert-Codecs([string]$Path, [string]$Video, [string]$Audio) {
    $json = & $ffprobe -v error -show_entries stream=codec_name,codec_type -of json $Path
    Assert ($LASTEXITCODE -eq 0) "ffprobe could not open $Path"
    $stream = ($json | Out-String | ConvertFrom-Json).streams
    $actualVideo = ($stream | Where-Object codec_type -eq 'video' | ForEach-Object { $_.codec_name }) -join ','
    $actualAudio = ($stream | Where-Object codec_type -eq 'audio' | ForEach-Object { $_.codec_name }) -join ','
    Assert ($actualVideo -eq $Video -and $actualAudio -eq $Audio) "Wrong codecs in $Path. Video=$actualVideo, audio=$actualAudio"
}

$mp4 = Join-Path $case 'sample-video.mp4'
$mov = Join-Path $case 'video-without-audio.mov'
$mkv = Join-Path $case 'fixture-input.mkv'
$wav = Join-Path $case 'sample-audio.wav'
$ogg = Join-Path $case 'sample-vorbis.ogg'
$m4a = Join-Path $case 'sample-aac.m4a'
Write-MediaFixture @('-f', 'lavfi', '-i', 'testsrc2=size=64x48:rate=12:duration=1', '-f', 'lavfi', '-i', 'sine=frequency=440:duration=1', '-c:v', 'libx264', '-c:a', 'aac', '-shortest', $mp4)
Write-MediaFixture @('-i', $mp4, '-an', '-c:v', 'copy', $mov)
Write-MediaFixture @('-i', $mp4, '-c', 'copy', $mkv)
Write-MediaFixture @('-i', $mp4, '-vn', '-c:a', 'pcm_s16le', $wav)
Write-MediaFixture @('-i', $wav, '-c:a', 'libvorbis', $ogg)
Write-MediaFixture @('-i', $wav, '-c:a', 'aac', $m4a)

$webm = Convert-File $mp4 webm
Assert-Codecs $webm 'vp9' 'opus'
Assert-Codecs (Convert-File $webm mp4) 'h264' 'aac'
Assert-Codecs (Convert-File $mkv mp4) 'h264' 'aac'
Assert-Codecs (Convert-File $mkv webm) 'vp9' 'opus'
Assert-Codecs (Convert-File $mov mp4) 'h264' ''
$gifOut = Convert-File $mp4 gif
$gifHdr = [Text.Encoding]::ASCII.GetString(([IO.File]::ReadAllBytes($gifOut))[0..2])
Assert ($gifHdr -eq 'GIF') 'MP4 to GIF header mismatch.'
$extracted = Convert-File $mp4 mp3
Assert-Codecs $extracted '' 'mp3'
Convert-File $mov mp3 -ExpectFailure | Out-Null
Assert (-not (Test-Path -LiteralPath ([IO.Path]::ChangeExtension($mov, '.mp3')))) 'A silent video published an invalid MP3.'
$rewritten = Convert-File $extracted wav
Assert-Codecs $rewritten '' 'pcm_s16le'
$flac = Convert-File $wav flac
Assert-Codecs $flac '' 'flac'
Assert-Codecs (Convert-File $flac mp3) '' 'mp3'
Assert-Codecs (Convert-File $wav m4a) '' 'aac'
Assert-Codecs (Convert-File $wav ogg) '' 'opus'
Assert-Codecs (Convert-File $ogg wav) '' 'pcm_s16le'
Assert-Codecs (Convert-File $m4a flac) '' 'flac'
$compressedVideos = Convert-Files @($mp4, $mov) compress
Assert ($compressedVideos.Count -eq 2) 'Batch video compress did not return 2 outputs.'
foreach ($cv in $compressedVideos) {
    Assert ([IO.Path]::GetFileNameWithoutExtension($cv) -like '*-compressed*') "Compressed video missing -compressed suffix: $cv"
    $cvJson = & $ffprobe -v error -show_entries stream=codec_name,codec_type -of json $cv
    Assert ($LASTEXITCODE -eq 0) "ffprobe failed on compressed video: $cv"
    $cvVideo = (($cvJson | Out-String | ConvertFrom-Json).streams | Where-Object codec_type -eq 'video' | ForEach-Object { $_.codec_name }) -join ','
    Assert ($cvVideo -eq 'h264') "Compressed video codec mismatch: $cvVideo"
}
$missing = Convert-File $mp4 webm -ExpectFailure -NoFfmpeg
Assert ($missing -match 'FFmpeg') 'Missing FFmpeg did not report the media dependency.'
$corrupt = Join-Path $case 'broken.mp4'
[IO.File]::WriteAllText($corrupt, 'not a video')
Convert-File $corrupt webm -ExpectFailure | Out-Null
Assert (-not (Test-Path -LiteralPath (Join-Path $case 'broken.webm'))) 'A broken video published an output.'
Assert (@(Get-ChildItem -LiteralPath $case -Force -Filter '.convert-anythings-*').Count -eq 0) 'Media conversion left staging files behind.'
Write-Output "Media codecs verified with ffprobe. Fixtures: $case"
