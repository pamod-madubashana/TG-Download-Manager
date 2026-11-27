use gpui_kit::base::StyledExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, TitleBar, h_flex, v_flex};
use gpui_kit::{
    App, Bounds, Context, Entity, Render, SharedString, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, rgb, size,
};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Instant;
use tgdl_core::bots::{QueuedFile, add_bots, ensure_chat, listener};
use tgdl_core::source::ChunkSource;
use tgdl_core::source::telegram::TelegramSource;
use tgdl_core::{
    BotPool, Config, DownloadPlan, FileMeta, ProgressUpdate, Scheduler, SchedulerOptions, verify,
};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

const CHEVRON_DOWN_SVG: &[u8] = include_bytes!("../assets/icons/chevron-down.svg");
const UPLOAD_FILE_SVG: &[u8] = include_bytes!("../assets/icons/upload-file.svg");

static RUNTIME: OnceLock<tokio::runtime::Handle> = OnceLock::new();

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Downloading,
    Completed,
    Failed,
}

struct DownloadRow {
    id: usize,
    name: SharedString,
    url: SharedString,
    size: u64,
    downloaded: u64,
    speed: f64,
    status: Status,
    per_source: Vec<u64>,
    last_bytes: u64,
    last_at: Instant,
    path: SharedString,
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
            name: name.into(),
            url: url.into(),
            size: 0,
            downloaded: 0,
            speed: 0.0,
            status: Status::Downloading,
            per_source: Vec::new(),
            last_bytes: 0,
            last_at: Instant::now(),
            path: "".into(),
        }
    }

    fn progress(&self) -> f32 {
        if self.size == 0 {
            0.0
        } else {
            (self.downloaded as f32 / self.size as f32) * 100.0
        }
    }
}

enum UiEvent {
    Progress(usize, ProgressUpdate),
    Meta(usize, u64, Vec<String>),
    Done(usize, String),
    Failed(usize, String),
    Queued(QueuedFile),
    ChatReady(i64, i64),
    Notice(String),
}

struct AppView {
    link_input: Entity<InputState>,
    output_input: Entity<InputState>,
    search_input: Entity<InputState>,
    api_id_input: Entity<InputState>,
    api_hash_input: Entity<InputState>,
    chunk_input: Entity<InputState>,
    slots_input: Entity<InputState>,
    attempts_input: Entity<InputState>,
    chat_title_input: Entity<InputState>,
    bot_username_inputs: Vec<Entity<InputState>>,
    bot_token_inputs: Vec<Entity<InputState>>,
    downloads: Vec<DownloadRow>,
    show_new: bool,
    show_settings: bool,
    listener_started: bool,
    status: SharedString,
    config: Arc<Config>,
    next_id: usize,
    event_tx: UnboundedSender<UiEvent>,
}

