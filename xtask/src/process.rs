//! External commands the gates run.

use std::path::Path;
use std::process::Command;
use std::{env, fmt};

use anyhow::{Context, Result, bail};

/// One external command: a program and its arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// Executable to run, looked up on `PATH`.
    pub program: String,
    /// Arguments passed to `program`.
    pub args: Vec<String>,
}

impl Step {
    /// Builds a step from a program name and its arguments.
    pub fn new<I, S>(program: &str, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            program: program.to_owned(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    /// Runs the step in `cwd` with inherited stdio and fails on a non-zero exit.
    pub fn run(&self, cwd: &Path) -> Result<()> {
        eprintln!("+ {self}");
        let status = self
            .command(cwd)
            .status()
            .with_context(|| format!("failed to start `{self}`"))?;
        if !status.success() {
            bail!("`{self}` exited with {status}");
        }
        Ok(())
    }

    /// Runs the step in `cwd` and returns the raw output, including a failed exit.
    pub fn captured(&self, cwd: &Path) -> Result<std::process::Output> {
        self.command(cwd)
            .output()
            .with_context(|| format!("failed to start `{self}`"))
    }

    /// Runs the step in `cwd` and returns its stdout, failing on a non-zero exit.
    pub fn output(&self, cwd: &Path) -> Result<String> {
        let output = self
            .command(cwd)
            .output()
            .with_context(|| format!("failed to start `{self}`"))?;
        if !output.status.success() {
            bail!(
                "`{self}` exited with {}:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        String::from_utf8(output.stdout).with_context(|| format!("`{self}` printed invalid UTF-8"))
    }

    fn command(&self, cwd: &Path) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.args).current_dir(cwd);
        // `cargo run` exports xtask's own package variables, and some tools
        // (cargo-machete) read them to decide how they were invoked.
        for (key, _) in env::vars_os() {
            if key.to_str().is_some_and(is_package_var) {
                command.env_remove(key);
            }
        }
        command
    }
}

fn is_package_var(key: &str) -> bool {
    key.starts_with("CARGO_PKG_")
        || matches!(
            key,
            "CARGO_MANIFEST_DIR"
                | "CARGO_MANIFEST_PATH"
                | "CARGO_CRATE_NAME"
                | "CARGO_BIN_NAME"
                | "CARGO_PRIMARY_PACKAGE"
        )
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.program)?;
        for arg in &self.args {
            if arg.contains(|c: char| c.is_whitespace() || "|$\\".contains(c)) {
                write!(f, " '{arg}'")?;
            } else {
                write!(f, " {arg}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{Step, is_package_var};

    const NO_ARGS: [&str; 0] = [];

    #[test]
    fn package_vars_are_not_inherited() {
        assert!(is_package_var("CARGO_PKG_NAME"));
        assert!(is_package_var("CARGO_MANIFEST_DIR"));
        assert!(!is_package_var("CARGO"));
        assert!(!is_package_var("CARGO_HOME"));
        assert!(!is_package_var("CARGO_TARGET_DIR"));

        let out = Step::new("sh", ["-c", "echo \"${CARGO_PKG_NAME:-unset}\""])
            .output(Path::new("."))
            .unwrap();
        assert_eq!(out, "unset\n");
    }

    #[test]
    fn display_quotes_shell_sensitive_args() {
        let step = Step::new(
            "cargo",
            ["llvm-cov", "--ignore-filename-regex", r"a\.rs$|b/"],
        );
        assert_eq!(
            step.to_string(),
            r"cargo llvm-cov --ignore-filename-regex 'a\.rs$|b/'"
        );
    }

    #[test]
    fn run_reports_exit_status() {
        let cwd = Path::new(".");
        assert!(Step::new("true", NO_ARGS).run(cwd).is_ok());
        let err = Step::new("false", NO_ARGS).run(cwd).unwrap_err();
        assert!(err.to_string().contains("`false` exited with"));
    }

    #[test]
    fn output_captures_stdout() {
        let out = Step::new("echo", ["hello"]).output(Path::new(".")).unwrap();
        assert_eq!(out, "hello\n");
        assert!(Step::new("false", NO_ARGS).output(Path::new(".")).is_err());
        assert!(
            Step::new("definitely-not-a-real-binary", NO_ARGS)
                .output(Path::new("."))
                .is_err()
        );
    }
}
