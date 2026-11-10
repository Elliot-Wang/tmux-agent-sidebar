//! Observed agent sessions retained independently of their terminal panes.
//! This cache never imports arbitrary old conversations or writes agent history.
mod codex;
mod store;

use crate::tmux::{self, AgentType, PaneInfo, PaneStatus, PermissionMode, SessionInfo, WindowInfo};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

pub const RETENTION_OPTION: &str = "@sidebar_idle_retention_minutes";
const GRACE_SECONDS: u64 = 3;
const SAVED_PREFIX: &str = "retained:";

pub fn is_retained(pane_id: &str) -> bool {
    pane_id.starts_with(SAVED_PREFIX)
}

pub fn stable_hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}

/// Conversation identity survives pane exit and resume in a different window.
pub fn identity(pane: &PaneInfo) -> String {
    pane.pane_id
        .strip_prefix(SAVED_PREFIX)
        .map(str::to_owned)
        .unwrap_or_else(|| live_key(pane, pane.session_id.as_deref()))
}

fn live_key(pane: &PaneInfo, session_id: Option<&str>) -> String {
    match session_id {
        Some(sid) => format!("{}:{sid}", pane.agent.as_str()),
        None => format!(
            "{}:unknown:{:016x}",
            pane.agent.as_str(),
            stable_hash(&format!(
                "{}|{:?}|{}|{}",
                pane.pane_id, pane.pane_pid, pane.path, pane.prompt
            ))
        ),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub key: String,
    pub agent: String,
    pub session_id: Option<String>,
    pub cwd: String,
    pub title: String,
    pub pinned: bool,
    pub last_seen: u64,
    pub closed_at: Option<u64>,
    pub live_pane: String,
    pub live_pid: Option<u32>,
    pub pending_until: u64,
}

impl Entry {
    pub fn ghost_id(&self) -> String {
        format!("{SAVED_PREFIX}{}", self.key)
    }
    pub fn to_json(&self) -> Value {
        json!({
            "key":self.key,"agent":self.agent,"session_id":self.session_id,"cwd":self.cwd,
            "title":self.title,"pinned":self.pinned,"last_seen":self.last_seen,
            "closed_at":self.closed_at,"live_pane":self.live_pane,"live_pid":self.live_pid,
            "pending_until":self.pending_until
        })
    }
    pub(super) fn from_json(v: &Value) -> Option<Self> {
        let text = |k: &str| v.get(k)?.as_str().map(str::to_string);
        Some(Self {
            key: text("key")?,
            agent: text("agent")?,
            session_id: text("session_id"),
            cwd: text("cwd")?,
            title: text("title")?,
            pinned: v["pinned"].as_bool()?,
            last_seen: v["last_seen"].as_u64()?,
            closed_at: v["closed_at"].as_u64(),
            live_pane: text("live_pane")?,
            live_pid: v["live_pid"].as_u64().and_then(|n| u32::try_from(n).ok()),
            pending_until: v["pending_until"].as_u64().unwrap_or(0),
        })
    }
    fn ghost(&self) -> PaneInfo {
        PaneInfo {
            pane_id: self.ghost_id(),
            pane_active: false,
            status: PaneStatus::Idle,
            attention: false,
            agent: AgentType::from_label(&self.agent).unwrap_or(AgentType::Unknown),
            path: self.cwd.clone(),
            current_command: String::new(),
            prompt: self.title.clone(),
            prompt_is_response: false,
            started_at: None,
            wait_reason: String::new(),
            permission_mode: PermissionMode::Default,
            subagents: vec![],
            pane_pid: None,
            worktree: Default::default(),
            session_id: self.session_id.clone(),
            session_name: String::new(),
            sidebar_spawned: false,
            bg_shell_cmd: None,
        }
    }
}

pub struct History {
    path: PathBuf,
    pub entries: BTreeMap<String, Entry>,
    row_keys: BTreeMap<String, String>,
    codex: codex::Index,
    last_live: BTreeMap<String, PaneInfo>,
}

impl History {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            entries: BTreeMap::new(),
            row_keys: BTreeMap::new(),
            codex: Default::default(),
            last_live: BTreeMap::new(),
        }
    }
    pub fn for_pane(pane: &str) -> Result<Self, String> {
        let socket = tmux::display_message(pane, "#{socket_path}");
        if socket.is_empty() {
            return Err("cannot identify tmux server".into());
        }
        let root = std::env::var_os("TMUX_AGENT_SIDEBAR_STATE_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("XDG_STATE_HOME")
                    .map(|p| PathBuf::from(p).join("tmux-agent-sidebar"))
            })
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|p| PathBuf::from(p).join(".local/state/tmux-agent-sidebar"))
            })
            .ok_or("no state directory available")?;
        Ok(Self::new(root.join(format!(
            "sessions-{:016x}.json",
            stable_hash(&socket)
        ))))
    }
    pub fn record(&self, pane: &str) -> Option<&Entry> {
        self.row_keys
            .get(pane)
            .and_then(|key| self.entries.get(key))
            .or_else(|| {
                pane.strip_prefix(SAVED_PREFIX)
                    .and_then(|key| self.entries.get(key))
            })
    }
    pub fn reload(&mut self) -> Result<(), String> {
        self.entries = store::transaction(&self.path, |db| Ok(db.clone()))?;
        Ok(())
    }
    pub fn sync(
        &mut self,
        sessions: &mut Vec<SessionInfo>,
        now: u64,
        minutes: u64,
    ) -> Result<(), String> {
        self.codex.enrich(sessions, now);
        let live: Vec<PaneInfo> = sessions
            .iter()
            .flat_map(|s| s.windows.iter())
            .flat_map(|w| w.panes.iter())
            .cloned()
            .collect();
        let (db, rows) = store::transaction(&self.path, |db| {
            let rows = reconcile(db, &live, now, minutes);
            Ok((db.clone(), rows))
        })?;
        self.entries = db;
        self.row_keys = rows;
        let seen: HashSet<_> = self.row_keys.values().cloned().collect();
        for pane in sessions
            .iter_mut()
            .flat_map(|s| &mut s.windows)
            .flat_map(|w| &mut w.panes)
        {
            if let Some(key) = self.row_keys.get(&pane.pane_id) {
                // Also expose a resume claim's exact ID before the agent's own
                // metadata arrives, so row identity remains stable.
                if pane.session_id.is_none() {
                    pane.session_id = self.entries[key].session_id.clone();
                }
                self.last_live.insert(key.clone(), pane.clone());
            }
        }
        let mut retained = Vec::new();
        for (key, entry) in &self.entries {
            if seen.contains(key) {
                continue;
            }
            let pane = if entry.closed_at.is_some() {
                entry.ghost()
            } else {
                // Keep the last visible row throughout the absence grace
                // period. It used to disappear until closed_at was set.
                self.last_live.get(key).cloned().unwrap_or_else(|| {
                    let mut pane = entry.ghost();
                    pane.pane_id = entry.live_pane.clone();
                    pane.pane_pid = entry.live_pid;
                    pane
                })
            };
            self.row_keys.insert(pane.pane_id.clone(), key.clone());
            retained.push(pane);
        }
        self.last_live
            .retain(|key, _| self.entries.contains_key(key));
        if !retained.is_empty() {
            sessions.push(SessionInfo {
                session_name: "retained sessions".into(),
                windows: vec![WindowInfo {
                    window_id: "retained".into(),
                    window_name: "retained".into(),
                    window_active: false,
                    auto_rename: false,
                    panes: retained,
                }],
            });
        }
        Ok(())
    }
    pub fn toggle_pin(&mut self, pane: &str, now: u64) -> Result<bool, String> {
        let key = self
            .record(pane)
            .ok_or("session record is unavailable")?
            .key
            .clone();
        let pinned = store::transaction(&self.path, |db| {
            let e = db.get_mut(&key).ok_or("session record expired")?;
            e.pinned = !e.pinned;
            if !e.pinned && e.closed_at.is_some() {
                e.closed_at = Some(now);
            }
            Ok(e.pinned)
        })?;
        self.reload()?;
        Ok(pinned)
    }
    pub fn resume(&mut self, pane: &str, sidebar: &str, now: u64) -> Result<String, String> {
        let saved = self
            .record(pane)
            .ok_or("session record is unavailable")?
            .clone();
        // A different sidebar or a manually resumed client may have connected
        // since this frame was drawn. Check live identities before claiming.
        let mut live = tmux::query_sessions();
        self.codex.enrich(&mut live, now);
        if live
            .iter()
            .flat_map(|s| &s.windows)
            .flat_map(|w| &w.panes)
            .any(|p| {
                p.agent.as_str() == saved.agent
                    && p.session_id.is_some()
                    && p.session_id == saved.session_id
            })
        {
            return Err("session is already connected".into());
        }
        let key = saved.key.clone();
        // Persist a short claim before starting a process. A failed cache write
        // cannot leave an unrecorded new pane, and simultaneous clicks cannot
        // launch the same conversation twice.
        let entry = store::transaction(&self.path, |db| {
            let e = db.get_mut(&key).ok_or("session record expired")?;
            if e.pending_until > now {
                return Err("session is already resuming".into());
            }
            if e.closed_at.is_none() {
                return Err("session is already connected".into());
            }
            resume_argv(e)?;
            if !Path::new(&e.cwd).is_dir() {
                return Err("original directory no longer exists".into());
            }
            e.pending_until = now.saturating_add(15);
            Ok(e.clone())
        })?;
        match spawn_resume(&entry, sidebar) {
            Ok(created) => {
                let pid = tmux::display_message(&created, "#{pane_pid}").parse().ok();
                store::transaction(&self.path, |db| {
                    let e = db
                        .get_mut(&key)
                        .ok_or("session record disappeared while resuming")?;
                    e.live_pane = created.clone();
                    e.live_pid = pid;
                    e.last_seen = now;
                    Ok(())
                })?;
                self.reload()?;
                Ok(created)
            }
            Err(error) => {
                let _ = store::transaction(&self.path, |db| {
                    if let Some(e) = db.get_mut(&key) {
                        e.pending_until = 0;
                    }
                    Ok(())
                });
                let _ = self.reload();
                Err(error)
            }
        }
    }
}

