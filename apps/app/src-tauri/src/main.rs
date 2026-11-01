// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(e) = app_lib::run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
