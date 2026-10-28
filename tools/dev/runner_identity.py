"""Old/new identity of one C driver: one case list, two builds, the same libraries.

`just runner driver-identity STEM [--base REV] [--case NAME]...` builds
tools/bisection/STEM.c from REV (default origin/main) and from the working
tree with the flags its runner uses, then runs every declared case against
the pin, the subject or an empty stub library, once per build. Each run gets
fresh user, empty, TMPDIR and working directories. stdout, stderr, the exit
or signal status and the entries the run leaves behind must be byte-identical
once the run's own directories and the driver's declared mkdtemp names are
normalised; nothing is sorted or filtered. Exit 0 identical, 2 divergent.
"""
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
import argparse
import difflib
import hashlib
import io
from pathlib import Path
import re
import shutil
import tarfile
import tempfile

from .cell import patch_digest
from .runners import Context, Finished

STRICT = ('-std=gnu11', '-Wall', '-Wextra', '-Werror', '-O2')
SIDES = ('pin', 'patched', 'subject', 'stub', 'none')
PLACEHOLDERS = ('so', 'data', 'pindata', 'user', 'empty', 'tmp')


@dataclass(frozen=True)
class Case:
    """One invocation; ARGS and ENV values may name any of PLACEHOLDERS."""
    name: str
    sides: tuple[str, ...]
    args: tuple[str, ...] = ('{so}', '{data}')
    env: tuple[tuple[str, str], ...] = ()


@dataclass(frozen=True)
class Driver:
    """A driver's runner build and cases. TEMPLATES are its mkdtemp prefixes;
    PIN_TABLES 'subject' gives the pin the subject's tables, as its registry row does."""
    cases: tuple[Case, ...]
    templates: tuple[str, ...] = ()
    compiler: str = 'gcc'
    flags: tuple[str, ...] = STRICT
    facade: str = 'pinyin'
    variant: str = 'unpatched'
    pin_tables: str = 'pin'


@dataclass(frozen=True)
class Observation:
    status: int
    stdout: bytes
    stderr: bytes
    left: tuple[str, ...]


# {pindata} lets the subject open the pin's tables, which lack interpolation2.text.
USER_DIR = (('SCHEME_DIFF_USER_DIR', '{user}'),)

