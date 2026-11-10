---
title: Codex setup
description: Wire up the Codex hooks from inside a Codex pane.
---

Codex registers hooks through a one-paste flow driven by the sidebar itself.

Before Codex will load hooks, enable the feature flag in `~/.codex/config.toml`:

```toml
[features]
codex_hooks = true
```

If you already have a `[features]` table, add `codex_hooks = true` under it.

## Steps

1. Open a Codex pane in tmux and focus it.
2. Press `prefix + e` to toggle the sidebar. A yellow `ⓘ` badge appears in the top row when required hooks are missing.
3. Click `ⓘ`, then click `[copy]` next to `codex` in the Notices popup.
4. Switch back to the Codex pane and paste. Codex runs `tmux-agent-sidebar setup codex` and merges the hooks into `~/.codex/hooks.json`.
5. Restart Codex after changing `config.toml` so the feature flag takes effect.

## Shared app-server daemon

Recent Codex CLI versions execute hooks in a shared daemon. The daemon may
not inherit the terminal client's `TMUX_PANE`, so hooks cannot identify the
pane even when they are enabled and trusted.

When hook metadata is absent, the sidebar now discovers a foreground Codex
client (including the npm `node` launcher) through its pane process tree.
It reads Codex's default terminal-title activity: the leading spinner means
running, either `Action Required` blink phase means waiting for input, and
an otherwise nonempty title means idle. An empty title has unknown status.
The row disappears when the client exits. Discovery does not write pane
options or change hook configuration.

This fallback requires Codex's default title activity item to remain enabled.
Custom titles that omit activity cannot reliably report busy or waiting state.
It displays the attached client's title and directory; session IDs, elapsed
time, and tool activity still require working hooks. Existing hook metadata
retains priority. Detached daemon tasks are not attached pane sessions.
