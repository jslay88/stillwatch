//! The `Players()` payload.

use serde::{Deserialize, Serialize};
use stillwatch_core::backend::MediaPlayer;

/// One MPRIS player, for the GUI's ignore-list picker.
///
/// `name` is the bus-name suffix. `identity` is `Identity` from
/// `org.mpris.MediaPlayer2`. They stay separate fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerInfo {
    /// Bus name without `org.mpris.MediaPlayer2.`.
    pub name: String,
    /// `Identity`. Empty when the player did not answer.
    pub identity: String,
}

impl From<&MediaPlayer> for PlayerInfo {
    fn from(player: &MediaPlayer) -> Self {
        Self {
            name: player.name.clone(),
            identity: player.identity.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::{from_json, to_json};

    #[test]
    fn list_round_trips() {
        let players = vec![PlayerInfo {
            name: "vlc".into(),
            identity: "VLC media player".into(),
        }];
        let json = to_json(&players).unwrap();
        assert_eq!(json, r#"[{"name":"vlc","identity":"VLC media player"}]"#);
        assert_eq!(from_json::<Vec<PlayerInfo>>(&json).unwrap(), players);
    }

    #[test]
    fn converts_from_the_watcher_player() {
        let player = MediaPlayer::with_identity("firefox.instance_1_42", "Firefox");
        assert_eq!(
            PlayerInfo::from(&player),
            PlayerInfo {
                name: "firefox.instance_1_42".into(),
                identity: "Firefox".into(),
            }
        );
    }
}
