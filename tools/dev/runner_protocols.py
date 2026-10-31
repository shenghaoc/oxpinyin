"""Protocol and option campaigns built on the shared registry runner services."""
from collections.abc import Sequence
from pathlib import Path
import hashlib
import re
import shutil

from .runners import Context, Finished, emit, status, write
from .runner_state import state_inputs

SEEDS = {
    'control-5': b'open counter:5\n', 'control-7': b'open counter:7\n', 'negative': b'open counter:-3\n',
    'plus': b'open counter:+3\n', 'plus-over': b'open counter:+7\n', 'garbage': b'open counter:3x\n',
    'garbage-over': b'open counter:7x\n', 'garbage-byte': b'open counter:3\xff\n',
    'invalid-prefix': b'open counter:5\n', 'space': b'open counter: 5\n', 'tab': b'open counter:\t5\n',
    'vtab': b'open counter:\v5\n', 'newline': b'open counter:\n5\n', 'indent': b'  open counter:5\n',
    'spaced': b'open   counter:5\n', 'joined': b'opencounter:5\n',
    'overflow-int': b'open counter:2147483648\n', 'overflow-wrap': b'open counter:4294967303\n',
    'overflow-long': b'open counter:99999999999999999999\n', 'overflow-neg': b'open counter:-99999999999999999999\n',
    'under-int': b'open counter:-2147483649\n', 'empty': b'open counter:\n', 'sign': b'open counter:-\n', 'missing': b''}
CONF_CASES = ('binver-plus', 'binver-wrap', 'binver-neg', 'binver-junk', 'binver-space',
              'modelver-plus', 'modelver-wrap', 'modelver-neg', 'foreign-line', 'reordered',
              'token-junk-tail', 'no-newline', 'empty')
ABORT_CASES = ('unknown-token', 'long-token', 'trunc-counter', 'modelver-junk', 'no-dbformat')
CLASS_C = b'check_format: unknown database format in user.conf'


def conf_body(case: str, identity: Sequence[bytes]) -> bytes:
    first, second, third = identity[:3]
    lines = [first, second, third, b'open counter:5']
    if case.startswith('binver-'):
        values = {'plus': b'+7', 'wrap': b'4294967303', 'neg': b'-1', 'junk': b'7x', 'space': b' 7'}
        lines[0] = b'binary format version:' + values[case.removeprefix('binver-')]
    elif case.startswith('modelver-'):
        values = {'plus': b'+14', 'wrap': b'4294967310', 'neg': b'-1', 'junk': b'14x'}
        lines[1] = b'model data version:' + values[case.removeprefix('modelver-')]
    elif case == 'foreign-line':
        lines.insert(3, b'junk line')
    elif case == 'reordered':
        lines[:2] = [second, first]
    elif case == 'token-junk-tail':
        lines[2] += b' extra'
    elif case == 'empty':
        return b''
    elif case in ('unknown-token', 'long-token', 'trunc-counter'):
        token = b'NotADbmLibrary' if case == 'unknown-token' else b'A' * (300 if case == 'long-token' else 255)
        lines[2] = b'database format:' + token
        if case == 'trunc-counter':
            return b'\n'.join(lines[:2] + [lines[2] + b'open counter:5']) + b'\n'
    elif case == 'no-dbformat':
        del lines[2]
    elif case != 'no-newline':
        raise ValueError('unknown case: ' + case)
    return b'\n'.join(lines) + (b'' if case == 'no-newline' else b'\n')


def snapshot(directory: Path) -> bytes:
    conf = directory / 'user.conf'
    body = conf.read_bytes() if conf.is_file() else None
    result = (f'user.conf: {len(body)} bytes\n'.encode() + b''.join(b'conf: ' + line for line in body.splitlines(keepends=True))
              if body is not None else b'user.conf: absent\n')
    return result + ('files: ' + ''.join(file.name + ' ' for file in sorted(directory.iterdir())) + '\n').encode()


def directory_digest(directory: Path) -> tuple[tuple[str, bytes], ...]:
    return tuple((file.name, hashlib.sha256(file.read_bytes()).digest()) for file in sorted(directory.iterdir()))


