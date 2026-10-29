"""Registry recipes using the shared cell, data, compiler and comparison services."""
from collections.abc import Sequence
from dataclasses import dataclass, replace
from pathlib import Path
import argparse
import os
import re
import shutil
import sys

from . import slot
from .cell import CellError, Settings, patch_digest
from .frontend import frontend_source
from .runners import Context, Finished, emit, python_script, status, write
from .system_data import NAMES, data_extension


def explicit_inputs(ctx: Context, args: Sequence[str], mode: str = 'pinyin') -> tuple[Path, Path, Path]:
    if not args:
        raise CellError('path to the pin-built lib' + mode + '.so is required')
    pin = ctx.path(args[0], caller=True)
    if not pin.is_file():
        ctx.emit(f'SKIP: missing oracle: {args[0]}', error=True)
        raise Finished(77)
    variable = 'OXPINYIN_CAPI_SO' if mode == 'pinyin' else 'OXPINYIN_ZHUYIN_SO'
    subject = (args[1] if len(args) > 1 and args[1] else ctx.env.get(variable))
    if not subject:
        raise CellError('prebuilt lib' + mode + ' required')
    if len(args) < 3:
        raise CellError('data directory is required')
    subject = ctx.path(subject, caller=True).absolute()
    data = ctx.path(args[2], caller=True).absolute()
    for directory in (subject.parent, data):
        if not directory.is_dir():
            ctx.emit(f'{directory}: No such file or directory', error=True)
            raise Finished(1)
    return pin.absolute(), subject, data


def return_law(log: bytes, scenario: str) -> bool:
    lines = log.splitlines()
    if any(line.startswith(b'train ') and not line.endswith(b': true') for line in lines):
        return False
    if any(line.startswith(b'row (null)') for line in lines):
        return False
    values = [line.split(b'\tget=', 1)[-1] for line in lines if line.startswith(b'row ')]
    return (bool(values) or scenario == 'empty') and values == [b'true'] * max(0, len(values) - 1) + ([b'false'] if values else [])


def explicit_compare(ctx: Context, args: Sequence[str]) -> int:
    mode = 'zhuyin' if ctx.name == 'zhuyin-import-diff' else 'pinyin'
    pin, subject, data = explicit_inputs(ctx, args, mode)
    work = ctx.scratch()
    stem = ctx.name
    if stem == 'bigram-export-diff':
        prefix = pin.parent.parent
        manifest = prefix / 'oracle-patches.sha256'
        if not manifest.is_file() or 'pin-bigram-export-strjoinv.patch' not in manifest.read_text():
            ctx.emit(f'FAIL: {prefix} was not built with tools/bisection/patches/bigram-export-strjoinv')
            return 1
    driver = ctx.compile(stem, work / ('export-diff' if stem == 'export-diff' else 'driver'),
                         glib=stem in ('zhuyin-import-diff', 'bigram-export-diff'))
    if stem == 'export-diff':
        logs = {}
        for side, so in [('oracle', pin), ('oxpinyin', subject)]:
            cwd = work / side
            cwd.mkdir()
            result = ctx.run(driver, so, [so, data], side=side, cwd=cwd, loader=True)
            if result.returncode:
                ctx.emit(f'FAIL: export-diff crashed against {side}')
                ctx.write(b''.join(result.stderr.splitlines(keepends=True)[-5:]))
                return 1
            logs[side] = result.stdout
        ctx.emit('oracle summary:')
        summary = b''.join(b'  ' + line for line in logs['oracle'].splitlines(keepends=True) if not line.startswith(b'row '))
        ctx.write(summary)
        if not summary:
            return 1
        rc, diff = ctx.compare(logs['oracle'], logs['oxpinyin'])
        if rc == 0:
            ctx.emit(f'export-diff: IDENTICAL ({logs["oracle"].count(bytes([10]))} lines)')
            return 0
        differing = sum(bool(re.match(rb'^[-+][^-+]', line)) for line in diff.splitlines())
        ctx.emit(f'export-diff: DIVERGENCE ({differing} differing lines)')
        ctx.write(b''.join(diff.splitlines(keepends=True)[:60]))
        return 2
    scenarios = ['empty', 'one', 'two', 'many', 'repeat', 'reopen', 'mask'] if stem == 'bigram-export-diff' else list(map(str, (0, 1, 2, 3, 4, 5, 6, 7, 15, 16, 255)))
    verdict = 0
    gate = ctx.env.get('BIGRAM_DIFF_GATE') or 'full'
    def one(ctx: Context, item) -> int:
        scenario = item
        verdict = 0
        logs, codes = {}, {}
        for side, so in [('oracle', pin), ('oxpinyin', subject)]:
            cwd = work / (side + '-' + scenario)
            cwd.mkdir()
            result = ctx.run(driver, so, [so, data, scenario], side=side, cwd=cwd, loader=True, quiet=True)
            logs[side], codes[side] = result.stdout, status(result.returncode)
            if result.returncode and scenario not in ('16', '255'):
                label = scenario if stem == 'bigram-export-diff' else 'library ' + scenario
                ctx.emit(f'{label}: FAIL ({side} driver exited nonzero)')
                verdict = 1
                break
        else:
            if scenario in ('16', '255') and stem != 'bigram-export-diff':
                if codes['oxpinyin'] or re.search(rb': true$', logs['oxpinyin'], re.M):
                    ctx.emit(f'library {scenario}: DIVERGENCE (oxpinyin rc={codes["oxpinyin"]}; every add must be false)')
                    verdict = verdict or 2
                else:
                    ctx.emit(f'library {scenario}: oxpinyin refuses every add; pin rc={codes["oracle"]} (class (b), not reproduced)')
                return verdict
            if stem == 'bigram-export-diff':
                law = 'ok'
                if not return_law(logs['oracle'], scenario):
                    law = 'pin breaks the law'
                if not return_law(logs['oxpinyin'], scenario):
                    law = 'oxpinyin breaks the law'
                if law != 'ok':
                    ctx.emit(f'{scenario}: DIVERGENCE (row 36: {law})')
                    verdict = verdict or 2
                    return verdict
            rc, diff = ctx.compare(logs['oracle'], logs['oxpinyin'])
            label = scenario if stem == 'bigram-export-diff' else 'library ' + scenario
            if rc == 0:
                if stem == 'bigram-export-diff':
                    count = sum(line.startswith(b'row ') for line in logs['oracle'].splitlines())
                    ctx.emit(f'{scenario}: IDENTICAL ({count} rows)')
                else:
                    ctx.emit(f'{label}: IDENTICAL ({logs["oracle"].count(bytes([10]))} lines)')
            else:
                if stem == 'bigram-export-diff' and gate != 'full' and scenario not in ('empty', 'one'):
                    counts = [sum(line.startswith(b'row ') for line in logs[side].splitlines()) for side in ('oracle', 'oxpinyin')]
                    ctx.emit(f'{scenario}: returns law holds on both sides; walk differs (pin {counts[0]} rows, oxpinyin {counts[1]} rows; not gated)')
                else:
                    ctx.emit(f'{label}: DIVERGENCE')
                    verdict = verdict or 2
                ctx.write(b''.join(diff.splitlines(keepends=True)[:40]))
        return verdict

    results = ctx.campaign(scenarios, one)
    verdict = 1 if 1 in results else 2 if 2 in results else 0
    result = ('PASS' if stem == 'bigram-export-diff' else 'IDENTICAL') if verdict == 0 else ('DIVERGENCE' if verdict == 2 else 'FAILURE')
    ctx.emit(f'{stem}' + (f' ({gate})' if stem == 'bigram-export-diff' else '') + ': ' + result)
    return verdict


DEFAULT_DRIVERS = ('key-surface-diff', 'key-parse-diff', 'tie-order-diff', 'dict-surface-diff',
                   'phrase-surface-diff', 'pred-order-diff', 'predict-diff', 'punct-diff',
                   'addon-candidate-diff', 'user-candidate-diff', 'union-diff', 'import-diff',
                   'live-typing-diff', 'nbest-train-diff', 'dynamic-adjust-diff')


def dynamic_observations(ctx: Context, driver: Path, pin: Path, subject: Path,
                         pin_data: Path, subject_data: Path, work: Path, *, loader: bool = False):
    logs = {}
    for side, so, data in [('oracle', pin, pin_data), ('oxpinyin', subject, subject_data)]:
        for mode in ('on', 'off'):
            cwd = work / (side + '-' + mode)
            (cwd / 'user').mkdir(parents=True)
            result = ctx.run(driver, so, [so, data, mode, cwd / 'user'], side=side, cwd=cwd, loader=loader)
            if result.returncode:
                raise CellError(f'  {driver.name} crashed against {side} ({mode})')
            ctx.write(result.stderr, error=True)
            logs[side + '-' + mode] = result.stdout
    choices = {key: [line for line in value.splitlines() if line.startswith(b'CHOICE|')] for key, value in logs.items()}
    for key, values in choices.items():
        if len(values) != 7:
            raise CellError(f'  FAIL: {key} did not report all fixed choices')
        if values != choices['oracle-on']:
            raise CellError(f'  FAIL: chosen identity or advanced cursor differs for {key}')
    for side in ('oracle', 'oxpinyin'):
        if logs[side + '-on'] == logs[side + '-off']:
            raise CellError(f'  VACUOUS: {side} produced identical output with the bit set and clear.')
    return logs


