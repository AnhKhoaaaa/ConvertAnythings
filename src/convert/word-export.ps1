$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$source = $env:CONVERT_WORD_INPUT
$destination = $env:CONVERT_WORD_OUTPUT
$plainText = [IO.Path]::GetExtension($source) -ieq '.txt'

if (-not $plainText) {
    Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
    $archive = [IO.Compression.ZipFile]::OpenRead($source)
    try {
        if (-not $archive.GetEntry('word/document.xml')) { throw 'This is not a readable DOCX file.' }
        foreach ($entry in $archive.Entries) {
            if (-not $entry.FullName.EndsWith('.rels', [StringComparison]::OrdinalIgnoreCase)) { continue }
            $inputStream = $entry.Open()
            try {
                $settings = New-Object Xml.XmlReaderSettings
                $settings.DtdProcessing = [Xml.DtdProcessing]::Prohibit
                $settings.MaxCharactersInDocument = 4000000
                $reader = [Xml.XmlReader]::Create($inputStream, $settings)
                try {
                    while ($reader.Read()) {
                        if ($reader.NodeType -eq [Xml.XmlNodeType]::Element -and $reader.LocalName -eq 'Relationship' -and
                            $reader.GetAttribute('TargetMode') -ieq 'External' -and
                            $reader.GetAttribute('Type') -notlike '*/hyperlink') {
                            throw 'DOCX uses linked external resources. Embed them in the document before exporting offline.'
                        }
                    }
                } finally { $reader.Dispose() }
            } finally { $inputStream.Dispose() }
        }
    } finally { $archive.Dispose() }
}

$previous = @([Diagnostics.Process]::GetProcessesByName('WINWORD') | ForEach-Object { $_.Id })
$word = $null
$doc = $null
$owned = $false
$security = $null
$missing = [Type]::Missing

try {
    $word = New-Object -ComObject Word.Application
    if ($word.Documents.Count -ne 0) { throw 'Could not create an independent Word session.' }
    $created = @([Diagnostics.Process]::GetProcessesByName('WINWORD') | Where-Object { $previous -notcontains $_.Id })
    if ($created.Count -ne 1) { throw 'Could not confirm the new Word session.' }
    $owned = $true
    $word.Visible = $false
    $word.DisplayAlerts = 0
    $security = $word.AutomationSecurity
    $word.AutomationSecurity = 3
    if ($plainText) {
        $doc = $word.Documents.Add($missing, $false, 0, $false)
        $doc.Content.Text = [IO.File]::ReadAllText($source, [Text.Encoding]::UTF8).Replace("`r`n", "`r").Replace("`n", "`r")
    } else {
        $doc = $word.Documents.OpenNoRepairDialog($source, $false, $true, $false)
    }
    $doc.ExportAsFixedFormat($destination, 17, $false)
} finally {
    if ($doc) { try { $doc.Close([ref]0) } catch {} }
    if ($owned) {
        if ($null -ne $security) { try { $word.AutomationSecurity = $security } catch {} }
        try { $word.Quit([ref]0) } catch {}
    }
}
