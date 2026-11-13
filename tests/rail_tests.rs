#[allow(dead_code, unused_imports)]
mod test_helpers;
use test_helpers::*;
use tmux_agent_sidebar::{
    state::StatusFilter,
    tmux::{AgentType, PaneStatus},
};

#[test]
fn collapsed_total_includes_idle_and_retained_sessions_and_keeps_two_digits() {
    let statuses = [
        PaneStatus::Running,
        PaneStatus::Background,
        PaneStatus::Waiting,
        PaneStatus::Error,
        PaneStatus::Idle,
        PaneStatus::Idle,
        PaneStatus::Idle,
        PaneStatus::Idle,
        PaneStatus::Idle,
        PaneStatus::Idle,
        PaneStatus::Idle,
        PaneStatus::Idle,
    ];
    let panes: Vec<_> = statuses
        .into_iter()
        .enumerate()
        .map(|(i, status)| {
            let mut pane = make_pane(AgentType::Codex, status);
            pane.pane_id = if i >= 10 {
                format!("retained:codex:saved-{i}")
            } else {
                format!("%{i}")
            };
            pane
        })
        .collect();
    let mut state = make_state(vec![]);
    state.repo_groups = vec![tmux_agent_sidebar::group::RepoGroup {
        name: "test".into(),
        has_focus: false,
        panes: panes.into_iter().map(|p| (p, Default::default())).collect(),
    }];
    state.sidebar_collapsed = true;
    // Choosing Running must not hide idle/closed sessions from the total.
    state.global.status_filter = StatusFilter::Running;
    let narrow = render_to_string(&mut state, 2, 8);
    insta::assert_snapshot!(narrow, @r"
    >
    ≡
    12
    !2
    *2
    ");
    let wider = render_to_string(&mut state, 3, 8);
    insta::assert_snapshot!(wider, @r"
    >
    ≡12
    !2
    *2
    ");
}
