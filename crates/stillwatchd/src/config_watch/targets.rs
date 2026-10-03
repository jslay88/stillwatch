//! Which paths a config file depends on and which directories to watch for
//! them.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Symlink hops followed before giving up, as the kernel does (`ELOOP`).
const MAX_LINKS: usize = 40;

/// The config path, every symlink target it resolves through, and the
/// nearest existing directory above each of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Targets {
    chain: Vec<PathBuf>,
    dirs: BTreeSet<PathBuf>,
}

impl Targets {
    /// Resolves `path` as it is on disk right now. A dangling link still
    /// counts its target, so the target's directory is watched for the file
    /// to appear.
    pub(super) fn resolve(path: &Path) -> Self {
        let mut chain = vec![normalize(path)];
        while chain.len() <= MAX_LINKS {
            let Some(current) = chain.last() else { break };
            let Ok(target) = std::fs::read_link(current) else {
                break;
            };
            let next = normalize(&parent(current).join(target));
            chain.push(next);
        }
        let dirs = chain
            .iter()
            .map(|link| nearest_existing(&parent(link)))
            .collect();
        Self { chain, dirs }
    }

    /// The directories to watch.
    pub(super) const fn dirs(&self) -> &BTreeSet<PathBuf> {
        &self.dirs
    }

    /// Whether an event on `path` can affect the config: it's the file, one
    /// of the links to it, or a directory above any of them (created,
    /// removed, or renamed).
    pub(super) fn is_relevant(&self, path: &Path) -> bool {
        self.chain.iter().any(|link| link.starts_with(path))
    }
}

/// `path` with its existing directories resolved, keeping the last
/// component (which may be a link) and anything not created yet as written.
///
/// inotify reports events under the path a directory was watched by, so
/// every path here has to be spelled the same way, even when it was reached
/// through `..` or a symlinked directory.
fn normalize(path: &Path) -> PathBuf {
    for ancestor in path.ancestors().skip(1) {
        let base = if ancestor.as_os_str().is_empty() {
            Path::new(".")
        } else {
            ancestor
        };
        if let (Ok(real), Ok(rest)) = (std::fs::canonicalize(base), path.strip_prefix(ancestor)) {
            return real.join(rest);
        }
    }
    path.to_path_buf()
}

fn parent(path: &Path) -> PathBuf {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

/// `dir` if it exists, otherwise its closest existing ancestor.
fn nearest_existing(dir: &Path) -> PathBuf {
    dir.ancestors()
        .find(|ancestor| ancestor.is_dir())
        .unwrap_or(dir)
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use super::*;

    fn dirs(targets: &Targets) -> Vec<PathBuf> {
        targets.dirs().iter().cloned().collect()
    }

    #[test]
    fn a_plain_file_watches_its_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let file = root.join("config.toml");
        let targets = Targets::resolve(&file);
        assert_eq!(dirs(&targets), std::slice::from_ref(&root));
        assert!(targets.is_relevant(&file));
        assert!(targets.is_relevant(&root));
        assert!(!targets.is_relevant(&root.join("config.toml.swp")));
        assert!(!targets.is_relevant(&root.join("other")));
    }

    #[test]
    fn a_missing_directory_watches_the_closest_ancestor() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let dir = root.join("a/b");
        let targets = Targets::resolve(&dir.join("config.toml"));
        assert_eq!(dirs(&targets), std::slice::from_ref(&root));
        assert!(targets.is_relevant(&root.join("a")));
        assert!(targets.is_relevant(&dir));
        assert!(!targets.is_relevant(&root.join("c")));
    }

    #[test]
    fn symlinks_add_every_target_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        for dir in ["config", "dotfiles", "store"] {
            std::fs::create_dir(root.join(dir)).unwrap();
        }
        let link = root.join("config/config.toml");
        symlink("../dotfiles/config.toml", &link).unwrap();
        let real = root.join("store/config.toml");
        symlink(&real, root.join("dotfiles/config.toml")).unwrap();

        let targets = Targets::resolve(&link);
        assert_eq!(
            dirs(&targets),
            ["config", "dotfiles", "store"].map(|dir| root.join(dir))
        );
        assert!(targets.is_relevant(&real));
        assert!(targets.is_relevant(&root.join("dotfiles/config.toml")));
        assert!(targets.is_relevant(&link));
    }

    #[test]
    fn a_link_loop_stops() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let link = root.join("config.toml");
        symlink("config.toml", &link).unwrap();
        let targets = Targets::resolve(&link);
        assert_eq!(targets.chain.len(), MAX_LINKS + 1);
        assert_eq!(dirs(&targets), std::slice::from_ref(&root));
    }

    #[test]
    fn a_bare_file_name_is_in_the_current_directory() {
        assert_eq!(parent(Path::new("config.toml")), Path::new("."));
        let cwd = std::env::current_dir().unwrap();
        let targets = Targets::resolve(Path::new("no-such-stillwatch-config.toml"));
        assert_eq!(dirs(&targets), [std::fs::canonicalize(cwd).unwrap()]);
    }

    #[test]
    fn missing_parts_are_kept_as_written() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        assert_eq!(
            normalize(&tmp.path().join("a/../b/config.toml")),
            root.join("a/../b/config.toml")
        );
    }
}
