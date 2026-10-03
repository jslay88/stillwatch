use stillwatch_core::config::PromptStyle;
use stillwatch_core::history::{PromptMedium, PromptReason};

use super::{Choice, PromptFacts, select};

#[test]
fn style_fullscreen_and_capability_pick_the_medium() {
    let notification = Choice {
        medium: PromptMedium::Notification,
        reason: PromptReason::Configured,
    };
    let dialog = Choice {
        medium: PromptMedium::Dialog,
        reason: PromptReason::Configured,
    };
    let auto = Choice {
        medium: PromptMedium::Notification,
        reason: PromptReason::Auto,
    };
    let fullscreen = Choice {
        medium: PromptMedium::Dialog,
        reason: PromptReason::Fullscreen,
    };
    // style, fullscreen, notifications hidden over fullscreen, expected
    let cases = [
        (PromptStyle::Notification, false, false, notification),
        (PromptStyle::Notification, false, true, notification),
        (PromptStyle::Notification, true, false, notification),
        (PromptStyle::Notification, true, true, notification),
        (PromptStyle::Dialog, false, false, dialog),
        (PromptStyle::Dialog, false, true, dialog),
        (PromptStyle::Dialog, true, false, dialog),
        (PromptStyle::Dialog, true, true, dialog),
        (PromptStyle::Auto, false, false, auto),
        (PromptStyle::Auto, false, true, auto),
        (PromptStyle::Auto, true, false, auto),
        (PromptStyle::Auto, true, true, fullscreen),
    ];
    for (style, fullscreen_active, hidden, expected) in cases {
        let facts = PromptFacts {
            fullscreen: fullscreen_active,
            notifications_hidden_over_fullscreen: hidden,
        };
        assert_eq!(
            select(style, facts),
            expected,
            "{style:?} fullscreen={fullscreen_active} hidden={hidden}"
        );
    }
}
