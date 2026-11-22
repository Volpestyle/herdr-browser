#!/usr/bin/env python3
"""Package a public stock-CEF Apple Silicon release of Herdr Browser."""
from __future__ import annotations

import argparse
import base64
import contextlib
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import shutil
import subprocess
import sys
import tarfile
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import browser_update as updater

PUBLIC_BUNDLE_ID = 'dev.volpestyle.herdr-browser'
ARCHIVE_NAME = 'herdr-browser-macos-aarch64.tar.gz'
DMG_NAME = 'Herdr-Browser-macos-aarch64.dmg'


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open('rb') as handle:
        while chunk := handle.read(1 << 20):
            digest.update(chunk)
    return digest.hexdigest()


def cargo_version(source: Path) -> str:
    text = (source / 'Cargo.toml').read_text()
    match = re.search(r'(?m)^version\s*=\s*"([^"]+)"', text)
    if not match:
        raise RuntimeError('Cannot read version from Cargo.toml')
    return match[1]


@contextlib.contextmanager
def temporary_signing_keychain(p12_b64: str, p12_password: str):
    """Import a base64 Developer ID .p12 into a temporary keychain and yield (identity, keychain_path)."""
    work = Path(tempfile.mkdtemp(prefix='herdr-browser-sign-'))
    keychain = work / 'signing.keychain-db'
    existing = updater.run(['security', 'list-keychains', '-d', 'user'], capture=True).stdout.decode()
    searched = [line.strip().strip('"') for line in existing.splitlines() if line.strip()]
    try:
        p12_path = work / 'certificate.p12'
        p12_path.write_bytes(base64.b64decode(p12_b64))
        p12_path.chmod(0o600)
        kc_password = secrets.token_hex(32)
        updater.run(['security', 'create-keychain', '-p', kc_password, keychain])
        updater.run(['security', 'set-keychain-settings', '-lut', '21600', keychain])
        updater.run(['security', 'unlock-keychain', '-p', kc_password, keychain])
        updater.run(['security', 'import', p12_path, '-k', keychain, '-P', p12_password, '-T', '/usr/bin/codesign'])
        updater.run(['security', 'set-key-partition-list', '-S', 'apple-tool:,apple:', '-s', '-k', kc_password, keychain],
                    capture=True)
        updater.run(['security', 'list-keychains', '-d', 'user', '-s', keychain, *searched])
        identities = updater.run(['security', 'find-identity', '-v', '-p', 'codesigning', keychain],
                                 capture=True).stdout.decode()
        identity = None
        for line in identities.splitlines():
            if '"Developer ID Application:' in line:
                parts = line.split()
                if len(parts) >= 2:
                    identity = parts[1]
                    break
        if not identity:
            raise RuntimeError('No Developer ID Application identity found in APPLE_CERTIFICATE_P12')
        yield identity, keychain
    finally:
        updater.run(['security', 'list-keychains', '-d', 'user', '-s', *searched], check=False, capture=True)
        updater.run(['security', 'delete-keychain', keychain], check=False, capture=True)
        shutil.rmtree(work, ignore_errors=True)


def notarize_path(target: Path, *, api_key: str, key_id: str, issuer: str, staple_target: Path | None = None):
    """Submit a .zip or .dmg to Apple's notarization service and staple the ticket."""
    with tempfile.TemporaryDirectory(prefix='herdr-browser-notary-') as temp:
        work = Path(temp)
        key_file = work / 'AuthKey.p8'
        key_file.write_text(api_key)
        key_file.chmod(0o600)
        result = subprocess.run(
            ['xcrun', 'notarytool', 'submit', str(target),
             '--key', str(key_file), '--key-id', key_id, '--issuer', issuer,
             '--wait', '--timeout', '30m', '--output-format', 'json'],
            capture_output=True, text=True, check=False,
        )
        submission = {}
        try:
            submission = json.loads(result.stdout)
        except json.JSONDecodeError:
            pass
        if result.returncode != 0 or submission.get('status') != 'Accepted':
            sub_id = submission.get('id')
            if sub_id:
                log_path = work / 'notary-log.json'
                subprocess.run(
                    ['xcrun', 'notarytool', 'log', sub_id,
                     '--key', str(key_file), '--key-id', key_id, '--issuer', issuer, str(log_path)],
                    check=False,
                )
                if log_path.exists():
                    print(log_path.read_text(), file=sys.stderr)
            raise RuntimeError(f'Notarization failed for {target.name}: {result.stdout or result.stderr}')
    to_staple = staple_target or target
    updater.run(['xcrun', 'stapler', 'staple', to_staple])
    updater.run(['xcrun', 'stapler', 'validate', to_staple])


