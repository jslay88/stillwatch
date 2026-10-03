//! A fake `org.freedesktop.systemd1` on a private bus.
//!
//! Nothing here talks to the session bus, the user systemd, or `systemctl`.

#![allow(missing_docs)]

use std::sync::{Arc, Mutex};

use stillwatch_gui::{
    UNIT_NAME, UnitQuery, disable_unit, enable_unit, query_unit, restart_unit, start_unit,
    stop_unit,
};
use stillwatch_testkit::PrivateBus;
use zbus::Connection;
use zbus::zvariant::OwnedObjectPath;

const MANAGER_PATH: &str = "/org/freedesktop/systemd1";
const UNIT_PATH: &str = "/org/freedesktop/systemd1/unit/stillwatch_2eservice";
const JOB_PATH: &str = "/org/freedesktop/systemd1/job/1";

type UnitFileChange = (String, String, String);

#[derive(Debug)]
struct State {
    installed: bool,
    active: String,
    unit_file: String,
    calls: Vec<String>,
}

impl State {
    fn installed() -> Self {
        Self {
            installed: true,
            active: "inactive".into(),
            unit_file: "disabled".into(),
            calls: Vec::new(),
        }
    }

    fn require(&self, name: &str) -> Result<(), SdError> {
        if self.installed && name == UNIT_NAME {
            Ok(())
        } else {
            Err(SdError::NoSuchUnit(format!("Unit {name} not found.")))
        }
    }
}

