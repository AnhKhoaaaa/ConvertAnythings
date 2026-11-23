use std::fs::File;
use std::io::Read;
use std::path::Path;

use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows::core::{GUID, Interface, PCWSTR};

use super::{Engines, Format, Result, execute};
use crate::wide;

pub fn convert(
    engines: &Engines,
    source: Format,
    input: &Path,
    output: Format,
    destination: &Path,
    stage: &Path,
) -> Result<()> {
    if source == Format::Webp {
        validate_webp(input)?;
        let intermediate = stage.join("decoded.png");
        execute(
            engines
                .ffmpeg()?
                .arg("-i")
                .arg(input)
                .args(["-frames:v", "1", "-c:v", "png"])
                .arg(&intermediate),
            "FFmpeg",
        )?;
        return encode(&intermediate, output, destination);
    }
    if output == Format::Webp {
        let intermediate = stage.join("source.png");
        encode(input, Format::Png, &intermediate)?;
        return execute(
            engines
                .ffmpeg()?
                .arg("-i")
                .arg(intermediate)
                .args(["-frames:v", "1", "-c:v", "libwebp", "-lossless", "1"])
                .arg(destination),
            "FFmpeg",
        );
    }
    encode(input, output, destination)
}

fn validate_webp(path: &Path) -> Result<()> {
    let mut header = [0; 21];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|error| error.to_string())?;
    if &header[..4] != b"RIFF" || &header[8..12] != b"WEBP" {
        return Err("Tệp WebP không hợp lệ.".to_owned());
    }
    if &header[12..16] == b"VP8X" && header[20] & 2 != 0 {
        return Err("Phiên bản này chỉ chuyển WebP tĩnh.".to_owned());
    }
    Ok(())
}

fn encode(input: &Path, output: Format, destination: &Path) -> Result<()> {
    encode_with_size(input, output, destination, None)
}

pub fn encode_with_size(
    input: &Path,
    output: Format,
    destination: &Path,
    dimensions: Option<(u32, u32, f64)>,
) -> Result<()> {
    fn wic_error(error: windows::core::Error) -> String {
        format!("Windows Imaging Component không thể chuyển ảnh. {error}")
    }

    let _apartment = Apartment::new().map_err(wic_error)?;
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)
                .map_err(wic_error)?;
        let name = wide(input);
        let decoder = factory
            .CreateDecoderFromFilename(
                PCWSTR(name.as_ptr()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .map_err(wic_error)?;
        if decoder.GetFrameCount().map_err(wic_error)? != 1 {
            return Err("Phiên bản này chỉ chuyển ảnh tĩnh một khung hoặc một trang.".to_owned());
        }
        let source: IWICBitmapSource = decoder
            .GetFrame(0)
            .and_then(|frame| frame.cast())
            .map_err(wic_error)?;
        let (mut width, mut height) = (0, 0);
        source.GetSize(&mut width, &mut height).map_err(wic_error)?;
        let (mut dpi_x, mut dpi_y) = (96.0, 96.0);
        let _ = source.GetResolution(&mut dpi_x, &mut dpi_y);
        let source = if let Some((requested_width, requested_height, resolution)) = dimensions {
            dpi_x = resolution;
            dpi_y = resolution;
            if (width, height) == (requested_width, requested_height) {
                source
            } else {
                let scaler = factory.CreateBitmapScaler().map_err(wic_error)?;
                scaler
                    .Initialize(
                        &source,
                        requested_width,
                        requested_height,
                        WICBitmapInterpolationModeFant,
                    )
                    .map_err(wic_error)?;
                width = requested_width;
                height = requested_height;
                scaler.cast().map_err(wic_error)?
            }
        } else {
            source
        };
        let (container, mut pixel_format, flatten) = match output {
            Format::Jpg => (GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR, true),
            Format::Bmp => (GUID_ContainerFormatBmp, GUID_WICPixelFormat24bppBGR, true),
            Format::Png => (GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA, false),
            Format::Tiff => (
                GUID_ContainerFormatTiff,
                GUID_WICPixelFormat32bppBGRA,
                false,
            ),
            _ => return Err("Định dạng đầu ra không dùng WIC.".to_owned()),
        };
        let stream = factory.CreateStream().map_err(wic_error)?;
        let name = wide(destination);
        stream
            .InitializeFromFilename(PCWSTR(name.as_ptr()), GENERIC_WRITE.0)
            .map_err(wic_error)?;
        let encoder = factory
            .CreateEncoder(&container, std::ptr::null())
            .map_err(wic_error)?;
        encoder
            .Initialize(&stream, WICBitmapEncoderNoCache)
            .map_err(wic_error)?;
        let (mut frame, mut properties) = (None, None);
        encoder
            .CreateNewFrame(&mut frame, &mut properties)
            .map_err(wic_error)?;
        let frame = frame.ok_or("WIC không tạo được khung ảnh đầu ra.")?;
        frame.Initialize(properties.as_ref()).map_err(wic_error)?;
        frame.SetSize(width, height).map_err(wic_error)?;
        if dpi_x > 0.0 && dpi_y > 0.0 {
            frame.SetResolution(dpi_x, dpi_y).map_err(wic_error)?;
        }
        frame.SetPixelFormat(&mut pixel_format).map_err(wic_error)?;

        let (ready, _pixels) = if flatten {
            flatten_white(&factory, &source, width, height)?
        } else {
            (source, Vec::new())
        };
        let converter = change_format(&factory, &ready, &pixel_format).map_err(wic_error)?;
        frame
            .WriteSource(&converter, std::ptr::null())
            .map_err(wic_error)?;
        frame.Commit().map_err(wic_error)?;
        encoder.Commit().map_err(wic_error)
    }
}

unsafe fn change_format(
    factory: &IWICImagingFactory,
    source: &IWICBitmapSource,
    format: &GUID,
) -> windows::core::Result<IWICFormatConverter> {
    unsafe {
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(
            source,
            format,
            WICBitmapDitherTypeNone,
            None::<&IWICPalette>,
            0.0,
            WICBitmapPaletteTypeCustom,
        )?;
        Ok(converter)
    }
}

fn flatten_white(
    factory: &IWICImagingFactory,
    source: &IWICBitmapSource,
    width: u32,
    height: u32,
) -> Result<(IWICBitmapSource, Vec<u8>)> {
    let stride = width.checked_mul(4).ok_or("Ảnh quá rộng.")?;
    let length = stride.checked_mul(height).ok_or("Ảnh quá lớn.")? as usize;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(length)
        .map_err(|_| "Không đủ RAM để chuyển ảnh này.")?;
    pixels.resize(length, 0_u8);
    unsafe {
        let converter = change_format(factory, source, &GUID_WICPixelFormat32bppBGRA)
            .map_err(|error| error.to_string())?;
        converter
            .CopyPixels(std::ptr::null(), stride, &mut pixels)
            .map_err(|error| error.to_string())?;
        for pixel in pixels.as_chunks_mut::<4>().0 {
            let alpha = u16::from(pixel[3]);
            for color in &mut pixel[..3] {
                *color = ((u16::from(*color) * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
            }
            pixel[3] = 255;
        }
        let bitmap = factory
            .CreateBitmapFromMemory(
                width,
                height,
                &GUID_WICPixelFormat32bppBGRA,
                stride,
                &pixels,
            )
            .and_then(|bitmap| bitmap.cast())
            .map_err(|error| error.to_string())?;
        Ok((bitmap, pixels))
    }
}

pub(crate) struct Apartment;

impl Apartment {
    pub(crate) fn new() -> windows::core::Result<Self> {
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        }
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}
