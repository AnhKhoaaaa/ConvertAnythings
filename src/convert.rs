use std::ffi::OsStr;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use windows::Win32::Storage::FileSystem::{MOVE_FILE_FLAGS, MoveFileExW};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
use windows::core::PCWSTR;

use crate::wide;

mod image;
mod media;
mod pdf;
mod word;

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Jpg,
    Png,
    Bmp,
    Tiff,
    Webp,
    Pdf,
    Txt,
    Docx,
    Mp4,
    Mov,
    Mkv,
    Webm,
    Mp3,
    Wav,
    Flac,
    M4a,
    Ogg,
}

impl Format {
    pub fn parse(value: &OsStr) -> Option<Self> {
        match value.to_str()?.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Some(Self::Jpg),
            "png" => Some(Self::Png),
            "bmp" => Some(Self::Bmp),
            "tif" | "tiff" => Some(Self::Tiff),
            "webp" => Some(Self::Webp),
            "pdf" => Some(Self::Pdf),
            "txt" => Some(Self::Txt),
            "docx" => Some(Self::Docx),
            "mp4" => Some(Self::Mp4),
            "mov" => Some(Self::Mov),
            "mkv" => Some(Self::Mkv),
            "webm" => Some(Self::Webm),
            "mp3" => Some(Self::Mp3),
            "wav" => Some(Self::Wav),
            "flac" => Some(Self::Flac),
            "m4a" => Some(Self::M4a),
            "ogg" => Some(Self::Ogg),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Jpg => "jpg",
            Self::Png => "png",
            Self::Bmp => "bmp",
            Self::Tiff => "tiff",
            Self::Webp => "webp",
            Self::Pdf => "pdf",
            Self::Txt => "txt",
            Self::Docx => "docx",
            Self::Mp4 => "mp4",
            Self::Mov => "mov",
            Self::Mkv => "mkv",
            Self::Webm => "webm",
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::M4a => "m4a",
            Self::Ogg => "ogg",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Jpg => "JPG",
            Self::Png => "PNG",
            Self::Bmp => "BMP",
            Self::Tiff => "TIFF",
            Self::Webp => "WebP",
            Self::Pdf => "PDF",
            Self::Txt => "TXT",
            Self::Docx => "DOCX",
            Self::Mp4 => "MP4",
            Self::Mov => "MOV",
            Self::Mkv => "MKV",
            Self::Webm => "WebM",
            Self::Mp3 => "MP3",
            Self::Wav => "WAV",
            Self::Flac => "FLAC",
            Self::M4a => "M4A",
            Self::Ogg => "OGG",
        }
    }

    fn is_video(self) -> bool {
        matches!(self, Self::Mp4 | Self::Mov | Self::Mkv | Self::Webm)
    }

    fn is_audio(self) -> bool {
        matches!(
            self,
            Self::Mp3 | Self::Wav | Self::Flac | Self::M4a | Self::Ogg
        )
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

const IMAGES: &[Format] = &[
    Format::Jpg,
    Format::Png,
    Format::Bmp,
    Format::Tiff,
    Format::Webp,
];

#[derive(Default)]
pub struct Engines {
    ffmpeg: Option<PathBuf>,
    word: bool,
}

impl Engines {
    pub fn detect() -> Self {
        let ffmpeg = std::env::var_os("PATH").and_then(|path| {
            std::env::split_paths(&path)
                .filter(|directory| directory.is_absolute())
                .map(|directory| directory.join("ffmpeg.exe"))
                .find(|executable| executable.is_file())
        });
        Self {
            ffmpeg,
            word: word::available(),
        }
    }

    pub fn choices(&self, path: &Path) -> (Vec<Choice>, Option<&'static str>) {
        let Some(input) = path.extension().and_then(Format::parse) else {
            return (Vec::new(), Some("Định dạng chưa hỗ trợ"));
        };
        if input == Format::Pdf {
            return (
                vec![
                    Choice {
                        group: Group::Image,
                        format: Format::Jpg,
                    },
                    Choice {
                        group: Group::Image,
                        format: Format::Png,
                    },
                ],
                None,
            );
        }
        if matches!(input, Format::Txt | Format::Docx) {
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
        if input.is_video() || input.is_audio() {
            if self.ffmpeg.is_none() {
                return (Vec::new(), Some("Cần FFmpeg"));
            }
            return (media::choices(input), None);
        }
        if input == Format::Webp && self.ffmpeg.is_none() {
            return (Vec::new(), Some("WebP cần FFmpeg"));
        }
        let choices = IMAGES
            .iter()
            .copied()
            .filter(|format| *format != input && (*format != Format::Webp || self.ffmpeg.is_some()))
            .map(|format| Choice {
                group: Group::Image,
                format,
            })
            .collect();
        (choices, None)
    }

    fn ffmpeg(&self) -> Result<Command> {
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
}

pub fn run(input: &Path, format: Format) -> Result<PathBuf> {
    let input = std::path::absolute(input).map_err(|error| error.to_string())?;
    if !input.is_file() {
        return Err("Hãy chọn một tệp có thể đọc được.".to_owned());
    }
    let engines = Engines::detect();
    let (choices, reason) = engines.choices(&input);
    if !choices.iter().any(|choice| choice.format == format) {
        return Err(reason
            .unwrap_or("Không hỗ trợ cặp định dạng này hoặc thiếu bộ chuyển đổi.")
            .to_owned());
    }
    let source = input
        .extension()
        .and_then(Format::parse)
        .ok_or("Không nhận diện được định dạng tệp.")?;
    let parent = input
        .parent()
        .ok_or("Không tìm được thư mục của tệp gốc.")?;
    let stem = input.file_stem().ok_or("Không đọc được tên tệp gốc.")?;
    let stage = Staging::new(parent)?;
    let temporary = stage.0.join(format!("result.{}", format.extension()));
    match source {
        Format::Pdf => {
            if pdf::convert(&input, format, &temporary, &stage.0, stem)? {
                let mut folder_name = stem.to_os_string();
                folder_name.push("-pages");
                return publish(&stage.0.join("pages"), parent, &folder_name, None);
            }
        }
        Format::Txt | Format::Docx => word::convert(&input, source, &temporary, &stage.0)?,
        format_source if format_source.is_audio() || format_source.is_video() => {
            media::convert(&engines, &input, format, &temporary)?;
        }
        _ => image::convert(&engines, source, &input, format, &temporary, &stage.0)?,
    }
    publish(&temporary, parent, stem, Some(format.extension()))
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