def same_data(ctx: Context, args: Sequence[str]) -> int:
    pin, subject, data = explicit_inputs(ctx, args)
    for file in (pin, subject):
        if not file.is_file():
            raise CellError(f'fatal: {file} not found')
    if not (data / 'gb_char.bin').is_file():
        raise CellError(f'fatal: {data} holds no gb_char.bin')
    drivers = args[3:] or DEFAULT_DRIVERS
    ctx.emit(f'data dir: {data}\noracle:   {pin}\noxpinyin: {subject}')
    verdict = 0
    def one(ctx: Context, item) -> int:
        stem = item
        verdict = 0
        ctx.emit(f'--- {stem} ---')
        work = ctx.scratch()
        try:
            driver = ctx.compile(stem, work / stem, flags=('-std=gnu11', '-Wall', '-Wextra', '-O2'), optional_glib=True)
        except Finished:
            ctx.emit('  build failed')
            verdict = 1
            return verdict
        if stem == 'dynamic-adjust-diff':
            try:
                logs = dynamic_observations(ctx, driver, pin, subject, data, data, work, loader=True)
            except CellError as error:
                ctx.emit(str(error))
                verdict = 1
                return verdict
            ctx.emit("  non-vacuity: both engines' output changes with the bit")
            for mode in ('on', 'off'):
                rc, diff = ctx.compare(logs['oracle-' + mode], logs['oxpinyin-' + mode], labels=('oracle-' + mode, 'oxpinyin-' + mode))
                if rc == 0:
                    ctx.emit(f'  DYNAMIC_ADJUST={mode}: IDENTICAL ({logs["oracle-" + mode].count(bytes([10]))} log lines)')
                else:
                    ctx.emit(f'  DYNAMIC_ADJUST={mode}: DIVERGENCE')
                    ctx.write(b''.join(b'    ' + line for line in diff.splitlines(keepends=True)[:60]))
                    verdict = verdict or 2
            return verdict
        logs = {}
        for side, so in [('oracle', pin), ('oxpinyin', subject)]:
            cwd = work / side
            cwd.mkdir()
            result = ctx.run(driver, so, [so, data, *(['matched-tables'] if stem == 'punct-diff' else [])],
                             side=side, cwd=cwd, loader=True)
            ctx.write(result.stderr, error=True)
            if result.returncode:
                ctx.emit(f'  {stem} crashed against {side}')
                verdict = 1
                break
            logs[side] = result.stdout
        else:
            rc, diff = ctx.compare(logs['oracle'], logs['oxpinyin'], labels=('oracle', 'oxpinyin'))
            if rc == 0:
                ctx.emit(f'  IDENTICAL ({logs["oracle"].count(bytes([10]))} log lines)')
            else:
                count = sum(bool(re.match(rb'^[-+][^-+]', line)) for line in diff.splitlines())
                ctx.emit(f'  DIVERGENCE ({count} differing lines):')
                ctx.write(b''.join(b'    ' + line for line in diff.splitlines(keepends=True)[:60]))
                verdict = verdict or 2
        return verdict

    results = ctx.campaign(drivers, one)
    verdict = 1 if 1 in results else 2 if 2 in results else 0
    ctx.emit()
    ctx.emit(f'same-data-dir differential: ' + (f'IDENTICAL on {len(drivers)} drivers' if verdict == 0 else 'DIVERGENCE' if verdict == 2 else 'FAILURE'))
    return verdict


def w11(ctx: Context, args: Sequence[str]) -> int:
    if not args:
        raise CellError('driver stem is required')
    stem = args[0]
    if not re.fullmatch('[a-z0-9-]+', stem):
        ctx.emit(f'FAIL: invalid driver stem: {stem}', error=True)
        return 1
    pin, pin_data = ctx.pin()
    ctx.emit(f'oracle: {pin}')
    work = ctx.scratch()
    ctx.emit(f'--- building {stem} driver ---')
    driver = ctx.compile(stem, work / stem)
    ctx.emit('build: ok')
    if len(args) > 1 and args[1]:
        subject = ctx.path(args[1], caller=True)
        if not subject.is_file():
            raise CellError(f'FAIL: missing OXPINYIN_CAPI_SO artifact: {subject}')
    else:
        subject = ctx.artifact()
    data = ctx.data.resolve('CAPI_W11_SYSTEM_DIR', 'w11-diff/' + stem)
    extra = ['punctuation-only'] if stem == 'punct-diff' else args[2:]
    pin_log, subject_log = ctx.pair(driver, pin, subject, pin_data, data, args=extra, quiet=True)
    ctx.emit('--- capi ---')
    ctx.write(subject_log)
    ctx.emit('--- oracle ---')
    ctx.write(pin_log)
    rc, diff = ctx.compare(pin_log, subject_log)
    if rc:
        ctx.emit(f'DIVERGENCE: {stem} logs differ')
        ctx.write(diff)
        return 2
    ctx.emit(f'{stem}: IDENTICAL')
    return 0


RECIPES = {name: explicit_compare for name in ('export-diff', 'import-semantics-diff', 'zhuyin-import-diff', 'bigram-export-diff')}
RECIPES.update({'same-data-dir-diff': same_data, 'w11-diff': w11})


@dataclass(frozen=True)
class SurfaceRecipe:
    variable: str
    required: tuple[str, ...]
    flags: tuple[str, ...]
    patterns: tuple[str, ...]
    inactive: str
    glib: bool = False


SURFACES = {
    'key-surface-diff': SurfaceRecipe('KEY_SYSTEM', ('pinyin_index.bin', 'phrase_index.bin', 'bigram.db', 'interpolation2.text'),
        ('-O2',), tuple([f'^full\\|{word}\\|' for word in ('0x18a', '0x1aa', '0x1ea', '0x1ca')] +
        [f'^double\\|{scheme}\\|0x18a\\|' for scheme in range(1, 7)] +
        [f'^chewing\\|{scheme}\\|0x18a\\|' for scheme in (1, 2, 3, 4, 5, 6, 8, 9)] +
        [f'^render\\|full\\|.*\\|{kind}\\|' for kind in ('zhuyin', 'pinyin', 'luoma', 'secondary', 'shengmu', 'yunmu', 'shengmu_skip', 'incomplete')] +
        [r'^zero\|' + kind + r'\|' for kind in ('zhuyin', 'strings', 'incomplete')] +
        [r'^context\|match\|', r'^unload_addon\|0\|', r'^unload_addon\|5\|', r'^unload_addon\|15\|']),
        'FAIL: probe matrix is incomplete (missing profile / scheme / render kind / zero / context / addon row)\n  An inactive slice means that surface compared nothing.'),
    'dict-surface-diff': SurfaceRecipe('DICT_SYSTEM', ('pinyin_index.bin', 'phrase_index.bin', 'bigram.db', 'punct.bin', 'interpolation2.text'),
        ('-O2', '-Wall'), tuple([r'^lookup\|你好\|1\|n=1\|', r'^token\|你好\|phrase=1\|len=2\|',
        r'^token\|你好\|add11=1\|.*shift=1$', r'^token\|中国\|add11=1\|.*shift=1$', '^absent-add=0'] +
        [r'^unload\|' + str(i) + r'\|' for i in range(8)] + [r'^load\|' + str(i) + r'\|' for i in (1, 2, 4, 7)]),
        'FAIL: a probe family is inactive (lookup / token / add / load-unload)', True),
    'phrase-surface-diff': SurfaceRecipe('PHRASE_SYSTEM', ('pinyin_index.bin', 'phrase_index.bin', 'bigram.db', 'interpolation2.text'),
        ('-O2', '-Wall'), (r'^segment\|你好中国\|1\|n=4\|', r'^segment\|你好，世界。\|0\|n=6\|',
         r'^prefix\|nihaoshijie\|你好\|1\|', r'^reset\|rok=1\|n=0\|'),
        'FAIL: a probe family is inactive (segment / prefix-guess / reset)')}


def surface(ctx: Context, args: Sequence[str]) -> int:
    spec = SURFACES[ctx.name]
    pin, pin_data = ctx.pin(hint=ctx.name == 'key-surface-diff')
    subject = ctx.artifact()
    ctx.emit(f'--- cc {ctx.name}.c ---')
    work = ctx.scratch()
    flags = list(spec.flags)
    link_flags = []
    if spec.glib:
        result = ctx.command(['pkg-config', '--libs', 'glib-2.0'], capture=True)
        if result.returncode:
            ctx.emit('FAIL: pkg-config could not resolve glib-2.0; install libglib2.0-dev (Debian/Ubuntu) or glib2-devel (Fedora)', error=True)
            return 1
        import shlex
        link_flags = shlex.split(result.stdout.decode())
    driver = ctx.compile(ctx.name, work / ctx.name, compiler='cc', flags=flags, link_flags=link_flags)
    value = ctx.env.get(spec.variable) or ''
    directory = ctx.path(value) if value else None
    if directory is not None and ctx.name == 'phrase-surface-diff' and not Path(value).is_absolute():
        directory = ctx.settings.root / value
    missing = [name for name in spec.required if directory is None or not (directory / name).is_file()]
    if missing:
        ctx.emit(f'SKIP: {spec.variable} must name a {ctx.settings.dbm_name} system data dir')
        ctx.emit('  missing: ' + ' '.join(missing))
        return 77
    have = data_extension(directory)
    if have is None:
        ctx.emit(f'FAIL: cannot tell which backend wrote {spec.variable}={value}')
        ctx.emit('  (no datagen-manifest.txt backend= and no known table.conf database format:)')
        return 1
    if have != ctx.settings.extension:
        ctx.emit(f"FAIL: BACKEND MISMATCH — the {ctx.settings.cell} cell's capi is {NAMES[ctx.settings.extension]},")
        ctx.emit(f'  but {spec.variable}={value} was written for {NAMES[have]}')
        return 1
    logs = {}
    for side, so, data in [('subject', subject, directory), ('pin', pin, pin_data)]:
        ctx.emit('--- capi side ---' if side == 'subject' else '--- oracle side ---')
        result = ctx.run(driver, so, [so, data], side=side, cwd=ctx.scratch())
        ctx.write(result.stderr, error=True)
        if result.returncode:
            ctx.emit(f'FAIL: {ctx.name} crashed against ' + ('oxpinyin-capi' if side == 'subject' else 'oracle'))
            ctx.write(result.stdout)
            return 1
        logs[side] = result.stdout
        ctx.emit('oxpinyin-capi: ok' if side == 'subject' else 'oracle: ok')
    ctx.emit('--- prerequisite: every probe family is active on both sides ---')
    for log in logs.values():
        text = log.decode(errors='replace')
        if not all(re.search(pattern, text, re.M) for pattern in spec.patterns):
            ctx.emit(spec.inactive)
            return 3
    rc, diff = ctx.compare(logs['pin'], logs['subject'])
    ctx.write(diff)
    if rc == 0:
        ctx.emit(f'IDENTICAL: {logs["subject"].count(bytes([10]))} probe lines agree with the pin')
        return 0
    ctx.emit('\nDIVERGENCE: the logs differ (above)')
    return 2


RECIPES.update({name: surface for name in SURFACES})


def build_subject(ctx: Context, *, heading: str | None = None, print_capi: bool = True,
                  flags: Sequence[str] = ('-std=gnu11', '-Wall', '-Wextra', '-Werror', '-O2'),
                  stem: str | None = None):
    ctx.emit('--- building ' + (heading or ctx.name) + ' driver ---')
    driver = ctx.compile(stem or ctx.name, flags=flags)
    ctx.emit('build: ok\n--- building oxpinyin-capi ---')
    subject = ctx.artifact()
    if print_capi:
        ctx.emit(f'capi: {subject}')
    return driver, subject


def active_rows(log: bytes, patterns: Sequence[str]) -> bool:
    return all(re.search(pattern, log.decode(errors='replace'), re.M) for pattern in patterns)


