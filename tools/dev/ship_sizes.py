"""Stripped shipping-library sizes for a base and a head (constitution section 2).

`just packaging sizes` builds both facades with the release recipe
(`just packaging install`), strips each shared object with
`strip --strip-unneeded`, checks that it exports exactly its version script's
set, and reports base bytes, head bytes, difference and ratio. A size increase
is reported, never failed: section 2 asks for a report, not a limit.

Both sides build from `git archive` output in the same directory with the same
path remapping, so a path never changes a byte. A base result is cached by
recipe key: the tree ids of every input that can change the libraries plus the
toolchain fingerprint. The key does not name a commit, so every main commit
with the same inputs shares one result.

The crate inputs are the root library's sources and build inputs and the two C
API crate directories, the crates that ship: the key needs no Cargo and no
closure read at the head, so a base revision is keyed by its own tree. A change
to a C API crate's tests only misses the cache.

The key and report subcommands use the standard library only.
"""
from __future__ import annotations

import argparse
from collections import deque
import hashlib
import json
import os
import platform
import re
from pathlib import Path
import shutil
import subprocess
import sys
import tomllib
from typing import Sequence

from . import cell
from .packaging import crate_dir, linux_exports, version_exports

ROOT = Path(__file__).resolve().parents[2]
# Each installed library, with its symbol prefix and [package.metadata.capi.library] name.
LIBRARIES = {'libpinyin': 'pinyin', 'libzhuyin': 'zhuyin'}
SCHEMA = 2
KEY_PREFIX = 'ship-sizes-v2-'
# These modules select the cell, run the release install and check the installed exports.
BUILD_TOOLS = ('tools/dev/__init__.py', 'tools/dev/entry.py', 'tools/dev/cell.py', 'tools/dev/packaging.py',
               'tools/dev/packaging_templates.py', 'tools/dev/runners.py', 'tools/dev/pool.py',
               'tools/dev/slot.py', 'tools/dev/host.py')
INPUTS = ('src', 'codegen', 'build.rs', 'capi', 'zhuyin-capi', 'Cargo.toml', 'Cargo.lock',
          'rust-toolchain.toml', '.cargo', '.gitattributes', 'justfile', '.python-version',
          'pyproject.toml', 'uv.toml', 'uv.lock', 'tools/oracle/oracle-pin.txt', *BUILD_TOOLS)
PACKAGES = ('binutils', 'gcc', 'g++', 'libc6', 'libglib2.0-dev', 'libkyotocabinet-dev')


class Failure(Exception):
    """A build, install or export check failed."""


def run(argv: Sequence[str | Path], *, cwd: Path | None = None, env: dict[str, str] | None = None,
        capture: bool = True) -> str:
    done = subprocess.run([str(word) for word in argv], cwd=cwd, env=env, text=True,
                          stdout=subprocess.PIPE if capture else None,
                          stderr=subprocess.PIPE if capture else None)
    if done.returncode:
        raise Failure(f"{' '.join(map(str, argv))} exited {done.returncode}\n{done.stderr or ''}".rstrip())
    return done.stdout or ''


def git(*arguments: str | Path, root: Path = ROOT) -> str:
    return run(['git', *arguments], cwd=root).strip()


def first_line(argv: Sequence[str], root: Path) -> str:
    try:
        return (run(argv, cwd=root).splitlines() or ['empty'])[0]
    except (Failure, OSError):
        return 'unavailable'


def reference_cell(root: Path = ROOT):
    """The cell of the size build: the one Linux cell (LOCAL_CELL in tools/dev/cell.py)."""
    return cell.Settings.read({'PINYIN_ORACLE_DBM': cell.LOCAL_CELL}, root)