def create_tarball(app: Path, destination: Path):
    """Create a tar.gz containing Herdr Browser.app while preserving macOS symlinks and permissions."""
    destination.parent.mkdir(parents=True, exist_ok=True)
    # Use tar directly on macOS with COPYFILE_DISABLE=1 to avoid AppleDouble ._ entries.
    env = os.environ.copy()
    env['COPYFILE_DISABLE'] = '1'
    updater.run(['tar', '-czf', destination, '-C', app.parent, app.name], env=env)


def create_dmg(app: Path, source: Path, destination: Path, *, sign_identity: str, keychain: Path | None = None):
    """Create and sign a drag-and-drop macOS DMG for Herdr Browser.app."""
    destination.parent.mkdir(parents=True, exist_ok=True)
    if destination.exists():
        destination.unlink()
    with tempfile.TemporaryDirectory(prefix='herdr-browser-dmg-') as temp:
        work = Path(temp).resolve()
        stage = work / 'source'
        stage.mkdir()
        updater.run(['ditto', app, stage / app.name])
        cmd = [
            'create-dmg',
            '--volname', 'Herdr Browser',
            '--volicon', str(source / 'assets/icons/Herdr.icns'),
            '--window-size', '660', '400',
            '--icon-size', '128',
            '--icon', app.name, '165', '190',
            '--hide-extension', app.name,
            '--app-drop-link', '495', '190',
        ]
        bg1 = source / 'distribution/macos/dmg/background.png'
        bg2 = source / 'distribution/macos/dmg/background@2x.png'
        if bg1.exists() and bg2.exists() and shutil.which('tiffutil'):
            bg_tiff = work / 'background.tiff'
            updater.run(['tiffutil', '-cathidpicheck', bg1, bg2, '-out', bg_tiff])
            cmd += ['--background', str(bg_tiff)]
        staged_dmg = work / DMG_NAME
        cmd += [str(staged_dmg), str(stage)]
        updater.run(cmd)
        sign_cmd = ['/usr/bin/codesign', '--force', '--timestamp']
        if keychain:
            sign_cmd += ['--keychain', str(keychain)]
        sign_cmd += ['--sign', sign_identity, staged_dmg]
        updater.run(sign_cmd)
        updater.run(['/usr/bin/codesign', '--verify', '--strict', staged_dmg])
        shutil.move(str(staged_dmg), str(destination))


def write_checksums(out_dir: Path, files: list[Path]) -> Path:
    checksums = out_dir / 'SHA256SUMS'
    lines = [f'{sha256_file(path)}  {path.name}' for path in files]
    checksums.write_text('\n'.join(lines) + '\n')
    return checksums