impl AppView {
    fn new(window: &mut Window, cx: &mut App) -> (Self, UnboundedReceiver<UiEvent>) {
        let (config, initial_status) = match Config::load() {
            Ok(config) => {
                let status = if config.telegram.api_id == 0 {
                    "Open Settings to configure"
                } else {
                    "Ready"
                };
                (config, status.to_string())
            }
            Err(e) => (Config::default(), format!("Config error: {e}")),
        };
        let config = Arc::new(config);
        let link_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://t.me/channel/123"));
        let output_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("downloads")
                .default_value(config.download.output_dir.clone())
        });
        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search in downloads..."));
        let api_id_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("API ID")
                .default_value(config.telegram.api_id.to_string())
        });
        let api_hash_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("API hash")
                .default_value(config.telegram.api_hash.clone())
        });
        let chunk_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("512")
                .default_value(config.download.chunk_size_kib.to_string())
        });
        let slots_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("3")
                .default_value(config.download.slots_per_bot.to_string())
        });
        let attempts_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("6")
                .default_value(config.download.max_attempts_per_chunk.to_string())
        });
        let chat_title_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("TG Downloads")
                .default_value(config.chat.title.clone())
        });
        let bot_username_inputs = config
            .bots
            .iter()
            .map(|bot| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("@bot_username")
                        .default_value(bot.username.clone())
                })
            })
            .collect();
        let bot_token_inputs = config
            .bots
            .iter()
            .map(|bot| {
                cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder("Bot token")
                        .default_value(bot.token.clone())
                })
            })
            .collect();
        let (event_tx, rx) = mpsc::unbounded_channel::<UiEvent>();
        let view = Self {
            link_input,
            output_input,
            search_input,
            api_id_input,
            api_hash_input,
            chunk_input,
            slots_input,
            attempts_input,
            chat_title_input,
            bot_username_inputs,
            bot_token_inputs,
            downloads: Vec::new(),
            show_new: false,
            show_settings: false,
            listener_started: false,
            status: initial_status.into(),
            config,
            next_id: 1,
            event_tx,
        };
        (view, rx)
    }

    fn apply_progress(cx: &mut App, id: usize, update: ProgressUpdate) {
        AppViews::update(cx, |view, cx| {
            if let Some(row) = view.downloads.iter_mut().find(|r| r.id == id) {
                let now = Instant::now();
                let elapsed = now.duration_since(row.last_at).as_secs_f64().max(0.001);
                let delta = update.downloaded.saturating_sub(row.last_bytes);
                row.speed = delta as f64 / elapsed;
                row.last_bytes = update.downloaded;
                row.last_at = now;
                row.downloaded = update.downloaded;
                row.size = update.total;
                row.per_source = update.per_source;
                row.status = Status::Downloading;
            }
            cx.notify();
        });
    }

    fn apply_meta(cx: &mut App, id: usize, size: u64, bots: Vec<String>) {
        AppViews::update(cx, |view, cx| {
            if let Some(row) = view.downloads.iter_mut().find(|r| r.id == id) {
                row.size = size;
                row.per_source = vec![0; bots.len()];
            }
            cx.notify();
        });
    }

    fn apply_done(cx: &mut App, id: usize, path: String) {
        AppViews::update(cx, |view, cx| {
            if let Some(row) = view.downloads.iter_mut().find(|r| r.id == id) {
                row.status = Status::Completed;
                row.downloaded = row.size;
                row.speed = 0.0;
                row.path = path.into();
            }
            view.status = "Download complete".into();
            cx.notify();
        });
    }

    fn apply_failed(cx: &mut App, id: usize, err: String) {
        AppViews::update(cx, |view, cx| {
            if let Some(row) = view.downloads.iter_mut().find(|r| r.id == id) {
                row.status = Status::Failed;
                row.speed = 0.0;
            }
            view.status = err.into();
            cx.notify();
        });
    }

    fn apply_queued(cx: &mut App, file: QueuedFile) {
        AppViews::update(cx, |view, cx| {
            let out = view.config.download.output_dir.clone();
            let id = view.enqueue(file.link.clone(), Some(file.name.clone()));
            view.status = format!("Queued {}", file.name).into();
            view.launch_download(id, file.link, out, cx);
            cx.notify();
        });
    }

    fn apply_chat_ready(cx: &mut App, id: i64, access_hash: i64) {
        AppViews::update(cx, |view, cx| {
            let mut config = (*view.config).clone();
            config.chat.chat_id = Some(id);
            config.chat.access_hash = Some(access_hash);
            if let Err(e) = config.save() {
                view.status = format!("Chat created but save failed: {e}").into();
            } else {
                view.config = Arc::new(config);
                view.status = format!("Chat ready: {id}").into();
            }
            view.spawn_listener();
            cx.notify();
        });
    }

    fn apply_notice(cx: &mut App, message: String) {
        AppViews::update(cx, |view, cx| {
            view.status = message.into();
            cx.notify();
        });
    }

    fn enqueue(&mut self, link: String, name: Option<String>) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.downloads.push(DownloadRow::new(id, link, name));
        id
    }

    fn launch_download(&mut self, id: usize, link: String, out: String, cx: &mut Context<Self>) {
        if link.trim().is_empty() {
            self.status = "Enter a Telegram link".into();
            cx.notify();
            return;
        }
        self.show_new = false;
        self.status = "Resolving file...".into();
        let cfg = (*self.config).clone();
        let tx = self.event_tx.clone();
        runtime().spawn(async move {
            match run_download(cfg, link, out, id, tx.clone()).await {
                Ok(path) => {
                    let _ = tx.send(UiEvent::Done(id, path));
                }
                Err(e) => {
                    let _ = tx.send(UiEvent::Failed(id, e));
                }
            }
        });
        cx.notify();
    }

    fn start_download(&mut self, cx: &mut Context<Self>) {
        let link = self.link_input.read(cx).value().to_string();
        let out = self.output_input.read(cx).value().to_string();
        let id = self.enqueue(link.clone(), None);
        self.launch_download(id, link, out, cx);
    }

    fn spawn_listener(&mut self) {
        if self.listener_started {
            return;
        }
        let Some(chat_id) = self.config.chat.chat_id else {
            return;
        };
        if !self.config.chat.listen {
            return;
        }
        self.listener_started = true;
        let cfg = (*self.config).clone();
        let tx = self.event_tx.clone();
        runtime().spawn(async move {
            let mut pool = match BotPool::from_config(&cfg).await {
                Ok(pool) => pool,
                Err(e) => {
                    let _ = tx.send(UiEvent::Notice(format!("listener sign-in: {e}")));
                    return;
                }
            };
            let Some(updates) = pool.take_updates(0) else {
                let _ = tx.send(UiEvent::Notice("listener updates unavailable".into()));
                return;
            };
            let Some(client) = pool.first().map(|bot| bot.client.clone()) else {
                let _ = tx.send(UiEvent::Notice("no bots configured".into()));
                return;
            };
            let (file_tx, mut file_rx) = mpsc::unbounded_channel::<QueuedFile>();
            let forward = tx.clone();
            tokio::spawn(async move {
                while let Some(file) = file_rx.recv().await {
                    let _ = forward.send(UiEvent::Queued(file));
                }
            });
            if let Err(e) = listener::listen(client, updates, chat_id, file_tx).await {
                let _ = tx.send(UiEvent::Notice(format!("listener: {e}")));
            }
            drop(pool);
        });
    }

    fn save_settings(&mut self, cx: &mut Context<Self>) {
        let mut config = (*self.config).clone();
        config.telegram.api_id = self.api_id_input.read(cx).value().parse().unwrap_or(0);
        config.telegram.api_hash = self.api_hash_input.read(cx).value().to_string();
        config.download.output_dir = self.output_input.read(cx).value().to_string();
        config.download.chunk_size_kib = self.chunk_input.read(cx).value().parse().unwrap_or(512);
        config.download.slots_per_bot = self.slots_input.read(cx).value().parse().unwrap_or(3);
        config.download.max_attempts_per_chunk =
            self.attempts_input.read(cx).value().parse().unwrap_or(6);
        config.chat.title = self.chat_title_input.read(cx).value().to_string();
        for (i, bot) in config.bots.iter_mut().enumerate() {
            if let Some(input) = self.bot_username_inputs.get(i) {
                bot.username = input.read(cx).value().to_string();
            }
            if let Some(input) = self.bot_token_inputs.get(i) {
                bot.token = input.read(cx).value().to_string();
            }
        }
        match config.save() {
            Ok(()) => {
                self.config = Arc::new(config);
                self.show_settings = false;
                self.status = "Settings saved to config.tgdm".into();
                self.spawn_listener();
            }
            Err(e) => self.status = format!("Could not save settings: {e}").into(),
        }
        cx.notify();
    }

    fn setup_chat(&mut self, cx: &mut Context<Self>) {
        self.save_settings(cx);
        let config = (*self.config).clone();
        let tx = self.event_tx.clone();
        self.status = "Creating chat and adding bots...".into();
        runtime().spawn(async move {
            let pool = match BotPool::from_config(&config).await {
                Ok(pool) => pool,
                Err(e) => {
                    let _ = tx.send(UiEvent::Notice(format!("setup: {e}")));
                    return;
                }
            };
            let Some(client) = pool.first().map(|bot| bot.client.clone()) else {
                let _ = tx.send(UiEvent::Notice("no bots configured".into()));
                return;
            };
            match ensure_chat(&client, &config).await {
                Ok(chat) => {
                    if let Err(e) = add_bots(&client, chat, &config).await {
                        let _ = tx.send(UiEvent::Notice(format!("add bots: {e}")));
                        return;
                    }
                    let _ = tx.send(UiEvent::ChatReady(chat.id, chat.access_hash));
                }
                Err(e) => {
                    let _ = tx.send(UiEvent::Notice(format!("create chat: {e}")));
                }
            }
            drop(pool);
        });
        cx.notify();
    }

    fn remove_selected(&mut self, id: usize, cx: &mut Context<Self>) {
        self.downloads.retain(|r| r.id != id);
        cx.notify();
    }
}

