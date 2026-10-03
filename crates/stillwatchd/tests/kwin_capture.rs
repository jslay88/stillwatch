//! `KwinCapture` against a fake `org.kde.KWin.ScreenShot2` over a
//! peer-to-peer connection. The fake writes a synthetic frame into the passed
//! pipe from another thread after replying, like `KWin` does, so the whole
//! path (pipe, reply, read, parse, downscale) runs without a compositor.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write as _;
use std::sync::{Arc, Mutex};

use stillwatch_core::backend::{BackendError, ScreenCapture as _};
use stillwatch_core::luma::OutputInfo;
use stillwatch_testkit::PrivateBus;
use stillwatchd::capture::kwin::{self, DESKTOP_FILE, KwinCapture};
use zbus::connection::Builder;
use zbus::zvariant::{OwnedFd, OwnedValue, Value};
use zbus::{Connection, Guid};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;
type Calls = Arc<Mutex<Vec<(String, HashMap<String, OwnedValue>)>>>;

const PATH: &str = "/org/kde/KWin/ScreenShot2";
const WIDTH: u32 = 256;
const HEIGHT: u32 = 128;
/// Bigger than a 64 KiB pipe buffer, with row padding.
const STRIDE: u32 = WIDTH * 4 + 16;
const RGB32: u32 = 4;

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.kde.KWin.ScreenShot2.Error")]
enum KwinError {
    #[zbus(error)]
    ZBus(zbus::Error),
    NoAuthorized(String),
    InvalidScreen(String),
}

#[derive(Clone)]
enum Reply {
    /// Reply with this format and write `written` of the frame's bytes.
    Frame {
        format: u32,
        written: usize,
    },
    NoAuthorized,
}

#[derive(Clone)]
struct FakeKwin {
    version: u32,
    reply: Reply,
    calls: Calls,
}

impl FakeKwin {
    fn new(reply: Reply) -> Self {
        Self {
            version: 5,
            reply,
            calls: Calls::default(),
        }
    }
}

/// Left half white, right half black, in `QImage::Format_RGB32` words.
fn frame() -> Vec<u8> {
    let mut data = Vec::new();
    for _ in 0..HEIGHT {
        for x in 0..WIDTH {
            let pixel: u32 = if x < WIDTH / 2 {
                0xffff_ffff
            } else {
                0xff00_0000
            };
            data.extend_from_slice(&pixel.to_ne_bytes());
        }
        data.extend_from_slice(&[0xab; (STRIDE - WIDTH * 4) as usize]);
    }
    data
}

fn value<'a>(value: impl Into<Value<'a>>) -> zbus::Result<OwnedValue> {
    Ok(OwnedValue::try_from(value.into())?)
}

fn metadata(name: &str, format: u32) -> zbus::Result<HashMap<String, OwnedValue>> {
    Ok(HashMap::from([
        ("type".to_owned(), value("raw")?),
        ("format".to_owned(), value(format)?),
        ("width".to_owned(), value(WIDTH)?),
        ("height".to_owned(), value(HEIGHT)?),
        ("stride".to_owned(), value(STRIDE)?),
        ("scale".to_owned(), value(1.0)?),
        ("screen".to_owned(), value(name)?),
    ]))
}

#[zbus::interface(name = "org.kde.KWin.ScreenShot2")]
impl FakeKwin {
    #[zbus(property)]
    fn version(&self) -> u32 {
        self.version
    }

    fn capture_screen(
        &self,
        name: &str,
        options: HashMap<String, OwnedValue>,
        pipe: OwnedFd,
    ) -> Result<HashMap<String, OwnedValue>, KwinError> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push((name.to_owned(), options));
        }
        if name != "Virtual-1" {
            return Err(KwinError::InvalidScreen("Invalid screen requested".into()));
        }
        let Reply::Frame { format, written } = self.reply else {
            return Err(KwinError::NoAuthorized(
                "The process is not authorized to take a screenshot".into(),
            ));
        };
        let reply = metadata(name, format)?;
        let mut file = File::from(std::os::fd::OwnedFd::from(pipe));
        std::thread::spawn(move || {
            let data = frame();
            let _ = file.write_all(&data[..written.min(data.len())]);
        });
        Ok(reply)
    }
}

async fn pair(fake: FakeKwin, path: &str) -> zbus::Result<(Connection, Connection)> {
    let (server, client) = tokio::net::UnixStream::pair()?;
    let server = Builder::unix_stream(server)
        .server(Guid::generate())?
        .p2p()
        .serve_at(path, fake)?
        .build();
    let client = Builder::unix_stream(client).p2p().build();
    tokio::try_join!(server, client)
}

async fn capture_with(reply: Reply) -> TestResult<(KwinCapture, FakeKwin, Connection)> {
    let fake = FakeKwin::new(reply);
    let (server, client) = pair(fake.clone(), PATH).await?;
    let capture = KwinCapture::on(&client)
        .await?
        .with_outputs(|| Box::pin(async { Ok(vec![OutputInfo::new("Virtual-1", WIDTH, HEIGHT)]) }));
    Ok((capture, fake, server))
}

