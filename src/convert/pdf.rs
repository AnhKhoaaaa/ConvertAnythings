use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use windows::Data::Pdf::{PdfDocument, PdfPage, PdfPageRenderOptions};
use windows::Graphics::Imaging::BitmapEncoder;
use windows::Storage::Streams::FileRandomAccessStream;
use windows::Storage::{FileAccessMode, StorageFile};
use windows::UI::Color;
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
use windows::core::HSTRING;

use super::{Engines, Format, Result, execute, image, img2pdf};

fn pdf_error(error: windows::core::Error) -> String {
    format!("Windows không thể đọc hoặc xuất PDF. {error}")
}

pub fn convert(
    engines: &Engines,
    input: &Path,
    output: Format,
    destination: &Path,
    stage: &Path,
    stem: &OsStr,
) -> Result<bool> {
    unsafe {
        RoInitialize(RO_INIT_MULTITHREADED).map_err(pdf_error)?;
    }
    let _runtime = Runtime;
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(input.as_os_str()))
        .and_then(|operation| operation.join())
        .map_err(pdf_error)?;
    let document = PdfDocument::LoadFromFileAsync(&file)
        .and_then(|operation| operation.join())
        .map_err(pdf_error)?;
    let pages = document.PageCount().map_err(pdf_error)?;
    if pages == 0 {
        return Err("PDF không có trang nào.".to_owned());
    }
    let raster = stage.join("pdf-render.png");
    if pages == 1 {
        let page = document.GetPage(0).map_err(pdf_error)?;
        render(engines, &page, output, destination, &raster, stage)?;
        return Ok(false);
    }
    let folder = stage.join("pages");
    fs::create_dir(&folder).map_err(|error| error.to_string())?;
    let digit_count = pages.to_string().len().max(3);
    for index in 0..pages {
        let mut name = stem.to_os_string();
        name.push(format!(
            "-{:0digit_count$}.{}",
            index + 1,
            output.extension()
        ));
        let page = document.GetPage(index).map_err(pdf_error)?;
        render(engines, &page, output, &folder.join(name), &raster, stage)?;
    }
    Ok(true)
}

fn render(
    engines: &Engines,
    page: &PdfPage,
    output: Format,
    path: &Path,
    raster: &Path,
    stage: &Path,
) -> Result<()> {
    let size = page.Size().map_err(pdf_error)?;
    let size_in_pixels = |dip: f32| -> Result<u32> {
        let pixels = (f64::from(dip) * 150.0 / 96.0).round();
        if !(1.0..=f64::from(u32::MAX)).contains(&pixels) {
            return Err("Kích thước trang PDF không hợp lệ.".to_owned());
        }
        Ok(pixels as u32)
    };
    let width = size_in_pixels(size.Width)?;
    let height = size_in_pixels(size.Height)?;
    let options = PdfPageRenderOptions::new().map_err(pdf_error)?;
    options.SetDestinationWidth(width).map_err(pdf_error)?;
    options.SetDestinationHeight(height).map_err(pdf_error)?;
    options
        .SetBackgroundColor(Color {
            A: 255,
            R: 255,
            G: 255,
            B: 255,
        })
        .map_err(pdf_error)?;
    let container = BitmapEncoder::PngEncoderId().map_err(pdf_error)?;
    options.SetBitmapEncoderId(container).map_err(pdf_error)?;
    File::create(raster).map_err(|error| error.to_string())?;
    let stream = FileRandomAccessStream::OpenAsync(
        &HSTRING::from(raster.as_os_str()),
        FileAccessMode::ReadWrite,
    )
    .and_then(|operation| operation.join())
    .map_err(pdf_error)?;
    let result = page
        .RenderWithOptionsToStreamAsync(&stream, &options)
        .and_then(|action| action.join())
        .map_err(pdf_error);
    let close = stream.Close().map_err(pdf_error);
    let page_close = page.Close().map_err(pdf_error);
    result.and(close).and(page_close)?;

    // WinRT uses display scaling for its bitmap. Normalize to the selected physical DPI.
    if output == Format::Webp {
        let normalized = stage.join("pdf-normalized.png");
        image::encode_with_size(raster, Format::Png, &normalized, Some((width, height, 150.0)))?;
        return image::convert(engines, Format::Png, &normalized, Format::Webp, path, stage);
    }
    image::encode_with_size(raster, output, path, Some((width, height, 150.0)))
}

