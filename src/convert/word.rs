use std::fs::{self, File};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use windows::Win32::System::Registry::{HKEY_CLASSES_ROOT, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
use windows::core::{PCWSTR, w};

use super::{Format, Result};

fn com_available(key: PCWSTR) -> bool {
    let mut bytes = 0;
    unsafe {
        RegGetValueW(
            HKEY_CLASSES_ROOT,
            key,
            PCWSTR::null(),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        )
        .is_ok()
    }
}

pub fn word_available() -> bool {
    com_available(w!("Word.Application\\CLSID"))
}

pub fn excel_available() -> bool {
    com_available(w!("Excel.Application\\CLSID"))
}

pub fn powerpoint_available() -> bool {
    com_available(w!("PowerPoint.Application\\CLSID"))
}

pub fn convert(input: &Path, source: Format, output: &Path, stage: &Path) -> Result<()> {
    let normalized;
    let input = if source == Format::Txt {
        normalized = stage.join("source.txt");
        normalize_text(input, &normalized)?;
        &normalized
    } else {
        input
    };
    let app_kind = if source.is_excel_doc() {
        "excel"
    } else if source.is_ppt_doc() {
        "powerpoint"
    } else {
        "word"
    };
    let system = std::env::var_os("SystemRoot").ok_or("Không tìm được thư mục Windows.")?;
    let executable = Path::new(&system).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe");
    let log_path = stage.join("office.log");
    let log = File::create(&log_path).map_err(|error| error.to_string())?;
    let mut child = Command::new(executable)
        .creation_flags(CREATE_NO_WINDOW.0)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "& ([scriptblock]::Create($env:CONVERT_WORD_SCRIPT))",
        ])
        .env("CONVERT_WORD_SCRIPT", include_str!("word-export.ps1"))
        .env("CONVERT_WORD_INPUT", input)
        .env("CONVERT_WORD_OUTPUT", output)
        .env("CONVERT_OFFICE_APP", app_kind)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(log)
        .spawn()
        .map_err(|error| format!("Không mở được bộ chuyển đổi Office. {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        match child.try_wait().map_err(|error| error.to_string())? {
            Some(status) if status.success() && output.is_file() => return Ok(()),
            Some(_) => {
                let message = fs::read_to_string(&log_path).unwrap_or_default();
                let detail: String = message.trim().chars().take(350).collect();
                return Err(format!("Office không thể chuyển tệp. {detail}"));
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(
                    "Office không hoàn tất trong 90 giây. Hãy kiểm tra hộp thoại hoặc trạng thái kích hoạt.".to_owned(),
                );
            }
            None => std::thread::sleep(Duration::from_millis(80)),
        }
    }
}

fn normalize_text(input: &Path, output: &Path) -> Result<()> {
    let bytes = fs::read(input).map_err(|error| error.to_string())?;
    let text = match bytes.as_slice() {
        [0xff, 0xfe, rest @ ..] | [0xfe, 0xff, rest @ ..] => {
            if !rest.len().is_multiple_of(2) {
                return Err("Tệp TXT UTF-16 không đầy đủ.".to_owned());
            }
            let little_endian = bytes[0] == 0xff;
            let units: Vec<_> = rest
                .as_chunks::<2>()
                .0
                .iter()
                .map(|&unit| {
                    if little_endian {
                        u16::from_le_bytes(unit)
                    } else {
                        u16::from_be_bytes(unit)
                    }
                })
                .collect();
            String::from_utf16(&units).map_err(|_| "Tệp TXT UTF-16 không hợp lệ.")?
        }
        bytes => std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
            .map_err(|_| "TXT cần dùng UTF-8 hoặc UTF-16 có BOM.")?
            .to_owned(),
    };
    let mut with_bom = vec![0xef, 0xbb, 0xbf];
    with_bom.extend_from_slice(text.as_bytes());
    fs::write(output, with_bom).map_err(|error| error.to_string())
}
