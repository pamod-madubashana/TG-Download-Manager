use dioxus::prelude::*;
use futures_channel::mpsc::UnboundedSender;
use futures_util::StreamExt;
use std::sync::{OnceLock, RwLock};
use std::time::Instant;
use tgdl_core::bots::{QueuedFile, add_bots, ensure_chat, listener};
use tgdl_core::source::ChunkSource;
use tgdl_core::source::telegram::TelegramSource;
use tgdl_core::{BotPool, Config, DownloadPlan, FileMeta, Scheduler, SchedulerOptions, verify};
use tokio::sync::mpsc as tokio_mpsc;

const CHEVRON_PATH: &str =
    "M18 8L12.2278 14.7343C12.108 14.8739 11.892 14.8739 11.7722 14.7343L6 8";
const UPLOAD_PATH: &str = "M445.767,308.42l-53.374-76.49v-20.656v-11.366V97.241c0-6.669-2.604-12.94-7.318-17.645L312.787,7.301C308.073,2.588,301.796,0,295.149,0H77.597C54.161,0,35.103,19.066,35.103,42.494V425.68c0,23.427,19.059,42.494,42.494,42.494h159.307h39.714c1.902,2.54,3.915,5,6.232,7.205c10.033,9.593,23.547,15.576,38.501,15.576c26.935,0-1.247,0,34.363,0c14.936,0,28.483-5.982,38.517-15.576c11.693-11.159,17.348-25.825,17.348-40.29v-40.06c16.216-3.418,30.114-13.866,37.91-28.811C459.151,347.704,457.731,325.554,445.767,308.42z M170.095,414.872H87.422V53.302h175.681v46.752c0,16.655,13.547,30.209,30.209,30.209h46.76v66.377h-0.255v0.039c-17.685-0.415-35.529,7.285-46.934,23.46l-61.586,88.28c-11.965,17.134-13.387,39.284-3.722,57.799c7.795,14.945,21.692,25.393,37.91,28.811v19.842h-10.29H170.095z M410.316,345.771c-2.03,3.866-5.99,6.271-10.337,6.271h-0.016h-32.575v83.048c0,6.437-5.239,11.662-11.659,11.662h-0.017H321.35h-0.017c-6.423,0-11.662-5.225-11.662-11.662v-83.048h-32.574h-0.016c-4.346,0-8.308-2.405-10.336-6.271c-2.012-3.866-1.725-8.49,0.783-12.07l61.424-88.064c2.189-3.123,5.769-4.984,9.57-4.984h0.017c3.802,0,7.38,1.861,9.568,4.984l61.427,88.064C412.04,337.28,412.328,341.905,410.316,345.771z";

static RUNTIME: OnceLock<tokio::runtime::Handle> = OnceLock::new();
static EVENT_TX: OnceLock<RwLock<Option<UnboundedSender<UiEvent>>>> = OnceLock::new();

fn runtime() -> tokio::runtime::Handle {
    RUNTIME
        .get_or_init(|| {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("tokio runtime");
            let handle = rt.handle().clone();
            Box::leak(Box::new(rt));
            handle
        })
        .clone()
}

fn event_bus() -> &'static RwLock<Option<UnboundedSender<UiEvent>>> {
    EVENT_TX.get_or_init(|| RwLock::new(None))
}

