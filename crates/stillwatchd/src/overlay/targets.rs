//! Which outputs should be covered, and with what.
//!
//! Requests name outputs by connector; an empty list means every output,
//! including ones plugged in later. [`Desired`] remembers the answer across
//! hotplug and reconnects, so an output that comes back while blanked is
//! covered again.

use std::collections::{BTreeMap, BTreeSet};

use super::Shade;

/// Whether a request for `outputs` covers the output called `name`.
#[must_use]
pub fn selects(outputs: &[String], name: &str) -> bool {
    outputs.is_empty() || outputs.iter().any(|wanted| wanted == name)
}

/// The outputs in `outputs` that aren't in `present`. Always empty for an
/// "every output" request.
#[must_use]
pub fn missing<'a>(outputs: &'a [String], present: &[String]) -> Vec<&'a str> {
    outputs
        .iter()
        .filter(|wanted| !present.contains(wanted))
        .map(String::as_str)
        .collect()
}

/// Which overlays to take down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lift {
    /// Every overlay (unblank).
    Any,
    /// Only dims; blanks stay (undim).
    DimOnly,
}

impl Lift {
    /// Whether an overlay with `shade` comes down.
    #[must_use]
    pub const fn lifts(self, shade: Shade) -> bool {
        match self {
            Self::Any => true,
            Self::DimOnly => shade.is_dim(),
        }
    }
}

/// The outputs that should be covered and the shade for each.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Desired {
    /// Shade for every output, from an empty-list request.
    all: Option<Shade>,
    /// Outputs `all` no longer applies to.
    except: BTreeSet<String>,
    /// Per-output shades, which win over `all`.
    named: BTreeMap<String, Shade>,
}

impl Desired {
    /// Nothing covered.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The shade `name` should have, if it should be covered.
    #[must_use]
    pub fn shade_for(&self, name: &str) -> Option<Shade> {
        self.named
            .get(name)
            .copied()
            .or_else(|| self.all.filter(|_| !self.except.contains(name)))
    }

    /// Covers `outputs` (empty = every output) with `shade`, replacing what
    /// they had.
    pub fn cover(&mut self, outputs: &[String], shade: Shade) {
        if outputs.is_empty() {
            *self = Self {
                all: Some(shade),
                ..Self::default()
            };
            return;
        }
        for name in outputs {
            self.except.remove(name);
            self.named.insert(name.clone(), shade);
        }
    }

