//! `.desktop` files that grant an executable `KWin`'s restricted D-Bus
//! interfaces.
//!
//! `KWin` resolves a caller's `/proc/<pid>/exe` and looks for an application
//! whose `Exec=` names that file and whose `X-KDE-DBUS-Restricted-Interfaces`
//! lists the interface. It reads applications through `KSycoca`, which only
//! notices new files a second or two later, so these are written before `KWin`
//! starts.

use std::fs;
use std::path::{Path, PathBuf};

use crate::Error;

/// `KWin`'s screenshot interface, which refuses callers without a grant.
pub const SCREENSHOT2: &str = "org.kde.KWin.ScreenShot2";

/// An executable and the restricted interfaces it may call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Authorization {
    /// The executable. `KWin` compares canonical paths, so symlinks are fine.
    pub exe: PathBuf,
    /// Interface names, such as [`SCREENSHOT2`].
    pub interfaces: Vec<String>,
}

impl Authorization {
    /// Grants `interfaces` to `exe`.
    pub fn new(exe: impl Into<PathBuf>, interfaces: &[&str]) -> Self {
        Self {
            exe: exe.into(),
            interfaces: interfaces.iter().map(|&name| name.to_owned()).collect(),
        }
    }

    /// Grants `interfaces` to the running test binary, which is the process
    /// `KWin` sees when a test calls it over its own connection.
    ///
    /// # Errors
    ///
    /// Fails if the path of the running executable can't be read.
    pub fn current_exe(interfaces: &[&str]) -> Result<Self, Error> {
        Ok(Self::new(std::env::current_exe()?, interfaces))
    }
}

/// Writes the grant as `<data_home>/applications/stillwatch-test-<n>.desktop`
/// and returns its path.
pub fn install(data_home: &Path, index: usize, grant: &Authorization) -> Result<PathBuf, Error> {
    let exe = fs::canonicalize(&grant.exe)?;
    let Some(exec) = exe.to_str().filter(|path| path.chars().all(plain)) else {
        return Err(Error::UnquotablePath(exe));
    };
    let dir = data_home.join("applications");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("stillwatch-test-{index}.desktop"));
    fs::write(&path, entry(exec, &grant.interfaces))?;
    Ok(path)
}

/// Characters an `Exec=` argument may hold without quoting.
fn plain(c: char) -> bool {
    c.is_ascii_alphanumeric() || "/_.+-".contains(c)
}

fn entry(exec: &str, interfaces: &[String]) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Stillwatch test\nExec={exec}\nNoDisplay=true\n\
         X-KDE-DBUS-Restricted-Interfaces={}\n",
        interfaces.join(",")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_an_entry_for_the_canonical_exe() {
        let data = tempfile::tempdir().unwrap();
        let grant = Authorization::current_exe(&[SCREENSHOT2, "org.kde.KWin.Other"]).unwrap();
        let path = install(data.path(), 3, &grant).unwrap();
        assert!(path.ends_with("applications/stillwatch-test-3.desktop"));
        let text = fs::read_to_string(path).unwrap();
        let exe = fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        assert!(text.starts_with("[Desktop Entry]\n"), "{text}");
        assert!(
            text.contains(&format!("\nExec={}\n", exe.display())),
            "{text}"
        );
        assert!(
            text.contains(
                "\nX-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2,org.kde.KWin.Other\n"
            ),
            "{text}"
        );
    }

    #[test]
    fn refuses_paths_that_need_quoting() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("has space");
        fs::write(&exe, "").unwrap();
        let err = install(dir.path(), 0, &Authorization::new(&exe, &[SCREENSHOT2])).unwrap_err();
        assert!(matches!(err, Error::UnquotablePath(_)), "{err}");
        let missing = install(dir.path(), 0, &Authorization::new("/nope/none", &[])).unwrap_err();
        assert!(matches!(missing, Error::Io(_)), "{missing}");
    }
}
