#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod convert;
mod menu;
mod render;
mod ui;

use std::{ffi::OsStr, fs::OpenOptions, io::Write, os::windows::ffi::OsStrExt, path::PathBuf};

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain([0]).collect()
}

pub(crate) fn config_dir() -> Option<PathBuf> {
    std::env::var_os("CONVERTANYTHINGS_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("ConvertAnythings"))
        })
}

fn trace(message: impl AsRef<str>) {
    if let Some(path) = std::env::var_os("CONVERTANYTHINGS_TRACE")
        && let Ok(mut file) = OpenOptions::new().append(true).create(true).open(path)
    {
        let _ = writeln!(file, "{}", message.as_ref());
    }
}

fn main() -> std::process::ExitCode {
    let mut args = std::env::args_os().skip(1);
    if let Some(command) = args.next() {
        let result: Result<Vec<PathBuf>, String> =
            if command == "--convert" && args.next().as_deref() == Some(OsStr::new("--to")) {
                match args.next() {
                    Some(format) => {
                        let inputs: Vec<PathBuf> = args.map(PathBuf::from).collect();
                        if inputs.is_empty() {
                            Err("Cú pháp: --convert --to <format> <path...>".to_owned())
                        } else {
                            convert::Format::parse(&format)
                                .ok_or_else(|| "Định dạng đầu ra không được hỗ trợ.".to_owned())
                                .and_then(|format| convert::run_batch(&inputs, format))
                        }
                    }
                    None => Err("Cú pháp: --convert --to <format> <path...>".to_owned()),
                }
            } else {
                Err("Cú pháp: --convert --to <format> <path...>".to_owned())
            };
        return match result {
            Ok(paths) => {
                for path in paths {
                    println!("{}", path.display());
                }
                std::process::ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::ExitCode::FAILURE
            }
        };
    }
    match ui::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            trace(format!("error {error}"));
            ui::show_error(&error.to_string());
            std::process::ExitCode::FAILURE
        }
    }
}
