//! `ScreenCast` session handshake against a fake portal on a private bus.
//!
//! Nothing here talks to the session bus, so it cannot open a permission
//! dialog. The `PipeWire` frame path is covered by the `frame` unit tests.

use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

use stillwatch_core::backend::BackendError;
use stillwatch_testkit::PrivateBus;
use stillwatchd::capture::portal::{TokenStore, open_session};
use zbus::Connection;
use zbus::connection::Builder;
use zbus::fdo;
use zbus::message::Header;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;
type Calls = Arc<Mutex<Vec<(&'static str, HashMap<String, OwnedValue>)>>>;

#[derive(Clone, Copy)]
enum Answer {
    Grant,
    Cancel,
}

struct Portal {
    answer: Answer,
    calls: Calls,
}

fn text(value: &OwnedValue) -> Option<String> {
    value
        .downcast_ref::<String>()
        .ok()
        .or_else(|| value.downcast_ref::<&str>().ok().map(str::to_owned))
}

fn number(value: &OwnedValue) -> Option<u32> {
    value.downcast_ref::<u32>().ok().or_else(|| {
        value
            .downcast_ref::<u64>()
            .ok()
            .and_then(|n| u32::try_from(n).ok())
    })
}

fn flag(value: &OwnedValue) -> Option<bool> {
    value.downcast_ref::<bool>().ok()
}

fn path_for(sender: &str, kind: &str, token: &str) -> String {
    let id = sender.trim_start_matches(':').replace('.', "_");
    format!("/org/freedesktop/portal/desktop/{kind}/{id}/{token}")
}

async fn respond(
    conn: &Connection,
    path: &str,
    code: u32,
    results: HashMap<String, OwnedValue>,
) -> fdo::Result<()> {
    let message =
        zbus::message::Message::signal(path, "org.freedesktop.portal.Request", "Response")?
            .build(&(code, results))?;
    conn.send(&message).await?;
    Ok(())
}

fn owned(value: impl Into<Value<'static>>) -> fdo::Result<OwnedValue> {
    OwnedValue::try_from(value.into()).map_err(|error| fdo::Error::Failed(error.to_string()))
}

fn object_path(path: String) -> fdo::Result<OwnedObjectPath> {
    OwnedObjectPath::try_from(path).map_err(|error| fdo::Error::Failed(error.to_string()))
}

fn handle_token(options: &HashMap<String, OwnedValue>) -> fdo::Result<String> {
    text(
        options
            .get("handle_token")
            .ok_or_else(|| fdo::Error::InvalidArgs("missing handle_token".into()))?,
    )
    .ok_or_else(|| fdo::Error::InvalidArgs("handle_token isn't a string".into()))
}

fn sender_of(header: &Header<'_>) -> fdo::Result<String> {
    header
        .sender()
        .map(ToString::to_string)
        .ok_or_else(|| fdo::Error::Failed("portal call has no sender".into()))
}

struct PortalSession {
    version: u32,
}

#[zbus::interface(name = "org.freedesktop.portal.Session")]
impl PortalSession {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        self.version
    }

    fn close(&self) {
        let _ = self.version;
    }
}

