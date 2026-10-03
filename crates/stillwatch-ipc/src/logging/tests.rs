use super::*;

#[test]
fn flag_beats_rust_log_and_config() {
    assert_eq!(
        filter_directives(Some("debug"), Some("warn"), "info"),
        "debug"
    );
}

#[test]
fn rust_log_beats_config() {
    assert_eq!(
        filter_directives(None, Some("stillwatchd=trace"), "info"),
        "stillwatchd=trace"
    );
}

#[test]
fn config_is_the_fallback() {
    assert_eq!(filter_directives(None, None, "error"), "error");
}

#[test]
fn blank_values_are_unset() {
    assert_eq!(
        filter_directives(Some("  "), Some(""), DEFAULT_LEVEL),
        DEFAULT_LEVEL
    );
    assert_eq!(filter_directives(Some(""), Some(" warn "), "info"), "warn");
}

#[test]
fn explicit_targets_ignore_the_environment() {
    let id = Some((1, 2));
    assert_eq!(
        select_sink(LogTarget::Stderr, Some("1:2"), id),
        Sink::Stderr
    );
    assert_eq!(select_sink(LogTarget::Journald, None, None), Sink::Journald);
}

#[test]
fn auto_uses_journal_when_stream_matches_stderr() {
    assert_eq!(
        select_sink(LogTarget::Auto, Some("64:12345"), Some((64, 12345))),
        Sink::Journald
    );
}

#[test]
fn auto_uses_stderr_without_journal_stream() {
    assert_eq!(
        select_sink(LogTarget::Auto, None, Some((64, 12345))),
        Sink::Stderr
    );
}

#[test]
fn auto_uses_stderr_when_stream_was_inherited() {
    assert_eq!(
        select_sink(LogTarget::Auto, Some("64:1"), Some((64, 2))),
        Sink::Stderr
    );
    assert_eq!(
        select_sink(LogTarget::Auto, Some("64:1"), None),
        Sink::Stderr
    );
}

#[test]
fn auto_uses_stderr_for_malformed_stream() {
    for value in ["", "64", "a:b", "64:", ":1", "64:1:2"] {
        assert_eq!(
            select_sink(LogTarget::Auto, Some(value), Some((64, 1))),
            Sink::Stderr,
            "{value}"
        );
    }
}

#[test]
fn default_target_is_auto() {
    assert_eq!(LogTarget::default(), LogTarget::Auto);
}

#[test]
fn journald_layer_skipped_for_stderr() {
    let (layer, sink) = journald_layer(Sink::Stderr, LogTarget::Auto, || -> io::Result<()> {
        unreachable!()
    })
    .unwrap();
    assert!(layer.is_none());
    assert_eq!(sink, Sink::Stderr);
}

#[test]
fn journald_layer_used_when_reachable() {
    let (layer, sink) = journald_layer(Sink::Journald, LogTarget::Journald, || Ok(7)).unwrap();
    assert_eq!(layer, Some(7));
    assert_eq!(sink, Sink::Journald);
}

#[test]
fn auto_falls_back_to_stderr_when_journald_is_down() {
    let down = || -> io::Result<()> { Err(io::Error::from(io::ErrorKind::NotFound)) };
    let (layer, sink) = journald_layer(Sink::Journald, LogTarget::Auto, down).unwrap();
    assert!(layer.is_none());
    assert_eq!(sink, Sink::Stderr);
}

#[test]
fn explicit_journald_errors_when_down() {
    let down = || -> io::Result<()> { Err(io::Error::from(io::ErrorKind::NotFound)) };
    let err = journald_layer(Sink::Journald, LogTarget::Journald, down).unwrap_err();
    assert!(matches!(err, LoggingError::Journald(_)));
    assert!(err.to_string().contains("journal"));
}

#[test]
fn stderr_id_is_readable() {
    assert!(stderr_id().is_some());
}

#[test]
fn invalid_filter_is_an_error() {
    let err =
        init_with_override(Some("stillwatch=loud"), DEFAULT_LEVEL, LogTarget::Stderr).unwrap_err();
    assert!(matches!(err, LoggingError::InvalidFilter { .. }));
    assert!(err.to_string().contains("stillwatch=loud"));
}

#[test]
fn second_init_is_an_error() {
    assert_eq!(
        init_with_override(Some("debug"), "info", LogTarget::Stderr).unwrap(),
        Sink::Stderr
    );
    let err = init("info", LogTarget::Stderr).unwrap_err();
    assert!(matches!(err, LoggingError::AlreadyInitialized(_)));
    assert_eq!(err.to_string(), "logging is already initialized");
}

#[test]
fn levels_all_parse() {
    for level in LEVELS {
        assert!(EnvFilter::try_new(level).is_ok(), "{level}");
    }
}
