#[allow(dead_code, unused_imports)]
mod test_helpers;
use test_helpers::*;
use tmux_agent_sidebar::{
    retained::History,
    state::{SessionAction, StatusFilter},
    tmux::{AgentType, PaneStatus, SessionInfo, WindowInfo},
};

#[test]
fn closed_pinned_session_has_pin_and_resume_controls() {
    let dir = tempfile::tempdir().unwrap();
    let mut history = History::new(dir.path().join("history.json"));
    let mut p = make_pane(AgentType::Claude, PaneStatus::Running);
    p.session_id = Some("test-session".into());
    p.path = "/workspace/project".into();
    p.prompt = "Review patch".into();
    let mut sessions = vec![SessionInfo {
        session_name: "test".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "test".into(),
            window_active: true,
            auto_rename: false,
            panes: vec![p.clone()],
        }],
    }];
    history.sync(&mut sessions, 100, 30).unwrap();
    assert!(history.toggle_pin(&p.pane_id, 101).unwrap());
    let mut closed = vec![];
    history.sync(&mut closed, 104, 30).unwrap();
    let mut state = make_state(vec![]);
    state.now = 104;
    state.bottom_panel_height = 0;
    state.repo_groups = tmux_agent_sidebar::group::group_panes_by_repo(&closed);
    state.retained = Some(history);
    state.global.status_filter = StatusFilter::Idle;
    state.rebuild_row_targets();
    let output = render_to_string(&mut state, 32, 15);
    insta::assert_snapshot!(output, @r"
     ≡1  ●0  ◐0  ○1  ✕0
    ⓘ                            — ▾
    project
      ◌ claude                 [P] +
        Review patch
    ");
    let targets = &state.layout.session_actions;
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].action, SessionAction::Pin);
    assert_eq!(targets[1].action, SessionAction::Resume);
    // Clicking a saved row selects it without opening a process. The explicit
    // '+' / n / Enter actions resume it.
    state.handle_mouse_click(4, 4);
    assert_eq!(state.global.selected_pane_row, 0);
    let pin = state.layout.session_actions[0].clone();
    state.handle_mouse_click(pin.rect.y, pin.rect.x);
    assert!(!state.retained_entry(&pin.pane_id).unwrap().pinned);
}
