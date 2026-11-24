use std::ffi::{OsStr, OsString, c_void};
use std::fs;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use windows::Win32::Storage::FileSystem::{MOVE_FILE_FLAGS, MoveFileExW};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ,
    RRF_RT_REG_SZ, RegGetValueW,
};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
use windows::core::{PCWSTR, w};

use crate::wide;

mod image;
mod img2pdf;
mod markdown;
mod media;
mod pdf;
mod pdf2docx;
mod sniff;
mod word;

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Jpg,
    Png,
    Webp,
    Avif,
    Heic,
    Bmp,
    Tiff,
    Gif,
    Ico,
    Pdf,
    Docx,
    Doc,
    Rtf,
    Txt,
    Md,
    Html,
    Xlsx,
    Xls,
    Csv,
    Pptx,
    Ppt,
    Mp4,
    Mov,
    Mkv,
    Webm,
    Avi,
    Wmv,
    Flv,
    M4v,
    Ts,
    Mp3,
    Wav,
    M4a,
    Flac,
    Ogg,
    Aac,
    Wma,
    Opus,
    Compress,
    MergePdf,
    SplitPdf,
}

impl Format {
    pub fn parse(value: &OsStr) -> Option<Self> {
        match value.to_str()?.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" | "jfif" => Some(Self::Jpg),
            "png" => Some(Self::Png),
            "webp" => Some(Self::Webp),
            "avif" => Some(Self::Avif),
            "heic" | "heif" => Some(Self::Heic),
            "bmp" => Some(Self::Bmp),
            "tif" | "tiff" => Some(Self::Tiff),
            "gif" => Some(Self::Gif),
            "ico" => Some(Self::Ico),
            "pdf" => Some(Self::Pdf),
            "docx" => Some(Self::Docx),
            "doc" => Some(Self::Doc),
            "rtf" => Some(Self::Rtf),
            "txt" => Some(Self::Txt),
            "md" | "markdown" => Some(Self::Md),
            "html" | "htm" => Some(Self::Html),
            "xlsx" => Some(Self::Xlsx),
            "xls" => Some(Self::Xls),
            "csv" => Some(Self::Csv),
            "pptx" => Some(Self::Pptx),
            "ppt" => Some(Self::Ppt),
            "mp4" => Some(Self::Mp4),
            "mov" => Some(Self::Mov),
            "mkv" => Some(Self::Mkv),
            "webm" => Some(Self::Webm),
            "avi" => Some(Self::Avi),
            "wmv" => Some(Self::Wmv),
            "flv" => Some(Self::Flv),
            "m4v" => Some(Self::M4v),
            "ts" | "m2ts" => Some(Self::Ts),
            "mp3" => Some(Self::Mp3),
            "wav" => Some(Self::Wav),
            "m4a" => Some(Self::M4a),
            "flac" => Some(Self::Flac),
            "ogg" => Some(Self::Ogg),
            "aac" => Some(Self::Aac),
            "wma" => Some(Self::Wma),
            "opus" => Some(Self::Opus),
            "compress" => Some(Self::Compress),
            "merge-pdf" => Some(Self::MergePdf),
            "split-pdf" => Some(Self::SplitPdf),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Avif => "avif",
            Self::Heic => "heic",
            Self::Bmp => "bmp",
            Self::Tiff => "tiff",
            Self::Gif => "gif",
            Self::Ico => "ico",
            Self::Pdf => "pdf",
            Self::Docx => "docx",
            Self::Doc => "doc",
            Self::Rtf => "rtf",
            Self::Txt => "txt",
            Self::Md => "md",
            Self::Html => "html",
            Self::Xlsx => "xlsx",
            Self::Xls => "xls",
            Self::Csv => "csv",
            Self::Pptx => "pptx",
            Self::Ppt => "ppt",
            Self::Mp4 => "mp4",
            Self::Mov => "mov",
            Self::Mkv => "mkv",
            Self::Webm => "webm",
            Self::Avi => "avi",
            Self::Wmv => "wmv",
            Self::Flv => "flv",
            Self::M4v => "m4v",
            Self::Ts => "ts",
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::M4a => "m4a",
            Self::Flac => "flac",
            Self::Ogg => "ogg",
            Self::Aac => "aac",
            Self::Wma => "wma",
            Self::Opus => "opus",
            Self::Compress => "compress",
            Self::MergePdf => "merge-pdf",
            Self::SplitPdf => "split-pdf",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Jpg => "JPG",
            Self::Png => "PNG",
            Self::Webp => "WebP",
            Self::Avif => "AVIF",
            Self::Heic => "HEIC",
            Self::Bmp => "BMP",
            Self::Tiff => "TIFF",
            Self::Gif => "GIF",
            Self::Ico => "ICO",
            Self::Pdf => "PDF",
            Self::Docx => "DOCX",
            Self::Doc => "DOC",
            Self::Rtf => "RTF",
            Self::Txt => "TXT",
            Self::Md => "MD",
            Self::Html => "HTML",
            Self::Xlsx => "XLSX",
            Self::Xls => "XLS",
            Self::Csv => "CSV",
            Self::Pptx => "PPTX",
            Self::Ppt => "PPT",
            Self::Mp4 => "MP4",
            Self::Mov => "MOV",
            Self::Mkv => "MKV",
            Self::Webm => "WebM",
            Self::Avi => "AVI",
            Self::Wmv => "WMV",
            Self::Flv => "FLV",
            Self::M4v => "M4V",
            Self::Ts => "TS",
            Self::Mp3 => "MP3",
            Self::Wav => "WAV",
            Self::M4a => "M4A",
            Self::Flac => "FLAC",
            Self::Ogg => "OGG",
            Self::Aac => "AAC",
            Self::Wma => "WMA",
            Self::Opus => "OPUS",
            Self::Compress => "Nén",
            Self::MergePdf => "Gộp PDF",
            Self::SplitPdf => "Tách PDF",
        }
    }

    pub fn is_action(self) -> bool {
        matches!(self, Self::Compress | Self::MergePdf | Self::SplitPdf)
    }

    pub fn is_static_image(self) -> bool {
        matches!(
            self,
            Self::Jpg
                | Self::Png
                | Self::Webp
                | Self::Avif
                | Self::Heic
                | Self::Bmp
                | Self::Tiff
                | Self::Ico
        )
    }

    pub fn is_image(self) -> bool {
        self.is_static_image() || self == Self::Gif
    }

    pub fn is_video(self) -> bool {
        matches!(
            self,
            Self::Mp4
                | Self::Mov
                | Self::Mkv
                | Self::Webm
                | Self::Avi
                | Self::Wmv
                | Self::Flv
                | Self::M4v
                | Self::Ts
        )
    }

    pub fn is_audio(self) -> bool {
        matches!(
            self,
            Self::Mp3
                | Self::Wav
                | Self::M4a
                | Self::Flac
                | Self::Ogg
                | Self::Aac
                | Self::Wma
                | Self::Opus
        )
    }

    pub fn is_word_doc(self) -> bool {
        matches!(self, Self::Docx | Self::Doc | Self::Rtf | Self::Txt)
    }

    pub fn is_excel_doc(self) -> bool {
        matches!(self, Self::Xlsx | Self::Xls | Self::Csv)
    }

    pub fn is_ppt_doc(self) -> bool {
        matches!(self, Self::Pptx | Self::Ppt)
    }

    fn needs_ffmpeg_as_image_input(self) -> bool {
        matches!(self, Self::Webp | Self::Avif | Self::Heic)
    }

    fn needs_ffmpeg_as_image_output(self) -> bool {
        matches!(self, Self::Webp | Self::Avif | Self::Ico)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Image,
    Document,
    Video,
    Audio,
}

