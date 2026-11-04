# claude-statusline-rs

A fast, segment-based statusline for Claude Code, written in Rust.

![screenshot](assets/screenshot.png)

## Install

### One-line install

**macOS / Linux / Git Bash:**
```bash
curl -fsSL https://raw.githubusercontent.com/hierichhsu/claude-statusline-rust/main/install.sh | bash
```

**Windows (PowerShell):** open Git Bash first, then run the command above. Or:
```powershell
& "C:\Program Files\Git\bin\bash.exe" -c "curl -fsSL https://raw.githubusercontent.com/hierichhsu/claude-statusline-rust/main/install.sh | bash"
```

### cargo install

```bash
cargo install claude-statusline-rs
```

Then manually add to `~/.claude/settings.json`:

```json
{
  "statusLine": {
    "type": "command",
    "command": "claude-statusline-rs"
  }
}
```

### Install from Claude Code

In your Claude Code session, just ask:

> Install claude-statusline-rs by running: `curl -fsSL https://raw.githubusercontent.com/hierichhsu/claude-statusline-rust/main/install.sh | bash`

## Uninstall

```bash
curl -fsSL https://raw.githubusercontent.com/hierichhsu/claude-statusline-rust/main/install.sh | bash -s uninstall
```

Or from Claude Code, ask:

> Uninstall claude-statusline-rs by running: `curl -fsSL https://raw.githubusercontent.com/hierichhsu/claude-statusline-rust/main/install.sh | bash -s uninstall`

## Display

```
📁 project │ 🌿 main*3 ↑1 ⎇feat :3000 │ 🤖 Opus 4.6 (high) 32% │ ● thinking
current  ●●●○○○○○○○  28%  ↻ 7:00pm
weekly   ●●●●●●●●○○  79%  ↻ mar 10, 10:00am
```

### Segments

| Segment | Description |
|---------|-------------|
| Directory | Current folder name |
| Git | Branch, changed-file count (`*3`), ahead/behind upstream (`↑2 ↓1`), and worktree name (`⎇name`) in linked worktrees |
| Servers | Ports this worktree's dev servers listen on (`⚡:3000 :5173`), plus how many other dev servers run on the machine (`+2`). Hidden when none run |
| Model | Current Claude model, effort level (`(high)`), and context usage % colored green / yellow / orange / red at 50 / 75 / 90 |
| Thinking | Green dot when thinking, gray when not |
| Usage | 5-hour and 7-day rate limit bars (OAuth API) |

### Colors (Morandi Palette)

Muted, sophisticated tones: dusty blue, sage green, warm sand, amber, dusty rose, mauve.

## Configure

Edit `~/.claude/statusline-rs/config.toml`:

```toml
[general]
separator = " │ "

[segments]
enabled = ["directory", "git", "servers", "model", "thinking", "usage"]

[git]
show_worktree = true   # append ⎇<name> when inside a linked git worktree

[theme]
name = "morandi"

[usage]
cache_ttl_secs = 60
```

### Dev servers

Nothing to set up: the segment reads the OS TCP listen table, so a server shows up however it was started (background task, your own terminal, an orphan whose shell is gone). A listener belongs to this worktree when it, or a runtime parent above it (npm → cmd → vite, wrangler → workerd), has its cwd inside the worktree or names the worktree path in its command line. Worktrees nested inside it (`.claude/worktrees/…`) keep their own servers. Claude Code and its MCP servers are left out.

"Other" counts dev runtimes only (node, bun, deno, python, workerd, ruby, php, java, Docker, WSL) on ports ≥ 1024, so system services do not inflate it. Docker/WSL ports, and processes started as administrator, cannot be tied to a repo and only appear there.

```bash
claude-statusline-rs servers               # list this worktree's servers and the others, with cwd and command
claude-statusline-rs servers --kill        # kill this worktree's server trees
claude-statusline-rs servers --kill --all  # also kill other repos' dev servers (never Docker/WSL)
claude-statusline-rs servers --kill --orphans  # kill orphaned dev servers anywhere; silent when there are none
claude-statusline-rs servers --cwd <path>  # inspect another worktree
```

An **orphan** (Windows only) is a dev server whose ancestry breaks before it reaches a long-lived root (Explorer, a service host, or Claude): the shell, terminal or Claude session that started it is gone, for example after a crash or a closed terminal. The listing marks these `(orphan)`. Servers that a live session or an open terminal started are left alone. A server deliberately detached with `start` from a shell that has since closed also counts as an orphan.

Two hooks clean up automatically. SessionEnd kills the ending session's own servers: those in its worktree whose process tree descends from that Claude process. Servers in the same worktree started from your own terminal or by another session are left alone. `--hook` takes the worktree from the hook's stdin JSON and does nothing without it, or when it is not running under a Claude session. SessionStart sweeps the orphans that sessions which did not end cleanly left behind:

```json
"SessionStart": [{ "hooks": [{ "type": "command", "command": "\"C:/Users/<you>/.claude/bin/claude-statusline-rs.exe\" servers --kill --orphans", "timeout": 15 }] }],
"SessionEnd": [{ "hooks": [{ "type": "command", "command": "\"C:/Users/<you>/.claude/bin/claude-statusline-rs.exe\" servers --kill --hook", "timeout": 15 }] }]
```

### Custom Colors

Override any theme color:

```toml
[theme]
name = "morandi"

[theme.colors]
directory = "#FF6B6B"
model = "#4ECDC4"
```

## License

MIT
