//! Optional pane resizing driven by tmux's mouse target. Mouse tracking stays
//! in tmux (no process per motion event); this controller polls at 100 ms only
//! for visible windows. Resizing never selects a pane or changes focus.
use std::time::{Duration, Instant};

use crate::{
    state::{AppState, PopupState},
    tmux,
};

const SNAPSHOT: &str = "#{@sidebar_auto_hide}|#{@sidebar_mouse_ready}|#{@sidebar_pinned}|#{@sidebar_hover_pane}|#{pane_active}|#{window_zoomed_flag}|#{window_active}|#{session_attached}|#{window_width}|#{@sidebar_width}|#{@sidebar_collapsed_width}|#{@sidebar_expand_delay}|#{@sidebar_hide_delay}|#{pane_width}|#{@sidebar_collapsed}";

#[derive(Default)]
struct Visibility {
    collapsed: bool,
    pending: Option<(bool, Instant)>,
}

impl Visibility {
    fn update(
        &mut self,
        now: Instant,
        hover: bool,
        protected: bool,
        suspended: bool,
        expand_delay: Duration,
        hide_delay: Duration,
    ) -> bool {
        if suspended {
            self.pending = None;
            return !self.collapsed;
        }
        if protected {
            self.pending = None;
            return true;
        }
        let expand = hover;
        let currently_expanded = !self.collapsed;
        if expand == currently_expanded {
            self.pending = None;
            return expand;
        }
        let since = match self.pending {
            Some((target, since)) if target == expand => since,
            _ => {
                self.pending = Some((expand, now));
                now
            }
        };
        let delay = if expand { expand_delay } else { hide_delay };
        if now.duration_since(since) >= delay {
            expand
        } else {
            !self.collapsed
        }
    }
}

pub(super) struct AutoHide {
    next_poll: Instant,
    visibility: Visibility,
    initialized: bool,
    last_target: Option<(bool, u16)>,
}

fn number(value: &str, fallback: u64, min: u64, max: u64) -> u64 {
    value.parse::<u64>().unwrap_or(fallback).clamp(min, max)
}

fn expanded_width(setting: &str, window_width: u16, collapsed: u16) -> u16 {
    let width = if let Some(percent) = setting.strip_suffix('%') {
        u32::from(window_width) * number(percent, 15, 1, 90) as u32 / 100
    } else {
        number(setting, 30, 1, u16::MAX as u64) as u32
    };
    width
        .max(u32::from(collapsed) + 1)
        .min(u32::from(window_width.saturating_sub(2).max(1))) as u16
}

impl AutoHide {
    pub(super) fn new() -> Self {
        Self {
            next_poll: Instant::now(),
            visibility: Visibility::default(),
            initialized: false,
            last_target: None,
        }
    }

