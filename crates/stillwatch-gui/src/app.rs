//! iced daemon: window open/close on top of [`crate::model::update`].

use std::sync::{Mutex, PoisonError};

use futures_util::SinkExt as _;
use iced::window::{self, Id};
use iced::{Element, Size, Subscription, Task, Theme};
use tokio::sync::mpsc;

use crate::args::Cli;
use crate::launch::LaunchMode;
use crate::session::{self, Outcome};
use crate::shell::{DaemonCall, DaemonEvent, Message, Pane, Shell, TrayAction, Visibility};
use crate::tray::{self, TrayModel};
use crate::tray_service::{self, GuiTray};
use crate::view;
use crate::windows::{self, Slots, WindowOp};

/// Runtime state around the shell.
pub struct App {
    shell: Shell,
    mode: LaunchMode,
    address: Option<String>,
    slots: Slots<Id>,
    calls: mpsc::Sender<DaemonCall>,
    tray_tx: mpsc::Sender<TrayAction>,
    calls_rx: std::sync::Arc<Mutex<Option<mpsc::Receiver<DaemonCall>>>>,
    tray_rx: std::sync::Arc<Mutex<Option<mpsc::Receiver<TrayAction>>>>,
    activations_rx: std::sync::Arc<Mutex<Option<mpsc::Receiver<LaunchMode>>>>,
    activations_tx: mpsc::Sender<LaunchMode>,
    tray: Option<ksni::Handle<GuiTray>>,
    shown: Option<TrayModel>,
}

/// Messages the iced runtime delivers.
pub enum AppMessage {
    /// This process owns the GUI bus name.
    Primary,
    /// A later invocation asked us to show `mode`.
    Activated(LaunchMode),
    /// Another GUI is already running.
    HandedOff,
    /// No bus. Open the window anyway.
    BusUnavailable(String),
    /// News from the daemon watcher.
    Daemon(DaemonEvent),
    /// A tray click or menu item.
    Tray(TrayAction),
    /// A window widget.
    Shell(Message),
    /// A window finished closing.
    Closed(Id),
    /// The user asked a window to close.
    CloseRequested(Id),
    /// The tray service came up, or didn't.
    TrayReady(Option<ksni::Handle<GuiTray>>),
    /// A task finished and the shell didn't change.
    Nop,
}

/// Builds the iced daemon from `cli`. Logging is already set up.
///
/// # Errors
///
/// Returns the iced error when the runtime can't start.
pub fn run(cli: &Cli) -> iced::Result {
    let presets = crate::presets::load();
    let mode = cli.launch_mode();
    let address = cli.bus_address.clone();
    iced::daemon(
        move || boot(presets.clone(), mode, address.clone()),
        update,
        view,
    )
    .title(title)
    .subscription(subscription)
    .theme(theme)
    .run()
}

fn boot(presets: Vec<u32>, mode: LaunchMode, address: Option<String>) -> (App, Task<AppMessage>) {
    let (calls, calls_rx) = mpsc::channel(16);
    let (tray_tx, tray_rx) = mpsc::channel(16);
    let (activations_tx, activations_rx) = mpsc::channel(8);
    let app = App {
        shell: crate::boot::shell(presets),
        mode,
        address,
        slots: Slots::default(),
        calls,
        tray_tx,
        calls_rx: std::sync::Arc::new(Mutex::new(Some(calls_rx))),
        tray_rx: std::sync::Arc::new(Mutex::new(Some(tray_rx))),
        activations_rx: std::sync::Arc::new(Mutex::new(Some(activations_rx))),
        activations_tx,
        tray: None,
        shown: None,
    };
    (app, Task::none())
}

fn theme(_app: &App, _window: Id) -> Theme {
    Theme::Dark
}

fn title(app: &App, id: Id) -> String {
    if app.slots.prompt == Some(id) {
        "Stillwatch prompt".to_owned()
    } else {
        "Stillwatch".to_owned()
    }
}

fn view(app: &App, id: Id) -> Element<'_, AppMessage> {
    let element = if app.slots.prompt == Some(id) {
        view::prompt()
    } else {
        view::shell(&app.shell)
    };
    element.map(AppMessage::Shell)
}

