#!/usr/bin/env python3
"""Forced-command SSH receiver that stages ONLY signature-verified Herdr Browser.app bundles.

Intended for ~/.ssh/authorized_keys with OpenSSH `restrict`:
  restrict,from="<lan-cidr>,fe80::/10",command="/Users/<user>/.local/bin/herdr-browser-receive" ssh-ed25519 ...

Never executes received binaries, never spawns a shell, and deletes temporary
files unconditionally on any validation or signature failure.
"""
from __future__ import annotations

import fcntl
import json
import os
from pathlib import Path
import plistlib
import posixpath
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
LOCAL = ROOT / '.local' / 'browser-updater'
APP = Path.home() / 'Applications' / 'Herdr Browser.app'
PIN_FILE = Path.home() / '.config' / 'herdr-browser-stage' / 'requirement'

APP_NAME = 'Herdr Browser.app'
BUNDLE_ID = 'dev.james.herdr-browser'
MAX_STREAM_BYTES = 750 * 1024 * 1024
MAX_EXTRACTED_BYTES = 1500 * 1024 * 1024
MAX_MEMBERS = 6000
MAX_METADATA_BYTES = 16 * 1024

BROWSER_USAGE_DESCRIPTIONS = (
    'NSBluetoothAlwaysUsageDescription',
    'NSWebBrowserPublicKeyCredentialUsageDescription',
    'NSCameraUsageDescription',
    'NSMicrophoneUsageDescription',
)

TAG_RE = re.compile(r'^v\d+\.\d+\.\d+$')
HEX40_RE = re.compile(r'^[0-9a-f]{40}$')
HEX64_RE = re.compile(r'^[0-9a-f]{64}$')
ENGINE_RE = re.compile(r'^\d+\.\d+\.\d+(?:\+\d+\.\d+\.\d+)?$')
CEF_VER_RE = re.compile(r'^\d+\.\d+\.\d+$')
ISO_RE = re.compile(r'^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$')


class StageError(RuntimeError):
    """Raised when an incoming staged bundle fails any safety or signature check."""


def version_tuple(tag: str) -> tuple[int, ...]:
    if not TAG_RE.fullmatch(tag):
        raise StageError(f'Invalid tag: {tag!r}')
    return tuple(map(int, tag[1:].split('.')))


def engine_tuple(version: str) -> tuple[int, ...]:
    if not ENGINE_RE.fullmatch(version):
        raise StageError(f'Invalid engine version: {version!r}')
    return tuple(map(int, re.split(r'[.+]', version)))


def read_json(path: Path) -> dict:
    try:
        return json.loads(path.read_text())
    except FileNotFoundError:
        return {}