impl Group {
    pub fn label(self) -> &'static str {
        match self {
            Self::Image => "Ảnh",
            Self::Document => "Tài liệu",
            Self::Video => "Video",
            Self::Audio => "Âm thanh",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice {
    pub group: Group,
    pub format: Format,
}

const IMAGE_TARGETS: &[Format] = &[
    Format::Jpg,
    Format::Png,
    Format::Webp,
    Format::Pdf,
    Format::Avif,
    Format::Ico,
    Format::Bmp,
    Format::Tiff,
];

#[derive(Clone, Default)]
pub struct Engines {
    ffmpeg: Option<PathBuf>,
    ffprobe: Option<PathBuf>,
    pdftotext: Option<PathBuf>,
    pdfunite: Option<PathBuf>,
    pdfseparate: Option<PathBuf>,
    ghostscript: Option<PathBuf>,
    browser: Option<PathBuf>,
    word: bool,
    excel: bool,
    powerpoint: bool,
}

impl Engines {
    pub fn detect() -> Self {
        let ffmpeg = find_tool("ffmpeg.exe");
        let ffprobe = find_tool("ffprobe.exe").or_else(|| {
            ffmpeg
                .as_ref()
                .and_then(|p| p.parent().map(|d| d.join("ffprobe.exe")))
                .filter(|p| p.is_file())
        });
        Self {
            ffmpeg,
            ffprobe,
            pdftotext: find_tool("pdftotext.exe"),
            pdfunite: find_tool("pdfunite.exe"),
            pdfseparate: find_tool("pdfseparate.exe"),
            ghostscript: find_tool("gswin64c.exe")
                .or_else(|| find_tool("gswin32c.exe"))
                .or_else(|| find_tool("gs.exe")),
            browser: find_browser(),
            word: word::word_available(),
            excel: word::excel_available(),
            powerpoint: word::powerpoint_available(),
        }
    }

    pub fn choices(&self, path: &Path) -> (Vec<Choice>, Option<&'static str>) {
        let Some(input) = sniff::detect_extension_format(path) else {
            return (Vec::new(), Some("Chưa hỗ trợ"));
        };
        if input == Format::Md {
            let mut choices = Vec::new();
            if self.browser.is_some() || self.word {
                choices.push(Choice {
                    group: Group::Document,
                    format: Format::Pdf,
                });
            }
            choices.extend([
                Choice {
                    group: Group::Document,
                    format: Format::Docx,
                },
                Choice {
                    group: Group::Document,
                    format: Format::Html,
                },
            ]);
            return (choices, None);
        }
        if input == Format::Pdf {
            let mut choices = vec![
                Choice {
                    group: Group::Document,
                    format: Format::Compress,
                },
                Choice {
                    group: Group::Document,
                    format: Format::SplitPdf,
                },
                Choice {
                    group: Group::Image,
                    format: Format::Png,
                },
                Choice {
                    group: Group::Image,
                    format: Format::Jpg,
                },
            ];
            if self.ffmpeg.is_some() {
                choices.push(Choice {
                    group: Group::Image,
                    format: Format::Webp,
                });
            }
            if self.pdftotext.is_some() {
                choices.push(Choice {
                    group: Group::Document,
                    format: Format::Docx,
                });
                choices.push(Choice {
                    group: Group::Document,
                    format: Format::Txt,
                });
            }
            return (choices, None);
        }
        if input.is_word_doc() {
            return if self.word {
                (
                    vec![Choice {
                        group: Group::Document,
                        format: Format::Pdf,
                    }],
                    None,
                )
            } else {
                (Vec::new(), Some("Cần Word Desktop"))
            };
        }
        if input.is_excel_doc() {
            return if self.excel {
                (
                    vec![Choice {
                        group: Group::Document,
                        format: Format::Pdf,
                    }],
                    None,
                )
            } else {
                (Vec::new(), Some("Cần Excel Desktop"))
            };
        }
        if input.is_ppt_doc() {
            return if self.powerpoint {
                (
                    vec![Choice {
                        group: Group::Document,
                        format: Format::Pdf,
                    }],
                    None,
                )
            } else {
                (Vec::new(), Some("Cần PowerPoint"))
            };
        }
        if input.is_video() || input.is_audio() {
            if self.ffmpeg.is_none() {
                return (Vec::new(), Some("Cần FFmpeg"));
            }
            return (media::choices(input), None);
        }
        if input == Format::Gif {
            let mut choices = Vec::new();
            if self.ffmpeg.is_some() {
                choices.extend([
                    Choice {
                        group: Group::Image,
                        format: Format::Compress,
                    },
                    Choice {
                        group: Group::Video,
                        format: Format::Mp4,
                    },
                    Choice {
                        group: Group::Video,
                        format: Format::Webm,
                    },
                    Choice {
                        group: Group::Image,
                        format: Format::Webp,
                    },
                ]);
            }
            choices.extend([
                Choice {
                    group: Group::Image,
                    format: Format::Png,
                },
                Choice {
                    group: Group::Image,
                    format: Format::Jpg,
                },
                Choice {
                    group: Group::Document,
                    format: Format::Pdf,
                },
            ]);
            return (choices, None);
        }
        if input.needs_ffmpeg_as_image_input() && self.ffmpeg.is_none() {
            return (Vec::new(), Some("Cần FFmpeg"));
        }
        let mut choices: Vec<Choice> = IMAGE_TARGETS
            .iter()
            .copied()
            .filter(|format| {
                *format != input
                    && (!format.needs_ffmpeg_as_image_output() || self.ffmpeg.is_some())
            })
            .take(if input != Format::Ico { 7 } else { 8 })
            .map(|format| Choice {
                group: if format == Format::Pdf {
                    Group::Document
                } else {
                    Group::Image
                },
                format,
            })
            .collect();
        if input != Format::Ico {
            choices.push(Choice {
                group: Group::Image,
                format: Format::Compress,
            });
        }
        (choices, None)
    }

    pub fn choices_for_files(&self, paths: &[PathBuf]) -> (Vec<Choice>, Option<&'static str>) {
        if paths.is_empty() {
            return (Vec::new(), Some("Chọn một tệp"));
        }
        if paths.len() == 1 {
            return self.choices(&paths[0]);
        }
        let mut iter = paths.iter();
        let first = iter.next().expect("non-empty");
        let (mut common, reason) = self.choices(first);
        if common.is_empty() {
            return (Vec::new(), reason);
        }
        for path in iter {
            let (other, reason) = self.choices(path);
            if other.is_empty() {
                return (Vec::new(), reason);
            }
            common.retain(|choice| other.iter().any(|item| item.format == choice.format));
        }
        let all_pdfs = paths
            .iter()
            .all(|path| sniff::detect_extension_format(path) == Some(Format::Pdf));
        if all_pdfs && !common.iter().any(|c| c.format == Format::MergePdf) {
            common.insert(
                0,
                Choice {
                    group: Group::Document,
                    format: Format::MergePdf,
                },
            );
            if common.len() > 8 {
                common.truncate(8);
            }
        }
        let all_images = paths.iter().all(|path| {
            sniff::detect_extension_format(path).is_some_and(Format::is_image)
        });
        if all_images && !common.iter().any(|c| c.format == Format::Pdf) {
            common.push(Choice {
                group: Group::Document,
                format: Format::Pdf,
            });
        }
        if common.is_empty() {
            return (Vec::new(), Some("Không có đích chung"));
        }
        (common, None)
    }

    pub(crate) fn ffmpeg_path(&self) -> Option<&Path> {
        self.ffmpeg.as_deref()
    }

    pub(crate) fn pdfunite_path(&self) -> Option<&Path> {
        self.pdfunite.as_deref()
    }

    pub(crate) fn pdfseparate_path(&self) -> Option<&Path> {
        self.pdfseparate.as_deref()
    }

    pub(crate) fn ghostscript_path(&self) -> Option<&Path> {
        self.ghostscript.as_deref()
    }

    pub(crate) fn browser_path(&self) -> Option<&Path> {
        self.browser.as_deref()
    }

    pub(crate) fn word_available(&self) -> bool {
        self.word
    }

    pub(crate) fn ffmpeg(&self) -> Result<Command> {
        let executable = self
            .ffmpeg
            .as_ref()
            .ok_or("Cần FFmpeg trên PATH để thực hiện chuyển đổi này.")?;
        let mut command = Command::new(executable);
        command.creation_flags(CREATE_NO_WINDOW.0).args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-n",
            "-protocol_whitelist",
            "file",
        ]);
        Ok(command)
    }