struct AppViews(Entity<AppView>);

impl gpui_kit::Global for AppViews {}

impl AppViews {
    fn update(cx: &mut App, f: impl FnOnce(&mut AppView, &mut Context<AppView>)) {
        let entity = cx.global::<AppViews>().0.clone();
        entity.update(cx, f);
    }
}

async fn run_download(
    config: Config,
    link: String,
    out: String,
    id: usize,
    tx: UnboundedSender<UiEvent>,
) -> Result<String, String> {
    let pool = BotPool::from_config(&config)
        .await
        .map_err(|e| format!("sign in: {e}"))?;
    let mut sources: Vec<Arc<dyn ChunkSource>> = Vec::new();
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
        sources.push(Arc::new(source));
    }
    let meta = meta.ok_or("no sources")?;
    let _ = tx.send(UiEvent::Meta(
        id,
        meta.size,
        (0..sources.len())
            .map(|i| format!("bot {}", i + 1))
            .collect(),
    ));

    let (ptx, mut prx) = mpsc::unbounded_channel::<ProgressUpdate>();
    let forward = tx.clone();
    tokio::spawn(async move {
        while let Some(update) = prx.recv().await {
            let _ = forward.send(UiEvent::Progress(id, update));
        }
    });

    let out_dir = if out.is_empty() {
        config.download.output_dir.clone()
    } else {
        out
    };
    tokio::fs::create_dir_all(&out_dir)
        .await
        .map_err(|e| e.to_string())?;
    let part = std::path::Path::new(&out_dir).join(format!("{filename}.part"));
    let plan = DownloadPlan::new(meta.clone());
    let options = SchedulerOptions {
        slots_per_bot: config.download.slots_per_bot.max(1),
        max_attempts: config.download.max_attempts_per_chunk.max(1),
        progress_tx: Some(ptx),
        ..Default::default()
    };
    Scheduler::run(sources, &plan, &part, None, options)
        .await
        .map_err(|e| format!("download: {e}"))?;
    verify::verify_size(&part, meta.size).map_err(|e| e.to_string())?;
    let final_path = std::path::Path::new(&out_dir).join(&filename);
    tokio::fs::rename(&part, &final_path)
        .await
        .map_err(|e| e.to_string())?;
    Ok(final_path.to_string_lossy().to_string())
}