fn send_event(event: UiEvent) {
    if let Ok(guard) = event_bus().read()
        && let Some(tx) = guard.as_ref()
    {
        let _ = tx.unbounded_send(event);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Downloading,
    Completed,
    Failed,
}

#[derive(Clone)]
struct DownloadRow {
    id: usize,
    name: String,
    url: String,
    size: u64,
    downloaded: u64,
    speed: f64,
    status: Status,
    last_bytes: u64,
    last_at: Instant,
    path: String,
}

impl DownloadRow {
    fn new(id: usize, url: String, name: Option<String>) -> Self {
        let name = name.unwrap_or_else(|| {
            url.rsplit('/')
                .find(|s| !s.is_empty())
                .unwrap_or("download")
                .to_string()
        });
        Self {
            id,
            name,
            url,
            size: 0,
            downloaded: 0,
            speed: 0.0,
            status: Status::Downloading,
            last_bytes: 0,
            last_at: Instant::now(),
            path: String::new(),
        }
    }
}

enum UiEvent {
    Progress(usize, tgdl_core::ProgressUpdate),
    Meta(usize, u64, usize),
    Done(usize, String),
    Failed(usize, String),
    Queued(QueuedFile),
    ChatReady(i64, i64),
    Notice(String),
}

#[component]
fn ChevronDown() -> Element {
    rsx! {
        svg {
            width: "12",
            height: "12",
            view_box: "0 0 24 24",
            fill: "none",
            path {
                d: CHEVRON_PATH,
                stroke: "currentColor",
                stroke_width: "2",
                stroke_linecap: "round",
            }
        }
    }
}

#[component]
fn UploadFile() -> Element {
    rsx! {
        svg {
            width: "34",
            height: "34",
            view_box: "0 0 490.955 490.955",
            fill: "currentColor",
            path { d: UPLOAD_PATH }
        }
    }
}

#[component]
fn Field(label: String, value: String, oninput: EventHandler<String>) -> Element {
    rsx! {
        div { style: "display:flex;flex-direction:column;gap:4px;",
            label { style: "font-size:12px;color:#6f6f6f;", "{label}" }
            input {
                style: "height:32px;padding:0 9px;border:1px solid #2e2e2e;border-radius:6px;background:#141414;color:#d8d8d8;outline:none;box-sizing:border-box;",
                value: "{value}",
                oninput: move |evt| oninput.call(evt.value()),
            }
        }
    }
}

async fn run_download(config: Config, link: String, out: String, id: usize) -> Result<(), String> {
    let pool = BotPool::from_config(&config)
        .await
        .map_err(|e| format!("sign in: {e}"))?;
    let mut sources: Vec<std::sync::Arc<dyn ChunkSource>> = Vec::new();
    let mut meta: Option<FileMeta> = None;
    let mut filename = String::from("download.bin");
    for bot in &pool.bots {
        let source = TelegramSource::resolve(bot.client.clone(), &link)
            .await
            .map_err(|e| format!("resolve: {e}"))?;
        if meta.is_none() {
            filename = source.file_name.clone();
            meta = Some(FileMeta {
                unique_id: source.meta_unique_id(),
                size: source.size,
                chunk_size: config.chunk_size_bytes().min(512 * 1024),
            });
        }
        sources.push(std::sync::Arc::new(source));
    }
    let meta = meta.ok_or("no sources")?;
    send_event(UiEvent::Meta(id, meta.size, sources.len()));

    let (progress_tx, mut progress_rx) =
        tokio_mpsc::unbounded_channel::<tgdl_core::ProgressUpdate>();
    runtime().spawn(async move {
        while let Some(update) = progress_rx.recv().await {
            send_event(UiEvent::Progress(id, update));
        }
    });
    let options = SchedulerOptions {
        slots_per_bot: config.download.slots_per_bot.max(1),
        max_attempts: config.download.max_attempts_per_chunk.max(1),
        progress_tx: Some(progress_tx),
        ..Default::default()
    };
    let out_dir = if out.trim().is_empty() {
        config.download.output_dir.clone()
    } else {
        out
    };
    tokio::fs::create_dir_all(&out_dir)
        .await
        .map_err(|e| e.to_string())?;
    let part = std::path::Path::new(&out_dir).join(format!("{filename}.part"));
    let plan = DownloadPlan::new(meta.clone());
    let stats = Scheduler::run(sources, &plan, &part, None, options)
        .await
        .map_err(|e| format!("download: {e}"))?;
    verify::verify_size(&part, meta.size).map_err(|e| e.to_string())?;
    let final_path = std::path::Path::new(&out_dir).join(&filename);
    tokio::fs::rename(&part, &final_path)
        .await
        .map_err(|e| e.to_string())?;
    send_event(UiEvent::Done(id, final_path.to_string_lossy().to_string()));
    let _ = stats;
    Ok(())
}

async fn spawn_listener(config: Config) {
    let Some(chat_id) = config.chat.chat_id else {
        return;
    };
    let mut pool = match BotPool::from_config(&config).await {
        Ok(pool) => pool,
        Err(e) => {
            send_event(UiEvent::Notice(format!("listener sign-in: {e}")));
            return;
        }
    };
    let Some(updates) = pool.take_updates(0) else {
        send_event(UiEvent::Notice("listener updates unavailable".into()));
        return;
    };
    let Some(client) = pool.first().map(|bot| bot.client.clone()) else {
        send_event(UiEvent::Notice("no bots configured".into()));
        return;
    };
    if let Err(e) = listener::listen(client, updates, chat_id, event_bundle_sender()).await {
        send_event(UiEvent::Notice(format!("listener: {e}")));
    }
    drop(pool);
}

fn event_bundle_sender() -> tokio_mpsc::UnboundedSender<QueuedFile> {
    let (tx, mut rx) = tokio_mpsc::unbounded_channel::<QueuedFile>();
    runtime().spawn(async move {
        while let Some(file) = rx.recv().await {
            send_event(UiEvent::Queued(file));
        }
    });
    tx
}

async fn spawn_setup_chat(config: Config) {
    let pool = match BotPool::from_config(&config).await {
        Ok(pool) => pool,
        Err(e) => {
            send_event(UiEvent::Notice(format!("setup: {e}")));
            return;
        }
    };
    let Some(client) = pool.first().map(|bot| bot.client.clone()) else {
        send_event(UiEvent::Notice("no bots configured".into()));
        return;
    };
    match ensure_chat(&client, &config).await {
        Ok(chat) => {
            if let Err(e) = add_bots(&client, chat, &config).await {
                send_event(UiEvent::Notice(format!("add bots: {e}")));
                return;
            }
            send_event(UiEvent::ChatReady(chat.id, chat.access_hash));
        }
        Err(e) => send_event(UiEvent::Notice(format!("create chat: {e}"))),
    }
    drop(pool);
}

fn format_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

fn toolbar_button(active: bool) -> &'static str {
    if active {
        "height:32px;padding:0 12px;display:flex;align-items:center;gap:6px;border:1px solid #3a6b8a;border-radius:6px;background:#1d3444;color:#d8d8d8;font-size:12px;cursor:pointer;"
    } else {
        "height:32px;padding:0 12px;display:flex;align-items:center;gap:6px;border:1px solid #2e2e2e;border-radius:6px;background:#1c1c1c;color:#d8d8d8;font-size:12px;cursor:pointer;"
    }
}

