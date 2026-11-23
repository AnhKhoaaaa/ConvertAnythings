use std::ffi::OsStr;
use std::fs::{self, File};
use std::path::Path;

use windows::Data::Pdf::{PdfDocument, PdfPage, PdfPageRenderOptions};
use windows::Graphics::Imaging::BitmapEncoder;
use windows::Storage::Streams::FileRandomAccessStream;
use windows::Storage::{FileAccessMode, StorageFile};
use windows::UI::Color;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
use windows::core::HSTRING;

use super::{Format, Result, image};

fn pdf_error(error: windows::core::Error) -> String {
    format!("Windows không thể đọc hoặc xuất PDF. {error}")
}

pub fn convert(
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
        render(&page, output, destination, &raster)?;
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
        render(&page, output, &folder.join(name), &raster)?;
    }
    Ok(true)
}

fn render(page: &PdfPage, output: Format, path: &Path, raster: &Path) -> Result<()> {
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
    image::encode_with_size(raster, output, path, Some((width, height, 150.0)))
}

struct Runtime;

impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe {
            RoUninitialize();
        }
    }
}
