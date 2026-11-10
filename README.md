<h1 align="center">tmux-agent-sidebar</h1>

## Codex compatibility fork

This fork of [hiroppy/tmux-agent-sidebar](https://github.com/hiroppy/tmux-agent-sidebar)
adds process-based discovery for attached Codex clients when hooks cannot identify
their tmux pane. It infers running/waiting/idle state from the default terminal
title and leaves working hook metadata in charge. Version: `0.13.0-codex.5`.

Build this fork from source; upstream release binaries do not contain the patch:

```sh
cd ~/.tmux/plugins/tmux-agent-sidebar
cargo build --release --locked
mkdir -p bin
cp target/release/tmux-agent-sidebar bin/tmux-agent-sidebar
if [ "$(uname -s)" = Darwin ]; then
  codesign --force --sign - bin/tmux-agent-sidebar
fi
```

Toggle existing sidebars off and on after building. The plugin prefers `bin/`
over `target/release/`, so copying the built binary is required when `bin/` exists.
The installer and update checker target this fork, not upstream releases.

Fallback limitations: terminal titles must retain Codex's default activity item;
custom nonempty titles are treated as idle. Existing `@pane_agent` metadata takes
priority even if its hooks have stopped updating. Elapsed time, tool activity,
and subagent details still require working hooks. The retained-session feature
can resolve session IDs from the local Codex index as described below. Noninteractive
command detection currently assumes the subcommand immediately follows `codex`.
See [Codex setup](website/src/content/docs/getting-started/codex.md).

The status bar has five filters: All (`≡`), Running (`●`), Waiting (`◐`),
Idle (`○`), and Error (`✕`). Running includes live background commands; these
still show their command details and return to idle when the process exits.
Existing `background` filter settings migrate to Running.

## Retained idle sessions and session pins

After an observed agent exits or its pane closes, its conversation stays in
**Idle** (and **All**) with a muted `◌` status icon for 30 minutes. Live idle
sessions keep `○`; the closed icon can be customized with `@sidebar_icon_closed`.
Configure the duration
in `~/.tmux.conf` before plugin initialization:

```tmux
set -g @sidebar_idle_retention_minutes 30
```

The option is read on refresh; use `tmux set -g @sidebar_idle_retention_minutes 60`
to change it immediately. `0` disables retention for unpinned closed sessions.
A three-second absence grace period avoids flicker during transient discovery
gaps. Only conversations observed by this sidebar version are retained.

- Click a session's `[p]`, or select it and press `p`, to pin it. `[P]` means
  pinned. A pinned conversation remains after exit without a time limit, even
  across sidebar restarts. Unpinning a closed conversation starts a fresh
  retention period.
- Click the closed session's `+`, or select it and press `n` (the existing spawn
  action) or Enter, to resume it in a **new pane in the current window**, using
  its original working directory and exact session ID. Selecting a closed row
  alone does not launch anything. The repository header's `+` also resumes a
  selected closed session in that group; otherwise worktree spawning works as
  before. Resume creates no worktree.
- Session pins are separate from `prefix + b`, which keeps the sidebar expanded.

Resume supports `codex resume <ID>`, `claude --resume <ID>`, and
`opencode --session <ID>`. Missing directories or unavailable IDs produce a
message instead of launching a different conversation. Codex without hook IDs
uses an optional read-only `sqlite3` lookup in `$CODEX_HOME/state_*.sqlite`
(default `~/.codex`), accepting only a unique directory + displayed-title match.
Ambiguous titles are not guessed, and `resume --last` is never used.

Session metadata and pins are stored per tmux socket under
`$XDG_STATE_HOME/tmux-agent-sidebar`, or `~/.local/state/tmux-agent-sidebar` when
unset. Files have mode 0600; multiple sidebars share a locked, atomically updated
cache. The cache contains IDs, titles and working directories, not conversation
transcripts. `TMUX_AGENT_SIDEBAR_STATE_DIR` overrides the directory for tests.

