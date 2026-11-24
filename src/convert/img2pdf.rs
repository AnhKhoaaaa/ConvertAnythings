use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::{Engines, Format, Result, image};

struct PageImage {
    jpeg_bytes: Vec<u8>,
    width: u32,
    height: u32,
    components: u8,
    dpi_x: f64,
    dpi_y: f64,
}

pub fn merge(
    engines: &Engines,
    inputs: &[PathBuf],
    destination: &Path,
    stage: &Path,
) -> Result<()> {
    let mut pages = Vec::with_capacity(inputs.len());
    for (idx, input) in inputs.iter().enumerate() {
        pages.push(prepare_jpeg(engines, input, stage, idx)?);
    }
    write_pdf(&pages, destination)
}

pub fn write_jpeg_pages(pages: &[(PathBuf, f64, f64)], destination: &Path) -> Result<()> {
    if pages.is_empty() {
        return Err("Không có trang nào để tạo PDF.".to_owned());
    }
    let mut page_images = Vec::with_capacity(pages.len());
    for (path, dpi_x, dpi_y) in pages {
        let raw = fs::read(path).map_err(|error| error.to_string())?;
        let (w, h, comp) =
            parse_jpeg_sof(&raw).ok_or("Không đọc được kích thước JPEG khi tạo PDF.")?;
        page_images.push(PageImage {
            jpeg_bytes: raw,
            width: w,
            height: h,
            components: comp,
            dpi_x: *dpi_x,
            dpi_y: *dpi_y,
        });
    }
    write_pdf(&page_images, destination)
}

fn prepare_jpeg(
    engines: &Engines,
    input: &Path,
    stage: &Path,
    index: usize,
) -> Result<PageImage> {
    let source = input
        .extension()
        .and_then(Format::parse)
        .ok_or("Không nhận diện được định dạng ảnh.")?;

    let (dpi_x, dpi_y) = image::probe_dimensions_and_dpi(input)
        .map(|(_, _, dx, dy)| (dx.clamp(72.0, 600.0), dy.clamp(72.0, 600.0)))
        .unwrap_or((96.0, 96.0));

    if source == Format::Jpg
        && let Ok(raw) = fs::read(input)
        && let Some((w, h, comp)) = parse_jpeg_sof(&raw)
        && (comp == 1 || comp == 3)
    {
        return Ok(PageImage {
            jpeg_bytes: raw,
            width: w,
            height: h,
            components: comp,
            dpi_x,
            dpi_y,
        });
    }

    let tmp_jpg = stage.join(format!("page-{index}.jpg"));
    image::convert(engines, source, input, Format::Jpg, &tmp_jpg, stage)?;
    let raw = fs::read(&tmp_jpg).map_err(|error| error.to_string())?;
    let (w, h, comp) =
        parse_jpeg_sof(&raw).ok_or("Không đọc được kích thước JPEG khi tạo PDF.")?;
    Ok(PageImage {
        jpeg_bytes: raw,
        width: w,
        height: h,
        components: comp,
        dpi_x,
        dpi_y,
    })
}

fn parse_jpeg_sof(bytes: &[u8]) -> Option<(u32, u32, u8)> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return None;
    }
    let mut i = 2;
    while i + 1 < bytes.len() {
        if bytes[i] != 0xff {
            i += 1;
            continue;
        }
        while i < bytes.len() && bytes[i] == 0xff {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let marker = bytes[i];
        i += 1;
        // Standalone markers
        if marker == 0xd8 || marker == 0xd9 || (0xd0..=0xd7).contains(&marker) || marker == 0x01 {
            continue;
        }
        if marker == 0xda {
            // SOS reached without SOF
            break;
        }
        if i + 2 > bytes.len() {
            break;
        }
        let seg_len = u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
        if seg_len < 2 || i + seg_len > bytes.len() {
            break;
        }
        // Baseline (C0) or Progressive (C2) SOF
        if matches!(marker, 0xc0..=0xc2) && seg_len >= 8 {
            let h = u16::from_be_bytes([bytes[i + 3], bytes[i + 4]]) as u32;
            let w = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
            let comp = bytes[i + 7];
            if w > 0 && h > 0 {
                return Some((w, h, comp));
            }
            return None;
        }
        i += seg_len;
    }
    None
}

fn write_pdf(pages: &[PageImage], destination: &Path) -> Result<()> {
    let mut out: Vec<u8> = Vec::with_capacity(4096);
    out.extend_from_slice(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n");

    let mut offsets: Vec<usize> = Vec::with_capacity(2 + pages.len() * 3);
    let kids: Vec<String> = (0..pages.len())
        .map(|i| format!("{} 0 R", 3 + i * 3))
        .collect();

    // 1: Catalog
    offsets.push(out.len());
    let _ = writeln!(out, "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj");

    // 2: Pages
    offsets.push(out.len());
    let _ = writeln!(
        out,
        "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {} >>\nendobj",
        kids.join(" "),
        pages.len()
    );

    for (i, page) in pages.iter().enumerate() {
        let page_id = 3 + i * 3;
        let img_id = page_id + 1;
        let content_id = page_id + 2;

        let pt_w = (f64::from(page.width) * 72.0 / page.dpi_x).max(1.0);
        let pt_h = (f64::from(page.height) * 72.0 / page.dpi_y).max(1.0);
        let cs = if page.components == 1 {
            "/DeviceGray"
        } else {
            "/DeviceRGB"
        };

        // Page object
        offsets.push(out.len());
        let _ = writeln!(
            out,
            "{page_id} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pt_w:.2} {pt_h:.2}] /Resources << /XObject << /Im0 {img_id} 0 R >> >> /Contents {content_id} 0 R >>\nendobj"
        );

        // Image XObject
        offsets.push(out.len());
        let _ = write!(
            out,
            "{img_id} 0 obj\n<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace {cs} /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
            page.width,
            page.height,
            page.jpeg_bytes.len()
        );
        out.extend_from_slice(&page.jpeg_bytes);
        out.extend_from_slice(b"\nendstream\nendobj\n");

        // Content stream
        let draw_cmd = format!("q\n{pt_w:.2} 0 0 {pt_h:.2} 0 0 cm\n/Im0 Do\nQ\n");
        offsets.push(out.len());
        let _ = write!(
            out,
            "{content_id} 0 obj\n<< /Length {} >>\nstream\n{draw_cmd}endstream\nendobj\n",
            draw_cmd.len()
        );
    }

    let xref = out.len();
    let total_objs = offsets.len() + 1;
    let _ = writeln!(out, "xref\n0 {total_objs}");
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in offsets {
        let _ = writeln!(out, "{offset:010} 00000 n ");
    }
    let _ = writeln!(
        out,
        "trailer\n<< /Size {total_objs} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF"
    );

    fs::write(destination, out).map_err(|error| error.to_string())
}