impl Render for AppView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bg = rgb(0x151515);
        let panel = rgb(0x1c1c1c);
        let border = rgb(0x2e2e2e);
        let muted = rgb(0x6f6f6f);
        let text = rgb(0xd8d8d8);
        let accent = rgb(0x58c4f6);
        let green = rgb(0x4ade80);

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(bg)
            .text_color(text)
            .child(
                TitleBar::new()
                    .h(px(36.))
                    .bg(rgb(0x181818))
                    .px_3()
                    .gap_2()
                    .child(div().size(px(14.)).rounded(px(3.)).bg(accent).child(""))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(0xe6e6e6))
                            .child("TG Download Manager"),
                    ),
            )
            .child(
                h_flex()
                    .h(px(44.))
                    .flex_shrink_0()
                    .px_3()
                    .gap_2()
                    .bg(bg)
                    .child(
                        div()
                            .id("select")
                            .h(px(32.))
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_1()
                            .rounded(px(6.))
                            .bg(panel)
                            .border_1()
                            .border_color(border)
                            .text_xs()
                            .child(Icon::default().data(CHEVRON_DOWN_SVG).xsmall())
                            .on_click(cx.listener(|_this, _, _window, cx| cx.notify())),
                    )
                    .child(
                        div()
                            .id("newest")
                            .h(px(32.))
                            .px_3()
                            .flex()
                            .items_center()
                            .gap_1()
                            .rounded(px(6.))
                            .bg(panel)
                            .border_1()
                            .border_color(border)
                            .text_xs()
                            .child("Newest")
                            .child(Icon::default().data(CHEVRON_DOWN_SVG).xsmall())
                            .on_click(cx.listener(|_this, _, _window, cx| cx.notify())),
                    )
                    .child(
                        div()
                            .id("allfiles")
                            .h(px(32.))
                            .px_3()
                            .flex()
                            .items_center()
                            .gap_1()
                            .rounded(px(6.))
                            .bg(panel)
                            .border_1()
                            .border_color(border)
                            .text_xs()
                            .child("All files")
                            .child(Icon::default().data(CHEVRON_DOWN_SVG).xsmall())
                            .on_click(cx.listener(|_this, _, _window, cx| cx.notify())),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("settings")
                            .h(px(32.))
                            .px_3()
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .bg(panel)
                            .border_1()
                            .border_color(border)
                            .text_xs()
                            .child("Settings")
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.show_settings = true;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("add")
                            .h(px(32.))
                            .px_4()
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .bg(accent)
                            .text_color(rgb(0x001018))
                            .text_xs()
                            .font_medium()
                            .child("+ Add download")
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.show_new = true;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .w(px(230.))
                            .h(px(32.))
                            .px_3()
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .bg(panel)
                            .border_1()
                            .border_color(border)
                            .child(Input::new(&self.search_input).appearance(false).text_sm()),
                    )
                    .child(
                        div()
                            .id("filter")
                            .h(px(32.))
                            .w(px(32.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .bg(panel)
                            .border_1()
                            .border_color(border)
                            .text_xs()
                            .child("=")
                            .on_click(cx.listener(|_this, _, _window, cx| cx.notify())),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .px_3()
                    .pb_1()
                    .overflow_y_scrollbar()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .size_full()
                            .rounded(px(8.))
                            .border_1()
                            .border_dashed()
                            .border_color(rgb(0x4a4a4a))
                            .when(self.downloads.is_empty(), |this| {
                                this.flex().items_center().justify_center().child(
                                    h_flex()
                                        .gap_2()
                                        .text_sm()
                                        .text_color(muted)
                                        .child(Icon::default().data(UPLOAD_FILE_SVG).size(px(26.)))
                                        .child("Drag-and-drop URL or torrent file"),
                                )
                            })
                            .when(!self.downloads.is_empty(), |this| {
                                this.p_2().gap_2().children(
                                    self.downloads.iter().map(|row| self.render_row(row, cx)),
                                )
                            }),
                    ),
            )
            .child(
                h_flex()
                    .h(px(36.))
                    .flex_shrink_0()
                    .px_3()
                    .gap_2()
                    .bg(rgb(0x1a1a1a))
                    .border_t_1()
                    .border_color(border)
                    .child(self.badge("OFF", muted))
                    .child(self.badge("OFF", muted))
                    .child(div().text_xs().text_color(muted).child("▼ 0 B/s   ▲ 0 B/s"))
                    .child(self.badge("HIGH", green))
                    .child(div().text_xs().text_color(muted).child(self.status.clone()))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("support")
                            .h(px(24.))
                            .px_3()
                            .flex()
                            .items_center()
                            .gap_1()
                            .rounded(px(5.))
                            .border_1()
                            .border_color(rgb(0x2f6b4a))
                            .bg(rgb(0x183024))
                            .text_xs()
                            .text_color(green)
                            .child("o Support the project"),
                    ),
            )
            .when(self.show_new, |this| this.child(self.render_new_dialog(cx)))
            .when(self.show_settings, |this| {
                this.child(self.render_settings(cx))
            })
    }
}
impl AppView {
    fn badge(&self, label: &str, color: gpui_kit::Rgba) -> impl IntoElement {
        div()
            .h(px(20.))
            .px_2()
            .flex()
            .items_center()
            .rounded(px(4.))
            .border_1()
            .border_color(rgb(0x3a3a3a))
            .bg(rgb(0x222222))
            .text_xs()
            .text_color(color)
            .child(label.to_string())
    }