SUMMARY = r'''function key() { return $2 "/" $3 "/" ($2 == "T" ? $4 : "") }
function val() { return $2 == "T" ? $5 : $4 "|" $5 }
/^launch: / { split($0, a, " "); n = a[2]; mode[n] = a[3]; p_init[n] = 0; t_init[n] = 0; p_saved[n] = 0; t_saved[n] = 0 }
/^phrases@init: / { split($0, a, " "); p_init[n] = a[2] }
/^phrases@saved: / { split($0, a, " "); p_saved[n] = a[2] }
$1 == "row@init" && ($2 == "T" || $2 == "R") { if (n == 1) base[key()] = val(); else if (val() != base[key()]) t_init[n]++ }
$1 == "row@saved" && ($2 == "T" || $2 == "R") { if (val() != base[key()]) t_saved[n]++ }
/^conf: open counter:/ { split($0, a, ":"); counter[n] = a[3] }
/^user.conf: absent/ { counter[n] = "-" }
END { left = 0; for (i = 1; i <= n; i++) {
found = p_init[i] + t_init[i]; wipe = (i > 1 && found < left) ? "  WIPE" : "";
printf "  launch %2d %-5s learned@init=%-2d (phrases %d, trained %d)  counter=%s%s\n", i, mode[i], found, p_init[i], t_init[i], counter[i], wipe;
left = (mode[i] == "cycle") ? p_saved[i] + t_saved[i] : found } }'''
SEED_SUMMARY = r'''/^launch: / { split($0, a, " "); n = a[2]; look = (a[3] == "look"); init = "" }
look && /^conf@init: / && index($0, "counter:") { init = substr($0, index($0, "counter:") + 8) }
look && /^conf: / && index($0, "counter:") { fini = substr($0, index($0, "counter:") + 8) }
look && /^files: / { kept = (NF > 2) ? "kept" : "wiped"; printf "  launch %d: counter %s at init, %s at fini, profile %s\n", n, (init == "" ? "-" : init), fini, kept }'''


