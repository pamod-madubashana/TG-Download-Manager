use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tgdl_core::bots::{add_bots, ensure_chat, listener};
use tgdl_core::scheduler::SchedulerOptions;
use tgdl_core::source::ChunkSource;
use tgdl_core::{
    Config, DownloadPlan, bots::BotPool, model::FileMeta, scheduler::Scheduler, verify,
};

mod args;
mod ui;

use args::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    ui::init_tracing();
    let cli = Cli::parse();
    let mut cfg = Config::load()?;

    match cli.command {
        Command::Login => {
            let pool = BotPool::from_config(&cfg).await?;
            for bot in &pool.bots {
                println!("signed in as {}", bot.name);
            }
        }
        Command::Setup => {
            let pool = BotPool::from_config(&cfg).await?;
            let bot = pool
                .first()
                .ok_or_else(|| anyhow::anyhow!("no bots configured"))?;
            let chat = ensure_chat(&bot.client, &cfg).await?;
            add_bots(&bot.client, chat, &cfg).await?;
            cfg.chat.chat_id = Some(chat.id);
            cfg.chat.access_hash = Some(chat.access_hash);
            cfg.save()?;
            println!("chat {} ready with {} bot(s)", chat.id, cfg.bots.len());
        }
        Command::Listen => {
            let mut pool = BotPool::from_config(&cfg).await?;
            let chat_id = cfg
                .chat
                .chat_id
                .ok_or_else(|| anyhow::anyhow!("run setup first"))?;
            let updates = pool
                .take_updates(0)
                .ok_or_else(|| anyhow::anyhow!("update stream already used"))?;
            let bot = pool
                .first()
                .ok_or_else(|| anyhow::anyhow!("no bots configured"))?;
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            tokio::spawn(listener::listen(bot.client.clone(), updates, chat_id, tx));
            while let Some(file) = rx.recv().await {
                println!("queued {} ({})", file.name, file.link);
            }
        }
        Command::Download {
            link,
            output,
            dry_run,
        } => {
            let pool = BotPool::from_config(&cfg).await?;
            if pool.bots.is_empty() {
                anyhow::bail!("no bots configured");
            }

            let mut sources: Vec<Arc<dyn ChunkSource>> = Vec::new();
            let mut meta: Option<FileMeta> = None;
            let mut filename = String::from("download.bin");
            for bot in &pool.bots {
                let source =
                    tgdl_core::source::telegram::TelegramSource::resolve(bot.client.clone(), &link)
                        .await?;
                if meta.is_none() {
                    filename = source.file_name.clone();
                    meta = Some(FileMeta {
                        unique_id: source.meta_unique_id(),
                        size: source.size,
                        chunk_size: cfg.chunk_size_bytes().min(512 * 1024),
                    });
                }
                sources.push(Arc::new(source));
            }
            let meta = meta.expect("source");
            let plan = DownloadPlan::new(meta.clone());
            if dry_run {
                println!(
                    "would download {} bytes in {} chunks using {} bot(s)",
                    meta.size,
                    plan.chunks.len(),
                    sources.len()
                );
                return Ok(());
            }

            let out_dir = output.unwrap_or_else(|| PathBuf::from(&cfg.download.output_dir));
            tokio::fs::create_dir_all(&out_dir).await?;
            let part_path = out_dir.join(format!("{filename}.part"));
            let options = SchedulerOptions {
                slots_per_bot: cfg.download.slots_per_bot.max(1),
                max_attempts: cfg.download.max_attempts_per_chunk.max(1),
                ..Default::default()
            };
            let stats = Scheduler::run(sources, &plan, &part_path, None, options).await?;
            verify::verify_size(&part_path, meta.size)?;
            let final_path = out_dir.join(filename);
            tokio::fs::rename(&part_path, &final_path).await?;

            println!(
                "saved {} ({} bytes)",
                final_path.display(),
                stats.total_bytes
            );
            for (i, bytes) in stats.per_source_bytes.iter().enumerate() {
                println!("  bot {}: {} bytes", i + 1, bytes);
            }
            println!(
                "average speed: {:.2} MiB/s",
                stats.total_bytes as f64 / stats.elapsed.as_secs_f64().max(0.001) / 1024.0 / 1024.0
            );
        }
    }
    Ok(())
}
