// Prevents additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(unused_crate_dependencies, missing_docs)]

use std::process::ExitCode;

fn main() -> ExitCode {
    if let Err(e) = canopus_desktop_lib::run() {
        eprintln!("Fatal: {e}");
        return ExitCode::from(1);
    }

    ExitCode::SUCCESS
}
