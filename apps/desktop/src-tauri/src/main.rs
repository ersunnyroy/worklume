// Release builds on Windows run without a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    worklume_desktop::start()
}
