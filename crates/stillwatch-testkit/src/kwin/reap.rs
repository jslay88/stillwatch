//! Killing `KWin`, its bus, and everything they started.
//!
//! The processes stay in the test's process group so a test runner that
//! kills the group on timeout still takes them down. That rules out killing
//! a group of our own, so descendants are found through `/proc` instead.

use std::fs;

use rustix::process::{Pid, Signal, kill_process};

/// SIGKILLs `roots` and every process descended from them.
pub fn kill_tree(roots: &[u32]) {
    for pid in descendants(roots, &process_table()) {
        if let Some(pid) = i32::try_from(pid).ok().and_then(Pid::from_raw) {
            let _ = kill_process(pid, Signal::KILL);
        }
    }
}

/// `roots` followed by all their descendants in `table` of `(pid, ppid)`.
fn descendants(roots: &[u32], table: &[(u32, u32)]) -> Vec<u32> {
    let mut found = roots.to_vec();
    let mut next = 0;
    while let Some(&parent) = found.get(next) {
        for &(pid, ppid) in table {
            if ppid == parent && !found.contains(&pid) {
                found.push(pid);
            }
        }
        next += 1;
    }
    found
}

fn process_table() -> Vec<(u32, u32)> {
    let Ok(entries) = fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(|entry| {
            let pid: u32 = entry.ok()?.file_name().to_str()?.parse().ok()?;
            let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            Some((pid, parent_of(&stat)?))
        })
        .collect()
}

/// The ppid field of a `/proc/<pid>/stat` line. The command name before it
/// is in parentheses and may itself contain spaces and parentheses.
fn parent_of(stat: &str) -> Option<u32> {
    let (_, rest) = stat.rsplit_once(')')?;
    rest.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    use super::*;

    #[test]
    fn parses_ppid_after_awkward_names() {
        assert_eq!(parent_of("42 (kwin_wayland) S 7 42 42 0"), Some(7));
        assert_eq!(parent_of("42 (a) b (c)) R 9 1"), Some(9));
        assert_eq!(parent_of("garbage"), None);
    }

    #[test]
    fn walks_every_generation() {
        let table = [(2, 1), (3, 2), (4, 3), (5, 1), (6, 99)];
        assert_eq!(descendants(&[2], &table), [2, 3, 4]);
        assert_eq!(descendants(&[1], &table), [1, 2, 5, 3, 4]);
    }

    #[test]
    fn kills_children_of_children() {
        let mut parent = Command::new("sh")
            .args(["-c", "sleep 600 & echo $!; wait"])
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        std::io::BufRead::read_line(
            &mut std::io::BufReader::new(parent.stdout.take().unwrap()),
            &mut line,
        )
        .unwrap();
        let grandchild: u32 = line.trim().parse().unwrap();
        assert!(descendants(&[parent.id()], &process_table()).contains(&grandchild));

        kill_tree(&[parent.id()]);
        assert!(!parent.wait().unwrap().success());
        let gone = (0..200).any(|_| {
            let alive = fs::read_to_string(format!("/proc/{grandchild}/stat")).is_ok_and(|stat| {
                !stat
                    .rsplit_once(')')
                    .is_some_and(|(_, s)| s.trim_start().starts_with('Z'))
            });
            if alive {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            !alive
        });
        assert!(gone, "sleep {grandchild} survived");
    }
}