    pub(crate) fn ffprobe(&self) -> Option<Command> {
        let executable = self.ffprobe.as_ref()?;
        let mut command = Command::new(executable);
        command.creation_flags(CREATE_NO_WINDOW.0);
        Some(command)
    }
}

fn find_browser() -> Option<PathBuf> {
    if let Some(edge) = find_tool("msedge.exe") {
        return Some(edge);
    }
    if let Some(chrome) = find_tool("chrome.exe") {
        return Some(chrome);
    }
    if std::env::var_os("CONVERTANYTHINGS_STRICT_PATH").is_some() {
        return None;
    }

    let relative_candidates = [
        ("ProgramFiles(x86)", "Microsoft\\Edge\\Application\\msedge.exe"),
        ("ProgramFiles", "Microsoft\\Edge\\Application\\msedge.exe"),
        ("ProgramFiles", "Google\\Chrome\\Application\\chrome.exe"),
        ("ProgramFiles(x86)", "Google\\Chrome\\Application\\chrome.exe"),
        ("LOCALAPPDATA", "Microsoft\\Edge\\Application\\msedge.exe"),
        ("LOCALAPPDATA", "Google\\Chrome\\Application\\chrome.exe"),
    ];
    for (env_var, rel) in relative_candidates {
        if let Some(base) = std::env::var_os(env_var) {
            let candidate = PathBuf::from(base).join(rel);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn find_tool(name: &str) -> Option<PathBuf> {
    // 1. Portable tools\ folder next to the executable
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("tools").join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    // 2. Current process PATH (absolute entries only)
    if let Some(path) = std::env::var_os("PATH")
        && let Some(found) = search_path_list(&path, name)
    {
        return Some(found);
    }

    // Allow tests to isolate PATH strictly when testing missing-tool errors
    if std::env::var_os("CONVERTANYTHINGS_STRICT_PATH").is_some() {
        return None;
    }

    // 3. Live Registry PATH (picks up tools installed via winget without restarting)
    for (root, subkey) in [
        (HKEY_CURRENT_USER, w!("Environment")),
        (
            HKEY_LOCAL_MACHINE,
            w!("SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Environment"),
        ),
    ] {
        if let Some(raw_path) = read_registry_string(root, subkey, w!("Path")) {
            let expanded = expand_env_vars(&raw_path);
            if let Some(found) = search_path_list(OsStr::new(&expanded), name) {
                return Some(found);
            }
        }
    }

    // 4. WinGet Links folder
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let candidate = PathBuf::from(local)
            .join("Microsoft\\WinGet\\Links")
            .join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    None
}

fn search_path_list(path_list: &OsStr, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path_list)
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(name))
        .find(|executable| executable.is_file())
}

fn read_registry_string(root: HKEY, subkey: PCWSTR, value_name: PCWSTR) -> Option<String> {
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
    let mut bytes = 0u32;
    unsafe {
        RegGetValueW(root, subkey, value_name, flags, None, None, Some(&mut bytes))
            .ok()
            .ok()?;
        if bytes < 2 {
            return None;
        }
        let mut buf = vec![0u16; (bytes as usize).div_ceil(2)];
        RegGetValueW(
            root,
            subkey,
            value_name,
            flags,
            None,
            Some(buf.as_mut_ptr() as *mut c_void),
            Some(&mut bytes),
        )
        .ok()
        .ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(OsString::from_wide(&buf[..len]).to_string_lossy().into_owned())
    }
}

fn expand_env_vars(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(end) = after.find('%') {
            let var_name = &after[..end];
            if !var_name.is_empty()
                && let Some(val) = std::env::var_os(var_name)
            {
                out.push_str(&val.to_string_lossy());
            } else {
                out.push('%');
                out.push_str(var_name);
                out.push('%');
            }
            rest = &after[end + 1..];
        } else {
            out.push('%');
            rest = after;
            break;
        }
    }
    out.push_str(rest);
    out
}

