$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$source = $env:CONVERT_WORD_INPUT
$destination = $env:CONVERT_WORD_OUTPUT
$appKind = $env:CONVERT_OFFICE_APP
$ext = [IO.Path]::GetExtension($source).ToLowerInvariant()
$missing = [Type]::Missing

if ($appKind -eq 'word' -and $ext -eq '.docx') {
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

if ($appKind -eq 'excel') {
    $previous = @([Diagnostics.Process]::GetProcessesByName('EXCEL') | ForEach-Object { $_.Id })
    $excel = $null
    $wb = $null
    $owned = $false
    $security = $null
    try {
        $excel = New-Object -ComObject Excel.Application
        if ($excel.Workbooks.Count -ne 0) { throw 'Could not create an independent Excel session.' }
        $created = @([Diagnostics.Process]::GetProcessesByName('EXCEL') | Where-Object { $previous -notcontains $_.Id })
        if ($created.Count -ne 1) { throw 'Could not confirm the new Excel session.' }
        $owned = $true
        $excel.Visible = $false
        $excel.DisplayAlerts = $false
        $security = $excel.AutomationSecurity
        $excel.AutomationSecurity = 3
        $wb = $excel.Workbooks.Open($source, 0, $true)
        $wb.ExportAsFixedFormat(0, $destination)
    } finally {
        if ($wb) { try { $wb.Close($false) } catch {} }
        if ($owned) {
            if ($null -ne $security) { try { $excel.AutomationSecurity = $security } catch {} }
            try { $excel.Quit() } catch {}
        }
    }
    exit 0
}

if ($appKind -eq 'powerpoint') {
    $previous = @([Diagnostics.Process]::GetProcessesByName('POWERPNT') | ForEach-Object { $_.Id })
    $ppt = $null
    $pres = $null
    $owned = $false
    $security = $null
    try {
        $ppt = New-Object -ComObject PowerPoint.Application
        if ($ppt.Presentations.Count -ne 0) { throw 'Could not create an independent PowerPoint session.' }
        $created = @([Diagnostics.Process]::GetProcessesByName('POWERPNT') | Where-Object { $previous -notcontains $_.Id })
        if ($created.Count -ne 1) { throw 'Could not confirm the new PowerPoint session.' }
        $owned = $true
        $ppt.DisplayAlerts = 1
        $security = $ppt.AutomationSecurity
        $ppt.AutomationSecurity = 3
        $pres = $ppt.Presentations.Open($source, -1, 0, 0)
        $pres.SaveAs($destination, 32)
    } finally {
        if ($pres) { try { $pres.Close() } catch {} }
        if ($owned) {
            if ($null -ne $security) { try { $ppt.AutomationSecurity = $security } catch {} }
            try { $ppt.Quit() } catch {}
        }
    }
    exit 0
}

$previous = @([Diagnostics.Process]::GetProcessesByName('WINWORD') | ForEach-Object { $_.Id })
$word = $null
$doc = $null
$owned = $false
$security = $null

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
    if ($ext -eq '.txt') {
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
