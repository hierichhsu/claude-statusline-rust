//! Dev servers: which TCP listeners belong to a worktree.
//!
//! Starts from the OS listen table, not from a shell, so a server is seen however
//! it was started (background task, own terminal, orphaned after its shell died).
//! Attribution: a process in the listener's runtime chain has its cwd under the
//! worktree root, or names the root in its command line.

use netstat2::{get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState};
use std::collections::{BTreeSet, HashMap};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// Processes that run dev servers. Only these count toward "other" listeners,
/// so system services do not inflate the number.
const RUNTIMES: &[&str] = &[
    "node", "bun", "deno", "python", "python3", "pythonw", "py", "workerd", "ruby", "php", "java",
];
/// Own ports for containers/WSL: counted as "other", never attributed or killed.
const COUNT_ONLY: &[&str] = &["com.docker.backend", "wslrelay"];

#[derive(Debug, Clone, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub name: String,
    pub cwd: String,
    pub cmd: String,
    pub exe: String,
}

#[derive(Debug, Clone)]
pub struct Listener {
    pub port: u16,
    /// Listener first, then each runtime ancestor (npm -> cmd -> vite ...).
    pub chain: Vec<Proc>,
}

impl Listener {
    pub fn proc(&self) -> &Proc {
        &self.chain[0]
    }
    /// Highest runtime ancestor: killing its tree stops the whole server.
    pub fn top(&self) -> &Proc {
        self.chain.last().unwrap()
    }
}