pub fn run(input: &Path, format: Format) -> Result<PathBuf> {
    let input = std::path::absolute(input).map_err(|error| error.to_string())?;
    if !input.is_file() {
        return Err("Hãy chọn một tệp có thể đọc được.".to_owned());
    }
    let source = sniff::detect_extension_format(&input)
        .ok_or("Không nhận diện được định dạng tệp.")?;
    sniff::verify(&input, source)?;

    let engines = Engines::detect();
    let (choices, reason) = engines.choices(&input);
    if !choices.iter().any(|choice| choice.format == format) {
        return Err(reason
            .unwrap_or("Không hỗ trợ cặp định dạng này hoặc thiếu bộ chuyển đổi.")
            .to_owned());
    }
    let parent = input
        .parent()
        .ok_or("Không tìm được thư mục của tệp gốc.")?;
    let stem = input.file_stem().ok_or("Không đọc được tên tệp gốc.")?;
    let (stage, output_dir) = Staging::new_with_fallback(parent)?;
    if format == Format::Compress {
        if source == Format::Pdf {
            let compress_tmp = stage.0.join("compressed.pdf");
            pdf::compress(&engines, &input, &compress_tmp, &stage.0)?;
            let mut out_stem = stem.to_os_string();
            out_stem.push("-compressed");
            return publish(&compress_tmp, &output_dir, &out_stem, Some("pdf"));
        }
        if source.is_image() {
            let out_ext = image::compressed_extension(source);
            let compress_tmp = stage.0.join(format!("compressed.{out_ext}"));
            image::compress(&engines, source, &input, &compress_tmp, &stage.0)?;
            let mut out_stem = stem.to_os_string();
            out_stem.push("-compressed");
            return publish(&compress_tmp, &output_dir, &out_stem, Some(out_ext));
        }
        if source.is_video() {
            let out_ext = media::compressed_extension(source);
            let compress_tmp = stage.0.join(format!("compressed.{out_ext}"));
            media::compress(&engines, source, &input, &compress_tmp)?;
            let mut out_stem = stem.to_os_string();
            out_stem.push("-compressed");
            return publish(&compress_tmp, &output_dir, &out_stem, Some(out_ext));
        }
    }
    if source == Format::Pdf && format == Format::SplitPdf {
        let split_dir = stage.0.join("split");
        fs::create_dir(&split_dir).map_err(|error| error.to_string())?;
        pdf::split(&engines, &input, &split_dir, stem, &stage.0)?;
        let mut folder_name = stem.to_os_string();
        folder_name.push("-split");
        return publish(&split_dir, &output_dir, &folder_name, None);
    }
    let temporary = stage.0.join(format!("result.{}", format.extension()));
    match source {
        Format::Md if format == Format::Html => markdown::convert(&input, &temporary)?,
        Format::Md if format == Format::Docx => markdown::convert_docx(&input, &temporary)?,
        Format::Md if format == Format::Pdf => {
            markdown::convert_pdf(&engines, &input, &temporary, &stage.0)?
        }
        Format::Pdf if format == Format::Docx => pdf2docx::convert(&engines, &input, &temporary)?,
        Format::Pdf if format == Format::Txt => {
            let exe = engines
                .pdftotext
                .as_ref()
                .ok_or("Cần pdftotext trên PATH để trích xuất văn bản PDF.")?;
            let mut cmd = Command::new(exe);
            cmd.creation_flags(CREATE_NO_WINDOW.0)
                .args(["-enc", "UTF-8"])
                .arg(&input)
                .arg(&temporary);
            execute(&mut cmd, "pdftotext")?;
        }
        Format::Pdf => {
            if pdf::convert(&engines, &input, format, &temporary, &stage.0, stem)? {
                let mut folder_name = stem.to_os_string();
                folder_name.push("-pages");
                return publish(&stage.0.join("pages"), &output_dir, &folder_name, None);
            }
        }
        s if s.is_word_doc() || s.is_excel_doc() || s.is_ppt_doc() => {
            word::convert(&input, source, &temporary, &stage.0)?;
        }
        Format::Gif if format.is_video() => {
            media::convert(&engines, &input, format, &temporary)?;
        }
        s if s.is_audio() || s.is_video() => {
            media::convert(&engines, &input, format, &temporary)?;
        }
        s if s.is_image() && format == Format::Pdf => {
            img2pdf::merge(&engines, std::slice::from_ref(&input), &temporary, &stage.0)?;
        }
        _ => image::convert(&engines, source, &input, format, &temporary, &stage.0)?,
    }
    publish(&temporary, &output_dir, stem, Some(format.extension()))
}