fn spawn_resume(entry: &Entry, sidebar: &str) -> Result<String, String> {
    let argv = resume_argv(entry)?;
    let session = tmux::display_message(sidebar, "#{session_id}");
    if !session.starts_with('$') {
        return Err("cannot identify the sidebar's tmux session".into());
    }
    // An empty window index chooses the next free index in this session.
    let target = format!("{session}:");
    // Multiple argv items use tmux's direct exec path, without shell interpolation.
    // Without -d, tmux selects the new window and focuses the resumed client.
    let mut args = vec![
        "new-window",
        "-t",
        target.as_str(),
        "-c",
        entry.cwd.as_str(),
        "-P",
        "-F",
        "#{pane_id}",
    ];
    args.extend(argv.iter().map(String::as_str));
    let created = tmux::run_tmux_capture(&args)?;
    if !created.starts_with('%') {
        return Err("tmux did not return a pane id".into());
    }
    Ok(created)
}

pub fn resume_argv(entry: &Entry) -> Result<Vec<String>, String> {
    let sid = entry
        .session_id
        .as_deref()
        .filter(|s| {
            !s.is_empty()
                && !s.starts_with('-')
                && s.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        })
        .ok_or("exact session ID unavailable; open the agent's resume picker manually")?;
    match entry.agent.as_str() {
        "codex" => Ok(vec!["codex".into(), "resume".into(), sid.into()]),
        "claude" => Ok(vec!["claude".into(), "--resume".into(), sid.into()]),
        "opencode" => Ok(vec!["opencode".into(), "--session".into(), sid.into()]),
        _ => Err("this agent does not support resume".into()),
    }
}