/// Lowercase, forward slashes, no trailing slash: Windows paths compare case-insensitively.
pub fn norm(p: &str) -> String {
    p.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

fn base_name(name: &str) -> String {
    let n = name.to_lowercase();
    n.strip_suffix(".exe").map(str::to_string).unwrap_or(n)
}

fn is_runtime(name: &str) -> bool {
    RUNTIMES.contains(&name)
}

fn is_claude(p: &Proc) -> bool {
    p.name == "claude" || p.cmd.contains("@anthropic-ai/claude-code")
}

fn under(path: &str, root: &str) -> bool {
    !path.is_empty() && (path == root || path.starts_with(&format!("{root}/")))
}

/// `root` appears in `hay` as a whole path, not as a prefix of a sibling (`repo` vs `repo-b`).
fn mentions(hay: &str, root: &str) -> bool {
    hay.match_indices(root).any(|(i, _)| {
        matches!(
            hay[i + root.len()..].chars().next(),
            None | Some('/' | '"' | '\'' | ' ')
        )
    })
}

/// A worktree root, minus other worktrees nested inside it (Claude Code puts
/// them under `.claude/worktrees/`), so the main checkout does not claim their servers.
#[derive(Debug, Clone)]
pub struct Root {
    pub path: String,
    pub nested: Vec<String>,
}

impl Root {
    fn holds(&self, path: &str) -> bool {
        under(path, &self.path) && !self.nested.iter().any(|n| under(path, n))
    }
    fn named_in(&self, cmd: &str) -> bool {
        mentions(cmd, &self.path) && !self.nested.iter().any(|n| mentions(cmd, n))
    }
}

/// Worktree containing `cwd`, from `git worktree list`; `cwd` itself outside git.
pub fn repo_root(cwd: &str) -> Root {
    // expands 8.3 short names (ERICHS~1) to the long form processes report as their cwd
    let cwd = std::fs::canonicalize(cwd)
        .map(|p| p.to_string_lossy().trim_start_matches(r"\\?\").to_string())
        .unwrap_or_else(|_| cwd.to_string());
    let cwd = norm(&cwd);
    let out = std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(&cwd)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    root_from_worktrees(&cwd, &out)
}

fn root_from_worktrees(cwd: &str, porcelain: &str) -> Root {
    let all: Vec<String> = porcelain
        .lines()
        .filter_map(|l| l.strip_prefix("worktree "))
        .map(norm)
        .collect();
    let path = all
        .iter()
        .filter(|w| under(cwd, w))
        .max_by_key(|w| w.len())
        .cloned()
        .unwrap_or_else(|| cwd.to_string());
    let nested = all
        .into_iter()
        .filter(|w| *w != path && under(w, &path))
        .collect();
    Root { path, nested }
}

pub fn belongs(l: &Listener, root: &Root) -> bool {
    let p = l.proc();
    if COUNT_ONLY.contains(&p.name.as_str()) {
        return false;
    }
    // a non-runtime listener (VS Code, Claude) only counts when its binary was built in the repo
    if !is_runtime(&p.name) && !root.holds(&p.exe) {
        return false;
    }
    l.chain
        .iter()
        .any(|c| root.holds(&c.cwd) || root.named_in(&c.cmd))
}

pub fn is_other_dev(l: &Listener) -> bool {
    let n = l.proc().name.as_str();
    l.port >= 1024 && (is_runtime(n) || COUNT_ONLY.contains(&n))
}

pub fn killable(l: &Listener) -> bool {
    is_runtime(&l.proc().name)
}

pub struct Scan {
    pub listeners: Vec<Listener>,
    sys: System,
}

/// Distinct `(port, pid)` pairs in TCP LISTEN state (v4 and v6 sockets of one server merge).
fn listen_pairs() -> BTreeSet<(u16, u32)> {
    let af = AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6;
    let mut out = BTreeSet::new();
    for s in get_sockets_info(af, ProtocolFlags::TCP).unwrap_or_default() {
        if let ProtocolSocketInfo::Tcp(t) = &s.protocol_socket_info {
            if t.state == TcpState::Listen {
                for pid in &s.associated_pids {
                    out.insert((t.local_port, *pid));
                }
            }
        }
    }
    out
}

pub fn scan() -> Option<Scan> {
    let pairs = listen_pairs();
    let mut sys = System::new().ok()?;
    // names and parents for every process are cheap; cmd/cwd are read only for the chains below
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());

    let name_of = |sys: &System, pid: Pid| {
        sys.process(pid)
            .map(|p| base_name(&p.name().to_string_lossy()))
    };
    let parent_of = |sys: &System, pid: Pid| sys.process(pid).and_then(|p| p.parent());

    // runtime chain per listener pid, plus the first non-runtime ancestor (to spot Claude's MCP children)
    let mut chains: HashMap<u32, (Vec<Pid>, Option<Pid>)> = HashMap::new();
    for &(_, pid) in &pairs {
        chains.entry(pid).or_insert_with(|| {
            let mut chain = vec![Pid::from_u32(pid)];
            let mut stopper = None;
            while chain.len() < 16 {
                let Some(parent) = parent_of(&sys, *chain.last().unwrap()) else {
                    break;
                };
                let pname = name_of(&sys, parent).unwrap_or_default();
                // npm on Windows runs scripts through `cmd /c`; climb it only when a runtime sits above it
                let climb = is_runtime(&pname)
                    || (pname == "cmd"
                        && parent_of(&sys, parent)
                            .and_then(|g| name_of(&sys, g))
                            .is_some_and(|g| is_runtime(&g)));
                if !climb || chain.contains(&parent) {
                    stopper = Some(parent);
                    break;
                }
                chain.push(parent);
            }
            (chain, stopper)
        });
    }

    let want: Vec<Pid> = chains
        .values()
        .flat_map(|(c, s)| c.iter().copied().chain(*s))
        .collect();
    sys.refresh_processes_specifics(
        ProcessesToUpdate::Some(&want),
        false,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_cwd(UpdateKind::Always)
            .with_exe(UpdateKind::Always),
    );

    let to_proc = |pid: Pid| -> Proc {
        let p = sys.process(pid);
        Proc {
            pid: pid.as_u32(),
            name: name_of(&sys, pid).unwrap_or_default(),
            cwd: p
                .and_then(|p| p.cwd())
                .map(|c| norm(&c.to_string_lossy()))
                .unwrap_or_default(),
            cmd: p
                .map(|p| {
                    let parts: Vec<String> = p
                        .cmd()
                        .iter()
                        .map(|a| a.to_string_lossy().into_owned())
                        .collect();
                    norm(&parts.join(" "))
                })
                .unwrap_or_default(),
            exe: p
                .and_then(|p| p.exe())
                .map(|e| norm(&e.to_string_lossy()))
                .unwrap_or_default(),
        }
    };

    let mut listeners = Vec::new();
    for &(port, pid) in &pairs {
        let (chain, stopper) = &chains[&pid];
        let chain: Vec<Proc> = chain.iter().map(|&p| to_proc(p)).collect();
        if chain.iter().any(is_claude) || stopper.map(to_proc).is_some_and(|s| is_claude(&s)) {
            continue; // Claude itself or one of its MCP servers
        }
        listeners.push(Listener { port, chain });
    }
    Some(Scan { listeners, sys })
}

impl Scan {
    pub fn mine(&self, root: &Root) -> Vec<&Listener> {
        self.listeners.iter().filter(|l| belongs(l, root)).collect()
    }

    pub fn others(&self, root: &Root) -> Vec<&Listener> {
        self.listeners
            .iter()
            .filter(|l| !belongs(l, root) && is_other_dev(l))
            .collect()
    }

    /// Kills `top` and everything below it. Returns the size of that tree.
    pub fn kill_tree(&self, top: u32) -> usize {
        let top = Pid::from_u32(top);
        let me = Pid::from_u32(std::process::id());
        let doomed: Vec<_> = self
            .sys
            .processes()
            .iter()
            .filter(|(pid, _)| {
                let mut cur = Some(**pid);
                for _ in 0..64 {
                    match cur {
                        Some(c) if c == me => return false,
                        Some(c) if c == top => return true,
                        Some(c) => cur = self.sys.process(c).and_then(|p| p.parent()),
                        None => return false,
                    }
                }
                false // parent loop from pid reuse
            })
            .map(|(_, p)| p)
            .collect();
        // a parent often exits on its own once its child dies, so its kill() fails; count the tree, not the kills
        for p in &doomed {
            p.kill();
        }
        doomed.len()
    }
}

/// Distinct sorted ports.
pub fn ports(ls: &[&Listener]) -> Vec<u16> {
    let set: BTreeSet<u16> = ls.iter().map(|l| l.port).collect();
    set.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(name: &str, cwd: &str, cmd: &str) -> Proc {
        Proc {
            pid: 1,
            name: name.into(),
            cwd: norm(cwd),
            cmd: norm(cmd),
            exe: String::new(),
        }
    }

    fn listener(port: u16, chain: Vec<Proc>) -> Listener {
        Listener { port, chain }
    }

    fn root() -> Root {
        Root {
            path: "d:/github/app".into(),
            nested: vec!["d:/github/app/.claude/worktrees/wt".into()],
        }
    }

    #[test]
    fn cwd_under_root_belongs() {
        let l = listener(
            3000,
            vec![proc("node", "D:\\Github\\app\\web", "node server.js")],
        );
        assert!(belongs(&l, &root()));
    }

    #[test]
    fn sibling_repo_with_same_prefix_does_not_belong() {
        let l = listener(
            3000,
            vec![proc("node", "D:/Github/app-b", "node D:/Github/app-b/x.js")],
        );
        assert!(!belongs(&l, &root()));
    }

    #[test]
    fn cmdline_path_belongs_when_cwd_is_elsewhere() {
        let l = listener(
            3000,
            vec![proc(
                "node",
                "C:/Users/u",
                "node \"D:/Github/app/server.js\"",
            )],
        );
        assert!(belongs(&l, &root()));
    }

    #[test]
    fn runtime_ancestor_attributes_child() {
        // wrangler (node, cwd in repo) -> workerd (cwd in temp)
        let l = listener(
            8787,
            vec![
                proc("workerd", "C:/Temp", "workerd serve"),
                proc("node", "D:/Github/app", "node wrangler dev"),
            ],
        );
        assert!(belongs(&l, &root()));
    }

    #[test]
    fn non_runtime_needs_exe_in_repo() {
        let mut p = proc("code", "D:/Github/app", "code .");
        let l = listener(5000, vec![p.clone()]);
        assert!(!belongs(&l, &root()));
        p.name = "server".into();
        p.exe = norm("D:/Github/app/target/debug/server.exe");
        assert!(belongs(&listener(5000, vec![p]), &root()));
    }

    #[test]
    fn docker_counts_as_other_but_never_mine_or_killable() {
        let l = listener(5432, vec![proc("com.docker.backend", "D:/Github/app", "")]);
        assert!(!belongs(&l, &root()));
        assert!(is_other_dev(&l));
        assert!(!killable(&l));
    }

    #[test]
    fn system_ports_are_not_other() {
        assert!(!is_other_dev(&listener(135, vec![proc("node", "", "")])));
        assert!(!is_other_dev(&listener(
            7680,
            vec![proc("svchost", "", "")]
        )));
    }

    #[test]
    fn claude_is_detected() {
        assert!(is_claude(&proc("claude", "", "")));
        assert!(is_claude(&proc(
            "node",
            "",
            "node C:/npm/node_modules/@anthropic-ai/claude-code/cli.js"
        )));
        assert!(!is_claude(&proc("node", "", "node vite")));
    }

    #[test]
    fn nested_worktree_server_is_not_the_main_checkouts() {
        let l = listener(
            4000,
            vec![proc(
                "node",
                "D:/Github/app/.claude/worktrees/wt",
                "node vite",
            )],
        );
        assert!(!belongs(&l, &root()));
    }

    #[test]
    fn root_picks_deepest_worktree_and_lists_nested() {
        let out = "worktree D:/Github/app\nHEAD a\nbranch main\n\nworktree D:/Github/app/.claude/worktrees/wt\nHEAD b\n\nworktree D:/Github/other\n";
        let r = root_from_worktrees("d:/github/app/src", out);
        assert_eq!(r.path, "d:/github/app");
        assert_eq!(r.nested, vec!["d:/github/app/.claude/worktrees/wt"]);
        let r = root_from_worktrees("d:/github/app/.claude/worktrees/wt/x", out);
        assert_eq!(r.path, "d:/github/app/.claude/worktrees/wt");
        assert!(r.nested.is_empty());
        assert_eq!(root_from_worktrees("c:/tmp", "").path, "c:/tmp");
    }

    #[test]
    fn live_listener_is_found_and_attributed() {
        // the test binary lives in target/ under the crate root, so it counts as a repo-built server
        let sock = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = sock.local_addr().unwrap().port();
        let root = repo_root(env!("CARGO_MANIFEST_DIR"));
        let scan = scan().unwrap();
        assert!(
            ports(&scan.mine(&root)).contains(&port),
            "port {port} not attributed to {root:?}"
        );
        assert!(!ports(&scan.others(&root)).contains(&port));
    }
}
