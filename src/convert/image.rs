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
    }

    // Fast single-pass FFmpeg routes for modern image outputs (no intermediate PNG).
    if matches!(output, Format::Webp | Format::Avif | Format::Ico) {
        return encode_via_ffmpeg(engines, source, input, output, destination, stage);
    }

    // Formats requiring FFmpeg decoding (WebP, AVIF, HEIC)
    if matches!(source, Format::Webp | Format::Avif | Format::Heic) {
        if matches!(output, Format::Png | Format::Tiff) {
            let codec = if output == Format::Png { "png" } else { "tiff" };
            return execute(
                engines
                    .ffmpeg()?
                    .arg("-i")
                    .arg(input)
                    .args(["-frames:v", "1", "-c:v", codec])
                    .arg(destination),
                "FFmpeg",
            );
        }
        // Uncompressed TIFF intermediate avoids slow zlib compression before WIC white-flattening.
        let intermediate = stage.join("decoded.tiff");
        execute(
            engines
                .ffmpeg()?
                .arg("-i")
                .arg(input)
                .args(["-frames:v", "1", "-c:v", "tiff", "-compression_algo", "raw"])
                .arg(&intermediate),
            "FFmpeg",
        )?;
        return encode(&intermediate, output, destination);
    }

    // Standard WIC encode, with FFmpeg fallback if WIC cannot decode (e.g. exotic/1-frame GIF/ICO).
    match encode(input, output, destination) {
        Ok(()) => Ok(()),
        Err(wic_err) if engines.ffmpeg.is_some() => {
            let intermediate = stage.join("decoded.tiff");
            if execute(
                engines
                    .ffmpeg()?
                    .arg("-i")
                    .arg(input)
                    .args(["-frames:v", "1", "-c:v", "tiff", "-compression_algo", "raw"])
                    .arg(&intermediate),
                "FFmpeg",
            )
            .is_ok()
            {
                encode(&intermediate, output, destination)
            } else {
                Err(wic_err)
            }
        }
        Err(err) => Err(err),
    }
}

pub fn compressed_extension(source: Format) -> &'static str {
    match source {
        Format::Png => "png",
        Format::Webp => "webp",
        Format::Avif => "avif",
        Format::Gif => "gif",
        _ => "jpg",
    }
}

pub fn compress(
    engines: &Engines,
    source: Format,
    input: &Path,
    destination: &Path,
    stage: &Path,
) -> Result<()> {
    if source == Format::Webp {
        validate_webp(input)?;
    }
    match source {
        Format::Jpg => {
            if engines.ffmpeg_path().is_some() {
                let mut cmd = engines.ffmpeg()?;
                cmd.arg("-i")
                    .arg(input)
                    .args(["-frames:v", "1", "-map_metadata", "-1", "-q:v", "6"])
                    .arg(destination);
                if execute(&mut cmd, "FFmpeg").is_ok() {
                    return Ok(());
                }
                let _ = std::fs::remove_file(destination);
            }
            encode(input, Format::Jpg, destination)
        }
        Format::Png => {
            if engines.ffmpeg_path().is_some() {
                let mut cmd = engines.ffmpeg()?;
                cmd.arg("-i")
                    .arg(input)
                    .args([
                        "-frames:v",
                        "1",
                        "-vf",
                        "split[s0][s1];[s0]palettegen=max_colors=256:reserve_transparent=1[p];[s1][p]paletteuse=dither=bayer:bayer_scale=3",
                        "-compression_level",
                        "9",
                        "-map_metadata",
                        "-1",
                    ])
                    .arg(destination);
                if execute(&mut cmd, "FFmpeg").is_ok() {
                    return Ok(());
                }
                let _ = std::fs::remove_file(destination);
            }
            encode(input, Format::Png, destination)
        }
        Format::Webp => {
            let mut cmd = engines.ffmpeg()?;
            cmd.arg("-i")
                .arg(input)
                .args([
                    "-frames:v",
                    "1",
                    "-c:v",
                    "libwebp",
                    "-quality",
                    "72",
                    "-compression_level",
                    "6",
                    "-map_metadata",
                    "-1",
                ])
                .arg(destination);
            execute(&mut cmd, "FFmpeg")
        }
        Format::Avif => {
            let mut cmd = engines.ffmpeg()?;
            cmd.arg("-i")
                .arg(input)
                .args([
                    "-frames:v",
                    "1",
                    "-c:v",
                    "libsvtav1",
                    "-crf",
                    "35",
                    "-preset",
                    "8",
                    "-svtav1-params",
                    "avif=1",
                    "-pix_fmt",
                    "yuv420p",
                    "-map_metadata",
                    "-1",
                ])
                .arg(destination);
            if execute(&mut cmd, "FFmpeg").is_ok() {
                return Ok(());
            }
            let _ = std::fs::remove_file(destination);
            let mut fallback = engines.ffmpeg()?;
            fallback
                .arg("-i")
                .arg(input)
                .args([
                    "-frames:v",
                    "1",
                    "-c:v",
                    "libaom-av1",
                    "-still-picture",
                    "1",
                    "-crf",
                    "36",
                    "-cpu-used",
                    "6",
                    "-pix_fmt",
                    "yuv420p",
                    "-map_metadata",
                    "-1",
                ])
                .arg(destination);
            execute(&mut fallback, "FFmpeg")
        }
        Format::Gif => {
            let mut cmd = engines.ffmpeg()?;
            cmd.arg("-i")
                .arg(input)
                .args([
                    "-vf",
                    "fps=12,scale='min(480,iw)':-1:flags=lanczos,split[s0][s1];[s0]palettegen=max_colors=128:stats_mode=diff[p];[s1][p]paletteuse=dither=bayer:bayer_scale=3",
                    "-loop",
                    "0",
                ])
                .arg(destination);
            execute(&mut cmd, "FFmpeg")
        }
        Format::Bmp | Format::Tiff | Format::Heic => {
            if engines.ffmpeg_path().is_some() {
                let mut cmd = engines.ffmpeg()?;
                cmd.arg("-i")
                    .arg(input)
                    .args(["-frames:v", "1", "-map_metadata", "-1", "-q:v", "6"])
                    .arg(destination);
                if execute(&mut cmd, "FFmpeg").is_ok() {
                    return Ok(());
                }
                let _ = std::fs::remove_file(destination);
            }
            convert(engines, source, input, Format::Jpg, destination, stage)
        }
        _ => Err("Định dạng ảnh này chưa hỗ trợ nén.".to_owned()),
    }
}