def build_release(*, binary: Path, source: Path, out_dir: Path, version: str,
                  bundle_id: str = PUBLIC_BUNDLE_ID, sign_identity: str | None = None,
                  allow_adhoc: bool = False, dmg: bool = False, notarize: bool = False) -> dict:
    """Build stock-CEF Herdr Browser release artifacts into out_dir."""
    p12_b64 = os.environ.get('APPLE_CERTIFICATE_P12', '').strip()
    p12_password = os.environ.get('APPLE_CERTIFICATE_PASSWORD', '')
    api_key = os.environ.get('APPLE_API_KEY', '').strip()
    api_key_id = os.environ.get('APPLE_API_KEY_ID', '').strip()
    api_issuer = os.environ.get('APPLE_API_ISSUER', '').strip()
    has_notary = bool(api_key and api_key_id and api_issuer)

    if notarize and not has_notary:
        raise RuntimeError('Notarization requested but APPLE_API_KEY / APPLE_API_KEY_ID / APPLE_API_ISSUER are not set')

    try:
        commit = updater.git('rev-parse', 'HEAD', cwd=source)
    except Exception:
        commit = 'release'

    out_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='herdr-browser-release-') as temp:
        work = Path(temp)
        app = work / 'Herdr Browser.app'

        ctx = (temporary_signing_keychain(p12_b64, p12_password)
               if (not sign_identity and p12_b64)
               else contextlib.nullcontext((sign_identity, None)))

        with ctx as (resolved_identity, keychain):
            if not resolved_identity:
                if not allow_adhoc:
                    raise RuntimeError(
                        'No Developer ID signing identity configured (set APPLE_CERTIFICATE_P12 + '
                        'APPLE_CERTIFICATE_PASSWORD, pass --sign-identity, or pass --allow-adhoc for a dry run)'
                    )
                resolved_identity = '-'
            hardened = resolved_identity != '-'
            codecs = updater.package(
                binary, app, version, source, commit,
                bundle_id=bundle_id,
                stock_only=True,
                sign_identity=resolved_identity,
                hardened_runtime=hardened,
                timestamp=hardened,
                keychain=keychain,
            )

            if notarize:
                if not hardened:
                    raise RuntimeError('Cannot notarize an ad-hoc signed app')
                notary_zip = work / 'Herdr-Browser-notary.zip'
                updater.run(['ditto', '-c', '-k', '--keepParent', app, notary_zip])
                notarize_path(notary_zip, api_key=api_key, key_id=api_key_id, issuer=api_issuer, staple_target=app)

            tarball = out_dir / ARCHIVE_NAME
            create_tarball(app, tarball)
            artifacts = [tarball]

            dmg_path = None
            if dmg:
                if not hardened:
                    raise RuntimeError('DMG creation requires a Developer ID signed app')
                dmg_path = out_dir / DMG_NAME
                create_dmg(app, source, dmg_path, sign_identity=resolved_identity, keychain=keychain)
                if notarize:
                    notarize_path(dmg_path, api_key=api_key, key_id=api_key_id, issuer=api_issuer)
                    updater.run(['spctl', '--assess', '--type', 'open', '--context', 'context:primary-signature',
                                 '--verbose=2', dmg_path])
                artifacts.append(dmg_path)

            checksums = write_checksums(out_dir, artifacts)

    return {
        'version': version,
        'bundle_id': bundle_id,
        'codecs': codecs,
        'signed': resolved_identity != '-',
        'notarized': notarize,
        'tarball': str(tarball),
        'dmg': str(dmg_path) if dmg_path else None,
        'checksums': str(checksums),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=updater.ROOT / 'target/release/herdr-app')
    parser.add_argument('--source', type=Path, default=updater.ROOT)
    parser.add_argument('--out-dir', type=Path, default=updater.ROOT / 'dist')
    parser.add_argument('--version', help='App version (defaults to Cargo.toml)')
    parser.add_argument('--bundle-id', default=PUBLIC_BUNDLE_ID)
    parser.add_argument('--sign-identity', default=os.environ.get('HERDR_BROWSER_SIGN_IDENTITY') or None)
    parser.add_argument('--allow-adhoc', action='store_true', help='Allow ad-hoc signing for dry-run builds')
    parser.add_argument('--dmg', action='store_true', help='Also create a signed macOS DMG')
    parser.add_argument('--notarize', action='store_true', help='Notarize and staple with Apple notarytool')
    args = parser.parse_args()

    version = args.version or cargo_version(args.source)
    summary = build_release(
        binary=args.binary,
        source=args.source,
        out_dir=args.out_dir,
        version=version,
        bundle_id=args.bundle_id,
        sign_identity=args.sign_identity,
        allow_adhoc=args.allow_adhoc,
        dmg=args.dmg,
        notarize=args.notarize,
    )
    print(json.dumps(summary, indent=2))
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f'package_release: {error}', file=sys.stderr)
        raise SystemExit(1)