def environment(root: Path = ROOT) -> dict[str, str]:
    """The toolchain facts that decide the bytes besides the source."""
    try:
        rustc = run(['rustc', '-vV'], cwd=root)
    except (Failure, OSError):
        rustc = 'unavailable'
    facts = {'backend': reference_cell(root).feature, 'rustc': rustc, 'cc': first_line(['cc', '--version'], root),
             'strip': first_line(['strip', '--version'], root),
             'cargo-c': first_line(['cargo', 'cinstall', '--version'], root)}
    # The backend's own package version is among the dpkg facts below: both
    # revisions link that one build of libkyotocabinet.
    if shutil.which('dpkg-query'):
        for package in PACKAGES:
            facts['dpkg:' + package] = first_line(['dpkg-query', '-W', '-f', '${Version}', package], root)
    # The current checkout compares and strips both sides, including a historical base.
    for name in (*BUILD_TOOLS, 'tools/dev/ship_sizes.py'):
        with (root / name).open('rb') as stream:
            facts[name] = hashlib.file_digest(stream, 'sha256').hexdigest()
    for name, value in os.environ.items():
        if name in ('CC', 'CXX', 'LD', 'AR', 'CFLAGS', 'CXXFLAGS', 'LDFLAGS', 'RUSTFLAGS',
                    'CARGO_ENCODED_RUSTFLAGS', 'RUSTC', 'RUSTC_WRAPPER', 'LIBRARY_PATH',
                    'C_INCLUDE_PATH', 'CPLUS_INCLUDE_PATH') or name.startswith(
                        ('CARGO_PROFILE_', 'CARGO_TARGET_', 'SYSTEM_DEPS_', 'PKG_CONFIG')):
            if name != 'CARGO_TARGET_DIR':
                facts['env:' + name] = value
    return facts


def tree_ids(revision: str, root: Path = ROOT) -> dict[str, str]:
    # cargo metadata and the locked workspace build also read the other member manifests.
    manifest = tomllib.loads(git('show', f'{revision}:Cargo.toml', root=root))
    members = manifest['workspace']['members']
    inputs = (*INPUTS, *(f'{member}/Cargo.toml' for member in members))
    return {path: git('rev-parse', '--verify', f'{revision}:{path}', root=root) for path in inputs}


def recipe_key(revision: str, facts: dict[str, str], root: Path = ROOT) -> str:
    text = json.dumps({'trees': tree_ids(revision, root), 'environment': facts}, sort_keys=True)
    return KEY_PREFIX + hashlib.sha256(text.encode()).hexdigest()[:32]


def version_script(source: Path, library: str) -> Path:
    """The version script in library's C crate directory, which the source's own Cargo metadata names.

    Only the crate directory is read: a fixture elsewhere in the tree can carry the same name
    (for example tests/fixtures/libpinyin.ver).
    """
    return crate_dir(source, LIBRARIES[library]) / f'{library}.ver'


def check_exports(library: str, nm_text: str, script: Path) -> None:
    """The installed object exports exactly its pin version script's set."""
    node, names = version_exports(script)
    actual, expected = set(linux_exports(nm_text, node)), set(names)
    if actual != expected:
        lines = [f'{library}: exported symbols are not the version script set']
        lines += ['  MISSING ' + name for name in sorted(expected - actual)]
        lines += ['  EXTRA   ' + name for name in sorted(actual - expected)]
        raise Failure('\n'.join(lines))


def extract(revision: str, destination: Path, root: Path = ROOT) -> None:
    """Export a commit into destination with every mtime set to now.

    Archive mtimes are commit times: a later side with older files would look
    fresh to cargo and reuse the earlier side's objects.
    """
    shutil.rmtree(destination, ignore_errors=True)
    destination.mkdir(parents=True)
    archive = subprocess.Popen(['git', 'archive', revision], cwd=root, stdout=subprocess.PIPE)
    unpack = subprocess.run(['tar', '-x', '-C', str(destination)], stdin=archive.stdout)
    archive.stdout.close()
    if archive.wait() or unpack.returncode:
        raise Failure(f'could not export {revision}')
    touch_tree(destination)


def touch_tree(directory: Path) -> None:
    """Set every mtime under directory to now without following a link out of it.

    Where utime can act on a link itself it does. Elsewhere (Windows) a symbolic
    link or a junction is skipped, so no timestamp changes outside the tree.
    """
    on_links = os.utime in os.supports_follow_symlinks
    for current, directories, files in os.walk(directory):
        if not on_links:
            directories[:] = [name for name in directories if not os.path.isjunction(os.path.join(current, name))]
        for name in (*directories, *files):
            path = os.path.join(current, name)
            if on_links:
                os.utime(path, follow_symlinks=False)
            elif not os.path.islink(path):
                os.utime(path)


def build_env(source: Path, target: Path, cell: str) -> dict[str, str]:
    env = dict(os.environ)
    # The outer `uv run` activates this checkout's environment. The revision in
    # SOURCE is another uv project, and its own `uv run` selects its own.
    env.pop('VIRTUAL_ENV', None)
    remap = f'--remap-path-prefix={source}=/oxpinyin --remap-path-prefix={target}=/oxpinyin-target'
    env['RUSTFLAGS'] = ((env.get('RUSTFLAGS') or '') + ' ' + remap).strip()
    env['PINYIN_ORACLE_DBM'] = cell
    return env