    fn render_row(&self, row: &DownloadRow, cx: &Context<Self>) -> impl IntoElement {
        let pct = row.progress();
        let status_text = match row.status {
            Status::Downloading => "Downloading",
            Status::Completed => "Completed",
            Status::Failed => "Failed",
        };
        let color = match row.status {
            Status::Completed => cx.theme().success,
            Status::Failed => cx.theme().danger,
            Status::Downloading => cx.theme().primary,
        };
        let row_id = row.id;
        let remove = row.id;
        v_flex()
            .p_3()
            .gap_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_sm().font_semibold().child(row.name.clone()))
                    .child(div().flex_1())
                    .child(div().text_xs().text_color(color).child(status_text)),
            )
            .child(
                div()
                    .text_xs()
                    .truncate()
                    .text_color(cx.theme().muted_foreground)
                    .child(row.url.clone()),
            )
            .child(Progress::new(("progress", row_id)).value(pct).color(color))
            .child(
                h_flex()
                    .gap_4()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "{} / {}",
                        format_bytes(row.downloaded),
                        format_bytes(row.size)
                    ))
                    .child(format!("{:.1} MiB/s", row.speed / 1024.0 / 1024.0))
                    .child(div().flex_1())
                    .child(
                        Button::new(("remove", remove))
                            .ghost()
                            .xsmall()
                            .label("Remove")
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.remove_selected(remove, cx);
                            })),
                    ),
            )
    }

    fn render_new_dialog(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .absolute()
            .inset_0()
            .bg(gpui_kit::black().opacity(0.45))
            .flex()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .w(px(520.))
                    .p_5()
                    .gap_4()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .shadow_lg()
                    .child(div().text_lg().font_semibold().child("New download"))
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Telegram message link"),
                            )
                            .child(Input::new(&self.link_input)),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Output directory"),
                            )
                            .child(Input::new(&self.output_input)),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .justify_end()
                            .child(Button::new("cancel").label("Cancel").on_click(cx.listener(
                                |this, _, _window, cx| {
                                    this.show_new = false;
                                    cx.notify();
                                },
                            )))
                            .child(Button::new("start").primary().label("Start").on_click(
                                cx.listener(|this, _, _window, cx| {
                                    this.start_download(cx);
                                }),
                            )),
                    ),
            )
    }

    fn render_settings(&self, cx: &Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let field = |label: &'static str, input: Entity<InputState>| {
            v_flex()
                .gap_1()
                .child(div().text_xs().text_color(muted).child(label))
                .child(Input::new(&input))
        };
        div()
            .absolute()
            .inset_0()
            .bg(gpui_kit::black().opacity(0.45))
            .flex()
            .items_center()
            .justify_center()
            .child(
                v_flex()
                    .w(px(600.))
                    .max_h(px(600.))
                    .p_5()
                    .gap_3()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().background)
                    .shadow_lg()
                    .child(div().text_lg().font_semibold().child("Settings"))
                    .child(
                        v_flex()
                            .gap_2()
                            .p_1()
                            .overflow_y_scrollbar()
                            .child(field("API ID", self.api_id_input.clone()))
                            .child(field("API hash", self.api_hash_input.clone()))
                            .child(field("Output directory", self.output_input.clone()))
                            .child(field("Chunk size (KiB)", self.chunk_input.clone()))
                            .child(field("Slots per bot", self.slots_input.clone()))
                            .child(field("Max attempts per chunk", self.attempts_input.clone()))
                            .child(field("Chat title", self.chat_title_input.clone()))
                            .children(self.bot_username_inputs.iter().enumerate().map(
                                |(i, input)| {
                                    field(
                                        if i == 0 {
                                            "Bot 1 username"
                                        } else if i == 1 {
                                            "Bot 2 username"
                                        } else {
                                            "Bot 3 username"
                                        },
                                        input.clone(),
                                    )
                                },
                            ))
                            .children(self.bot_token_inputs.iter().enumerate().map(
                                |(i, input)| {
                                    field(
                                        if i == 0 {
                                            "Bot 1 token"
                                        } else if i == 1 {
                                            "Bot 2 token"
                                        } else {
                                            "Bot 3 token"
                                        },
                                        input.clone(),
                                    )
                                },
                            )),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .justify_end()
                            .child(Button::new("settings-cancel").label("Cancel").on_click(
                                cx.listener(|this, _, _window, cx| {
                                    this.show_settings = false;
                                    cx.notify();
                                }),
                            ))
                            .child(
                                Button::new("settings-create")
                                    .label("Create/Connect chat")
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.setup_chat(cx);
                                    })),
                            )
                            .child(
                                Button::new("settings-save")
                                    .primary()
                                    .label("Save")
                                    .on_click(cx.listener(|this, _, _window, cx| {
                                        this.save_settings(cx);
                                    })),
                            ),
                    ),
            )
    }
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