def nbest_witness_cases(path: Path) -> dict[str, dict[str, str]]:
    """Read the diagnostic driver's fixed TSV grammar before starting the pin."""
    cases = {}
    for number, line in enumerate(path.read_text().splitlines(), 1):
        if not line or line.startswith('#'):
            continue
        fields = line.split('\t')
        if len(fields) != 5:
            raise CellError(f'{path.name}:{number}: expected five tab-separated fields')
        case_id, parser, scheme, options, text = fields
        if not re.fullmatch(r'[a-zA-Z0-9_.-]+', case_id) or case_id in cases:
            raise CellError(f'{path.name}:{number}: invalid or duplicate case id')
        if (parser, scheme) not in (('full', '1'), ('double', '2')) or options not in ('0x1fe00198', '0x1fe00018'):
            raise CellError(f'{path.name}:{number}: unsupported parser, scheme or options')
        if not re.fullmatch(r"[a-z;']+", text):
            raise CellError(f'{path.name}:{number}: expected nonempty ASCII pinyin')
        cases[case_id] = dict(parser=parser, scheme=scheme, options=options, input=text)
    if not cases:
        raise CellError('empty n-best witness case file')
    return cases


def nbest_witness_results(cases: dict[str, dict[str, str]], stdout: bytes, stderr: bytes) -> dict:
    """Validate capture completeness; keep pin floats as strings, never recompute predicates."""
    def fields(line):
        result = {}
        for field in line.split('\t')[1:]:
            key, separator, value = field.partition('=')
            if not separator or key in result:
                raise CellError('malformed n-best witness record')
            result[key] = value
        return result

    try:
        completed, totals = {}, []
        for line in stdout.decode().splitlines():
            if line.startswith('case\t'):
                row = fields(line)
                case_id = row.pop('id')
                if case_id not in cases or case_id in completed:
                    raise CellError('unexpected or duplicate completed case')
                expected = dict(cases[case_id], parsed=str(len(cases[case_id]['input'])), guess='1')
                if row != expected:
                    raise CellError(f'{case_id}: incomplete parse, failed guess or wrong case parameters')
                completed[case_id] = row
            elif line.startswith('cases\t'):
                totals.append(fields(line))
        if list(completed) != list(cases) or totals != [dict(completed=str(len(cases)))]:
            raise CellError('incomplete n-best witness case set')

        seen = {case_id: {'S1': set(), 'S2': set(), 'S3': set()} for case_id in cases}
        summary = {site: dict(calls=0, witnessed_calls=0, pairs=0, first=None) for site in ('S1', 'S2', 'S3')}
        for line in stderr.decode().splitlines():
            # A fresh user directory makes the pin print a user.conf message
            # without a newline immediately before the first beam record.
            _, marker, record = line.partition('precondition\t')
            if not marker:
                continue
            line = marker + record
            row = fields(line)
            case_id, site = row['case'], row['site']
            step, count = int(row['step']), int(row['count'])
            witnessed = int(row['witness'])
            if case_id not in cases or site not in summary or step < 0 or count < 0 or witnessed not in (0, 1):
                raise CellError('invalid n-best witness range')
            if step in seen[case_id][site]:
                raise CellError(f'{case_id}: duplicate {site} range at step {step}')
            seen[case_id][site].add(step)
            if site == 'S3':
                if count > 3:
                    raise CellError('pre-sort tails exceeds nbest=3')
            else:
                pairs = int(row['pairs'])
                if not 0 <= pairs <= count * (count - 1) // 2 or witnessed != (pairs > 0):
                    raise CellError('inconsistent violating-pair count')
                summary[site]['pairs'] += pairs
            if witnessed:
                labels = 'abc' if site == 'S3' else 'ab'
                positions = []
                for label in labels:
                    position = int(row[label + '_position'])
                    positions.append(position)
                    if not 0 <= position < count or int(row[label + '_length']) < 0 or int(row[label + '_slot']) not in (0, 1):
                        raise CellError('invalid witness candidate identity')
                    for token in ('prev', 'token'):
                        if not re.fullmatch(r'0x[0-9a-f]{8}', row[label + '_' + token]):
                            raise CellError('invalid witness token')
                    if not re.fullmatch(r'-?0x[0-9a-f]+\.[0-9a-f]*p[+-][0-9]+|-?0x[0-9a-f]+p[+-][0-9]+', row[label + '_poss']):
                        raise CellError('witness must contain finite hexadecimal floats')
                if len(set(positions)) != len(positions):
                    raise CellError('witness reuses a candidate')
                if site == 'S3':
                    if any(row['compare_' + pair] != '0' for pair in ('ab', 'ba', 'bc', 'cb')) or \
                            int(row['compare_ac']) * int(row['compare_ca']) >= 0:
                        raise CellError('invalid pin triple predicate results')
                elif row['less_ab'] != '1' or row['less_ba'] != '1':
                    raise CellError('invalid pin pair predicate results')
                if summary[site]['first'] is None:
                    summary[site]['first'] = line
            summary[site]['calls'] += 1
            summary[site]['witnessed_calls'] += witnessed
        for case_id, sites in seen.items():
            length = len(cases[case_id]['input'])
            if sites != {'S1': set(range(length)), 'S2': {length}, 'S3': {length}}:
                raise CellError(f'{case_id}: missing or unexpected diagnostic ranges')
        return summary
    except (KeyError, ValueError, UnicodeError) as error:
        raise CellError(f'malformed n-best witness output: {error}') from error


def nbest_witness(ctx: Context, args: Sequence[str]) -> int:
    """Run one case file on the diagnostic BDB pin, without building a subject."""
    parser = argparse.ArgumentParser(prog='nbest-train-diff')
    parser.add_argument('--pin-witness-cases', required=True)
    parser.add_argument('--prefix', required=True)
    parser.add_argument('--data', required=True)
    options = parser.parse_args(args)
    if ctx.settings.cell != 'bdb':
        raise CellError('n-best precondition witnesses require the BDB cell')
    case_file = ctx.path(options.pin_witness_cases, caller=True).resolve()
    cases = nbest_witness_cases(case_file)
    prefix = ctx.path(options.prefix, caller=True).resolve()
    data = ctx.path(options.data, caller=True).resolve()
    digest = patch_digest(ctx.settings.root / 'tools/bisection/patches/nbest-tails-dump')
    identity = (prefix / 'oracle-pin.txt').read_text().splitlines()
    if ctx.settings.expected_pin + '+patches-' + digest not in identity or \
            'patches_manifest_sha256=' + digest not in identity or 'dbm=BerkeleyDB' not in identity:
        raise CellError('diagnostic pin has the wrong revision, backend or patch identity')
    if not (data / 'bigram.db').is_file() or 'database format:BerkeleyDB' not in (data / 'table.conf').read_text().splitlines():
        raise CellError('n-best witness data must be the reference BDB system data')
    pin = prefix / 'lib/libpinyin.so'
    work = ctx.scratch()
    driver = ctx.compile('nbest-train-diff', work / 'driver')
    result = ctx.run(driver, pin, [pin, data, '--cases', case_file], side='pin', cwd=work,
                     loader=True)
    ctx.write(result.stdout)
    ctx.write(result.stderr, error=True)
    if result.returncode:
        raise CellError(f'n-best witness driver failed with status {status(result.returncode)}')
    summary = nbest_witness_results(cases, result.stdout, result.stderr)
    ctx.emit(f'n-best witnesses: {len(cases)}/{len(cases)} cases completed (bdb)')
    for site, row in summary.items():
        if row['first'] is None:
            ctx.emit(f'{site}: none found in the case set ({row["calls"]} ranges)')
        else:
            ctx.emit(f'{site}: witness found in {row["witnessed_calls"]}/{row["calls"]} ranges' +
                     (f'; {row["pairs"]} violating pairs' if site != 'S3' else ''))
            ctx.emit(row['first'])
    return 0


