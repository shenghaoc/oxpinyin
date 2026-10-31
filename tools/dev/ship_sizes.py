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

The gate, key and report subcommands use the standard library only.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
from typing import Sequence

ROOT = Path(__file__).resolve().parents[2]
LIBRARIES = {'libpinyin': ('oxpinyin-capi', 'pinyin'), 'libzhuyin': ('oxpinyin-zhuyin-capi', 'zhuyin')}
SCHEMA = 1
KEY_PREFIX = 'ship-sizes-v1-'
# Normal and build path dependencies of the two capi crates. shipping_closure()
# computes the same set from cargo metadata; the tests compare the two, so a new
# crate in the graph fails there. The gate runs without Cargo and reads this tuple.
SHIPPING_CRATES = ('oxpinyin-capi', 'oxpinyin-capi-marshal', 'oxpinyin-chewing', 'oxpinyin-core',
                   'oxpinyin-data', 'oxpinyin-engine', 'oxpinyin-facade', 'oxpinyin-input',
                   'oxpinyin-runtime', 'oxpinyin-store', 'oxpinyin-user', 'oxpinyin-zhuyin-capi')
# Inputs that can change a library byte: they key the cached base result.
INPUTS = (*('crates/' + name for name in SHIPPING_CRATES),
          'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', '.cargo',
          'tools/codegen/string_tables.rs',
          'tools/dev/packaging.py', 'tools/dev/packaging_templates.py', 'tools/oracle/oracle-pin.txt')
# Inputs that cannot change a byte but change this report or its job.
REPORT_INPUTS = ('justfile', 'tools/dev/cell.py', 'tools/dev/ship_sizes.py', '.github/workflows/ci.yml', '.github/actions')
PACKAGES = ('binutils', 'gcc', 'g++', 'libc6', 'libglib2.0-dev')
# Distribution packages each backend's build needs, beside the common set above.
BACKEND_PACKAGES = {
    'bdb': ('libdb-dev',),
    'kyotocabinet': ('libkyotocabinet-dev',),
    'tkrzw': ('libtkrzw-dev', 'liblzma-dev', 'liblz4-dev', 'libzstd-dev', 'zlib1g-dev'),
}


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


def gate(base: str, root: Path = ROOT) -> bool:
    """True when a path that can change a library, or this job, differs from base.

    An unavailable base runs the job: the over-trigger direction, never a skip.
    """
    if not base:
        return True
    try:
        run(['git', 'cat-file', '-e', base + '^{commit}'], cwd=root)
        return bool(git('diff', '--name-only', base, 'HEAD', '--', *INPUTS, *REPORT_INPUTS, root=root))
    except Failure:
        return True


def first_line(argv: Sequence[str], root: Path) -> str:
    try:
        return (run(argv, cwd=root).splitlines() or ['empty'])[0]
    except (Failure, OSError):
        return 'unavailable'


def reference_cell(root: Path = ROOT):
    """The reference cell, from the one place that defines its default."""
    from .cell import Settings
    return Settings.read({}, root)


def backend_packages(root: Path = ROOT) -> tuple[str, ...]:
    return BACKEND_PACKAGES[reference_cell(root).feature]


def shipping_closure(root: Path = ROOT) -> set[str]:
    """The capi crates and their normal and build path dependencies."""
    metadata = json.loads(run(['cargo', 'metadata', '--no-deps', '--locked', '--format-version', '1'], cwd=root))
    packages = {package['name']: package for package in metadata['packages']}
    seen: set[str] = set()
    pending = [crate for crate, _ in LIBRARIES.values()]
    while pending:
        name = pending.pop()
        if name not in seen:
            seen.add(name)
            pending += [dependency['name'] for dependency in packages[name]['dependencies']
                        if dependency.get('path') and dependency['kind'] in (None, 'build')]
    return seen


def environment(root: Path = ROOT) -> dict[str, str]:
    """The toolchain facts that decide the bytes besides the source."""
    try:
        rustc = run(['rustc', '-vV'], cwd=root)
    except (Failure, OSError):
        rustc = 'unavailable'
    facts = {'backend': reference_cell(root).feature, 'rustc': rustc, 'cc': first_line(['cc', '--version'], root),
             'strip': first_line(['strip', '--version'], root),
             'cargo-c': first_line(['cargo', 'cinstall', '--version'], root)}
    if shutil.which('dpkg-query'):
        for package in (*PACKAGES, *backend_packages(root)):
            facts['dpkg:' + package] = first_line(['dpkg-query', '-W', '-f', '${Version}', package], root)
    return facts


def tree_ids(revision: str, root: Path = ROOT) -> dict[str, str]:
    result = {}
    for path in INPUTS:
        try:
            result[path] = git('rev-parse', f'{revision}:{path}', root=root)
        except Failure:
            result[path] = '-'
    return result


def recipe_key(revision: str, facts: dict[str, str], root: Path = ROOT) -> str:
    text = json.dumps({'trees': tree_ids(revision, root), 'environment': facts}, sort_keys=True)
    return KEY_PREFIX + hashlib.sha256(text.encode()).hexdigest()[:32]


def exports(nm_text: str, prefix: str) -> set[str]:
    """Defined dynamic symbols of one library, version suffix removed."""
    names = set()
    for line in nm_text.splitlines():
        fields = line.split()
        if len(fields) == 3 and fields[1] in 'TtWwDdBbRrVvi':
            names.add(fields[2].split('@', 1)[0])
    return {name for name in names if name.startswith(prefix + '_') or name.startswith('oxpinyin_')}