def open_counter(ctx: Context, args: Sequence[str]) -> int:
    inputs = state_inputs(ctx, 'OPEN_COUNTER', 'pinyin zhuyin')
    protocols = (ctx.env.get('OPEN_COUNTER_PROTOCOLS') or 'cycle crash seeded abort').split()
    cycles, crashes = int(ctx.env.get('OPEN_COUNTER_CYCLES') or '10'), int(ctx.env.get('OPEN_COUNTER_CRASHES') or '10')
    ctx.emit('--- building open-counter-diff driver ---')
    driver = ctx.compile('open-counter-diff', inputs.work / 'open-counter-diff', glib=True, absolute_source=True)
    ctx.emit('build: ok')



    verdict = 0
    expanded = []
    for protocol in protocols:
        if protocol == 'seeded':
            expanded += ['seed-' + seed for seed in SEEDS] + ['conf-' + case for case in CONF_CASES]
        elif protocol != 'abort':
            expanded.append(protocol)
    def one(ctx: Context, library) -> int:
        verdict = 0
        def launch(library: str, so: Path, user: Path, number: int, mode: str, temporary: Path):
            return ctx.run(driver, so, [library, so, inputs.data, user, str(number), mode],
                           side='pin' if so == inputs.pin[library] else 'subject', env={'TMPDIR': str(temporary)})
        def run_protocol(library: str, so: Path, protocol: str, log: Path) -> bytes:
            user, temporary = inputs.work / ('user-' + log.stem), inputs.work / ('tmp-' + log.stem)
            for directory in (user, temporary):
                shutil.rmtree(directory, ignore_errors=True)
                directory.mkdir()
            body, diagnostics = b'', b''
            modes = (['cycle'] * cycles if protocol == 'cycle' else
                     ['cycle', 'cycle', *(['crash'] * crashes), 'cycle'] if protocol == 'crash' else
                     ['cycle', 'look', 'look'] if protocol.startswith(('seed-', 'conf-')) else [])
            for number, mode in enumerate(modes, 1):
                result = launch(library, so, user, number, mode, temporary)
                body += result.stdout
                diagnostics += result.stderr
                rc = status(result.returncode)
                if (mode == 'crash' and rc != 137) or (mode != 'crash' and rc):
                    ctx.emit(f'FAIL: launch {number} ({mode}) of {so} exited {rc}', error=True)
                    ctx.write(diagnostics, error=True)
                    raise Finished(1)
                body += b'exit: SIGKILL\n' if mode == 'crash' else b'exit: 0\n'
                body += snapshot(user)
                if number == 1 and protocol.startswith(('seed-', 'conf-')):
                    case = protocol.split('-', 1)[1]
                    conf = user / 'user.conf'
                    identity = conf.read_bytes().splitlines()[:3]
                    if protocol.startswith('seed-'):
                        conf.write_bytes((b'\xff\n' if case == 'invalid-prefix' else b'') + b'\n'.join(identity) + b'\n' + SEEDS[case])
                    else:
                        conf.write_bytes(conf_body(case, identity))
                    body += f'seeded: {case}\n'.encode() + snapshot(user)
            log.write_bytes(body)
            log.with_name(log.name + '.stderr').write_bytes(diagnostics)
            return body

        for protocol in expanded:
            ctx.emit(f'=== {library} / {protocol} ===')
            paths = [inputs.output / f'{library}-{protocol}-{side}.log' for side in ('oracle', 'oxpinyin')]
            logs = [run_protocol(library, so, protocol, path) for so, path in zip((inputs.pin[library], inputs.subject[library]), paths)]
            if any(b'save: 1' not in log.splitlines() for log in logs):
                ctx.emit('FAIL: a side never saved a learned phrase; the comparison would be vacuous', error=True)
                raise Finished(1)
            for side, path in zip(('pin', 'oxpinyin'), paths):
                ctx.emit(side + ':')
                ctx.checked(['awk', '-F\t', SEED_SUMMARY if protocol.startswith('seed-') else SUMMARY, path])
            rc, diff = ctx.compare(*logs, paths=tuple(paths))
            (inputs.output / f'{library}-{protocol}.diff').write_bytes(diff)
            if rc > 1:
                ctx.emit(f'FAIL: could not compare {paths[0]} and {paths[1]}', error=True)
                raise Finished(1)
            ctx.emit(f'{library}/{protocol}: ' + (f'IDENTICAL ({logs[0].count(bytes([10]))} lines)' if rc == 0 else 'DIVERGED'))
            if rc:
                ctx.write(diff)
                verdict = 2
        if 'abort' not in protocols:
            return verdict
        for case in ABORT_CASES:
            ctx.emit(f'=== {library} / abort-{case} ===')
            seed = inputs.work / f'abort-{library}-{case}'
            shutil.rmtree(seed, ignore_errors=True)
            (seed / 'user').mkdir(parents=True)
            (seed / 'tmp-seed').mkdir()
            result = launch(library, inputs.pin[library], seed / 'user', 1, 'cycle', seed / 'tmp-seed')
            if result.returncode:
                raise Finished(1)
            conf = seed / 'user/user.conf'
            conf.write_bytes(conf_body(case, conf.read_bytes().splitlines()[:3]))
            shutil.copytree(seed / 'user', seed / 'template')
            wanted = directory_digest(seed / 'template')
            failed = False
            for side, so in [('oracle', inputs.pin[library]), ('oxpinyin', inputs.subject[library])]:
                user, temporary = seed / ('user-' + side), seed / ('tmp-' + side)
                shutil.copytree(seed / 'template', user)
                temporary.mkdir()
                result = launch(library, so, user, 2, 'look', temporary)
                log = inputs.output / f'{library}-abort-{case}-{side}.log'
                log.write_bytes(result.stdout)
                log.with_name(log.name + '.stderr').write_bytes(result.stderr)
                rc = status(result.returncode)
                if side == 'oracle':
                    if rc == 134:
                        ctx.emit('  oracle: exit 134 (SIGABRT)')
                    else:
                        ctx.emit(f'  oracle: FAIL - expected exit 134, got {rc}', error=True)
                        failed = True
                elif library == 'pinyin':
                    # The pinyin facade reproduces the pin's `abort()`
                    # (register row 44, 2026-10-31 UTC): SIGABRT, no
                    # warning line. The zhuyin facade still refuses below.
                    if rc == 134:
                        ctx.emit('  oxpinyin: exit 134 (SIGABRT)')
                    else:
                        ctx.emit(f'  oxpinyin: FAIL - expected exit 134, got {rc}', error=True)
                        failed = True
                    count = sum(CLASS_C in line for line in result.stderr.splitlines())
                    if count:
                        ctx.emit(f'  oxpinyin: FAIL - expected no refusal line, got {count}', error=True)
                        failed = True
                else:
                    if b'init: NULL' in result.stdout.splitlines():
                        ctx.emit('  oxpinyin: init answered NULL')
                    else:
                        ctx.emit('  oxpinyin: FAIL - expected \'init: NULL\', got:               ' + result.stdout.replace(b'\n', b' ')[:200].decode(errors='replace'), error=True)
                        failed = True
                    if rc == 1:
                        ctx.emit('  oxpinyin: exit 1 (the open failed, as the C ABI says)')
                    else:
                        ctx.emit(f'  oxpinyin: FAIL - expected exit 1, got {rc}', error=True)
                        failed = True
                    count = sum(CLASS_C in line for line in result.stderr.splitlines())
                    if count == 1:
                        ctx.emit('  oxpinyin: one class-(c) line (' + CLASS_C.decode() + ')')
                    else:
                        ctx.emit(f'  oxpinyin: FAIL - expected exactly one class-(c) line, got {count}', error=True)
                        ctx.write(b''.join(b'    ' + line for line in result.stderr.splitlines(keepends=True)), error=True)
                        failed = True
                if directory_digest(user) == wanted:
                    ctx.emit(f'  {side}: user dir byte-identical to the seeded dir')
                else:
                    ctx.emit(f'  {side}: FAIL - the user dir moved' + (' (a wipe is not this class)' if side == 'oxpinyin' else ''), error=True)
                    failed = True
            if failed:
                ctx.emit(f'FAIL: abort-{case} did not hold its expectation', error=True)
                verdict = 2
        return verdict

    results = ctx.campaign(inputs.libraries, one)
    verdict = 1 if 1 in results else 2 if 2 in results else 0
    ctx.emit('RESULT: every open-counter protocol is byte-identical to the pin' if not verdict else
         'RESULT: at least one open-counter protocol DIVERGED from the pin')
    return verdict