    pub(super) fn tick(&mut self, state: &mut AppState) -> bool {
        let now = Instant::now();
        if now < self.next_poll {
            return false;
        }
        self.next_poll = now + Duration::from_secs(1);
        let raw = tmux::display_message(&state.tmux_pane, SNAPSHOT);
        let parts: Vec<&str> = raw.split('|').collect();
        if parts.len() != 15 {
            return false;
        }
        let enabled = parts[0] == "on" && parts[1] == "on";
        let visible = parts[6] == "1" && parts[7].parse::<u32>().unwrap_or(0) > 0;
        if enabled && visible {
            self.next_poll = now + Duration::from_millis(100);
        }
        if !self.initialized {
            self.visibility.collapsed = parts[14] == "on";
            self.initialized = true;
        }
        let zoomed = parts[5] == "1";
        let protected = !enabled
            || parts[2] == "1"
            || parts[2] == "on"
            || parts[4] == "1"
            || !matches!(state.popup, PopupState::None);
        let expand = self.visibility.update(
            now,
            parts[3] == state.tmux_pane,
            protected,
            zoomed || (enabled && !visible),
            Duration::from_millis(number(parts[11], 200, 0, 5000)),
            Duration::from_millis(number(parts[12], 800, 0, 10000)),
        );
        let collapsed_width = number(parts[10], 2, 1, 6) as u16;
        let window_width = parts[8].parse::<u16>().unwrap_or(80);
        let target = if expand {
            expanded_width(parts[9], window_width, collapsed_width)
        } else {
            collapsed_width
        };
        let current = parts[13].parse::<u16>().unwrap_or(0);
        let currently_expanded = !self.visibility.collapsed;
        let changed = expand != currently_expanded;
        // Do not override manual sizing on every poll. Reapply percentage widths
        // after a window resize, and retry failed transitions at most once/sec.
        let resize =
            changed || (enabled && self.last_target.is_some_and(|old| old != (expand, target)));
        if !zoomed && resize && (visible || !enabled) {
            let result = tmux::run_tmux(&[
                "resize-pane",
                "-t",
                &state.tmux_pane,
                "-x",
                &target.to_string(),
            ]);
            if result.is_none() {
                self.next_poll = now + Duration::from_secs(1);
                return false;
            }
            self.visibility.collapsed = !expand;
            self.visibility.pending = None;
            tmux::set_pane_option(
                &state.tmux_pane,
                "@sidebar_collapsed",
                if expand { "off" } else { "on" },
            );
        }
        if !zoomed && (visible || !enabled) {
            self.last_target = Some((expand, target));
        }
        let collapsed = self.visibility.collapsed && (current <= collapsed_width || resize);
        let redraw = state.sidebar_collapsed != collapsed || resize;
        state.sidebar_collapsed = collapsed;
        redraw
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tick(
        v: &mut Visibility,
        at: Instant,
        hover: bool,
        protected: bool,
        suspended: bool,
    ) -> bool {
        v.update(
            at,
            hover,
            protected,
            suspended,
            Duration::from_millis(200),
            Duration::from_millis(800),
        )
    }
    #[test]
    fn hover_requires_dwell_and_stationary_pointer_stays_expanded() {
        let t = Instant::now();
        let mut v = Visibility {
            collapsed: true,
            ..Default::default()
        };
        assert!(!tick(&mut v, t, true, false, false));
        assert!(!tick(
            &mut v,
            t + Duration::from_millis(199),
            true,
            false,
            false
        ));
        assert!(tick(
            &mut v,
            t + Duration::from_millis(200),
            true,
            false,
            false
        ));
        v.collapsed = false;
        assert!(tick(
            &mut v,
            t + Duration::from_secs(30),
            true,
            false,
            false
        ));
    }
    #[test]
    fn leave_is_delayed_and_reentry_cancels_it() {
        let t = Instant::now();
        let mut v = Visibility::default();
        assert!(tick(&mut v, t, false, false, false));
        assert!(tick(
            &mut v,
            t + Duration::from_millis(700),
            true,
            false,
            false
        ));
        assert!(tick(
            &mut v,
            t + Duration::from_millis(900),
            false,
            false,
            false
        ));
        assert!(!tick(
            &mut v,
            t + Duration::from_millis(1700),
            false,
            false,
            false
        ));
    }
    #[test]
    fn focus_pin_popup_or_disable_opens_immediately_and_restarts_hide_delay() {
        let t = Instant::now();
        let mut v = Visibility {
            collapsed: true,
            ..Default::default()
        };
        assert!(tick(&mut v, t, false, true, false));
        v.collapsed = false;
        assert!(tick(
            &mut v,
            t + Duration::from_secs(1),
            false,
            false,
            false
        ));
        assert!(!tick(
            &mut v,
            t + Duration::from_millis(1800),
            false,
            false,
            false
        ));
    }
    #[test]
    fn zoom_or_inactive_window_suspends_pending_resize() {
        let t = Instant::now();
        let mut v = Visibility::default();
        assert!(tick(&mut v, t, false, false, false));
        assert!(tick(&mut v, t + Duration::from_secs(1), false, false, true));
        assert!(tick(
            &mut v,
            t + Duration::from_secs(2),
            false,
            false,
            false
        ));
    }
    #[test]
    fn widths_follow_configuration_and_fit_small_windows() {
        assert_eq!(expanded_width("15%", 200, 2), 30);
        assert_eq!(expanded_width("40", 200, 2), 40);
        assert_eq!(expanded_width("15%", 20, 2), 3);
        assert_eq!(expanded_width("999", 20, 2), 18);
        assert_eq!(expanded_width("bad", 100, 2), 30);
    }
}
