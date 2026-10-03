//! Screen capture through `KWin`'s `org.kde.KWin.ScreenShot2` D-Bus interface.
//!
//! No screen-sharing indicator or prompt appears, but the interface is
//! restricted: `KWin` only answers processes whose executable is named by an
//! installed `.desktop` file (see [`error::not_authorized`] for the exact
//! rule, and `packaging/` for the file). Each capture creates a pipe, passes
//! the write end with `CaptureScreen`, parses the reply's metadata, reads
//! exactly `stride * height` bytes, downscales them to a [`LumaGrid`], and
//! drops the buffer before returning.
//!
//! Outputs come from the compositor's `wl_output` globals
//! ([`crate::outputs`]), whose names are the connector names `CaptureScreen`
//! takes.

mod check;
pub mod error;
mod meta;
mod pipe;
mod proxy;

use std::path::PathBuf;
use std::time::Duration;

use stillwatch_core::backend::{BackendError, BackendFuture, ScreenCapture};
use stillwatch_core::luma::{self, LumaGrid, OutputInfo};
use zbus::proxy::CacheProperties;
use zbus::zvariant::Fd;

pub use check::{CaptureReport, check_output, run_check, startup_check};
pub use error::DESKTOP_FILE;
pub use meta::{FrameMeta, MAX_FRAME_BYTES};
pub use pipe::READ_TIMEOUT;
use proxy::ScreenShot2Proxy;

type ListOutputs = dyn Fn() -> BackendFuture<'static, Vec<OutputInfo>> + Send + Sync;

/// How long [`KwinCapture::connect`]'s connection waits for any reply. `KWin`
/// renders the output before it answers `CaptureScreen`, which takes tens of
/// milliseconds at 4K.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// One capture: what `KWin` reported, and the downscaled luma.
#[derive(Debug, Clone, PartialEq)]
pub struct Capture {
    /// The full-resolution frame's metadata.
    pub meta: FrameMeta,
    /// The frame, downscaled.
    pub grid: LumaGrid,
}

/// A [`ScreenCapture`] backed by `org.kde.KWin.ScreenShot2`.
pub struct KwinCapture {
    proxy: ScreenShot2Proxy<'static>,
    list_outputs: Box<ListOutputs>,
}

impl KwinCapture {
    /// Connects to `KWin` on the session bus, listing outputs over Wayland.
    ///
    /// # Errors
    ///
    /// [`BackendError::Unavailable`] when there's no session bus or `KWin`'s
    /// screenshot interface isn't on it.
    pub async fn connect() -> Result<Self, BackendError> {
        let no_bus = |err: zbus::Error| BackendError::Unavailable(format!("no session bus: {err}"));
        let conn = zbus::connection::Builder::session()
            .map_err(no_bus)?
            .method_timeout(CALL_TIMEOUT)
            .build()
            .await
            .map_err(no_bus)?;
        Self::on(&conn).await
    }

    /// Uses `conn`, which must reach `KWin` (or something serving its
    /// interface), and checks that the interface answers.
    ///
    /// # Errors
    ///
    /// As [`connect`](Self::connect).
    pub async fn on(conn: &zbus::Connection) -> Result<Self, BackendError> {
        let proxy = ScreenShot2Proxy::builder(conn)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(|err| error::call_error(err, "", &current_exe()))?;
        let capture = Self {
            proxy,
            list_outputs: Box::new(|| Box::pin(crate::outputs::list())),
        };
        let version = capture.version().await?;
        tracing::debug!(version, "found KWin ScreenShot2");
        Ok(capture)
    }

    /// Replaces where [`outputs`](ScreenCapture::outputs) comes from.
    #[must_use]
    pub fn with_outputs(
        mut self,
        list: impl Fn() -> BackendFuture<'static, Vec<OutputInfo>> + Send + Sync + 'static,
    ) -> Self {
        self.list_outputs = Box::new(list);
        self
    }

    /// The interface's `Version` property.
    ///
    /// # Errors
    ///
    /// [`BackendError::Unavailable`] when `KWin` doesn't serve the interface.
    pub async fn version(&self) -> Result<u32, BackendError> {
        self.proxy
            .version()
            .await
            .map_err(|err| error::call_error(err, "", &current_exe()))
    }

    /// Captures `output` and downscales it to `downscale_width` cells wide.
    ///
    /// # Errors
    ///
    /// [`BackendError::PermissionDenied`] (with remediation) when `KWin`
    /// hasn't authorized this executable, [`BackendError::NotFound`] for an
    /// unknown output, [`BackendError::Unsupported`] for a pixel format
    /// Stillwatch can't decode, and [`BackendError::Protocol`] for a reply
    /// or frame that doesn't add up.
    pub async fn capture(
        &self,
        output: &str,
        downscale_width: u32,
    ) -> Result<Capture, BackendError> {
        let (mut receiver, writer) = pipe::open()?;
        let reply = self
            .proxy
            .capture_screen(output, proxy::capture_options(), Fd::from(&writer))
            .await;
        drop(writer);
        let results = reply.map_err(|err| error::call_error(err, output, &current_exe()))?;
        let meta = FrameMeta::parse(&results)?;
        let data = pipe::read_frame(&mut receiver, meta.byte_len()?).await?;
        let grid = luma::downscale(&meta.frame(&data), downscale_width)?;
        Ok(Capture { meta, grid })
    }
}

impl ScreenCapture for KwinCapture {
    fn outputs(&self) -> BackendFuture<'_, Vec<OutputInfo>> {
        (self.list_outputs)()
    }

    fn capture_luma<'a>(
        &'a self,
        output: &'a str,
        downscale_width: u32,
    ) -> BackendFuture<'a, LumaGrid> {
        Box::pin(async move {
            self.capture(output, downscale_width)
                .await
                .map(|capture| capture.grid)
        })
    }
}

/// This process's executable as `KWin` sees it, for error messages.
fn current_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("stillwatchd"))
}