# A missing symbol runs against the stub; usage and dlopen refusals need no library.
DRIVERS = {
    'residue-mechanism-diff': Driver(
        templates=('/tmp/residue-mech-', '/tmp/residue-mech-b-'),
        cases=(Case('walk', ('pin', 'subject')),
               Case('missing-symbol', ('stub',)),
               Case('init-failure', ('pin', 'subject'), ('{so}', '{empty}')),
               Case('dlopen-failure', ('none',), ('{empty}/absent.so', '{data}')),
               Case('usage', ('none',), ()))),
    'scheme-diff': Driver(
        cases=(Case('double', ('pin', 'subject'), env=USER_DIR),
               Case('zrm', ('pin', 'subject'), ('{so}', '{data}', '1'), USER_DIR),
               Case('fixture-init', ('subject',), ('{so}', '{pindata}'), USER_DIR),
               Case('missing-symbol', ('stub',), env=USER_DIR),
               Case('init-failure', ('pin', 'subject'), ('{so}', '{empty}'), USER_DIR),
               Case('dlopen-failure', ('none',), ('{empty}/absent.so', '{data}')),
               Case('usage', ('none',), ('{empty}/absent.so',)))),
    'fullpin-diff': Driver(
        cases=(Case('hanyu', ('pin', 'subject'), ('{so}', '{data}', '1'), USER_DIR),
               Case('hanyu-tone', ('pin', 'subject'), ('{so}', '{data}', '1'),
                    (*USER_DIR, ('SCHEME_DIFF_TONE', '1'))),
               Case('luoma', ('patched', 'subject'), ('{so}', '{data}', '2'), USER_DIR),
               Case('secondary-zhuyin', ('patched', 'subject'), ('{so}', '{data}', '3'), USER_DIR),
               Case('fixture-init', ('subject',), ('{so}', '{pindata}', '1'), USER_DIR),
               Case('missing-symbol', ('stub',), ('{so}', '{data}', '1'), USER_DIR),
               Case('init-failure', ('pin', 'subject'), ('{so}', '{empty}', '1'), USER_DIR),
               Case('dlopen-failure', ('none',), ('{empty}/absent.so', '{data}', '1')),
               *(Case(f'bad-scheme-{value or "empty"}', ('none',), ('{empty}/absent.so', '{data}', value))
                 for value in ('0', '4', '2junk', '')),
               Case('usage', ('none',), ('{empty}/absent.so',)))),
    'zhuyin-diff': Driver(
        compiler='cc', flags=('-O2', '-Wall', '-Wextra'), facade='zhuyin', pin_tables='subject',
        cases=(Case('standard', ('pin', 'subject'), ('{so}', '{data}', '{user}')),
               Case('noguess', ('pin', 'subject'), ('{so}', '{data}', '{user}', 'noguess')),
               Case('choose', ('pin', 'subject'), ('{so}', '{data}', '{user}', 'choose')),
               Case('missing-symbol', ('stub',), ('{so}', '{data}', '{user}')),
               Case('init-failure', ('pin', 'subject'), ('{so}', '{empty}', '{user}')),
               Case('dlopen-failure', ('none',), ('{empty}/absent.so', '{data}')),
               Case('usage', ('none',), ('{empty}/absent.so',)))),
    'train-diff': Driver(
        templates=('/tmp/traindiff-user-',),
        cases=(*(Case(f'rounds-{rounds}', ('pin', 'subject'), env=(('TRAINDIFF_ROUNDS', rounds),))
                 for rounds in ('1', '8')),
               *(Case(f'mask-{mask}', ('pin', 'subject'),
                      env=(('TRAINDIFF_ROUNDS', '8'), ('TRAINDIFF_MASK', mask)))
                 for mask in ('user', 'all')),
               Case('dynamic-off-reopen', ('pin', 'subject'),
                    env=(('TRAINDIFF_ROUNDS', '1'), ('TRAINDIFF_OPTIONS', '0x00000188'),
                         ('TRAINDIFF_DUMP_CANDIDATES', '1'), ('TRAINDIFF_REOPEN', '1'))),
               Case('bad-mask', ('pin',), env=(('TRAINDIFF_ROUNDS', '1'), ('TRAINDIFF_MASK', 'some'))),
               Case('missing-symbol', ('stub',)),
               Case('init-failure', ('pin', 'subject'), ('{so}', '{empty}')),
               Case('bad-rounds', ('none',), ('{empty}/absent.so', '{data}'),
                    (('TRAINDIFF_ROUNDS', '65'),)),
               Case('dlopen-failure', ('none',), ('{empty}/absent.so', '{data}')),
               Case('usage', ('none',), ('{empty}/absent.so',)))),
}


def normalise(text: bytes, paths: Mapping[str, Path], templates: Sequence[str]) -> bytes:
    """Name the run's own directories and blank the six-character mkdtemp suffixes."""
    for name, path in sorted(paths.items(), key=lambda item: -len(str(item[1]))):
        text = text.replace(str(path).encode(), b'{' + name.encode() + b'}')
    for template in templates:
        prefix = template.encode()
        text = re.sub(re.escape(prefix) + rb'[A-Za-z0-9]{6}', lambda _: prefix + b'XXXXXX', text)
    return text


def differing(base: Observation, head: Observation) -> list[str]:
    return [name for name in ('status', 'stdout', 'stderr', 'left') if getattr(base, name) != getattr(head, name)]


def scan(templates: Sequence[str]) -> set[Path]:
    """Existing entries named by the absolute templates (the drivers' fixed /tmp ones)."""
    found = set()
    for template in templates:
        if template.startswith('/'):
            path = Path(template)
            found.update(path.parent.glob(path.name + '*'))
    return found


def extract(ctx: Context, revision: str, destination: Path) -> Path:
    archive = ctx.command(['git', '-C', ctx.settings.root, 'archive', '--format=tar', revision,
                           'tools/bisection'], capture=True)
    if archive.returncode:
        ctx.write(archive.stderr, error=True)
        raise Finished(1)
    with tarfile.open(fileobj=io.BytesIO(archive.stdout)) as tar:
        members = [member for member in tar.getmembers()
                   if member.isfile() and member.name.endswith(('.c', '.h'))]
        tar.extractall(destination, members=members, filter='data')
    return destination / 'tools/bisection'


