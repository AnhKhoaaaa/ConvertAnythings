use std::fs;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::UNIX_EPOCH;

use windows::Win32::System::Threading::CREATE_NO_WINDOW;

use super::{Choice, Engines, Format, Group, Result, execute};

pub fn choices(input: Format) -> Vec<Choice> {
    let outputs: &[(Format, Group)] = if input.is_video() {
        &[
            (Format::Compress, Group::Video),
            (Format::Mp4, Group::Video),
            (Format::Webm, Group::Video),
            (Format::Mov, Group::Video),
            (Format::Gif, Group::Image),
            (Format::Mp3, Group::Audio),
            (Format::Wav, Group::Audio),
            (Format::M4a, Group::Audio),
        ]
    } else {
        &[
            (Format::Mp3, Group::Audio),
            (Format::Wav, Group::Audio),
            (Format::M4a, Group::Audio),
            (Format::Flac, Group::Audio),
            (Format::Ogg, Group::Audio),
        ]
    };
    outputs
        .iter()
        .filter(|(format, _)| *format != input)
        .map(|&(format, group)| Choice { group, format })
        .collect()
}

pub fn compressed_extension(source: Format) -> &'static str {
    if source == Format::Webm { "webm" } else { "mp4" }
}

pub fn compress(engines: &Engines, source: Format, input: &Path, destination: &Path) -> Result<()> {
    let filter =
        "scale='min(1920,iw)':-2:force_original_aspect_ratio=decrease,pad=ceil(iw/2)*2:ceil(ih/2)*2";
    if source == Format::Webm {
        let mut cmd = build_base_command_with_filter(engines, input, true, filter);
        cmd.args([
            "-c:v",
            "libvpx-vp9",
            "-b:v",
            "0",
            "-crf",
            "36",
            "-deadline",
            "realtime",
            "-cpu-used",
            "6",
            "-row-mt",
            "1",
            "-threads",
            "0",
            "-c:a",
            "libopus",
            "-b:a",
            "96k",
        ])
        .arg(destination);
        return execute(&mut cmd, "FFmpeg");
    }

    if let Some(hw) = detect_hw_h264(engines) {
        let mut cmd = build_base_command_with_filter(engines, input, true, filter);
        match hw {
            "h264_nvenc" => {
                cmd.args(["-c:v", "h264_nvenc", "-preset", "p4", "-cq", "28"]);
            }
            "h264_qsv" => {
                cmd.args(["-c:v", "h264_qsv", "-global_quality", "28"]);
            }
            _ => {
                cmd.args(["-c:v", "h264_amf", "-quality", "balanced", "-qp_i", "28", "-qp_p", "28"]);
            }
        }
        cmd.args(["-c:a", "aac", "-b:a", "96k", "-movflags", "+faststart"])
            .arg(destination);
        if execute(&mut cmd, "FFmpeg").is_ok() {
            return Ok(());
        }
        let _ = fs::remove_file(destination);
    }

    let mut sw = build_base_command_with_filter(engines, input, true, filter);
    sw.args([
        "-c:v",
        "libx264",
        "-crf",
        "28",
        "-preset",
        "faster",
        "-c:a",
        "aac",
        "-b:a",
        "96k",
        "-movflags",
        "+faststart",
    ])
    .arg(destination);
    execute(&mut sw, "FFmpeg")
}

fn probe_codecs(engines: &Engines, input: &Path) -> (Option<String>, Option<String>) {
    let Some(mut cmd) = engines.ffprobe() else {
        return (None, None);
    };
    let Ok(output) = cmd
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_type,codec_name",
            "-of",
            "default=noprint_wrappers=1:nokey=0",
        ])
        .arg(input)
        .output()
    else {
        return (None, None);
    };
    if !output.status.success() {
        return (None, None);
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut video = None;
    let mut audio = None;
    let mut current_name: Option<String> = None;
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("codec_name=") {
            current_name = Some(name.trim().to_ascii_lowercase());
        } else if let Some(kind) = line.strip_prefix("codec_type=") {
            match kind.trim() {
                "video" if video.is_none() => video = current_name.take(),
                "audio" if audio.is_none() => audio = current_name.take(),
                _ => {
                    current_name = None;
                }
            }
        }
    }
    (video, audio)
}

fn detect_hw_h264(engines: &Engines) -> Option<&'static str> {
    let ffmpeg_path = engines.ffmpeg_path()?;
    let meta = fs::metadata(ffmpeg_path).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let signature = format!("{}|{}|{}", ffmpeg_path.display(), meta.len(), mtime);

    let cache_path = crate::config_dir().map(|dir| dir.join("hw_encoder_cache.txt"));
    if let Some(path) = &cache_path
        && let Ok(cached) = fs::read_to_string(path)
    {
        let mut lines = cached.lines();
        if lines.next() == Some(signature.as_str())
            && let Some(val) = lines.next()
        {
            return match val.trim() {
                "h264_nvenc" => Some("h264_nvenc"),
                "h264_qsv" => Some("h264_qsv"),
                "h264_amf" => Some("h264_amf"),
                "none" => return None,
                _ => None,
            };
        }
    }

    let mut detected = None;
    for hw in ["h264_nvenc", "h264_qsv", "h264_amf"] {
        let ok = Command::new(ffmpeg_path)
            .creation_flags(CREATE_NO_WINDOW.0)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=256x256:d=0.04",
                "-frames:v",
                "1",
                "-c:v",
                hw,
                "-f",
                "null",
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            detected = Some(hw);
            break;
        }
    }

    if let Some(path) = cache_path {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, format!("{signature}\n{}", detected.unwrap_or("none")));
    }

    detected
}

