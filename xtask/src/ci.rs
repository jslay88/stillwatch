//! Running a sequence of gates, failing fast.

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::gate::Gate;
use crate::workspace;

/// What to do with a gate once tool availability is known.
#[derive(Debug, PartialEq, Eq)]
enum Plan {
    Run(Gate),
    Skip { gate: Gate, missing: &'static str },
}

/// Runs `gates` in order from `root`, stopping at the first failure.
///
/// A gate whose tool is missing is skipped with a message, or fails the run
/// when `strict` is set.
pub fn run(root: &Path, gates: &[Gate], strict: bool) -> Result<()> {
    let plans = plan(gates, strict, workspace::on_path)?;
    let mut skipped = Vec::new();
    for item in plans {
        match item {
            Plan::Skip { gate, missing } => {
                eprintln!(
                    "==> skipping {}: `{missing}` is not installed (use --strict to fail instead)",
                    gate.name()
                );
                skipped.push(gate.name());
            }
            Plan::Run(gate) => {
                eprintln!("==> {}", gate.name());
                gate.run(root)
                    .with_context(|| format!("gate `{}` failed", gate.name()))?;
            }
        }
    }
    if skipped.is_empty() {
        eprintln!("==> all gates passed");
    } else {
        eprintln!("==> gates passed, skipped: {}", skipped.join(", "));
    }
    Ok(())
}

fn plan(gates: &[Gate], strict: bool, available: impl Fn(&str) -> bool) -> Result<Vec<Plan>> {
    let mut plans = Vec::with_capacity(gates.len());
    for &gate in gates {
        match gate.tools().iter().copied().find(|tool| !available(tool)) {
            None => plans.push(Plan::Run(gate)),
            Some(missing) if strict => {
                bail!(
                    "gate `{}` needs `{missing}`, which is not installed",
                    gate.name()
                )
            }
            Some(missing) => plans.push(Plan::Skip { gate, missing }),
        }
    }
    Ok(plans)
}

#[cfg(test)]
mod tests {
    use super::{Plan, plan};
    use crate::gate::Gate;

    #[test]
    fn runs_every_gate_when_tools_exist() {
        let plans = plan(&Gate::ALL, true, |_| true).unwrap();
        assert_eq!(plans, Gate::ALL.map(Plan::Run));
    }

    #[test]
    fn skips_gates_with_missing_tools() {
        let plans = plan(&Gate::ALL, false, |tool| {
            tool != "jscpd" && tool != "cargo-nextest"
        })
        .unwrap();
        assert_eq!(
            plans[3],
            Plan::Skip {
                gate: Gate::Jscpd,
                missing: "jscpd"
            }
        );
        assert_eq!(
            plans[6],
            Plan::Skip {
                gate: Gate::Coverage,
                missing: "cargo-nextest"
            }
        );
        assert_eq!(plans[7], Plan::Run(Gate::Bench));
    }

    #[test]
    fn strict_fails_before_running_anything() {
        let err = plan(&Gate::ALL, true, |tool| tool != "cargo-deny").unwrap_err();
        assert_eq!(
            err.to_string(),
            "gate `deny` needs `cargo-deny`, which is not installed"
        );
    }
}
