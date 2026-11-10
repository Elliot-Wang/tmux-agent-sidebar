#!/usr/bin/env python3
"""Isolated real-tmux mouse test. Usage: python3 tests/auto_hide_smoke.py [binary].

Uses a private socket and a synthetic PTY client, never the user's tmux server.
Requires tmux 3.7b and a previously built sidebar binary.
"""
import fcntl
import os
from pathlib import Path
import pty
import shlex
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / 'target/debug/tmux-agent-sidebar'


def main():
    with tempfile.TemporaryDirectory(prefix='sidebar-hover-') as tmp:
        socket = str(Path(tmp) / 'tmux.sock')
        received = Path(tmp) / 'input.bin'
        env = dict(os.environ, TERM='xterm-256color', TMUX=f'{socket},0,0',
                   TMUX_AGENT_SIDEBAR_STATE_DIR=str(Path(tmp) / 'state'))
        env.pop('TMUX_PANE', None)
        def tmux(*args):
            return subprocess.check_output(['tmux', '-S', socket, *args], env=env, text=True, timeout=8).strip()
        def binding(table, key):
            return next((line for line in tmux('list-keys', '-T', table, '-F', '#{key_string} #{key_command}').splitlines() if line.split(' ', 1)[0] == key), '')
        def wait_for(check, label, timeout=6):
            end = time.monotonic() + timeout
            while time.monotonic() < end:
                if check():
                    print('PASS:', label, flush=True)
                    return
                time.sleep(.05)
            raise AssertionError(label)
        def setup_mouse():
            subprocess.run(['bash', str(ROOT / 'scripts/auto-hide-mouse.sh')], env=env, check=True)
        recorder = (
            "import os,sys,tty; tty.setraw(0); "
            "sys.stdout.write('\\x1b[?1003h\\x1b[?1006h\\x1b[?2004h'); sys.stdout.flush(); "
            f"f=open({str(received)!r},'ab',buffering=0); "
            "exec('while True:\\n f.write(os.read(0,4096))')"
        )
        command = shlex.join([sys.executable, '-u', '-c', recorder])
        master = slave = client = None
        terminal_output = bytearray()
        try:
            tmux('-f', '/dev/null', 'new-session', '-d', '-s', 'smoke', '-x', '120', '-y', '40', command)
            primary = tmux('display-message', '-p', '#{pane_id}')
            window = tmux('display-message', '-p', '#{window_id}')
            tmux('set-option', '-g', 'mouse', 'on')
            tmux('set-option', '-g', 'focus-follows-mouse', 'off')
            tmux('set-option', '-g', '@sidebar_auto_hide', 'on')
            tmux('set-option', '-g', '@agent_sidebar_bin', str(BINARY))
            tmux('source-file', str(ROOT / 'agent-sidebar.conf'))
            setup_mouse()
            assert tmux('show-option', '-gqv', '@sidebar_mouse_ready') == 'on'
            subprocess.run([str(BINARY), 'toggle', '--create-only', window, tmp], env=env, check=True)
            sidebar = next(line.split('|')[0] for line in tmux('list-panes', '-F', '#{pane_id}|#{@pane_role}').splitlines() if line.endswith('|sidebar'))
            def width(): return int(tmux('display-message', '-p', '-t', sidebar, '#{pane_width}'))
            def active(): return tmux('display-message', '-p', '-t', window, '#{pane_id}')
            master, slave = pty.openpty()
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 120, 0, 0))
            client = subprocess.Popen(['tmux', '-S', socket, 'attach-session', '-t', 'smoke'],
                                      stdin=slave, stdout=slave, stderr=slave, env=env, start_new_session=True)
            def drain():
                try:
                    while True:
                        chunk = os.read(master, 65536)
                        if not chunk: break
                        terminal_output.extend(chunk)
                        del terminal_output[:-16000]
                except OSError: pass
            threading.Thread(target=drain, daemon=True).start()
            def motion(x, y=4): os.write(master, f'\x1b[<35;{x};{y}M'.encode())
            wait_for(lambda: width() == 2, 'initial automatic collapse')
            assert active() == primary
            for _ in range(3):
                motion(1)
                time.sleep(.15)
            wait_for(lambda: tmux('show-option', '-wqv', '-t', window, '@sidebar_hover_pane') == sidebar, 'mouse target tracked')
            wait_for(lambda: width() == 18, 'hover expands to configured 15%')
            time.sleep(1)
            assert width() == 18 and active() == primary, 'stationary hover/focus preservation'
            motion(60)
            wait_for(lambda: width() == 2, 'leave collapses')
            wait_for(lambda: received.exists() and b'\x1b[<35;' in received.read_bytes(), 'mouse movement still forwarded to application')
            # Unbound keys (including Unicode, modifiers, arrows and paste) pass intact.
            payload = 'abc测试'.encode() + b'\x01\x1b[A\x1bx'
            os.write(master, payload)
            wait_for(lambda: payload in received.read_bytes(), 'keyboard forwarding preserves bytes')
            paste = b'\x1b[200~paste\nblock\x1b[201~'
            os.write(master, paste)
            wait_for(lambda: paste in received.read_bytes(), 'bracketed paste preserved')
            os.write(master, b'\x1b[<0;60;4M\x1b[<0;60;4m\x1b[<64;60;4M')
            wait_for(lambda: b'\x1b[<64;' in received.read_bytes(), 'scroll events still forwarded')
            assert active() == primary
            # Pin/unpin via the actual prefix binding (default tmux prefix C-b).
            os.write(master, b'\x02b')
            wait_for(lambda: width() == 18, 'pin expands')
            time.sleep(1)
            assert width() == 18
            os.write(master, b'\x02b')
            wait_for(lambda: width() == 2, 'unpin resumes automatic collapse')
            # Keyboard focus keeps the rail expanded after the mouse leaves.
            os.write(master, b'\x1b[<0;1;5M\x1b[<0;1;5m')
            wait_for(lambda: active() == sidebar, 'click focuses rail')
            wait_for(lambda: width() == 18, 'focus expands')
            motion(60)
            time.sleep(1.1)
            assert width() == 18
            tmux('select-pane', '-t', primary)
            wait_for(lambda: width() == 2, 'focus release collapses')
            tmux('resize-pane', '-Z', '-t', primary)
            time.sleep(1)
            assert tmux('display-message', '-p', '-t', primary, '#{window_zoomed_flag}') == '1'
            tmux('resize-pane', '-Z', '-t', primary)
            tmux('set-option', '-w', '@sidebar_pinned', '1')
            wait_for(lambda: width() == 18, 'pin after zoom')
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 40, 160, 0, 0))
            os.kill(client.pid, signal.SIGWINCH)
            wait_for(lambda: width() == 24, 'percentage width follows terminal resize')
            tmux('set-option', '-w', '@sidebar_pinned', '0')
            wait_for(lambda: width() == 2, 'collapse after resize')
            # Disabling restores width and removes only plugin-owned bindings.
            tmux('set-option', '-g', '@sidebar_auto_hide', 'off')
            setup_mouse()
            wait_for(lambda: width() == 24, 'disable restores full sidebar')
            assert not binding('root', 'Any')
            tmux('bind-key', '-T', 'root', 'Any', 'display-message', 'custom-motion')
            tmux('set-option', '-g', '@sidebar_auto_hide', 'on')
            setup_mouse()
            assert 'custom-motion' in binding('root', 'Any')
            assert tmux('show-option', '-gqv', '@sidebar_mouse_ready') == 'off'
            print('PASS: real mouse hover, dwell, leave, forwarding, focus, pin, zoom, disable, custom bindings')
        except Exception:
            print('DIAGNOSTICS:', tmux('list-panes', '-F', '#{pane_id} w=#{pane_width} x=#{pane_left} active=#{pane_active} mouse=#{mouse_all_flag} hover=#{@sidebar_hover_pane}'))
            print('BINDING:', [line for line in tmux('list-keys', '-T', 'root').splitlines() if 'Any ' in line])
            print('MESSAGES:', '\n'.join(line for line in tmux('show-messages').splitlines() if 'show-options' not in line and 'display-message -p' not in line)[:6000])
            print('TERMINAL:', repr(bytes(terminal_output[-1000:])))
            raise
        finally:
            subprocess.run(['tmux', '-S', socket, 'kill-server'], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            if client:
                try: client.wait(timeout=3)
                except subprocess.TimeoutExpired: client.kill(); client.wait()
            for fd in [slave, master]:
                if fd is not None: os.close(fd)


if __name__ == '__main__':
    main()