pub fn convert(engines: &Engines, input: &Path, output: Format, destination: &Path) -> Result<()> {
    if output == Format::Gif {
        let mut command = engines.ffmpeg()?;
        command.arg("-i").arg(input).args([
            "-vf",
            "fps=12,scale='min(640,iw)':-1:flags=lanczos,split[s0][s1];[s0]palettegen=stats_mode=diff[p];[s1][p]paletteuse=dither=bayer:bayer_scale=3",
            "-loop",
            "0",
        ]);
        return execute(command.arg(destination), "FFmpeg");
    }

    let (src_video, src_audio) = probe_codecs(engines, input);

    // Fast stream copy (remux) when container/codecs are already compatible
    if matches!(output, Format::Mp4 | Format::Mov)
        && matches!(src_video.as_deref(), Some("h264" | "hevc"))
        && matches!(src_audio.as_deref(), None | Some("aac" | "mp3"))
    {
        let mut remux = engines.ffmpeg()?;
        remux
            .arg("-i")
            .arg(input)
            .args([
                "-sn",
                "-dn",
                "-map_chapters",
                "-1",
                "-map",
                "0:V:0",
                "-map",
                "0:a:0?",
                "-c:v",
                "copy",
                "-c:a",
                "copy",
                "-movflags",
                "+faststart",
            ])
            .arg(destination);
        if execute(&mut remux, "FFmpeg").is_ok() {
            return Ok(());
        }
        let _ = fs::remove_file(destination);
    }

    if output == Format::M4a && src_audio.as_deref() == Some("aac") {
        let mut remux = engines.ffmpeg()?;
        remux
            .arg("-i")
            .arg(input)
            .args([
                "-sn",
                "-dn",
                "-map_chapters",
                "-1",
                "-map",
                "0:a:0",
                "-vn",
                "-c:a",
                "copy",
                "-movflags",
                "+faststart",
            ])
            .arg(destination);
        if execute(&mut remux, "FFmpeg").is_ok() {
            return Ok(());
        }
        let _ = fs::remove_file(destination);
    }

    if matches!(output, Format::Mp4 | Format::Mov) {
        if let Some(hw) = detect_hw_h264(engines) {
            let mut cmd = build_base_command(engines, input, true);
            match hw {
                "h264_nvenc" => {
                    cmd.args(["-c:v", "h264_nvenc", "-preset", "p4", "-cq", "23"]);
                }
                "h264_qsv" => {
                    cmd.args(["-c:v", "h264_qsv", "-global_quality", "23"]);
                }
                _ => {
                    cmd.args(["-c:v", "h264_amf", "-quality", "balanced"]);
                }
            }
            cmd.args(["-c:a", "aac", "-b:a", "160k", "-movflags", "+faststart"])
                .arg(destination);
            if execute(&mut cmd, "FFmpeg").is_ok() {
                return Ok(());
            }
            let _ = fs::remove_file(destination);
        }

        let mut sw = build_base_command(engines, input, true);
        sw.args([
            "-c:v",
            "libx264",
            "-crf",
            "23",
            "-preset",
            "veryfast",
            "-c:a",
            "aac",
            "-b:a",
            "160k",
            "-movflags",
            "+faststart",
        ])
        .arg(destination);
        return execute(&mut sw, "FFmpeg");
    }

    let mut command = build_base_command(engines, input, output.is_video());
    let arguments: &[&str] = match output {
        Format::Webm => &[
            "-c:v",
            "libvpx-vp9",
            "-b:v",
            "0",
            "-crf",
            "32",
            "-deadline",
            "realtime",
            "-cpu-used",
            "6",
            "-row-mt",
            "1",
            "-threads",
            "0",
            "-c:a",
            "libopus",
            "-b:a",
            "128k",
        ],
        Format::Mp3 => &["-c:a", "libmp3lame", "-q:a", "3", "-ac", "2"],
        Format::Wav => &["-c:a", "pcm_s16le"],
        Format::M4a => &["-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"],
        Format::Flac => &["-c:a", "flac"],
        Format::Ogg => &["-c:a", "libopus", "-b:a", "128k"],
        _ => return Err("Định dạng âm thanh hoặc video chưa được hỗ trợ.".to_owned()),
    };
    execute(command.args(arguments).arg(destination), "FFmpeg")
}

fn build_base_command(engines: &Engines, input: &Path, is_video: bool) -> Command {
    build_base_command_with_filter(engines, input, is_video, "pad=ceil(iw/2)*2:ceil(ih/2)*2")
}

fn build_base_command_with_filter(
    engines: &Engines,
    input: &Path,
    is_video: bool,
    video_filter: &str,
) -> Command {
    let mut command = engines.ffmpeg().expect("ffmpeg checked before media convert");
    command
        .arg("-i")
        .arg(input)
        .args(["-sn", "-dn", "-map_chapters", "-1"]);
    if is_video {
        command.args([
            "-map",
            "0:V:0",
            "-map",
            "0:a:0?",
            "-vf",
            video_filter,
            "-pix_fmt",
            "yuv420p",
        ]);
    } else {
        command.args(["-map", "0:a:0", "-vn"]);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_catalog_hides_identity_conversions_and_keeps_audio_extraction() {
        let mp4_choices = choices(Format::Mp4);
        assert!(!mp4_choices.iter().any(|c| c.format == Format::Mp4));
        assert!(mp4_choices.iter().any(|c| c.format == Format::Webm));
        assert!(mp4_choices.iter().any(|c| c.format == Format::Mp3));

        let wav_choices = choices(Format::Wav);
        assert!(!wav_choices.iter().any(|c| c.format == Format::Wav));
        assert!(wav_choices.iter().any(|c| c.format == Format::Mp3));
        assert!(wav_choices.iter().any(|c| c.format == Format::Flac));
        assert!(wav_choices.iter().any(|c| c.format == Format::M4a));
    }
}
