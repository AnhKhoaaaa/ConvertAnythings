use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::Win32::System::Threading::CREATE_NO_WINDOW;

use super::pdf2docx::{escape_xml, pack_docx_bytes};
use super::{Engines, Format, Result};

pub fn convert(input: &Path, destination: &Path) -> Result<()> {
    let text = read_text(input)?;
    let title = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Document");
    let html = render_html(title, &text);
    fs::write(destination, html).map_err(|error| error.to_string())
}

pub fn convert_docx(input: &Path, destination: &Path) -> Result<()> {
    let text = read_text(input)?;
    let docx = render_docx_bytes(&text);
    fs::write(destination, docx).map_err(|error| error.to_string())
}

pub fn convert_pdf(
    engines: &Engines,
    input: &Path,
    destination: &Path,
    stage: &Path,
) -> Result<()> {
    if let Some(browser) = engines.browser_path() {
        let text = read_text(input)?;
        let title = input
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Document");
        let html = render_html_for_print(title, &text);
        let html_path = stage.join("render.html");
        fs::write(&html_path, html).map_err(|error| error.to_string())?;
        return print_html_to_pdf(browser, &html_path, destination, stage);
    }

    if engines.word_available() {
        let intermediate = stage.join("source.docx");
        convert_docx(input, &intermediate)?;
        return super::word::convert(&intermediate, Format::Docx, destination, stage);
    }

    Err("Cần Microsoft Edge, Chrome hoặc Word Desktop để chuyển Markdown sang PDF.".to_owned())
}

fn print_html_to_pdf(
    browser: &Path,
    html_path: &Path,
    destination: &Path,
    stage: &Path,
) -> Result<()> {
    let profile = TempProfileDir::new()?;
    let log_path = stage.join("browser.log");
    let log = File::create(&log_path).map_err(|error| error.to_string())?;

    let mut user_data_arg = OsString::from("--user-data-dir=");
    user_data_arg.push(profile.0.as_os_str());
    let mut pdf_arg = OsString::from("--print-to-pdf=");
    pdf_arg.push(destination.as_os_str());
    let url = path_to_file_url(html_path);

    let mut child = Command::new(browser)
        .creation_flags(CREATE_NO_WINDOW.0)
        .args([
            "--headless=new",
            "--disable-gpu",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-extensions",
            "--disable-background-networking",
            "--disable-crash-reporter",
            "--no-pdf-header-footer",
        ])
        .arg(&user_data_arg)
        .arg(&pdf_arg)
        .arg(&url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .map_err(|error| format!("Không chạy được trình duyệt headless. {error}"))?;

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match child.try_wait().map_err(|error| error.to_string())? {
            Some(status) => {
                if status.success() && is_valid_pdf_file(destination) {
                    return Ok(());
                }
                let detail: String = fs::read_to_string(&log_path)
                    .unwrap_or_default()
                    .trim()
                    .chars()
                    .take(300)
                    .collect();
                return Err(if detail.is_empty() {
                    "Trình duyệt không xuất được tệp PDF.".to_owned()
                } else {
                    format!("Trình duyệt không xuất được tệp PDF. {detail}")
                });
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Xuất PDF từ Markdown quá thời gian chờ (30 giây).".to_owned());
            }
            None => std::thread::sleep(Duration::from_millis(60)),
        }
    }
}

fn is_valid_pdf_file(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path) else {
        return false;
    };
    bytes.starts_with(b"%PDF-")
}

struct TempProfileDir(PathBuf);

impl TempProfileDir {
    fn new() -> Result<Self> {
        let base = std::env::temp_dir();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for count in 0..32 {
            let path = base.join(format!(
                "convert-anythings-browser-{}-{now:x}-{count}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!("Không tạo được thư mục tạm cho trình duyệt. {error}"));
                }
            }
        }
        Err("Không tạo được thư mục tạm cho trình duyệt.".to_owned())
    }
}

