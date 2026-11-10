use crate::state::AppState;
use ratatui::{Frame, layout::Rect, style::Style, widgets::Paragraph};

/// A two-column rail. Counts saturate at '+' so they never wrap into another
/// row. ASCII status markers stay readable without a patched terminal font.
pub(super) fn draw(frame: &mut Frame, state: &AppState) {
    let area = frame.area();
    let (_, running, background, waiting, _, errors) = state.status_counts();
    let count = |n: usize| {
        if n > 9 {
            "+".to_string()
        } else {
            n.to_string()
        }
    };
    for (y, text, color) in [
        (0, ">".to_string(), state.theme.accent),
        (
            2,
            format!("!{}", count(waiting + errors)),
            state.theme.status_waiting,
        ),
        (
            3,
            format!("*{}", count(running + background)),
            state.theme.status_running,
        ),
    ] {
        if y < area.height {
            frame.render_widget(
                Paragraph::new(text).style(Style::default().fg(color)),
                Rect::new(area.x, area.y + y, area.width, 1),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};
    #[test]
    fn collapsed_rail_replaces_full_ui_and_clears_click_targets() {
        let mut state = AppState::new("%test".into());
        state.sidebar_collapsed = true;
        state.layout.repo_button_col = Some(30);
        let mut terminal = Terminal::new(TestBackend::new(2, 6)).unwrap();
        terminal.draw(|f| crate::ui::draw(f, &mut state)).unwrap();
        let output = (0..6)
            .map(|y| {
                format!(
                    "|{}|",
                    (0..2)
                        .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                        .collect::<String>()
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!(output, @r"
        |> |
        |  |
        |!0|
        |*0|
        |  |
        |  |
        ");
        assert!(state.layout.repo_button_col.is_none());
        assert!(state.layout.pane_row_targets.is_empty());
    }
}
