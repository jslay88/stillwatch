//! Real backends, built from the config. Tests assemble their own [`Built`].

use std::sync::Arc;

use stillwatch_core::backend::{
    BackendFuture, Blanker, Dimmer, GamepadSource, HistorySink, IdleSource, MediaWatcher, Prompter,
    ScreenCapture, SessionMonitor,
};
use stillwatch_core::config::{CaptureBackend, Config};
use stillwatch_core::state::StaleDetector;
use stillwatch_core::time::Clock;

use crate::action::ActionRunner;
use crate::action::ddc::DdcBlanker;
use crate::action::dpms::DpmsBlanker;
use crate::capture::kwin::KwinCapture;
use crate::capture::portal::{PortalCapture, Presence};
use crate::clock::TokioClock;
use crate::gamepad::{EvdevGamepadSource, GamepadSettings};
use crate::history::HistoryRing;
use crate::idle::WaylandIdleSource;
use crate::media::MprisWatcher;
use crate::overlay::OverlayBlanker;
use crate::process::{CommandRunner, TokioRunner};
use crate::prompt::StylePrompter;
use crate::session::DbusSessionMonitor;

/// Something a reload has to tell that the config traits don't expose
/// (gamepad deadzone, notification urgency, history ring size).
pub(super) type ApplyConfig = Arc<dyn Fn(Config) -> BackendFuture<'static, ()> + Send + Sync>;

/// Everything the loop needs, already constructed.
pub(super) struct Built {
    /// Config the pieces were built from.
    pub config: Config,
    pub idle: Arc<dyn IdleSource>,
    /// Present even when gamepads are off, so a reload can start watching.
    /// Construction opens nothing.
    pub gamepad: Option<Arc<dyn GamepadSource>>,
    /// Whether `activity.gamepad` is on. The source stays unwound when it isn't.
    pub gamepad_on: bool,
    pub capture: Option<Arc<dyn ScreenCapture>>,
    /// Set when `capture` is a portal session, so the loop can start and stop
    /// the stream. `None` for `KWin`, which has no sharing indicator.
    pub portal: Option<Arc<dyn CaptureSession>>,
    pub capture_backend: Option<String>,
    /// Selected backends and why, for `Status()`.
    pub backends: Option<stillwatch_ipc::status::BackendReport>,
    /// Blank method that replaces the configured one.
    pub blank_override: Option<stillwatch_core::command::BlankMethod>,
    /// Last probe, so a reload can re-select without touching the session
    /// when the tests leave this empty.
    pub probe: Option<crate::platform::Probe>,
    /// Names recorded for the last selection. A repeat is not logged again.
    pub selected_names: String,
    pub media: Arc<dyn MediaWatcher>,
    pub prompter: Arc<dyn Prompter>,
    pub session: Arc<dyn SessionMonitor>,
    pub dpms: Arc<dyn Blanker>,
    pub overlay: Arc<dyn Blanker>,
    pub ddc: Arc<dyn Blanker>,
    pub dimmer: Arc<dyn Dimmer>,
    pub history: Arc<dyn HistorySink>,
    pub commands: Arc<dyn CommandRunner>,
    pub clock: Arc<dyn Clock>,
    pub detector: Box<dyn StaleDetector>,
    pub apply_config: ApplyConfig,
    /// When set, the loop watches `wl_output` on `WAYLAND_DISPLAY`. Tests
    /// leave this off so they never open the desktop session.
    pub watch_outputs: bool,
}

impl Built {
    /// The action runner over these blankers. Connected outputs start empty
    /// and are filled once capture lists them.
    pub(super) fn runner(&self) -> ActionRunner {
        ActionRunner::new(
            self.config.clone(),
            Vec::new(),
            crate::action::ActionBackends {
                dpms: Arc::clone(&self.dpms),
                overlay: Arc::clone(&self.overlay),
                ddc: Arc::clone(&self.ddc),
                dimmer: Arc::clone(&self.dimmer),
                session: Arc::clone(&self.session),
                commands: Arc::clone(&self.commands),
                history: Arc::clone(&self.history),
                clock: Arc::clone(&self.clock),
                brightness: None,
            },
        )
    }
}