def write_json(path: Path, value: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix('.tmp')
    tmp.write_text(json.dumps(value, indent=2) + '\n')
    tmp.replace(path)


def expected_requirement(*, pin_file: Path = PIN_FILE, installed_app: Path = APP) -> str:
    if pin_file.is_file():
        req = pin_file.read_text().strip()
        if req:
            return req
    if not installed_app.is_dir():
        raise StageError(f'No pinned requirement ({pin_file}) and no installed app ({installed_app}) to anchor trust')
    result = subprocess.run(
        ['/usr/bin/codesign', '-dr', '-', str(installed_app)],
        capture_output=True, text=True, check=False,
    )
    if result.returncode != 0:
        raise StageError(f'Could not read designated requirement from installed app: {result.stderr.strip()}')
    combined = (result.stdout or '') + '\n' + (result.stderr or '')
    for line in combined.splitlines():
        if line.startswith('designated => '):
            req = line.removeprefix('designated => ').strip()
            if req:
                return req
    raise StageError('Installed app has no designated requirement')


def validate_metadata(raw: dict) -> dict:
    if not isinstance(raw, dict):
        raise StageError('metadata.json must be a JSON object')
    allowed_keys = {'tag', 'commit', 'patch', 'engine', 'codecs', 'built'}
    extra = set(raw) - allowed_keys
    if extra:
        raise StageError(f'Unexpected metadata keys: {sorted(extra)}')
    tag = raw.get('tag')
    commit = raw.get('commit')
    patch = raw.get('patch')
    engine = raw.get('engine')
    built = raw.get('built')
    if not isinstance(tag, str) or not TAG_RE.fullmatch(tag):
        raise StageError(f'Invalid tag in metadata: {tag!r}')
    if not isinstance(commit, str) or not HEX40_RE.fullmatch(commit):
        raise StageError('Invalid commit hash in metadata')
    if not isinstance(patch, str) or not HEX64_RE.fullmatch(patch):
        raise StageError('Invalid patch hash in metadata')
    if not isinstance(engine, str) or not ENGINE_RE.fullmatch(engine):
        raise StageError(f'Invalid engine version in metadata: {engine!r}')
    if not isinstance(built, str) or not ISO_RE.fullmatch(built):
        raise StageError(f'Invalid built timestamp in metadata: {built!r}')
    codecs_raw = raw.get('codecs') or {}
    if not isinstance(codecs_raw, dict):
        raise StageError('codecs must be an object')
    codecs = {}
    if 'h264' in codecs_raw:
        if not isinstance(codecs_raw['h264'], bool):
            raise StageError('codecs.h264 must be bool')
        codecs['h264'] = codecs_raw['h264']
    if 'cef' in codecs_raw:
        if not isinstance(codecs_raw['cef'], str) or not CEF_VER_RE.fullmatch(codecs_raw['cef']):
            raise StageError('Invalid codecs.cef')
        codecs['cef'] = codecs_raw['cef']
    if 'sha256' in codecs_raw:
        if not isinstance(codecs_raw['sha256'], str) or not HEX64_RE.fullmatch(codecs_raw['sha256']):
            raise StageError('Invalid codecs.sha256')
        codecs['sha256'] = codecs_raw['sha256']
    return {
        'tag': tag,
        'commit': commit,
        'patch': patch,
        'engine': engine,
        'codecs': codecs,
        'built': built,
    }


def _clean_tar_path(name: str) -> str:
    if not name or '\x00' in name:
        raise StageError(f'Invalid archive member path: {name!r}')
    if name.startswith('/'):
        raise StageError(f'Absolute path refused in archive: {name!r}')
    parts = [p for p in name.split('/') if p not in ('', '.')]
    if not parts or any(p == '..' for p in parts):
        raise StageError(f'Path traversal refused in archive: {name!r}')
    if parts[0] not in (APP_NAME, 'metadata.json'):
        raise StageError(f'Unexpected top-level entry in archive: {parts[0]!r}')
    if parts[0] == 'metadata.json' and len(parts) != 1:
        raise StageError(f'Invalid metadata.json entry: {name!r}')
    return '/'.join(parts)


def _assert_within_app(member_path: str, link_target: str, *, kind: str) -> None:
    if not link_target or '\x00' in link_target or link_target.startswith('/'):
        raise StageError(f'Disallowed {kind} target {link_target!r} for {member_path!r}')
    if not member_path.startswith(APP_NAME + '/'):
        raise StageError(f'{kind} only allowed inside {APP_NAME}: {member_path!r}')
    if kind == 'symlink':
        parent = posixpath.dirname(member_path)
        resolved = posixpath.normpath(posixpath.join(parent, link_target))
    else:
        resolved = posixpath.normpath(link_target)
    if resolved == '..' or resolved.startswith('../') or not (resolved == APP_NAME or resolved.startswith(APP_NAME + '/')):
        raise StageError(f'{kind} escapes {APP_NAME}: {member_path!r} -> {link_target!r}')


def _is_within(path: Path, root: Path) -> bool:
    return path == root or str(path).startswith(str(root) + os.sep)


def safe_extract_tar(archive_path: Path, dest_dir: Path) -> None:
    """Validate every member strictly before extracting into dest_dir."""
    total_bytes = 0
    seen_app = False
    seen_meta = False
    seen_names: set[str] = set()
    symlink_paths: set[str] = set()
    with tarfile.open(archive_path, mode='r:*') as tf:
        members = tf.getmembers()
        if len(members) > MAX_MEMBERS:
            raise StageError(f'Too many archive members ({len(members)} > {MAX_MEMBERS})')
        validated = []
        for member in members:
            clean = _clean_tar_path(member.name)
            if clean in seen_names:
                raise StageError(f'Duplicate archive member refused: {clean}')
            seen_names.add(clean)
            if member.mode & 0o6000:
                raise StageError(f'Setuid/setgid bit refused: {clean}')
            if member.isdev() or member.isfifo():
                raise StageError(f'Special file refused: {clean}')
            if clean == 'metadata.json':
                if not member.isfile():
                    raise StageError('metadata.json must be a regular file')
                if member.size > MAX_METADATA_BYTES:
                    raise StageError('metadata.json exceeds size limit')
                seen_meta = True
            else:
                seen_app = True
                if member.issym():
                    _assert_within_app(clean, member.linkname, kind='symlink')
                    symlink_paths.add(clean)
                elif member.islnk():
                    _assert_within_app(clean, member.linkname, kind='hardlink')
                elif not (member.isfile() or member.isdir()):
                    raise StageError(f'Unsupported tar member type for {clean}')
            if member.isfile():
                total_bytes += member.size
                if total_bytes > MAX_EXTRACTED_BYTES:
                    raise StageError('Extracted bundle exceeds size limit')
            member.name = clean
            validated.append(member)

        if not seen_app or not seen_meta:
            raise StageError('Archive must contain both Herdr Browser.app and metadata.json')

        # Refuse any member whose parent directory traverses a symlink in the archive.
        for member in validated:
            parts = member.name.split('/')
            for i in range(1, len(parts)):
                prefix = '/'.join(parts[:i])
                if prefix in symlink_paths:
                    raise StageError(f'Archive member traverses symlink {prefix!r}: {member.name!r}')

        # Pass 1: Extract directories and regular files only (no symlinks on disk yet).
        deferred_symlinks: list[tarfile.TarInfo] = []
        deferred_hardlinks: list[tuple[Path, Path]] = []
        dest_resolved = dest_dir.resolve()
        app_root = dest_resolved / APP_NAME
        for member in validated:
            target = dest_dir / member.name
            allowed_root = dest_resolved if member.name == 'metadata.json' else app_root
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
                if target.is_symlink() or not _is_within(target.resolve(), app_root):
                    raise StageError(f'Directory resolves outside bundle: {member.name!r}')
                os.chmod(target, (member.mode & 0o777) | 0o700)
            elif member.isfile():
                target.parent.mkdir(parents=True, exist_ok=True)
                if not _is_within(target.parent.resolve(), allowed_root):
                    raise StageError(f'File parent escapes bundle: {member.name!r}')
                src = tf.extractfile(member)
                if src is None:
                    raise StageError(f'Unreadable archive member: {member.name}')
                flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, 'O_NOFOLLOW', 0)
                fd = os.open(target, flags, 0o600)
                with src, os.fdopen(fd, 'wb') as out:
                    shutil.copyfileobj(src, out, length=1024 * 1024)
                os.chmod(target, member.mode & 0o755)
            elif member.issym():
                deferred_symlinks.append(member)
            elif member.islnk():
                deferred_hardlinks.append((target, dest_dir / member.linkname))

        # Pass 2: Create symlinks after all directories and files exist.
        created_symlinks: list[tuple[str, Path]] = []
        for member in deferred_symlinks:
            target = dest_dir / member.name
            target.parent.mkdir(parents=True, exist_ok=True)
            if not _is_within(target.parent.resolve(), app_root):
                raise StageError(f'Symlink parent escapes bundle: {member.name!r}')
            if target.exists() or target.is_symlink():
                raise StageError(f'Symlink collides with existing entry: {member.name!r}')
            os.symlink(member.linkname, target)
            created_symlinks.append((member.name, target))

        # Pass 3: Verify every symlink resolves strictly within Herdr Browser.app on disk
        # (catches multi-hop / chained relative symlinks that pass lexical checks).
        for name, target in created_symlinks:
            resolved = target.resolve(strict=False)
            if not _is_within(resolved, app_root):
                raise StageError(f'Symlink escapes {APP_NAME}: {name!r}')

        # Pass 4: Create hardlinks to verified regular files inside the bundle.
        for target, source in deferred_hardlinks:
            if source.is_symlink() or not source.is_file():
                raise StageError(f'Hardlink source must be a regular file inside bundle: {source}')
            src_resolved = source.resolve()
            if not _is_within(src_resolved, app_root):
                raise StageError(f'Hardlink target outside bundle: {source}')
            target.parent.mkdir(parents=True, exist_ok=True)
            if not _is_within(target.parent.resolve(), app_root):
                raise StageError(f'Hardlink parent escapes bundle: {target}')
            if target.exists() or target.is_symlink():
                raise StageError(f'Hardlink collides with existing entry: {target}')
            os.link(src_resolved, target)


