use super::*;
fn pane(id: &str, sid: Option<&str>) -> PaneInfo {
    Entry {
        key: "".into(),
        agent: "codex".into(),
        session_id: sid.map(str::to_string),
        cwd: "/repo".into(),
        title: "task | repo".into(),
        pinned: false,
        last_seen: 0,
        closed_at: None,
        live_pane: id.into(),
        live_pid: Some(42),
        pending_until: 0,
    }
    .ghost_with_live_id(id)
}
impl Entry {
    fn ghost_with_live_id(&self, id: &str) -> PaneInfo {
        let mut p = self.ghost();
        p.pane_id = id.into();
        p.pane_pid = Some(42);
        p.status = PaneStatus::Running;
        p
    }
}
#[test]
fn closed_session_becomes_idle_then_expires_at_configured_deadline() {
    let mut db = BTreeMap::new();
    reconcile(&mut db, &[pane("%1", Some("abc"))], 100, 30);
    reconcile(&mut db, &[], 102, 30);
    assert!(db["codex:abc"].closed_at.is_none());
    reconcile(&mut db, &[], 103, 30);
    assert_eq!(db["codex:abc"].closed_at, Some(103));
    assert_eq!(db["codex:abc"].ghost().status, PaneStatus::Idle);
    reconcile(&mut db, &[], 1902, 30);
    assert_eq!(db.len(), 1);
    reconcile(&mut db, &[], 1903, 30);
    assert!(db.is_empty());
}
#[test]
fn pins_survive_expiration_and_reconnection_preserves_identity() {
    let mut db = BTreeMap::new();
    reconcile(&mut db, &[pane("%1", Some("abc"))], 100, 0);
    db.get_mut("codex:abc").unwrap().pinned = true;
    reconcile(&mut db, &[], 10000, 0);
    assert_eq!(db.len(), 1);
    reconcile(&mut db, &[pane("%9", Some("abc"))], 10001, 0);
    assert!(db["codex:abc"].pinned);
    assert!(db["codex:abc"].closed_at.is_none());
    assert_eq!(db.len(), 1);
}
#[test]
fn unresolved_identity_upgrade_keeps_pin_and_no_duplicate() {
    let mut db = BTreeMap::new();
    let p = pane("%1", None);
    reconcile(&mut db, std::slice::from_ref(&p), 100, 30);
    db.values_mut().next().unwrap().pinned = true;
    let mut identified = p;
    identified.session_id = Some("abc".into());
    reconcile(&mut db, &[identified], 101, 30);
    assert_eq!(db.len(), 1);
    assert!(db["codex:abc"].pinned);
}
#[test]
fn different_sessions_in_same_pane_do_not_replace_saved_conversation() {
    let mut db = BTreeMap::new();
    reconcile(&mut db, &[pane("%1", Some("abc"))], 100, 30);
    reconcile(&mut db, &[pane("%1", Some("def"))], 104, 30);
    assert_eq!(db.len(), 2);
    assert_eq!(db["codex:abc"].closed_at, Some(103));
    assert!(db["codex:def"].closed_at.is_none());
}
#[test]
fn persistence_and_concurrent_instances_preserve_pin() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("records.json");
    store::transaction(&path, |db| {
        reconcile(db, &[pane("%1", Some("abc"))], 100, 30);
        Ok(())
    })
    .unwrap();
    let mut a = History::new(path.clone());
    a.reload().unwrap();
    assert!(a.toggle_pin("retained:codex:abc", 101).unwrap());
    store::transaction(&path, |db| {
        reconcile(db, &[], 10000, 30);
        Ok(())
    })
    .unwrap();
    let mut b = History::new(path);
    b.reload().unwrap();
    assert!(b.entries["codex:abc"].pinned);
    assert!(!b.toggle_pin("retained:codex:abc", 10001).unwrap());
    assert_eq!(b.entries["codex:abc"].closed_at, Some(10001));
}
#[test]
fn corrupt_cache_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("records.json");
    std::fs::write(&path, "broken").unwrap();
    assert!(History::new(path.clone()).reload().is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "broken");
}
#[test]
fn resume_is_exact_and_rejects_unknown_identity_or_option_injection() {
    let mut db = BTreeMap::new();
    reconcile(&mut db, &[pane("%1", Some("abc-def"))], 100, 30);
    let mut e = db["codex:abc-def"].clone();
    assert_eq!(resume_argv(&e).unwrap(), vec!["codex", "resume", "abc-def"]);
    e.agent = "claude".into();
    assert_eq!(
        resume_argv(&e).unwrap(),
        vec!["claude", "--resume", "abc-def"]
    );
    e.agent = "opencode".into();
    assert_eq!(
        resume_argv(&e).unwrap(),
        vec!["opencode", "--session", "abc-def"]
    );
    e.session_id = None;
    assert!(resume_argv(&e).is_err());
    e.session_id = Some("--last".into());
    assert!(resume_argv(&e).is_err());
    e.session_id = Some("x; touch /tmp/not-allowed".into());
    assert!(resume_argv(&e).is_err());
}

