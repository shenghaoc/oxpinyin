"""Explicit cell settings and shared build/identity services for registry runners."""
from collections.abc import Mapping, Sequence
from dataclasses import dataclass, replace
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tempfile

from . import slot

ROOT = Path(__file__).resolve().parents[2]


def patch_digest(directory: Path) -> str:
    rows = ''.join(f'{hashlib.sha256(file.read_bytes()).hexdigest()}  ./{file.name}\n'
                   for file in sorted(directory.glob('*.patch')))
    return hashlib.sha256(rows.encode()).hexdigest()


def bash_word(value: str) -> str:
    """Render invalid settings as the former Bash printf %q diagnostic did."""
    if not value:
        return "''"
    if any(not character.isprintable() for character in value):
        escapes = {'\a': r'\a', '\b': r'\b', '\t': r'\t', '\n': r'\n',
                   '\v': r'\v', '\f': r'\f', '\r': r'\r', '\x1b': r'\E',
                   "'": r"\'", '\\': r'\\'}
        def quote(character):
            if character in escapes:
                return escapes[character]
            if character.isprintable():
                return character
            number = ord(character)
            if 0xdc80 <= number <= 0xdcff:
                number -= 0xdc00
            return f'\\{number:03o}'
        return "$'" + ''.join(map(quote, value)) + "'"
    safe = '_@%+=:,./-'
    return ''.join(character if character.isalnum() or character in safe or ord(character) > 127
                   else '\\' + character for character in value)


def main(argv: Sequence[str], *, root: Path = ROOT,
         environment: Mapping[str, str] | None = None) -> int:
    """Build or describe registry artifacts, preserving the former entry grammar."""
    try:
        environment = os.environ if environment is None else environment
        if list(argv[:1]) == ['--library-path'] and len(argv) in (1, 2):
            sys.stdout.write(library_path(environment.get('LD_LIBRARY_PATH', ''),
                                         Path(argv[1]) if len(argv) == 2 else Path.cwd()))
            return 0
        settings = Settings.read(environment, root)
        if list(argv) == ['--shell']:
            keys = ('ORACLE_DBM', 'ORACLE_DBM_NAME', 'CAPI_FEATURE', 'OXPINYIN_CAPI_BACKEND_EXT',
                    'EXPECTED_PIN_REF', 'CARGO_TARGET_DIR', 'CARGO_PROFILE_DEV_OPT_LEVEL',
                    'OXPINYIN_CAPI_SO', 'OXPINYIN_ZHUYIN_SO')
            for key in keys:
                if key in settings.environment:
                    print(f'export {key}={shlex.quote(settings.environment[key])}')
            return 0
        if len(argv) == 4 and argv[0] == '--artifact':
            print(settings.artifact(*argv[1:]))
            return 0
        if list(argv) == ['--describe']:
            print(json.dumps(settings.describe()))
            return 0
        if len(argv) == 3 and argv[0] == '--describe-oracle':
            print(json.dumps(settings.oracle(argv[1], argv[2]).describe()))
            return 0
        wanted = ['pinyin-capi', 'zhuyin-capi', 'dictool']
        if environment.get('OXPINYIN_BUILD_USER_RT') == '1':
            wanted.append('user-rt-test')
        if len(argv) == 2 and argv[0] == '--artifacts':
            wanted = argv[1].split(',')
        elif argv:
            raise CellError('usage: cell-artifacts [--artifacts LIST | --describe | '
                            '--describe-oracle BASE_PREFIX VARIANT]', 2)
        status = settings.build(wanted)
        return 128 - status if status < 0 else status
    except CellError as exc:
        print(exc, file=sys.stderr)
        return exc.status
    except slot.Cancelled as exc:
        print(exc, file=sys.stderr)
        return slot.cancellation_status()
    except (OSError, ValueError) as exc:
        print(f'FAIL: {exc}', file=sys.stderr)
        return 1


class CellError(ValueError):
    def __init__(self, message: str, status: int = 1):
        super().__init__(message)
        self.status = status


def library_path(value: str, cwd: Path) -> str:
    """Snapshot relative loader entries before a runner changes directory."""
    if not value:
        return ''
    tokens = ('$ORIGIN', '${ORIGIN}', '$LIB', '${LIB}', '$PLATFORM', '${PLATFORM}')
    return ':'.join(entry if entry.startswith('/') or any(t in entry for t in tokens)
                    else str(cwd) + ('/' + entry if entry else '')
                    for entry in value.split(':'))


