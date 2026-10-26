"""Stateful registry recipes sharing driver, cell and comparison services."""
from collections.abc import Sequence
from dataclasses import dataclass
from pathlib import Path
import re
import shutil
import stat

from .cell import CellError
from .runners import Context, Finished, emit, status, write


@dataclass(frozen=True)
class StateInputs:
    tag: str
    data: Path
    pin: dict[str, Path]
    subject: dict[str, Path]
    libraries: tuple[str, ...]
    output: Path
    work: Path


def state_inputs(ctx: Context, tag: str, defaults: str, *, cross: bool = False,
                 single: bool = False) -> StateInputs:
    variable = tag + '_ORACLE_PREFIX'
    value = ctx.env.get(variable) or ctx.env.get('PINYIN_ORACLE_PREFIX') or ''
    prefix = ctx.path(value)
    if not value or not prefix.is_dir():
        ctx.emit(f'missing input: {variable} is unset or not a directory', error=True)
        ctx.emit('  build it with tools/oracle/build-oracle.sh' + ('' if single else ', configure line plus --enable-libzhuyin'), error=True)
        raise Finished(77)
    pinfile = prefix / 'oracle-pin.txt'
    expected = ctx.settings.expected_pin.split('+model20-')[0]
    if not pinfile.is_file() or not any(line.startswith(expected) for line in pinfile.read_text().splitlines()):
        ctx.emit(f'missing input: {prefix} is not a prefix of the 074a2219 pin (oracle-pin.txt)', error=True)
        raise Finished(3)
    data = prefix / 'lib/libpinyin/data'
    pin = {facade: prefix / f'lib/lib{facade}.so.15' for facade in ('pinyin', 'zhuyin')}
    subject = {facade: ctx.path(ctx.env.get(tag + '_' + facade.upper() + '_SO') or
                               ctx.env.get('OXPINYIN_CAPI_SO' if facade == 'pinyin' else 'OXPINYIN_ZHUYIN_SO') or
                               str(ctx.settings.target / f'debug/lib{facade}_capi.so')) for facade in pin}
    libraries = tuple((ctx.env.get(tag + '_LIBS') or defaults).split())
    if not (data / 'table.conf').is_file():
        ctx.emit(f'missing input: {data}/table.conf', error=True)
        raise Finished(3)
    for library in libraries:
        facades = ('pinyin', 'zhuyin') if cross and library == 'cross' else (library,)
        for facade in facades:
            if facade not in pin:
                ctx.emit(f'unknown library: {library}', error=True)
                raise Finished(3)
            for so in (pin[facade], subject[facade]):
                if not so.is_file():
                    ctx.emit(f'missing input: {so}', error=True)
                    raise Finished(3)
    output = ctx.output(tag + '_OUT')
    work = ctx.scratch() if ctx.env.get(tag + '_OUT') else output
    return StateInputs(tag, data, pin, subject, libraries, output, work)


def verify_steps(library: str, scenario: str, log: bytes) -> tuple[tuple[str, bool], ...]:
    if library == 'cross':
        first, second = ('Z', 'P') if scenario.startswith('zhuyin-first') else ('P', 'Z')
        contexts, learners, savers = [first, second, 'P2', 'Z2'], [first, second], [first, second]
    else:
        contexts, learners, savers = ['A', 'B'], ['A'], ['A', 'B']
        if scenario in ('both-learn', 'late-save'):
            contexts += ['C']
            learners += ['B']
    checks = [(f'scenario: {scenario}', False)]
    checks += [(f'{action} {name}: ok', False) for name in contexts for action in ('init', 'alloc', 'fini')]
    for name in learners:
        checks.append((f'^train {name}: .+ ok$', True))
        if library == 'pinyin' or name == 'P':
            checks.append((f'^import {name}: .+ ok$', True))
    checks += [(f'save {name}: ' + ('unchanged' if library != 'cross' and name == 'B' and scenario in ('one-learns', 'fini-reversed') else 'ok'), False) for name in savers]
    text = log.decode(errors='replace')
    return tuple((pattern, regex) for pattern, regex in checks if not (
        re.search(pattern, text, re.M) if regex else pattern in text.splitlines()))


