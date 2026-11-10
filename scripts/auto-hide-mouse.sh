#!/usr/bin/env bash
# tmux 3.7b emits mouse motion internally but cannot bind MouseMovePane by name.
# An unclaimed root Any binding observes unbound mouse events; ordinary keys are
# forwarded unchanged with argument-less send-keys. Specific bindings win first.
# No shell/process is started for each motion event and focus is never changed.
set -euo pipefail
binding_for() {
    tmux list-keys -T "$1" -F '#{key_string} #{key_command}' 2>/dev/null |
        awk -v key="$2" '$1 == key { print }'
}
enabled=$(tmux show-option -gqv @sidebar_auto_hide)
tmux set-option -g @sidebar_mouse_ready off
binding=$(binding_for root Any || true)
pin_key=$(tmux show-option -gqv @sidebar_pin_key)
pin_key=${pin_key:-b}

if [[ "$enabled" != on ]]; then
    if [[ "$binding" == *'@sidebar_hover_pane'* ]]; then
        tmux unbind-key -T root Any
    fi
    pin_binding=$(binding_for prefix "$pin_key" || true)
    if [[ "$pin_binding" == *'@sidebar_pinned'* ]]; then
        tmux unbind-key -T prefix "$pin_key"
    fi
    exit 0
fi
if [[ -n "$binding" && "$binding" != *'@sidebar_hover_pane'* ]]; then
    tmux display-message 'Sidebar auto-hide disabled: custom root Any binding already exists'
    exit 0
fi

tmux source-file - <<'TMUX'
bind-key -T root Any if-shell -F '#{!=:#{mouse_x},}' {
    run-shell -C 'set-option -w -t "#{session_id}:" @sidebar_hover_pane "#{mouse_pane}"'
    if-shell -F '#{mouse_pane}' { send-keys -M }
} {
    send-keys
}
TMUX
pin_binding=$(binding_for prefix "$pin_key" || true)
if [[ -z "$pin_binding" || "$pin_binding" == *'@sidebar_pinned'* ]]; then
    tmux bind-key -T prefix "$pin_key" \
        set-option -wF @sidebar_pinned '#{?@sidebar_pinned,0,1}' \; \
        display-message 'Sidebar pinned: #{@sidebar_pinned}'
fi
tmux set-option -g @sidebar_mouse_ready on
