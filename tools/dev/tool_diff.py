"""Case-file comparison of pin-built utilities and their Rust counterparts.

Run via `just runner tool-diff --help` or the registered compat runner.
No utility is executed to discover expected behavior. Cases cite source-derived
contracts; policy observations bind through the existing policy-record machinery.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import re
import time
from dataclasses import replace

from tools.bisection import contract_policy, contract_verdict
from . import pool

GLIB_PREFIX = re.compile(r'(\*\* \([^():\n]+:)\d+(\): (?:CRITICAL|WARNING|MESSAGE|ERROR|DEBUG|INFO) \*\*: )'
                         r'\d\d:\d\d:\d\d\.\d+: ')


def relative(name):
    path = Path(name)
    if not name or path.is_absolute() or '..' in path.parts:
        raise ValueError(f'case path must stay in its working directory: {name!r}')
    return path


def payload(value, root, model):
    """Literal bytes, a checked-in file, or a model table in generator row order."""
    if set(value) == {'text'}:
        return value['text'].encode()
    if set(value) == {'hex'}:
        return bytes.fromhex(value['hex'])
    if set(value) == {'file'}:
        return (root / relative(value['file'])).read_bytes()
    if set(value) == {'model_table', 'format'}:
        if model is None:
            raise ValueError('model-table case requires PINYIN_MODEL_DIR')
        mapping = {}
        if value['format'] == 'zhuyin':
            # The committed original spelling rows, not a query of either binary.
            table = (root / 'crates/oxpinyin-core/src/table_data/zhuyin_indexes.rs').read_text()
            table = table.split('];', 1)[0]
            mapping = {pinyin: zhuyin for _, zhuyin, pinyin in re.findall(
                r'\("([^"]*)", "([^"]*)", "([^"]*)", 0\)', table)}
        elif value['format'] != 'pinyin':
            raise ValueError('model format must be pinyin or zhuyin')
        rows = []
        for number, line in enumerate((model / relative(value['model_table'])).read_text().splitlines(), 1):
            reading, phrase, _token, frequency = line.split()
            if value['format'] == 'zhuyin':
                syllables = []
                for syllable in reading.split("'"):
                    match = re.fullmatch(r'(.+?)([1-5]?)', syllable)
                    key, tone = match.groups()
                    if key not in mapping:
                        raise ValueError(f'{value["model_table"]}:{number}: no canonical zhuyin for {key}')
                    syllables.append(mapping[key] + {'': '', '1': '', '2': 'ˊ', '3': 'ˇ', '4': 'ˋ', '5': '˙'}[tone])
                rows.append(f'{phrase} {frequency} ' + "'".join(syllables) + '\n')
            else:
                rows.append(f'{phrase} {reading} {frequency}\n')
        return ''.join(rows).encode()
    raise ValueError('input must specify exactly text, hex, file, or model_table/format')


def load_cases(paths):
    cases = []
    for path in paths:
        document = json.loads(path.read_text())
        if document.get('schema') != 1 or not isinstance(document.get('cases'), list):
            raise ValueError(f'{path}: expected schema 1 case file')
        for case in document['cases']:
            if (not isinstance(case.get('id'), str) or not case['id'] or
                    not re.fullmatch(r'[A-Za-z0-9_]+', case.get('utility', '')) or
                    not case.get('source') or
                    not isinstance(case.get('args', []), list) or
                    not all(isinstance(arg, str) for arg in case.get('args', []))):
                raise ValueError(f'{path}: case needs id, utility, arguments and source citation')
            if case.get('pin_signal') not in (None, -11, -6):
                raise ValueError(f'{path}: unsupported registered pin signal')
            if case.get('pin_signal') and not case.get('policy_record'):
                raise ValueError(f'{path}: a pin signal requires a named policy record')
            for name in case.get('files', {}):
                relative(name)
            cases.append(case)
    if not cases or len({case['id'] for case in cases}) != len(cases):
        raise ValueError('case selection must be nonempty with unique IDs')
    return cases


def snapshot(directory):
    """Every file, including unchanged inputs and zero-byte outputs; no text decoding."""
    files = {}
    for path in sorted(directory.rglob('*')):
        name = str(path.relative_to(directory))
        if path.is_symlink():
            files[name] = 'symlink:' + path.readlink().as_posix()
        elif path.is_file():
            files[name] = 'hex:' + path.read_bytes().hex()
        elif path.is_dir():
            files[name + '/'] = 'directory'
        else:
            raise ValueError(f'unsupported output file kind: {path}')
    return files


def observation(ctx, binary, case, directory, inputs, stdin, deadline):
    directory.mkdir()
    for name, content in inputs.items():
        destination = directory / relative(name)
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(content)
    environment = dict(ctx.env, LC_ALL='C', **case.get('env', {}))
    # Keep argv[0]'s basename identical (including --help) using the built
    # utility, rather than libtool's lt-* executable in .libs.
    parent = ctx.services.context
    context = replace(parent, cancelled=lambda: (parent.check() or time.monotonic() >= deadline))
    result = context.launch([str(binary), *case.get('args', [])], cwd=directory,
                            env=environment, input=stdin)
    return dict(exit=result.status, stdout=result.stdout.decode('utf-8', 'surrogateescape'),
                stderr=GLIB_PREFIX.sub(r'\1PID\2TIME: ', result.stderr.decode('utf-8', 'surrogateescape')),
                files=snapshot(directory))


def verdict(case, pin, subject, policy_root, cell):
    for side, value in (('pin', pin), ('subject', subject)):
        problem = contract_policy.tool_observation_problem(
            value, pin_signal=case.get('pin_signal') if side == 'pin' else None)
        if problem:
            return 'INVALID', f'{side}: {problem}'
    if case.get('policy_record'):
        try:
            kept = contract_verdict.policy_expectations(
                dict(policy_record=case['policy_record'], case_id=case['id']),
                policy_root=policy_root, cell=cell)
        except (ValueError, OSError, KeyError, TypeError) as error:
            return 'INVALID', str(error)
        actual = dict(pin=pin, subject=subject)
        if contract_policy.accepted(kept, actual):
            return 'POLICY-ACCEPTED', case['policy_record']
        return 'DIFFER', contract_policy.describe(contract_policy.diagnose(kept, actual))
    return ('MATCH', '') if pin == subject else ('DIFFER', '\n'.join(contract_policy.divergence(pin, subject)))


def run(ctx, argv):
    parser = argparse.ArgumentParser(prog='just runner tool-diff', description=__doc__)
    parser.add_argument('--case-file', action='append', type=Path)
    parser.add_argument('--cases', help='comma-separated case IDs; default every case')
    parser.add_argument('--pin-build', type=Path, default=ctx.env.get('OXPINYIN_TOOL_PIN_BUILD'))
    parser.add_argument('--subject-dir', type=Path, default=ctx.settings.target / 'debug')
    parser.add_argument('--observations', type=Path, help='capture for just policy-record (local only)')
    parser.add_argument('--case-timeout', type=float, default=300)
    parser.add_argument('--expect-differ', action='store_true', help='named mutation/revert check; requires --cases')
    args = parser.parse_args(argv)
    if args.case_timeout <= 0 or args.expect_differ and not args.cases:
        parser.error('positive timeout and explicit --cases for --expect-differ required')
    cases = load_cases(args.case_file or sorted((ctx.here / 'tool-cases').glob('*.json')))
    if args.cases:
        names = args.cases.split(',')
        catalog = {case['id']: case for case in cases}
        if any(name not in catalog for name in names):
            parser.error('unknown case in --cases')
        cases = [catalog[name] for name in dict.fromkeys(names)]
    if args.pin_build is None:
        parser.error('requires --pin-build or OXPINYIN_TOOL_PIN_BUILD (oracle libpinyin source/build tree)')
    pin_build = args.pin_build.resolve()
    expected = dict(re.findall(r'^(LIBPINYIN_SHA)=(.+)$',
                   (ctx.settings.root / 'tools/oracle/build-oracle.sh').read_text(), re.M))['LIBPINYIN_SHA']
    head = ctx.checked(['git', 'rev-parse', 'HEAD'], cwd=pin_build, capture=True).stdout.decode().strip()
    if head != expected or ctx.checked(['git', 'diff', 'HEAD', '--', 'utils', 'src'],
                                       cwd=pin_build, capture=True).stdout:
        raise ValueError('pin tool build must use the unmodified oracle source at ' + expected)
    model = Path(ctx.env['PINYIN_MODEL_DIR']) if ctx.env.get('PINYIN_MODEL_DIR') else None
    binaries = {}
    for case in cases:
        name = case['utility']
        pair = (pin_build / 'utils/storage' / name, args.subject_dir.resolve() / name)
        if any(not binary.is_file() for binary in pair):
            raise ValueError(f'missing built utility: {pair}')
        binaries[name] = pair
    capture = ctx.stack.enter_context(args.observations.open('w', encoding='utf-8')) if args.observations else None
    def compare(child, case):
        scratch = child.scratch()
        inputs = {name: payload(value, child.settings.root, model) for name, value in case.get('files', {}).items()}
        stdin = payload(case.get('stdin', {'text': ''}), child.settings.root, model)
        deadline = time.monotonic() + args.case_timeout
        try:
            pin, subject = [observation(child, binary, case, scratch / side, inputs, stdin, deadline)
                            for side, binary in zip(('pin', 'subject'), binaries[case['utility']])]
        except pool.UnitStopped as error:
            return case, 'INVALID', str(error), None
        result, detail = verdict(case, pin, subject, child.settings.root / 'docs/findings', child.settings.cell)
        row = dict(runner='tool-diff', cell=child.settings.cell, case=case['id'], userdir=False, abort=False,
                   pin_signal=case.get('pin_signal'), pin=pin, subject=subject)
        return case, result, detail, row
    results = ctx.campaign(cases, compare)
    counts = Counter()
    success = True
    for case, result, detail, row in results:
        counts[result] += 1
        ctx.emit(f'{result}: {case["id"]}' + (f' ({detail})' if detail else ''))
        success &= result == 'DIFFER' if args.expect_differ else result in ('MATCH', 'POLICY-ACCEPTED')
        if capture and row:
            capture.write(contract_policy.render(row) + '\n')
    ctx.emit(', '.join(f'{count} {name}' for name, count in sorted(counts.items())))
    return 0 if success else 1