def two_context(ctx: Context, args: Sequence[str]) -> int:
    inputs = state_inputs(ctx, 'TWO_CONTEXT', 'pinyin zhuyin cross', cross=True)
    ctx.emit('--- building two-context-diff driver ---')
    driver = ctx.compile('two-context-diff', inputs.work / 'two-context-diff', glib=True, absolute_source=True)
    ctx.emit('build: ok')
    verdict = 0
    def one(ctx: Context, library) -> int:
        verdict = 0
        scenarios = (['zhuyin-first', 'zhuyin-first-fini-reversed', 'pinyin-first', 'pinyin-first-fini-reversed',
                      'zhuyin-first-late-save', 'pinyin-first-late-save'] if library == 'cross' else
                     ['one-learns', 'fini-reversed', 'both-learn', 'late-save'])
        facades = ('pinyin', 'zhuyin') if library == 'cross' else (library,)
        for scenario in scenarios:
            paths, logs = [], []
            for side, libraries in [('oracle', inputs.pin), ('oxpinyin', inputs.subject)]:
                log = inputs.output / f'{library}-{scenario}-{side}.log'
                user, temporary = inputs.work / ('user-' + log.stem), inputs.work / ('tmp-' + log.stem)
                for directory in (user, temporary):
                    shutil.rmtree(directory, ignore_errors=True)
                    directory.mkdir()
                objects = [libraries[facade] for facade in facades]
                # Both dlopened facades must pass the same runtime check before
                # a cross-facade driver starts; run() checks the first again.
                env = dict(ctx.env, TMPDIR=str(temporary))
                for obj in objects:
                    ctx.services.check_object(obj, runner=ctx.name, side=str(log), environment=env, cwd=ctx.here)
                result = ctx.run(driver, objects[0], [library, *objects, inputs.data, user, scenario], side=side, env=env)
                log.write_bytes(result.stdout)
                log.with_name(log.name + '.stderr').write_bytes(result.stderr)
                if result.returncode:
                    ctx.emit(f'FAIL: {library}/{scenario} on ' + ' '.join(map(str, objects)), error=True)
                    ctx.write(result.stderr, error=True)
                    raise Finished(1)
                missing = verify_steps(library, scenario, result.stdout)
                if missing:
                    pattern, regex = missing[0]
                    ctx.emit(f"FAIL: missing {'step matching ' if regex else ''}'{pattern}' in {log}", error=True)
                    raise Finished(1)
                paths.append(log)
                logs.append(result.stdout)
            rc, diff = ctx.compare(*logs, paths=tuple(paths))
            (inputs.output / f'{library}-{scenario}.diff').write_bytes(diff)
            if rc > 1:
                ctx.emit(f'FAIL: could not compare {paths[0]} and {paths[1]}', error=True)
                raise Finished(1)
            if rc == 0:
                ctx.emit(f'{library}/{scenario}: IDENTICAL ({logs[0].count(bytes([10]))} lines)')
            else:
                ctx.emit(f'{library}/{scenario}: DIVERGED')
                ctx.write(diff)
                verdict = 2
        return verdict

    results = ctx.campaign(inputs.libraries, one)
    verdict = 1 if 1 in results else 2 if 2 in results else 0
    ctx.emit("RESULT: two contexts on one user dir behave as the pin's do" if not verdict else
         'RESULT: at least one two-context scenario DIVERGED from the pin')
    return verdict


def session_fork(ctx: Context, args: Sequence[str]) -> int:
    inputs = state_inputs(ctx, 'SESSION_FORK', 'pinyin', single=True)
    modes = (ctx.env.get('SESSION_FORK_MODES') or 'fork-_exit fork-exit fork-nosave crash').split()
    timeout = ctx.env.get('SESSION_FORK_TIMEOUT') or '60'
    ctx.emit('--- building session-fork-diff driver ---')
    driver = ctx.compile('session-fork-diff', inputs.work / 'session-fork-diff', glib=True, absolute_source=True)
    ctx.emit('build: ok')
    verdict = 0
    def one(ctx: Context, mode) -> int:
        verdict = 0
        logs, paths = [], []
        for side, so in [('oracle', inputs.pin['pinyin']), ('oxpinyin', inputs.subject['pinyin'])]:
            log = inputs.output / f'{side}-{mode}.log'
            user, temporary = inputs.work / f'user-{side}-{mode}', inputs.work / f'tmp-{side}-{mode}'
            for directory in (user, temporary):
                shutil.rmtree(directory, ignore_errors=True)
                directory.mkdir()
            result = ctx.run(Path('timeout'), so, ['-k', '5', timeout, driver, so, inputs.data, user, mode],
                             side=side, env={'TMPDIR': str(temporary)})
            rc = status(result.returncode)
            log.write_bytes(result.stdout)
            log.with_name(log.name + '.stderr').write_bytes(result.stderr)
            if rc == 124 or rc == 137 and mode != 'crash':
                ctx.emit(f'FAIL: {side}/{mode} did not finish within {timeout}s', error=True)
                verdict = 1
                break
            if rc != (137 if mode == 'crash' else 0):
                ctx.emit(f'FAIL: {side}/{mode} exited with unexpected status {rc}', error=True)
                if ctx.settings.cell == 'tkrzw':
                    ctx.write(result.stderr, error=True)
                verdict = 1
                break
            inventory = sorted('./' + str(path.relative_to(user)) for path in user.rglob('*'))
            body = result.stdout + f'exit: {rc}\nuser dir:\n'.encode() + ''.join('  ' + name + '\n' for name in inventory).encode()
            body += f'tmpdir entries: {sum(1 for _ in temporary.rglob("*"))}\n'.encode()
            log.write_bytes(body)
            logs.append(body)
            paths.append(log)
        else:
            rc, diff = ctx.compare(*logs, paths=tuple(paths))
            (inputs.output / f'{mode}.diff').write_bytes(diff)
            ctx.emit(f'{mode}: ' + ('IDENTICAL' if rc == 0 else 'DIVERGENT'))
            if rc:
                ctx.write(diff)
                verdict = verdict or 2
        return verdict

    results = ctx.campaign(modes, one)
    verdict = 1 if 1 in results else 2 if 2 in results else 0
    return verdict