pub fn merge(
    engines: &Engines,
    inputs: &[PathBuf],
    destination: &Path,
    stage: &Path,
) -> Result<()> {
    if inputs.len() < 2 {
        return Err("Cần ít nhất 2 tệp PDF để gộp.".to_owned());
    }
    if let Some(exe) = engines.pdfunite_path() {
        let mut staged_inputs = Vec::with_capacity(inputs.len());
        for (idx, input) in inputs.iter().enumerate() {
            let staged = stage.join(format!("merge-in-{idx}.pdf"));
            fs::copy(input, &staged).map_err(|error| error.to_string())?;
            staged_inputs.push(staged);
        }
        let mut cmd = Command::new(exe);
        cmd.creation_flags(CREATE_NO_WINDOW.0)
            .args(&staged_inputs)
            .arg(destination);
        if execute(&mut cmd, "pdfunite").is_ok() && is_valid_pdf_file(destination) {
            return Ok(());
        }
        let _ = fs::remove_file(destination);
    }

    if let Some(gs) = engines.ghostscript_path() {
        let mut staged_inputs = Vec::with_capacity(inputs.len());
        for (idx, input) in inputs.iter().enumerate() {
            let staged = stage.join(format!("gs-merge-{idx}.pdf"));
            fs::copy(input, &staged).map_err(|error| error.to_string())?;
            staged_inputs.push(staged);
        }
        let mut out_arg = OsString::from("-sOutputFile=");
        out_arg.push(destination.as_os_str());
        let mut cmd = Command::new(gs);
        cmd.creation_flags(CREATE_NO_WINDOW.0)
            .args(["-dBATCH", "-dNOPAUSE", "-q", "-sDEVICE=pdfwrite"])
            .arg(&out_arg)
            .args(&staged_inputs);
        if execute(&mut cmd, "Ghostscript").is_ok() && is_valid_pdf_file(destination) {
            return Ok(());
        }
        let _ = fs::remove_file(destination);
    }

    let mut all_pages = Vec::new();
    for (doc_idx, input) in inputs.iter().enumerate() {
        let mut pages =
            render_pdf_pages_to_jpeg(engines, input, stage, &format!("m{doc_idx}"), 150.0, false)?;
        all_pages.append(&mut pages);
    }
    img2pdf::write_jpeg_pages(&all_pages, destination)
}

pub fn split(
    engines: &Engines,
    input: &Path,
    output_dir: &Path,
    stem: &OsStr,
    stage: &Path,
) -> Result<()> {
    if let Some(exe) = engines.pdfseparate_path() {
        let staged_in = stage.join("split-in.pdf");
        fs::copy(input, &staged_in).map_err(|error| error.to_string())?;
        let pattern = stage.join("split-out-%03d.pdf");
        let mut cmd = Command::new(exe);
        cmd.creation_flags(CREATE_NO_WINDOW.0)
            .arg(&staged_in)
            .arg(&pattern);
        if execute(&mut cmd, "pdfseparate").is_ok() {
            let mut produced: Vec<(u32, PathBuf)> = fs::read_dir(stage)
                .map_err(|error| error.to_string())?
                .filter_map(|entry| entry.ok())
                .filter_map(|entry| {
                    let path = entry.path();
                    let name = path.file_name()?.to_str()?;
                    let num_part = name
                        .strip_prefix("split-out-")?
                        .strip_suffix(".pdf")?;
                    let page_num = num_part.parse::<u32>().ok()?;
                    Some((page_num, path))
                })
                .collect();
            produced.sort_by_key(|(num, _)| *num);
            if !produced.is_empty() && produced.iter().all(|(_, p)| is_valid_pdf_file(p)) {
                let digit_count = produced
                    .last()
                    .map(|(n, _)| n.to_string().len().max(3))
                    .unwrap_or(3);
                for (page_num, src_path) in produced {
                    let mut filename = stem.to_os_string();
                    filename.push(format!("-{page_num:0digit_count$}.pdf"));
                    fs::rename(&src_path, output_dir.join(filename))
                        .map_err(|error| error.to_string())?;
                }
                return Ok(());
            }
        }
        if let Ok(entries) = fs::read_dir(stage) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("split-out-"))
                {
                    let _ = fs::remove_file(p);
                }
            }
        }
    }

    let pages = render_pdf_pages_to_jpeg(engines, input, stage, "split", 150.0, false)?;
    let digit_count = pages.len().to_string().len().max(3);
    for (idx, page) in pages.into_iter().enumerate() {
        let mut filename = stem.to_os_string();
        filename.push(format!("-{:0digit_count$}.pdf", idx + 1));
        img2pdf::write_jpeg_pages(&[page], &output_dir.join(filename))?;
    }
    Ok(())
}