RECIPES = {'open-counter-diff': open_counter}


def candidate_sequences(log: bytes) -> dict[str, str]:
    table, current = {}, None
    for raw in log.decode().splitlines():
        if raw.startswith('input='):
            current = raw.split()[0].split('=', 1)[1]
            table[current] = []
        elif current is not None and raw.startswith('cand['):
            table[current].append(raw.split('=', 1)[1])
    return {name: '|'.join(values) for name, values in table.items()}


def text_order(pin: bytes, subject: bytes) -> tuple[str, list[str]]:
    oracle, capi = candidate_sequences(pin), candidate_sequences(subject)
    if oracle == capi:
        return 'identical', ['  TEXT/ORDER identical (top-10)']
    verdict, lines = 'identical', []
    for name, oracle_sequence in oracle.items():
        capi_sequence = capi.get(name, '')
        if oracle_sequence == capi_sequence:
            continue
        oracle_items, capi_items = oracle_sequence.split('|'), capi_sequence.split('|')
        if sorted(oracle_items) != sorted(capi_items):
            if name in ('cang', 'sang', 'lve', 'lue', 'agn', 'amg', 'ang'):
                if verdict == 'identical':
                    verdict = 'w12-residual'
                lines.append(f'  W12  input={name}  TEXT-set tail; all-off residual, not W10 (docs/findings/option-bits.md)')
            else:
                verdict = 'stop'
                lines.append(f'  STOP  input={name}  TEXT set differs (not W11 ground; ABI verification)')
            lines += ['    oracle: ' + (oracle_sequence or '<empty>'), '    capi:   ' + (capi_sequence or '<empty>')]
        else:
            mismatch = any(len(left) != len(right) for left, right in zip(oracle_items, capi_items))
            if mismatch:
                verdict = 'stop'
                lines.append(f'  STOP  input={name}  order differs and phrase_length (RankKey 1) mismatches')
            else:
                if verdict == 'identical':
                    verdict = 'tie-order'
                lines.append(f'  TIE-ORDER  input={name}  same 10-set, phrase_length tied; span/freq/collection-order not on the ABI')
            lines += ['    oracle: ' + oracle_sequence, '    capi:   ' + capi_sequence]
    return verdict, lines