fn main() {
    gpui_kit::application().run(|cx: &mut App| {
        gpui_kit::init(cx);
        gpui_kit::component::theme::Theme::change(
            gpui_kit::component::theme::ThemeMode::Dark,
            None,
            cx,
        );
        cx.activate(true);
        let bounds = Bounds::centered(None, size(px(1138.0), px(679.0)), cx);
        let mut rx_slot = None;
        let (_, view) = gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(900.0), px(560.0))),
                ..TitleBar::window_options()
            },
            cx,
            |window, cx| {
                let (view, rx) = AppView::new(window, cx);
                rx_slot = Some(rx);
                cx.new(|_| view)
            },
        )
        .expect("open window");
        cx.set_global(AppViews(view));
        let rx = rx_slot.expect("event receiver");
        cx.spawn(async move |cx| {
            let mut rx = rx;
            while let Some(event) = rx.recv().await {
                cx.update(|app| match event {
                    UiEvent::Progress(id, update) => AppView::apply_progress(app, id, update),
                    UiEvent::Meta(id, size, bots) => AppView::apply_meta(app, id, size, bots),
                    UiEvent::Done(id, path) => AppView::apply_done(app, id, path),
                    UiEvent::Failed(id, err) => AppView::apply_failed(app, id, err),
                    UiEvent::Queued(file) => AppView::apply_queued(app, file),
                    UiEvent::ChatReady(id, access_hash) => {
                        AppView::apply_chat_ready(app, id, access_hash)
                    }
                    UiEvent::Notice(message) => AppView::apply_notice(app, message),
                });
            }
        })
        .detach();
    });
}