/// Pure reconciliation, shared by all sidebar instances under the store lock.
fn reconcile(
    db: &mut BTreeMap<String, Entry>,
    live: &[PaneInfo],
    now: u64,
    minutes: u64,
) -> BTreeMap<String, String> {
    let mut seen = HashSet::new();
    let mut rows = BTreeMap::new();
    for p in live {
        let pending = db.values().find(|e| {
            e.pending_until > now && e.live_pane == p.pane_id && e.live_pid == p.pane_pid
        });
        let pending_deadline = pending.map_or(0, |e| e.pending_until);
        let sid = p
            .session_id
            .clone()
            .or_else(|| pending.and_then(|e| e.session_id.clone()));
        let key = live_key(p, sid.as_deref());
        // Upgrade previously unresolved records without losing their pin.
        let old_key = db
            .iter()
            .find(|(k, e)| {
                *k != &key
                    && e.session_id.is_none()
                    && e.live_pane == p.pane_id
                    && e.live_pid == p.pane_pid
                    && e.cwd == p.path
                    && e.title == p.prompt
            })
            .map(|(k, _)| k.clone());
        let old = old_key.and_then(|k| db.remove(&k));
        let pinned = db.get(&key).is_some_and(|e| e.pinned) || old.is_some_and(|e| e.pinned);
        let e = Entry {
            key: key.clone(),
            agent: p.agent.as_str().into(),
            session_id: sid,
            cwd: p.path.clone(),
            title: p.prompt.clone(),
            pinned,
            last_seen: now,
            closed_at: None,
            live_pane: p.pane_id.clone(),
            live_pid: p.pane_pid,
            pending_until: if p.session_id.is_none() {
                pending_deadline
            } else {
                0
            },
        };
        db.insert(key.clone(), e);
        seen.insert(key.clone());
        rows.insert(p.pane_id.clone(), key);
    }
    for (key, e) in db.iter_mut() {
        if !seen.contains(key)
            && (now.saturating_sub(e.last_seen) >= GRACE_SECONDS || rows.contains_key(&e.live_pane))
            && e.closed_at.is_none()
        {
            // A different conversation in the same pane confirms the old
            // one closed; do not retain two rows with the same pane target.
            e.closed_at = Some(now.min(e.last_seen.saturating_add(GRACE_SECONDS)));
            e.pending_until = 0;
        }
    }
    db.retain(|_, e| {
        e.pinned
            || e.pending_until > now
            || e.closed_at
                .is_none_or(|at| now.saturating_sub(at) < minutes.saturating_mul(60))
    });
    rows
}

#[cfg(test)]
mod tests;