def file_modes(directory: Path) -> bytes:
    return ''.join(f'{stat.S_IMODE(file.stat().st_mode):o} {file.name}\n' for file in sorted(directory.iterdir(), key=lambda p: p.name)).encode()


def user_modes(ctx: Context, args: Sequence[str]) -> int:
    inputs = state_inputs(ctx, 'MODES', 'pinyin zhuyin')
    masks = (ctx.env.get('MODES_UMASKS') or '022 002 077').split()
    ctx.emit('--- building open-counter-diff driver ---')
    driver = ctx.compile('open-counter-diff', inputs.work / 'open-counter-diff', glib=True, absolute_source=True)
    ctx.emit('build: ok')
    verdict = 0
    def one(ctx: Context, library) -> int:
        verdict = 0
        ctx.emit(f'=== {library} ===')
        logs, paths = [], []
        for side, so in [('oracle', inputs.pin[library]), ('oxpinyin', inputs.subject[library])]:
            log = inputs.output / f'{library}-{side}.log'
            body, diagnostics = b'', b''
            for mask in masks:
                user, temporary = inputs.work / f'user-{mask}-{log.stem}', inputs.work / f'tmp-{mask}-{log.stem}'
                for directory in (user, temporary):
                    shutil.rmtree(directory, ignore_errors=True)
                    directory.mkdir()
                for number in (1, 2):
                    if number == 2:
                        for file in user.iterdir():
                            if file.is_file():
                                file.chmod(0o640)
                    result = ctx.run(driver, so, [library, so, inputs.data, user, str(number), 'cycle'], side=side,
                                     env={'TMPDIR': str(temporary)}, umask=int(mask, 8))
                    diagnostics += result.stderr
                    log.with_name(log.name + '.launch').write_bytes(result.stdout)
                    log.with_name(log.name + '.stderr').write_bytes(diagnostics)
                    if result.returncode:
                        ctx.emit(f'FAIL: launch {number} of {so} under umask {mask}', error=True)
                        ctx.write(diagnostics, error=True)
                        raise Finished(1)
                    if b'save: 1' not in result.stdout.splitlines():
                        ctx.emit(f'FAIL: launch {number} of {so} under umask {mask} saved nothing', error=True)
                        raise Finished(1)
                    prefix = f'umask {mask} launch {number}' + (' after chmod 640' if number == 2 else '') + ': '
                    body += b''.join(prefix.encode() + line for line in file_modes(user).splitlines(keepends=True))
            log.write_bytes(body)
            logs.append(body)
            paths.append(log)
        if not all(logs):
            ctx.emit('FAIL: an empty mode listing', error=True)
            raise Finished(1)
        rc, diff = ctx.compare(*logs, paths=tuple(paths))
        (inputs.output / f'{library}.diff').write_bytes(diff)
        if rc > 1:
            ctx.emit(f'FAIL: could not compare {paths[0]} and {paths[1]}', error=True)
            raise Finished(1)
        if rc == 0:
            ctx.emit(f'{library}: IDENTICAL ({logs[0].count(bytes([10]))} file modes)')
        else:
            ctx.emit(f'{library}: DIVERGED')
            ctx.write(diff)
            verdict = 2
        return verdict

    results = ctx.campaign(inputs.libraries, one)
    verdict = 1 if 1 in results else 2 if 2 in results else 0
    ctx.emit("RESULT: every user file's mode matches the pin" if not verdict else
         "RESULT: at least one user file's mode DIVERGED from the pin")
    return verdict


