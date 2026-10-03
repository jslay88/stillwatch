//! Human-friendly duration arguments like `45m` or `1h30m`.

use std::time::Duration;

/// Parses a positive duration such as `45m`, `90s`, or `1h 30m`.
///
/// # Errors
///
/// Returns a message suitable for clap if the text isn't a duration or is
/// zero.
pub fn parse_positive(text: &str) -> Result<Duration, String> {
    let duration =
        humantime::parse_duration(text).map_err(|err| format!("{err} (try 45m or 1h30m)"))?;
    if duration.is_zero() {
        return Err("duration must be greater than zero".to_owned());
    }
    Ok(duration)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_forms() {
        assert_eq!(parse_positive("45m"), Ok(Duration::from_mins(45)));
        assert_eq!(parse_positive("90s"), Ok(Duration::from_secs(90)));
        assert_eq!(parse_positive("1h30m"), Ok(Duration::from_mins(90)));
        assert_eq!(parse_positive("1h 30m"), Ok(Duration::from_mins(90)));
        assert_eq!(parse_positive("2h"), Ok(Duration::from_hours(2)));
    }

    #[test]
    fn rejects_zero() {
        for text in ["0s", "0m", "0h 0m"] {
            assert_eq!(
                parse_positive(text),
                Err("duration must be greater than zero".to_owned())
            );
        }
    }

    #[test]
    fn rejects_garbage_with_a_hint() {
        for text in ["", "soon", "45", "-5m", "5 parsecs"] {
            let err = parse_positive(text).unwrap_err();
            assert!(err.contains("try 45m"), "{text:?}: {err}");
        }
    }
}