After `cargo build`, run the lifecycle integration test with:

```sh
python3 tests/retained_sessions_smoke.py
```

It uses a private tmux socket and a fake agent, and checks pin persistence,
retention, exact resume arguments, working-directory quoting, and duplicate
launch prevention without starting any real AI session.

## Hover auto-hide (optional)

On tmux 3.7b, enable a narrow hover rail before the plugin initialization:

```tmux
set -g mouse on
set -g @sidebar_auto_hide on
set -g @sidebar_collapsed_width 2
set -g @sidebar_expand_delay 200
set -g @sidebar_hide_delay 800
set -g @sidebar_pin_key b
```

Delays are milliseconds. The rail expands to `@sidebar_width` (default `15%`)
after hovering, without changing keyboard focus. Moving into another pane or
the status bar starts the hide delay. A focused sidebar or an open sidebar
menu stays expanded. `prefix + b` pins/unpins the current window's sidebar;
`prefix + e` still closes or creates it. `!` counts waiting/error agents and
`*` counts running/background agents; counts above nine display `+`.

Set `@sidebar_auto_hide off` and reload the plugin to restore normal width and
remove its mouse tracking. The setting defaults to off. Resizing uses a real
tmux pane, so the main application's text reflows; this is not an overlay.
Zoomed windows are left alone until unzoomed. Terminals must report mouse motion;
when the pointer leaves the terminal entirely, tmux retains its last position
until the next mouse event. Multiple clients sharing a window share its width.

The plugin uses an otherwise unbound `root Any` key to observe motion on tmux
3.7b. Existing specific bindings retain priority; other keys and mouse events
are forwarded. If a custom `root Any` already exists, auto-hide stays disabled
instead of replacing it. A custom pin-key binding is also preserved.

To run the isolated PTY/mouse integration test after `cargo build`:

```sh
python3 tests/auto_hide_smoke.py
```

<p align="center">One tmux sidebar that tracks every Claude Code, Codex, and OpenCode pane across every session and window. See status, background shells, prompts, Git state, activity, and worktrees without switching windows.</p>

<p align="center"><img src="website/src/assets/captures/hero.png" alt="tmux-agent-sidebar hero" /></p>

<p align="center">
  <a href="https://hiroppy.github.io/tmux-agent-sidebar/">Documentation</a> ·
  <a href="https://hiroppy.github.io/tmux-agent-sidebar/getting-started/installation/">Getting Started</a> ·
  <a href="https://hiroppy.github.io/tmux-agent-sidebar/features/agent-pane/">Features</a>
</p>

## Features

- **Every pane, one view** 
  — tracks Claude Code, Codex, and OpenCode panes across all tmux sessions and windows
- **Live metadata** 
  — prompts, tool calls, response previews, background shell state, wait reasons, task progress, and subagent trees refresh as the agents work
- **Worktrees, included** 
  — spawn a fresh worktree + agent from the sidebar and tear it down — window, worktree, and branch — in one keystroke
- **Desktop notifications** 
  — native alerts when an agent finishes, needs permission, or errors out

OpenCode uses a small local plugin bridge instead of per-event hook config. The plugin lives at `.opencode/plugins/tmux-agent-sidebar.js` and can be symlinked as a single file into `~/.config/opencode/plugins/` so it coexists with any existing plugins.

## Requirements