pub fn run_batch(inputs: &[PathBuf], format: Format) -> Result<Vec<PathBuf>> {
    if inputs.is_empty() {
        return Err("Chưa có tệp đầu vào.".to_owned());
    }
    if inputs.len() == 1 {
        return Ok(vec![run(&inputs[0], format)?]);
    }
    if format == Format::MergePdf {
        return Ok(vec![run_merge_pdfs(inputs)?]);
    }
    if format == Format::Pdf
        && inputs
            .iter()
            .all(|path| sniff::detect_extension_format(path).is_some_and(Format::is_image))
    {
        return Ok(vec![run_merge_pdf(inputs)?]);
    }
    let worker_count = inputs.len().min(4);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let outputs: std::sync::Mutex<Vec<(usize, Result<PathBuf>)>> =
        std::sync::Mutex::new(Vec::with_capacity(inputs.len()));
    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if index >= inputs.len() {
                        break;
                    }
                    let res = run(&inputs[index], format);
                    if let Ok(mut guard) = outputs.lock() {
                        guard.push((index, res));
                    }
                }
            });
        }
    });
    let mut collected = outputs.into_inner().unwrap_or_default();
    collected.sort_by_key(|(index, _)| *index);
    let results: Vec<Result<PathBuf>> = collected.into_iter().map(|(_, res)| res).collect();
    let mut paths = Vec::with_capacity(results.len());
    for result in results {
        paths.push(result?);
    }
    Ok(paths)
}

