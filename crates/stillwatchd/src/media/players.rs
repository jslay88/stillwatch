//! The MPRIS players on the bus and which of them are playing.

use std::collections::BTreeMap;

use stillwatch_core::backend::MediaPlayer;

/// One player, keyed in [`PlayerSet`] by its well-known bus name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Player {
    /// The unique connection name owning the bus name. `PropertiesChanged`
    /// signals arrive from this sender, not from the well-known name.
    pub owner: String,
    /// The reported name: the bus name without `org.mpris.MediaPlayer2.`.
    pub name: String,
    /// `Identity` from `org.mpris.MediaPlayer2`, read once when the player appears.
    pub identity: String,
    /// Whether `PlaybackStatus` is `Playing`.
    pub playing: bool,
}

impl Player {
    fn reported(&self) -> MediaPlayer {
        MediaPlayer::with_identity(self.name.clone(), self.identity.clone())
    }
}

/// Every known player plus the playing set that was last reported.
#[derive(Debug, Default)]
pub(crate) struct PlayerSet {
    players: BTreeMap<String, Player>,
    reported: Option<Vec<MediaPlayer>>,
}

impl PlayerSet {
    /// Adds the player at `bus_name`, replacing any earlier owner.
    pub fn insert(&mut self, bus_name: String, player: Player) {
        self.players.insert(bus_name, player);
    }

    /// Forgets the player at `bus_name`.
    pub fn remove(&mut self, bus_name: &str) {
        self.players.remove(bus_name);
    }

    /// Whether any player is owned by the connection `owner`.
    pub fn owns(&self, owner: &str) -> bool {
        self.players.values().any(|player| player.owner == owner)
    }

    /// Sets the status of every player owned by `owner`.
    pub fn set_playing(&mut self, owner: &str, playing: bool) {
        for player in self.players.values_mut() {
            if player.owner == owner {
                player.playing = playing;
            }
        }
    }

    /// Playing players, in bus-name order.
    pub fn playing(&self) -> Vec<MediaPlayer> {
        self.players
            .values()
            .filter(|player| player.playing)
            .map(Player::reported)
            .collect()
    }

    /// Every player, in bus-name order.
    pub fn all(&self) -> Vec<MediaPlayer> {
        self.players.values().map(Player::reported).collect()
    }

    /// The playing set if it differs from the one last returned here. The
    /// first call always returns it, so a fresh watch reports where it starts.
    pub fn take_change(&mut self) -> Option<Vec<MediaPlayer>> {
        let playing = self.playing();
        if self.reported.as_ref() == Some(&playing) {
            return None;
        }
        self.reported = Some(playing.clone());
        Some(playing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(owner: &str, name: &str, playing: bool) -> Player {
        Player {
            owner: owner.into(),
            name: name.into(),
            identity: String::new(),
            playing,
        }
    }

    fn reported(names: &[&str]) -> Vec<MediaPlayer> {
        names.iter().copied().map(MediaPlayer::named).collect()
    }

    fn set(players: &[(&str, Player)]) -> PlayerSet {
        let mut set = PlayerSet::default();
        for (bus_name, player) in players {
            set.insert((*bus_name).into(), player.clone());
        }
        set
    }

    #[test]
    fn the_first_change_is_always_reported_even_when_empty() {
        let mut players = PlayerSet::default();
        assert_eq!(players.take_change(), Some(vec![]));
        assert_eq!(players.take_change(), None);
    }

    #[test]
    fn only_changes_to_the_playing_set_are_reported() {
        let mut players = set(&[
            ("org.mpris.MediaPlayer2.mpv", player(":1.5", "mpv", false)),
            (
                "org.mpris.MediaPlayer2.spotify",
                player(":1.6", "spotify", true),
            ),
        ]);
        assert_eq!(players.take_change(), Some(reported(&["spotify"])));

        players.set_playing(":1.6", true);
        assert_eq!(players.take_change(), None);

        players.set_playing(":1.5", true);
        assert_eq!(players.take_change(), Some(reported(&["mpv", "spotify"])));

        players.insert(
            "org.mpris.MediaPlayer2.vlc".into(),
            player(":1.7", "vlc.instance7389", false),
        );
        assert_eq!(players.take_change(), None);
        assert_eq!(
            players.all(),
            reported(&["mpv", "spotify", "vlc.instance7389"])
        );
    }

    #[test]
    fn a_player_that_leaves_while_playing_is_dropped_from_the_set() {
        let mut players = set(&[("org.mpris.MediaPlayer2.mpv", player(":1.5", "mpv", true))]);
        assert_eq!(players.take_change(), Some(reported(&["mpv"])));
        players.remove("org.mpris.MediaPlayer2.mpv");
        assert_eq!(players.take_change(), Some(vec![]));
        assert!(!players.owns(":1.5"));
        assert_eq!(players.all(), Vec::<MediaPlayer>::new());
    }

    #[test]
    fn status_follows_the_owner_and_ignores_strangers() {
        let mut players = set(&[
            ("org.mpris.MediaPlayer2.a", player(":1.5", "a", false)),
            ("org.mpris.MediaPlayer2.b", player(":1.5", "b", false)),
            ("org.mpris.MediaPlayer2.c", player(":1.9", "c", false)),
        ]);
        assert!(players.owns(":1.5"));
        assert!(!players.owns(":1.42"));
        players.set_playing(":1.42", true);
        assert_eq!(players.playing(), Vec::<MediaPlayer>::new());
        players.set_playing(":1.5", true);
        assert_eq!(players.playing(), reported(&["a", "b"]));
    }

    #[test]
    fn a_new_owner_replaces_the_old_one() {
        let mut players = set(&[("org.mpris.MediaPlayer2.mpv", player(":1.5", "mpv", true))]);
        players.insert(
            "org.mpris.MediaPlayer2.mpv".into(),
            player(":1.8", "mpv", false),
        );
        assert!(!players.owns(":1.5"));
        assert!(players.owns(":1.8"));
        assert_eq!(players.playing(), Vec::<MediaPlayer>::new());
    }

    #[test]
    fn identity_rides_along_and_a_repeat_status_is_quiet() {
        let mut players = set(&[(
            "org.mpris.MediaPlayer2.vlc",
            Player {
                owner: ":1.2".into(),
                name: "vlc".into(),
                identity: "VLC media player".into(),
                playing: true,
            },
        )]);
        assert_eq!(
            players.take_change(),
            Some(vec![MediaPlayer::with_identity("vlc", "VLC media player")])
        );
        players.set_playing(":1.2", true);
        assert_eq!(players.take_change(), None);
        assert_eq!(
            players.all(),
            vec![MediaPlayer::with_identity("vlc", "VLC media player")]
        );
    }
}