fn encode_via_ffmpeg(
    engines: &Engines,
    source: Format,
    input: &Path,
    output: Format,
    destination: &Path,
    stage: &Path,
) -> Result<()> {
    let direct_input;
    let src_path = if source == Format::Tiff {
        // FFmpeg supports TIFF directly, so pass straight through; keep helper for exotic WIC-only sources.
        input
    } else {
        input
    };
    let run_encode = |src: &Path, dst: &Path| -> Result<()> {
        let mut cmd = engines.ffmpeg()?;
        cmd.arg("-i").arg(src).args(["-frames:v", "1"]);
        match output {
            Format::Webp => {
                cmd.args(["-c:v", "libwebp", "-quality", "88", "-compression_level", "4"]);
            }
            Format::Avif => {
                cmd.args([
                    "-c:v",
                    "libsvtav1",
                    "-crf",
                    "28",
                    "-preset",
                    "8",
                    "-svtav1-params",
                    "avif=1",
                    "-pix_fmt",
                    "yuv420p",
                ]);
            }
            Format::Ico => {
                cmd.args([
                    "-vf",
                    "scale='min(256,iw)':'min(256,ih)':force_original_aspect_ratio=decrease",
                    "-c:v",
                    "png",
                ]);
            }
            _ => return Err("Định dạng ảnh đích không hợp lệ.".to_owned()),
        }
        execute(cmd.arg(dst), "FFmpeg")
    };

    match run_encode(src_path, destination) {
        Ok(()) => Ok(()),
        Err(err) => {
            // Fallback: if SVT-AV1 isn't available for AVIF, try libaom-av1; or if FFmpeg couldn't read input, decode via WIC to uncompressed TIFF.
            if output == Format::Avif {
                let mut cmd = engines.ffmpeg()?;
                cmd.arg("-i").arg(src_path).args([
                    "-frames:v",
                    "1",
                    "-c:v",
                    "libaom-av1",
                    "-still-picture",
                    "1",
                    "-crf",
                    "30",
                    "-cpu-used",
                    "6",
                    "-pix_fmt",
                    "yuv420p",
                ]);
                if execute(cmd.arg(destination), "FFmpeg").is_ok() {
                    return Ok(());
                }
            }
            direct_input = stage.join("wic_norm.tiff");
            if encode(input, Format::Tiff, &direct_input).is_ok() {
                run_encode(&direct_input, destination)
            } else {
                Err(err)
            }
        }
    }
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

pub fn encode(input: &Path, output: Format, destination: &Path) -> Result<()> {
    encode_with_size(input, output, destination, None)
}

pub fn probe_dimensions_and_dpi(input: &Path) -> Option<(u32, u32, f64, f64)> {
    let _apartment = Apartment::new().ok()?;
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let name = wide(input);
        let decoder = factory
            .CreateDecoderFromFilename(
                PCWSTR(name.as_ptr()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut w, mut h) = (0, 0);
        frame.GetSize(&mut w, &mut h).ok()?;
        let (mut dx, mut dy) = (96.0, 96.0);
        let _ = frame.GetResolution(&mut dx, &mut dy);
        if dx <= 0.0 {
            dx = 96.0;
        }
        if dy <= 0.0 {
            dy = 96.0;
        }
        Some((w, h, dx, dy))
    }
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

        let src_format = source.GetPixelFormat().unwrap_or_default();
        let already_opaque = src_format == GUID_WICPixelFormat24bppBGR
            || src_format == GUID_WICPixelFormat24bppRGB
            || src_format == GUID_WICPixelFormat32bppBGR
            || src_format == GUID_WICPixelFormat8bppGray;

        let (ready, _pixels) = if flatten && !already_opaque {
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
            if alpha == 255 {
                continue;
            }
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