fn lock(state: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.systemd1")]
enum SdError {
    #[zbus(error)]
    ZBus(zbus::Error),
    NoSuchUnit(String),
}

struct Manager {
    state: Arc<Mutex<State>>,
}

struct UnitObject {
    state: Arc<Mutex<State>>,
}

#[zbus::interface(name = "org.freedesktop.systemd1.Manager")]
impl Manager {
    fn get_unit_file_state(&self, file: &str) -> Result<String, SdError> {
        let mut state = lock(&self.state);
        state.calls.push(format!("GetUnitFileState {file}"));
        state.require(file)?;
        Ok(state.unit_file.clone())
    }

    fn load_unit(&self, name: &str) -> Result<OwnedObjectPath, SdError> {
        let mut state = lock(&self.state);
        state.calls.push(format!("LoadUnit {name}"));
        state.require(name)?;
        OwnedObjectPath::try_from(UNIT_PATH).map_err(|err| SdError::NoSuchUnit(err.to_string()))
    }

    fn start_unit(&self, name: &str, mode: &str) -> Result<OwnedObjectPath, SdError> {
        self.job("StartUnit", name, mode, "active")
    }

    fn stop_unit(&self, name: &str, mode: &str) -> Result<OwnedObjectPath, SdError> {
        self.job("StopUnit", name, mode, "inactive")
    }

    fn restart_unit(&self, name: &str, mode: &str) -> Result<OwnedObjectPath, SdError> {
        self.job("RestartUnit", name, mode, "active")
    }

    fn enable_unit_files(
        &self,
        files: Vec<String>,
        runtime: bool,
        force: bool,
    ) -> Result<(bool, Vec<UnitFileChange>), SdError> {
        let mut state = lock(&self.state);
        let name = files.into_iter().next().unwrap_or_default();
        state.calls.push(format!(
            "EnableUnitFiles {name} runtime={runtime} force={force}"
        ));
        state.require(&name)?;
        state.unit_file = "enabled".into();
        Ok((true, Vec::new()))
    }

    fn disable_unit_files(
        &self,
        files: Vec<String>,
        runtime: bool,
    ) -> Result<Vec<UnitFileChange>, SdError> {
        let mut state = lock(&self.state);
        let name = files.into_iter().next().unwrap_or_default();
        state
            .calls
            .push(format!("DisableUnitFiles {name} runtime={runtime}"));
        state.require(&name)?;
        state.unit_file = "disabled".into();
        Ok(Vec::new())
    }

    fn reload(&self) {
        lock(&self.state).calls.push("Reload".into());
    }
}

impl Manager {
    fn job(
        &self,
        method: &str,
        name: &str,
        mode: &str,
        active: &str,
    ) -> Result<OwnedObjectPath, SdError> {
        let mut state = lock(&self.state);
        state.calls.push(format!("{method} {name} {mode}"));
        state.require(name)?;
        active.clone_into(&mut state.active);
        OwnedObjectPath::try_from(JOB_PATH).map_err(|err| SdError::NoSuchUnit(err.to_string()))
    }
}

#[zbus::interface(name = "org.freedesktop.systemd1.Unit")]
impl UnitObject {
    #[zbus(property)]
    fn active_state(&self) -> String {
        lock(&self.state).active.clone()
    }
}

async fn serve(conn: &Connection, state: Arc<Mutex<State>>) -> zbus::Result<()> {
    let manager = conn
        .object_server()
        .at(
            MANAGER_PATH,
            Manager {
                state: Arc::clone(&state),
            },
        )
        .await?;
    let unit = conn
        .object_server()
        .at(
            UNIT_PATH,
            UnitObject {
                state: Arc::clone(&state),
            },
        )
        .await?;
    if !manager || !unit {
        return Err(zbus::Error::Failure("systemd path already served".into()));
    }
    conn.request_name("org.freedesktop.systemd1").await?;
    Ok(())
}

#[tokio::test]
async fn start_stop_enable_and_failed_go_through_the_fake_manager() {
    let Some(bus) = PrivateBus::start().unwrap() else {
        return;
    };
    let client = bus.connect().await.unwrap();
    let missing = query_unit(&client).await.unwrap_err();
    assert!(missing.contains("not available"), "{missing}");

    let server = bus.connect().await.unwrap();
    let state = Arc::new(Mutex::new(State::installed()));
    serve(&server, Arc::clone(&state)).await.unwrap();

    assert_eq!(
        query_unit(&client).await.unwrap(),
        UnitQuery::Present {
            active: "inactive".into(),
            file_state: "disabled".into(),
        }
    );

    start_unit(&client).await.unwrap();
    assert_eq!(active(&client).await.unwrap(), "active");
    assert!(
        calls(&state)
            .iter()
            .any(|call| call == "StartUnit stillwatch.service replace")
    );

    stop_unit(&client).await.unwrap();
    assert_eq!(active(&client).await.unwrap(), "inactive");

    restart_unit(&client).await.unwrap();
    assert_eq!(active(&client).await.unwrap(), "active");

    enable_unit(&client).await.unwrap();
    assert_eq!(file_state(&client).await.unwrap(), "enabled");
    assert!(
        calls(&state)
            .iter()
            .any(|call| call.starts_with("EnableUnitFiles"))
    );
    assert!(calls(&state).iter().any(|call| call == "Reload"));

    disable_unit(&client).await.unwrap();
    assert_eq!(file_state(&client).await.unwrap(), "disabled");

    state.lock().unwrap().active = "failed".into();
    assert_eq!(active(&client).await.unwrap(), "failed");

    state.lock().unwrap().installed = false;
    assert_eq!(query_unit(&client).await.unwrap(), UnitQuery::Missing);
    let err = start_unit(&client).await.unwrap_err();
    assert!(err.contains("not installed"), "{err}");
}

async fn active(conn: &Connection) -> Result<String, String> {
    match query_unit(conn).await? {
        UnitQuery::Present { active, .. } => Ok(active),
        UnitQuery::Missing => Err("unit disappeared".into()),
    }
}

async fn file_state(conn: &Connection) -> Result<String, String> {
    match query_unit(conn).await? {
        UnitQuery::Present { file_state, .. } => Ok(file_state),
        UnitQuery::Missing => Err("unit disappeared".into()),
    }
}

fn calls(state: &Mutex<State>) -> Vec<String> {
    lock(state).calls.clone()
}