pub fn run_merge_pdf(inputs: &[PathBuf]) -> Result<PathBuf> {
    if inputs.is_empty() {
        return Err("Chưa có ảnh để gộp PDF.".to_owned());
    }
    let mut abs_inputs = Vec::with_capacity(inputs.len());
    for path in inputs {
        let abs = std::path::absolute(path).map_err(|error| error.to_string())?;
        if !abs.is_file() {
            return Err(format!("Không tìm thấy tệp {}", abs.display()));
        }
        let source = sniff::detect_extension_format(&abs)
            .filter(|f| f.is_image())
            .ok_or_else(|| format!("Tệp không phải ảnh: {}", abs.display()))?;
        sniff::verify(&abs, source)?;
        abs_inputs.push(abs);
    }
    let engines = Engines::detect();
    let first = &abs_inputs[0];
    let parent = first
        .parent()
        .ok_or("Không tìm được thư mục của tệp gốc.")?;
    let stem = first.file_stem().ok_or("Không đọc được tên tệp gốc.")?;
    let (stage, output_dir) = Staging::new_with_fallback(parent)?;
    let temporary = stage.0.join("merged.pdf");
    img2pdf::merge(&engines, &abs_inputs, &temporary, &stage.0)?;
    publish(&temporary, &output_dir, stem, Some("pdf"))
}

