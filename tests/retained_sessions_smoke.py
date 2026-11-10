#!/usr/bin/env python3
"""Isolated lifecycle test. Builds a fake Codex; never launches an AI session.
Usage: python3 tests/retained_sessions_smoke.py [sidebar-binary]
"""
import json
import os
from pathlib import Path
import shlex
import sqlite3
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else ROOT / 'target/debug/tmux-agent-sidebar'


def main():
    with tempfile.TemporaryDirectory(prefix='sidebar-retained-') as tmp:
        root = Path(tmp).resolve()
        work = root / "work with 'quotes' $literal"
        work.mkdir()
        bindir = root / 'bin'; bindir.mkdir()
        source = root / 'fake.c'
        source.write_text(r'''
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
int main(int argc,char **argv) {
  char cwd[4096];getcwd(cwd,sizeof(cwd));
  FILE *f=fopen(getenv("FAKE_CODEX_LOG"),"a");
  fprintf(f,"%s\t",cwd);
  for(int i=1;i<argc;i++)fprintf(f,"%s%s",i>1?"\t":"",argv[i]);
  fprintf(f,"\n");fclose(f);
  printf("\033]0;retained-task | fixture\007");fflush(stdout);
  while(1)sleep(1);
}
''')
        subprocess.run(['cc', str(source), '-o', str(bindir / 'codex')], check=True)
        home = root / 'codex-home'; home.mkdir()
        with sqlite3.connect(home / 'state_5.sqlite') as db:
            db.execute('create table threads(id text,cwd text,name text,title text,archived integer)')
            db.execute('insert into threads values(?,?,?,?,0)', ('test-session', str(work), 'retained-task', 'original prompt'))
        socket = str(root / 'tmux.sock')
        log = root / 'commands.log'
        env = dict(os.environ, TMUX=f'{socket},0,0', CODEX_HOME=str(home),
                   TMUX_AGENT_SIDEBAR_STATE_DIR=str(root / 'state'),
                   FAKE_CODEX_LOG=str(log), PATH=str(bindir) + os.pathsep + os.environ['PATH'])
        env.pop('TMUX_PANE', None)
        def tmux(*args):
            return subprocess.check_output(['tmux', '-S', socket, *args], env=env, text=True, timeout=8).strip()
        def history(action, *args, check=True):
            return subprocess.run([str(BINARY), 'history', action, sidebar, *args], env=env,
                                  check=check, text=True, capture_output=True, timeout=8)
        def records():
            return json.loads(history('list').stdout)
        def wait(check, label, timeout=12):
            until = time.monotonic() + timeout
            while time.monotonic() < until:
                if check():
                    print('PASS:', label, flush=True); return
                time.sleep(.1)
            raise AssertionError(label)
        try:
            tmux('-f', '/dev/null', 'new-session', '-d', '-s', 'test', '-x', '120', '-y', '40', '-c', str(work), str(bindir / 'codex'), 'initial')
            agent = tmux('display-message', '-p', '#{pane_id}')
            window = tmux('display-message', '-p', '#{window_id}')
            session = tmux('display-message', '-p', '#{session_id}')
            for key, value in {'@pane_agent':'codex', '@pane_session_id':'test-session', '@pane_status':'idle', '@pane_prompt':'retained-task | fixture', '@pane_cwd':str(work)}.items():
                tmux('set-option', '-p', '-t', agent, key, value)
            tmux('split-window', '-h', '-t', agent, 'sleep 600')
            tmux('set-option', '-g', '@sidebar_idle_retention_minutes', '30')
            tmux('set-option', '-g', '@sidebar_auto_hide', 'off')
            tmux('set-option', '-g', '@agent_sidebar_bin', str(BINARY))
            tmux('source-file', str(ROOT / 'agent-sidebar.conf'))
            subprocess.run([str(BINARY), 'toggle', '--create-only', window, str(work)], env=env, check=True)
            sidebar = next(row.split('|')[0] for row in tmux('list-panes', '-F', '#{pane_id}|#{@pane_role}').splitlines() if row.endswith('|sidebar'))
            wait(lambda: len(records()) == 1, 'live session recorded with exact identity')
            assert records()[0]['session_id'] == 'test-session'
            tmux('select-pane', '-t', sidebar)
            tmux('send-keys', '-t', sidebar, 'p')
            wait(lambda: records()[0]['pinned'], 'row keyboard pin persists')
            tmux('kill-pane', '-t', agent)
            wait(lambda: records()[0]['closed_at'] is not None, 'closed pane retained as idle record')
            tmux('set-option', '-g', '@sidebar_idle_retention_minutes', '0')
            time.sleep(1.5)
            assert len(records()) == 1, 'pin ignores zero retention'
            tmux('respawn-pane', '-k', '-t', sidebar, str(BINARY))
            wait(lambda: records()[0]['pinned'], 'pin survives sidebar restart')
            time.sleep(.5)
            key = records()[0]['key']
            moved = root / 'temporarily-moved'
            work.rename(moved)
            result = history('resume', key, check=False)
            assert result.returncode != 0 and 'directory' in result.stderr
            assert len(tmux('list-panes', '-F', '#{pane_id}').splitlines()) == 2
            moved.rename(work)
            tmux('set-option', '-g', '@sidebar_idle_retention_minutes', '30')
            original_panes = tmux('list-panes', '-t', window, '-F', '#{pane_id}')
            # n is the existing spawn action; a retained selection resumes instead.
            tmux('send-keys', '-t', sidebar, 'n')
            wait(lambda: len(log.read_text().splitlines()) == 2, 'spawn key starts resume')
            assert log.read_text().splitlines()[1].split('\t') == [str(work), 'resume', 'test-session'], log.read_text()
            assert len(tmux('list-windows', '-t', session, '-F', '#{window_id}').splitlines()) == 2
            resumed_window = tmux('display-message', '-p', '#{window_id}')
            assert resumed_window != window, 'resume selects a new window'
            assert tmux('list-panes', '-t', window, '-F', '#{pane_id}') == original_panes, 'original window is not split'
            wait(lambda: len(records()) == 1 and records()[0]['closed_at'] is None, 'resumed session replaces saved row without duplicate')
            assert records()[0]['pinned']
            result = history('resume', key, check=False)
            assert result.returncode != 0, 'duplicate resume blocked'
            live = records()[0]['live_pane']
            assert tmux('display-message', '-p', '-t', live, '#{window_id}') == resumed_window
            assert tmux('display-message', '-p', '-t', live, '#{session_id}') == session
            assert len(tmux('list-windows', '-t', session, '-F', '#{window_id}').splitlines()) == 2, 'duplicate resume creates no extra window'
            tmux('kill-pane', '-t', live)
            wait(lambda: records()[0]['closed_at'] is not None, 'second exit retained')
            tmux('set-option', '-g', '@sidebar_idle_retention_minutes', '0')
            tmux('select-window', '-t', window)
            tmux('select-pane', '-t', sidebar)
            # Another sidebar may have marked the record closed first; let the
            # original sidebar refresh its selection before sending a key.
            time.sleep(1.2)
            tmux('send-keys', '-t', sidebar, 'p')
            wait(lambda: records() == [], 'unpin obeys configurable expiry')
            print('PASS: exact argv/cwd, pin, restart, expiry, missing cwd, new selected window, original window preserved, deduplication')
        finally:
            subprocess.run(['tmux', '-S', socket, 'kill-server'], env=env,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=8)


if __name__ == '__main__':
    main()