def option_data(ctx: Context) -> Path:
    value = ctx.env.get('OPTION_SWEEP_CAPI_DATA')
    if value:
        data = ctx.path(value)
        ctx.data.require_complete(data, 'OPTION_SWEEP_CAPI_DATA', 'option-sweep')
        return data
    export = Path('/tmp/oxpinyin-export')
    extension = ctx.data.detect(export) if not ctx.env.get('OXPINYIN_SYSTEM_DIR') else None
    if extension:
        missing = [f'{stem}.{extension}' for stem in ('pinyin_index', 'phrase_index', 'bigram') if not (export / f'{stem}.{extension}').is_file()]
        candidates = [ctx.path(ctx.env['PINYIN_MODEL_DIR'])] if ctx.env.get('PINYIN_MODEL_DIR') else []
        candidates.append(ctx.settings.root / 'target/model20/extracted')
        interpolation = next((directory / 'interpolation2.text' for directory in candidates if (directory / 'interpolation2.text').is_file()), None)
        if missing or interpolation is None:
            lines = ['fatal: option-sweep: cannot assemble a system directory.']
            if missing:
                lines += ['', 'Missing from /tmp/oxpinyin-export:', *('  ' + name for name in missing)]
            if interpolation is None:
                lines += ['', 'No interpolation2.text in:', '  $PINYIN_MODEL_DIR', f'  {ctx.settings.root}/target/model20/extracted']
            lines += ['', 'All four are required: the three tables plus interpolation2.text,',
                      "whose real unigrams are what the oracle scores against. Short of",
                      "them the comparison is flat-export unigrams versus the pin's real",
                      'ones -- a data mismatch that reports as a divergence. Fetch the',
                      'model with just fetch-model, or point',
                      'OPTION_SWEEP_CAPI_DATA at a directory that already holds all four.']
            ctx.emit('\n'.join(lines), error=True)
            raise Finished(3)
        data = ctx.scratch()
        ctx.data.copy(export, data)
        shutil.copyfile(interpolation, data / 'interpolation2.text')
        return data
    return ctx.data.resolve('OPTION_SWEEP_CAPI_DATA', 'option-sweep')