    /// Uncovers `outputs` (empty = every output), limited to what `lift`
    /// takes down.
    pub fn uncover(&mut self, outputs: &[String], lift: Lift) {
        let all_lifts = self.all.is_some_and(|shade| lift.lifts(shade));
        if outputs.is_empty() {
            if all_lifts {
                self.all = None;
                self.except.clear();
            }
            self.named.retain(|_, shade| !lift.lifts(*shade));
            return;
        }
        for name in outputs {
            if self.named.get(name).is_some_and(|shade| lift.lifts(*shade)) {
                self.named.remove(name);
            }
            if all_lifts {
                self.except.insert(name.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIM: Shade = Shade::Dim { alpha: 204 };

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|&name| name.to_owned()).collect()
    }

    #[test]
    fn empty_request_selects_everything_and_misses_nothing() {
        assert!(selects(&[], "HDMI-A-1"));
        assert_eq!(missing(&[], &names(&["HDMI-A-1"])), Vec::<&str>::new());
    }

    #[test]
    fn named_request_selects_only_its_connectors() {
        let wanted = names(&["HDMI-A-1", "DP-2"]);
        assert!(selects(&wanted, "DP-2"));
        assert!(!selects(&wanted, "DP-1"));
        assert_eq!(missing(&wanted, &names(&["HDMI-A-1", "DP-1"])), ["DP-2"]);
    }

    #[test]
    fn starts_empty() {
        let desired = Desired::new();
        assert_eq!(desired.shade_for("HDMI-A-1"), None);
    }

    #[test]
    fn covering_everything_includes_outputs_plugged_in_later() {
        let mut desired = Desired::new();
        desired.cover(&[], Shade::Black);
        assert_eq!(desired.shade_for("HDMI-A-1"), Some(Shade::Black));
        assert_eq!(desired.shade_for("never-seen"), Some(Shade::Black));
    }

    #[test]
    fn named_cover_only_touches_those_outputs() {
        let mut desired = Desired::new();
        desired.cover(&names(&["HDMI-A-1"]), DIM);
        assert_eq!(desired.shade_for("HDMI-A-1"), Some(DIM));
        assert_eq!(desired.shade_for("DP-1"), None);
    }

    #[test]
    fn blank_replaces_dim() {
        let mut desired = Desired::new();
        let outputs = names(&["HDMI-A-1"]);
        desired.cover(&outputs, DIM);
        desired.cover(&outputs, Shade::Black);
        assert_eq!(desired.shade_for("HDMI-A-1"), Some(Shade::Black));

        desired.cover(&[], DIM);
        desired.cover(&[], Shade::Black);
        assert_eq!(desired.shade_for("DP-1"), Some(Shade::Black));
    }

    #[test]
    fn covering_everything_drops_earlier_named_shades_and_exceptions() {
        let mut desired = Desired::new();
        desired.cover(&[], DIM);
        desired.uncover(&names(&["DP-1"]), Lift::Any);
        desired.cover(&names(&["HDMI-A-1"]), Shade::Black);
        desired.cover(&[], DIM);
        assert_eq!(desired.shade_for("DP-1"), Some(DIM));
        assert_eq!(desired.shade_for("HDMI-A-1"), Some(DIM));
    }

    #[test]
    fn unblank_everything_clears_everything() {
        let mut desired = Desired::new();
        desired.cover(&[], Shade::Black);
        desired.cover(&names(&["DP-1"]), DIM);
        desired.uncover(&[], Lift::Any);
        assert_eq!(desired, Desired::new());
        assert_eq!(desired.shade_for("DP-1"), None);
    }

    #[test]
    fn unblanking_one_output_of_everything_keeps_the_rest() {
        let mut desired = Desired::new();
        desired.cover(&[], Shade::Black);
        desired.uncover(&names(&["DP-1"]), Lift::Any);
        assert_eq!(desired.shade_for("DP-1"), None);
        assert_eq!(desired.shade_for("HDMI-A-1"), Some(Shade::Black));

        desired.cover(&names(&["DP-1"]), DIM);
        assert_eq!(desired.shade_for("DP-1"), Some(DIM));
    }

    #[test]
    fn undim_leaves_blanks_alone() {
        let mut desired = Desired::new();
        desired.cover(&names(&["HDMI-A-1"]), Shade::Black);
        desired.cover(&names(&["DP-1"]), DIM);
        desired.uncover(&[], Lift::DimOnly);
        assert_eq!(desired.shade_for("HDMI-A-1"), Some(Shade::Black));
        assert_eq!(desired.shade_for("DP-1"), None);

        desired.uncover(&names(&["HDMI-A-1"]), Lift::DimOnly);
        assert_eq!(desired.shade_for("HDMI-A-1"), Some(Shade::Black));
    }

    #[test]
    fn undim_of_a_dimmed_everything() {
        let mut desired = Desired::new();
        desired.cover(&[], DIM);
        desired.uncover(&names(&["DP-1"]), Lift::DimOnly);
        assert_eq!(desired.shade_for("DP-1"), None);
        assert_eq!(desired.shade_for("HDMI-A-1"), Some(DIM));

        desired.uncover(&[], Lift::DimOnly);
        assert_eq!(desired, Desired::new());
    }

    #[test]
    fn undim_of_a_blanked_everything_is_a_no_op() {
        let mut desired = Desired::new();
        desired.cover(&[], Shade::Black);
        desired.uncover(&names(&["DP-1"]), Lift::DimOnly);
        desired.uncover(&[], Lift::DimOnly);
        assert_eq!(desired.shade_for("DP-1"), Some(Shade::Black));
    }

    #[test]
    fn lift_rules() {
        assert!(Lift::Any.lifts(Shade::Black));
        assert!(Lift::Any.lifts(DIM));
        assert!(Lift::DimOnly.lifts(DIM));
        assert!(!Lift::DimOnly.lifts(Shade::Black));
    }
}