def revision_recipe(revision: str, source: Path, recipe: Sequence[str | Path], env: dict[str, str]) -> None:
    """Run a just recipe of REVISION from its extracted SOURCE.

    The revision's own justfile starts its tooling: its recipe, and any recipe
    that it depends on, choose the interpreter (`.python-version` and
    `uv.toml` in SOURCE), install it and pass the startup flags. This module
    names no interpreter and no flag of another revision.

    Output is not captured: a release build reports its progress. The last
    lines of stderr are kept for the failure message.
    """
    argv = ['just', '--justfile', str(source / 'justfile'), *(str(word) for word in recipe)]
    try:
        process = subprocess.Popen(argv, cwd=source, env=env, stderr=subprocess.PIPE)
    except OSError as error:
        raise Failure(f'{revision}: could not start just ({error}); '
                      'see docs/runbooks/setup.md#comparing-against-another-revision') from error
    tail: deque[str] = deque(maxlen=20)
    with process:
        for line in process.stderr:
            sys.stderr.buffer.write(line)
            sys.stderr.buffer.flush()
            tail.append(line.decode(errors='replace').rstrip())
    status = process.returncode
    if status:
        raise Failure('\n'.join([f'{revision}: its own `just {recipe[0]}` recipe exited {status}:', *tail,
                                 "The revision's justfile chooses its interpreter and startup flags; "
                                 'see docs/runbooks/setup.md#comparing-against-another-revision']))


def build_side(revision: str, key: str, work: Path, target: Path) -> dict[str, object]:
    """Build both facades of one commit with that commit's own `packaging install`.

    The source, stage and target paths are the same for every side and run:
    they enter RUSTFLAGS, so a changing path would rebuild every dependency.
    """
    source, stage = work / 'src', work / 'stage'
    if platform.system() != 'Linux':
        raise Failure('the size build is the Linux release recipe; run it on Linux')
    cell = reference_cell()
    extract(revision, source)
    shutil.rmtree(stage, ignore_errors=True)
    env = build_env(source, target, cell.cell)
    for library in LIBRARIES:
        features = cell.feature
        revision_recipe(revision, source, [
            'packaging', 'install', library,
            '--prefix=/usr', '--libdir=/usr/lib', f'--destdir={stage}', '--',
            '--release', '--locked', '--no-default-features', '--features', features,
            f'--target-dir={target}'], env)
    sizes = {}
    for library in LIBRARIES:
        installed = stage / 'usr/lib' / f'{library}.so.15.0.0'
        if not installed.is_file():
            raise Failure(f'{library}: {installed} was not installed')
        stripped = work / f'{library}.stripped.so'
        shutil.copyfile(installed, stripped)
        run(['strip', '--strip-unneeded', '--', stripped])
        nm = run(['nm', '-D', '--defined-only', stripped])
        script = version_script(source, library)
        if not script.is_file():
            raise Failure(f'{revision}: no {library}.ver at {script.relative_to(source).as_posix()}')
        check_exports(library, nm, script)
        sizes[library] = stripped.stat().st_size
    return {'schema': SCHEMA, 'revision': revision, 'recipe_key': key, 'backend': cell.feature,
            'libraries': sizes}


def read_result(path: Path, key: str, backend: str) -> dict[str, object] | None:
    try:
        data = json.loads(path.read_text())
    except (OSError, ValueError):
        return None
    if not isinstance(data, dict) or type(data.get('schema')) is not int or data['schema'] != SCHEMA:
        return None
    if data.get('recipe_key') != key or data.get('backend') != backend:
        return None
    revision = data.get('revision')
    if not isinstance(revision, str) or not re.fullmatch('[0-9a-f]{40}', revision):
        return None
    sizes = data.get('libraries')
    if not isinstance(sizes, dict) or sizes.keys() != LIBRARIES.keys():
        return None
    if any(type(size) is not int or size <= 0 for size in sizes.values()):
        return None
    return data


