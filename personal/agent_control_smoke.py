#!/usr/bin/env python3
"""Real in-process CEF control proof; scratch app home/core, non-activating window.

Run through clankie heavy -- python3 personal/agent_control_smoke.py.
No debugging port or OS pointer/key automation. Never launches the installed app.
"""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import re
import subprocess
import struct
import threading
import time

ROOT = Path(__file__).resolve().parents[1]
PAGE = b'''<!doctype html><title>Agent control fixture</title>
<style>body{font:20px system-ui;padding:25px}input,button{font:inherit}</style>
<main><h1>Agent control fixture</h1>
<div style="width:180px;height:100px;overflow:auto;border:1px solid" onscroll="document.querySelector('#boxed').textContent='Box scrolled'"><button>Inner top</button><div style="height:600px"></div></div><p id="boxed"></p>
<img alt="Dot" src="data:image/gif;base64,R0lGODlhAQABAAAAACw=">
<button onclick="document.querySelector('#result').textContent='Clicked safely'">Click me</button>
<form onsubmit="event.preventDefault();document.querySelector('#result').textContent='Submitted: '+document.querySelector('#name').value">
<label>Name <input id="name" autocomplete="off"></label>
<label>Secret <input type="password" id="secret" autocomplete="off"></label><button>Submit</button></form>
<p id="result">Waiting for input</p><p id="key"></p><p id="scroll"></p><p id="cookie"></p><script>document.querySelector('#cookie').textContent='Cookie: '+document.cookie;document.addEventListener('keydown',e=>{if(e.key==='Tab')document.querySelector('#key').textContent='Tab received'});window.addEventListener('scroll',()=>{if(scrollY>0)document.querySelector('#scroll').textContent='Scroll received'})</script><p><a href="/next">Next page</a></p></main>
<div style="height:1500px"></div><p>Bottom of fixture</p>'''
# Every byte value, so a download proves the base64 round trip is exact.
ASSET = bytes(range(256)) * 40


def asn(value):
    # `find` includes the display name, while `front` prints only the ASN.
    match = re.search(r'ASN:0x[0-9a-f]+-0x[0-9a-f]+', value, re.IGNORECASE)
    return match[0] if match else value.strip()


