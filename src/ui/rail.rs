use crate::state::AppState;
use ratatui::{Frame, layout::Rect, style::Style, widgets::Paragraph};

/// Compact status rail. The total stays exact; narrow rails put multi-digit
/// totals below the All icon. Activity counters still saturate at '+'.
pub(super) fn draw(frame: &mut Frame, state: &AppState) {
    let area = frame.area();
    if area.width == 0 {
        return;
    }
    let (all, running, background, waiting, _, errors) = state.status_counts();
    let count = |n: usize| {
        if n > 9 {
            "+".to_string()
        } else {
            n.to_string()
        }
    };
    let mut rows = vec![(">".to_string(), state.theme.accent)];
    let total = format!("{}{all}", state.icons.all_icon());
    if crate::ui::text::display_width(&total) <= area.width as usize {
        rows.push((total, state.theme.status_all));
    } else {
        rows.push((state.icons.all_icon().to_string(), state.theme.status_all));
        for digits in all.to_string().as_bytes().chunks(area.width as usize) {
            rows.push((
                String::from_utf8_lossy(digits).into_owned(),
                state.theme.status_all,
            ));
        }
    }
    rows.extend([
        (
            format!("!{}", count(waiting + errors)),
            state.theme.status_waiting,
        ),
        (
            format!("*{}", count(running + background)),
            state.theme.status_running,
        ),
    ]);
    for (y, (text, color)) in rows.into_iter().enumerate().take(area.height as usize) {
        frame.render_widget(
            Paragraph::new(text).style(Style::default().fg(color)),
            Rect::new(area.x, area.y + y as u16, area.width, 1),
        );
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
        |≡0|
        |!0|
        |*0|
        |  |
        |  |
        ");
        assert!(state.layout.repo_button_col.is_none());
        assert!(state.layout.pane_row_targets.is_empty());
    }
}