RECIPES = {'two-context-diff': two_context, 'session-fork-diff': session_fork, 'user-file-modes-diff': user_modes}


def locale_campaign(ctx: Context, args: Sequence[str]) -> int:
    inputs = state_inputs(ctx, 'LOCALE_DIFF', 'pinyin zhuyin')
    forms = (ctx.env.get('LOCALE_DIFF_FORMS') or 'abs empty empty-chdir null dot').split()
    locale = ctx.env.get('LOCALE_DIFF_LOCALE') or 'zh_CN.UTF-8'
    for form in forms:
        if form not in ('abs', 'empty', 'empty-chdir', 'null', 'dot'):
            ctx.emit(f'missing input: unknown form in LOCALE_DIFF_FORMS: {form}', error=True)
            return 3
    available = ctx.command(['locale', '-a'], capture=True)
    normalize = lambda value: value.lower().replace('-', '')
    if normalize(locale) not in map(normalize, available.stdout.decode().splitlines()):
        ctx.emit(f'missing input: locale {locale} is not generated on this host (locale -a)', error=True)
        return 3
    out = inputs.output.absolute()
    driver = ctx.compile('locale-diff', out / 'locale-diff', compiler='cc', flags=('-std=c11', '-O1', '-Wall', '-Wextra'), glib=True, absolute_source=True)
    scratch = out / 'scratch'
    for name in ('empty', 'garbage', 'truncated'):
        (scratch / name).mkdir(parents=True, exist_ok=True)
    (scratch / 'garbage/table.conf').write_text('not a table.conf\n')
    (scratch / 'truncated/table.conf').write_bytes(b''.join((inputs.data / 'table.conf').read_bytes().splitlines(keepends=True)[:2]))
    ctx.emit(f'data dir: {inputs.data}\nlocale:   {locale}')
    verdict = 0
    def one(ctx: Context, library) -> int:
        verdict = 0
        for form in forms:
            paths, logs = [], []
            for side, so in [('oracle', inputs.pin[library]), ('ox', inputs.subject[library])]:
                base = out / f'{library}-{form}-{side}'
                shutil.rmtree(base, ignore_errors=True)
                (base / 'cwd').mkdir(parents=True)
                if form == 'abs':
                    (base / 'userdir').mkdir()
                    extra = [base / 'userdir', scratch]
                elif form == 'empty-chdir':
                    (base / 'moved-to').mkdir()
                    extra = ['', scratch, base / 'moved-to']
                else:
                    extra = [{'empty': '', 'null': 'NULL', 'dot': '.'}[form], scratch]
                body, diagnostics = b'', b''
                for process in (1, 2):
                    body += f'== process {process} ==\n'.encode()
                    result = ctx.run(driver, so, [library, so, inputs.data, *extra], side=side,
                                     cwd=base / 'cwd', env={'LC_ALL': locale})
                    body += result.stdout
                    diagnostics += result.stderr
                    if result.returncode:
                        ctx.emit(f'run failure: {library}/{form}/{side} process {process} (see {base}.err)', error=True)
                        if ctx.settings.cell == 'tkrzw':
                            ctx.write(b''.join(diagnostics.splitlines(keepends=True)[-5:]), error=True)
                        verdict = 1
                log = base.with_name(base.name + '.log')
                log.write_bytes(body)
                base.with_name(base.name + '.err').write_bytes(diagnostics)
                paths.append(log)
                logs.append(body)
            if form != 'null' and logs[0].splitlines().count(b'train: ok') != 2:
                ctx.emit(f"run failure: {library}/{form}: the oracle's train did not set m_modified in both processes; save-modified is not measured", error=True)
                verdict = 1
            rc, diff = ctx.compare(*logs, paths=tuple(paths))
            (out / f'{library}-{form}.diff').write_bytes(diff)
            if rc == 0:
                ctx.emit(f'{library} {form}: identical ({logs[0].count(bytes([10]))} lines)')
            else:
                ctx.emit(f'{library} {form}: DIVERGENT')
                ctx.write(diff)
                verdict = verdict or 2
        return verdict

    results = ctx.campaign(inputs.libraries, one)
    verdict = 1 if 1 in results else 2 if 2 in results else 0
    return verdict