def option_sweep(ctx: Context, args: Sequence[str]) -> int:
    from .runner_recipes import build_subject
    driver, subject = build_subject(ctx, stem='option-sweep', heading='option-sweep')
    data = option_data(ctx)
    ctx.emit(f'capi data: {data}')
    pin, pin_data = ctx.pin(hint=True)
    ctx.emit(f'oracle: {pin}\noracle data: {pin_data}\n')
    base = 0x8 | 0x80 | 0x100
    cases = [('baseline', base), ('fork-default', 0x1fe00198)]
    cases += [(f'correct-{bit}', base | (1 << bit)) for bit in range(21, 29)]
    cases += [(f'amb-{bit}', base | (1 << bit)) for bit in range(10, 20)]
    cases += [('amb-chain', base | (1 << 10) | (1 << 17)), ('all-off', 0),
              ('divided-contrast-off', 0x8), ('divided-contrast-on', 0x88)]
    pair_cases = [(f'pairs-{bit}', base | (1 << bit)) for bit in (11, 12, 21, 22, 23, 24, 25, 26, 27, 28)]
    pair_cases += [('pairs-correct-all', base | (0xFF << 21))]
    stopped = False
    def one(ctx: Context, item) -> bool:
        pairs, name, options = item
        stopped = False
        logs = []
        for side, so, directory in [('subject', subject, data), ('pin', pin, pin_data)]:
            result = ctx.run(driver, so, [so, directory, name, f'{options:x}', *(['pairs'] if pairs else [])], side=side, quiet=True)
            if result.returncode:
                ctx.emit('FAIL: option-sweep' + (' pairs' if pairs else '') + ' crashed against ' +
                     ('oxpinyin-capi' if side == 'subject' else 'the oracle') + f' ({name})')
                if not pairs:
                    ctx.write(result.stdout)
                raise Finished(1)
            logs.append(result.stdout)
        subject_log, pin_log = logs
        if not pairs:
            filtered = [b''.join(line for line in log.splitlines(keepends=True) if re.match(rb'^(case=|input=|aux=|keys=)', line)) for log in (pin_log, subject_log)]
            rc, diff = ctx.compare(*filtered)
            verdict, lines = text_order(pin_log, subject_log)
            ctx.emit(f'{"FAIL" if rc else "PASS"}  {name}  0x{options:08x}  [parse]')
            for line in lines:
                ctx.emit(line)
            ctx.emit('  TEXT/ORDER ' + verdict)
            if rc:
                ctx.write(diff)
                raise Finished(2)
            if verdict == 'stop':
                ctx.emit(f'STOP: {name} candidate TEXT/ORDER diverges beyond a RankKey-1 tie')
                stopped = True
        else:
            count = '\n'.join(line[6:].decode() for line in pin_log.splitlines() if line.startswith(b'pairs='))
            if pin_log == subject_log:
                ctx.emit(f'PASS  {name}  0x{options:08x}  [pairs: {count}]')
                return stopped
            work = ctx.scratch()
            paths = []
            for side, log in [('pin', pin_log), ('subject', subject_log)]:
                path = work / side
                path.write_bytes(log)
                lines = log.splitlines()
                path.write_bytes(b''.join(lines[index] + b'\t' + (lines[index + 1] if index + 1 < len(lines) else b'') + b'\n' for index in range(0, len(lines), 2)))
                paths.append(path)
            result = ctx.command(['diff', *paths], capture=True)
            differing = sum(line.startswith(b'<') for line in result.stdout.splitlines())
            ctx.emit(f'FAIL  {name}  0x{options:08x}  [pairs: {differing} of {count} differ]')
            ctx.write(b''.join(ctx.compare(pin_log, subject_log)[1].splitlines(keepends=True)[:40]))
            raise Finished(2)
        return stopped

    selections = [(False, name, options) for name, options in cases] + [(True, name, options) for name, options in pair_cases]
    stopped = any(ctx.campaign(selections, one))
    ctx.emit()
    ctx.emit('option-sweep: STOP — candidate TEXT/ORDER diverged beyond a RankKey-1 tie' if stopped else
         'option-sweep: PASS — parse/aux identical; TEXT/ORDER identical, tie-order-only, or W12-excluded')
    return 2 if stopped else 0


RECIPES['option-sweep'] = option_sweep


