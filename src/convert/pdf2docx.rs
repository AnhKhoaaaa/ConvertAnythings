use std::fmt::Write as _;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use windows::Win32::System::Threading::CREATE_NO_WINDOW;

use super::{Engines, Result};

pub fn convert(engines: &Engines, input: &Path, destination: &Path) -> Result<()> {
    let exe = engines
        .pdftotext
        .as_ref()
        .ok_or("Cần pdftotext trên PATH để chuyển PDF sang DOCX.")?;
    let output = Command::new(exe)
        .creation_flags(CREATE_NO_WINDOW.0)
        .args(["-enc", "UTF-8"])
        .arg(input)
        .arg("-")
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Không chạy được pdftotext. {error}"))?;
    if !output.status.success() {
        let diagnostic: String = String::from_utf8_lossy(&output.stderr)
            .trim()
            .chars()
            .take(300)
            .collect();
        return Err(format!("pdftotext không thể đọc PDF. {diagnostic}"));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let docx = text_to_docx_bytes(&text);
    fs::write(destination, docx).map_err(|error| error.to_string())
}

pub fn text_to_docx_bytes(text: &str) -> Vec<u8> {
    let document_xml = build_document_xml(text);
    pack_docx_bytes(&document_xml)
}

pub(crate) fn pack_docx_bytes(document_xml: &str) -> Vec<u8> {
    let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;

    write_zip_stored(&[
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        ("word/document.xml", document_xml.as_bytes()),
    ])
}

fn build_document_xml(text: &str) -> String {
    let mut body = String::new();
    let pages: Vec<&str> = text.split('\x0c').collect();
    let non_empty_pages: Vec<&str> = pages
        .iter()
        .copied()
        .filter(|page| !page.trim().is_empty())
        .collect();
    let pages_to_emit = if non_empty_pages.is_empty() {
        vec![""]
    } else {
        non_empty_pages
    };

    for (page_idx, page) in pages_to_emit.iter().enumerate() {
        if page_idx > 0 {
            body.push_str("<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>");
        }
        let normalized = page.replace("\r\n", "\n");
        let mut emitted = false;
        for line in normalized.lines() {
            let clean: String = line
                .chars()
                .filter(|&c| c == '\t' || !c.is_control())
                .collect();
            let trimmed = clean.trim_end();
            if trimmed.is_empty() {
                continue;
            }
            emitted = true;
            let _ = write!(
                body,
                "<w:p><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p>",
                escape_xml(trimmed)
            );
        }
        if !emitted {
            body.push_str("<w:p/>");
        }
    }

    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>{body}<w:sectPr><w:pgSz w:w="12240" w:h="15840"/></w:sectPr></w:body>
</w:document>"#
    )
}

pub(crate) fn escape_xml(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn write_zip_stored(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();

    for &(name, data) in entries {
        let name_bytes = name.as_bytes();
        let crc = crc32(data);
        let size = data.len() as u32;
        let offset = out.len() as u32;

        // Local file header (30 bytes + name)
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 filename flag
        out.extend_from_slice(&0u16.to_le_bytes()); // compression: STORED
        out.extend_from_slice(&0u16.to_le_bytes()); // mod time
        out.extend_from_slice(&0x5421u16.to_le_bytes()); // mod date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra len
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(data);

        // Central directory header (46 bytes + name)
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0x0800u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0x5421u16.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&size.to_le_bytes());
        central.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name_bytes);
    }

    let central_offset = out.len() as u32;
    let central_size = central.len() as u32;
    let count = entries.len() as u16;
    out.extend_from_slice(&central);

    // End of central directory (22 bytes)
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(&central_size.to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_valid_docx_zip_with_page_breaks() {
        let bytes = text_to_docx_bytes("Trang 1 <ok>\x0cTrang 2 & done");
        assert!(bytes.starts_with(b"PK\x03\x04"));
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("word/document.xml"));
        assert!(text.contains("Trang 1 &lt;ok&gt;"));
        assert!(text.contains("<w:br w:type=\"page\"/>"));
        assert!(text.contains("Trang 2 &amp; done"));
    }
}
