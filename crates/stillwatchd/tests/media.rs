//! The MPRIS watcher against fake players on a private session bus.

use std::error::Error;
use std::sync::Arc;
use std::time::Duration;

use stillwatch_core::backend::{BackendError, EventSink, MediaPlayer, MediaWatcher};
use stillwatch_core::config::Config;
use stillwatch_core::detector::{BlockDetector, BlockMeans};
use stillwatch_core::event::Event;
use stillwatch_core::stats::ThresholdReason;
use stillwatch_testkit::PrivateBus;
use stillwatch_testkit::mpris::{FakePlayer, SECRET_TITLE};
use stillwatchd::media::MprisWatcher;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::timeout;

type TestResult = Result<(), Box<dyn Error>>;

const WAIT: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_millis(300);
const ALLOWED_READS: [&str; 2] = ["Identity", "PlaybackStatus"];

/// A running `watch` with its events collected.
struct Watch {
    watcher: Arc<MprisWatcher>,
    events: mpsc::UnboundedReceiver<Event>,
    task: JoinHandle<Result<(), BackendError>>,
}

impl Watch {
    fn start(bus: &PrivateBus) -> Self {
        let watcher = Arc::new(MprisWatcher::at_address(bus.address()));
        let (tx, events) = mpsc::unbounded_channel();
        let sink: Arc<dyn EventSink> = Arc::new(move |event| {
            let _ = tx.send(event);
        });
        let task = tokio::spawn({
            let watcher = Arc::clone(&watcher);
            async move { watcher.watch(sink).await }
        });
        Self {
            watcher,
            events,
            task,
        }
    }

    /// The next reported playing set.
    async fn playing(&mut self) -> Result<Vec<MediaPlayer>, Box<dyn Error>> {
        let event = timeout(WAIT, self.events.recv())
            .await?
            .ok_or("the watch stopped")?;
        match event {
            Event::Media { playing } => {
                assert!(playing.iter().all(|player| {
                    !player.name.contains(SECRET_TITLE) && !player.identity.contains(SECRET_TITLE)
                }));
                Ok(playing)
            }
            other => Err(format!("unexpected event {other:?}").into()),
        }
    }

    async fn assert_quiet(&mut self) -> TestResult {
        match timeout(QUIET, self.events.recv()).await {
            Err(_) => Ok(()),
            Ok(event) => Err(format!("expected no event, got {event:?}").into()),
        }
    }

