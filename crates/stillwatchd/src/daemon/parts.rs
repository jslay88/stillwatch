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
/// `auto` and `kwin` try `ScreenShot2`. `portal` opens a `ScreenCast` session
/// that stays stopped until a capture is wanted. If neither connects, capture
/// stays empty and the daemon runs on input idle.
///
/// # Errors
///
/// The history ring's state directory can't be resolved.
pub(super) async fn assemble(config: &Config) -> anyhow::Result<Built> {
    let clock: Arc<dyn Clock> = Arc::new(TokioClock);
    let opened = open_capture(config).await;
    let capture = opened.capture;
    let portal = opened.portal;
    let capture_backend = opened.name;
    let pads = Arc::new(EvdevGamepadSource::with_clock(
        &GamepadSettings::from(&config.activity),
        Arc::clone(&clock),
    ));
    let history = HistoryRing::open_default(&config.history)?;
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

/// What [`open_capture`] connected.
pub(super) struct OpenedCapture {
    pub capture: Option<Arc<dyn ScreenCapture>>,
    pub portal: Option<Arc<dyn CaptureSession>>,
    pub name: Option<String>,
}

impl OpenedCapture {
    fn none() -> Self {
        Self {
            capture: None,
            portal: None,
            name: None,
        }
    }
}

/// `portal` opens `ScreenCast`. `auto` and `kwin` try `ScreenShot2`.
pub(super) async fn open_capture(config: &Config) -> OpenedCapture {
    if config.capture.backend == CaptureBackend::Portal {
        return open_portal().await;
    }
    match KwinCapture::connect().await {
        Ok(capture) => {
            tracing::info!("using KWin ScreenShot2 capture");
            OpenedCapture {
                capture: Some(Arc::new(capture)),
                portal: None,
                name: Some("kwin".into()),
            }
        }
        Err(error) => {
            tracing::warn!(
                %error,
                "KWin ScreenShot2 capture is unavailable; running on input idle only"
            );
            OpenedCapture::none()
        }
    }
}

async fn open_portal() -> OpenedCapture {
    match PortalCapture::connect().await {
        Ok(capture) => {
            tracing::info!("using portal ScreenCast capture");
            let portal = Arc::new(capture);
            OpenedCapture {
                capture: Some(Arc::clone(&portal) as Arc<dyn ScreenCapture>),
                portal: Some(portal),
                name: Some("portal".into()),
            }
        }
        Err(error) => {
            tracing::warn!(
                %error,
                "portal capture is unavailable; running on input idle only"
            );
            OpenedCapture::none()
        }
    }
}