fn main() {
    let cfg = dioxus::desktop::Config::default().with_window(
        dioxus::desktop::WindowBuilder::new()
            .with_title("TG Download Manager")
            .with_inner_size(dioxus::desktop::LogicalSize::new(1138.0, 679.0))
            .with_min_inner_size(dioxus::desktop::LogicalSize::new(900.0, 560.0)),
    );
    dioxus::LaunchBuilder::desktop().with_cfg(cfg).launch(App);
}

#[component]
fn App() -> Element {
    let (initial_config, initial_status) = use_hook(|| match Config::load() {
        Ok(cfg) => {
            let status = if cfg.telegram.api_id == 0 {
                "Open Settings to configure".to_string()
            } else {
                "Ready".to_string()
            };
            (cfg, status)
        }
        Err(e) => (Config::default(), format!("Config error: {e}")),
    });
    let mut config = use_signal(|| initial_config);
    let mut status = use_signal(|| initial_status);
    let defaults = {
        let cfg = config.read();
        (
            cfg.telegram.api_id.to_string(),
            cfg.telegram.api_hash.clone(),
            cfg.download.output_dir.clone(),
            cfg.download.chunk_size_kib.to_string(),
            cfg.download.slots_per_bot.to_string(),
            cfg.download.max_attempts_per_chunk.to_string(),
            cfg.chat.title.clone(),
            cfg.bots
                .iter()
                .map(|b| b.username.clone())
                .collect::<Vec<_>>(),
            cfg.bots.iter().map(|b| b.token.clone()).collect::<Vec<_>>(),
        )
    };
    let mut downloads = use_signal(Vec::new);
    let mut show_new = use_signal(|| false);
    let mut show_settings = use_signal(|| false);
    let mut listener_started = use_signal(|| false);
    let mut next_id = use_signal(|| 1usize);
    let mut link = use_signal(String::new);
    let mut api_id = use_signal(|| defaults.0.clone());
    let mut api_hash = use_signal(|| defaults.1.clone());
    let mut output = use_signal(|| defaults.2.clone());
    let mut chunk = use_signal(|| defaults.3.clone());
    let mut slots = use_signal(|| defaults.4.clone());
    let mut attempts = use_signal(|| defaults.5.clone());
    let mut chat_title = use_signal(|| defaults.6.clone());
    let mut bot_names = use_signal(|| defaults.7.clone());
    let mut bot_tokens = use_signal(|| defaults.8.clone());

    let events = use_coroutine(move |mut rx| async move {
        while let Some(event) = rx.next().await {
            match event {
                UiEvent::Progress(id, update) => {
                    let now = Instant::now();
                    let mut rows = downloads.write();
                    if let Some(row) = rows.iter_mut().find(|row: &&mut DownloadRow| row.id == id) {
                        let elapsed = now.duration_since(row.last_at).as_secs_f64().max(0.001);
                        row.speed =
                            update.downloaded.saturating_sub(row.last_bytes) as f64 / elapsed;
                        row.last_bytes = update.downloaded;
                        row.last_at = now;
                        row.downloaded = update.downloaded;
                        row.size = update.total;
                        row.status = Status::Downloading;
                    }
                }
                UiEvent::Meta(id, size, bots) => {
                    if let Some(row) = downloads
                        .write()
                        .iter_mut()
                        .find(|row: &&mut DownloadRow| row.id == id)
                    {
                        row.size = size;
                    }
                    let _ = bots;
                }
                UiEvent::Done(id, path) => {
                    if let Some(row) = downloads
                        .write()
                        .iter_mut()
                        .find(|row: &&mut DownloadRow| row.id == id)
                    {
                        row.status = Status::Completed;
                        row.downloaded = row.size;
                        row.speed = 0.0;
                        row.path = path;
                    }
                    status.set("Download complete".into());
                }
                UiEvent::Failed(id, err) => {
                    if let Some(row) = downloads
                        .write()
                        .iter_mut()
                        .find(|row: &&mut DownloadRow| row.id == id)
                    {
                        row.status = Status::Failed;
                        row.speed = 0.0;
                    }
                    status.set(err);
                }
                UiEvent::Queued(file) => {
                    let id = *next_id.read();
                    next_id.set(id + 1);
                    downloads.write().push(DownloadRow::new(
                        id,
                        file.link.clone(),
                        Some(file.name.clone()),
                    ));
                    status.set(format!("Queued {}", file.name));
                    let cfg = config.read().clone();
                    let out = cfg.download.output_dir.clone();
                    runtime().spawn(async move {
                        if let Err(e) = run_download(cfg, file.link, out, id).await {
                            send_event(UiEvent::Failed(id, e));
                        }
                    });
                }
                UiEvent::ChatReady(id, access_hash) => {
                    let mut cfg = config.read().clone();
                    cfg.chat.chat_id = Some(id);
                    cfg.chat.access_hash = Some(access_hash);
                    match cfg.save() {
                        Ok(()) => {
                            config.set(cfg);
                            status.set(format!("Chat ready: {id}"));
                        }
                        Err(e) => status.set(format!("Chat created but save failed: {e}")),
                    }
                }
                UiEvent::Notice(message) => status.set(message),
            }
        }
    });
    *event_bus().write().unwrap() = Some(events.tx());

    use_effect(move || {
        if *listener_started.read() {
            return;
        }
        let cfg = config.read();
        if cfg.chat.chat_id.is_some() && cfg.chat.listen {
            listener_started.set(true);
            runtime().spawn(spawn_listener(cfg.clone()));
        }
    });

    let launch_download = move |id: usize, download_link: String, out: String| {
        let cfg = config.read().clone();
        runtime().spawn(async move {
            if let Err(e) = run_download(cfg, download_link, out, id).await {
                send_event(UiEvent::Failed(id, e));
            }
        });
    };

    let mut start_download = move || {
        let value = link.read().clone();
        if value.trim().is_empty() {
            status.set("Enter a Telegram link".into());
            return;
        }
        let id = *next_id.read();
        next_id.set(id + 1);
        downloads
            .write()
            .push(DownloadRow::new(id, value.clone(), None));
        show_new.set(false);
        status.set("Resolving file...".into());
        launch_download(id, value, output.read().clone());
    };

    let mut save_settings = move || {
        let mut cfg = config.read().clone();
        cfg.telegram.api_id = api_id.read().parse().unwrap_or(0);
        cfg.telegram.api_hash = api_hash.read().clone();
        cfg.download.output_dir = output.read().clone();
        cfg.download.chunk_size_kib = chunk.read().parse().unwrap_or(512);
        cfg.download.slots_per_bot = slots.read().parse().unwrap_or(3);
        cfg.download.max_attempts_per_chunk = attempts.read().parse().unwrap_or(6);
        cfg.chat.title = chat_title.read().clone();
        let names = bot_names.read().clone();
        let tokens = bot_tokens.read().clone();
        for (i, bot) in cfg.bots.iter_mut().enumerate() {
            bot.username = names.get(i).cloned().unwrap_or_default();
            bot.token = tokens.get(i).cloned().unwrap_or_default();
        }
        match cfg.save() {
            Ok(()) => {
                config.set(cfg);
                show_settings.set(false);
                status.set("Settings saved to config.tgdm".into());
            }
            Err(e) => status.set(format!("Could not save settings: {e}")),
        }
    };

    let mut setup_chat = move || {
        save_settings();
        let cfg = config.read().clone();
        status.set("Creating chat and adding bots...".into());
        runtime().spawn(spawn_setup_chat(cfg));
    };

    let row_elements = downloads
        .read()
        .iter()
        .map(|row| {
            let id = row.id;
            let name = row.name.clone();
            let url = row.url.clone();
            let label = match row.status {
                Status::Downloading => "Downloading",
                Status::Completed => "Completed",
                Status::Failed => "Failed",
            };
            let color = match row.status {
                Status::Completed => "#4ade80",
                Status::Failed => "#f87171",
                Status::Downloading => "#58c4f6",
            };
            let pct = if row.size == 0 {
                0.0
            } else {
                row.downloaded as f32 / row.size as f32 * 100.0
            };
            rsx! {
                div {
                    key: "{id}",
                    style: "padding:12px;border:1px solid #2e2e2e;border-radius:8px;background:#1c1c1c;display:flex;flex-direction:column;gap:8px;",
                    div { style: "display:flex;gap:8px;align-items:center;",
                        div { style: "font-size:14px;font-weight:600;", "{name}" }
                        div { style: "flex:1;" }
                        div { style: "font-size:12px;color:{color};", "{label}" }
                    }
                    div { style: "font-size:12px;color:#6f6f6f;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;", "{url}" }
                    div { style: "height:6px;border-radius:4px;background:#2a2a2a;overflow:hidden;",
                        div { style: "height:100%;width:{pct}%;background:{color};" }
                    }
                    div { style: "display:flex;gap:16px;font-size:12px;color:#6f6f6f;",
                        span { "{format_bytes(row.downloaded)} / {format_bytes(row.size)}" }
                        span { "{row.speed / 1024.0 / 1024.0:.1} MiB/s" }
                        div { style: "flex:1;" }
                        button {
                            style: "border:0;background:transparent;color:#6f6f6f;font-size:12px;cursor:pointer;",
                            onclick: move |_| downloads.write().retain(|item| item.id != id),
                            "Remove"
                        }
                    }
                }
            }
        })
        .collect::<Vec<_>>();

    let name_values = bot_names.read().clone();
    let token_values = bot_tokens.read().clone();
    let mut settings_fields = vec![
        rsx! { Field { label: "API ID", value: api_id.read().clone(), oninput: move |v| api_id.set(v) } },
        rsx! { Field { label: "API hash", value: api_hash.read().clone(), oninput: move |v| api_hash.set(v) } },
        rsx! { Field { label: "Output directory", value: output.read().clone(), oninput: move |v| output.set(v) } },
        rsx! { Field { label: "Chunk size (KiB)", value: chunk.read().clone(), oninput: move |v| chunk.set(v) } },
        rsx! { Field { label: "Slots per bot", value: slots.read().clone(), oninput: move |v| slots.set(v) } },
        rsx! { Field { label: "Max attempts per chunk", value: attempts.read().clone(), oninput: move |v| attempts.set(v) } },
        rsx! { Field { label: "Chat title", value: chat_title.read().clone(), oninput: move |v| chat_title.set(v) } },
    ];
    for i in 0..name_values.len() {
        let idx = i;
        settings_fields.push(rsx! {
            Field {
                label: format!("Bot {} username", i + 1),
                value: name_values.get(i).cloned().unwrap_or_default(),
                oninput: move |v| {
                    if let Some(slot) = bot_names.write().get_mut(idx) {
                        *slot = v;
                    }
                }
            }
        });
        settings_fields.push(rsx! {
            Field {
                label: format!("Bot {} token", i + 1),
                value: token_values.get(i).cloned().unwrap_or_default(),
                oninput: move |v| {
                    if let Some(slot) = bot_tokens.write().get_mut(idx) {
                        *slot = v;
                    }
                }
            }
        });
    }

    let mut all_rows = row_elements;
    if all_rows.is_empty() {
        all_rows.push(rsx! {
            div {
                style: "flex:1;display:flex;align-items:center;justify-content:center;gap:10px;color:#6f6f6f;font-size:14px;",
                UploadFile {}
                span { "Drag-and-drop URL or torrent file" }
            }
        });
    }

    rsx! {
        div { style: "width:100vw;height:100vh;display:flex;flex-direction:column;background:#151515;box-sizing:border-box;overflow:hidden;",
            style { "body {{ margin:0; background:#151515; color:#d8d8d8; font-family:'Segoe UI',sans-serif; }}" }
            div { style: "height:36px;flex-shrink:0;display:flex;align-items:center;gap:8px;padding:0 12px;background:#181818;",
                div { style: "width:14px;height:14px;border-radius:3px;background:#58c4f6;" }
                div { style: "font-size:12px;color:#e6e6e6;", "TG Download Manager" }
            }
            div { style: "height:44px;flex-shrink:0;display:flex;align-items:center;gap:8px;padding:0 12px;",
                button { style: toolbar_button(false), onclick: move |_| {}, ChevronDown {} }
                button { style: toolbar_button(false), onclick: move |_| {}, "Newest", ChevronDown {} }
                button { style: toolbar_button(false), onclick: move |_| {}, "All files", ChevronDown {} }
                div { style: "flex:1;" }
                button {
                    style: toolbar_button(false),
                    onclick: move |_| show_settings.set(true),
                    "Settings"
                }
                button {
                    style: "height:32px;padding:0 16px;border:0;border-radius:6px;background:#58c4f6;color:#001018;font-size:12px;font-weight:600;cursor:pointer;",
                    onclick: move |_| show_new.set(true),
                    "+ Add download"
                }
                input {
                    style: "width:230px;height:32px;padding:0 10px;border:1px solid #2e2e2e;border-radius:6px;background:#1c1c1c;color:#d8d8d8;box-sizing:border-box;outline:none;",
                    placeholder: "Search in downloads...",
                }
                button { style: toolbar_button(false), onclick: move |_| {}, "=" }
            }
            div { style: "flex:1;min-height:0;margin:0 12px 4px 12px;border:1px dashed #4a4a4a;border-radius:8px;padding:8px;display:flex;flex-direction:column;gap:8px;overflow:auto;",
                {all_rows.into_iter()}
            }
            div { style: "height:36px;flex-shrink:0;display:flex;align-items:center;gap:8px;padding:0 12px;background:#1a1a1a;border-top:1px solid #2e2e2e;font-size:12px;color:#6f6f6f;",
                span { style: "padding:2px 8px;border:1px solid #3a3a3a;border-radius:4px;background:#222222;color:#6f6f6f;", "OFF" }
                span { style: "padding:2px 8px;border:1px solid #3a3a3a;border-radius:4px;background:#222222;color:#6f6f6f;", "OFF" }
                span { "▼ 0 B/s   ▲ 0 B/s" }
                span { style: "padding:2px 8px;border:1px solid #3a3a3a;border-radius:4px;background:#222222;color:#4ade80;", "HIGH" }
                span { style: "color:#6f6f6f;", "{status}" }
                div { style: "flex:1;" }
                span { style: "padding:3px 10px;border:1px solid #2f6b4a;border-radius:5px;background:#183024;color:#4ade80;", "o Support the project" }
            }
            if show_new() {
                div { style: "position:absolute;inset:0;background:rgba(0,0,0,0.45);display:flex;align-items:center;justify-content:center;",
                    div { style: "width:520px;padding:20px;border:1px solid #2e2e2e;border-radius:10px;background:#1c1c1c;display:flex;flex-direction:column;gap:16px;box-shadow:0 12px 40px rgba(0,0,0,0.5);",
                        div { style: "font-size:18px;font-weight:600;", "New download" }
                        Field { label: "Telegram message link", value: link.read().clone(), oninput: move |v| link.set(v) }
                        Field { label: "Output directory", value: output.read().clone(), oninput: move |v| output.set(v) }
                        div { style: "display:flex;justify-content:flex-end;gap:8px;",
                            button {
                                style: toolbar_button(false),
                                onclick: move |_| show_new.set(false),
                                "Cancel"
                            }
                            button {
                                style: "height:32px;padding:0 16px;border:0;border-radius:6px;background:#58c4f6;color:#001018;font-size:12px;font-weight:600;cursor:pointer;",
                                onclick: move |_| start_download(),
                                "Start"
                            }
                        }
                    }
                }
            }
            if show_settings() {
                div { style: "position:absolute;inset:0;background:rgba(0,0,0,0.45);display:flex;align-items:center;justify-content:center;",
                    div { style: "width:600px;max-height:600px;padding:20px;border:1px solid #2e2e2e;border-radius:10px;background:#1c1c1c;display:flex;flex-direction:column;gap:12px;box-shadow:0 12px 40px rgba(0,0,0,0.5);",
                        div { style: "font-size:18px;font-weight:600;", "Settings" }
                        div { style: "display:flex;flex-direction:column;gap:10px;overflow:auto;max-height:450px;padding-right:4px;",
                            {settings_fields.into_iter()}
                        }
                        div { style: "display:flex;justify-content:flex-end;gap:8px;",
                            button { style: toolbar_button(false), onclick: move |_| show_settings.set(false), "Cancel" }
                            button { style: toolbar_button(false), onclick: move |_| setup_chat(), "Create/Connect chat" }
                            button {
                                style: "height:32px;padding:0 16px;border:0;border-radius:6px;background:#58c4f6;color:#001018;font-size:12px;font-weight:600;cursor:pointer;",
                                onclick: move |_| save_settings(),
                                "Save"
                            }
                        }
                    }
                }
            }
        }
    }
}