#[test]
fn resumed_client_can_exit_again_before_identity_bridge_expires() {
    let mut db = BTreeMap::new();
    reconcile(&mut db, &[pane("%1", Some("abc"))], 100, 30);
    let e = db.get_mut("codex:abc").unwrap();
    e.pending_until = 120;
    e.closed_at = Some(103);
    reconcile(&mut db, &[pane("%1", None)], 105, 30);
    assert_eq!(db.len(), 1);
    assert!(db["codex:abc"].closed_at.is_none());
    reconcile(&mut db, &[], 108, 30);
    assert_eq!(db["codex:abc"].closed_at, Some(108));
    assert_eq!(db["codex:abc"].pending_until, 0);
}

fn snapshot(panes: Vec<PaneInfo>) -> Vec<SessionInfo> {
    vec![SessionInfo {
        session_name: "test".into(),
        windows: vec![WindowInfo {
            window_id: "@1".into(),
            window_name: "test".into(),
            window_active: true,
            auto_rename: false,
            panes,
        }],
    }]
}

fn visible(sessions: &[SessionInfo]) -> Vec<&PaneInfo> {
    sessions
        .iter()
        .flat_map(|s| &s.windows)
        .flat_map(|w| &w.panes)
        .collect()
}

#[test]
fn missing_session_stays_visible_during_grace_and_reconnects_without_duplicate() {
    let dir = tempfile::tempdir().unwrap();
    let mut history = History::new(dir.path().join("history.json"));
    let p = pane("%1", Some("abc"));
    let mut live = snapshot(vec![p.clone()]);
    history.sync(&mut live, 100, 30).unwrap();
    let key = identity(&p);
    for now in 101..=103 {
        let mut missing = vec![];
        history.sync(&mut missing, now, 30).unwrap();
        let rows = visible(&missing);
        assert_eq!(rows.len(), 1, "the row must never disappear during exit");
        assert_eq!(identity(rows[0]), key);
        assert_eq!(is_retained(&rows[0].pane_id), now == 103);
        if now < 103 {
            assert_eq!(rows[0].status, PaneStatus::Running);
            assert_eq!(history.record("%1").unwrap().key, key);
        } else {
            assert_eq!(rows[0].status, PaneStatus::Idle);
        }
    }
    let mut resumed = snapshot(vec![pane("%9", Some("abc"))]);
    history.sync(&mut resumed, 104, 30).unwrap();
    assert_eq!(visible(&resumed).len(), 1);
    assert_eq!(identity(visible(&resumed)[0]), key);
    assert!(!is_retained(&visible(&resumed)[0].pane_id));
}

#[test]
fn brief_discovery_gap_keeps_the_same_live_row() {
    let dir = tempfile::tempdir().unwrap();
    let mut history = History::new(dir.path().join("history.json"));
    let p = pane("%1", Some("abc"));
    for (now, available) in [(100, true), (101, false), (102, true)] {
        let mut sessions = snapshot(if available { vec![p.clone()] } else { vec![] });
        history.sync(&mut sessions, now, 30).unwrap();
        assert_eq!(visible(&sessions).len(), 1);
        assert_eq!(visible(&sessions)[0].pane_id, "%1");
        assert!(history.entries["codex:abc"].closed_at.is_none());
    }
}

#[test]
fn pane_reuse_during_grace_does_not_create_duplicate_click_targets() {
    let dir = tempfile::tempdir().unwrap();
    let mut history = History::new(dir.path().join("history.json"));
    history
        .sync(&mut snapshot(vec![pane("%1", Some("abc"))]), 100, 30)
        .unwrap();
    let mut sessions = snapshot(vec![pane("%1", Some("def"))]);
    history.sync(&mut sessions, 101, 30).unwrap();
    let rows = visible(&sessions);
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].pane_id, rows[1].pane_id);
    assert_eq!(history.entries["codex:abc"].closed_at, Some(101));
}
