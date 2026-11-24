use std::fs::File;
use std::io::Read;
use std::path::Path;

use super::Format;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SniffedKind {
    Jpg,
    Png,
    Gif,
    Bmp,
    Tiff,
    Webp,
    Avif,
    Heic,
    Ico,
    Pdf,
    ZipOffice,
    Rtf,
    MpegTs,
    EbmlVideo,
    IsoMedia,
    Avi,
    Wav,
    Flac,
    Ogg,
    Mp3,
    Text,
}

impl SniffedKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Jpg => "JPG",
            Self::Png => "PNG",
            Self::Gif => "GIF",
            Self::Bmp => "BMP",
            Self::Tiff => "TIFF",
            Self::Webp => "WebP",
            Self::Avif => "AVIF",
            Self::Heic => "HEIC",
            Self::Ico => "ICO",
            Self::Pdf => "PDF",
            Self::ZipOffice => "ZIP/Office",
            Self::Rtf => "RTF",
            Self::MpegTs => "MPEG-TS",
            Self::EbmlVideo => "MKV/WebM",
            Self::IsoMedia => "MP4/MOV",
            Self::Avi => "AVI",
            Self::Wav => "WAV",
            Self::Flac => "FLAC",
            Self::Ogg => "OGG",
            Self::Mp3 => "MP3",
            Self::Text => "văn bản thuần",
        }
    }

    fn matches(self, format: Format) -> bool {
        match self {
            Self::Jpg => format == Format::Jpg,
            Self::Png => format == Format::Png,
            Self::Gif => format == Format::Gif,
            Self::Bmp => format == Format::Bmp,
            Self::Tiff => format == Format::Tiff,
            Self::Webp => format == Format::Webp,
            Self::Avif | Self::Heic => matches!(format, Format::Avif | Format::Heic),
            Self::Ico => format == Format::Ico,
            Self::Pdf => format == Format::Pdf,
            Self::ZipOffice => matches!(format, Format::Docx | Format::Xlsx | Format::Pptx),
            Self::Rtf => matches!(format, Format::Rtf | Format::Doc),
            Self::MpegTs => format == Format::Ts,
            Self::EbmlVideo => matches!(format, Format::Mkv | Format::Webm),
            Self::IsoMedia => matches!(format, Format::Mp4 | Format::Mov | Format::M4v | Format::M4a),
            Self::Avi => format == Format::Avi,
            Self::Wav => format == Format::Wav,
            Self::Flac => format == Format::Flac,
            Self::Ogg => matches!(format, Format::Ogg | Format::Opus),
            Self::Mp3 => format == Format::Mp3,
            Self::Text => matches!(
                format,
                Format::Txt | Format::Csv | Format::Md | Format::Html | Format::Rtf
            ),
        }
    }
}

fn read_header(path: &Path) -> Option<Vec<u8>> {
    let mut file = File::open(path).ok()?;
    let mut buf = vec![0u8; 512];
    let n = file.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(buf)
}

pub fn is_mpeg_ts_bytes(bytes: &[u8]) -> bool {
    let ts_188 = bytes.len() >= 377 && bytes[0] == 0x47 && bytes[188] == 0x47 && bytes[376] == 0x47;
    let ts_192 = bytes.len() >= 389 && bytes[4] == 0x47 && bytes[196] == 0x47 && bytes[388] == 0x47;
    ts_188 || ts_192
}

pub fn is_mpeg_ts(path: &Path) -> bool {
    read_header(path).is_some_and(|bytes| is_mpeg_ts_bytes(&bytes))
}

pub fn detect_extension_format(path: &Path) -> Option<Format> {
    let format = path.extension().and_then(Format::parse)?;
    if format.is_action() {
        return None;
    }
    if format == Format::Ts && !is_mpeg_ts(path) {
        return None;
    }
    Some(format)
}