def version_globals(text: str) -> set[str]:
    from .abi import version_globals as parse
    return parse(text)


def check_exports(library: str, nm_text: str, version_text: str) -> None:
    """The installed object exports exactly its pin version script's set."""
    prefix = LIBRARIES[library][1]
    actual, expected = exports(nm_text, prefix), version_globals(version_text)
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
    for current, directories, files in os.walk(destination):
        for name in (*directories, *files):
            os.utime(os.path.join(current, name), follow_symlinks=False)


def build_env(source: Path, target: Path, cell: str) -> dict[str, str]:
    env = dict(os.environ)
    remap = f'--remap-path-prefix={source}=/oxpinyin --remap-path-prefix={target}=/oxpinyin-target'
    env['RUSTFLAGS'] = ((env.get('RUSTFLAGS') or '') + ' ' + remap).strip()
    env['PINYIN_ORACLE_DBM'] = cell
    return env


def build_side(revision: str, work: Path, target: Path, root: Path = ROOT) -> dict[str, object]:
    """Build both facades of one commit with that commit's own `packaging install`.

    The source, stage and target paths are the same for every side and run:
    they enter RUSTFLAGS, so a changing path would rebuild every dependency.
    """
    source, stage = work / 'src', work / 'stage'
    if platform_system() != 'Linux':
        raise Failure('the size build is the Linux release recipe; run it in the Linux reference container')
    cell = reference_cell(root)
    extract(revision, source, root)
    shutil.rmtree(stage, ignore_errors=True)
    env = build_env(source, target, cell.cell)
    for library in LIBRARIES:
        features = cell.feature + (',shipped' if library == 'libpinyin' else '')
        run([sys.executable, '-X', 'lazy_imports=normal', '-m', 'tools.dev', 'packaging', 'install', library,
             '--prefix=/usr', '--libdir=/usr/lib', f'--destdir={stage}', '--',
             '--release', '--locked', '--no-default-features', '--features', features,
             f'--target-dir={target}'], cwd=source, env=env, capture=False)
    sizes = {}
    for library, (crate, _) in LIBRARIES.items():
        installed = stage / 'usr/lib' / f'{library}.so.15.0.0'
        if not installed.is_file():
            raise Failure(f'{library}: {installed} was not installed')
        stripped = work / f'{library}.stripped.so'
        shutil.copyfile(installed, stripped)
        run(['strip', '--strip-unneeded', '--', stripped])
        nm = run(['nm', '-D', '--defined-only', stripped])
        check_exports(library, nm, (source / 'crates' / crate / f'{library}.ver').read_text())
        sizes[library] = stripped.stat().st_size
    return {'schema': SCHEMA, 'revision': git('rev-parse', revision, root=root), 'backend': cell.feature,
            'libraries': sizes}


def platform_system() -> str:
    import platform
    return platform.system()


def valid(result: object) -> bool:
    return (isinstance(result, dict) and result.get('schema') == SCHEMA
            and isinstance(result.get('revision'), str)
            and isinstance(result.get('libraries'), dict)
            and set(result['libraries']) == set(LIBRARIES)
            and all(isinstance(size, int) and size > 0 for size in result['libraries'].values()))


def read_result(path: Path) -> dict[str, object] | None:
    try:
        result = json.loads(path.read_text())
    except (OSError, ValueError):
        return None
    return result if valid(result) else None


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


def cmd_gate(args: argparse.Namespace) -> int:
    value = 'true' if gate(args.base) else 'false'
    print('ship=' + value)
    output = os.environ.get('GITHUB_OUTPUT')
    if output:
        with open(output, 'a', encoding='utf-8') as handle:
            handle.write(f'ship={value}\n')
    return 0


def cmd_apt(args: argparse.Namespace) -> int:
    print(' '.join(backend_packages()))
    return 0


def cmd_key(args: argparse.Namespace) -> int:
    key = recipe_key(args.commit, environment())
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


def cmd_build(args: argparse.Namespace) -> int:
    result = build_side(args.commit, *directories(args))
    write_result(Path(args.out), result)
    print(json.dumps(result, sort_keys=True))
    return 0


def cmd_run(args: argparse.Namespace) -> int:
    """Head and base sizes in one table; the base comes from the cache file when it holds one."""
    work, target = directories(args)
    cached = read_result(Path(args.base_cache)) if args.base_cache else None
    if cached:
        base, note = cached, 'cached by recipe key'
    else:
        base, note = build_side(args.base, work, target), 'built in this run'
        if args.base_cache:
            write_result(Path(args.base_cache), base)
    head = build_side(args.head, work, target)
    if args.out:
        write_result(Path(args.out), head)
    emit_summary(table(base, head, note))
    return 0


def parser() -> argparse.ArgumentParser:
    top = argparse.ArgumentParser(prog='just packaging sizes', description=__doc__)
    sub = top.add_subparsers(dest='command', required=True)
    p = sub.add_parser('gate', help='print ship=true|false for changes since BASE')
    p.add_argument('base')
    p.set_defaults(func=cmd_gate)
    p = sub.add_parser('apt', help='print the distribution packages the reference backend needs')
    p.set_defaults(func=cmd_apt)
    p = sub.add_parser('key', help='print the recipe key of a commit')
    p.add_argument('--commit', default='HEAD')
    p.set_defaults(func=cmd_key)
    p = sub.add_parser('build', help='build one commit and write its sizes as JSON')
    p.add_argument('--commit', default='HEAD')
    p.add_argument('--out', required=True)
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
    except Failure as error:
        print(f'error: {error}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    # Run as a script only for `gate`, which needs no package import.
    raise SystemExit(main())