fn subscription(app: &App) -> Subscription<AppMessage> {
    Subscription::batch([
        window::close_events().map(AppMessage::Closed),
        window::close_requests().map(AppMessage::CloseRequested),
        shell_events(app),
    ])
}

fn shell_events(app: &App) -> Subscription<AppMessage> {
    let sub = ShellSub {
        address: app.address.clone(),
        mode: app.mode,
        calls: std::sync::Arc::clone(&app.calls_rx),
        trays: std::sync::Arc::clone(&app.tray_rx),
        activations: std::sync::Arc::clone(&app.activations_rx),
        activations_tx: app.activations_tx.clone(),
    };
    Subscription::run_with(sub, shell_stream)
}

#[derive(Clone)]
struct ShellSub {
    address: Option<String>,
    mode: LaunchMode,
    calls: std::sync::Arc<Mutex<Option<mpsc::Receiver<DaemonCall>>>>,
    trays: std::sync::Arc<Mutex<Option<mpsc::Receiver<TrayAction>>>>,
    activations: std::sync::Arc<Mutex<Option<mpsc::Receiver<LaunchMode>>>>,
    activations_tx: mpsc::Sender<LaunchMode>,
}

impl PartialEq for ShellSub {
    fn eq(&self, other: &Self) -> bool {
        self.address == other.address
            && self.mode == other.mode
            && std::sync::Arc::ptr_eq(&self.calls, &other.calls)
            && std::sync::Arc::ptr_eq(&self.trays, &other.trays)
            && std::sync::Arc::ptr_eq(&self.activations, &other.activations)
    }
}

impl Eq for ShellSub {}

impl std::hash::Hash for ShellSub {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.address.hash(state);
        self.mode.hash(state);
        std::sync::Arc::as_ptr(&self.calls).hash(state);
        std::sync::Arc::as_ptr(&self.trays).hash(state);
        std::sync::Arc::as_ptr(&self.activations).hash(state);
    }
}

fn shell_stream(
    sub: &ShellSub,
) -> std::pin::Pin<Box<dyn futures_util::Stream<Item = AppMessage> + Send>> {
    let sub = sub.clone();
    Box::pin(iced::stream::channel(64, async move |mut output| {
        let (Some(calls), Some(mut tray_actions), Some(mut activations)) =
            (take(&sub.calls), take(&sub.trays), take(&sub.activations))
        else {
            std::future::pending::<()>().await;
            return;
        };
        let started =
            session::start(sub.address.as_deref(), sub.mode, sub.activations_tx, calls).await;
        let message = match &started.outcome {
            Outcome::Primary => AppMessage::Primary,
            Outcome::HandedOff => AppMessage::HandedOff,
            Outcome::NoBus(text) => AppMessage::BusUnavailable(text.clone()),
        };
        if output.send(message).await.is_err() {
            return;
        }
        let Outcome::Primary = started.outcome else {
            std::future::pending::<()>().await;
            return;
        };
        let mut daemon = started.daemon;
        loop {
            let next = tokio::select! {
                biased;
                event = daemon.recv() => event.map(AppMessage::Daemon),
                mode = activations.recv() => mode.map(AppMessage::Activated),
                action = tray_actions.recv() => action.map(AppMessage::Tray),
            };
            let Some(next) = next else { break };
            if output.send(next).await.is_err() {
                break;
            }
        }
    }))
}

fn take<T>(slot: &Mutex<Option<T>>) -> Option<T> {
    slot.lock().unwrap_or_else(PoisonError::into_inner).take()
}

