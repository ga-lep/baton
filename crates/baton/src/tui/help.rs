//! The `?` help overlay: the live key bindings of both modes.

use baton_core::keymap::Keymap;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

/// Width of the action-name column.
const NAME_WIDTH: usize = 16;

fn column(rows: Vec<(String, String)>) -> Vec<String> {
    rows.into_iter()
        .map(|(name, keys)| format!("{name:<NAME_WIDTH$}{keys}"))
        .collect()
}

/// `action  keys` lines for normal mode and focus mode, from the live keymap.
pub fn columns(keymap: &Keymap) -> (Vec<String>, Vec<String>) {
    (
        column(keymap.describe_normal()),
        column(keymap.describe_focus()),
    )
}

/// Draws the overlay over the whole frame.
pub fn draw(f: &mut Frame, keymap: &Keymap) {
    let (normal, focus) = columns(keymap);
    let area = f.area();
    let rows = u16::try_from(normal.len().max(focus.len()) + 3).unwrap_or(u16::MAX);
    let width = 100.min(area.width);
    let height = rows.saturating_add(2).min(area.height);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .title("Key bindings (? or Esc to close)");
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(inner);
    for (area, title, lines) in [(left, "Normal mode", normal), (right, "Focus mode", focus)] {
        let mut text = vec![
            Line::styled(title, Style::default().add_modifier(Modifier::BOLD)),
            Line::default(),
        ];
        text.extend(lines.into_iter().map(Line::from));
        f.render_widget(Paragraph::new(text), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use baton_core::keymap::{Keymap, Overrides};

    fn remapped() -> Keymap {
        let mut o = Overrides::new();
        o.entry("normal".into())
            .or_default()
            .insert("next_attention".into(), vec!["x".into()]);
        o.entry("focus".into())
            .or_default()
            .insert("unfocus".into(), vec!["ctrl-g".into()]);
        Keymap::from_overrides(&o).expect("valid")
    }

    #[test]
    fn lists_the_active_bindings_of_both_modes() {
        let (normal, focus) = columns(&remapped());
        assert!(
            normal.iter().any(|l| l == "next_attention  x"),
            "{normal:?}"
        );
        assert!(
            normal.iter().any(|l| l == "move_down       j, down"),
            "{normal:?}"
        );
        assert!(
            normal.iter().any(|l| l == "quit            q"),
            "{normal:?}"
        );
        assert!(
            focus.iter().any(|l| l == "unfocus         ctrl-g"),
            "{focus:?}"
        );
        assert!(
            focus.iter().any(|l| l == "session_3       alt-3"),
            "{focus:?}"
        );
        assert!(
            !focus.iter().any(|l| l.contains("ctrl-\\")),
            "old key is gone"
        );
    }

    #[test]
    fn default_help_mentions_every_action_once() {
        let (normal, focus) = columns(&Keymap::default());
        assert_eq!(
            normal.len(),
            16 + 8,
            "16 actions with select_1..9: {normal:?}"
        );
        assert_eq!(focus.len(), 2 + 9);
    }
}