fn full_frame() -> Reply {
    Reply::Frame {
        format: RGB32,
        written: usize::MAX,
    }
}

#[tokio::test]
async fn captures_and_downscales_a_frame() {
    let (capture, fake, _server) = capture_with(full_frame()).await.unwrap();
    assert_eq!(capture.version().await.unwrap(), 5);

    let result = capture.capture("Virtual-1", 2).await.unwrap();
    assert_eq!(
        (result.meta.width, result.meta.height, result.meta.stride),
        (WIDTH, HEIGHT, STRIDE)
    );
    assert_eq!(result.meta.screen.as_deref(), Some("Virtual-1"));
    assert_eq!((result.grid.width(), result.grid.height()), (2, 1));
    assert_eq!(result.grid.data(), &[255, 0]);

    let calls = fake.calls.lock().unwrap();
    let (name, options) = &calls[0];
    assert_eq!(name, "Virtual-1");
    assert_eq!(options["include-cursor"], value(false).unwrap());
    assert_eq!(options["native-resolution"], value(true).unwrap());
}

#[tokio::test]
async fn implements_screen_capture() {
    let (capture, _fake, _server) = capture_with(full_frame()).await.unwrap();
    assert_eq!(
        capture.outputs().await.unwrap(),
        [OutputInfo::new("Virtual-1", WIDTH, HEIGHT)]
    );
    let grid = capture.capture_luma("Virtual-1", 64).await.unwrap();
    assert_eq!((grid.width(), grid.height()), (64, 32));

    let report = kwin::startup_check(&capture).await.unwrap();
    assert_eq!(report.output, "Virtual-1");
    assert_eq!((report.grid_width, report.grid_height), (1, 1));
}

#[tokio::test]
async fn startup_check_needs_an_output() {
    let (capture, _fake, _server) = capture_with(full_frame()).await.unwrap();
    let capture = capture.with_outputs(|| Box::pin(async { Ok(Vec::new()) }));
    assert!(matches!(
        kwin::startup_check(&capture).await,
        Err(BackendError::NotFound(_))
    ));
}

#[tokio::test]
async fn not_authorized_explains_the_desktop_file() {
    let (capture, _fake, _server) = capture_with(Reply::NoAuthorized).await.unwrap();
    let err = capture.capture("Virtual-1", 2).await.unwrap_err();
    let BackendError::PermissionDenied(message) = &err else {
        panic!("expected PermissionDenied, got {err}");
    };
    assert!(message.contains(DESKTOP_FILE), "{message}");
    let exe = std::env::current_exe().unwrap();
    assert!(
        message.contains(&format!("Exec={}", exe.display())),
        "{message}"
    );
}

#[tokio::test]
async fn unknown_outputs_are_not_found() {
    let (capture, _fake, _server) = capture_with(full_frame()).await.unwrap();
    assert_eq!(
        capture.capture("DP-9", 2).await.unwrap_err(),
        BackendError::NotFound("KWin has no output named \"DP-9\"".into())
    );
}

#[tokio::test]
async fn unsupported_formats_are_reported() {
    let reply = Reply::Frame {
        format: 13,
        written: 0,
    };
    let (capture, _fake, _server) = capture_with(reply).await.unwrap();
    assert_eq!(
        capture.capture("Virtual-1", 2).await.unwrap_err(),
        BackendError::Unsupported("unsupported QImage format 13".into())
    );
}

#[tokio::test]
async fn a_short_frame_is_a_protocol_error() {
    let reply = Reply::Frame {
        format: RGB32,
        written: 1000,
    };
    let (capture, _fake, _server) = capture_with(reply).await.unwrap();
    let err = capture.capture("Virtual-1", 2).await.unwrap_err();
    assert!(
        matches!(&err, BackendError::Protocol(m) if m.contains("closed before all")),
        "{err}"
    );
}

#[tokio::test]
async fn a_session_bus_without_kwin_is_unavailable() -> TestResult<()> {
    let Some(bus) = PrivateBus::start()? else {
        return Ok(());
    };
    let conn = bus.connect().await?;
    let err = KwinCapture::on(&conn).await.err().unwrap();
    assert!(
        matches!(&err, BackendError::Unavailable(m) if m.contains("org.kde.KWin.ScreenShot2")),
        "{err}"
    );
    Ok(())
}

#[tokio::test]
async fn a_peer_without_the_interface_is_unavailable() {
    let (_server, client) = pair(FakeKwin::new(full_frame()), "/org/kde/KWin")
        .await
        .unwrap();
    let err = KwinCapture::on(&client).await.err().unwrap();
    assert!(matches!(err, BackendError::Unavailable(_)), "{err}");
}
