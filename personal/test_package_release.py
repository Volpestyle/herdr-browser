import hashlib
import http.server
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import socketserver
import subprocess
import sys
import tarfile
import tempfile
import threading
import unittest
from unittest.mock import patch

PERSONAL = Path(__file__).resolve().parent
sys.path.insert(0, str(PERSONAL))

import browser_update as updater
import package_release as rel


def make_fake_cef_tree(root: Path, cef_version: str) -> Path:
    cef_dir = root / 'cef'
    fw = cef_dir / updater.CEF_FRAMEWORK
    (fw / 'Resources').mkdir(parents=True)
    (fw / 'Libraries').mkdir(parents=True)
    (fw / 'Helpers').mkdir(parents=True)
    with (fw / 'Resources/Info.plist').open('wb') as out:
        plistlib.dump({'CFBundleShortVersionString': f'{cef_version}.0'}, out)
    (fw / 'Libraries/libEGL.dylib').write_bytes(b'fake-dylib')
    (fw / 'Helpers/chrome_crashpad_handler').write_bytes(b'fake-helper')
    (cef_dir / 'CREDITS.html').write_text('<html>credits</html>')
    return cef_dir


def make_fake_source(root: Path) -> Path:
    source = root / 'source'
    (source / 'assets/icons').mkdir(parents=True)
    (source / 'assets/icons/Herdr.icns').write_bytes(b'icns')
    (source / 'LICENSE').write_text('Apache-2.0\n')
    (source / 'THIRD-PARTY-NOTICES').write_text('Notices\n')
    (source / 'Cargo.toml').write_text('[package]\nname = "herdr-app"\nversion = "0.1.12"\n')
    (source / 'personal').mkdir(parents=True)
    (source / 'personal/browser_open.py').write_text((PERSONAL / 'browser_open.py').read_text())
    return source


def make_fake_binaries(root: Path, cef_dir: Path) -> Path:
    bin_dir = root / 'target/release'
    bin_dir.mkdir(parents=True)
    app_bin = bin_dir / 'herdr-app'
    app_bin.write_text(
        f'#!{sys.executable}\n'
        'import json, os, sys\n'
        'from pathlib import Path\n'
        'if sys.argv[1:] == ["commands"]:\n'
        '    print(json.dumps([{"name":"browser.open"},{"name":"browser.navigate"},{"name":"browser.inspect"}]))\n'
        'elif sys.argv[1:] == ["status"]:\n'
        '    raise SystemExit(0)\n'
        'elif os.environ.get("HERDR_TEST_CAPTURE"):\n'
        '    Path(os.environ["HERDR_TEST_CAPTURE"]).write_text(json.dumps({"argv": sys.argv[1:], "home": os.environ.get("HERDR_APP_HOME")}))\n'
    )
    app_bin.chmod(0o755)
    helper = bin_dir / 'herdr-browser-helper'
    helper.write_text(f'#!{sys.executable}\nprint({str(cef_dir)!r})\n')
    helper.chmod(0o755)
    return app_bin


