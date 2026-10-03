//! The `stillwatch-gui` binary: tray, settings window, and prompt mode.

use std::process::ExitCode;

use clap::Parser as _;

fn main() -> ExitCode {
    match stillwatch_gui::run(&stillwatch_gui::Cli::parse()) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(err) => {
            eprintln!("stillwatch-gui: {err}");
            ExitCode::FAILURE
        }
    }
}