def train_off(ctx: Context, args: Sequence[str]) -> int:
    from .runner_recipes import build_subject
    driver, subject = build_subject(ctx, heading='train-diff', print_capi=False, stem='train-diff')
    pin, pin_data = ctx.pin(verify=False)
    data = ctx.data.resolve('TRAINDIFF_SYSTEM', 'train-diff')
    options = ctx.env.get('TRAINDIFF_OFF_OPTIONS') or '0x00000188'
    ctx.emit('options: ' + options)

    def observations(directory: Path, *, populated: bool = False):
        logs = []
        env = {'TRAINDIFF_ROUNDS': '1', 'TRAINDIFF_OPTIONS': options}
        if populated:
            env.update(TRAINDIFF_DUMP_CANDIDATES='1', TRAINDIFF_REOPEN='1')
        for side, so, system in [('subject', subject, directory), ('pin', pin, pin_data)]:
            result = ctx.run(driver, so, [so, system], side=side, env=env)
            if result.returncode:
                ctx.write(result.stderr, error=True)
                ctx.emit('FAIL: train-diff' + (' candidate dump' if populated else '') + ' failed against ' +
                     ('oxpinyin-capi' if side == 'subject' else 'the oracle') + f' (exit {status(result.returncode)})')
                ctx.write(result.stdout)
                raise Finished(1)
            logs.append(result.stdout)
        return logs[1], logs[0]

    def ordered(log: bytes) -> bytes:
        file = ctx.scratch() / 'rows'
        file.write_bytes(log)
        return ctx.checked(['sort', file], capture=True).stdout

    pin_log, subject_log = observations(data)
    rc, diff = ctx.compare(ordered(pin_log), ordered(subject_log))
    if rc:
        ctx.emit('FAIL: DYNAMIC_ADJUST-off training exports differ')
        ctx.write(diff)
        return 2
    rows = sum(bool(re.match(rb'^(bigram|phrase):', line)) for line in subject_log.splitlines())
    ctx.emit(f'DYNAMIC_ADJUST off: exports identical; wrote {rows} rows (pin writes, no no-op)')
    capi_data = ctx.path(ctx.env.get('OPTION_SWEEP_CAPI_DATA') or str(data))
    if not (capi_data / 'interpolation2.text').is_file():
        scratch = ctx.scratch()
        shutil.copytree(capi_data, scratch, dirs_exist_ok=True)
        capi_data = scratch
        candidates = [ctx.path(ctx.env['PINYIN_MODEL_DIR'])] if ctx.env.get('PINYIN_MODEL_DIR') else []
        candidates.append(ctx.settings.root / 'target/model20/extracted')
        source = next((directory / 'interpolation2.text' for directory in candidates if (directory / 'interpolation2.text').is_file()), None)
        if source:
            shutil.copyfile(source, capi_data / 'interpolation2.text')
    if not (capi_data / 'interpolation2.text').is_file():
        ctx.emit('SKIP: no full capi tables + interpolation2.text; cannot run populated-store candidate dump\n'
             'train-diff dynamic-off: PASS (exports only)')
        return 77
    ctx.emit(f'--- populated-store candidate dump (DYNAMIC_ADJUST clear) ---\ncapi data: {capi_data}')
    pin_log, subject_log = observations(capi_data, populated=True)
    for log in (pin_log, subject_log):
        if not all(re.search(pattern, log, re.M) for pattern in (rb'^cand:reopened@0 n=', rb'^train-reopened:1$', rb'^reopen-bigram:', rb'^reopen-phrase:')):
            ctx.emit('FAIL: populated persistence phase did not execute\n  reopened cand / train / export lines in log:')
            ctx.emit(str(sum(line.startswith(b'cand:reopened') for line in log.splitlines())))
            return 1
    def select(log: bytes, pattern: bytes) -> bytes:
        return b''.join(line for line in log.splitlines(keepends=True) if re.search(pattern, line))
    rc, diff = ctx.compare(select(pin_log, rb'^cand:'), select(subject_log, rb'^cand:'))
    if rc:
        ctx.emit('DIVERGENCE: populated-store candidate TEXT/ORDER with DYNAMIC_ADJUST clear')
        ctx.write(diff)
        return 2
    ctx.emit('populated-store candidates identical (bit-clear; unigram term ungated, bigram term skipped)')
    for log in (pin_log, subject_log):
        reopened = select(log, rb'^cand:reopened').replace(b'cand:reopened@0', b'cand:nihao@0').replace(b'cand:reopened-after-ni', b'cand:after-ni')
        rc, diff = ctx.compare(select(log, rb'^cand:(nihao@0|after-ni)'), reopened)
        if rc:
            ctx.emit('DIVERGENCE: reopened candidate window differs from the in-memory one')
            ctx.write(diff)
            return 2
    ctx.emit('reopened candidate window equals the in-memory one (both sides)')
    pin_export = ordered(select(pin_log, rb'^reopen-(phrase|bigram):'))
    subject_export = ordered(select(subject_log, rb'^reopen-(phrase|bigram):'))
    rc, diff = ctx.compare(pin_export, subject_export)
    if rc:
        ctx.emit('DIVERGENCE: reopened export triples differ (persistence / subsequent session)')
        ctx.write(diff)
        return 2
    ctx.emit('reopened export identical (subsequent-session train included):')
    ctx.write(subject_export)
    ctx.write(select(subject_log, rb'^cand:'))
    ctx.emit('train-diff dynamic-off: PASS')
    return 0


RECIPES['train-diff-dynamic-off'] = train_off