def ordinary(ctx: Context, args: Sequence[str]) -> int:
    name = ctx.name
    if name == 'nbest-train-diff' and '--pin-witness-cases' in args:
        return nbest_witness(ctx, args)
    surface_args = []
    if name == 'nbest-train-diff':
        parser = argparse.ArgumentParser(prog=name)
        parser.add_argument('--surface-case')
        parser.add_argument('--case-file', default=str(ctx.here / 'nbest-precondition-cases.tsv'))
        options = parser.parse_args(args)
        if options.surface_case is not None:
            cases = nbest_witness_cases(ctx.path(options.case_file, caller=True))
            if options.surface_case not in cases:
                raise CellError(f'unknown n-best surface case: {options.surface_case}')
            row = cases[options.surface_case]
            surface_args = ['--surface-case', options.surface_case,
                            *(row[key] for key in ('parser', 'scheme', 'options', 'input'))]
    driver, subject = build_subject(ctx, heading='abi-probe' if name == 'abi-probe-diff' else None)
    skip = 0 if name == 'live-typing-diff' else 77
    pin, pin_data = ctx.pin(missing=skip, hint=True, require_data=True)
    environment = {}
    patterns = ()
    if name == 'abi-probe-diff':
        data = ctx.data.resolve('ABI_PROBE_SYSTEM', 'abi-probe')
        driver_args = [ctx.env.get('ABI_PROBE_SORT') or '1e']
        capi_heading = '--- capi side ---'
    elif name == 'live-typing-diff':
        data = ctx.data.resolve('LIVETYPING_SYSTEM', name)
        if not (data / 'interpolation2.text').is_file():
            ctx.emit(f'SKIP: {data} lacks interpolation2.text\n'
                 '  (the real-unigram source; copy it from the model cache beside\n'
                 '  the tables, e.g. target/model20/extracted/interpolation2.text)')
            return 0
        rounds = ctx.env.get('LIVETYPING_ROUNDS') or '3'
        environment['LIVETYPING_ROUNDS'] = rounds
        driver_args = []
        capi_heading = f'--- capi side (rounds={rounds}) ---'
        patterns = ('^live:cursor=', '^round:1 cursor=', r'^probe:after-choose n=[1-9]', '^cr:cursor=', '^cr:clear0=1')
    elif name == 'nbest-train-diff':
        value = ctx.env.get('NBEST_CAPI_SYSTEM') or ctx.env.get('OXPINYIN_SYSTEM_DIR') or ''
        data = ctx.path(value)
        if not value or not (data / 'interpolation2.text').is_file() or not ctx.data.detect(data):
            ctx.emit('SKIP: NBEST_CAPI_SYSTEM must name a real-unigram system dir\n'
                 '  (pinyin_index, phrase_index and bigram in the extension of the capi\'s\n'
                 '   compiled backend -- .db by default -- plus interpolation2.text)')
            return 77
        user, augmented = ctx.env.get('NBESTTRAINDIFF_USER_ROUNDS') or '3', ctx.env.get('NBESTTRAINDIFF_ROUNDS') or '3'
        environment.update(NBESTTRAINDIFF_USER_ROUNDS=user, NBESTTRAINDIFF_ROUNDS=augmented)
        driver_args = surface_args
        capi_heading = f'--- capi side (user={user} augmented={augmented}) ---'
    else:
        value = ctx.env.get('UNCOVERED_SYSTEM') or ctx.env.get('OXPINYIN_SYSTEM_DIR') or ''
        data = ctx.path(value)
        extension = ctx.data.detect(data) if value else None
        if not extension or not (data / 'interpolation2.text').is_file() or not (data / ('punct.' + extension)).is_file():
            ctx.emit('SKIP: UNCOVERED_SYSTEM must name a real-unigram system dir\n'
                 '  (pinyin_index, phrase_index, bigram and punct all in the extension of\n'
                 '   the capi\'s compiled backend -- .db by default -- plus interpolation2.text;\n'
                 '   punct is the Option A export of the same model20 punct.table)')
            return 77
        driver_args = []
        capi_heading = '--- capi side ---'
        patterns = ('^page-r0-shi:page=0', '^punct-hao:predict=', '^opt:0x60-ni3hao3@0:parsed=',
                    '^cur:0 aux=', '^raw:nihao@3:guess=1', '^raw:nihao@3:n=0', '^raw:nihao\\+sent@3:guess=1',
                    '^raw:nihao\\+sent@3:n=1', '^raw:nihaoshijie@3:guess=1', '^raw:nihaoshijie@3:n=0',
                    '^raw:nihaoshijie\\+sent@3:guess=1', '^raw:nihaoshijie\\+sent@3:n=3')
    logs = {}
    for side, so, directory in [('subject', subject, data), ('pin', pin, pin_data)]:
        ctx.emit(capi_heading if side == 'subject' else '--- oracle side ---')
        if name == 'abi-probe-diff' and side == 'subject':
            ctx.emit('sort word: 0x' + driver_args[0])
        result = ctx.run(driver, so, [so, directory, *driver_args], side=side, env=environment,
                         quiet=name == 'nbest-train-diff' and not surface_args)
        if result.returncode:
            ctx.emit(f'FAIL: {name} crashed against ' + ('oxpinyin-capi' if side == 'subject' else 'the oracle' if name == 'abi-probe-diff' else 'oracle'))
            ctx.write(result.stdout)
            if name != 'nbest-train-diff' or surface_args:
                ctx.emit('--- driver diagnostics (stderr) ---')
                ctx.write(result.stderr)
            return 1
        logs[side] = result.stdout
        label = 'oxpinyin-capi' if side == 'subject' else 'oracle'
        ctx.emit(label + ': ok' + (f' ({result.stdout.count(bytes([10]))} log lines)' if name == 'abi-probe-diff' else ''))
    if name == 'abi-probe-diff':
        ctx.emit('--- differential (whole-surface rows for all 79 exported symbols) ---')
        success, failure = 'abi-probe-diff: IDENTICAL', 'DIVERGENCE'
    elif name == 'live-typing-diff':
        ctx.emit('--- prerequisite: the post-choose surface is active on both sides ---')
        if not all(active_rows(log, patterns) for log in logs.values()):
            ctx.emit('FAIL: live-typing surface inactive (cursor / core round / after-choose window)\n'
                 '  A missing cursor or an empty after-choose candidate window means the run\n'
                 '  compared only the pre-choose surface and never exercised the\n'
                 '  post-choose decode or the decoded-continuation train.')
            return 1
        ctx.emit('post-choose surface active on both sides\n--- differential (full log: live-typing surface + train + export) ---')
        success, failure = 'live-typing-diff: IDENTICAL', 'DIVERGENCE (live-typing surface or train/export)'
    elif name == 'nbest-train-diff':
        ctx.emit('--- prerequisite: the NBEST row path is active on both sides ---')
        counts = {side: sum(b'cand[0]=NBEST' in line for line in log.splitlines()) for side, log in logs.items()}
        if not all(counts.values()):
            ctx.emit(f'FAIL: NBEST row path inactive (capi rows: {counts["subject"]}, oracle rows: {counts["pin"]})\n'
                 '  Zero NBEST rows means every training round chose a NORMAL candidate;\n'
                 '  the differential then proves nothing about the shifted-row selection\n'
                 '  record (docs/findings/sentence-surface.md §8).')
            return 1
        ctx.emit(f'NBEST rows active: capi={counts["subject"]} oracle={counts["pin"]}\n'
             '--- differential (full log: probe surface + user-store export) ---')
        labels = dict.fromkeys(re.findall(rb'^probe:([^ ]+) guess=', logs['pin'], re.M))
        for label in labels:
            prefix = b'probe:' + label + b' '
            rows = {side: [line for line in log.splitlines() if line.startswith(prefix)]
                    for side, log in logs.items()}
            verdict = 'IDENTICAL' if rows['pin'] == rows['subject'] else 'DIVERGENCE'
            ctx.emit(f'case:{label.decode()} {verdict}')
        success, failure = 'nbest-train-diff: IDENTICAL', 'DIVERGENCE (probe surface or user-store export)'
    else:
        ctx.emit("--- prerequisite: every phase's surface is active on both sides ---")
        if not all(active_rows(log, patterns) for log in logs.values()):
            ctx.emit('FAIL: a phase surface is inactive (paging / punct / profiles / cursor / raw offsets)\n'
                 '  A missing page walk, punct prediction, profile probe, cursor\n'
                 '  table, or raw-offset probe means that phase compared nothing.')
            return 1
        ctx.emit('all five phase surfaces active on both sides\n'
             '--- differential (full log: paging + punct + profiles + cursor + raw offsets) ---')
        success, failure = 'uncovered-surface-diff: IDENTICAL', 'DIVERGENCE (paging / punct / option profiles / cursor surfaces)'
    rc, diff = ctx.compare(logs['pin'], logs['subject'])
    if rc > 1:
        ctx.emit(f'FAIL: could not compare the differential logs (diff status {rc})', error=True)
        return 1
    ctx.emit(success if rc == 0 else failure)
    if rc:
        ctx.write(diff)
    elif surface_args:
        ctx.write(logs['subject'])
    elif name in ('live-typing-diff', 'nbest-train-diff') and not surface_args:
        rows = b''.join(line for line in logs['subject'].splitlines(keepends=True) if re.match(rb'^(phrase|bigram):', line))
        ctx.write(rows)
        if not rows:
            return 1
    return 0 if rc == 0 else 2


def training(ctx: Context, args: Sequence[str]) -> int:
    driver, subject = build_subject(ctx)
    pin, pin_data = ctx.pin(hint=True, require_data=True)
    ctx.emit(f'oracle: {pin}\ndata:   {pin_data}\n')
    data = ctx.data.resolve('TRAINDIFF_SYSTEM', 'train-diff')
    ctx.emit(f'capi system dir: {data}\n')
    verdict = 0
    def one(ctx: Context, item) -> int:
        (phase, value) = item
        verdict = 0
        ctx.emit(f'--- rounds={value} ---' if phase == 'rounds' else f'--- mask={value} (rounds=8) ---')
        env = {'TRAINDIFF_ROUNDS': value if phase == 'rounds' else '8'}
        if phase == 'mask':
            env['TRAINDIFF_MASK'] = value
        pin_log, subject_log = ctx.pair(driver, pin, subject, pin_data, data, env=env, quiet=True,
            failure=(f'FAIL: train-diff crashed against oxpinyin-capi ({phase}={value})',
                     f'FAIL: train-diff crashed against the oracle ({phase}={value})'))
        # LC_ALL follows the caller, exactly as the original sort commands did.
        work = ctx.scratch()
        sorted_logs = []
        for side, log in [('pin', pin_log), ('subject', subject_log)]:
            path = work / side
            path.write_bytes(log)
            sorted_logs.append(ctx.checked(['sort', path], capture=True).stdout)
        rc, diff = ctx.compare(*sorted_logs)
        if rc:
            ctx.emit(f'DIVERGENCE at {phase}={value}: exported triples differ')
            ctx.write(diff)
            verdict = 2
        else:
            ctx.emit(f'{phase}={value}: IDENTICAL')
            rows = b''.join(line for line in sorted_logs[1].splitlines(keepends=True) if re.match(rb'^(bigram|phrase):', line))
            if rows:
                ctx.write(rows)
            elif phase == 'mask':
                ctx.emit('(empty export)')
            else:
                raise Finished(1)
        ctx.emit()
        return verdict

    results = ctx.campaign([('rounds', str(i)) for i in range(1, 9)] + [('mask', 'user'), ('mask', 'all')], one)
    verdict = 1 if 1 in results else 2 if 2 in results else 0
    if not verdict:
        ctx.emit('train-diff: PASS — all round counts and mask phases identical')
    return verdict


RECIPES.update({name: ordinary for name in ('abi-probe-diff', 'live-typing-diff', 'nbest-train-diff', 'uncovered-surface-diff')})
RECIPES['train-diff'] = training

from .runner_state import RECIPES as STATE_RECIPES
RECIPES.update(STATE_RECIPES)


def candidate_blocks(log: bytes) -> dict[str, list[bytes]]:
    blocks, name = {}, None
    for raw in log.splitlines(keepends=True):
        if raw.startswith(b'== '):
            name = raw[3:].decode().rstrip('\n')
            blocks[name] = []
        elif name is not None:
            blocks[name].append(raw)
    return blocks