@dataclass(frozen=True)
class Settings:
    root: Path
    cell: str
    feature: str
    extension: str
    dbm_name: str
    expected_pin: str
    target: Path
    environment: Mapping[str, str]

    @classmethod
    def read(cls, environment: Mapping[str, str], root: Path = ROOT) -> 'Settings':
        root = root.resolve()
        cell = environment.get('PINYIN_ORACLE_DBM') or 'bdb'
        choices = {'bdb': ('bdb', 'db', 'BerkeleyDB'),
                   'kc': ('kyotocabinet', 'kct', 'KyotoCabinet'),
                   'tkrzw': ('tkrzw', 'tkt', 'Tkrzw')}
        if cell not in choices:
            raise CellError(f'fatal: PINYIN_ORACLE_DBM={bash_word(cell)} is not one of: bdb kc tkrzw', 3)
        feature, extension, name = choices[cell]
        source = root / 'tools/oracle/build-oracle.sh'
        constants = dict(re.findall(r'^(LIBPINYIN_VERSION|LIBPINYIN_SHA|MODEL_SHA256)=(.+)$',
                                    source.read_text(), re.M))
        if len(constants) != 3:
            raise CellError(f'fatal: could not read the pin constants from {source}')
        expected = (f'pin_ref=libpinyin-{constants["LIBPINYIN_VERSION"]}-'
                    f'{constants["LIBPINYIN_SHA"]}+model20-{constants["MODEL_SHA256"]}+dbm-{cell}')
        target = Path(environment.get('CARGO_TARGET_DIR') or
                      str(Path(environment.get('OXPINYIN_TARGET_ROOT') or root / 'target/cells') / cell))
        if not target.is_absolute():
            target = root / target
        env = dict(environment, PINYIN_ORACLE_DBM=cell, ORACLE_DBM=cell, ORACLE_DBM_NAME=name,
                   CAPI_FEATURE=feature, OXPINYIN_CAPI_BACKEND_EXT=extension,
                   EXPECTED_PIN_REF=expected, CARGO_TARGET_DIR=str(target),
                   CARGO_PROFILE_DEV_OPT_LEVEL='1')
        for key in ('OXPINYIN_CAPI_SO', 'OXPINYIN_ZHUYIN_SO'):
            if env.get(key) and not Path(env[key]).is_absolute():
                env[key] = str(root / env[key])
        return cls(root, cell, feature, extension, name, expected, target, env)

    def describe(self) -> dict[str, str]:
        keys = ('PINYIN_ORACLE_DBM', 'ORACLE_DBM', 'CAPI_FEATURE', 'OXPINYIN_CAPI_BACKEND_EXT',
                'EXPECTED_PIN_REF', 'CARGO_TARGET_DIR', 'CARGO_PROFILE_DEV_OPT_LEVEL',
                'PINYIN_ORACLE_PREFIX', 'EXPECTED_ORACLE_PIN_REF', 'ORACLE_PATCH_FILE')
        return {key: self.environment[key] for key in keys if key in self.environment}

    def oracle(self, prefix: str, variant: str) -> 'Settings':
        expected, patch = self.expected_pin, ''
        if variant == 'bigram-export-strjoinv':
            prefix += '-' + variant
            patch = 'pin-bigram-export-strjoinv.patch'
            directory = self.root / 'tools/bisection/patches' / variant
            expected += '+patches-' + patch_digest(directory)
        elif variant != 'unpatched':
            raise CellError(f'FAIL: unknown oracle variant: {variant}')
        return replace(self, environment=dict(self.environment, PINYIN_ORACLE_PREFIX=prefix,
                       EXPECTED_ORACLE_PIN_REF=expected, ORACLE_PATCH_FILE=patch))

    def artifact(self, variable: str, filename: str, package: str) -> Path:
        value = self.environment.get(variable)
        if value:
            artifact = Path(value)
            if not artifact.is_absolute():
                artifact = self.root / artifact
        else:
            command = ['cargo', 'build', '--locked', '--manifest-path',
                       str(self.root / 'Cargo.toml'), '-p', package, '--no-default-features',
                       '--features', self.feature]
            if slot.run_command(command, env=self.environment, out=sys.stderr, cwd=self.root, stdout=2):
                raise CellError(f'FAIL: building {package}', 1)
            artifact = self.target / 'debug' / filename
        if not artifact.is_file():
            raise CellError(f'FAIL: missing {variable} artifact: {artifact}')
        return artifact

    def owner(self) -> dict[str, str]:
        return dict(worktree=str(self.root), cell=self.cell, feature=self.feature,
                    profile='dev', opt_level='1')

    def claim(self) -> None:
        self.target.mkdir(parents=True, exist_ok=True)
        path = self.target / 'oxpinyin-cell-identity.json'
        fd, staged = tempfile.mkstemp(prefix='.oxpinyin-cell-claim-', dir=self.target)
        try:
            with os.fdopen(fd, 'w') as output:
                json.dump(dict(schema=1, **self.owner(), artifacts={}, claimed_utc=utc()), output,
                          indent=1, sort_keys=True)
                output.write('\n')
                output.flush()
            try:
                os.link(staged, path)
            except FileExistsError:
                try:
                    owner = json.loads(path.read_text())
                    if not isinstance(owner, dict):
                        raise ValueError('expected a JSON object')
                except (OSError, ValueError, UnicodeError) as exc:
                    raise CellError(f'fatal: unreadable identity record {path}: {exc}') from exc
                claimed = {key: owner.get(key) for key in self.owner()}
                if claimed != self.owner():
                    raise CellError(f'fatal: target directory {path} belongs to '
                                    f'{json.dumps(claimed, sort_keys=True)}; this build is '
                                    f'{json.dumps(self.owner(), sort_keys=True)}. One target directory per '
                                    '(worktree, cell, profile): use another --target-root/CARGO_TARGET_DIR, '
                                    'or remove the directory to start fresh.', 3)
        finally:
            os.unlink(staged)

    def build(self, wanted: Sequence[str]) -> int:
        packages = {'pinyin-capi': 'oxpinyin-capi', 'zhuyin-capi': 'oxpinyin-zhuyin-capi',
                    'dictool': 'oxpinyin-dictool', 'gen-pinyin-table': 'oxpinyin-utils',
                    'gen-zhuyin-table': 'oxpinyin-utils'}
        for item in wanted:
            if item and item not in (*packages, 'user-rt-test'):
                raise CellError(f"fatal: unknown artifact '{item}' (pinyin-capi, zhuyin-capi, dictool, user-rt-test)", 2)
        if not any(wanted):
            return 0
        self.claim()
        common = ['--locked', '--manifest-path', str(self.root / 'Cargo.toml')]
        features = ['--no-default-features', '--features', self.feature]
        selected = [part for package in dict.fromkeys(packages[item] for item in wanted if item in packages)
                    for part in ('-p', package)]
        if selected:
            status = slot.run_command(['cargo', 'build', *common, *selected, *features],
                                      env=self.environment, out=sys.stderr, cwd=self.root)
            if status:
                return status
        if 'user-rt-test' in wanted:
            # A file, not a pipe: run_command supervises the child and drains nothing.
            with tempfile.TemporaryFile('w+') as messages:
                status = slot.run_command(['cargo', 'test', *common, '--profile', 'dev', '--no-run',
                                           '--message-format=json', '-p', 'oxpinyin-runtime', *features,
                                           '--test', 'user_dir_round_trip'],
                                          env=self.environment, out=sys.stderr, cwd=self.root, stdout=messages)
                if status:
                    return status
                messages.seek(0)
                lines = messages.read().splitlines()
            executables = [row['executable'] for line in lines
                           if (row := json.loads(line)).get('reason') == 'compiler-artifact'
                           and row.get('target', {}).get('name') == 'user_dir_round_trip'
                           and row.get('executable')]
            if len(executables) != 1:
                raise CellError('FAIL: expected exactly one user_dir_round_trip executable')
            link = self.target / 'debug/user-dir-round-trip-test'
            staged = link.with_name(f'.{link.name}.{os.getpid()}')
            staged.unlink(missing_ok=True)
            staged.symlink_to(executables[0])
            os.replace(staged, link)
        self.record()
        return 0

    def record(self) -> None:
        def output(*command: str) -> str:
            return subprocess.run(command, cwd=self.root, env=self.environment, capture_output=True,
                                  text=True, check=True).stdout
        rustc = dict(line.split(': ', 1) for line in output('rustc', '-vV').splitlines() if ': ' in line)
        if self.environment.get('OXPINYIN_COMPAT_CONTAINER') == '1':
            from .compat.identity import current_inputs, seal_artifacts
            inputs = current_inputs()
            source = dict(head=inputs['head'], container_source_sha256=inputs['source_sha256'])
        else:
            source = dict(head=output('git', 'rev-parse', 'HEAD').strip(),
                          tracked_diff_sha256=hashlib.sha256(output('git', 'diff', 'HEAD', '--', '.').encode()).hexdigest())
        artifacts = {}
        for name, filename in (('pinyin-capi', 'libpinyin_capi.so'), ('zhuyin-capi', 'libzhuyin_capi.so'),
                               ('dictool', 'oxpinyin-dictool'), ('user-rt-test', 'user-dir-round-trip-test'),
                               ('gen-pinyin-table', 'gen_pinyin_table'), ('gen-zhuyin-table', 'gen_zhuyin_table')):
            file = self.target / 'debug' / filename
            if file.exists():
                stat = file.stat()
                artifacts[name] = dict(path=str(file), size=stat.st_size, mtime_ns=stat.st_mtime_ns)
        record = dict(schema=1, **self.owner(), toolchain=dict(release=rustc.get('release'),
                      commit=rustc.get('commit-hash'), host=rustc.get('host')),
                      source=source, built_utc=utc(), artifacts=artifacts)
        if self.environment.get('OXPINYIN_COMPAT_CONTAINER') == '1':
            record['container'] = seal_artifacts(record, str(self.target))
        path = self.target / 'oxpinyin-cell-identity.json'
        staged = path.with_name(path.name + f'.{os.getpid()}')
        staged.write_text(json.dumps(record, indent=1, sort_keys=True) + '\n')
        os.replace(staged, path)


def utc() -> str:
    return datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
