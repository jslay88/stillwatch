//! The `stillwatch-gui` binary: tray, settings window, and prompt mode.

use std::process::ExitCode;

use clap::Parser as _;

fn main() -> ExitCode {
    match stillwatch_gui::run(&stillwatch_gui::Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("stillwatch-gui: {err}");
            ExitCode::FAILURE
        }
    }
}
