#!/usr/bin/env python3
"""Push this Mac's signed Herdr Browser.app to a locked-down remote stage receiver.

Usage:
  python3 personal/stage_push.py other-mac-stage
  python3 personal/stage_push.py other-mac-stage --pending
  python3 personal/stage_push.py other-mac-stage --status
"""
from __future__ import annotations

import argparse
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile

ROOT = Path(__file__).resolve().parents[1]
LOCAL = ROOT / '.local' / 'browser-updater'
APP = Path.home() / 'Applications' / 'Herdr Browser.app'
APP_NAME = 'Herdr Browser.app'


def build_metadata(state_path: Path = LOCAL / 'state.json', *, slot: str = 'installed') -> tuple[Path, dict]:
    state = json.loads(state_path.read_text())
    record = state.get(slot)
    if not isinstance(record, dict):
        raise RuntimeError(f'No {slot} build record in {state_path}')
    bundle = Path(record['bundle']) if slot == 'pending' and record.get('bundle') else APP
    return bundle, {
        'tag': record['tag'],
        'commit': record['commit'],
        'patch': record['patch'],
        'engine': record['engine'],
        'codecs': record.get('codecs', {}),
        'built': record['built'],
    }


def stream_bundle_tar(app_path: Path, metadata: dict, fileobj) -> None:
    with tarfile.open(fileobj=fileobj, mode='w|gz', compresslevel=1) as tf:
        meta_bytes = (json.dumps(metadata, indent=2) + '\n').encode('utf-8')
        info = tarfile.TarInfo(name='metadata.json')
        info.size = len(meta_bytes)
        info.mode = 0o644
        tf.addfile(info, io.BytesIO(meta_bytes))

        base = app_path.resolve()
        for root, dirs, files in os.walk(base, followlinks=False):
            root_path = Path(root)
            rel_root = root_path.relative_to(base)
            arc_root = Path(APP_NAME) / rel_root if str(rel_root) != '.' else Path(APP_NAME)
            dir_info = tf.gettarinfo(str(root_path), arcname=arc_root.as_posix())
            dir_info.mode &= 0o777
            tf.addfile(dir_info)

            # Handle symlinked directories (e.g. Frameworks/.../Versions/Current) without descending.
            real_dirs = []
            for d in sorted(dirs):
                entry = root_path / d
                if entry.is_symlink():
                    sym_info = tf.gettarinfo(str(entry), arcname=(arc_root / d).as_posix())
                    tf.addfile(sym_info)
                else:
                    real_dirs.append(d)
            dirs[:] = real_dirs

            for f in sorted(files):
                if f.startswith('._'):
                    continue
                entry = root_path / f
                file_info = tf.gettarinfo(str(entry), arcname=(arc_root / f).as_posix())
                file_info.mode &= 0o777
                if file_info.isfile():
                    with entry.open('rb') as handle:
                        tf.addfile(file_info, handle)
                else:
                    tf.addfile(file_info)


def run_status(target: str) -> int:
    result = subprocess.run(
        ['ssh', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10', target, 'status'],
        text=True, check=False,
    )
    return result.returncode


def push_stage(
    target: str,
    *,
    app_path: Path | None = None,
    state_path: Path = LOCAL / 'state.json',
    slot: str = 'installed',
) -> int:
    default_bundle, metadata = build_metadata(state_path, slot=slot)
    bundle = app_path or default_bundle
    if not bundle.is_dir():
        raise RuntimeError(f'App bundle not found: {bundle}')
    subprocess.run(
        ['/usr/bin/codesign', '--verify', '--deep', '--strict', str(bundle)],
        check=True,
    )
    print(f'Streaming signed {APP_NAME} ({metadata["tag"]}, CEF {metadata["engine"]}, {slot}) to {target}...', file=sys.stderr)
    proc = subprocess.Popen(
        ['ssh', '-o', 'BatchMode=yes', '-o', 'ConnectTimeout=10', target, 'stage'],
        stdin=subprocess.PIPE,
    )
    assert proc.stdin is not None
    try:
        stream_bundle_tar(bundle, metadata, proc.stdin)
    except BrokenPipeError:
        pass
    finally:
        proc.stdin.close()
    return proc.wait()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('target', help='SSH host alias configured for the forced-command stage key (e.g. other-mac-stage)')
    parser.add_argument('--status', action='store_true', help='Read remote staged/installed status without uploading')
    parser.add_argument('--pending', action='store_true', help='Send the locally staged pending bundle instead of the installed app')
    parser.add_argument('--app', type=Path, default=None, help='Override app bundle path to send')
    args = parser.parse_args()
    if args.status:
        return run_status(args.target)
    return push_stage(args.target, app_path=args.app, slot='pending' if args.pending else 'installed')


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except Exception as error:
        print(f'stage_push: {error}', file=sys.stderr)
        raise SystemExit(1)
