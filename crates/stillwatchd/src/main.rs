//! The Stillwatch daemon.

use clap::Parser as _;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    stillwatchd::run(stillwatchd::Args::parse()).await
}