def candidate_assembly(ctx: Context, args: Sequence[str]) -> int:
    if len(args) < 3:
        raise CellError('expected oracle prefix and two subject libraries')
    prefix, pinyin, zhuyin = [ctx.path(value, caller=True).absolute() for value in args[:3]]
    if not (prefix / 'oracle-pin.txt').is_file() or not (prefix / 'lib/libpinyin.so').is_file():
        ctx.emit(f'SKIP: missing oracle: {prefix}', error=True)
        return 77
    for file in (prefix / 'oracle-pin.txt', prefix / 'lib/libpinyin.so', prefix / 'lib/libzhuyin.so',
                 prefix / 'lib/libpinyin/data/bigram.db', pinyin, zhuyin):
        if not file.exists():
            ctx.emit(f'FAIL: missing input {file}', error=True)
            return 1
    if not any(line.startswith(ctx.settings.expected_pin.split('+model20-')[0]) for line in (prefix / 'oracle-pin.txt').read_text().splitlines()):
        ctx.emit(f'FAIL: {prefix} is not a build of the pin', error=True)
        return 1
    work = ctx.scratch()
    driver = ctx.compile('candidate-assembly-diff', work / 'candidate-assembly-diff')
    out = ctx.path(args[3], caller=True).absolute() if len(args) > 3 and args[3] else ctx.scratch()
    out.mkdir(parents=True, exist_ok=True)
    if not (len(args) > 3 and args[3]):
        ctx.retained.add(out)
    observations = {}
    for mode, subject in [('pinyin', pinyin), ('zhuyin', zhuyin)]:
        for side, so in [('pin', prefix / f'lib/lib{mode}.so'), ('ox', subject)]:
            cwd = work / f'{mode}-{side}'
            cwd.mkdir()
            result = ctx.run(driver, so, [mode, so, prefix / 'lib/libpinyin/data'], side=side, cwd=cwd)
            (out / f'{mode}-{side}.log').write_bytes(result.stdout)
            (out / f'{mode}-{side}.err').write_bytes(result.stderr)
            if result.returncode:
                ctx.emit(f'FAIL: {mode} driver crashed against {side} ({so})', error=True)
                ctx.write(b''.join(result.stderr.splitlines(keepends=True)[-5:]), error=True)
                return 1
            observations[mode, side] = candidate_blocks(result.stdout)
    verdict = 0
    for mode in ('pinyin', 'zhuyin'):
        pin, subject = observations[mode, 'pin'], observations[mode, 'ox']
        if list(pin) != list(subject):
            ctx.emit(f'{mode}: FAIL — the two logs walk different cases')
            verdict = 2
            continue
        identical = 0
        for name in pin:
            if pin[name] == subject[name]:
                identical += 1
            else:
                ctx.emit(f'{mode}: DIVERGENT {name}')
                verdict = 2
        ctx.emit(f'{mode}: {len(pin)} cases, {identical} identical, 0 declared divergent')
    ctx.emit('RESULT: as declared' if verdict == 0 else f'RESULT: FAIL (logs: {out})')
    if verdict == 0:
        ctx.retained.discard(out)
    return verdict


def nbest_choose(ctx: Context, args: Sequence[str]) -> int:
    if len(args) != 5:
        raise CellError('expected backend, prefix, two subject libraries and new output directory')
    cell, value, pinyin, zhuyin, output = args
    settings = Settings.read(dict(ctx.env, PINYIN_ORACLE_DBM=cell))
    ctx = replace(ctx, settings=settings, services=replace(ctx.services, settings=settings))
    prefix, pinyin, zhuyin, out = [ctx.path(value, caller=True) for value in (value, pinyin, zhuyin, output)]
    if not (prefix / 'oracle-pin.txt').is_file() or not (prefix / 'lib/libpinyin.so').is_file():
        ctx.emit(f'SKIP: missing oracle: {value}', error=True)
        return 77
    for file in (prefix / 'oracle-pin.txt', prefix / 'lib/libpinyin.so', prefix / 'lib/libzhuyin.so',
                 prefix / 'lib/libpinyin/data/bigram.db', pinyin, zhuyin):
        if not file.is_file():
            ctx.emit(f'missing input: {file}', error=True)
            return 1
    if not any(line.startswith(settings.expected_pin.split('+model20-')[0]) for line in (prefix / 'oracle-pin.txt').read_text().splitlines()):
        return 1
    out.mkdir()
    driver = ctx.compile('candidate-assembly-diff', out / 'driver', absolute_source=True)
    verdict = 0
    for mode, subject in [('pinyin', pinyin), ('zhuyin', zhuyin)]:
        logs, paths = [], []
        for side, so in [('pin', prefix / f'lib/lib{mode}.so'), ('ox', subject)]:
            dest = out / f'{mode}-{side}'
            dest.mkdir()
            result = ctx.run(driver, so, [mode, so, prefix / 'lib/libpinyin/data'], side=side,
                             env={'TMPDIR': str(dest), 'CAND_ASSEMBLY_TRAIN_DIR': str(dest)})
            log = dest / 'driver.log'
            log.write_bytes(result.stdout)
            (dest / 'driver.err').write_bytes(result.stderr)
            if result.returncode:
                return status(result.returncode)
            paths.append(log)
            logs.append(result.stdout)
        rc, diff = ctx.compare(*logs, paths=tuple(paths))
        ctx.write(diff)
        if rc:
            verdict = 2
        rc = python_script(ctx, 'compare-nbest-training-state.py', [cell, str(out / f'{mode}-pin'), str(out / f'{mode}-ox'),
                           '--pin-so', str(prefix / 'lib/libpinyin.so'), '--ox-so', str(pinyin),
                           '--data', str(prefix / 'lib/libpinyin/data'), '--facade', mode])
        if rc:
            verdict = rc
    return verdict


def system_round_trip(ctx: Context, args: Sequence[str]) -> int:
    if len(args) != 4:
        raise CellError('expected facade, pin-so, ox-so and system-data-dir')
    facade, *values = args
    pin, subject, data = [ctx.path(value, caller=True).resolve() for value in values]
    if facade not in ('pinyin', 'zhuyin'):
        return 2
    work = ctx.scratch()
    driver = ctx.compile('import-semantics-diff' if facade == 'pinyin' else 'zhuyin-import-diff', work / 'driver',
                         compiler='cc', glib=facade == 'zhuyin', absolute_source=True)

    def run(profile: Path, so: Path, library: int, mode: str | None = None) -> bytes:
        result = ctx.run(driver, so, [so, data, str(library), *([mode] if mode else [])],
                         side='pin' if so == pin else 'subject', cwd=profile, loader=True)
        ctx.write(result.stderr, error=True)
        if result.returncode:
            raise Finished(status(result.returncode))
        return result.stdout

    names = ('gb_char', 'gbk_char', 'opengram', 'merged')
    for library in range(1, 5):
        pin_user, subject_user = work / f'pin-{library}', work / f'ox-{library}'
        pin_user.mkdir()
        subject_user.mkdir()
        pin_log, subject_log = run(pin_user, pin, library), run(subject_user, subject, library)
        rc, diff = ctx.compare(pin_log, subject_log)
        ctx.write(diff)
        if rc:
            return rc
        for name in names:
            ctx.checked(['cmp', pin_user / f'user/{name}.dbin', subject_user / f'user/{name}.dbin'])
        expected = b''.join(line for line in pin_log.splitlines(keepends=True) if line.startswith(b'reopened '))
        before = {name: (pin_user / f'user/{name}.dbin').read_bytes() for name in names}
        rewritten = run(pin_user, subject, library, 'rewrite')
        for name in names:
            if (pin_user / f'user/{name}.dbin').read_bytes() != before[name]:
                return 1
        for observed in (rewritten, run(pin_user, pin, library, 'read'), run(subject_user, pin, library, 'read')):
            rc, diff = ctx.compare(expected, observed)
            ctx.write(diff)
            if rc:
                return rc
        ctx.emit(f'{facade} library {library}: IDENTICAL pin->ox, pin->ox->pin, ox->pin')
    return 0


RECIPES.update({'candidate-assembly-diff': candidate_assembly, 'nbest-choose-train-diff': nbest_choose,
                'system-import-round-trip': system_round_trip})


def dynamic_adjust(ctx: Context, args: Sequence[str]) -> int:
    ctx.emit('--- building dynamic-adjust-diff driver ---')
    work = ctx.scratch()
    driver = ctx.compile('dynamic-adjust-diff', work / 'dynamic-adjust-diff', flags=('-Wall', '-Wextra', '-Werror', '-O2'))
    ctx.emit('--- building oxpinyin-capi ---')
    subject = ctx.artifact()
    pin, pin_data = ctx.pin(hint=True, verify=False)
    data = ctx.data.resolve('DYNAMIC_ADJUST_SYSTEM', 'dynamic-adjust')
    ctx.emit(f'capi system dir: {data}\n--- driving oxpinyin (bit on / off) ---')
    logs = {}
    for side, so, directory in [('ox', subject, data), ('pin', pin, pin_data)]:
        if side == 'pin':
            ctx.emit('--- driving the pin (bit on / off) ---')
        for mode in ('on', 'off'):
            user = work / 'user'
            shutil.rmtree(user, ignore_errors=True)
            user.mkdir()
            result = ctx.run(driver, so, [so, directory, mode, user], side='pin' if side == 'pin' else 'subject')
            ctx.write(result.stderr, error=True)
            if result.returncode:
                return status(result.returncode)
            logs[side + '-' + mode] = result.stdout
    choices = {key: b''.join(line for line in value.splitlines(keepends=True) if line.startswith(b'CHOICE|')) for key, value in logs.items()}
    for key in ('pin-on', 'pin-off', 'ox-on', 'ox-off'):
        if choices[key].count(bytes([10])) != 7:
            ctx.emit(f'FAIL: {key} did not report all seven fixed choices')
            return 1
        if choices[key] != choices['pin-on']:
            ctx.emit(f'FAIL: chosen identity or advanced cursor differs for {key}')
            ctx.write(ctx.compare(choices['pin-on'], choices[key])[1])
            return 1
    ctx.emit('choices: identical identities and advanced cursors in all four runs')
    for side in ('ox', 'pin'):
        if logs[side + '-on'] == logs[side + '-off']:
            ctx.emit(f'VACUOUS: {side} produced identical output with the bit set and clear.\n'
                 '  The probe is not reaching DYNAMIC_ADJUST; a passing comparison\n'
                 '  would prove nothing. Fix the probe before trusting this gate.')
            return 3
    ctx.emit("non-vacuity: both engines' output changes with the bit — probe is live")
    verdict = 0
    for mode in ('on', 'off'):
        rc, diff = ctx.compare(logs['pin-' + mode], logs['ox-' + mode])
        ctx.emit(f'DYNAMIC_ADJUST={mode}: ' + ('IDENTICAL' if not rc else 'DIVERGENT'))
        if rc:
            ctx.write(b''.join(diff.splitlines(keepends=True)[:40]))
            verdict = 2
    return verdict