- tmux 3.0+
- [TPM](https://github.com/tmux-plugins/tpm) (or the manual install in [Installation](https://hiroppy.github.io/tmux-agent-sidebar/getting-started/installation/))
- [GitHub CLI](https://cli.github.com/) (optional — required only for PR numbers in the Git tab)

## Quick Start

### 1. Install the plugin

Using [TPM](https://github.com/tmux-plugins/tpm):

```tmux
set -g @plugin 'Elliot-Wang/tmux-agent-sidebar'
```

Reload tmux (`tmux source ~/.tmux.conf`), then press `prefix + I`. Choose **Build from source** in the install wizard for this fork.

### 2. Wire up the agent hooks

- **Claude Code** — register the plugin inside Claude Code:

  ```sh
  /plugin marketplace add ~/.tmux/plugins/tmux-agent-sidebar
  /plugin install tmux-agent-sidebar@hiroppy
  ```

- **Codex** — open a Codex pane, press `prefix + e`, click the yellow `ⓘ` badge, copy the setup snippet, paste it into the Codex pane.
- **OpenCode** — symlink just the plugin file so your existing `~/.config/opencode/plugins/` contents stay untouched:

  ```sh
  mkdir -p ~/.config/opencode/plugins
  ln -sf ~/.tmux/plugins/tmux-agent-sidebar/.opencode/plugins/tmux-agent-sidebar.js \
    ~/.config/opencode/plugins/tmux-agent-sidebar.js
  ```

Full walkthroughs: [Claude Code setup](https://hiroppy.github.io/tmux-agent-sidebar/getting-started/claude-code/) · [Codex setup](https://hiroppy.github.io/tmux-agent-sidebar/getting-started/codex/) · [OpenCode setup](https://hiroppy.github.io/tmux-agent-sidebar/getting-started/opencode/)

### 3. Toggle the sidebar

`prefix + e` toggles the sidebar in the current window, `prefix + E` toggles it everywhere.

## Documentation

The [documentation site](https://hiroppy.github.io/tmux-agent-sidebar/) covers every feature and option:

- [Agent pane breakdown](https://hiroppy.github.io/tmux-agent-sidebar/features/agent-pane/)
- [Worktree lifecycle](https://hiroppy.github.io/tmux-agent-sidebar/features/worktree/)
- [Activity log](https://hiroppy.github.io/tmux-agent-sidebar/features/activity-log/) · [Git tab](https://hiroppy.github.io/tmux-agent-sidebar/features/git-status/) · [Notifications](https://hiroppy.github.io/tmux-agent-sidebar/features/notifications/)
- [Agent support matrix](https://hiroppy.github.io/tmux-agent-sidebar/agents/)
- [Keybindings](https://hiroppy.github.io/tmux-agent-sidebar/reference/keybindings/) · [tmux options](https://hiroppy.github.io/tmux-agent-sidebar/reference/tmux-options/) · [Scripting](https://hiroppy.github.io/tmux-agent-sidebar/reference/scripting/)

## Development

Symlink the plugin directory to your working copy so builds are picked up without copying:

```sh
rm -rf ~/.tmux/plugins/tmux-agent-sidebar
ln -s <path-to-this-repo> ~/.tmux/plugins/tmux-agent-sidebar
cargo build --release
```

Toggle the sidebar off → on to pick up the new binary.

### Picking up local builds for the Claude Code plugin

If you also installed this as a Claude Code plugin (`/plugin`), its install path
holds a copy of the released binary that hooks resolve before falling back to
`target/release/`. To make local builds flow through Claude Code hooks too,
replace that copy with a symlink to your working copy:

```sh
# Replace the cached plugin install with a symlink to your repo
PLUGIN_CACHE=~/.claude/plugins/cache/<owner>/tmux-agent-sidebar/<version>
rm -rf "$PLUGIN_CACHE"
ln -s <path-to-this-repo> "$PLUGIN_CACHE"
```

Also remove the stale release binary at `bin/tmux-agent-sidebar` in your repo
if present — both the tmux launcher and `hook.sh` prefer `bin/` over
`target/release/`, so a leftover binary there will mask `cargo build --release`
output:

```sh
rm -f bin/tmux-agent-sidebar
```

Note: Claude Code's plugin updater may overwrite the symlink on a future
update; re-run the symlink step if that happens.

## License

[MIT](./LICENSE)