pub fn run_merge_pdfs(inputs: &[PathBuf]) -> Result<PathBuf> {
    if inputs.len() < 2 {
        return Err("Cần ít nhất 2 tệp PDF để gộp.".to_owned());
    }
    let mut abs_inputs = Vec::with_capacity(inputs.len());
    for path in inputs {
        let abs = std::path::absolute(path).map_err(|error| error.to_string())?;
        if !abs.is_file() {
            return Err(format!("Không tìm thấy tệp {}", abs.display()));
        }
        let source = sniff::detect_extension_format(&abs)
            .filter(|&f| f == Format::Pdf)
            .ok_or_else(|| format!("Tệp không phải PDF: {}", abs.display()))?;
        sniff::verify(&abs, source)?;
        abs_inputs.push(abs);
    }
    let engines = Engines::detect();
    let first = &abs_inputs[0];
    let parent = first
        .parent()
        .ok_or("Không tìm được thư mục của tệp gốc.")?;
    let stem = first.file_stem().ok_or("Không đọc được tên tệp gốc.")?;
    let (stage, output_dir) = Staging::new_with_fallback(parent)?;
    let temporary = stage.0.join("merged.pdf");
    pdf::merge(&engines, &abs_inputs, &temporary, &stage.0)?;
    let mut out_stem = stem.to_os_string();
    out_stem.push("-merged");
    publish(&temporary, &output_dir, &out_stem, Some("pdf"))
}

fn fallback_output_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CONVERTANYTHINGS_FALLBACK_DIR") {
        let path = PathBuf::from(dir);
        let _ = fs::create_dir_all(&path);
        return Some(path);
    }
    std::env::var_os("USERPROFILE").map(|home| {
        let downloads = PathBuf::from(&home).join("Downloads");
        if downloads.is_dir() || fs::create_dir_all(&downloads).is_ok() {
            downloads
        } else {
            PathBuf::from(home)
        }
    })
}

