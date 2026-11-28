#!/usr/bin/env python3
"""Exercise the real Chromium/CEF in an isolated app home and herdr-core session."""
import argparse
import functools
from http.server import ThreadingHTTPServer, SimpleHTTPRequestHandler
import json
import os
from pathlib import Path
import subprocess
import threading
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', default=str(ROOT / '.local/preview/Herdr Browser.app/Contents/MacOS/HerdrBrowser'))
    parser.add_argument('--work', default=None)
    parser.add_argument('--keep', action='store_true')
    args = parser.parse_args()
    work = Path(args.work) if args.work else ROOT / '.local/native-smoke' / str(time.time_ns())
    home = work / 'home'; pages = work / 'pages'
    home.mkdir(parents=True); pages.mkdir()
    (home / 'shared.json').write_text('{"frame.welcomed":"finished"}')
    (home / 'settings.json').write_text('{"sounds":{"enabled":false},"core":"' + str(Path.home() / '.local/bin/hc') + '"}')
    mod = home / 'mods/toast-smoke'; mod.mkdir(parents=True)
    (mod / 'mod.json').write_text('{"place":"none"}')
    (mod / 'main.js').write_text('import { View, div } from "gpui-kit";\nimport { notify } from "herdr";\nexport default class extends View { init() { notify({ title: "Toast smoke", body: "The page stays", timeout: 4000 }); } render() { return div(); } }\n')
    for page in ['one.html', 'two.html']:
        (pages / page).write_text(f'<!doctype html><meta name="viewport" content="width=device-width,initial-scale=1"><title>{page}</title><style>body{{font:20px system-ui;padding:24px}}button{{font:inherit}}</style><h1>Native Herdr browser</h1><p>Page {page}. Rendered by native Chromium.</p><button onclick="this.textContent=\'Clicked\'">Click test</button><a href="/two.html">Next page</a><script>if(document.modelContext){{document.modelContext.registerTool({{name:"echo",description:"Echo input",inputSchema:{{type:"object",properties:{{msg:{{type:"string"}}}}}},execute:async(i)=>({{echoed:i.msg,nav:Boolean(navigator.modelContext)}})}});}}document.title="{page} / JavaScript ready"</script>')
    (pages / 'download.bin').write_bytes(b'Herdr native download check\n')
    class Handler(SimpleHTTPRequestHandler):
        def do_GET(self):
            self.server.user_agent = self.headers.get('User-Agent')
            super().do_GET()
    handler = functools.partial(Handler, directory=str(pages))
    server = ThreadingHTTPServer(('127.0.0.1', 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    env = os.environ.copy(); env['HERDR_APP_HOME'] = str(home); env['HC_SESSION'] = f'hb-{os.getpid()}'
    # Exercise the portable bundle's literal ~/ home without changing HOME or
    # touching the person's real app profile.
    try: env['HERDR_APP_HOME'] = '~/' + str(home.relative_to(Path.home()))
    except ValueError: pass
    env.pop('HC_TERMINAL_ID', None)
    env['HERDR_BROWSER_TEST_WINDOW_FRONT'] = '1'
    env['HERDR_BROWSER_TEST_KEYCHAIN'] = 'Herdr Browser Test ' + work.name
    binary = str(Path(args.binary).resolve())
    log = (work / 'app.log').open('w')
    app = subprocess.Popen([binary], env=env, stdout=log, stderr=subprocess.STDOUT)
    def command(command_name, caller=None, **kwargs):
        cmd_env = env if caller is None else {**env, 'HC_TERMINAL_ID': caller}
        result = subprocess.run([binary, 'do', command_name, json.dumps(kwargs)], env=cmd_env, text=True, capture_output=True, timeout=5)
        value = json.loads(result.stdout)
        if result.returncode or value.get('error'): raise RuntimeError(f'{command_name}: {value}')
        return value
    def listing():
        result = subprocess.run([binary, 'ls'], env=env, text=True, capture_output=True, timeout=5)
        if result.returncode: raise RuntimeError(f'ls: {result.stdout} {result.stderr}')
        return json.loads(result.stdout)
    def until(test, description, timeout=30):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if app.poll() is not None: raise RuntimeError(f'App exited: see {work}/app.log')
            try:
                value = test()
                if value: return value
            except (subprocess.SubprocessError, json.JSONDecodeError, RuntimeError): pass
            time.sleep(.1)
        raise RuntimeError(f'Timed out: {description}; see {work}/app.log')
    try:
        until(lambda: subprocess.run([binary, 'status'], env=env, capture_output=True, timeout=5).returncode == 0, 'app control socket')
        shell = command('space.new', name='Browser smoke', folder=str(work), tab='Terminal')
        url = f'http://127.0.0.1:{server.server_port}'
        # Agent browser.open defaults to focus: false, appends at the end of the caller's space,
        # and still loads hidden without stealing focus or inserting beside the active tab.
        second = command('tab.new', name='Second', space=shell['space'])
        command('tab.select', id=shell['tab'])
        bg = command('browser.open', profile='agent', url=url + '/one.html')
        bg_state = until(lambda: (x if (x := command('browser.inspect', pane=bg['pane'])).get('title') == 'one.html / JavaScript ready' and not x.get('loading') else None), 'background browser page')
        assert not bg_state.get('native', {}).get('shown'), bg_state
        ls1 = listing()
        sp1 = next(s for s in ls1['spaces'] if s['id'] == shell['space'])
        assert [t['id'] for t in sp1['tabs']] == [shell['tab'], second['tab'], bg['tab']], sp1
        assert next(t['id'] for t in sp1['tabs'] if t['active']) == shell['tab'], sp1
        # A caller terminal in another space routes its browser tab to that caller's space without switching spaces.
        other = command('space.new', name='Other space', folder=str(work), tab='Other')
        routed = command('browser.open', caller=shell['terminal'], profile='agent', url=url + '/two.html')
        assert routed['space'] == shell['space'], routed
        ls2 = listing()
        assert next(s['id'] for s in ls2['spaces'] if s['active']) == other['space'], ls2
        sp1_after = next(s for s in ls2['spaces'] if s['id'] == shell['space'])
        assert [t['id'] for t in sp1_after['tabs']] == [shell['tab'], second['tab'], bg['tab'], routed['tab']], sp1_after
        assert next(t['id'] for t in sp1_after['tabs'] if t['active']) == shell['tab'], sp1_after
        command('pane.close', pane=routed['pane'])
        command('pane.close', pane=bg['pane'])
        command('tab.close', id=second['tab'])
        command('space.close', id=other['space'], confirmed=True)
        command('space.select', id=shell['space'])
        browser = command('browser.open', profile='agent', url=url + '/one.html', focus=True)
        pane = browser['pane']
        subprocess.run([binary], env=env, timeout=5)
        def inspected():
            try:
                value = command('browser.inspect', pane=pane)
            except Exception as error:
                (work / 'inspection.json').write_text(json.dumps({'error': str(error)}))
                raise
            (work / 'inspection.json').write_text(json.dumps(value, indent=2))
            return value
        first = until(lambda: (x if (x := inspected()).get('title') == 'one.html / JavaScript ready' and x.get('native', {}).get('shown') and not x.get('loading') else None), 'native page and JavaScript')
        bounds = first['native']['bounds']; assert bounds[2] > 100 and bounds[3] > 100, first
        wm_tools = command('browser.tools', pane=pane)
        assert [t['name'] for t in wm_tools.get('tools', [])] == ['echo'], wm_tools
        wm_call = command('browser.tool', pane=pane, name='echo', input={'msg': 'hello-webmcp'})
        assert wm_call.get('result') == {'echoed': 'hello-webmcp', 'nav': True}, wm_call
        # Read-only commands and non-DOM evals must not spawn a parked cursor.
        eval_read = command('browser.eval', pane=pane, js='document.title')
        assert eval_read.get('result') == 'one.html / JavaScript ready', eval_read
        cursors_after_read = inspected().get('native', {}).get('agent_cursors', {})
        assert cursors_after_read.get('count') == 0, cursors_after_read
        # A DOM click inside browser.eval must move the cursor to the real element.
        eval_click = command('browser.eval', pane=pane, js='const b = document.querySelector("button"); b.click(); b.textContent')
        assert eval_click.get('result') == 'Clicked', eval_click
        cursors_after_click = inspected().get('native', {}).get('agent_cursors', {})
        assert cursors_after_click.get('count') == 1 and cursors_after_click.get('clicks', 0) >= 1 and cursors_after_click.get('highlights', 0) >= 1, cursors_after_click
        clicked_agent = cursors_after_click['agents'][0]
        assert clicked_agent['x'] is not None and clicked_agent['x'] < bounds[2] - 160, (clicked_agent, bounds)
        command('browser.navigate', pane=pane, url=url + '/two.html')
        until(lambda: inspected().get('title') == 'two.html / JavaScript ready', 'second page')
        cursors_after_nav = inspected().get('native', {}).get('agent_cursors', {})
        assert all(a.get('x') is None and a.get('y') is None for a in cursors_after_nav.get('agents', [])), cursors_after_nav
        command('browser.back', pane=pane)
        until(lambda: inspected().get('title') == 'one.html / JavaScript ready', 'back navigation')
        command('browser.forward', pane=pane)
        until(lambda: inspected().get('title') == 'two.html / JavaScript ready', 'forward navigation')
        command('tab.prev')
        until(lambda: not inspected().get('native', {}).get('shown'), 'hidden browser tab')
        command('tab.next')
        until(lambda: inspected().get('native', {}).get('shown'), 'restored browser tab')
        split = command('browser.open', profile='agent', pane=pane, place='right', url=url + '/one.html')
        def split_bounds():
            left = inspected()['native']['bounds']
            right = command('browser.inspect', pane=split['pane']).get('native', {})
            bounds = right.get('bounds')
            return bounds and right.get('shown') and left[0] + left[2] <= bounds[0] + 1
        until(split_bounds, 'two native browsers within split bounds')
        command('pane.close', pane=split['pane'])
        command('app.settings')
        until(lambda: not inspected().get('native', {}).get('shown'), 'native view behind app dialog')
        command('overlay.close')
        until(lambda: inspected().get('native', {}).get('shown'), 'native view after dialog closes')
        until(lambda: inspected().get('native', {}).get('focused'), 'native page focus after dialog closes')
        command('mod.open', name='toast-smoke')
        until(lambda: not inspected().get('native', {}).get('shown'), 'native view behind the toast-posting dialog')
        command('mod.open', name='toast-smoke')
        until(lambda: (x := inspected().get('native', {})).get('shown') and x.get('focused') and 0 < x.get('cut', 0) < x['bounds'][3], 'page clipped, shown and focused under a toast')
        until(lambda: (x := inspected().get('native', {})).get('shown') and x.get('cut') == 0, 'full page after the toast leaves')
        command('browser.navigate', pane=pane, url=url + '/download.bin')
        until(lambda: (home / 'downloads/download.bin').exists(), 'native download')
        assert (home / 'downloads/download.bin').read_bytes() == b'Herdr native download check\n'
        command('browser.navigate', pane=pane, url=url + '/one.html')
        until(lambda: inspected().get('title') == 'one.html / JavaScript ready', 'page after download')
        command('browser.focus_url', pane=pane)
        command('browser.find', pane=pane, query='Chromium')
        found = until(lambda: (x if (x := inspected()).get('find_open') and (x.get('find_matches') or [0, 0])[1] >= 1 else None), 'find in page match result')
        assert found['find_query'] == 'Chromium' and found['find_matches'][1] == 1, found
        dest = command('space.new', name='Move target', folder=str(work), tab='Dest')
        moved_tab = command('tab.new', name='Movable', space=shell['space'])
        command('tab.move', id=moved_tab['tab'], space=dest['space'], index=1)
        ls_moved = listing()
        dest_sp = next(s for s in ls_moved['spaces'] if s['id'] == dest['space'])
        assert dest_sp['tabs'][0]['id'] == moved_tab['tab'], dest_sp
        command('space.close', id=dest['space'], confirmed=True)
        command('space.select', id=shell['space'])
        md_file = pages / 'notes.md'
        md_file.write_text('# Built-in Markdown\n\n> [!WARNING]\n> Initial warning text.\n')
        md_browser = command('browser.markdown', profile='agent', path=str(md_file))
        md_pane = md_browser['pane']
        until(lambda: (x := command('browser.inspect', pane=md_pane)).get('title') == 'notes.md' and not x.get('loading'), 'markdown viewer page')
        command('browser.wait', pane=md_pane, text='Initial warning text', timeout=10000)
        md_file.write_text('# Built-in Markdown\n\nLive reload confirmed.\n')
        command('browser.wait', pane=md_pane, text='Live reload confirmed', timeout=10000)
        command('pane.close', pane=md_pane)
        report = {'ok': True, 'home': str(home), 'session': env['HC_SESSION'], 'browser': browser,
                  'inspection': inspected(), 'user_agent': getattr(server, 'user_agent', None), 'checks': ['background append + caller space routing', 'page load', 'JavaScript', 'back/forward', 'tab next/prev', 'split bounds', 'dialog hide/show', 'toast clip', 'native download', 'address focus', 'markdown viewer', 'markdown live reload']}
        (work / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
        print(json.dumps(report, indent=2), flush=True)
        if args.keep:
            print('Keeping the isolated preview open. Press Ctrl+C here to close it.', flush=True)
            app.wait()
    except Exception as error:
        print(f'SMOKE FAILED: {error}', flush=True)
        if args.keep and app.poll() is None:
            print(f'Keeping failed preview for inspection: {home}', flush=True)
            app.wait()
        raise
    finally:
        if app.poll() is None:
            try: command('app.quit'); app.wait(timeout=10)
            except Exception: app.terminate(); app.wait(timeout=10)
        subprocess.run([str(Path.home() / '.local/bin/hc'), 'server', 'stop'], env=env, capture_output=True, timeout=10)
        server.shutdown(); log.close()


if __name__ == '__main__': main()
