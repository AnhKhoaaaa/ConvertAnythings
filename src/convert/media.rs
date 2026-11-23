use std::path::Path;

use super::{Choice, Engines, Format, Group, Result, execute};

pub fn choices(input: Format) -> Vec<Choice> {
    let outputs: &[(Format, Group)] = if input.is_video() {
        &[
            (Format::Mp4, Group::Video),
            (Format::Webm, Group::Video),
            (Format::Mp3, Group::Audio),
        ]
    } else {
        &[
            (Format::Mp3, Group::Audio),
            (Format::Wav, Group::Audio),
            (Format::Flac, Group::Audio),
        ]
    };
    outputs
        .iter()
        .filter(|(format, _)| *format != input)
        .map(|&(format, group)| Choice { group, format })
        .collect()
}

pub fn convert(engines: &Engines, input: &Path, output: Format, destination: &Path) -> Result<()> {
    let mut command = engines.ffmpeg()?;
    command
        .arg("-i")
        .arg(input)
        .args(["-sn", "-dn", "-map_chapters", "-1"]);
    if output.is_video() {
        command.args([
            "-map",
            "0:V:0",
            "-map",
            "0:a:0?",
            "-vf",
            "pad=ceil(iw/2)*2:ceil(ih/2)*2",
            "-pix_fmt",
            "yuv420p",
        ]);
    } else {
        command.args(["-map", "0:a:0", "-vn"]);
    }
    let arguments: &[&str] = match output {
        Format::Mp4 => &[
            "-c:v",
            "libx264",
            "-crf",
            "23",
            "-preset",
            "fast",
            "-c:a",
            "aac",
            "-b:a",
            "160k",
            "-movflags",
            "+faststart",
        ],
        Format::Webm => &[
            "-c:v",
            "libvpx-vp9",
            "-b:v",
            "0",
            "-crf",
            "32",
            "-deadline",
            "good",
            "-cpu-used",
            "4",
            "-c:a",
            "libopus",
            "-b:a",
            "128k",
        ],
        Format::Mp3 => &["-c:a", "libmp3lame", "-q:a", "3", "-ac", "2"],
        Format::Wav => &["-c:a", "pcm_s16le"],
        Format::Flac => &["-c:a", "flac"],
        _ => return Err("Định dạng âm thanh hoặc video chưa được hỗ trợ.".to_owned()),
    };
    execute(command.args(arguments).arg(destination), "FFmpeg")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_catalog_hides_identity_conversions_and_keeps_audio_extraction() {
        assert_eq!(
            choices(Format::Mp4),
            [
                Choice {
                    group: Group::Video,
                    format: Format::Webm
                },
                Choice {
                    group: Group::Audio,
                    format: Format::Mp3
                }
            ]
        );
        assert_eq!(
            choices(Format::Wav),
            [
                Choice {
                    group: Group::Audio,
                    format: Format::Mp3
                },
                Choice {
                    group: Group::Audio,
                    format: Format::Flac
                }
            ]
        );
    }
}
