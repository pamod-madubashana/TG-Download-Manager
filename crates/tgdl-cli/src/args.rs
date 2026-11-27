use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "tgdl", version, about = "Telegram multi-bot downloader")]
pub struct Cli {
    #[arg(long, default_value = "config.toml")]
    pub config: PathBuf,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    Download {
        link: String,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
    },
}
