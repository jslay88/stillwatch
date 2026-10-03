//! Substituting the installed binary directory into packaging files.

/// `Exec=` and `ExecStart=` prefix baked into the files under `packaging/`.
pub const PACKAGED_BINDIR: &str = "/usr/bin";

/// Stillwatch binaries. `stillwatch` is a prefix of the other two names, so a
/// match has to end at a boundary.
const BINARIES: [&str; 3] = ["stillwatchd", "stillwatch-gui", "stillwatch"];

/// Replaces packaged `/usr/bin/<binary>` paths with `bindir`.
///
/// `/usr/bin/kill` and every other `/usr/bin` path stay put, so `ExecReload`
/// still works when the prefix is not `/usr`.
#[must_use]
pub fn rewrite_bindir(text: &str, bindir: &str) -> String {
    let needle = format!("{PACKAGED_BINDIR}/");
    let mut rewritten = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find(&needle) {
        rewritten.push_str(&rest[..index]);
        let after = &rest[index + needle.len()..];
        if let Some(name) = longest_binary(after) {
            rewritten.push_str(bindir);
            rewritten.push('/');
            rewritten.push_str(name);
            rest = &after[name.len()..];
        } else {
            rewritten.push_str(&needle);
            rest = after;
        }
    }
    rewritten.push_str(rest);
    rewritten
}

fn longest_binary(text: &str) -> Option<&'static str> {
    BINARIES
        .into_iter()
        .filter(|name| binary_at(text, name))
        .max_by_key(|name| name.len())
}

fn binary_at(text: &str, name: &str) -> bool {
    let Some(tail) = text.strip_prefix(name) else {
        return false;
    };
    tail.chars().next().is_none_or(|c| !is_name_char(c))
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

#[cfg(test)]
mod tests {
    use super::{PACKAGED_BINDIR, rewrite_bindir};

    #[test]
    fn rewrites_stillwatch_binaries_and_leaves_kill() {
        let text = "\
ExecStart=/usr/bin/stillwatchd
ExecReload=/usr/bin/kill -HUP $MAINPID
Exec=/usr/bin/stillwatch-gui settings
Path=/usr/bin/stillwatch
Other=/usr/bin/stillwatcher
";
        let out = rewrite_bindir(text, "/home/jslay/.local/bin");
        assert_eq!(
            out,
            "\
ExecStart=/home/jslay/.local/bin/stillwatchd
ExecReload=/usr/bin/kill -HUP $MAINPID
Exec=/home/jslay/.local/bin/stillwatch-gui settings
Path=/home/jslay/.local/bin/stillwatch
Other=/usr/bin/stillwatcher
"
        );
    }

    #[test]
    fn usr_prefix_is_unchanged() {
        let text = "ExecStart=/usr/bin/stillwatchd\nExec=/usr/bin/stillwatch\n";
        assert_eq!(rewrite_bindir(text, PACKAGED_BINDIR), text);
    }

    #[test]
    fn text_without_a_packaged_path_is_unchanged() {
        let text = "Exec=stillwatch-gui settings\n";
        assert_eq!(rewrite_bindir(text, "/opt/stillwatch/bin"), text);
    }
}