def verify_bundle_plists(bundle: Path, meta: dict) -> None:
    info_path = bundle / 'Contents/Info.plist'
    if not info_path.is_file():
        raise StageError('Missing Contents/Info.plist')
    with info_path.open('rb') as f:
        info = plistlib.load(f)
    if info.get('CFBundleIdentifier') != BUNDLE_ID:
        raise StageError(f'Unexpected CFBundleIdentifier: {info.get("CFBundleIdentifier")!r}')
    expected_ver = meta['tag'][1:]
    if info.get('CFBundleShortVersionString') != expected_ver:
        raise StageError(
            f'Bundle version {info.get("CFBundleShortVersionString")!r} does not match metadata tag {meta["tag"]!r}'
        )
    if info.get('HerdrBrowserPatch') != meta['patch']:
        raise StageError('Bundle HerdrBrowserPatch does not match metadata patch hash')
    plists = [info_path]
    plists += sorted((bundle / 'Contents/Frameworks').glob('HerdrBrowser Helper*.app/Contents/Info.plist'))
    for path in plists:
        with path.open('rb') as f:
            data = plistlib.load(f)
        for key in BROWSER_USAGE_DESCRIPTIONS:
            if not isinstance(data.get(key), str) or not data[key].strip():
                raise StageError(f'{path.name}: missing privacy declaration {key}')


