//! Install prefixes and the layout under them.

use std::env;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, ensure};

/// Where `cargo xtask install` puts Stillwatch.
///
/// `/usr` uses the vendor paths (`bin`, `lib/systemd/user`, `share`). Any
/// other prefix, including `~/.local`, uses the XDG layout so a user install
/// is on systemd's and `KWin`'s default search paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefix {
    root: PathBuf,
}

impl Prefix {
    /// Parses a `--prefix` value. A leading `~` is replaced with `home`.
    ///
    /// # Errors
    ///
    /// Fails on an empty prefix, a non-UTF-8 path, or a path whose characters
    /// would need quoting in a desktop `Exec=` line. `KWin` matches the first
    /// word only.
    pub fn parse(arg: &str, home: &Path) -> Result<Self> {
        let trimmed = arg.trim();
        ensure!(!trimmed.is_empty(), "prefix is empty");
        let expanded = expand_tilde(trimmed, home);
        let absolute = if expanded.is_absolute() {
            expanded
        } else {
            env::current_dir()
                .context("can't read the current directory")?
                .join(expanded)
        };
        let root = normalize(&absolute);
        ensure_plain(&root)?;
        Ok(Self { root })
    }

    /// The prefix directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Whether this is the system prefix `/usr`.
    #[must_use]
    pub fn is_system(&self) -> bool {
        self.root == Path::new("/usr")
    }

    /// Directory the binaries are copied into.
    #[must_use]
    pub fn bin_dir(&self) -> PathBuf {
        self.root.join("bin")
    }

    /// `bin_dir` as a string, for rewriting `Exec=` lines.
    ///
    /// # Errors
    ///
    /// Fails if the path is not UTF-8. [`parse`](Self::parse) already rejects
    /// that, so this only fires if the prefix was built some other way.
    pub fn bin_dir_string(&self) -> Result<String> {
        self.bin_dir()
            .into_os_string()
            .into_string()
            .map_err(|_| anyhow::anyhow!("binary directory is not UTF-8"))
    }

    /// The systemd user unit path.
    ///
    /// `/usr/lib/systemd/user` for the system prefix. Everywhere else,
    /// `<prefix>/share/systemd/user`, which is `$XDG_DATA_HOME/systemd/user`
    /// when the prefix is `~/.local`.
    #[must_use]
    pub fn unit_path(&self) -> PathBuf {
        let dir = if self.is_system() {
            self.root.join("lib/systemd/user")
        } else {
            self.root.join("share/systemd/user")
        };
        dir.join("stillwatch.service")
    }

    /// Desktop files `KWin` and the menu see (`share/applications`).
    #[must_use]
    pub fn applications_dir(&self) -> PathBuf {
        self.root.join("share/applications")
    }

    /// Template directory for the tray autostart file.
    ///
    /// The GUI toggle copies it to `~/.config/autostart`. Install does not
    /// put it there, so installing Stillwatch does not start the tray.
    #[must_use]
    pub fn template_dir(&self) -> PathBuf {
        self.root.join("share/stillwatch")
    }
}

fn expand_tilde(arg: &str, home: &Path) -> PathBuf {
    if arg == "~" {
        home.to_path_buf()
    } else if let Some(rest) = arg.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(arg)
    }
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn ensure_plain(path: &Path) -> Result<()> {
    let text = path.to_str().context("prefix is not valid UTF-8")?;
    ensure!(
        text.chars().all(plain_exec_char),
        "prefix {text} can't go in a .desktop Exec= line unquoted"
    );
    Ok(())
}

fn plain_exec_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "/_.+-".contains(c)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::Prefix;

    #[test]
    fn tilde_expands_and_user_layout_uses_xdg_paths() {
        let prefix = Prefix::parse("~/.local", Path::new("/home/jslay")).unwrap();
        assert_eq!(prefix.root(), Path::new("/home/jslay/.local"));
        assert!(!prefix.is_system());
        assert_eq!(prefix.bin_dir(), Path::new("/home/jslay/.local/bin"));
        assert_eq!(
            prefix.unit_path(),
            Path::new("/home/jslay/.local/share/systemd/user/stillwatch.service")
        );
        assert_eq!(prefix.bin_dir_string().unwrap(), "/home/jslay/.local/bin");
    }

    #[test]
    fn usr_uses_the_vendor_unit_path() {
        let prefix = Prefix::parse("/usr/", Path::new("/home/jslay")).unwrap();
        assert!(prefix.is_system());
        assert_eq!(
            prefix.unit_path(),
            Path::new("/usr/lib/systemd/user/stillwatch.service")
        );
        assert_eq!(prefix.bin_dir_string().unwrap(), "/usr/bin");
    }

    #[test]
    fn rejects_empty_and_paths_that_need_quoting() {
        let home = Path::new("/home/jslay");
        assert!(Prefix::parse("  ", home).is_err());
        assert!(Prefix::parse("/opt/my prefix", home).is_err());
        assert_eq!(Prefix::parse("~", home).unwrap().root(), home);
    }
}