struct Staging(PathBuf);

impl Staging {
    fn new(parent: &Path) -> Result<Self> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        for count in 0..32 {
            let path = parent.join(format!(
                ".convert-anythings-{}-{now:x}-{count}",
                std::process::id()
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("Không tạo được kết quả cạnh tệp gốc. {error}")),
            }
        }
        Err("Không tạo được thư mục tạm duy nhất.".to_owned())
    }

    fn new_with_fallback(preferred_parent: &Path) -> Result<(Self, PathBuf)> {
        match Self::new(preferred_parent) {
            Ok(stage) => Ok((stage, preferred_parent.to_path_buf())),
            Err(primary_err) => {
                if let Some(fallback) = fallback_output_dir()
                    && fallback != preferred_parent
                    && let Ok(stage) = Self::new(&fallback)
                {
                    return Ok((stage, fallback));
                }
                Err(primary_err)
            }
        }
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn publish(
    temporary: &Path,
    parent: &Path,
    stem: &OsStr,
    extension: Option<&str>,
) -> Result<PathBuf> {
    let from = wide(temporary);
    for counter in 0..10_000 {
        let mut filename = stem.to_os_string();
        if counter > 0 {
            filename.push(format!(" ({counter})"));
        }
        if let Some(extension) = extension {
            filename.push(".");
            filename.push(extension);
        }
        let destination = parent.join(filename);
        let to = wide(&destination);
        let result = unsafe {
            MoveFileExW(
                PCWSTR(from.as_ptr()),
                PCWSTR(to.as_ptr()),
                MOVE_FILE_FLAGS(0),
            )
        };
        match result {
            Ok(()) => return Ok(destination),
            Err(error)
                if destination.exists()
                    || error.code().0 == 0x8007_00b7u32 as i32
                    || error.code().0 == 0x8007_0050u32 as i32 =>
            {
                continue;
            }
            Err(error) => return Err(format!("Không lưu được kết quả. {error}")),
        }
    }
    Err("Có quá nhiều tệp trùng tên.".to_owned())
}

fn execute(command: &mut Command, engine: &str) -> Result<()> {
    use std::process::Stdio;

    let result = command
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("Không chạy được {engine}. {error}"))?;
    if result.status.success() {
        return Ok(());
    }
    let diagnostic = String::from_utf8_lossy(&result.stderr);
    let diagnostic: String = diagnostic.trim().chars().take(300).collect();
    Err(format!("{engine} không thể chuyển tệp. {diagnostic}"))
}

pub fn child() -> std::io::Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    command.creation_flags(CREATE_NO_WINDOW.0);
    Ok(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_formats_parse_but_are_rejected_as_input_extensions() {
        for (raw, expected) in [
            ("compress", Format::Compress),
            ("merge-pdf", Format::MergePdf),
            ("split-pdf", Format::SplitPdf),
        ] {
            assert_eq!(Format::parse(OsStr::new(raw)), Some(expected));
            assert!(expected.is_action());
            assert_eq!(sniff::detect_extension_format(Path::new(&format!("file.{raw}"))), None);
        }
    }

    #[test]
    fn multi_pdf_and_mixed_files_offer_merge_and_compress() {
        let engines = Engines::default();
        let pdfs = vec![PathBuf::from("a.pdf"), PathBuf::from("b.pdf")];
        let (pdf_choices, reason) = engines.choices_for_files(&pdfs);
        assert_eq!(reason, None);
        assert_eq!(pdf_choices[0].format, Format::MergePdf);
        assert!(pdf_choices.iter().any(|c| c.format == Format::Compress));
        assert!(pdf_choices.iter().any(|c| c.format == Format::SplitPdf));

        let engines_with_ffmpeg = Engines {
            ffmpeg: Some(PathBuf::from("ffmpeg.exe")),
            ..Default::default()
        };
        let mixed = vec![
            PathBuf::from("photo.png"),
            PathBuf::from("clip.mp4"),
            PathBuf::from("doc.pdf"),
        ];
        let (mixed_choices, mixed_reason) = engines_with_ffmpeg.choices_for_files(&mixed);
        assert_eq!(mixed_reason, None);
        assert!(mixed_choices.iter().any(|c| c.format == Format::Compress));
    }
}
