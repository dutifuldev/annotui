use clap::Parser;

use annotui::{cli::Cli, runner};

fn main() -> anyhow::Result<()> {
    runner::run(&Cli::parse())
}
