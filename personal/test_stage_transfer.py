import importlib.util
import io
import json
from pathlib import Path
import plistlib
import tarfile
import tempfile
import unittest

def load_module(name: str):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(f'{name}.py'))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod

recv = load_module('stage_receive')
push = load_module('stage_push')


def make_meta(**overrides) -> dict:
    base = {
        'tag': 'v0.1.16',
        'commit': '0' * 40,
        'patch': 'a' * 64,
        'engine': '156.7.0+156.0.31',
        'codecs': {'h264': True, 'cef': '156.0.31', 'sha256': 'b' * 64},
        'built': '2026-11-22T22:45:50.074902+00:00',
    }
    base.update(overrides)
    return base


def make_valid_app(root: Path, meta: dict) -> Path:
    app = root / 'Herdr Browser.app'
    (app / 'Contents/MacOS').mkdir(parents=True)
    (app / 'Contents/Frameworks/Chromium Embedded Framework.framework/Versions/A').mkdir(parents=True)
    (app / 'Contents/MacOS/HerdrBrowser').write_bytes(b'binary')
    (app / 'Contents/Frameworks/Chromium Embedded Framework.framework/Versions/Current').symlink_to('A')
    info = {
        'CFBundleIdentifier': 'dev.james.herdr-browser',
        'CFBundleShortVersionString': meta['tag'][1:],
        'HerdrBrowserPatch': meta['patch'],
    }
    for key in recv.BROWSER_USAGE_DESCRIPTIONS:
        info[key] = 'test'
    with (app / 'Contents/Info.plist').open('wb') as f:
        plistlib.dump(info, f)
    return app


def tar_bytes(entries: list[tuple[tarfile.TarInfo, bytes | None]]) -> io.BytesIO:
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode='w:gz') as tf:
        for info, data in entries:
            if data is not None:
                info.size = len(data)
                tf.addfile(info, io.BytesIO(data))
            else:
                tf.addfile(info)
    buf.seek(0)
    return buf


class StageTransferTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.local = self.root / 'updater'
        self.local.mkdir()

    def test_disallowed_commands_rejected(self):
        for bad in ('sh', 'cat /etc/passwd', 'stage --force', 'status; id', 'rm -rf /'):
            with self.assertRaises(recv.StageError):
                recv.dispatch(bad, io.BytesIO(b''), local_dir=self.local)

    def test_status_is_read_only(self):
        recv.write_json(self.local / 'state.json', {
            'status': 'installed',
            'target': 'v0.1.15',
            'installed': {'tag': 'v0.1.15'},
            'pending': None,
            'secret_ignored': 'should-not-leak',
        })
        out = recv.dispatch('status', io.BytesIO(b''), local_dir=self.local)
        self.assertTrue(out['ok'])
        self.assertEqual(out['target'], 'v0.1.15')
        self.assertNotIn('secret_ignored', out)

    def test_valid_stream_stages_and_preserves_internal_symlinks(self):
        meta = make_meta()
        app = make_valid_app(self.root / 'src', meta)
        buf = io.BytesIO()
        push.stream_bundle_tar(app, meta, buf)
        buf.seek(0)

        verified = []
        result = recv.receive_stage(
            buf,
            local_dir=self.local,
            req_provider=lambda: 'identifier "dev.james.herdr-browser"',
            verifier=lambda bundle, req: verified.append((bundle.name, req)),
        )
        self.assertTrue(result['ok'])
        self.assertEqual(verified, [('Herdr Browser.app', 'identifier "dev.james.herdr-browser"')])
        staged = Path(result['staged']['bundle'])
        self.assertTrue(staged.is_dir())
        sym = staged / 'Contents/Frameworks/Chromium Embedded Framework.framework/Versions/Current'
        self.assertTrue(sym.is_symlink())
        self.assertEqual(sym.readlink(), Path('A'))
        state = json.loads((self.local / 'state.json').read_text())
        self.assertEqual(state['status'], 'staged')
        self.assertEqual(state['pending']['tag'], 'v0.1.16')

    def test_signature_failure_cleans_up_temp_files(self):
        meta = make_meta()
        app = make_valid_app(self.root / 'src', meta)
        buf = io.BytesIO()
        push.stream_bundle_tar(app, meta, buf)
        buf.seek(0)

        def fail_verify(_bundle, _req):
            raise recv.StageError('bad signature')

        with self.assertRaises(recv.StageError):
            recv.receive_stage(
                buf,
                local_dir=self.local,
                req_provider=lambda: 'req',
                verifier=fail_verify,
            )
        self.assertEqual(list((self.local / 'builds').iterdir()), [])
        self.assertFalse((self.local / 'state.json').exists())

    def test_refuses_downgrade(self):
        recv.write_json(self.local / 'state.json', {
            'status': 'installed',
            'installed': make_meta(tag='v0.1.17'),
        })
        meta = make_meta(tag='v0.1.16')
        app = make_valid_app(self.root / 'src', meta)
        buf = io.BytesIO()
        push.stream_bundle_tar(app, meta, buf)
        buf.seek(0)

        with self.assertRaisesRegex(recv.StageError, 'downgrade'):
            recv.receive_stage(
                buf,
                local_dir=self.local,
                req_provider=lambda: 'req',
                verifier=lambda _b, _r: None,
            )

    def test_rejects_path_traversal_and_symlink_escape(self):
        meta_bytes = json.dumps(make_meta()).encode()
        meta_info = tarfile.TarInfo('metadata.json')

        # 1. Top-level traversal
        evil = tarfile.TarInfo('../evil.txt')
        buf = tar_bytes([(meta_info, meta_bytes), (evil, b'pwn')])
        with self.assertRaisesRegex(recv.StageError, 'Path traversal'):
            recv.receive_stage(buf, local_dir=self.local, req_provider=lambda: 'req', verifier=lambda _b, _r: None)

        # 2. Symlink escaping bundle
        sym = tarfile.TarInfo('Herdr Browser.app/Contents/escape')
        sym.type = tarfile.SYMTYPE
        sym.linkname = '../../../.ssh/authorized_keys'
        buf2 = tar_bytes([(meta_info, meta_bytes), (sym, None)])
        with self.assertRaisesRegex(recv.StageError, 'escapes'):
            recv.receive_stage(buf2, local_dir=self.local, req_provider=lambda: 'req', verifier=lambda _b, _r: None)

        # 3. Setuid bit refused
        suid = tarfile.TarInfo('Herdr Browser.app/Contents/MacOS/HerdrBrowser')
        suid.mode = 0o4755
        buf3 = tar_bytes([(meta_info, meta_bytes), (suid, b'bin')])
        with self.assertRaisesRegex(recv.StageError, 'Setuid'):
            recv.receive_stage(buf3, local_dir=self.local, req_provider=lambda: 'req', verifier=lambda _b, _r: None)

        # 4. Chained symlink traversal (each symlink looks lexical-safe on its own)
        s1 = tarfile.TarInfo('Herdr Browser.app/a/b')
        s1.type = tarfile.SYMTYPE
        s1.linkname = '..'
        s2 = tarfile.TarInfo('Herdr Browser.app/a/b/c')
        s2.type = tarfile.SYMTYPE
        s2.linkname = '..'
        write_through = tarfile.TarInfo('Herdr Browser.app/a/b/c/escaped.txt')
        buf4 = tar_bytes([(meta_info, meta_bytes), (s1, None), (s2, None), (write_through, b'nope')])
        with self.assertRaisesRegex(recv.StageError, 'traverses symlink'):
            recv.receive_stage(buf4, local_dir=self.local, req_provider=lambda: 'req', verifier=lambda _b, _r: None)
        self.assertFalse((self.local / 'escaped.txt').exists())

        # 5. Multi-hop symlink chain resolving outside bundle without traversing symlink prefix
        h1 = tarfile.TarInfo('Herdr Browser.app/d1/up')
        h1.type = tarfile.SYMTYPE
        h1.linkname = '..'
        h2 = tarfile.TarInfo('Herdr Browser.app/d2/escape')
        h2.type = tarfile.SYMTYPE
        h2.linkname = '../d1/up/..'
        buf5 = tar_bytes([(meta_info, meta_bytes), (h1, None), (h2, None)])
        with self.assertRaisesRegex(recv.StageError, 'escapes'):
            recv.receive_stage(buf5, local_dir=self.local, req_provider=lambda: 'req', verifier=lambda _b, _r: None)

        # 6. Duplicate archive member refused
        f1 = tarfile.TarInfo('Herdr Browser.app/Contents/dup')
        f2 = tarfile.TarInfo('Herdr Browser.app/Contents/dup')
        buf6 = tar_bytes([(meta_info, meta_bytes), (f1, b'one'), (f2, b'two')])
        with self.assertRaisesRegex(recv.StageError, 'Duplicate'):
            recv.receive_stage(buf6, local_dir=self.local, req_provider=lambda: 'req', verifier=lambda _b, _r: None)

    def test_real_codesign_verify_on_installed_app_if_present(self):
        installed = Path.home() / 'Applications' / 'Herdr Browser.app'
        if not installed.is_dir():
            self.skipTest('No installed Herdr Browser.app on this machine')
        req = recv.expected_requirement(pin_file=self.root / 'missing', installed_app=installed)
        self.assertIn('dev.james.herdr-browser', req)
        recv.verify_signature(installed, req)
        with self.assertRaises(recv.StageError):
            recv.verify_signature(installed, 'identifier "com.example.wrong-app"')


if __name__ == '__main__':
    unittest.main()