def write_result(path: Path, result: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')


def table(base: dict[str, object], head: dict[str, object], base_note: str) -> str:
    lines = ['## Stripped shipping-library sizes', '',
             f'Backend `{head["backend"]}`, release recipe, `strip --strip-unneeded`. '
             f'Base `{str(base["revision"])[:12]}` ({base_note}); head `{str(head["revision"])[:12]}`.', '',
             '| library | base bytes | PR bytes | difference | ratio |', '|---|---:|---:|---:|---:|']
    grown = []
    for library in LIBRARIES:
        before, after = base['libraries'][library], head['libraries'][library]
        lines.append(f'| `{library}.so.15.0.0` | {before} | {after} | {after - before:+d} | {after / before:.4f} |')
        if after > before:
            grown.append(library)
    lines += ['', 'Increase over base: ' + (', '.join(grown) if grown else 'none') + '.']
    return '\n'.join(lines) + '\n'


def emit_summary(text: str) -> None:
    print(text)
    summary = os.environ.get('GITHUB_STEP_SUMMARY')
    if summary:
        with open(summary, 'a', encoding='utf-8') as handle:
            handle.write(text + '\n')


def cmd_apt(args: argparse.Namespace) -> int:
    print('libkyotocabinet-dev')
    return 0


def cmd_key(args: argparse.Namespace) -> int:
    key = recipe_key(resolve(args.commit), environment())
    print(key)
    output = os.environ.get('GITHUB_OUTPUT')
    if output:
        with open(output, 'a', encoding='utf-8') as handle:
            handle.write(f'key={key}\n')
    return 0


def directories(args: argparse.Namespace) -> tuple[Path, Path]:
    work = Path(args.work).resolve() if args.work else ROOT / 'target/ship-sizes'
    work.mkdir(parents=True, exist_ok=True)
    return work, Path(args.target).resolve() if args.target else work / 'target'


def resolve(revision: str) -> str:
    return git('rev-parse', '--verify', '--end-of-options', f'{revision}^{{commit}}')


def cmd_build(args: argparse.Namespace) -> int:
    revision = resolve(args.commit)
    facts = environment()
    key = recipe_key(revision, facts)
    path = Path(args.out)
    result = read_result(path, key, facts['backend']) if args.reuse else None
    if result is None:
        result = build_side(revision, key, *directories(args))
        write_result(path, result)
    print(json.dumps(result, sort_keys=True))
    return 0


def cmd_run(args: argparse.Namespace) -> int:
    """Build the head and reuse the base only when the recorded build identity agrees."""
    work, target = directories(args)
    facts = environment()
    base_revision, head_revision = resolve(args.base), resolve(args.head)
    base_key = recipe_key(base_revision, facts)
    head_key = recipe_key(head_revision, facts)
    cache = Path(args.base_cache) if args.base_cache else None
    base = read_result(cache, base_key, facts['backend']) if cache else None
    if base is None:
        base = build_side(base_revision, base_key, work, target)
        note = 'built in this run'
        if cache:
            write_result(cache, base)
    else:
        note = 'cached by recipe key'
    head = build_side(head_revision, head_key, work, target)
    if args.out:
        write_result(Path(args.out), head)
    emit_summary(table(base, head, note))
    return 0


def parser() -> argparse.ArgumentParser:
    top = argparse.ArgumentParser(prog='just packaging sizes', description=__doc__)
    sub = top.add_subparsers(dest='command', required=True)
    p = sub.add_parser('apt', help='print the distribution packages the reference backend needs')
    p.set_defaults(func=cmd_apt)
    p = sub.add_parser('key', help='print the recipe key of a commit')
    p.add_argument('--commit', default='HEAD')
    p.set_defaults(func=cmd_key)
    p = sub.add_parser('build', help='build one commit and write its sizes as JSON')
    p.add_argument('--commit', default='HEAD')
    p.add_argument('--out', required=True)
    p.add_argument('--reuse', action='store_true', help='reuse OUT if its build identity matches')
    p.add_argument('--work', help='source, stage and strip scratch (default target/ship-sizes)')
    p.add_argument('--target', help='cargo target dir (default WORK/target)')
    p.set_defaults(func=cmd_build)
    p = sub.add_parser('run', help='report base and head sizes (the local reproduction)')
    p.add_argument('--base', default='origin/main')
    p.add_argument('--head', default='HEAD')
    p.add_argument('--base-cache', help='read base sizes from this file when valid; else build and write it')
    p.add_argument('--out', help='also write the head sizes here')
    p.add_argument('--work', help='source, stage and strip scratch (default target/ship-sizes)')
    p.add_argument('--target', help='cargo target dir (default WORK/target)')
    p.set_defaults(func=cmd_run)
    return top


def main(argv: Sequence[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        return args.func(args)
    except (Failure, OSError, ValueError, KeyError) as error:
        print(f'error: {error}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