def front():
    return asn(subprocess.run(['/usr/bin/lsappinfo', 'front'], text=True, capture_output=True).stdout)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', default=str(ROOT / '.local/preview/Herdr Browser.app/Contents/MacOS/HerdrBrowser'))
    args = parser.parse_args()
    work = ROOT / '.local/agent-control-smoke' / str(time.time_ns())
    # macOS sockaddr_un paths are limited to 104 bytes; task worktrees are longer.
    home = Path('/tmp') / f'hb-agent-{os.getpid()}-{work.name}'
    work.mkdir(parents=True)
    home.mkdir()
    home = home.resolve()
    (home / 'shared.json').write_text('{"frame.welcomed":"finished"}')
    (home / 'settings.json').write_text(json.dumps({'sounds': {'enabled': False}, 'telemetry': False, 'core': str(Path.home() / '.local/bin/hc')}))

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_): pass
        def do_GET(self):
            if self.path == '/missing':
                self.send_response(404)
                self.send_header('Content-Length', '0')
                self.end_headers()
                return
            body = PAGE if self.path != '/next' else b'<!doctype html><title>Next fixture</title><h1>Navigation complete</h1>'
            if self.path.startswith('/files/asset.bin'): body = ASSET
            self.send_response(200)
            self.send_header('Content-Type', 'application/octet-stream' if body is ASSET else 'text/html')
            # Like a public CDN: any origin may read it, which refuses reads that carry cookies.
            if self.path == '/files/asset.bin?public': self.send_header('Access-Control-Allow-Origin', '*')
            if self.path.startswith('/?set=agent'): self.send_header('Set-Cookie', 'profile_marker=agent-only; Path=/; SameSite=Lax')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    env = os.environ.copy()
    env.update(HERDR_APP_HOME=str(home), HC_SESSION=f'hb-agent-{os.getpid()}', HERDR_APP_BACKGROUND='1', HERDR_BROWSER_TEST_KEYCHAIN=f'Herdr Browser Agent QA {work.name}', HERDR_TELEMETRY='0')
    for name in ['HERDR_BROWSER_DEBUG_PORT', 'HERDR_BROWSER_TEST_WINDOW_FRONT', 'HC_TERMINAL_ID', 'HERDR_PANE_ID']:
        env.pop(name, None)
    binary = str(Path(args.binary).resolve())
    assert '/.local/' in binary and not binary.startswith(str(Path.home() / 'Applications/'))
    before = front()
    log = (work / 'app.log').open('w')
    app = subprocess.Popen([binary], env=env, stdout=log, stderr=subprocess.STDOUT)
    transcript = []
    foreground_audit = []
    space = None

    def command(command_name, expect_error=None, expect_timeout=False, caller='qa-terminal-a', caller_key='HC_TERMINAL_ID', **kwargs):
        cli_env = env.copy()
        if caller is not None: cli_env[caller_key] = caller
        result = subprocess.run([binary, 'do', command_name, json.dumps(kwargs)], env=cli_env, text=True, capture_output=True, timeout=45)
        try: value = json.loads(result.stdout)
        except Exception: raise RuntimeError(f'{command_name}: {result.stderr}')
        transcript.append({'command': command_name, 'args': kwargs, 'caller': caller, 'result': value})
        if expect_error:
            assert expect_error in value.get('error', ''), value
        else:
            assert result.returncode == 0 and 'error' not in value, value
            if expect_timeout: assert value.get('timeout') is True, value
            else: assert not value.get('timeout'), value
        current_front = front()
        own_asn = asn(subprocess.run(['/usr/bin/lsappinfo', 'find', f'pid={app.pid}'], text=True, capture_output=True).stdout)
        foreground_audit.append({'command': command_name, 'before': before, 'current': current_front, 'preview': own_asn})
        # The owner may switch apps while the finite test runs. Detect our own
        # preview becoming frontmost without attributing their activity to us.
        assert current_front == before or (own_asn and current_front != own_asn), foreground_audit[-1]
        assert not own_asn or current_front != own_asn, foreground_audit[-1]
        return value

    def ref(snapshot, role, name):
        for line in snapshot['tree'].splitlines():
            match = re.match(r'(e\d+) (\S+) (.*)', line)
            if match and match[2] == role and json.loads(match[3]).strip() == name.strip():
                return match[1]
        raise AssertionError(f'No {role} {name}: {snapshot}')

    try:
        deadline = time.monotonic() + 60
        while not (home / 'herdr-app.sock').exists():
            assert app.poll() is None, f'App exited; {work}/app.log'
            assert time.monotonic() < deadline, f'No socket; {work}/app.log'
            time.sleep(.1)
        while True:
            try:
                space = command('space.new', name='Agent QA', folder=str(work), tab='Scratch terminal')
                break
            except AssertionError as error:
                # The scratch core may still be connecting.
                if 'unavailable' not in str(error) or time.monotonic() > deadline: raise
                time.sleep(.5)
        url = f'http://127.0.0.1:{server.server_port}/'
        # Unknown callers must never target the person's focused page.
        command('browser.read', caller='unmapped', expect_error='Pass pane explicitly')
        browser = command('browser.open', profile='agent', url=url + '?set=agent')
        pane = browser['pane']
        # Browser entity appears on the next frame; do not front the window to force it.
        deadline = time.monotonic() + 30
        while True:
            time.sleep(.1)
            try:
                state = command('browser.inspect')
                if state.get('page_error'): raise RuntimeError(state['page_error'])
                if state['title'] == 'Agent control fixture' and not state['loading']: break
            except AssertionError:
                if time.monotonic() >= deadline: raise
            assert time.monotonic() < deadline, f'Page did not load: {work}'
        assert state['native']['window_key'] is False, state
        listeners = subprocess.run(['/usr/sbin/lsof', '-nP', '-a', '-p', str(app.pid), '-iTCP', '-sTCP:LISTEN'], capture_output=True, text=True).stdout
        assert not listeners.strip(), listeners
        assert command('browser.read', caller_key='HERDR_PANE_ID')['pane'] == pane
        assert state['profile'] == 'agent'
        assert 'profile_marker=agent-only' in command('browser.read')['text']
        command('browser.wait', load=True)
        command('browser.wait', text='Agent control fixture')
        command('browser.wait', url='127.0.0.1')
        assert command('browser.wait', text='missing text', timeout=100, expect_timeout=True)['timeout']
        snap = command('browser.snapshot')
        click = ref(snap, 'button', 'Click me')
        command('browser.click', ref=click)
        assert 'Clicked safely' in command('browser.read')['text']
        # Refs survive unrelated DOM changes; navigation or node removal expires them.
        command('browser.click', ref=click)
        command('browser.click', name='Click me')
        command('browser.click', name='No such control', expect_error='No interactive element')
        command('browser.type', name='Name', text='Typed by name', submit=True)
        command('browser.wait', text='Submitted: Typed by name')
        command('browser.type', name='Secret', text='must never be inserted', expect_error='Password fields')
        snap = command('browser.snapshot')
        field = ref(snap, 'textbox', 'Name ')
        command('browser.type', ref=field, text='discard this value')
        snap = command('browser.snapshot')
        command('browser.type', ref=ref(snap, 'textbox', 'Name'), text='Background CEF input', submit=True)
        command('browser.wait', text='Submitted: Background CEF input')
        assert 'Submitted: Background CEF input' in command('browser.read', format='markdown')['text']
        snap = command('browser.snapshot')
        password = ref(snap, 'textbox', 'Secret ')
        command('browser.type', ref=password, text='must never be inserted', expect_error='Password fields')
        command('browser.click', ref=password)
        command('browser.press', key='x', expect_error='Password fields')
        creds = work / 'test.env'
        creds.write_text('QA_USER=qa@example.com\nQA_PASS="from env file"\n')
        signin = dict(env_file=str(creds), user_key='QA_USER', password_key='QA_PASS')
        command('browser.signin', site='example.com', expect_error='refusing to fill', **signin)
        assert command('browser.signin', site='127.0.0.1', **signin)['filled'] == ['password']
        command('browser.signin', expect_error='needs site', **signin)
        # A newer snapshot replaces the previous one's refs.
        old = ref(snap, 'button', 'Click me')
        scoped = command('browser.snapshot', scope='main')
        assert 'Bottom of fixture' not in scoped['tree'] and 'Agent control fixture' in scoped['tree']
        command('browser.click', ref=old, expect_error='snapshot again')
        shot = command('browser.screenshot')
        path = Path(shot['path'])
        assert path.is_relative_to(home / 'screenshots') and path.read_bytes().startswith(b'\x89PNG\r\n\x1a\n')
        assert min(struct.unpack('>II', path.read_bytes()[16:24])) > 100, 'Capture has no usable viewport'
        (work / 'page.png').write_bytes(path.read_bytes())
        size = lambda shot: struct.unpack('>II', Path(shot['path']).read_bytes()[16:24])
        whole, half = size(shot), size(command('browser.screenshot', scale=.5))
        assert all(abs(a - 2 * b) <= 2 for a, b in zip(whole, half)), (whole, half)
        # The fixture is taller than any pane: a full capture has to outgrow the viewport.
        assert size(command('browser.screenshot', full=True))[1] > whole[1]
        jpeg = Path(command('browser.screenshot', format='jpeg')['path'])
        assert jpeg.suffix == '.jpg' and jpeg.read_bytes().startswith(b'\xff\xd8')
        command('browser.screenshot', scale=2, expect_error='scale must be')
        # Page-world JavaScript: values, awaited promises, thrown errors, runaway code.
        assert command('browser.eval', js='document.title')['result'] == 'Agent control fixture'
        assert command('browser.eval', js='await Promise.resolve(41) + 1')['result'] == 42
        assert command('browser.eval', js="[...document.links].map(a => a.getAttribute('href'))")['result'] == ['/next']
        assert command('browser.eval', js=[1, 2])['result'] == [1, 2]
        command('browser.eval', js='nope()', expect_error='JavaScript threw: ReferenceError')
        command('browser.eval', js='for (;;) {}', timeout=300, expect_error='did not finish within 300 ms')
        command('browser.eval', js="'x'.repeat(300000)", expect_error='Result exceeds')
        command('browser.eval', expect_error='needs js')
        assert command('browser.eval', js='1 + 1')['result'] == 2, 'the page must survive a stopped script'
        # Each call has its own scope: names neither clash across calls nor leak into the page.
        assert command('browser.eval', js='const reused = 1; reused')['result'] == 1
        assert command('browser.eval', js='let reused = 2; reused')['result'] == 2
        assert command('browser.eval', js='typeof reused')['result'] == 'undefined'
        assert command('browser.eval', js='window.kept = 3; 0')['result'] == 0 and command('browser.eval', js='kept')['result'] == 3
        assert command('browser.eval', js='const v = await Promise.resolve(5); v * 2 // trailing comment')['result'] == 10
        command('browser.snapshot', scope='form#absent', expect_error='No element matches scope')
        # Downloads: fetched by the page, saved under the app home, never overwritten.
        saved = command('browser.download', url='files/asset.bin')
        first = Path(saved['path'])
        assert first.parent == home / 'downloads' and first.name == 'asset.bin' and first.read_bytes() == ASSET, saved
        assert saved['bytes'] == len(ASSET) and saved['status'] == 200 and saved['url'].endswith('/files/asset.bin'), saved
        again = Path(command('browser.download', url=url + 'files/asset.bin')['path'])
        assert again != first and again.read_bytes() == ASSET and first.read_bytes() == ASSET
        dot = command('browser.download', url='data:image/gif;base64,R0lGODlhAQABAAAAACw=')
        assert Path(dot['path']).name.startswith('download') and dot['path'].endswith('.gif') and dot['url'] is None, dot
        command('browser.download', url='missing', expect_error='HTTP 404')
        # Another origin that sends no CORS header stays unreadable to this page.
        command('browser.download', url=f'http://localhost:{server.server_port}/files/asset.bin', expect_error='could not fetch')
        public = command('browser.download', url=f'http://localhost:{server.server_port}/files/asset.bin?public')
        assert Path(public['path']).read_bytes() == ASSET, public
        command('browser.download', expect_error='needs url')
        command('browser.wait', settled=True)
        snap = command('browser.snapshot')
        assert 'InlineTextBox' not in snap['tree'] and 'link "Next page" -> /next' in snap['tree'], snap['tree']
        assert 'image "Dot" -> data:image/gif' in snap['tree'], snap['tree']
        controls = command('browser.snapshot', filter='interactive')['tree']
        assert 'link "Next page" -> /next' in controls and 'StaticText' not in controls and 'heading' not in controls, controls
        snap = command('browser.snapshot')
        command('browser.click', ref=ref(snap, 'textbox', 'Name'))
        command('browser.press', key='Tab')
        command('browser.wait', text='Tab received')
        # The wheel lands mid-page, not on a corner, and says what it moved.
        moved = command('browser.scroll', dy=700)
        command('browser.wait', text='Scroll received')
        # Chromium decides the distance (743.5 px for a 700 px wheel here); the reply is what it measured.
        assert moved['via'] == 'wheel' and moved['moved'] and not moved['at_end'] and 500 < moved['dy'] < 900 and moved['dy'] <= moved['top'] < moved['max_top'], moved
        assert 'Box scrolled' not in command('browser.read')['text']
        end = command('browser.scroll', dy=100000)
        assert end['moved'] and end['at_end'], end
        stuck = command('browser.scroll', dy=500)
        assert stuck['moved'] is False and stuck['dy'] == 0 and stuck['at_end'], stuck
        assert command('browser.scroll', dy=-100000)['top'] == 0
        # With an element, the wheel goes to the scroller around it, not the page,
        # even straight after the page's own wheel (Chromium would latch onto the page).
        inner = command('browser.scroll', name='Inner top', dy=200)
        command('browser.wait', text='Box scrolled')
        assert inner['moved'] and not inner['at_end'] and 100 < inner['top'] < 300 < inner['max_top'] < 1000, inner
        command('browser.scroll', expect_error='Pass ref or name')
        command('browser.scroll', name='Inner top')
        person = command('browser.open', profile='person', url=url, caller='qa-terminal-b')
        command('browser.read', pane=person['pane'], expect_error='person-profile')
        command('browser.eval', pane=person['pane'], js='document.title', expect_error='person-profile')
        command('browser.download', pane=person['pane'], url='files/asset.bin', expect_error='person-profile')
        # ls finds both browsers again, but shows only the agent page's address.
        listed = {b['pane']: b for s in json.loads(subprocess.run([binary, 'ls'], env=env, text=True, capture_output=True).stdout)['spaces'] for tab in s['tabs'] for b in tab['browsers']}
        assert listed[pane]['profile'] == 'agent' and listed[pane]['url'].startswith(url) and listed[pane]['title'] == 'Agent control fixture', listed
        assert listed[person['pane']] == {'pane': person['pane'], 'profile': 'person'}, listed
        command('browser.navigate', pane=person['pane'], url=url, expect_error='person-profile')
        # Explicit agent target continues to work even while person pane is selected.
        assert command('browser.read')['pane'] == pane
        command('browser.inspect', pane=person['pane'], confirmed=True)
        command('browser.wait', pane=person['pane'], confirmed=True, load=True)
        assert command('browser.eval', pane=person['pane'], confirmed=True, js='document.title')['result'] == 'Agent control fixture'
        assert 'profile_marker=agent-only' not in command('browser.read', pane=person['pane'], confirmed=True)['text']
        command('browser.read', caller='unknown', expect_error='Pass pane explicitly')
        # Navigate invalidates refs even when URL returns to the same page.
        command('browser.navigate', url=url + 'next')
        command('browser.wait', text='Navigation complete')
        command('browser.click', ref=old, expect_error='snapshot again')
        # A browser split into a tab nobody is viewing still starts and takes commands.
        listing = json.loads(subprocess.run([binary, 'ls'], env=env, text=True, capture_output=True).stdout)
        scratch = next(t['pane'] for s in listing['spaces'] if s['id'] == space['space'] for tab in s['tabs'] if tab['name'] == 'Scratch terminal' for t in tab['terminals'])
        command('tab.new', name='Elsewhere', space=space['space'])
        hidden = command('browser.split', pane=scratch, profile='agent', url=url, caller='qa-terminal-c')['pane']
        deadline = time.monotonic() + 30
        while True:
            time.sleep(.1)
            try:
                if command('browser.inspect', pane=hidden)['title'] == 'Agent control fixture': break
            except AssertionError:
                pass
            assert time.monotonic() < deadline, f'Hidden browser did not start: {work}'
        command('browser.click', pane=hidden, name='Click me')
        # A window on screen stops rendering a page nobody is viewing and a wheel stalls there,
        # so such a page is scrolled by script. This background window never hides one: ask for it.
        assert command('browser.scroll', pane=hidden, dy=300)['via'] == 'wheel'
        command('browser.scroll', pane=hidden, dy=-100000, via='script')
        unseen = command('browser.scroll', pane=hidden, dy=300, via='script')
        assert unseen['via'] == 'script' and unseen['moved'] and unseen['top'] == 300 and not unseen['at_end'], unseen
        assert command('browser.scroll', pane=hidden, dy=100000, via='script')['at_end']
        boxed = command('browser.scroll', pane=hidden, name='Inner top', dy=200, via='script')
        assert boxed['via'] == 'script' and boxed['top'] == 200 < boxed['max_top'] < 1000, boxed
        command('browser.scroll', pane=hidden, dy=10, via='teleport', expect_error='via must be')
        command('browser.scroll', pane=hidden, name='Inner top', via='script', expect_error='via applies')
        command('pane.close', pane=hidden)
        command('pane.close', pane=pane)
        time.sleep(.2)
        command('browser.read', expect_error='No native browser')
        result = {'ok': True, 'tcp_listeners': listeners, 'binary': binary, 'work': str(work), 'home': str(home), 'session': env['HC_SESSION'], 'front_before': before, 'front_after': front(), 'foreground_audit': foreground_audit.copy(), 'inspection': state, 'screenshot': str(work / 'page.png'), 'commands': transcript}
        (work / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
        print(json.dumps({k: v for k, v in result.items() if k != 'commands'}, indent=2), flush=True)
    finally:
        (work / 'foreground-audit.json').write_text(json.dumps(foreground_audit, indent=2))
        (work / 'transcript.json').write_text(json.dumps(transcript, indent=2))
        if app.poll() is None:
            try:
                if space: command('space.close', id=space['space'], confirmed=True)
                command('app.quit')
                app.wait(timeout=10)
            except Exception:
                app.terminate(); app.wait(timeout=10)
        # Stopping the core retains kept terminal records. End only this test's
        # terminal first, even if the preview failed before it could close space.
        cleanup = {}
        if space:
            terminal = subprocess.run([str(Path.home() / '.local/bin/hc'), 'kill', space['terminal'], '--json'], env=env, capture_output=True, text=True, timeout=10)
            cleanup['terminal'] = {'code': terminal.returncode, 'stdout': terminal.stdout, 'stderr': terminal.stderr}
        stopped = subprocess.run([str(Path.home() / '.local/bin/hc'), 'server', 'stop', '--json'], env=env, capture_output=True, text=True, timeout=10)
        cleanup['core'] = {'code': stopped.returncode, 'stdout': stopped.stdout, 'stderr': stopped.stderr}
        (work / 'cleanup.json').write_text(json.dumps(cleanup, indent=2))
        assert stopped.returncode == 0, cleanup
        server.shutdown()
        log.close()
        print(f'Evidence: {work}', flush=True)


if __name__ == '__main__': main()
