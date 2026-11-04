mod cli;
mod config;
mod engine;
mod segments;
mod servers;
mod utils;

use cli::{Cli, Command};
use config::theme::ResolvedTheme;
use config::Config;
use engine::input::InputData;
use engine::renderer::{join_segments, RESET};
use std::io::{self, Read};

fn main() {
    let cli = Cli::parse_args();
    if let Some(Command::Servers {
        kill,
        all,
        mut cwd,
        hook,
        orphans,
    }) = cli.command
    {
        if orphans {
            kill_orphans();
            return;
        }
        let mut session = None;
        if hook {
            let mut raw = String::new();
            io::stdin().read_to_string(&mut raw).unwrap_or(0);
            cwd = InputData::from_json(&raw).ok().and_then(|i| i.cwd);
            if cwd.is_none() {
                // killing by a guessed directory could hit another repo's servers
                eprintln!("--hook: no cwd in stdin JSON, nothing done");
                return;
            }
            // only this session's own servers: the user's terminal and other sessions share the worktree
            session = servers::session_pid();
            if session.is_none() {
                eprintln!("--hook: not running under a Claude session, nothing done");
                return;
            }
        }
        servers_command(kill, all, cwd, session);
        return;
    }

    let mut input_str = String::new();
    io::stdin().read_to_string(&mut input_str).unwrap_or(0);

    if input_str.is_empty() {
        eprintln!("No stdin data. Claude Code pipes JSON via stdin.");
        std::process::exit(1);
    }

    let input = match InputData::from_json(&input_str) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Failed to parse stdin JSON: {}", e);
            std::process::exit(1);
        }
    };

    let config = Config::load();
    let theme = ResolvedTheme::from_config(&config.theme.name, &config.theme.colors);

    // Collect line-1 segments
    let mut line1_parts: Vec<String> = Vec::new();
    let enabled = &config.segments.enabled;

    if enabled.contains(&"directory".into()) {
        if let Some(out) = segments::directory::DirectorySegment.render(&input, &theme) {
            line1_parts.push(out.text);
        }
    }
    // git and servers each spawn git (~60ms on Windows); run them side by side
    let (git_out, servers_out) = std::thread::scope(|s| {
        let servers = s.spawn(|| {
            enabled
                .contains(&"servers".into())
                .then(|| segments::servers::ServersSegment.render(&input, &theme))
                .flatten()
        });
        let git = enabled
            .contains(&"git".into())
            .then(|| segments::git::GitSegment.render(&input, &theme, &config.git))
            .flatten();
        (git, servers.join().ok().flatten())
    });
    line1_parts.extend(git_out.map(|o| o.text));
    line1_parts.extend(servers_out.map(|o| o.text));
    if enabled.contains(&"model".into()) {
        if let Some(out) = segments::model::ModelSegment.render(&input, &theme) {
            line1_parts.push(out.text);
        }
    }
    if enabled.contains(&"thinking".into()) {
        if let Some(out) = segments::thinking::ThinkingSegment.render(&input, &theme) {
            line1_parts.push(out.text);
        }
    }

    // Line 1
    let line1 = join_segments(
        &line1_parts,
        &config.general.separator,
        theme.separator_ansi,
    );
    println!("{}{}", line1, RESET);

    // Lines 2-3: Usage
    if enabled.contains(&"usage".into()) {
        let usage_seg = segments::usage::UsageSegment {
            cache_ttl_secs: config.usage.cache_ttl_secs,
        };
        for out in usage_seg.render(&input, &theme) {
            println!("{}{}", out.text, RESET);
        }
    }
}

/// `servers --kill --orphans`: silent unless it kills something, since a
/// SessionStart hook's stdout lands in Claude's context.
fn kill_orphans() {
    let Some(scan) = servers::scan() else {
        return;
    };
    let mut done: Vec<u32> = Vec::new();
    for l in scan.orphans() {
        let top = l.top();
        if done.contains(&top.pid) {
            continue;
        }
        done.push(top.pid);
        let n = scan.kill_tree(top.pid);
        println!(
            "killed orphaned dev server :{} (pid {}, {} process(es)) from {}",
            l.port, top.pid, n, top.cwd
        );
    }
}

/// `servers`: list, or kill, the dev servers attributed to a worktree;
/// with `session`, kill only those that Claude session started.
fn servers_command(kill: bool, all: bool, cwd: Option<String>, session: Option<u32>) {
    let cwd = cwd.unwrap_or_else(|| {
        std::env::current_dir()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let root = servers::repo_root(&cwd);
    let Some(scan) = servers::scan() else {
        eprintln!("cannot read the process list");
        std::process::exit(1);
    };
    let mine = scan.mine(&root);
    let others = scan.others(&root);

    let line = |l: &servers::Listener| {
        let p = l.proc();
        let top = l.top();
        let cmd: String = top.cmd.chars().take(80).collect();
        let mark = if l.orphan { " (orphan)" } else { "" };
        format!(
            ":{:<6} pid {:<7} {:<8} {}{}\n           {}",
            l.port, p.pid, p.name, top.cwd, mark, cmd
        )
    };
    println!(
        "{}  ({} server{})",
        root.path,
        mine.len(),
        if mine.len() == 1 { "" } else { "s" }
    );
    for l in &mine {
        println!("  {}", line(l));
    }
    if !others.is_empty() {
        println!("other dev servers ({})", others.len());
        for l in &others {
            println!("  {}", line(l));
        }
    }
    if !kill {
        return;
    }

    let mut targets: Vec<&servers::Listener> = mine;
    if all {
        targets.extend(others.into_iter().filter(|l| servers::killable(l)));
    }
    if let Some(s) = session {
        targets = scan.started_by(targets, s);
    }
    // one kill per tree: several ports can share a top process
    let mut tops: Vec<u32> = targets.iter().map(|l| l.top().pid).collect();
    tops.sort_unstable();
    tops.dedup();
    for top in tops {
        println!("killed tree {}: {} process(es)", top, scan.kill_tree(top));
    }
}