pub fn sniff_bytes(bytes: &[u8]) -> Option<SniffedKind> {
    if bytes.is_empty() {
        return None;
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(SniffedKind::Jpg);
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some(SniffedKind::Png);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(SniffedKind::Gif);
    }
    if bytes.starts_with(b"BM") && bytes.len() >= 14 {
        return Some(SniffedKind::Bmp);
    }
    if bytes.starts_with(b"II*\x00") || bytes.starts_with(b"MM\x00*") {
        return Some(SniffedKind::Tiff);
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" {
        return match &bytes[8..12] {
            b"WEBP" => Some(SniffedKind::Webp),
            b"AVI " => Some(SniffedKind::Avi),
            b"WAVE" => Some(SniffedKind::Wav),
            _ => None,
        };
    }
    if bytes.len() >= 6 && bytes[0..4] == [0x00, 0x00, 0x01, 0x00] {
        return Some(SniffedKind::Ico);
    }
    if bytes.starts_with(b"%PDF-") {
        return Some(SniffedKind::Pdf);
    }
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        return Some(SniffedKind::ZipOffice);
    }
    if bytes.starts_with(b"{\\rtf") {
        return Some(SniffedKind::Rtf);
    }
    if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return Some(SniffedKind::EbmlVideo);
    }
    if bytes.starts_with(b"fLaC") {
        return Some(SniffedKind::Flac);
    }
    if bytes.starts_with(b"OggS") {
        return Some(SniffedKind::Ogg);
    }
    if bytes.starts_with(b"ID3")
        || (bytes.len() >= 2 && bytes[0] == 0xff && matches!(bytes[1] & 0xfe, 0xfa | 0xf2 | 0xe2))
    {
        return Some(SniffedKind::Mp3);
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        return match &bytes[8..12] {
            b"avif" | b"avis" => Some(SniffedKind::Avif),
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"mif1" | b"msf1" => Some(SniffedKind::Heic),
            _ => Some(SniffedKind::IsoMedia),
        };
    }
    if is_mpeg_ts_bytes(bytes) {
        return Some(SniffedKind::MpegTs);
    }
    if looks_like_text(bytes) {
        return Some(SniffedKind::Text);
    }
    None
}

fn looks_like_text(bytes: &[u8]) -> bool {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        return true;
    }
    let slice = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    if slice.contains(&0) {
        return false;
    }
    // Truncate at last UTF-8 boundary in case the 512-byte read cut a multibyte character.
    let valid_end = match std::str::from_utf8(slice) {
        Ok(_) => slice.len(),
        Err(err) if err.error_len().is_none() => err.valid_up_to(),
        Err(_) => return false,
    };
    let text = std::str::from_utf8(&slice[..valid_end]).unwrap_or("");
    if text.is_empty() {
        return false;
    }
    text.chars()
        .all(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
}

pub fn verify(path: &Path, claimed: Format) -> Result<(), String> {
    let Some(bytes) = read_header(path) else {
        return Ok(());
    };
    let Some(detected) = sniff_bytes(&bytes) else {
        return Ok(());
    };
    if detected.matches(claimed) {
        return Ok(());
    }
    Err(format!(
        "Nội dung tệp thực chất là {} nhưng mang đuôi {}.",
        detected.label(),
        claimed.label()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disambiguates_mpeg_ts_vs_typescript() {
        let mut ts_pkt = vec![0u8; 400];
        ts_pkt[0] = 0x47;
        ts_pkt[188] = 0x47;
        ts_pkt[376] = 0x47;
        assert_eq!(sniff_bytes(&ts_pkt), Some(SniffedKind::MpegTs));

        let ts_code = b"export const answer: number = 42;\n";
        assert_eq!(sniff_bytes(ts_code), Some(SniffedKind::Text));
        assert!(!is_mpeg_ts_bytes(ts_code));
    }

    #[test]
    fn detects_misnamed_png_as_mp4() {
        let png = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
        let kind = sniff_bytes(&png).expect("png detected");
        assert_eq!(kind, SniffedKind::Png);
        assert!(!kind.matches(Format::Mp4));
        assert!(kind.matches(Format::Png));
    }
}
