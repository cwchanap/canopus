// Prevents additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(e) = canopus_desktop_lib::run() {
        eprintln!("Fatal: {e}");
        std::process::exit(1);
    }
}
