pub fn separator() -> String {
    " │ ".into()
}

pub fn enabled_segments() -> Vec<String> {
    vec![
        "directory".into(),
        "git".into(),
        "servers".into(),
        "model".into(),
        "thinking".into(),
        "usage".into(),
    ]
}

pub fn git_show_worktree() -> bool {
    true
}

pub fn theme_name() -> String {
    "morandi".into()
}

pub fn cache_ttl() -> u64 {
    60
}