def zhuyin(ctx: Context, args: Sequence[str]) -> int:
    value = ctx.env.get('ZHUYIN_ORACLE_PREFIX') or ctx.env.get('PINYIN_ORACLE_PREFIX') or ''
    prefix = ctx.path(value)
    if not value or not prefix.is_dir():
        ctx.emit('SKIP: ZHUYIN_ORACLE_PREFIX is unset or not a directory\n'
             '  build the pin-built libzhuyin oracle (tools/oracle/build-oracle.sh patched\n'
             '  with --enable-libzhuyin) and set ZHUYIN_ORACLE_PREFIX to its prefix.', error=True)
        return 77
    pin = ctx.path(ctx.env.get('ZHUYIN_ORACLE_SO') or str(prefix / 'lib/libzhuyin.so.15.0.0'))
    pin_data = ctx.path(ctx.env.get('ZHUYIN_ORACLE_DATA') or str(prefix / 'lib/libpinyin/data'))
    subject = ctx.path(ctx.env.get('ZHUYIN_RUST_SO') or ctx.env.get('OXPINYIN_ZHUYIN_SO') or str(ctx.settings.target / 'debug/libzhuyin_capi.so'))
    value = ctx.env.get('ZHUYIN_RUST_DATA') or ''
    data = ctx.path(value)
    work = ctx.scratch()
    pin_user = work / 'oracle-user'
    pin_user.mkdir()
    subject_user = ctx.path(ctx.env['ZHUYIN_USER_DIR']) if ctx.env.get('ZHUYIN_USER_DIR') else ctx.scratch()
    subject_user.mkdir(parents=True, exist_ok=True)
    if not value or not data.is_dir():
        ctx.emit('SKIP: ZHUYIN_RUST_DATA is unset or not a directory\n'
             '  point it at an oxpinyin-native converted systemdir (pinyin_index.bin,\n'
             '  phrase_index.bin, bigram.db, punct.bin + interpolation2.text).', error=True)
        return 77
    for file in (pin, subject):
        if not file.is_file():
            ctx.emit(f'so not found: {file}', error=True)
            return 1
    for directory in (pin_data, data):
        if not (directory / 'interpolation2.text').is_file():
            ctx.emit(f'data dir missing interpolation2.text: {directory}', error=True)
            return 1
    pinned = prefix / 'lib/libpinyin/data'
    if pin_data.resolve() != pinned.resolve():
        for file in sorted(pinned.iterdir()):
            other = pin_data / file.name
            if file.is_file() and (not other.is_file() or other.read_bytes() != file.read_bytes()):
                ctx.emit(f"FAIL: ZHUYIN_ORACLE_DATA differs from the pinned oracle's {file.name}: {pin_data}", error=True)
                return 1
    ctx.emit('== building zhuyin-diff ==')
    driver = ctx.compile('zhuyin-diff', work / 'zhuyin-diff', compiler='cc', flags=('-O2', '-Wall', '-Wextra'), absolute_source=True)
    logs, paths = [], []
    for side, so, directory, user in [('oracle', pin, pin_data, pin_user), ('rust', subject, data, subject_user)]:
        ctx.emit(f'== running against {"oracle" if side == "oracle" else "rust facade"} (data={directory}) ==')
        result = ctx.run(driver, so, [so, directory, user], side='pin' if side == 'oracle' else 'subject')
        log = work / f'zhuyin-{side}.log'
        log.write_bytes(result.stdout)
        if result.returncode:
            ctx.emit(f'FAIL: the {side} driver exited nonzero', error=True)
            ctx.write(result.stderr, error=True)
            return 1
        ctx.emit(f'{side} log lines: {result.stdout.count(bytes([10]))}')
        paths.append(log)
        logs.append(result.stdout)
    if not all(logs):
        ctx.emit('FAIL: one side produced no output (empty log)', error=True)
        ctx.emit(f'  oracle: {paths[0]} ({logs[0].count(bytes([10]))} lines)\n'
             f'  rust:   {paths[1]} ({logs[1].count(bytes([10]))} lines)', error=True)
        return 1
    ctx.emit('== diff (oracle vs rust) ==')
    rc, diff = ctx.compare(*logs, paths=tuple(paths))
    ctx.emit('IDENTICAL' if rc == 0 else 'DIFF FOUND')
    if rc:
        ctx.write(diff)
    return 2 if rc else 0


RECIPES.update({'locale-diff': locale_campaign, 'zhuyin-diff': zhuyin})
