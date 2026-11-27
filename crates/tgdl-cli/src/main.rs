use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tgdl_core::{
    DownloadPlan, bots::BotPool, config::Config, model::FileMeta, scheduler::Scheduler, verify,
};

mod args;
mod ui;

use args::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    ui::init_tracing();
    let cli = Cli::parse();
    let cfg = Config::load(&cli.config)?;

    match cli.command {
        Command::Download {
            link,
            output,
            dry_run,
        } => {
            let pool = BotPool::from_config(&cfg).await?;
            let bot = pool
                .first()
                .ok_or_else(|| anyhow::anyhow!("no bots configured"))?;
            let source =
                tgdl_core::source::telegram::TelegramSource::resolve(bot.client.clone(), &link)
                    .await?;
            let meta = FileMeta {
                unique_id: source.meta_unique_id(),
                size: source.size,
                chunk_size: cfg.chunk_size_bytes().min(512 * 1024),
            };
            let plan = DownloadPlan::new(meta.clone());
            if dry_run {
                println!(
                    "would download {} bytes in {} chunks",
                    meta.size,
                    plan.chunks.len()
                );
                return Ok(());
            }
            let filename = source.file_name.clone();
            let out_dir = output.unwrap_or_else(|| PathBuf::from(&cfg.download.output_dir));
            tokio::fs::create_dir_all(&out_dir).await?;
            let part_path = out_dir.join(format!("{}.part", filename));
            Scheduler::run_sequential(&source, &plan, &part_path).await?;
            verify::verify_size(&part_path, meta.size)?;
            let final_path = out_dir.join(filename);
            tokio::fs::rename(&part_path, &final_path).await?;
            println!("saved {}", final_path.display());
        }
    }
    Ok(())
}
