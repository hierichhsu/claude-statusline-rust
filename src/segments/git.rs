use crate::config::theme::ResolvedTheme;
use crate::config::GitConfig;
use crate::engine::input::InputData;
use crate::engine::renderer::ansi_bold;
use crate::engine::segment::SegmentOutput;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

pub struct GitSegment;

impl GitSegment {
    pub fn render(
        &self,
        input: &InputData,
        theme: &ResolvedTheme,
        git_cfg: &GitConfig,
    ) -> Option<SegmentOutput> {
        let cwd = input.cwd.as_ref()?;
        let status = git_status(cwd)?;

        let mut label = format!("\u{e725} {}", status.branch);
        if status.changed > 0 {
            label.push_str(&format!("*{}", status.changed));
        }
        if status.ahead > 0 {
            label.push_str(&format!(" \u{2191}{}", status.ahead));
        }
        if status.behind > 0 {
            label.push_str(&format!(" \u{2193}{}", status.behind));
        }

        // Worktree name (only when in a linked worktree).
        if git_cfg.show_worktree {
            if let Some(wt) = worktree_name(cwd) {
                label.push_str(&format!(" \u{2387}{}", wt));
            }
        }

        // Dev server port from convention file at repo/worktree root.
        if !git_cfg.port_file.is_empty() {
            if let Some(port) = read_port(cwd, &git_cfg.port_file) {
                let mark = if port_alive(port) { "" } else { "\u{2717}" };
                label.push_str(&format!(" :{}{}", port, mark));
            }
        }

        let text = ansi_bold(&label, theme.git_ansi);
        Some(SegmentOutput { text })
    }
}

#[derive(Debug, PartialEq)]
struct GitStatus {
    branch: String,
    ahead: u32,
    behind: u32,
    changed: usize,
}

/// One `git status --porcelain=v2 --branch` call: branch, ahead/behind, changed entries.
fn git_status(cwd: &str) -> Option<GitStatus> {
    let output = Command::new("git")
        .args(["status", "--porcelain=v2", "--branch"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_porcelain_v2(&String::from_utf8_lossy(&output.stdout))
}

fn parse_porcelain_v2(out: &str) -> Option<GitStatus> {
    let mut branch: Option<String> = None;
    let mut oid = "";
    let mut ahead = 0;
    let mut behind = 0;
    let mut changed = 0;

    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            branch = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.oid ") {
            oid = rest;
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            // "+A -B"
            let mut it = rest.split_whitespace();
            ahead = it.next().and_then(|s| s[1..].parse().ok()).unwrap_or(0);
            behind = it.next().and_then(|s| s[1..].parse().ok()).unwrap_or(0);
        } else if matches!(line.chars().next(), Some('1' | '2' | 'u' | '?')) {
            changed += 1;
        }
    }

    let branch = match branch?.as_str() {
        "(detached)" => format!("@{}", &oid[..oid.len().min(7)]),
        b => b.to_string(),
    };
    Some(GitStatus {
        branch,
        ahead,
        behind,
        changed,
    })
}

/// Returns the worktree name when cwd is a linked worktree, else None.
/// In a linked worktree `git rev-parse --git-dir` points at
/// `.../.git/worktrees/<name>`; the main worktree does not.
fn worktree_name(cwd: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let git_dir = String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .replace('\\', "/");
    let name = git_dir.rsplit_once("/worktrees/")?.1;
    // `name` is the trailing path component after `/worktrees/`.
    let name = name.split('/').next()?.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Reads the dev port from `<repo-root>/<port_file>` (trimmed first line).
fn read_port(cwd: &str, port_file: &str) -> Option<u16> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8(output.stdout).ok()?.trim().to_string();
    let content = std::fs::read_to_string(Path::new(&root).join(port_file)).ok()?;
    // parsing as u16 rejects anything a stray file could use to inject escape codes
    content.lines().next()?.trim().parse().ok()
}

/// True when something accepts a TCP connection on the port, over IPv4 or IPv6
/// (Node 17+ binds `localhost` to ::1 on Windows).
fn port_alive(port: u16) -> bool {
    let timeout = Duration::from_millis(50);
    let addrs: [SocketAddr; 2] = [
        (Ipv4Addr::LOCALHOST, port).into(),
        (Ipv6Addr::LOCALHOST, port).into(),
    ];
    addrs
        .iter()
        .any(|addr| TcpStream::connect_timeout(addr, timeout).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_clean_tracking_branch() {
        let out = "# branch.oid abcdef1234567890\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +0 -0\n";
        let s = parse_porcelain_v2(out).unwrap();
        assert_eq!(
            s,
            GitStatus {
                branch: "main".into(),
                ahead: 0,
                behind: 0,
                changed: 0
            }
        );
    }

    #[test]
    fn test_parse_ahead_behind_and_changes() {
        let out = "# branch.oid abcdef1234567890\n# branch.head feat\n# branch.upstream origin/feat\n# branch.ab +2 -1\n1 .M N... 100644 100644 100644 aaa bbb src/a.rs\n2 R. N... 100644 100644 100644 aaa bbb R100 new.rs\told.rs\nu UU N... 100644 100644 100644 100644 a b c d conflict.rs\n? untracked.txt\n";
        let s = parse_porcelain_v2(out).unwrap();
        assert_eq!(s.branch, "feat");
        assert_eq!((s.ahead, s.behind), (2, 1));
        assert_eq!(s.changed, 4);
    }

    #[test]
    fn test_parse_no_upstream() {
        let out = "# branch.oid abcdef1234567890\n# branch.head local-only\n? x\n";
        let s = parse_porcelain_v2(out).unwrap();
        assert_eq!(s.branch, "local-only");
        assert_eq!((s.ahead, s.behind, s.changed), (0, 0, 1));
    }

    #[test]
    fn test_parse_detached_head() {
        let out = "# branch.oid abcdef1234567890\n# branch.head (detached)\n";
        let s = parse_porcelain_v2(out).unwrap();
        assert_eq!(s.branch, "@abcdef1");
    }

    #[test]
    fn test_parse_ignores_ignored_entries() {
        let out = "# branch.oid abc\n# branch.head main\n! target/\n";
        assert_eq!(parse_porcelain_v2(out).unwrap().changed, 0);
    }

    #[test]
    fn test_port_alive_detects_listener() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(port_alive(port));
        drop(listener);
        assert!(!port_alive(port));
    }
}