fn update(app: &mut App, message: AppMessage) -> Task<AppMessage> {
    let mut calls = Vec::new();
    let mut start_tray = false;
    match message {
        AppMessage::Nop => {}
        AppMessage::HandedOff => return iced::exit(),
        AppMessage::Primary => {
            let _ = crate::model::update(
                &mut app.shell,
                Message::BecamePrimary {
                    first: true,
                    mode: app.mode,
                },
            );
            start_tray = true;
        }
        AppMessage::Activated(mode) => {
            let _ = crate::model::update(
                &mut app.shell,
                Message::BecamePrimary { first: false, mode },
            );
        }
        AppMessage::BusUnavailable(text) => {
            app.shell.notice = Some(text);
            let _ = crate::model::update(
                &mut app.shell,
                Message::BecamePrimary {
                    first: true,
                    mode: app.mode.without_bus(),
                },
            );
        }
        AppMessage::Daemon(event) => {
            let _ = crate::model::update(&mut app.shell, Message::Daemon(event));
        }
        AppMessage::Tray(action) => {
            calls = crate::model::update(&mut app.shell, Message::Tray(action));
        }
        AppMessage::Shell(message) => {
            calls = crate::model::update(&mut app.shell, message);
        }
        AppMessage::CloseRequested(id) => {
            if let Some(pane) = app.slots.pane_of(&id) {
                hide(&mut app.shell, pane);
            }
        }
        AppMessage::Closed(id) => {
            if let Some(pane) = app.slots.take_id(&id) {
                hide(&mut app.shell, pane);
            }
        }
        AppMessage::TrayReady(handle) => {
            app.tray = handle;
            app.shown = None;
        }
    }
    if app.shell.quit {
        return shutdown(app);
    }
    let mut tasks = apply_windows(app);
    if start_tray {
        tasks.push(spawn_tray(app));
    }
    tasks.extend(send_calls(&app.calls, calls));
    if let Some(task) = refresh_tray(app) {
        tasks.push(task);
    }
    Task::batch(tasks)
}

fn hide(shell: &mut Shell, pane: Pane) {
    match pane {
        Pane::Settings => shell.settings = Visibility::Closed,
        Pane::Prompt => shell.prompt = Visibility::Closed,
    }
}

fn shutdown(app: &mut App) -> Task<AppMessage> {
    let Some(handle) = app.tray.take() else {
        return iced::exit();
    };
    Task::perform(
        async move {
            handle.shutdown().await;
        },
        |()| AppMessage::HandedOff,
    )
}

fn spawn_tray(app: &App) -> Task<AppMessage> {
    let model = tray::tray_model(&app.shell);
    let actions = app.tray_tx.clone();
    Task::perform(
        async move {
            match tray_service::spawn(model, actions).await {
                Ok(handle) => Some(handle),
                Err(err) => {
                    tracing::warn!(%err, "tray didn't start");
                    None
                }
            }
        },
        AppMessage::TrayReady,
    )
}

fn send_calls(tx: &mpsc::Sender<DaemonCall>, calls: Vec<DaemonCall>) -> Vec<Task<AppMessage>> {
    calls
        .into_iter()
        .map(|call| {
            let tx = tx.clone();
            Task::perform(
                async move {
                    let _ = tx.send(call).await;
                },
                |()| AppMessage::Nop,
            )
        })
        .collect()
}

/// Wayland ignores [`window::gain_focus`]. Attention is the request a
/// compositor can still honor when a second process asks the window forward.
fn apply_windows(app: &mut App) -> Vec<Task<AppMessage>> {
    let ops = windows::plan(&app.slots, &app.shell);
    app.shell.settle_focus();
    let mut tasks = Vec::new();
    for op in ops {
        match op {
            WindowOp::Open(pane) => {
                let (id, opened) = window::open(window_settings(pane));
                app.slots.insert(pane, id);
                tasks.push(opened.map(|_| AppMessage::Nop));
            }
            WindowOp::Close(id) => {
                let _ = app.slots.take_id(&id);
                tasks.push(window::close(id));
            }
            WindowOp::Focus(id) => {
                tasks.push(window::gain_focus(id));
                tasks.push(window::request_user_attention(
                    id,
                    Some(window::UserAttention::Critical),
                ));
            }
        }
    }
    tasks
}

fn window_settings(pane: Pane) -> window::Settings {
    let size = match pane {
        Pane::Settings => Size::new(840.0, 560.0),
        Pane::Prompt => Size::new(420.0, 240.0),
    };
    window::Settings {
        size,
        ..window::Settings::default()
    }
}

fn refresh_tray(app: &mut App) -> Option<Task<AppMessage>> {
    let model = tray::tray_model(&app.shell);
    if app.shown.as_ref() == Some(&model) {
        return None;
    }
    let handle = app.tray.clone()?;
    app.shown = Some(model.clone());
    Some(Task::perform(
        async move {
            handle.update(|tray| tray.set_model(model)).await;
        },
        |()| AppMessage::Nop,
    ))
}