def scheme(ctx: Context, args: Sequence[str]) -> int:
    name, number = (args[0] if args else 'double'), (args[1] if len(args) > 1 else '')
    ctx.emit('--- building scheme-diff driver ---')
    driver = ctx.compile('scheme-diff')
    if name == 'full':
        if number and number not in ('1', '2', '3'):
            ctx.emit(f"fatal: full scheme must be 1, 2 or 3 (got '{number}')")
            return 1
        driver = ctx.compile('fullpin-diff')
    elif name == 'bopomofo':
        driver = ctx.compile('chewing-diff')
    ctx.emit('build: ok\n--- building oxpinyin-capi ---')
    subject = ctx.artifact()
    pin, pin_data = ctx.pin(hint=True, exact=False)
    if name == 'full' and number in ('2', '3'):
        prefix = ctx.path(ctx.env.get('PINYIN_PATCHED_ORACLE_PREFIX') or str(Path.home() / '.local/opt/pinyin-oracle-patched'))
        if not (prefix / 'oracle-pin.txt').is_file() or not (prefix / 'lib/libpinyin.so').is_file():
            ctx.emit(f'SKIP: patched oracle not found at {prefix} (needed for full scheme {number})\n'
                 '  build it with:\n    tools/oracle/build-oracle.sh --prefix "$HOME/.local/opt/pinyin-oracle-patched" \\\n'
                 '        --apply-patches tools/oracle/patches')
            return 77
        digest = patch_digest(ctx.settings.root / 'tools/oracle/patches')
        if ctx.settings.expected_pin + '+patches-' + digest not in (prefix / 'oracle-pin.txt').read_text().splitlines():
            ctx.emit(f'SKIP: patched oracle at {prefix} is off-pin or carries a different patch set\n'
                 f'  expected +patches-{digest} (from tools/oracle/patches)')
            return 77
        pin, pin_data = prefix / 'lib/libpinyin.so', prefix / 'lib/libpinyin/data'
    if name == 'bopomofo' and number and number not in ('1', '2', '3', '4', '5', '6', '8', '9'):
        ctx.emit(f"fatal: bopomofo scheme must be one of 1 2 3 4 5 6 8 9 (got '{number}')")
        return 1
    extra = [number or '1'] if name in ('full', 'bopomofo') else ([number] if number else [])
    user, system = ctx.scratch(), ctx.scratch()
    resolved = ctx.data.resolve('W13_CAPI_SYSTEM', 'scheme-diff')
    ctx.data.copy(resolved, system)
    interpolation = ctx.env.get('W13_INTERPOLATION') or str(resolved / 'interpolation2.text')
    if ctx.path(interpolation).is_file():
        shutil.copyfile(ctx.path(interpolation), system / 'interpolation2.text')
    logs = {}
    for side, so, data in [('subject', subject, system), ('pin', pin, pin_data)]:
        ctx.emit('--- capi side ---' if side == 'subject' else '--- oracle side ---')
        result = ctx.run(driver, so, [so, data, *extra], side=side, env={'SCHEME_DIFF_USER_DIR': str(user)})
        if result.returncode:
            ctx.emit('FAIL: scheme-diff crashed against ' + ('oxpinyin-capi' if side == 'subject' else 'oracle'))
            ctx.write(result.stdout)
            ctx.write(result.stderr)
            return 1
        logs[side] = result.stdout
        ctx.emit('oxpinyin-capi: ok' if side == 'subject' else 'oracle: ok')
    ctx.emit('--- differential ---')
    filtered = ctx.env.get('SCHEME_DIFF_PARSE_AUX_ONLY') == '1'
    if filtered:
        logs = {side: b''.join(line for line in log.splitlines(keepends=True) if not re.search(rb'candidate|sentence', line)) for side, log in logs.items()}
    rc, diff = ctx.compare(logs['pin'], logs['subject'])
    ctx.emit(('PARSE_AUX_IDENTICAL' if filtered else 'IDENTICAL') if not rc else
         ('DIVERGENCE (parse/aux surface)' if filtered else 'DIVERGENCE'))
    if rc:
        ctx.write(diff)
    return 2 if rc else 0


def predicted_order(ctx: Context, args: Sequence[str]) -> int:
    driver, subject = build_subject(ctx, print_capi=False)
    pin, pin_data = ctx.pin()
    value = ctx.env.get('UNCOVERED_SYSTEM') or ctx.env.get('OXPINYIN_SYSTEM_DIR') or ''
    data = ctx.path(value)
    if not value or not ctx.data.detect(data):
        ctx.emit('SKIP: UNCOVERED_SYSTEM must name the five-file system dir\n  (see run-uncovered-surface-diff.sh)')
        return 77
    pin_log, subject_log = ctx.pair(driver, pin, subject, pin_data, data,
        failure=('FAIL: pred-order-diff crashed against oxpinyin-capi', 'FAIL: pred-order-diff crashed against oracle'))
    ctx.emit('--- per-prefix position mismatches (the moving number) ---')
    total, rows, failed = 0, 0, False
    tags = ('hao', 'de', 'yi', 'ni', 'zhongguo', 'wo', 'shi', 'le')
    from itertools import zip_longest
    for tag in (*tags, *('plain-' + tag for tag in tags)):
        prefix = ('pred-' + tag + ':').encode()
        pin_rows = [line for line in pin_log.splitlines() if line.startswith(prefix)]
        subject_rows = [line for line in subject_log.splitlines() if line.startswith(prefix)]
        if not pin_rows:
            ctx.emit(f'{tag}: no oracle rows (prefix inactive?)')
            failed = True
            continue
        mismatch = sum(left != right for left, right in zip_longest(pin_rows, subject_rows, fillvalue=b''))
        ctx.emit(f'{tag}: {mismatch}/{len(pin_rows)} rows at different positions')
        total += mismatch
        rows += len(pin_rows)
    if failed:
        ctx.emit('FAIL: a prefix produced no oracle rows')
        return 1
    if total == 0:
        ctx.emit(f'pred-order-diff: IDENTICAL ({rows} rows)')
        return 0
    ctx.emit(f'DIVERGENCE: {total}/{rows} rows at different positions\n'
         '  Raw metric (text shape + order). Pre-fix baseline 2026-08-25: ~all\n'
         '  rows differ; after the prefix-slice fix the residual is order-only\n'
         '  (measured hao 174/178) — the recorded Tkrzw bucket-walk divergence.\n'
         '  See docs/findings/uncovered-surface-differentials.md.')
    return 2


RECIPES.update({'dynamic-adjust-diff': dynamic_adjust, 'scheme-diff': scheme, 'pred-order-diff': predicted_order})

from .runner_protocols import RECIPES as PROTOCOL_RECIPES
RECIPES.update(PROTOCOL_RECIPES)


def residue(ctx: Context, args: Sequence[str]) -> int:
    tail = ctx.name == 'residue-a-tail-diff'
    label, tag = ('residue-a-tail', 'RESIDUE_A') if tail else ('residue-mechanism', 'RESIDUE_MECH')
    ctx.emit(f'--- building {label} driver ---')
    driver = ctx.compile(ctx.name)
    ctx.emit('build: ok')
    settings = ctx.settings
    supplied = ctx.env.get('RESIDUE_A_CAPI_SO') or ctx.env.get('OXPINYIN_CAPI_SO') if tail else ctx.env.get('OXPINYIN_CAPI_SO')
    if tail and ctx.env.get('RESIDUE_A_CAPI_SO'):
        settings = Settings.read(dict(ctx.env, OXPINYIN_CAPI_SO=ctx.env['RESIDUE_A_CAPI_SO']))
    if not tail:
        ctx.emit('--- building oxpinyin-capi ---')
    subject = settings.artifact('OXPINYIN_CAPI_SO', 'libpinyin_capi.so', 'oxpinyin-capi')
    ctx.emit(f'capi: {subject}')
    if tail:
        prefix = ctx.path(ctx.env.get('PINYIN_ORACLE_PREFIX') or str(Path.home() / '.local/opt/pinyin-oracle'))
        manifest = prefix / 'oracle-patches.sha256'
        if not manifest.is_file() or 'pin-bigram-export-strjoinv.patch' not in manifest.read_text():
            ctx.emit(f'FAIL: {prefix} was not built with tools/bisection/patches/bigram-export-strjoinv')
            return 1
        pin, pin_data = prefix / 'lib/libpinyin.so', prefix / 'lib/libpinyin/data'
        if not (prefix / 'oracle-pin.txt').is_file() or not pin.is_file():
            ctx.emit(f'FAIL: patched pin-built oracle not found at {prefix}')
            return 1
        digest = patch_digest(ctx.settings.root / 'tools/bisection/patches/bigram-export-strjoinv')
        fields = (prefix / 'oracle-pin.txt').read_text().splitlines()
        expected = ctx.settings.expected_pin + '+patches-' + digest
        if expected not in fields or 'patches_manifest_sha256=' + digest not in fields:
            ctx.emit(f'FAIL: oracle prefix at {prefix} is off-pin, wrong-cell or carries a different patch set\n  expected {expected}')
            return 1
        if not (pin_data / 'bigram.db').is_file():
            ctx.emit(f'FAIL: oracle data not found at {pin_data}')
            return 1
    else:
        pin, pin_data = ctx.pin(require_data=True)
    if ctx.env.get(tag + '_SAME_DIR') == '1':
        data = pin_data
        ctx.emit(f'same-dir mode: both sides on {data}')
    else:
        data = ctx.data.resolve(tag + '_SYSTEM', label)
    out = ctx.output(tag + '_OUT')
    logs, paths = [], []
    for side, so, directory, filename in [('subject', subject, data, 'capi'), ('pin', pin, pin_data, 'oracle')]:
        ctx.emit('--- capi side ---' if side == 'subject' else '--- oracle side ---')
        result = ctx.run(driver, so, [so, directory], side=side)
        path = out / (('residue-a-' if tail else 'residue-mech-') + filename + '.log')
        path.write_bytes(result.stdout)
        (out / (filename + '.err')).write_bytes(result.stderr)
        if result.returncode:
            ctx.emit(f'FAIL: {ctx.name} crashed against ' + ('oxpinyin-capi' if side == 'subject' else 'the oracle'))
            ctx.write(result.stderr)
            return 1
        ctx.emit(('oxpinyin-capi' if side == 'subject' else 'oracle') + f': ok ({result.stdout.count(bytes([10]))} log lines)')
        logs.append(result.stdout)
        paths.append(path)
    if not ctx.env.get(tag + '_OUT'):
        ctx.retained.add(out)
    if tail and ctx.env.get('RESIDUE_A_DUMP_PREFIX'):
        prefix = ctx.path(ctx.env['RESIDUE_A_DUMP_PREFIX'])
        dump = prefix / 'lib/libpinyin.so'
        ctx.emit('--- oracle side, instrumented (tails to stderr) ---')
        if not dump.is_file():
            ctx.emit(f'FAIL: instrumented oracle not found at {dump}')
            return 1
        result = ctx.run(driver, dump, [dump, prefix / 'lib/libpinyin/data'], side='pin', env={'OXPINYIN_DUMP_TAILS': '1'})
        (out / 'residue-a-oracle-dump.log').write_bytes(result.stdout)
        (out / 'pin-tails.log').write_bytes(result.stderr)
        if result.returncode:
            ctx.emit('FAIL: residue-a-tail-diff crashed against the instrumented oracle')
            if ctx.settings.cell == 'tkrzw':
                ctx.write(result.stderr, error=True)
            return 1
        if result.stdout != logs[1]:
            ctx.emit("FAIL: the instrumented oracle's stdout differs from the pin's")
            return 1
        ctx.emit(f'instrumented oracle: stdout identical to the pin; tails in {out}/pin-tails.log')
    if tail and ctx.env.get('RESIDUE_A_PROBE') == '1':
        ctx.emit('--- runtime-side probe ---')
        probe = ctx.env.get('OXPINYIN_NBEST_TAIL_PROBE')
        if not probe:
            if supplied:
                ctx.emit('FAIL: RESIDUE_A_PROBE needs OXPINYIN_NBEST_TAIL_PROBE with a supplied library')
                return 1
            built = slot.run_command(['cargo', 'build', '--locked', '-p', 'oxpinyin-runtime',
                                      '--example', 'nbest_tail_probe', '--no-default-features', '--features',
                                      settings.feature, '--manifest-path', str(settings.root / 'Cargo.toml')],
                                     env=ctx.env, out=sys.stderr, cwd=settings.root)
            if built:
                raise Finished(status(built))
            probe = str(settings.target / 'debug/examples/nbest_tail_probe')
        result = ctx.command([probe, data, ctx.scratch()], capture=True)
        (out / 'ox-probe.log').write_bytes(result.stdout)
        (out / 'ox-probe.err').write_bytes(result.stderr)
        if result.returncode:
            ctx.emit('FAIL: nbest_tail_probe failed')
            ctx.write(result.stderr)
            return 1
        ctx.emit(f'probe: {out}/ox-probe.log')
    ctx.emit('--- diff ---')
    rc, diff = ctx.compare(logs[1], logs[0], paths=(paths[1], paths[0]))
    ctx.write(diff)
    if not rc:
        ctx.emit('IDENTICAL')
        ctx.retained.discard(out)
        return 0
    if tail:
        ctx.emit('DIVERGENT (expected while rows 33/34/36/37 — residues B, C, X2, E —\n'
             'remain open; residue A itself is closed: register #35, landed with\n'
             'fix/nbest-step-cost-user-token. An A, X or D line in the diff above\n'
             'is a regression of that fix, not an open-residue artifact.)')
    else:
        ctx.emit('DIVERGENT (expected while residues B/D remain open)')
    ctx.emit(f'logs: {paths[1]}  {paths[0]}')
    return 2