    async fn players(&self) -> Result<Vec<MediaPlayer>, BackendError> {
        self.watcher.players().await
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn listed(players: &[(&str, &str)]) -> Vec<MediaPlayer> {
    players
        .iter()
        .map(|(name, identity)| MediaPlayer::with_identity(*name, *identity))
        .collect()
}

/// Asserts the watcher only ever read `Identity` and `PlaybackStatus`.
fn assert_private(players: &[&FakePlayer]) {
    for player in players {
        let reads = player.reads();
        assert!(
            reads
                .iter()
                .all(|read| ALLOWED_READS.contains(&read.as_str())),
            "{reads:?}"
        );
    }
}

/// Which threshold the detector picks for a playing set, with the default
/// config (`media_ignore_players = ["spotify"]`).
fn threshold(playing: &[MediaPlayer]) -> ThresholdReason {
    threshold_for(&["spotify"], playing)
}

fn threshold_for(ignore: &[&str], playing: &[MediaPlayer]) -> ThresholdReason {
    let mut config = Config::default();
    config.stale.media_ignore_players = ignore.iter().copied().map(str::to_owned).collect();
    let mut detector = BlockDetector::new(&config);
    let means = [128.0_f32; 256];
    let frame = BlockMeans {
        output: "DP-1",
        means: &means,
    };
    detector.observe_means(&[frame], playing).threshold.reason
}

#[tokio::test]
async fn reports_players_already_on_the_bus_when_it_starts() -> TestResult {
    let Some(bus) = PrivateBus::start()? else {
        return Ok(());
    };
    let mpv = FakePlayer::spawn(&bus, "mpv", "mpv", "Playing").await?;
    let vlc = FakePlayer::spawn(&bus, "vlc.instance7389", "VLC media player", "Paused").await?;

    let mut watch = Watch::start(&bus);
    assert_eq!(watch.playing().await?, listed(&[("mpv", "mpv")]));
    assert_eq!(
        watch.players().await?,
        listed(&[("mpv", "mpv"), ("vlc.instance7389", "VLC media player")])
    );
    assert_private(&[&mpv, &vlc]);
    let mut reads = mpv.reads();
    reads.sort();
    reads.dedup();
    assert_eq!(reads, ALLOWED_READS);
    Ok(())
}

#[tokio::test]
async fn play_and_pause_switch_the_threshold_and_repeats_are_quiet() -> TestResult {
    let Some(bus) = PrivateBus::start()? else {
        return Ok(());
    };
    let mpv = FakePlayer::spawn(&bus, "mpv", "mpv", "Paused").await?;
    let mut watch = Watch::start(&bus);
    let playing = watch.playing().await?;
    assert_eq!(playing, listed(&[]));
    assert_eq!(threshold(&playing), ThresholdReason::Normal);

    mpv.set_status("Playing").await?;
    let playing = watch.playing().await?;
    assert_eq!(playing, listed(&[("mpv", "mpv")]));
    assert_eq!(threshold(&playing), ThresholdReason::Media);

    mpv.set_status("Playing").await?;
    mpv.change_track().await?;
    watch.assert_quiet().await?;

    mpv.set_status("Paused").await?;
    assert_eq!(watch.playing().await?, listed(&[]));
    mpv.set_status("Stopped").await?;
    watch.assert_quiet().await?;
    assert_private(&[&mpv]);
    assert_eq!(
        mpv.reads()
            .iter()
            .filter(|read| read.as_str() == "Identity")
            .count(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn an_ignored_player_is_reported_but_keeps_the_normal_threshold() -> TestResult {
    let Some(bus) = PrivateBus::start()? else {
        return Ok(());
    };
    let spotify = FakePlayer::spawn(&bus, "spotify", "Spotify", "Playing").await?;
    let mut watch = Watch::start(&bus);
    let playing = watch.playing().await?;
    assert_eq!(playing, listed(&[("spotify", "Spotify")]));
    assert_eq!(threshold(&playing), ThresholdReason::Normal);

    let mpv = FakePlayer::spawn(&bus, "mpv", "mpv", "Playing").await?;
    let playing = watch.playing().await?;
    assert_eq!(playing, listed(&[("mpv", "mpv"), ("spotify", "Spotify")]));
    assert_eq!(threshold(&playing), ThresholdReason::Media);
    assert_private(&[&spotify, &mpv]);
    Ok(())
}

#[tokio::test]
async fn an_identity_entry_keeps_the_normal_threshold() -> TestResult {
    let Some(bus) = PrivateBus::start()? else {
        return Ok(());
    };
    let vlc = FakePlayer::spawn(&bus, "vlc", "VLC media player", "Playing").await?;
    let mut watch = Watch::start(&bus);
    let playing = watch.playing().await?;
    assert_eq!(playing, listed(&[("vlc", "VLC media player")]));
    assert_eq!(
        threshold_for(&["VLC media player"], &playing),
        ThresholdReason::Normal
    );
    assert_eq!(
        threshold_for(&["media player"], &playing),
        ThresholdReason::Media
    );
    assert_eq!(
        vlc.reads()
            .iter()
            .filter(|read| read.as_str() == "Identity")
            .count(),
        1
    );
    assert_private(&[&vlc]);
    Ok(())
}

#[tokio::test]
async fn a_player_exiting_while_playing_leaves_the_set() -> TestResult {
    let Some(bus) = PrivateBus::start()? else {
        return Ok(());
    };
    let mpv = FakePlayer::spawn(&bus, "mpv", "mpv", "Playing").await?;
    let mut watch = Watch::start(&bus);
    assert_eq!(watch.playing().await?, listed(&[("mpv", "mpv")]));
    assert_private(&[&mpv]);

    mpv.exit().await?;
    assert_eq!(watch.playing().await?, listed(&[]));
    assert_eq!(watch.players().await?, listed(&[]));
    Ok(())
}

#[tokio::test]
async fn a_player_that_comes_back_is_subscribed_again() -> TestResult {
    let Some(bus) = PrivateBus::start()? else {
        return Ok(());
    };
    let mpv = FakePlayer::spawn(&bus, "mpv", "mpv", "Playing").await?;
    let mut watch = Watch::start(&bus);
    assert_eq!(watch.playing().await?, listed(&[("mpv", "mpv")]));
    mpv.exit().await?;
    assert_eq!(watch.playing().await?, listed(&[]));

    let again = FakePlayer::spawn(&bus, "mpv", "mpv", "Playing").await?;
    assert_eq!(watch.playing().await?, listed(&[("mpv", "mpv")]));
    assert_private(&[&again]);
    Ok(())
}

#[tokio::test]
async fn players_appearing_later_are_picked_up() -> TestResult {
    let Some(bus) = PrivateBus::start()? else {
        return Ok(());
    };
    let mut watch = Watch::start(&bus);
    assert_eq!(watch.playing().await?, listed(&[]));

    let not_a_player = bus.connect().await?;
    not_a_player.request_name("org.mpris.MediaPlayer2").await?;
    not_a_player.request_name("org.example.Stillwatch").await?;
    watch.assert_quiet().await?;
    assert_eq!(watch.players().await?, listed(&[]));

    let firefox = FakePlayer::spawn(&bus, "firefox.instance_1_42", "Firefox", "Playing").await?;
    assert_eq!(
        watch.playing().await?,
        listed(&[("firefox.instance_1_42", "Firefox")])
    );
    let paused = FakePlayer::spawn(&bus, "mpv", "mpv", "Paused").await?;
    watch.assert_quiet().await?;
    assert_eq!(
        watch.players().await?,
        listed(&[("firefox.instance_1_42", "Firefox"), ("mpv", "mpv")])
    );
    assert_private(&[&firefox, &paused]);
    Ok(())
}

#[tokio::test]
async fn multiple_players_are_tracked_independently() -> TestResult {
    let Some(bus) = PrivateBus::start()? else {
        return Ok(());
    };
    let mpv = FakePlayer::spawn(&bus, "mpv", "mpv", "Playing").await?;
    let vlc = FakePlayer::spawn(&bus, "vlc", "VLC media player", "Playing").await?;
    let mut watch = Watch::start(&bus);
    assert_eq!(
        watch.playing().await?,
        listed(&[("mpv", "mpv"), ("vlc", "VLC media player")])
    );

    mpv.set_status("Paused").await?;
    assert_eq!(
        watch.playing().await?,
        listed(&[("vlc", "VLC media player")])
    );
    vlc.set_status("Stopped").await?;
    assert_eq!(watch.playing().await?, listed(&[]));
    mpv.set_status("Playing").await?;
    assert_eq!(watch.playing().await?, listed(&[("mpv", "mpv")]));
    assert_private(&[&mpv, &vlc]);
    Ok(())
}

#[tokio::test]
async fn an_invalidated_status_is_read_back() -> TestResult {
    let Some(bus) = PrivateBus::start()? else {
        return Ok(());
    };
    let mpv = FakePlayer::spawn(&bus, "mpv", "mpv", "Paused").await?;
    let mut watch = Watch::start(&bus);
    assert_eq!(watch.playing().await?, listed(&[]));
    mpv.invalidate_status("Playing").await?;
    assert_eq!(watch.playing().await?, listed(&[("mpv", "mpv")]));
    assert_private(&[&mpv]);
    Ok(())
}

#[tokio::test]
async fn players_can_be_listed_without_a_running_watch() -> TestResult {
    let Some(bus) = PrivateBus::start()? else {
        return Ok(());
    };
    let vlc = FakePlayer::spawn(&bus, "vlc", "VLC media player", "Stopped").await?;
    let watcher = MprisWatcher::at_address(bus.address());
    assert_eq!(
        watcher.players().await?,
        listed(&[("vlc", "VLC media player")])
    );
    assert_private(&[&vlc]);
    Ok(())
}

#[tokio::test]
async fn losing_the_bus_ends_the_watch_with_a_transient_error() -> TestResult {
    let Some(mut bus) = PrivateBus::start()? else {
        return Ok(());
    };
    let _mpv = FakePlayer::spawn(&bus, "mpv", "mpv", "Playing").await?;
    let mut watch = Watch::start(&bus);
    assert_eq!(watch.playing().await?, listed(&[("mpv", "mpv")]));

    bus.stop();
    let ended = timeout(WAIT, &mut watch.task).await??;
    let err = ended.err().ok_or("watch ended cleanly")?;
    assert!(err.is_transient(), "{err}");
    let listed = watch.players().await;
    assert!(listed.is_err(), "{listed:?}");
    Ok(())
}

#[tokio::test]
async fn an_unreachable_bus_is_a_transient_error() {
    let watcher = MprisWatcher::at_address("unix:path=/nonexistent/stillwatch/bus");
    let sink: Arc<dyn EventSink> = Arc::new(|_event| {});
    let err = watcher.watch(sink).await.err();
    assert!(
        err.as_ref().is_some_and(BackendError::is_transient),
        "{err:?}"
    );
    assert!(watcher.players().await.is_err());
}