class StockCefAndSigningTests(unittest.TestCase):
    def test_stock_only_ignores_missing_personal_codec_archive_at_pinned_version(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            cef_dir = make_fake_cef_tree(root, updater.CEF_CODEC_VERSION)
            frameworks = root / 'Frameworks'
            frameworks.mkdir()
            with patch.object(updater, 'CEF_CODEC_ARCHIVE', root / 'missing-codecs.tar.bz2'):
                result = updater.install_cef_framework(cef_dir / updater.CEF_FRAMEWORK, frameworks, stock_only=True)
            self.assertEqual(result, {'h264': False, 'cef': updater.CEF_CODEC_VERSION, 'stock': True})
            self.assertTrue((frameworks / updater.CEF_FRAMEWORK / 'Resources/Info.plist').is_file())

    def test_sign_bundle_inside_out_orders_dylibs_framework_helpers_then_app(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            app = root / 'Herdr Browser.app'
            cef_dir = make_fake_cef_tree(root, '156.0.31')
            frameworks = app / 'Contents/Frameworks'
            frameworks.mkdir(parents=True)
            updater.install_cef_framework(cef_dir / updater.CEF_FRAMEWORK, frameworks, stock_only=True)
            (frameworks / 'HerdrBrowser Helper.app/Contents/MacOS').mkdir(parents=True)
            (frameworks / 'HerdrBrowser Helper (GPU).app/Contents/MacOS').mkdir(parents=True)
            entitlements = root / 'entitlements.plist'
            entitlements.write_text('<plist/>')

            calls = []

            def fake_run(args, **kwargs):
                calls.append([str(a) for a in args])
                return subprocess.CompletedProcess(args, 0, stdout=b'')

            with patch.object(updater, 'run', side_effect=fake_run):
                updater.sign_bundle_inside_out(app, 'Developer ID Application: Test', entitlements,
                                               hardened_runtime=True, timestamp=True, keychain=root / 'kc')

            targets = [call[-1] for call in calls]
            self.assertTrue(targets[0].endswith('libEGL.dylib'))
            self.assertTrue(targets[1].endswith('chrome_crashpad_handler'))
            self.assertTrue(targets[2].endswith(updater.CEF_FRAMEWORK))
            self.assertTrue(targets[3].endswith('HerdrBrowser Helper (GPU).app'))
            self.assertTrue(targets[4].endswith('HerdrBrowser Helper.app'))
            self.assertEqual(targets[-1], str(app))
            for call in calls:
                self.assertIn('--options', call)
                self.assertIn('runtime', call)
                self.assertIn('--timestamp', call)
                self.assertIn('--keychain', call)
            self.assertIn('--entitlements', calls[-1])
            self.assertNotIn('--entitlements', calls[0])


class PublicReleasePackageTests(unittest.TestCase):
    def test_build_release_refuses_unsigned_without_allow_adhoc(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            cef_dir = make_fake_cef_tree(root, updater.CEF_CODEC_VERSION)
            source = make_fake_source(root)
            binary = make_fake_binaries(root, cef_dir)
            with patch.dict(os.environ, {'APPLE_CERTIFICATE_P12': '', 'APPLE_CERTIFICATE_PASSWORD': ''}, clear=False):
                with self.assertRaisesRegex(RuntimeError, 'No Developer ID signing identity'):
                    rel.build_release(binary=binary, source=source, out_dir=root / 'dist', version='0.1.12')

    def test_build_release_adhoc_bundles_opener_and_writes_checksums(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            cef_dir = make_fake_cef_tree(root, updater.CEF_CODEC_VERSION)
            source = make_fake_source(root)
            binary = make_fake_binaries(root, cef_dir)
            out_dir = root / 'dist'

            real_run = updater.run

            def fake_run(args, **kwargs):
                if args and str(args[0]) == '/usr/bin/codesign':
                    return subprocess.CompletedProcess(args, 0, stdout=b'')
                return real_run(args, **kwargs)

            with patch.object(updater, 'run', side_effect=fake_run), \
                 patch.dict(os.environ, {'APPLE_CERTIFICATE_P12': '', 'APPLE_CERTIFICATE_PASSWORD': ''}, clear=False):
                summary = rel.build_release(
                    binary=binary, source=source, out_dir=out_dir, version='0.1.12', allow_adhoc=True,
                )

            tarball = out_dir / rel.ARCHIVE_NAME
            checksums = out_dir / 'SHA256SUMS'
            self.assertTrue(tarball.is_file())
            self.assertTrue(checksums.is_file())
            self.assertEqual(summary['codecs']['stock'], True)
            self.assertFalse(summary['signed'])
            expected = hashlib.sha256(tarball.read_bytes()).hexdigest()
            self.assertEqual(checksums.read_text().strip(), f'{expected}  {rel.ARCHIVE_NAME}')

            with tarfile.open(tarball, 'r:gz') as archive:
                names = set(archive.getnames())
            self.assertIn('Herdr Browser.app/Contents/Resources/browser_open.py', names)
            self.assertIn('Herdr Browser.app/Contents/MacOS/HerdrBrowser', names)
            self.assertIn('Herdr Browser.app/Contents/Info.plist', names)


class PublicInstallerTests(unittest.TestCase):
    def test_install_sh_syntax_is_valid(self):
        result = subprocess.run(['sh', '-n', str(PERSONAL / 'install.sh')], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)

    @unittest.skipUnless(sys.platform == 'darwin', 'macOS-only installer integration test')
    def test_install_sh_installs_standalone_bundle_and_launchers(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            cef_dir = make_fake_cef_tree(root, updater.CEF_CODEC_VERSION)
            source = make_fake_source(root)
            binary = make_fake_binaries(root, cef_dir)
            dist = root / 'dist'

            with patch.dict(os.environ, {'APPLE_CERTIFICATE_P12': '', 'APPLE_CERTIFICATE_PASSWORD': ''}, clear=False):
                rel.build_release(binary=binary, source=source, out_dir=dist, version='0.1.12', allow_adhoc=True)

            class QuietHandler(http.server.SimpleHTTPRequestHandler):
                def __init__(self, *args, **kwargs):
                    super().__init__(*args, directory=str(dist), **kwargs)

                def log_message(self, format, *args):
                    pass

            with socketserver.TCPServer(('127.0.0.1', 0), QuietHandler) as server:
                port = server.server_address[1]
                thread = threading.Thread(target=server.serve_forever, daemon=True)
                thread.start()
                try:
                    fake_home = root / 'home'
                    fake_bin = fake_home / '.local/bin'
                    fake_bin.mkdir(parents=True)
                    # Pre-populate herdr-core so the test stays fully offline.
                    (fake_bin / 'herdr-core').write_text('#!/bin/sh\nexit 0\n')
                    (fake_bin / 'herdr-core').chmod(0o755)
                    env = dict(
                        os.environ,
                        HOME=str(fake_home),
                        HERDR_BROWSER_BASE_URL=f'http://127.0.0.1:{port}',
                        HERDR_BROWSER_VERSION='browser-v0.1.12',
                        PATH=f'{fake_bin}:{os.environ.get("PATH", "")}',
                    )
                    result = subprocess.run(['sh', str(PERSONAL / 'install.sh')], env=env,
                                            capture_output=True, text=True)
                    self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                finally:
                    server.shutdown()

            installed_app = fake_home / 'Applications/Herdr Browser.app'
            self.assertTrue((installed_app / 'Contents/MacOS/HerdrBrowser').is_file())
            self.assertTrue((installed_app / 'Contents/Resources/browser_open.py').is_file())
            self.assertTrue((fake_bin / 'herdr-browser').is_file())
            self.assertTrue((fake_bin / 'herdr-browser-open').is_file())
            # Standalone opener must point into the app bundle, never a git checkout.
            self.assertIn(str(installed_app / 'Contents/Resources/browser_open.py'),
                          (fake_bin / 'herdr-browser-open').read_text())

            capture = root / 'capture.json'
            url = 'https://example.test/docs'
            open_env = dict(os.environ, HOME=str(fake_home), HERDR_TEST_CAPTURE=str(capture))
            subprocess.run([str(fake_bin / 'herdr-browser-open'), url], env=open_env, check=True)
            recorded = json.loads(capture.read_text())
            self.assertEqual(recorded['argv'], ['do', 'browser.open', '--url', url])
            self.assertEqual(recorded['home'], str(fake_home / '.config/herdr-app-browser'))


if __name__ == '__main__':
    unittest.main()