def build(ctx: Context, driver: Driver, source: Path, destination: Path) -> Path:
    destination.parent.mkdir(parents=True, exist_ok=True)
    ctx.checked([driver.compiler, *driver.flags, '-o', destination, source, '-ldl'])
    return destination


def pin(ctx: Context, driver: Driver) -> tuple[Path, Path]:
    if driver.variant == 'unpatched':
        so, data = ctx.pin(require_data=True)
    else:
        settings = ctx.settings.oracle(ctx.env.get('PINYIN_ORACLE_PREFIX') or
                                       str(Path.home() / '.local/opt/pinyin-oracle'), driver.variant)
        prefix = ctx.path(settings.environment['PINYIN_ORACLE_PREFIX'])
        record = prefix / 'oracle-pin.txt'
        if not record.is_file() or settings.environment['EXPECTED_ORACLE_PIN_REF'] not in record.read_text().splitlines():
            ctx.emit(f'FAIL: {prefix} is not the {driver.variant} oracle for the {ctx.settings.cell} cell')
            raise Finished(1)
        so, data = prefix / 'lib/libpinyin.so', prefix / 'lib/libpinyin/data'
    return (so.parent / 'libzhuyin.so.15.0.0' if driver.facade == 'zhuyin' else so), data


def patched(ctx: Context) -> tuple[Path, Path]:
    """The scheme runner's tools/oracle/patches pin, which full schemes 2 and 3 need."""
    prefix = ctx.path(ctx.env.get('PINYIN_PATCHED_ORACLE_PREFIX') or
                      str(Path.home() / '.local/opt/pinyin-oracle-patched'))
    record = prefix / 'oracle-pin.txt'
    expected = ctx.settings.expected_pin + '+patches-' + patch_digest(ctx.settings.root / 'tools/oracle/patches')
    if not record.is_file() or expected not in record.read_text().splitlines():
        ctx.emit(f'FAIL: {prefix} is not the tools/oracle/patches oracle for the {ctx.settings.cell} cell')
        raise Finished(1)
    return prefix / 'lib/libpinyin.so', prefix / 'lib/libpinyin/data'


def libraries(ctx: Context, driver: Driver, sides: set[str], root: Path) -> dict[str, tuple[Path | None, Path]]:
    pin_so, pin_data = pin(ctx, driver)
    # The subject's tables only when a subject side or the pin (pin_tables 'subject') reads them.
    if 'subject' in sides or driver.pin_tables == 'subject':
        subject_data = ctx.data.resolve('OXPINYIN_SYSTEM_DIR', 'driver-identity')
        if driver.pin_tables == 'subject':
            pin_data = subject_data
    inputs = {'pin': (pin_so, pin_data), 'none': (None, pin_data)}
    if 'patched' in sides:
        inputs['patched'] = patched(ctx)
    if 'subject' in sides:
        inputs['subject'] = (ctx.artifact(driver.facade), subject_data)
    if 'stub' in sides:
        source, stub = root / 'stub.c', root / 'libdriver-identity-stub.so'
        source.write_text('int driver_identity_stub;\n')
        ctx.checked(['cc', '-shared', '-fPIC', '-o', stub, source])
        inputs['stub'] = (stub, pin_data)
    return inputs