def union(ctx: Context, args: Sequence[str]) -> int:
    ctx.emit('--- building union-diff drivers ---')
    driver, edge = ctx.compile('union-diff'), ctx.compile('union-edge-diff')
    ctx.emit('--- building plant-oracle-bigram ---')
    flags = {'db': ['-DORACLE_BDB', '-ldb'], 'kct': ['-DORACLE_KC', '-lkyotocabinet'], 'tkt': ['-ltkrzw', '-lpthread']}[ctx.settings.extension]
    planter = ctx.here / 'plant-oracle-bigram'
    ctx.checked(['g++', '-std=c++17', '-Wall', '-Wextra', '-Werror', '-O2', 'plant-oracle-bigram.cc', *flags, '-o', planter])
    ctx.emit('build: ok\n--- building oxpinyin-capi ---')
    subject = ctx.artifact()
    pin, pin_data = ctx.pin()
    ctx.emit(f'oracle: {pin}')
    data = ctx.data.resolve('UNION_SYSTEM', 'union-diff')
    pin_log, subject_log = ctx.pair(driver, pin, subject, pin_data, data, quiet=True)
    ctx.emit('--- capi ---')
    ctx.write(subject_log)
    ctx.emit('--- oracle ---')
    ctx.write(pin_log)
    rc, diff = ctx.compare(pin_log, subject_log)
    if rc:
        ctx.emit('DIVERGENCE: union-diff logs differ')
        ctx.write(diff)
        return 2
    ctx.emit('union-diff: IDENTICAL\n--- filter-edge setup ---')
    users = {}
    for side, so, directory in [('subject', subject, data), ('pin', pin, pin_data)]:
        user = ctx.scratch()
        users[side] = user
        result = ctx.run(edge, so, [so, directory, 'setup', user], side=side)
        ctx.write(result.stdout)
        ctx.write(result.stderr, error=True)
        if result.returncode:
            ctx.emit('FAIL: ' + ('capi' if side == 'subject' else 'oracle') + ' edge setup')
            return 1
    for side in ('subject', 'pin'):
        result = ctx.command([planter, users[side], '0x07000001', '0x07000002', '9', '0x07000003', '10'])
        if result.returncode:
            ctx.emit('FAIL: ' + ('capi' if side == 'subject' else 'oracle') + ' edge plant')
            return 1
    logs = {}
    for side, so, directory in [('subject', subject, data), ('pin', pin, pin_data)]:
        result = ctx.run(edge, so, [so, directory, 'predict', users[side]], side=side, quiet=True)
        if result.returncode:
            ctx.emit('FAIL: union-edge-diff predict crashed against ' + ('oxpinyin-capi' if side == 'subject' else 'the oracle'))
            ctx.write(result.stdout)
            return 1
        logs[side] = result.stdout
    ctx.emit('--- capi edge ---')
    ctx.write(logs['subject'])
    ctx.emit('--- oracle edge ---')
    ctx.write(logs['pin'])
    rc, diff = ctx.compare(logs['pin'], logs['subject'])
    if rc:
        ctx.emit('DIVERGENCE: filter-edge logs differ')
        ctx.write(diff)
        return 2
    for line, failure in [(b'pred-edge-9: \xe7\x94\xb2\xe7\x94\xb2 absent', 'FAIL: count=9 successor was not filtered'),
                          (b'pred-edge-10: \xe4\xb9\x99\xe4\xb9\x99 type=4', 'FAIL: count=10 successor was not predicted')]:
        if line not in logs['subject'].splitlines():
            ctx.emit(failure)
            return 2
    ctx.emit('union-diff filter-edge: IDENTICAL')
    return 0


RECIPES.update({'residue-a-tail-diff': residue, 'residue-mechanism-diff': residue, 'union-diff': union})


def pinyin_needed(text: str) -> str:
    return '\n'.join(re.findall(r'Shared library: \[(libpinyin[^]]*)\]', text))


def pinyin_resolved(text: str) -> str:
    return '\n'.join(re.findall(r'libpinyin\.so\.15 => ([^ ]*)', text))


def cpp_smoke(ctx: Context, args: Sequence[str]) -> int:
    ctx.emit('--- building oxpinyin-capi for the C++ smoke gate ---')
    subject = ctx.artifact()
    work = ctx.scratch()
    user = work / 'user'
    user.mkdir()
    system = work / 'sys'
    ctx.services.mini_fixture(system)
    libraries = work / 'lib'
    libraries.mkdir()
    (libraries / 'libpinyin_capi.so').symlink_to(subject)
    (libraries / 'libpinyin.so.15').symlink_to('libpinyin_capi.so')
    ctx.emit('--- compiling C++ smoke TU against pinyin.h ---')
    import shlex
    flags = shlex.split(ctx.env['GLIB_CFLAGS']) if ctx.env.get('GLIB_CFLAGS') else ctx.flags('--cflags')
    driver = work / 'cpp-smoke'
    ctx.checked(['g++', '-std=c++17', '-Wall', '-Wextra', '-Werror', '-O2', *flags,
                 '-I' + str(ctx.settings.root / 'crates/oxpinyin-capi'), 'cpp-smoke.cc',
                 '-L' + str(libraries), '-Wl,-rpath,' + str(libraries), '-lpinyin_capi', '-o', driver])
    needed = pinyin_needed(ctx.checked(['readelf', '-d', driver], capture=True).stdout.decode())
    if needed != 'libpinyin.so.15':
        ctx.emit(f"fatal: smoke binary needs '{needed}', expected libpinyin.so.15")
        return 1
    resolved = pinyin_resolved(ctx.checked(['ldd', driver], environment=dict(ctx.env, LD_LIBRARY_PATH=''), capture=True).stdout.decode())
    if not resolved.startswith(str(libraries) + '/'):
        ctx.emit(f"fatal: libpinyin.so.15 resolved to '{resolved}', not the build under test")
        return 1
    ctx.emit('--- running C++ smoke TU ---')
    result = ctx.run(driver, driver, [system, user], side='subject')
    ctx.write(result.stdout)
    ctx.write(result.stderr, error=True)
    if result.returncode:
        return status(result.returncode)
    ctx.emit('cpp-smoke: ok')
    return 0