pub fn compress(
    engines: &Engines,
    input: &Path,
    destination: &Path,
    stage: &Path,
) -> Result<()> {
    if let Some(gs) = engines.ghostscript_path() {
        let staged_in = stage.join("gs-comp-in.pdf");
        fs::copy(input, &staged_in).map_err(|error| error.to_string())?;
        let mut out_arg = OsString::from("-sOutputFile=");
        out_arg.push(destination.as_os_str());
        let mut cmd = Command::new(gs);
        cmd.creation_flags(CREATE_NO_WINDOW.0)
            .args([
                "-sDEVICE=pdfwrite",
                "-dCompatibilityLevel=1.4",
                "-dPDFSETTINGS=/ebook",
                "-dNOPAUSE",
                "-dQUIET",
                "-dBATCH",
            ])
            .arg(&out_arg)
            .arg(&staged_in);
        if execute(&mut cmd, "Ghostscript").is_ok() && is_valid_pdf_file(destination) {
            return Ok(());
        }
        let _ = fs::remove_file(destination);
    }

    let pages = render_pdf_pages_to_jpeg(engines, input, stage, "cmp", 115.0, true)?;
    img2pdf::write_jpeg_pages(&pages, destination)
}

fn is_valid_pdf_file(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut header = [0u8; 5];
    file.read_exact(&mut header).is_ok() && &header == b"%PDF-"
}

fn render_pdf_pages_to_jpeg(
    engines: &Engines,
    input: &Path,
    stage: &Path,
    tag: &str,
    dpi: f64,
    strong_compress: bool,
) -> Result<Vec<(PathBuf, f64, f64)>> {
    unsafe {
        RoInitialize(RO_INIT_MULTITHREADED).map_err(pdf_error)?;
    }
    let _runtime = Runtime;
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(input.as_os_str()))
        .and_then(|operation| operation.join())
        .map_err(pdf_error)?;
    let document = PdfDocument::LoadFromFileAsync(&file)
        .and_then(|operation| operation.join())
        .map_err(pdf_error)?;
    let page_count = document.PageCount().map_err(pdf_error)?;
    if page_count == 0 {
        return Err("PDF không có trang nào.".to_owned());
    }

    let raster = stage.join(format!("{tag}-raster.png"));
    let mut results = Vec::with_capacity(page_count as usize);
    for index in 0..page_count {
        let page = document.GetPage(index).map_err(pdf_error)?;
        let (width, height) = render_page_png_at_dpi(&page, &raster, dpi)?;
        let jpg_path = stage.join(format!("{tag}-p-{index:04}.jpg"));
        if strong_compress && engines.ffmpeg_path().is_some() {
            let mut cmd = engines.ffmpeg()?;
            cmd.arg("-i")
                .arg(&raster)
                .args(["-frames:v", "1", "-map_metadata", "-1", "-q:v", "7"])
                .arg(&jpg_path);
            if execute(&mut cmd, "FFmpeg").is_err() {
                let _ = fs::remove_file(&jpg_path);
                image::encode_with_size(&raster, Format::Jpg, &jpg_path, Some((width, height, dpi)))?;
            }
        } else {
            image::encode_with_size(&raster, Format::Jpg, &jpg_path, Some((width, height, dpi)))?;
        }
        results.push((jpg_path, dpi, dpi));
    }
    Ok(results)
}

fn render_page_png_at_dpi(page: &PdfPage, raster: &Path, dpi: f64) -> Result<(u32, u32)> {
    let size = page.Size().map_err(pdf_error)?;
    let size_in_pixels = |dip: f32| -> Result<u32> {
        let pixels = (f64::from(dip) * dpi / 96.0).round();
        if !(1.0..=f64::from(u32::MAX)).contains(&pixels) {
            return Err("Kích thước trang PDF không hợp lệ.".to_owned());
        }
        Ok(pixels as u32)
    };
    let width = size_in_pixels(size.Width)?;
    let height = size_in_pixels(size.Height)?;
    let options = PdfPageRenderOptions::new().map_err(pdf_error)?;
    options.SetDestinationWidth(width).map_err(pdf_error)?;
    options.SetDestinationHeight(height).map_err(pdf_error)?;
    options
        .SetBackgroundColor(Color {
            A: 255,
            R: 255,
            G: 255,
            B: 255,
        })
        .map_err(pdf_error)?;
    let container = BitmapEncoder::PngEncoderId().map_err(pdf_error)?;
    options.SetBitmapEncoderId(container).map_err(pdf_error)?;
    File::create(raster).map_err(|error| error.to_string())?;
    let stream = FileRandomAccessStream::OpenAsync(
        &HSTRING::from(raster.as_os_str()),
        FileAccessMode::ReadWrite,
    )
    .and_then(|operation| operation.join())
    .map_err(pdf_error)?;
    let result = page
        .RenderWithOptionsToStreamAsync(&stream, &options)
        .and_then(|action| action.join())
        .map_err(pdf_error);
    let close = stream.Close().map_err(pdf_error);
    let page_close = page.Close().map_err(pdf_error);
    result.and(close).and(page_close)?;
    Ok((width, height))
}

struct Runtime;

impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe {
            RoUninitialize();
        }
    }
}
