//! Build the `sh -c` spec for action hooks. The runner fires these off so
//! they never block the state machine.

use std::time::Duration;

use stillwatch_core::command::{BlankMethod, HookKind};

use crate::process::CommandSpec;

/// How long a hook or `action.command` may run before it is killed.
pub const HOOK_TIMEOUT: Duration = Duration::from_secs(10);

/// Connector names the hook is acting on, comma-separated.
pub const ENV_OUTPUTS: &str = "STILLWATCH_OUTPUTS";

/// The blank method in effect (`dpms`, `overlay`, `ddc_standby`).
pub const ENV_METHOD: &str = "STILLWATCH_METHOD";

/// Why the hook is running (`blank`, `resume`, `command`, `panel_care`).
pub const ENV_REASON: &str = "STILLWATCH_REASON";

/// `sh -c <script>` with the hook environment and [`HOOK_TIMEOUT`].
#[must_use]
pub fn hook_spec(
    script: &str,
    outputs: &[String],
    method: BlankMethod,
    kind: HookKind,
) -> CommandSpec {
    CommandSpec::new("sh", HOOK_TIMEOUT)
        .arg("-c")
        .arg(script)
        .env(ENV_OUTPUTS, outputs.join(","))
        .env(ENV_METHOD, method.as_str())
        .env(ENV_REASON, kind.reason())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_is_sh_c_with_timeout_and_env() {
        let spec = hook_spec(
            "tv-off",
            &["HDMI-A-1".into(), "DP-1".into()],
            BlankMethod::Dpms,
            HookKind::OnBlank,
        );
        assert_eq!(spec.program, "sh");
        assert_eq!(spec.args, ["-c", "tv-off"]);
        assert_eq!(spec.timeout, HOOK_TIMEOUT);
        assert_eq!(
            spec.env,
            [
                (ENV_OUTPUTS.into(), "HDMI-A-1,DP-1".into()),
                (ENV_METHOD.into(), "dpms".into()),
                (ENV_REASON.into(), "blank".into()),
            ]
        );
    }
}