def bisect(ctx: Context, args: Sequence[str]) -> int:
    ctx.emit('--- building bisect harness ---')
    driver = ctx.compile('bisect')
    ctx.emit('build: ok\n--- building oxpinyin-capi ---')
    subject = ctx.artifact()
    ctx.emit(f'capi: {subject}')
    data = ctx.settings.root / 'fixtures/w3' / ctx.settings.extension
    if not data.is_dir():
        ctx.emit(f'fatal: no per-backend fixture directory under {ctx.settings.root}/fixtures/w3')
        return 1
    ctx.emit(f'data: {data}\n')
    rc = cpp_smoke(ctx, [])
    if rc:
        return rc
    ctx.emit('\n--- running against oxpinyin-capi ---')
    result = ctx.run(driver, subject, [subject, data], side='subject')
    if result.returncode:
        ctx.emit('FAIL: bisect crashed against oxpinyin-capi')
        ctx.write(result.stdout)
        ctx.emit('--- driver diagnostics (stderr) ---')
        ctx.write(result.stderr)
        return 1
    ctx.emit('oxpinyin-capi: ok')
    ctx.write(result.stdout)
    ctx.emit('\n--- valgrind ---')
    if shutil.which('valgrind', path=ctx.env.get('PATH')):
        result = ctx.run(Path('valgrind'), subject,
                         ['--error-exitcode=1', '--leak-check=full', '--errors-for-leak-kinds=definite', driver, subject, data], side='subject')
        if result.returncode:
            ctx.emit('FAIL: valgrind detected errors')
            ctx.write(result.stderr)
            return 3
        ctx.write(b''.join(line for line in result.stderr.splitlines(keepends=True) if b'ERROR SUMMARY:' in line))
        ctx.write(b''.join(line for line in result.stderr.splitlines(keepends=True) if b'definitely lost:' in line))
        ctx.emit('valgrind: ok\n')
    else:
        ctx.emit('SKIP: valgrind not found\n')
    ctx.emit('--- ld-preload integration ---')
    engines = [Path('/usr/libexec/ibus-engine-libpinyin'), Path('/usr/lib/ibus/ibus-engine-libpinyin')]
    engine = next((path for path in engines if path.is_file() and os.access(path, os.X_OK)), None)
    if ctx.env.get('BISECT_LD_PRELOAD') == '1' and engine is not None:
        ctx.emit(f'engine: {engine}')
        # The existing shared pool launcher owns cancellation/reaping. timeout
        # supplies the legacy two-second integration observation interval.
        result = ctx.run(Path('timeout'), subject, ['2', engine, '--ibus'], side='subject',
                         env={'LD_PRELOAD': str(subject), 'LD_DEBUG': 'bindings'})
        text = result.stderr.decode(errors='replace')
        bound = sum(bool(re.search('to ' + re.escape(str(subject)) + '.*pinyin_', line)) for line in text.splitlines())
        ctx.emit(f'symbols bound from capi: {bound}')
        if bound == 0:
            ctx.emit(f'FAIL: no pinyin_ symbols resolved from {subject}')
            ctx.write(result.stderr)
            return 4
        missing = [symbol for symbol in ('pinyin_init', 'pinyin_fini', 'pinyin_alloc_instance', 'pinyin_set_options')
                   if not re.search('to ' + re.escape(str(subject)) + '.*`' + symbol + "'", text)]
        for symbol in missing:
            ctx.emit(f'  MISSING: {symbol} not bound from capi')
        if missing:
            ctx.emit('FAIL: critical symbols not overridden')
            return 4
        ctx.emit(f'ld-preload: ok (engine loaded, {bound} symbols overridden)\n')
    else:
        ctx.emit('SKIP: not requested (set BISECT_LD_PRELOAD=1 to enable)' if ctx.env.get('BISECT_LD_PRELOAD') != '1' else 'SKIP: ibus-engine-libpinyin not found')
        ctx.emit()
    if len(args) > 1 and all(args[:2]):
        pin, pin_data = map(ctx.path, args[:2])
        ctx.emit('--- right-tail assertion contract (nihao, offset 5) ---')
        result = ctx.run(driver, pin, [pin, pin_data, '--right-tail-case'], side='pin')
        expected = b'right-tail input=nihao offset=5 flags=0x0000018a'
        if status(result.returncode) != 134 or expected not in result.stdout.splitlines() or not re.search(
                rb'pinyin.cpp:3092:.*pinyin_get_right_pinyin_offset.*Assertion.*_check_offset\(matrix, right\).*failed', result.stderr):
            ctx.emit(f'FAIL: unexpected oracle right-tail observation (exit {status(result.returncode)})')
            ctx.write(result.stdout + result.stderr)
            return 1
        result = ctx.services.run([str(driver), str(subject), str(pin_data), '--right-tail-case'], subject,
                                  runner=ctx.name, side='subject', cwd=ctx.here, merged=True)
        if result.returncode or expected not in result.stdout.splitlines() or b'right-tail: false; output untouched' not in result.stdout.splitlines():
            ctx.emit('FAIL: unexpected C API right-tail observation')
            ctx.write(result.stdout)
            return 1
        ctx.emit('oracle: expected SIGABRT, pinyin.cpp:3092 _check_offset(matrix, right)\n'
             'capi: expected false, output untouched (compatibility register row 14)')
    ctx.emit('\nbisection: PASS')
    return 0


RECIPES.update({'cpp-smoke': cpp_smoke, 'bisect': bisect})


def import_diff(ctx: Context, args: Sequence[str]) -> int:
    ctx.emit('--- building import-diff driver ---')
    driver = ctx.compile('import-diff')
    ctx.emit('build: ok\n--- building oxpinyin-capi ---')
    subject = ctx.artifact()
    ctx.emit(f'capi: {subject} ({ctx.settings.feature})')
    pin, pin_data = ctx.pin(hint=True, require_data=True)
    prefix = pin.parent.parent
    fields = (prefix / 'oracle-pin.txt').read_text().splitlines()
    model = ctx.settings.expected_pin.split('+model20-')[1].split('+dbm-')[0]
    if not any(line.startswith('model_sha256=' + model) for line in fields) or 'dbm=' + ctx.settings.dbm_name not in fields:
        ctx.emit(f'SKIP: oracle prefix at {prefix} is off-pin (model/dbm fields)\n'
             f'  expected model_sha256={model} dbm={ctx.settings.dbm_name}')
        return 77
    ctx.emit(f'oracle: {pin}\ndata:   {pin_data}')
    data = ctx.data.resolve('IMPORT_SYSTEM', 'import-diff')
    ctx.emit(f'capi data: {data} ({NAMES[data_extension(data)]})\n')
    pin_log, subject_log = ctx.pair(driver, pin, subject, pin_data, data, quiet=True)
    sorted_logs = []
    for label, log in [('capi', subject_log), ('oracle', pin_log)]:
        path = ctx.scratch() / 'rows'
        path.write_bytes(log)
        ordered = ctx.checked(['sort', path], capture=True).stdout
        sorted_logs.append(ordered)
        ctx.emit(f'--- {label} triples ---')
        rows = b''.join(line for line in ordered.splitlines(keepends=True) if re.match(rb'^(add |empty batch|save|phrase)', line))
        ctx.write(rows)
        if not rows:
            return 1
    rc, diff = ctx.compare(sorted_logs[1], sorted_logs[0])
    if rc:
        ctx.emit('DIVERGENCE: import logs differ')
        ctx.write(diff)
        return 2
    ctx.emit('import-diff: IDENTICAL\n\n--- classic-format frontend interop ---')
    fixture = ctx.path(ctx.env.get('IMPORT_FIXTURE') or str(ctx.here / 'classic-import.txt'))

    build_value = ctx.env.get('PINYIN_IBUS_BUILD_DIR')
    build = ctx.path(build_value) if build_value else next((candidate for candidate in (
        ctx.settings.root / 'target/oracle-pin-build', ctx.settings.root / 'target/oracle-tkrzw-build',
        ctx.settings.root / 'target/oracle-w2') if frontend_source(candidate) is not None), None)
    source = frontend_source(build) if build is not None else None
    schema = source / 'data/com.github.libpinyin.ibus-libpinyin.gschema.xml' if source is not None else None
    if build is None or source is None or schema is None or not schema.is_file():
        ctx.emit('SKIP: ibus-libpinyin build tree not found (set PINYIN_IBUS_BUILD_DIR)')
        return 77
    ibus_prefix = ctx.path(ctx.env.get('PINYIN_IBUS_PREFIX') or str(build / 'prefix'))
    if not (ibus_prefix / 'oracle-pin.txt').is_file():
        ibus_prefix = prefix
    if not any(line.startswith(ctx.settings.expected_pin.split('+model20-')[0]) for line in (ibus_prefix / 'oracle-pin.txt').read_text().splitlines()):
        ctx.emit(f'SKIP: ibus-libpinyin build at {build} is off-pin')
        return 77
    for command in ('g++', 'pkg-config', 'glib-compile-schemas'):
        if not shutil.which(command, path=ctx.env.get('PATH')):
            ctx.emit(f'SKIP: {command} not found for frontend interop harness')
            return 77
    import shlex
    cflags = ctx.checked(['pkg-config', '--cflags', 'ibus-1.0', 'glib-2.0', 'gio-2.0', 'sqlite3'], capture=True).stdout
    libraries = ctx.checked(['pkg-config', '--libs', 'ibus-1.0', 'glib-2.0', 'gio-2.0', 'sqlite3'], capture=True).stdout
    objects = sorted(file for file in (source / 'src').glob('*.o') if not file.name.endswith('PYMain.o'))
    frontend = ctx.here / 'frontend-import'
    ctx.checked(['g++', '-std=gnu++17', '-O2', '-I' + str(source / 'src'),
                 '-I' + str(ibus_prefix / 'include/libpinyin-2.11.92'), *shlex.split(cflags.decode()),
                 'frontend-import.cc', *objects, *shlex.split(libraries.decode()),
                 '-L' + str(ibus_prefix / 'lib'), '-lpinyin', '-o', frontend])
    schemas = ctx.scratch()
    shutil.copyfile(schema, schemas / schema.name)
    ctx.checked(['glib-compile-schemas', schemas])
    work = ctx.scratch()
    inherited = ctx.env.get('LD_LIBRARY_PATH', '')
    from .cell import library_path
    inherited = library_path(inherited, ctx.caller)

    def run_frontend(input_file: Path, output_file: Path, cache: Path):
        shutil.rmtree(cache, ignore_errors=True)
        cache.mkdir()
        result = ctx.run(frontend, frontend, [input_file, output_file], side='pin', env={
            'LD_LIBRARY_PATH': str(ibus_prefix / 'lib') + (':' + inherited if inherited else ''),
            'XDG_CACHE_HOME': str(cache), 'GSETTINGS_SCHEMA_DIR': str(schemas)})
        (cache / 'stderr').write_bytes(result.stderr)
        if result.returncode:
            ctx.emit(f'FAIL: pinned frontend import/export failed for {input_file}')
            ctx.write(result.stderr)
            raise Finished(1)

    dictool = ctx.settings.artifact('OXPINYIN_DICTOOL', 'oxpinyin-dictool', 'oxpinyin-dictool')
    frontend_a, dictool_b, frontend_c = work / 'frontend-a.txt', work / 'dictool-b.txt', work / 'frontend-c.txt'
    run_frontend(fixture, frontend_a, work / 'frontend-cache-a')
    for command in ([dictool, 'import', '--user-dir', work / 'dictool-user', fixture],
                    [dictool, 'export', '--user-dir', work / 'dictool-user', dictool_b]):
        result = ctx.run(dictool, dictool, command[1:], side='subject')
        ctx.write(result.stdout)
        ctx.write(result.stderr, error=True)
        if result.returncode:
            return status(result.returncode)
    def ordered(file: Path) -> bytes:
        return ctx.checked(['sort', file], capture=True).stdout
    ctx.emit('frontend(fixture):')
    left = ordered(frontend_a)
    ctx.write(left)
    ctx.emit('dictool(fixture):')
    right = ordered(dictool_b)
    ctx.write(right)
    rc, diff = ctx.compare(left, right)
    if rc:
        ctx.emit('DIVERGENCE: classic fixture export differs between frontend and dictool')
        ctx.write(diff)
        return 2
    run_frontend(dictool_b, frontend_c, work / 'frontend-cache-c')
    ctx.emit('frontend(dictool-written file):')
    third = ordered(frontend_c)
    ctx.write(third)
    rc, diff = ctx.compare(right, third)
    if rc:
        ctx.emit('DIVERGENCE: dictool-written file did not round-trip through the frontend')
        ctx.write(diff)
        return 2
    ctx.emit('classic frontend interop: IDENTICAL (both directions)')
    return 0


RECIPES['import-diff'] = import_diff

from .runner_identity import RECIPES as IDENTITY_RECIPES
RECIPES.update(IDENTITY_RECIPES)
