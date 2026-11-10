#[allow(dead_code, unused_imports)]
mod test_helpers;
use test_helpers::*;
use tmux_agent_sidebar::{
    retained::History,
    state::{SessionAction, StatusFilter},
    tmux::{AgentType, PaneStatus, SessionInfo, WindowInfo},
};

#[test]
fn closed_git_session_reuses_group_spawn_button() {
    check_closed_session_controls(true);
}

#[test]
fn closed_non_git_session_has_group_resume_button() {
    check_closed_session_controls(false);
}

fn check_closed_session_controls(git_repo: bool) {
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
    if git_repo {
        state.repo_groups[0].panes[0].1.repo_root = Some("/workspace/project".into());
    }
    state.retained = Some(history);
    state.global.status_filter = StatusFilter::Idle;
    state.rebuild_row_targets();
    let output = render_to_string(&mut state, 32, 15);
    insta::allow_duplicates! {
        insta::assert_snapshot!(output, @r"
     ≡1  ●0  ◐0  ○1  ✕0
    ⓘ                            — ▾
    project                        +
      ◌ claude                   [P]
        Review patch
    ");
    }
    let pins: Vec<_> = state
        .layout
        .session_actions
        .iter()
        .filter(|t| t.action == SessionAction::Pin)
        .cloned()
        .collect();
    assert_eq!(pins.len(), 1);
    let resume_rect = if git_repo {
        assert_eq!(state.layout.session_actions.len(), 1);
        assert_eq!(state.layout.repo_spawn_targets.len(), 1);
        state.layout.repo_spawn_targets[0].rect
    } else {
        assert!(state.layout.repo_spawn_targets.is_empty());
        assert_eq!(state.layout.session_actions.len(), 2);
        state
            .layout
            .session_actions
            .iter()
            .find(|t| t.action == SessionAction::Resume)
            .unwrap()
            .rect
    };
    assert_eq!(resume_rect.y, 2, "resume belongs to the group header");
    assert_eq!(pins[0].rect.y, 3, "the session row only has pin");
    // Clicking a saved row selects it without opening a process. The explicit
    // '+' / n / Enter actions resume it.
    state.handle_mouse_click(4, 4);
    assert_eq!(state.global.selected_pane_row, 0);
    let pin = pins[0].clone();
    state.handle_mouse_click(pin.rect.y, pin.rect.x);
    assert!(!state.retained_entry(&pin.pane_id).unwrap().pinned);
    // Verify routing without launching tmux or an agent from a UI snapshot test.
    state.retained = None;
    state.handle_mouse_click(resume_rect.y, resume_rect.x);
    assert!(!state.is_spawn_input_open());
    assert_eq!(
        state.flash.as_ref().unwrap().0,
        "resume: session cache unavailable"
    );
}