#[zbus::interface(name = "org.freedesktop.portal.ScreenCast")]
impl Portal {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        let _ = self.answer;
        5
    }

    async fn create_session(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<OwnedObjectPath> {
        self.record("CreateSession", &options);
        let sender = sender_of(&header)?;
        let handle = text(
            options
                .get("handle_token")
                .ok_or_else(|| fdo::Error::InvalidArgs("missing handle_token".into()))?,
        )
        .ok_or_else(|| fdo::Error::InvalidArgs("handle_token isn't a string".into()))?;
        let session_token = text(
            options
                .get("session_handle_token")
                .ok_or_else(|| fdo::Error::InvalidArgs("missing session_handle_token".into()))?,
        )
        .ok_or_else(|| fdo::Error::InvalidArgs("session_handle_token isn't a string".into()))?;
        let request = path_for(&sender, "request", &handle);
        let session = path_for(&sender, "session", &session_token);
        conn.object_server()
            .at(session.as_str(), PortalSession { version: 4 })
            .await
            .map_err(|error| fdo::Error::Failed(error.to_string()))?;
        let results = HashMap::from([("session_handle".to_owned(), owned(session)?)]);
        respond(conn, &request, 0, results).await?;
        object_path(request)
    }

    async fn select_sources(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        session: OwnedObjectPath,
        options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<OwnedObjectPath> {
        let _ = session;
        self.record("SelectSources", &options);
        let sender = sender_of(&header)?;
        let handle = handle_token(&options)?;
        let request = path_for(&sender, "request", &handle);
        respond(conn, &request, 0, HashMap::new()).await?;
        object_path(request)
    }

    async fn start(
        &self,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        session: OwnedObjectPath,
        parent: String,
        options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<OwnedObjectPath> {
        let _ = (session, parent);
        self.record("Start", &options);
        let sender = sender_of(&header)?;
        let handle = handle_token(&options)?;
        let request = path_for(&sender, "request", &handle);
        if matches!(self.answer, Answer::Cancel) {
            respond(conn, &request, 1, HashMap::new()).await?;
            return object_path(request);
        }
        let props = HashMap::from([
            ("position".to_owned(), owned((0i32, 0i32))?),
            ("size".to_owned(), owned((1920i32, 1080i32))?),
            ("source_type".to_owned(), owned(1u32)?),
        ]);
        let streams = vec![(7u32, props)];
        let results = HashMap::from([
            ("streams".to_owned(), owned(streams)?),
            ("restore_token".to_owned(), owned("fresh-token")?),
        ]);
        respond(conn, &request, 0, results).await?;
        object_path(request)
    }

    fn open_pipe_wire_remote(
        &self,
        session: OwnedObjectPath,
        options: HashMap<String, OwnedValue>,
    ) -> fdo::Result<zbus::zvariant::OwnedFd> {
        let _ = (session, options);
        self.record("OpenPipeWireRemote", &HashMap::new());
        let (read, write) =
            UnixStream::pair().map_err(|error| fdo::Error::Failed(error.to_string()))?;
        drop(write);
        Ok(OwnedFd::from(read).into())
    }
}

impl Portal {
    fn record(&self, method: &'static str, options: &HashMap<String, OwnedValue>) {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push((method, options.clone()));
        }
    }
}

struct Running {
    bus: PrivateBus,
    server: Connection,
    calls: Calls,
}

async fn serve(answer: Answer) -> TestResult<Option<Running>> {
    let Some(bus) = PrivateBus::start()? else {
        return Ok(None);
    };
    let calls = Calls::default();
    let portal = Portal {
        answer,
        calls: Arc::clone(&calls),
    };
    let server = Builder::address(bus.address())?
        .name("org.freedesktop.portal.Desktop")?
        .serve_at("/org/freedesktop/portal/desktop", portal)?
        .build()
        .await?;
    Ok(Some(Running { bus, server, calls }))
}

async fn client(running: &Running) -> TestResult<Connection> {
    Ok(Builder::address(running.bus.address())?.build().await?)
}

fn recorded(calls: &Calls, method: &str, key: &str) -> Option<OwnedValue> {
    let Ok(calls) = calls.lock() else {
        return None;
    };
    calls
        .iter()
        .find(|(name, _)| *name == method)
        .and_then(|(_, options)| options.get(key).cloned())
}

#[tokio::test]
async fn handshake_restores_and_refreshes_the_token() -> TestResult<()> {
    let Some(running) = serve(Answer::Grant).await? else {
        eprintln!("skipping: dbus-daemon isn't installed");
        return Ok(());
    };
    let dir = tempfile::tempdir()?;
    let tokens = TokenStore::in_dir(dir.path());
    tokens.store(Some("old-token"))?;
    let client = client(&running).await?;
    let opened = open_session(&client, tokens.load()?.as_deref()).await?;
    assert_eq!(opened.streams.len(), 1);
    assert_eq!(opened.streams[0].node_id, 7);
    assert_eq!(opened.streams[0].position, Some((0, 0)));
    assert_eq!(opened.streams[0].size, Some((1920, 1080)));
    assert_eq!(opened.restore_token.as_deref(), Some("fresh-token"));
    tokens.store(opened.restore_token.as_deref())?;
    assert_eq!(tokens.load()?.as_deref(), Some("fresh-token"));

    let multiple = recorded(&running.calls, "SelectSources", "multiple").expect("multiple");
    assert_eq!(flag(&multiple), Some(true));
    let persist = recorded(&running.calls, "SelectSources", "persist_mode").expect("persist");
    assert_eq!(number(&persist), Some(2), "persistent until revoked");
    let kinds = recorded(&running.calls, "SelectSources", "types").expect("types");
    assert_eq!(number(&kinds), Some(1), "monitors only");
    let token = recorded(&running.calls, "SelectSources", "restore_token").expect("token");
    assert_eq!(text(&token).as_deref(), Some("old-token"));
    opened.close().await?;
    drop(running.server);
    Ok(())
}

#[tokio::test]
async fn a_cancelled_start_is_permission_denied_and_keeps_the_token() -> TestResult<()> {
    let Some(running) = serve(Answer::Cancel).await? else {
        eprintln!("skipping: dbus-daemon isn't installed");
        return Ok(());
    };
    let dir = tempfile::tempdir()?;
    let tokens = TokenStore::in_dir(dir.path());
    tokens.store(Some("kept"))?;
    let client = client(&running).await?;
    let Err(error) = open_session(&client, Some("kept")).await else {
        panic!("a cancelled start should fail");
    };
    assert!(
        matches!(error, BackendError::PermissionDenied(_)),
        "{error}"
    );
    assert_eq!(tokens.load()?.as_deref(), Some("kept"));
    Ok(())
}

#[tokio::test]
async fn a_bus_without_a_portal_is_unavailable() -> TestResult<()> {
    let Some(bus) = PrivateBus::start()? else {
        eprintln!("skipping: dbus-daemon isn't installed");
        return Ok(());
    };
    let client = Builder::address(bus.address())?.build().await?;
    let Err(error) = open_session(&client, None).await else {
        panic!("a bus without a portal should fail");
    };
    assert!(matches!(error, BackendError::Unavailable(_)), "{error}");
    Ok(())
}