/// Builds the production backends for `config`.
///
/// Probes the session and selects backends. `auto` uses `KWin` `ScreenShot2` when
/// it is present and authorized, otherwise the portal, otherwise input idle.
/// A missing ext-idle-notify v2, or a forced capture backend that isn't
/// there, fails startup.
///
/// # Errors
///
/// Idle or a forced capture backend isn't available, or the history ring's
/// state directory can't be resolved.
pub(super) async fn assemble(config: &Config) -> anyhow::Result<Built> {
    let clock: Arc<dyn Clock> = Arc::new(TokioClock);
    let started = super::platform::startup(config).await?;
    let opened = started.opened;
    let capture = opened.capture;
    let portal = opened.portal;
    let capture_backend = opened.name;
    let pads = Arc::new(EvdevGamepadSource::with_clock(
        &GamepadSettings::from(&config.activity),
        Arc::clone(&clock),
    ));
    let history = HistoryRing::open_default(&config.history)?;
    let selection = started.selection;
    if let Err(error) = history
        .record(selection.history_entry(clock.wall_now()))
        .await
    {
        tracing::warn!(%error, "couldn't record backend selection");
    }
    let prompter = Arc::new(StylePrompter::session(
        &config.prompt,
        Arc::new(history.clone()),
    ));
    let overlay = Arc::new(OverlayBlanker::new());
    let apply_config = apply_hook(Arc::clone(&pads), Arc::clone(&prompter), history.clone());
    Ok(Built {
        config: config.clone(),
        idle: Arc::new(WaylandIdleSource::new()),
        gamepad: Some(Arc::clone(&pads) as Arc<dyn GamepadSource>),
        gamepad_on: config.activity.gamepad,
        capture,
        portal,
        capture_backend,
        backends: Some(selection.report()),
        blank_override: selection.blank_override(),
        probe: Some(started.probe),
        selected_names: selection.history_names(),
        media: Arc::new(MprisWatcher::session()),
        prompter: prompter as Arc<dyn Prompter>,
        session: Arc::new(DbusSessionMonitor::new()),
        dpms: Arc::new(DpmsBlanker::new()),
        ddc: Arc::new(DdcBlanker::new()),
        dimmer: Arc::clone(&overlay) as Arc<dyn Dimmer>,
        overlay: overlay as Arc<dyn Blanker>,
        history: Arc::new(history),
        commands: Arc::new(TokioRunner),
        clock,
        detector: Box::new(stillwatch_core::detector::BlockDetector::new(config)),
        apply_config,
        watch_outputs: true,
    })
}

fn apply_hook(
    pads: Arc<EvdevGamepadSource>,
    prompter: Arc<StylePrompter>,
    history: HistoryRing,
) -> ApplyConfig {
    Arc::new(move |config: Config| {
        let pads = Arc::clone(&pads);
        let prompter = Arc::clone(&prompter);
        let history = history.clone();
        Box::pin(async move {
            pads.update_settings(&GamepadSettings::from(&config.activity));
            prompter.apply(&config.prompt);
            if let Err(error) = history.apply_config(&config.history).await {
                tracing::warn!(%error, "couldn't apply history settings");
            }
            Ok(())
        })
    })
}

/// A capture backend whose stream must not run while the user is active.
pub(super) trait CaptureSession: Send + Sync {
    /// Open the stream. The sharing indicator comes up here.
    fn set_away(&self) -> BackendFuture<'_, ()>;
    /// Close the stream. The sharing indicator goes away.
    fn set_active(&self) -> BackendFuture<'_, ()>;
}

impl CaptureSession for PortalCapture {
    fn set_away(&self) -> BackendFuture<'_, ()> {
        Box::pin(self.set_presence(Presence::Away))
    }

    fn set_active(&self) -> BackendFuture<'_, ()> {
        Box::pin(self.set_presence(Presence::Active))
    }
}

/// What [`open_choice`] connected.
pub(super) struct OpenedCapture {
    pub capture: Option<Arc<dyn ScreenCapture>>,
    pub portal: Option<Arc<dyn CaptureSession>>,
    pub name: Option<String>,
}

impl OpenedCapture {
    pub(super) fn none() -> Self {
        Self {
            capture: None,
            portal: None,
            name: None,
        }
    }
}

/// Opens the capture backend [`select`](crate::platform::select) already chose.
///
/// `auto` tries the portal when `KWin` was selected but then refuses the
/// connection, and input idle when that fails too. A forced backend returns
/// the error.
///
/// # Errors
///
/// The forced backend didn't connect.
pub(super) async fn open_choice(
    config: &Config,
    selection: &crate::platform::Selection,
) -> anyhow::Result<OpenedCapture> {
    use crate::platform::CaptureChoice;
    match selection.capture {
        CaptureChoice::Kwin { .. } => match open_kwin().await {
            Ok(opened) => Ok(opened),
            Err(error) if config.capture.backend == CaptureBackend::Auto => {
                tracing::warn!(%error, "KWin ScreenShot2 didn't connect; trying portal ScreenCast");
                open_portal_or_idle().await
            }
            Err(error) => Err(error.into()),
        },
        CaptureChoice::Portal => match open_portal().await {
            Ok(opened) => Ok(opened),
            Err(error) if config.capture.backend == CaptureBackend::Auto => {
                tracing::warn!(%error, "portal ScreenCast didn't connect; running on input idle only");
                Ok(OpenedCapture::none())
            }
            Err(error) => Err(error.into()),
        },
        CaptureChoice::InputIdleOnly | CaptureChoice::Unavailable => Ok(OpenedCapture::none()),
    }
}

async fn open_kwin() -> Result<OpenedCapture, stillwatch_core::backend::BackendError> {
    let capture = KwinCapture::connect().await?;
    Ok(OpenedCapture {
        capture: Some(Arc::new(capture)),
        portal: None,
        name: Some("kwin".into()),
    })
}

async fn open_portal_or_idle() -> anyhow::Result<OpenedCapture> {
    match open_portal().await {
        Ok(opened) => Ok(opened),
        Err(error) => {
            tracing::warn!(%error, "portal ScreenCast didn't connect; running on input idle only");
            Ok(OpenedCapture::none())
        }
    }
}

async fn open_portal() -> Result<OpenedCapture, stillwatch_core::backend::BackendError> {
    let capture = PortalCapture::connect().await?;
    let portal = Arc::new(capture);
    Ok(OpenedCapture {
        capture: Some(Arc::clone(&portal) as Arc<dyn ScreenCapture>),
        portal: Some(portal),
        name: Some("portal".into()),
    })
}
