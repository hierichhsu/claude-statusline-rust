use crate::config::theme::ResolvedTheme;
use crate::engine::input::InputData;
use crate::engine::renderer::{ansi_bold, ansi_fg};
use crate::engine::segment::SegmentOutput;
use crate::servers;

pub struct ServersSegment;

impl ServersSegment {
    pub fn render(&self, input: &InputData, theme: &ResolvedTheme) -> Option<SegmentOutput> {
        let root = servers::repo_root(input.cwd.as_ref()?);
        let scan = servers::scan()?;
        let text = format_ports(
            &servers::ports(&scan.mine(&root)),
            servers::ports(&scan.others(&root)).len(),
            theme,
        )?;
        Some(SegmentOutput { text })
    }
}

/// `⚡:3000 :5173 +2`; nothing at all when no dev server runs anywhere.
fn format_ports(mine: &[u16], others: usize, theme: &ResolvedTheme) -> Option<String> {
    if mine.is_empty() && others == 0 {
        return None;
    }
    let mut text = String::new();
    if !mine.is_empty() {
        let list: Vec<String> = mine.iter().map(|p| format!(":{p}")).collect();
        text.push_str(&ansi_bold(
            &format!("\u{26a1}{}", list.join(" ")),
            theme.git_ansi,
        ));
    }
    if others > 0 {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&ansi_fg(&format!("+{others}"), theme.effort_dim_ansi));
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(s: &str) -> String {
        let mut out = String::new();
        let mut esc = false;
        for c in s.chars() {
            match (esc, c) {
                (_, '\x1b') => esc = true,
                (true, 'm') => esc = false,
                (true, _) => {}
                (false, c) => out.push(c),
            }
        }
        out
    }

    #[test]
    fn formats_mine_and_others() {
        let t = ResolvedTheme::morandi();
        assert_eq!(
            plain(&format_ports(&[3000, 5173], 2, &t).unwrap()),
            "\u{26a1}:3000 :5173 +2"
        );
        assert_eq!(plain(&format_ports(&[], 1, &t).unwrap()), "+1");
        assert_eq!(
            plain(&format_ports(&[8787], 0, &t).unwrap()),
            "\u{26a1}:8787"
        );
        assert!(format_ports(&[], 0, &t).is_none());
    }
}
