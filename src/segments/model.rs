use crate::config::theme::ResolvedTheme;
use crate::engine::input::InputData;
use crate::engine::renderer::{ansi_bold, ansi_fg, rgb_fg};
use crate::engine::segment::SegmentOutput;

pub struct ModelSegment;

impl ModelSegment {
    pub fn render(&self, input: &InputData, theme: &ResolvedTheme) -> Option<SegmentOutput> {
        let model = input.model.as_ref()?;
        let name = model.display_name.as_ref().or(model.id.as_ref())?;

        let mut parts = vec![ansi_bold(name, theme.model_ansi)];

        if let Some(level) = input.effort.as_ref().and_then(|e| e.level.as_deref()) {
            parts.push(ansi_fg(&format!("({})", level), theme.effort_dim_ansi));
        }

        if let Some(pct) = input
            .context_window
            .as_ref()
            .and_then(|c| c.used_percentage)
        {
            // Claude Code may send a decimal (docs strip it with `cut -d.`); round, do not reject.
            let pct = pct.round() as u64;
            let colored = rgb_fg(&format!("{}%", pct), ctx_color(pct, theme));
            parts.push(format!("\x1b[1m{}", colored));
        }

        let text = parts.join(" ");
        Some(SegmentOutput { text })
    }
}

/// Context fill color: green under 50%, yellow under 75%, orange under 90%, red past that.
fn ctx_color(pct: u64, theme: &ResolvedTheme) -> &str {
    match pct {
        0..=49 => &theme.ctx_green,
        50..=74 => &theme.ctx_yellow,
        75..=89 => &theme.ctx_orange,
        _ => &theme.ctx_red,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::renderer::rgb_fg_code;

    #[test]
    fn test_model_display_name() {
        let input = InputData::from_json(
            r#"{
            "model": { "id": "claude-opus-4-6", "display_name": "Opus 4.6" }
        }"#,
        )
        .unwrap();
        let theme = ResolvedTheme::morandi();
        let output = ModelSegment.render(&input, &theme).unwrap();
        assert!(output.text.contains("Opus 4.6"));
        assert!(!output.text.contains('('), "no effort shown when absent");
    }

    #[test]
    fn test_model_fallback_to_id() {
        let input = InputData::from_json(r#"{ "model": { "id": "claude-sonnet-4" } }"#).unwrap();
        let theme = ResolvedTheme::morandi();
        let output = ModelSegment.render(&input, &theme).unwrap();
        assert!(output.text.contains("claude-sonnet-4"));
    }

    #[test]
    fn test_model_with_effort_and_context() {
        let input = InputData::from_json(
            r#"{
            "model": { "display_name": "Opus 4.6" },
            "effort": { "level": "high" },
            "context_window": { "used_percentage": 64 }
        }"#,
        )
        .unwrap();
        let theme = ResolvedTheme::morandi();
        let output = ModelSegment.render(&input, &theme).unwrap();
        assert!(output.text.contains("(high)"));
        assert!(output.text.contains("64%"));
        assert!(output.text.contains(&rgb_fg_code(&theme.ctx_yellow)));
    }

    #[test]
    fn test_ctx_color_thresholds() {
        let theme = ResolvedTheme::morandi();
        assert_eq!(ctx_color(0, &theme), theme.ctx_green);
        assert_eq!(ctx_color(49, &theme), theme.ctx_green);
        assert_eq!(ctx_color(50, &theme), theme.ctx_yellow);
        assert_eq!(ctx_color(75, &theme), theme.ctx_orange);
        assert_eq!(ctx_color(90, &theme), theme.ctx_red);
        assert_eq!(ctx_color(100, &theme), theme.ctx_red);
    }

    #[test]
    fn test_decimal_percentage_is_rounded_not_rejected() {
        let input = InputData::from_json(
            r#"{ "model": { "display_name": "M" }, "context_window": { "used_percentage": 23.5 } }"#,
        )
        .unwrap();
        let theme = ResolvedTheme::morandi();
        let output = ModelSegment.render(&input, &theme).unwrap();
        assert!(output.text.contains("24%"));
    }
}