impl Drop for TempProfileDir {
    fn drop(&mut self) {
        for attempt in 0..6 {
            if fs::remove_dir_all(&self.0).is_ok() || !self.0.exists() {
                return;
            }
            if attempt < 5 {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

pub fn path_to_file_url(path: &Path) -> String {
    let raw = path.to_string_lossy();
    let stripped = if let Some(unc) = raw.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else if let Some(local) = raw.strip_prefix(r"\\?\") {
        local.to_owned()
    } else {
        raw.into_owned()
    };

    let slashed = stripped.replace('\\', "/");
    let mut url = String::with_capacity(slashed.len() + 16);
    if let Some(unc_rest) = slashed.strip_prefix("//") {
        url.push_str("file://");
        percent_encode_path(&mut url, unc_rest, false);
    } else {
        url.push_str("file:///");
        let is_drive = slashed.len() >= 2
            && slashed.as_bytes()[0].is_ascii_alphabetic()
            && slashed.as_bytes()[1] == b':';
        percent_encode_path(&mut url, slashed.trim_start_matches('/'), is_drive);
    }
    url
}

fn percent_encode_path(out: &mut String, input: &str, preserve_first_colon: bool) {
    for (idx, byte) in input.bytes().enumerate() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(byte, b'/' | b'-' | b'.' | b'_' | b'~')
            || (preserve_first_colon && idx == 1 && byte == b':');
        if keep {
            out.push(byte as char);
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
}

fn read_text(input: &Path) -> Result<String> {
    let bytes = fs::read(input).map_err(|error| error.to_string())?;
    match bytes.as_slice() {
        [0xff, 0xfe, rest @ ..] | [0xfe, 0xff, rest @ ..] => {
            if !rest.len().is_multiple_of(2) {
                return Err("Tệp Markdown UTF-16 không đầy đủ.".to_owned());
            }
            let le = bytes[0] == 0xff;
            let units: Vec<u16> = rest
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&u| if le { u16::from_le_bytes(u) } else { u16::from_be_bytes(u) })
                .collect();
            String::from_utf16(&units).map_err(|_| "Tệp Markdown UTF-16 không hợp lệ.".to_owned())
        }
        slice => std::str::from_utf8(slice.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(slice))
            .map(str::to_owned)
            .map_err(|_| "Markdown cần dùng UTF-8 hoặc UTF-16.".to_owned()),
    }
}

pub fn render_html(title: &str, markdown: &str) -> String {
    render_html_document(title, markdown, true)
}

pub fn render_html_for_print(title: &str, markdown: &str) -> String {
    render_html_document(title, markdown, false)
}

fn render_html_document(title: &str, markdown: &str, allow_dark_mode: bool) -> String {
    let body = render_body(markdown);
    let escaped_title = escape_html(title);
    let color_scheme_css = if allow_dark_mode {
        r#"  :root { color-scheme: light dark; }
  @media (prefers-color-scheme: dark) {
    body { color: #e6edf3; background: #0d1117; }
    pre, code { background: #161b22 !important; }
    blockquote { color: #8b949e; border-color: #30363d; }
    a { color: #58a6ff; }
    hr { border-color: #30363d; }
  }"#
    } else {
        r#"  @page { margin: 18mm; }
  :root { color-scheme: light; }
  body {
    max-width: none;
    margin: 0;
    padding: 0;
    -webkit-print-color-adjust: exact;
    print-color-adjust: exact;
  }
  pre, blockquote { break-inside: avoid; }
  h1, h2, h3, h4 { break-after: avoid; }"#
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="vi">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>{escaped_title}</title>
<style>
  body {{
    font-family: "Segoe UI", -apple-system, BlinkMacSystemFont, sans-serif;
    line-height: 1.65;
    max-width: 780px;
    margin: 2.5rem auto;
    padding: 0 1.25rem;
    color: #1f2328;
    background: #ffffff;
  }}
{color_scheme_css}
  h1, h2, h3, h4 {{ line-height: 1.3; margin-top: 1.5em; }}
  h1 {{ border-bottom: 1px solid #d0d7de; padding-bottom: 0.3em; }}
  code {{
    font-family: "Cascadia Code", Consolas, monospace;
    background: #f6f8fa;
    padding: 0.15em 0.4em;
    border-radius: 4px;
    font-size: 0.9em;
  }}
  pre {{
    background: #f6f8fa;
    padding: 1rem;
    border-radius: 8px;
    overflow-x: auto;
  }}
  pre code {{ background: none; padding: 0; }}
  blockquote {{
    margin: 1em 0;
    padding: 0.25em 1em;
    border-left: 4px solid #d0d7de;
    color: #57606a;
  }}
  img {{ max-width: 100%; border-radius: 6px; }}
  hr {{ border: none; border-top: 1px solid #d0d7de; margin: 1.5rem 0; }}
</style>
</head>
<body>
{body}</body>
</html>
"#
    )
}

pub fn render_docx_bytes(markdown: &str) -> Vec<u8> {
    let document_xml = render_docx_xml(markdown);
    pack_docx_bytes(&document_xml)
}

#[derive(Clone, Copy)]
struct DocxStyle {
    bold: bool,
    italic: bool,
    code: bool,
    underline: bool,
    color: Option<&'static str>,
    size: u16,
}

impl Default for DocxStyle {
    fn default() -> Self {
        Self {
            bold: false,
            italic: false,
            code: false,
            underline: false,
            color: None,
            size: 22,
        }
    }
}

fn render_docx_xml(markdown: &str) -> String {
    let mut body = String::new();
    let lines: Vec<&str> = markdown.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let trimmed = lines[i].trim();

        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        // Fenced code block
        if trimmed.starts_with("```") {
            i += 1;
            let mut has_lines = false;
            while i < lines.len() && !lines[i].trim().starts_with("```") {
                has_lines = true;
                body.push_str("<w:p><w:pPr><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F6F8FA\"/><w:ind w:left=\"180\" w:right=\"180\"/><w:spacing w:before=\"20\" w:after=\"20\"/></w:pPr>");
                let code_style = DocxStyle {
                    code: true,
                    size: 20,
                    color: Some("1F2328"),
                    ..Default::default()
                };
                push_docx_run(&mut body, lines[i], code_style);
                body.push_str("</w:p>");
                i += 1;
            }
            if !has_lines {
                body.push_str("<w:p/>");
            }
            i += 1;
            continue;
        }

        // Horizontal rule
        if is_hr(trimmed) {
            body.push_str("<w:p><w:pPr><w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"1\" w:color=\"D0D7DE\"/></w:pBdr><w:spacing w:before=\"120\" w:after=\"120\"/></w:pPr></w:p>");
            i += 1;
            continue;
        }

        // Headings
        if let Some((level, heading)) = parse_heading(trimmed) {
            let size = match level {
                1 => 40,
                2 => 32,
                3 => 26,
                4 => 24,
                _ => 22,
            };
            let border = if level == 1 {
                "<w:pBdr><w:bottom w:val=\"single\" w:sz=\"6\" w:space=\"4\" w:color=\"D0D7DE\"/></w:pBdr>"
            } else {
                ""
            };
            let _ = write!(
                body,
                "<w:p><w:pPr>{border}<w:spacing w:before=\"200\" w:after=\"100\"/></w:pPr>"
            );
            let style = DocxStyle {
                bold: true,
                size,
                color: Some("1F2328"),
                ..Default::default()
            };
            render_docx_inline(&mut body, heading, style);
            body.push_str("</w:p>");
            i += 1;
            continue;
        }

        // Blockquote
        if trimmed.starts_with('>') {
            while i < lines.len() && lines[i].trim().starts_with('>') {
                let content = lines[i].trim().trim_start_matches('>').trim_start();
                if !content.is_empty() {
                    body.push_str("<w:p><w:pPr><w:pBdr><w:left w:val=\"single\" w:sz=\"18\" w:space=\"10\" w:color=\"D0D7DE\"/></w:pBdr><w:ind w:left=\"420\"/><w:spacing w:before=\"40\" w:after=\"80\"/></w:pPr>");
                    let quote_style = DocxStyle {
                        italic: true,
                        color: Some("57606A"),
                        ..Default::default()
                    };
                    render_docx_inline(&mut body, content, quote_style);
                    body.push_str("</w:p>");
                }
                i += 1;
            }
            continue;
        }

        // Unordered list
        if is_ul_item(trimmed).is_some() {
            while i < lines.len() {
                if let Some(item) = is_ul_item(lines[i].trim()) {
                    body.push_str("<w:p><w:pPr><w:ind w:left=\"540\" w:hanging=\"270\"/><w:spacing w:before=\"30\" w:after=\"50\"/></w:pPr>");
                    let style = DocxStyle::default();
                    push_docx_run(&mut body, "• ", style);
                    render_docx_inline(&mut body, item, style);
                    body.push_str("</w:p>");
                    i += 1;
                } else {
                    break;
                }
            }
            continue;
        }

        // Ordered list
        if is_ol_item(trimmed).is_some() {
            let mut number = 1usize;
            while i < lines.len() {
                if let Some(item) = is_ol_item(lines[i].trim()) {
                    body.push_str("<w:p><w:pPr><w:ind w:left=\"540\" w:hanging=\"320\"/><w:spacing w:before=\"30\" w:after=\"50\"/></w:pPr>");
                    let style = DocxStyle::default();
                    push_docx_run(&mut body, &format!("{number}. "), style);
                    render_docx_inline(&mut body, item, style);
                    body.push_str("</w:p>");
                    number += 1;
                    i += 1;
                } else {
                    break;
                }
            }
            continue;
        }

        // Paragraph
        body.push_str("<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"140\"/></w:pPr>");
        let mut first_line = true;
        while i < lines.len() {
            let cur = lines[i].trim();
            if cur.is_empty()
                || cur.starts_with("```")
                || cur.starts_with('>')
                || is_hr(cur)
                || parse_heading(cur).is_some()
                || is_ul_item(cur).is_some()
                || is_ol_item(cur).is_some()
            {
                break;
            }
            if !first_line {
                body.push_str("<w:r><w:br/></w:r>");
            }
            first_line = false;
            render_docx_inline(&mut body, cur, DocxStyle::default());
            i += 1;
        }
        body.push_str("</w:p>");
    }

    if body.is_empty() {
        body.push_str("<w:p/>");
    }

    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>{body}<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr></w:body>
</w:document>"#
    )
}

fn render_docx_inline(out: &mut String, text: &str, base: DocxStyle) {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut plain_start = 0;

    let flush_plain = |out: &mut String, start: usize, end: usize| {
        if end > start {
            push_docx_run(out, &text[start..end], base);
        }
    };

    while i < bytes.len() {
        // Inline code
        if bytes[i] == b'`'
            && let Some(end) = text[i + 1..].find('`')
        {
            flush_plain(out, plain_start, i);
            let code = &text[i + 1..i + 1 + end];
            let style = DocxStyle {
                code: true,
                size: base.size.saturating_sub(2).max(16),
                ..base
            };
            push_docx_run(out, code, style);
            i += end + 2;
            plain_start = i;
            continue;
        }

        // Image ![alt](url)
        if text[i..].starts_with("![")
            && let Some((alt, _url, consumed)) = parse_link(&text[i + 1..])
        {
            flush_plain(out, plain_start, i);
            let label = if alt.trim().is_empty() {
                "[Image]".to_owned()
            } else {
                format!("[Image: {alt}]")
            };
            push_docx_run(
                out,
                &label,
                DocxStyle {
                    italic: true,
                    color: Some("57606A"),
                    ..base
                },
            );
            i += 1 + consumed;
            plain_start = i;
            continue;
        }

        // Link [text](url)
        if bytes[i] == b'['
            && let Some((label, _url, consumed)) = parse_link(&text[i..])
        {
            flush_plain(out, plain_start, i);
            render_docx_inline(
                out,
                label,
                DocxStyle {
                    underline: true,
                    color: Some("0969DA"),
                    ..base
                },
            );
            i += consumed;
            plain_start = i;
            continue;
        }

        // Bold **...**
        if text[i..].starts_with("**")
            && let Some(end) = text[i + 2..].find("**")
        {
            flush_plain(out, plain_start, i);
            let inner = &text[i + 2..i + 2 + end];
            render_docx_inline(
                out,
                inner,
                DocxStyle {
                    bold: true,
                    ..base
                },
            );
            i += end + 4;
            plain_start = i;
            continue;
        }

        // Italic *...*
        if bytes[i] == b'*'
            && let Some(end) = text[i + 1..].find('*')
        {
            let inner = &text[i + 1..i + 1 + end];
            if !inner.is_empty() {
                flush_plain(out, plain_start, i);
                render_docx_inline(
                    out,
                    inner,
                    DocxStyle {
                        italic: true,
                        ..base
                    },
                );
                i += end + 2;
                plain_start = i;
                continue;
            }
        }

        let ch = text[i..].chars().next().expect("valid char");
        i += ch.len_utf8();
    }

    flush_plain(out, plain_start, bytes.len());
}

fn push_docx_run(out: &mut String, text: &str, style: DocxStyle) {
    let clean: String = text
        .chars()
        .filter(|&c| c == '\t' || !c.is_control())
        .collect();
    let font = if style.code { "Consolas" } else { "Segoe UI" };
    let _ = write!(
        out,
        "<w:r><w:rPr><w:rFonts w:ascii=\"{font}\" w:hAnsi=\"{font}\" w:cs=\"{font}\"/><w:sz w:val=\"{}\"/><w:szCs w:val=\"{}\"/>",
        style.size, style.size
    );
    if style.bold {
        out.push_str("<w:b/>");
    }
    if style.italic {
        out.push_str("<w:i/>");
    }
    if style.underline {
        out.push_str("<w:u w:val=\"single\"/>");
    }
    if let Some(color) = style.color {
        let _ = write!(out, "<w:color w:val=\"{color}\"/>");
    }
    if style.code {
        out.push_str("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F3F4F6\"/>");
    }
    let _ = write!(
        out,
        "</w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>",
        escape_xml(&clean)
    );
}

fn render_body(markdown: &str) -> String {
    let mut out = String::new();
    let lines: Vec<&str> = markdown.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        // Fenced code block
        if trimmed.starts_with("```") {
            let lang = trimmed.trim_start_matches('`').trim();
            if lang.is_empty() {
                out.push_str("<pre><code>");
            } else {
                let _ = write!(out, "<pre><code class=\"language-{}\">", escape_html(lang));
            }
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("```") {
                out.push_str(&escape_html(lines[i]));
                out.push('\n');
                i += 1;
            }
            out.push_str("</code></pre>\n");
            i += 1;
            continue;
        }

        // Horizontal rule
        if is_hr(trimmed) {
            out.push_str("<hr />\n");
            i += 1;
            continue;
        }

        // Headings
        if let Some((level, heading)) = parse_heading(trimmed) {
            let _ = writeln!(out, "<h{level}>{}</h{level}>", render_inline(heading));
            i += 1;
            continue;
        }

        // Blockquote
        if trimmed.starts_with('>') {
            out.push_str("<blockquote>\n");
            while i < lines.len() && lines[i].trim().starts_with('>') {
                let content = lines[i].trim().trim_start_matches('>').trim_start();
                if !content.is_empty() {
                    let _ = writeln!(out, "<p>{}</p>", render_inline(content));
                }
                i += 1;
            }
            out.push_str("</blockquote>\n");
            continue;
        }

        // Unordered list
        if is_ul_item(trimmed).is_some() {
            out.push_str("<ul>\n");
            while i < lines.len() {
                if let Some(item) = is_ul_item(lines[i].trim()) {
                    let _ = writeln!(out, "<li>{}</li>", render_inline(item));
                    i += 1;
                } else {
                    break;
                }
            }
            out.push_str("</ul>\n");
            continue;
        }

        // Ordered list
        if is_ol_item(trimmed).is_some() {
            out.push_str("<ol>\n");
            while i < lines.len() {
                if let Some(item) = is_ol_item(lines[i].trim()) {
                    let _ = writeln!(out, "<li>{}</li>", render_inline(item));
                    i += 1;
                } else {
                    break;
                }
            }
            out.push_str("</ol>\n");
            continue;
        }

        // Paragraph
        let mut para = Vec::new();
        while i < lines.len() {
            let cur = lines[i].trim();
            if cur.is_empty()
                || cur.starts_with("```")
                || cur.starts_with('>')
                || is_hr(cur)
                || parse_heading(cur).is_some()
                || is_ul_item(cur).is_some()
                || is_ol_item(cur).is_some()
            {
                break;
            }
            para.push(render_inline(cur));
            i += 1;
        }
        let _ = writeln!(out, "<p>{}</p>", para.join("<br />\n"));
    }

    out
}

fn parse_heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line.bytes().take_while(|&b| b == b'#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &line[hashes..];
    rest.strip_prefix(' ').map(|text| (hashes, text.trim()))
}

fn is_hr(line: &str) -> bool {
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    compact.len() >= 3
        && (compact.chars().all(|c| c == '-')
            || compact.chars().all(|c| c == '*')
            || compact.chars().all(|c| c == '_'))
}

fn is_ul_item(line: &str) -> Option<&str> {
    line.strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))
        .map(str::trim)
}

fn is_ol_item(line: &str) -> Option<&str> {
    let digits = line.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    line[digits..].strip_prefix(". ").map(str::trim)
}

fn render_inline(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;

    while i < bytes.len() {
        // Inline code
        if bytes[i] == b'`'
            && let Some(end) = text[i + 1..].find('`')
        {
            let code = &text[i + 1..i + 1 + end];
            let _ = write!(out, "<code>{}</code>", escape_html(code));
            i += end + 2;
            continue;
        }

        // Image ![alt](url)
        if text[i..].starts_with("![")
            && let Some((alt, url, consumed)) = parse_link(&text[i + 1..])
        {
            let _ = write!(
                out,
                "<img src=\"{}\" alt=\"{}\" />",
                escape_html(url),
                escape_html(alt)
            );
            i += 1 + consumed;
            continue;
        }

        // Link [text](url)
        if bytes[i] == b'['
            && let Some((label, url, consumed)) = parse_link(&text[i..])
        {
            let _ = write!(
                out,
                "<a href=\"{}\">{}</a>",
                escape_html(url),
                render_inline(label)
            );
            i += consumed;
            continue;
        }

        // Bold **...**
        if text[i..].starts_with("**")
            && let Some(end) = text[i + 2..].find("**")
        {
            let inner = &text[i + 2..i + 2 + end];
            let _ = write!(out, "<strong>{}</strong>", render_inline(inner));
            i += end + 4;
            continue;
        }

        // Italic *...*
        if bytes[i] == b'*'
            && let Some(end) = text[i + 1..].find('*')
        {
            let inner = &text[i + 1..i + 1 + end];
            if !inner.is_empty() {
                let _ = write!(out, "<em>{}</em>", render_inline(inner));
                i += end + 2;
                continue;
            }
        }

        let ch = text[i..].chars().next().expect("valid char");
        push_escaped_char(&mut out, ch);
        i += ch.len_utf8();
    }

    out
}

fn parse_link(slice: &str) -> Option<(&str, &str, usize)> {
    let close_bracket = slice.find("](")?;
    let label = &slice[1..close_bracket];
    let after_paren = close_bracket + 2;
    let close_paren = slice[after_paren..].find(')')?;
    let url = &slice[after_paren..after_paren + close_paren];
    Some((label, url, after_paren + close_paren + 1))
}

fn escape_html(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        push_escaped_char(&mut out, ch);
    }
    out
}

fn push_escaped_char(out: &mut String, ch: char) {
    match ch {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '"' => out.push_str("&quot;"),
        '\'' => out.push_str("&#39;"),
        _ => out.push(ch),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_headings_lists_bold_and_code() {
        let md = "# Tiêu đề\n\nĐoạn văn có **đậm**, *nghiêng*, và `code`.\n\n- Mục 1\n- Mục 2\n";
        let html = render_html("Doc", md);
        assert!(html.contains("<h1>Tiêu đề</h1>"));
        assert!(html.contains("<strong>đậm</strong>"));
        assert!(html.contains("<em>nghiêng</em>"));
        assert!(html.contains("<code>code</code>"));
        assert!(html.contains("<ul>\n<li>Mục 1</li>\n<li>Mục 2</li>\n</ul>"));
    }

    #[test]
    fn renders_docx_zip_with_markdown_formatting() {
        let md = "# Tiếng Việt\n\nĐoạn **đậm**, *nghiêng*, `code`, [link](https://example.com).\n\n> Trích dẫn\n\n- Mục A\n1. Bước 1\n\n```rust\nlet x = 1;\n```\n";
        let bytes = render_docx_bytes(md);
        assert!(bytes.starts_with(b"PK\x03\x04"));
        let xml = String::from_utf8_lossy(&bytes);
        assert!(xml.contains("word/document.xml"));
        assert!(xml.contains("Tiếng Việt"));
        assert!(xml.contains("<w:b/>"));
        assert!(xml.contains("<w:i/>"));
        assert!(xml.contains("Consolas"));
        assert!(xml.contains("• "));
        assert!(xml.contains("1. "));
        assert!(xml.contains("let x = 1;"));
    }

    #[test]
    fn renders_print_html_in_light_mode() {
        let html = render_html_for_print("Report", "# Báo cáo\n\nNội dung.");
        assert!(html.contains("@page { margin: 18mm; }"));
        assert!(html.contains("color-scheme: light;"));
        assert!(!html.contains("prefers-color-scheme: dark"));
    }

    #[test]
    fn converts_windows_paths_to_percent_encoded_file_urls() {
        assert_eq!(
            path_to_file_url(Path::new(r"\\?\D:\Thư mục\render file.html")),
            "file:///D:/Th%C6%B0%20m%E1%BB%A5c/render%20file.html"
        );
        assert_eq!(
            path_to_file_url(Path::new(r"C:\plain\test[1].html")),
            "file:///C:/plain/test%5B1%5D.html"
        );
    }
}
