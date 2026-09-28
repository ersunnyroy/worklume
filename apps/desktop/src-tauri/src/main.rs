// Release builds on Windows run without a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    match worklume_desktop::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            #[expect(
                clippy::print_stderr,
                reason = "no logger exists if startup fails; file logging arrives in E09-S11"
            )]
            {
                eprintln!("Worklume failed to start: {error}");
            }
            ExitCode::FAILURE
        }
    }
}