def observe(ctx: Context, driver: Driver, binary: Path, stage: Path, case: Case, side: str,
            library: tuple[Path | None, Path], pin_data: Path, root: Path) -> Observation:
    run = Path(tempfile.mkdtemp(prefix='run-', dir=root))
    directories = {name: run / name for name in ('user', 'empty', 'tmp', 'cwd')}
    for directory in directories.values():
        directory.mkdir()
    shutil.copy2(binary, stage)
    values = dict(so=str(library[0] or ''), data=str(library[1]), pindata=str(pin_data),
                  **{name: str(path) for name, path in directories.items() if name != 'cwd'})
    argv = [str(stage), *(argument.format(**values) for argument in case.args)]
    environment = dict(ctx.env, TMPDIR=str(directories['tmp']),
                       **{name: value.format(**values) for name, value in case.env})
    before = scan(driver.templates)
    result = ctx.services.run(argv, library[0] or stage, runner=ctx.name,
                              side='pin' if side in ('pin', 'patched') else 'subject',
                              cwd=directories['cwd'], environment=environment)
    leaked = sorted(scan(driver.templates) - before)
    left = [f'{{{name}}}/{path.relative_to(directories[name])}'
            for name in ('user', 'tmp', 'cwd') for path in sorted(directories[name].rglob('*'))]
    left += [str(path) for path in leaked]
    for path in leaked:
        if path.is_dir() and not path.is_symlink():
            shutil.rmtree(path)
        else:
            path.unlink()
    paths = dict(directories, run=run)
    return Observation(result.returncode, normalise(result.stdout, paths, driver.templates),
                       normalise(result.stderr, paths, driver.templates),
                       tuple(normalise(entry.encode(), paths, driver.templates).decode() for entry in left))


def report(ctx: Context, name: str, base: Observation, head: Observation) -> None:
    for field in differing(base, head):
        before, after = getattr(base, field), getattr(head, field)
        if field == 'status':
            ctx.emit(f'  status: base {before}, head {after}')
            continue
        lines = (list(before) if field == 'left' else before.decode(errors='replace').splitlines(),
                 list(after) if field == 'left' else after.decode(errors='replace').splitlines())
        for line in list(difflib.unified_diff(*lines, f'base/{name}/{field}', f'head/{name}/{field}',
                                              lineterm=''))[:60]:
            ctx.emit('  ' + line)


def identity(ctx: Context, args: Sequence[str]) -> int:
    parser = argparse.ArgumentParser(prog='driver-identity')
    parser.add_argument('stem', choices=sorted(DRIVERS))
    parser.add_argument('--base', default='origin/main')
    parser.add_argument('--case', action='append', default=[])
    options = parser.parse_args(args)
    driver = DRIVERS[options.stem]
    unknown = sorted(set(options.case) - {case.name for case in driver.cases})
    if unknown:
        ctx.emit('unknown case: ' + ', '.join(unknown))
        return 2
    cases = [case for case in driver.cases if not options.case or case.name in options.case]
    revision = ctx.command(['git', '-C', ctx.settings.root, 'rev-parse', '--verify', '--short',
                            options.base + '^{commit}'], capture=True)
    if revision.returncode:
        ctx.write(revision.stderr, error=True)
        return 1
    root = ctx.scratch()
    source = f'{options.stem}.c'
    builds = {'base': build(ctx, driver, extract(ctx, options.base, root / 'base-tree') / source,
                            root / 'base' / options.stem),
              'head': build(ctx, driver, ctx.here / source, root / 'head' / options.stem)}
    inputs = libraries(ctx, driver, {side for case in cases for side in case.sides}, root)
    stage = root / 'stage' / options.stem
    stage.parent.mkdir()
    runs = divergent = 0
    for case in cases:
        for side in case.sides:
            seen = {name: observe(ctx, driver, binary, stage, case, side, inputs[side],
                                  inputs['pin'][1], root)
                    for name, binary in builds.items()}
            label = f'{case.name}/{side}'
            runs += 1
            head = seen['head']
            if differing(seen['base'], head):
                divergent += 1
                ctx.emit(f'DIFFER {label}: ' + ', '.join(differing(seen['base'], head)))
                report(ctx, label, seen['base'], head)
            else:
                digest = hashlib.sha256(head.stdout).hexdigest()[:12]
                ctx.emit(f'MATCH  {label}: status={head.status} stdout={len(head.stdout)}B/{digest} '
                         f'stderr={len(head.stderr)}B left={len(head.left)}')
    verdict = 'DIVERGENCE' if divergent else 'IDENTICAL'
    ctx.emit(f'driver-identity {options.stem}: {verdict} ({runs - divergent}/{runs} runs match, '
             f'base {revision.stdout.decode().strip()})')
    return 2 if divergent else 0


RECIPES = {'driver-identity': identity}
