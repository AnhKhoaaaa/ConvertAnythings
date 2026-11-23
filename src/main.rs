#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ui;

use std::{ffi::OsStr, fs::OpenOptions, io::Write, os::windows::ffi::OsStrExt};

fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain([0]).collect()
}

fn trace(message: impl AsRef<str>) {
    if let Some(path) = std::env::var_os("CONVERTANYTHINGS_TRACE")
        && let Ok(mut file) = OpenOptions::new().append(true).create(true).open(path)
    {
        let _ = writeln!(file, "{}", message.as_ref());
    }
}

fn main() -> std::process::ExitCode {
    match ui::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            trace(format!("error {error}"));
            ui::show_error(&error.to_string());
            std::process::ExitCode::FAILURE
        }
    }
}
