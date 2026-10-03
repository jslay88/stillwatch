//! Live outputs, gamepads, and MPRIS players from the daemon.
//!
//! An empty catalog means the daemon isn't running (or hasn't answered yet).
//! The pickers still edit the configured lists, and anything that isn't in
//! this snapshot is shown as not connected.

use stillwatch_ipc::gamepad::GamepadInfo;
use stillwatch_ipc::json::from_json;
use stillwatch_ipc::player::PlayerInfo;
use stillwatch_ipc::proxy::StillwatchProxy;

use crate::error::Error;

/// How long a gamepad event keeps the activity mark lit.
///
/// `Gamepads()` reports age in whole seconds and the settings page polls about
/// once a second, so two seconds keeps one event past the deadzone visible
/// across a poll. The daemon only stamps [`GamepadInfo::seconds_since_activity`]
/// for input that already cleared the deadzone.
pub const ACTIVITY_PULSE_SECONDS: u64 = 2;

/// Label shown beside a gamepad that moved inside [`ACTIVITY_PULSE_SECONDS`].
pub const ACTIVITY_LABEL: &str = "active";

/// Whether a gamepad event is still inside the pulse window.
#[must_use]
pub fn activity_lit(seconds_since_activity: Option<u64>) -> bool {
    seconds_since_activity.is_some_and(|age| age < ACTIVITY_PULSE_SECONDS)
}

/// What the daemon last reported for the pickers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Catalog {
    /// Connected output connector names.
    pub outputs: Vec<String>,
    /// Detected gamepads.
    pub gamepads: Vec<GamepadSeen>,
    /// MPRIS players on the bus.
    pub players: Vec<PlayerSeen>,
}

/// One player from `Players()`.
///
/// `name` is the bus-name suffix. `identity` is shown beside it. The checkbox
/// still stores the suffix (or whatever free text the file already has).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerSeen {
    /// Bus-name suffix, including an instance suffix when the player has one.
    pub name: String,
    /// `Identity`. Empty when the player did not report one.
    pub identity: String,
}

impl From<PlayerInfo> for PlayerSeen {
    fn from(info: PlayerInfo) -> Self {
        Self {
            name: info.name,
            identity: info.identity,
        }
    }
}

/// One gamepad from `Gamepads()`, without the daemon's saved ignore flag.
///
/// The checkbox follows the form, which may have unsaved edits. Activity age
/// is the only fact taken from the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GamepadSeen {
    /// Stable id for this connection.
    pub id: String,
    /// Reported device name. Selecting the row stores this substring.
    pub name: String,
    /// Seconds since input past the deadzone, if any was seen.
    pub seconds_since_activity: Option<u64>,
}

impl From<GamepadInfo> for GamepadSeen {
    fn from(info: GamepadInfo) -> Self {
        Self {
            id: info.id,
            name: info.name,
            seconds_since_activity: info.seconds_since_activity,
        }
    }
}

/// `Outputs()`, `Gamepads()`, and `Players()` in one round trip.
///
/// # Errors
///
/// Returns the D-Bus or JSON error from the first call that fails.
pub(crate) async fn load(proxy: &StillwatchProxy<'_>) -> Result<Catalog, Error> {
    let outputs = proxy.outputs().await?;
    let pads: Vec<GamepadInfo> = from_json(&proxy.gamepads().await?)?;
    let players: Vec<PlayerInfo> = from_json(&proxy.players().await?)?;
    Ok(Catalog {
        outputs,
        gamepads: pads.into_iter().map(GamepadSeen::from).collect(),
        players: players.into_iter().map(PlayerSeen::from).collect(),
    })
}
