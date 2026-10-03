//! What goes into one `Notify` call: text, actions, hints, and timeout.

use std::collections::HashMap;
use std::time::Duration;

use stillwatch_core::config::PromptUrgency;
use stillwatch_core::prompt::{PromptRequest, Reminder};
use zbus::zvariant::Value;

use super::actions::prompt_actions;
use super::proxy::NotificationsProxy;
use super::text;

/// App name, icon, and `desktop-entry` hint.
pub(crate) const APP_ID: &str = "io.github.jslay88.Stillwatch";

/// `expire_timeout` for a notification the server must never drop by itself.
const NEVER_EXPIRE: i32 = 0;

/// `expire_timeout` that leaves it to the server.
const SERVER_DEFAULT: i32 = -1;

/// One notification's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Message {
    pub(crate) summary: String,
    pub(crate) body: String,
    /// Key and label pairs, flattened.
    pub(crate) actions: Vec<String>,
    pub(crate) urgency: PromptUrgency,
    /// Keep the notification after an action is invoked; Stillwatch closes it.
    pub(crate) resident: bool,
    pub(crate) expire_timeout: i32,
}

impl Message {
    /// The burn-in prompt with `remaining` on its countdown.
    pub(crate) fn prompt(
        request: &PromptRequest,
        remaining: Duration,
        urgency: PromptUrgency,
    ) -> Self {
        Self {
            summary: text::prompt_summary(remaining),
            body: text::prompt_body(&request.stale_outputs),
            actions: prompt_actions(request),
            urgency,
            resident: true,
            expire_timeout: NEVER_EXPIRE,
        }
    }

    /// An informational reminder with no buttons that the user dismisses.
    pub(crate) fn reminder(reminder: Reminder) -> Self {
        let (summary, body) = match reminder {
            Reminder::PanelCare { screen_on } => (
                text::PANEL_CARE_SUMMARY.to_owned(),
                text::panel_care_body(screen_on),
            ),
        };
        Self {
            summary,
            body,
            actions: Vec::new(),
            urgency: PromptUrgency::Normal,
            resident: false,
            expire_timeout: SERVER_DEFAULT,
        }
    }

    /// The `hints` argument of `Notify`.
    pub(crate) fn hints(&self) -> HashMap<&'static str, Value<'static>> {
        let mut hints = HashMap::from([
            ("urgency", Value::U8(urgency_level(self.urgency))),
            ("desktop-entry", Value::from(APP_ID)),
        ]);
        if self.resident {
            hints.insert("resident", Value::Bool(true));
        }
        hints
    }

    /// Sends the notification, replacing `replaces_id` unless it's 0, and
    /// returns the id the server gave it.
    pub(crate) async fn send(
        &self,
        proxy: &NotificationsProxy<'_>,
        replaces_id: u32,
    ) -> zbus::Result<u32> {
        let args = (
            APP_ID,
            replaces_id,
            APP_ID,
            &self.summary,
            &self.body,
            &self.actions,
            self.hints(),
            self.expire_timeout,
        );
        proxy.inner().call("Notify", &args).await
    }
}

/// The spec's urgency byte.
pub(crate) const fn urgency_level(urgency: PromptUrgency) -> u8 {
    match urgency {
        PromptUrgency::Low => 0,
        PromptUrgency::Normal => 1,
        PromptUrgency::Critical => 2,
    }
}

#[cfg(test)]
mod tests {
    use stillwatch_core::prompt::StaleOutput;

    use super::*;

    fn request() -> PromptRequest {
        PromptRequest {
            countdown: Duration::from_mins(1),
            presets: vec![Duration::from_mins(15)],
            allow_custom: false,
            stale_outputs: vec![StaleOutput {
                output: "HDMI-A-1".into(),
                unchanged_percent: 84,
            }],
        }
    }

    #[test]
    fn urgency_levels_follow_the_spec() {
        assert_eq!(urgency_level(PromptUrgency::Low), 0);
        assert_eq!(urgency_level(PromptUrgency::Normal), 1);
        assert_eq!(urgency_level(PromptUrgency::Critical), 2);
    }

    #[test]
    fn prompts_are_resident_and_never_expire() {
        let message = Message::prompt(&request(), Duration::from_secs(50), PromptUrgency::Critical);
        assert_eq!(message.summary, "Blanking the screen in 50 s");
        assert!(message.body.starts_with("HDMI-A-1 has been static: 84%"));
        assert_eq!(
            message.actions,
            ["snooze:15", "15 min", "blank-now", "Blank now"]
        );
        assert_eq!(message.expire_timeout, 0);
        let hints = message.hints();
        assert_eq!(hints.get("urgency"), Some(&Value::U8(2)));
        assert_eq!(hints.get("resident"), Some(&Value::Bool(true)));
        assert_eq!(hints.get("desktop-entry"), Some(&Value::from(APP_ID)));
    }

    #[test]
    fn reminders_have_no_buttons_and_normal_urgency() {
        let message = Message::reminder(Reminder::PanelCare {
            screen_on: Duration::from_hours(5),
        });
        assert_eq!(message.summary, "Give the display a rest");
        assert!(message.body.contains("5 h"), "{}", message.body);
        assert_eq!(message.actions, Vec::<String>::new());
        assert_eq!(message.expire_timeout, -1);
        let hints = message.hints();
        assert_eq!(hints.get("urgency"), Some(&Value::U8(1)));
        assert_eq!(hints.get("resident"), None);
    }
}
