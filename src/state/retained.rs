use super::AppState;
use crate::{
    retained::{Entry, History, is_retained},
    tmux,
};

impl AppState {
    pub fn retained_entry(&self, pane: &str) -> Option<&Entry> {
        self.retained.as_ref()?.record(pane)
    }
    pub fn toggle_session_pin(&mut self, pane: &str) {
        let result = self
            .retained
            .as_mut()
            .ok_or_else(|| "session cache unavailable".to_string())
            .and_then(|h| h.toggle_pin(pane, crate::time::now_epoch_secs()));
        match result {
            Ok(true) => self.set_flash("Session pinned"),
            Ok(false) => self.set_flash("Session unpinned"),
            Err(e) => self.set_flash(e),
        }
    }
    pub fn resume_retained(&mut self, pane: &str) {
        let result = self
            .retained
            .as_mut()
            .ok_or_else(|| "session cache unavailable".to_string())
            .and_then(|h| h.resume(pane, &self.tmux_pane, crate::time::now_epoch_secs()));
        match result {
            Ok(_) => self.set_flash("Resuming session in a new window"),
            Err(e) => self.set_flash(format!("resume: {e}")),
        }
    }
    pub(crate) fn retain_closed_sessions(&mut self, sessions: &mut Vec<tmux::SessionInfo>) {
        if self.retained.is_none() {
            return;
        }
        let minutes = tmux::display_message(
            &self.tmux_pane,
            &format!("#{{{}}}", crate::retained::RETENTION_OPTION),
        )
        .parse::<u64>()
        .unwrap_or(30);
        if let Some(history) = self.retained.as_mut()
            && let Err(e) = history.sync(sessions, self.now, minutes)
        {
            self.set_flash(format!("session cache: {e}"));
        }
    }
    pub(crate) fn init_retained(&mut self) {
        match History::for_pane(&self.tmux_pane) {
            Ok(h) => self.retained = Some(h),
            Err(e) => self.set_flash(e),
        }
    }
    pub(crate) fn resume_selected_in_group(&mut self, group_name: &str) -> bool {
        let Some(pane) = self.selected_pane() else {
            return false;
        };
        let id = pane.pane_id.clone();
        if is_retained(&id)
            && self
                .repo_groups
                .iter()
                .any(|g| g.name == group_name && g.panes.iter().any(|(p, _)| p.pane_id == id))
        {
            self.resume_retained(&id);
            return true;
        }
        false
    }
}