def verify_signature(bundle: Path, requirement: str) -> None:
    req_arg = requirement if requirement.startswith('=') else f'={requirement}'
    result = subprocess.run(
        ['/usr/bin/codesign', '--verify', '--deep', '--strict', '-R', req_arg, str(bundle)],
        capture_output=True, text=True, check=False,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout or '').strip()
        raise StageError(f'Code signature verification failed: {detail}')


def copy_bounded_stdin(stream, dest: Path, max_bytes: int = MAX_STREAM_BYTES) -> int:
    total = 0
    with dest.open('wb') as out:
        while True:
            chunk = stream.read(1024 * 1024)
            if not chunk:
                break
            total += len(chunk)
            if total > max_bytes:
                raise StageError(f'Incoming stream exceeds {max_bytes} byte limit')
            out.write(chunk)
    if total == 0:
        raise StageError('Empty input stream')
    return total


def receive_stage(
    stream,
    *,
    local_dir: Path = LOCAL,
    installed_app: Path = APP,
    pin_file: Path = PIN_FILE,
    verifier=verify_signature,
    req_provider=None,
) -> dict:
    requirement = req_provider() if req_provider is not None else expected_requirement(
        pin_file=pin_file, installed_app=installed_app,
    )
    builds_dir = local_dir / 'builds'
    builds_dir.mkdir(parents=True, exist_ok=True)
    tmp_dir = Path(tempfile.mkdtemp(prefix='.incoming-stage-', dir=builds_dir))
    os.chmod(tmp_dir, 0o700)
    try:
        archive_path = tmp_dir / 'payload.tar.gz'
        copy_bounded_stdin(stream, archive_path)
        extract_dir = tmp_dir / 'extracted'
        extract_dir.mkdir(mode=0o700)
        safe_extract_tar(archive_path, extract_dir)
        archive_path.unlink(missing_ok=True)

        meta = validate_metadata(json.loads((extract_dir / 'metadata.json').read_text()))
        candidate = extract_dir / APP_NAME
        if not candidate.is_dir():
            raise StageError('Extracted archive missing Herdr Browser.app')
        verify_bundle_plists(candidate, meta)
        verifier(candidate, requirement)

        local_dir.mkdir(parents=True, exist_ok=True)
        with (local_dir / 'lock').open('w') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            state_path = local_dir / 'state.json'
            state = read_json(state_path)
            installed = state.get('installed') or {}
            if installed.get('tag') and version_tuple(meta['tag']) < version_tuple(installed['tag']):
                raise StageError(f'Refusing downgrade from {installed["tag"]} to {meta["tag"]}')
            if installed.get('engine') and engine_tuple(meta['engine']) < engine_tuple(installed['engine']):
                raise StageError(f'Refusing Chromium downgrade from {installed["engine"]} to {meta["engine"]}')

            key = f'{meta["tag"]}-{meta["patch"][:12]}-cef{meta["engine"]}-{time.time_ns()}'
            final_dir = builds_dir / key
            final_dir.mkdir(mode=0o755)
            final_bundle = final_dir / APP_NAME
            shutil.move(str(candidate), str(final_bundle))

            pending = {
                'bundle': str(final_bundle),
                'tag': meta['tag'],
                'commit': meta['commit'],
                'patch': meta['patch'],
                'engine': meta['engine'],
                'codecs': meta['codecs'],
                'built': meta['built'],
            }
            state.update(
                status='staged',
                target=meta['tag'],
                pending=pending,
                failed_key=None,
                error=None,
                latest_engine=meta['engine'],
            )
            write_json(state_path, state)
            return {'ok': True, 'staged': pending}
    finally:
        shutil.rmtree(tmp_dir, ignore_errors=True)


def read_status(local_dir: Path = LOCAL) -> dict:
    state = read_json(local_dir / 'state.json')
    return {
        'ok': True,
        'status': state.get('status'),
        'target': state.get('target'),
        'installed': state.get('installed'),
        'pending': state.get('pending'),
    }


def dispatch(original_command: str | None, stream, **kwargs) -> dict:
    cmd = (original_command or '').strip()
    if cmd == 'status':
        return read_status(kwargs.get('local_dir', LOCAL))
    if cmd in ('', 'stage'):
        return receive_stage(stream, **kwargs)
    raise StageError(f'Disallowed command: {cmd!r} (only "stage" and "status" are permitted)')


def main() -> int:
    try:
        result = dispatch(os.environ.get('SSH_ORIGINAL_COMMAND'), sys.stdin.buffer)
    except Exception as error:
        print(json.dumps({'ok': False, 'error': str(error)}))
        return 1
    print(json.dumps(result, indent=2))
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
