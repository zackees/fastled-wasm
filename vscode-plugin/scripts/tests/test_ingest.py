import json
import sys
from pathlib import Path

import pytest

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
from clangd_common import load_lock, safe_relative, sha256  # noqa: E402
from ingest_clangd import parse_installer_json, stage_bundle  # noqa: E402

ROOT = SCRIPTS.parent


def test_lock_has_exact_targets_and_safe_paths():
    lock = load_lock(ROOT / 'clangd-artifacts.json')
    assert set(lock['targets']) == {'win32-x64', 'win32-arm64', 'linux-x64', 'linux-arm64', 'darwin-x64', 'darwin-arm64'}
    assert all(safe_relative(value['binary_path']) for value in lock['targets'].values())


@pytest.mark.parametrize('value', ['', '../escape', '/absolute', 'bin\\clangd'])
def test_unsafe_paths_are_rejected(value):
    assert not safe_relative(value)


def test_installer_requires_one_object_and_rejects_identity_noise():
    assert parse_installer_json('{"status":"installed"}') == {'status': 'installed'}
    with pytest.raises(ValueError):
        parse_installer_json('noise\n{}')
    with pytest.raises(json.JSONDecodeError):
        parse_installer_json('not-json')


def test_staging_failure_preserves_existing_output(tmp_path):
    lock = load_lock(ROOT / 'clangd-artifacts.json')
    target = dict(lock['targets']['linux-x64'])
    source = tmp_path / 'source'
    (source / 'bin').mkdir(parents=True)
    (source / 'lib/clang/21/include').mkdir(parents=True)
    (source / 'bin/clangd').write_bytes(b'wrong')
    (source / 'lib/clang/21/include/stddef.h').write_text('')
    (source / 'lib/clang/21/include/stdint.h').write_text('')
    output = tmp_path / 'output'
    output.mkdir()
    (output / 'keep').write_text('still valid')
    with pytest.raises(ValueError):
        stage_bundle('linux-x64', target, lock['provider'], source, output)
    assert (output / 'keep').read_text() == 'still valid'


def test_payload_hash_helper(tmp_path):
    item = tmp_path / 'item'
    item.write_bytes(b'fastled')
    assert sha256(item) == 'ab52dd1c4cd06e210f7c7f3bf85d73e702643efb06b92d3da313706da7317d5d'
