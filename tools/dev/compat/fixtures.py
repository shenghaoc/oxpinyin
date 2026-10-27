"""Oracle fixture capture and two-way user/data interchange controllers.

These recipes consume frozen inputs; they never update committed fixtures.
"""
import json
import os
from pathlib import Path
import re
import glob
import shutil
import sys
import tempfile

from ..cell import Settings
from ..measure import bench
from ..runners import Finished
from tools.tkrzw import runtime
from .identity import digest_file
from .images import fields
from .process import ROOT, PYTHON, capture, fail, run, text

CAPTURE_PIN = ('libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99+'
               'model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155+dbm-tkrzw')


def words(value, *, expand=False):
    """Keep Bash's IFS word-list flags, including literal quote/backslash bytes."""
    parts = re.findall(r'[^ \t\n]+', value)
    return [word for part in parts for word in (glob.glob(part) or [part])] if expand else parts


def qualified(command, cell, side, obj, *, env, cwd=None, stdout=None):
    """Certify the object and directly linked worker through the single resolver."""
    try:
        runtime.qualify(cell, 'user-dir-round-trip.sh', side, obj, command, env=env, cwd=cwd)
    except runtime.RuntimeRefused as error:
        fail('REFUSED: ' + str(error))
    return run(command, env=env, cwd=cwd, stdout=stdout)


def checksum_manifest(prefix, path):
    """Verify the installed payload manifest with the original checksum tool."""
    return run(['sha256sum', '--check', '--status', path], cwd=prefix, check=False) == 0


def capture_fixtures(argv, *, root=ROOT):
    """Validate the capture pin and payload, compile the driver, and write its three formats."""
    if len(argv) != 2:
        fail('usage: tools/capture/run-capture.sh ORACLE_PREFIX OUTPUT_DIR', 2)
    prefix, output = Path(text(['realpath', argv[0]])), Path(argv[1])
    here = Path(root) / 'tools/capture'
    build = Path(root) / 'target/capture'
    data = prefix / 'lib/libpinyin/data'
    header = prefix / 'include/libpinyin-2.11.92/pinyin.h'
    library = prefix / 'lib/libpinyin.so.15.0.0'
    manifest = prefix / 'oracle-pin.txt'
    stable = prefix / 'oracle-data.sha256'
    unstable = prefix / 'oracle-data-unstable.sha256'
    for path, message, directory in ((manifest, 'oracle pin manifest not found', False),
                                      (header, 'pinned pinyin.h not found', False),
                                      (library, 'pinned shared object not found', False),
                                      (data, 'pinned data directory not found', True),
                                      (stable, 'oracle data manifest not found', False)):
        if not (path.is_dir() if directory else path.is_file()):
            fail(f'{message}: {path}')
    record = fields(manifest)
    if 'data_unstable_manifest_sha256' in record and not unstable.is_file():
        fail(f'oracle unstable data manifest not found: {unstable}')
    if record.get('schema') != 'pinyin-oracle-v1':
        fail('unsupported oracle manifest schema')
    if 'pin_ref' not in record:
        fail('missing oracle pin ref')
    if record['pin_ref'] != CAPTURE_PIN:
        fail('oracle pin ref does not match capture pin')
    if record.get('dbm') != 'Tkrzw':
        fail('oracle DBM is not Tkrzw')
    def verify(key, path):
        if key not in record:
            fail(f'missing manifest field: {key}')
        if digest_file(path) != record[key]:
            fail(f'oracle checksum mismatch: {path}')
    for key, path in (('header_sha256', header), ('shared_object_sha256', library), ('data_manifest_sha256', stable)):
        verify(key, path)
    if not checksum_manifest(prefix, stable.name):
        fail('oracle data payload checksum mismatch')
    if 'data_unstable_manifest_sha256' in record:
        verify('data_unstable_manifest_sha256', unstable)
        if not checksum_manifest(prefix, unstable.name):
            fail('oracle unstable data payload checksum mismatch')
    build.mkdir(parents=True, exist_ok=True)
    output.mkdir(parents=True, exist_ok=True)
    for name in ('user-f-a', 'user-f-c', 'user-zh'):
        shutil.rmtree(build / name, ignore_errors=True)
        (build / name).mkdir()
    env = dict(os.environ, PKG_CONFIG_PATH=str(prefix / 'lib/pkgconfig'))
    cflags = words(text(['pkg-config', '--cflags', 'libpinyin'], env=env, check=False), expand=True)
    libs = words(text(['pkg-config', '--libs', 'libpinyin'], env=env, check=False), expand=True)
    driver = build / 'pinyin-capture'
    run(['cc', '-std=c11', '-Wall', '-Wextra', '-Werror', *cflags, here / 'capture.c', '-o', driver, *libs], env=env)
    env['LD_LIBRARY_PATH'] = bench.loader_path(prefix / 'lib', env.get('LD_LIBRARY_PATH'))
    for family, user, filename in (('F-A', 'user-f-a', 'f-a.txt'), ('F-C', 'user-f-c', 'f-c.txt'),
                                    ('ZH', 'user-zh', 'zhuyin-parse.txt')):
        command = [driver, family, data, build / user, record['pin_ref']]
        try:
            runtime.qualify('tkrzw', 'run-capture.sh', 'pin', driver, command, env=env)
        except runtime.RuntimeRefused as error:
            fail('REFUSED: ' + str(error))
        with (output / filename).open('wb') as stream:
            run(command, env=env, stdout=stream)
    return 0


def round_trip(argv, *, root=ROOT):
    """Preserve all four phases and the existing system-import interchange check."""
    if not argv or not argv[0]:
        fail('usage: user-dir-round-trip.sh <oracle-prefix> [input ...]', 1)
    prefix = Path(argv[0])
    inputs = list(argv[1:]) or ['nihao', 'nisha']
    data = prefix / 'lib/libpinyin/data'
    if not (prefix / 'oracle-pin.txt').is_file() or not (prefix / 'lib/libpinyin.so').is_file():
        fail(f'SKIP: missing oracle: {prefix}', 77)
    if not data.is_dir():
        fail(f'no data dir under prefix: {data}')
    headers = [path for path in (prefix / 'include').glob('libpinyin-*') if path.is_dir()]
    if not headers:
        fail(f'no libpinyin header dir under {prefix}/include')
    libraries = list((prefix / 'lib').glob('libpinyin.so.15*'))
    if not libraries:
        fail(f'no libpinyin.so.15 under {prefix}')
    so = libraries[0]
    with tempfile.TemporaryDirectory(prefix='oxpinyin-user-rt.') as scratch:
        work = Path(scratch)
        driver = work / 'user_driver'
        cflags = words(text(['pkg-config', '--cflags', 'glib-2.0'], check=False))
        libs = words(text(['pkg-config', '--libs', 'glib-2.0'], check=False))
        run(['cc', '-O2', '-o', driver, Path(root) / 'tools/oracle/user_driver.c', '-I' + str(headers[0]),
             *cflags, '-L' + str(prefix / 'lib'), '-lpinyin', *libs, '-Wl,-rpath,' + str(so.parent)])
        for name in ('pin', 'ox'):
            (work / name).mkdir()
        dbm = fields(prefix / 'oracle-pin.txt').get('dbm')
        cell = {'BerkeleyDB': 'bdb', 'KyotoCabinet': 'kc', 'Tkrzw': 'tkrzw'}.get(dbm)
        if cell is None:
            fail(f'fatal: {prefix}/oracle-pin.txt names no known dbm (BerkeleyDB, KyotoCabinet, Tkrzw)')
        settings = Settings.read(dict(os.environ, PINYIN_ORACLE_DBM=os.environ.get('PINYIN_ORACLE_DBM') or cell), Path(root))
        env = dict(settings.environment)
        feature_flags = words(env.get('OX_CARGO_FEATURES') or '--no-default-features --features ' + settings.feature)
        def pin(action, profile, *, output=None):
            command = [str(driver), action, str(data), str(work / profile), *(inputs if action == 'train' else [])]
            if output is None:
                qualified(command, settings.cell, 'pin', driver, env=env)
                return ''
            try:
                runtime.qualify(settings.cell, 'user-dir-round-trip.sh', 'pin', driver, command, env=env)
            except runtime.RuntimeRefused as error:
                fail('REFUSED: ' + str(error))
            done = capture(command, env=env)
            # POSIX sort, in the caller's locale, is part of the dump contract.
            with (work / 'unsorted').open('wb') as stream:
                stream.write(done.stdout)
            with (work / output).open('wb') as stream, (work / 'unsorted').open('rb') as source:
                run(['sort'], stdin=source, stdout=stream)
            return (work / output).read_text()
        def subject(name, additions):
            environment = dict(env, **additions)
            executable = env.get('OXPINYIN_USER_RT_TEST')
            if executable:
                qualified([executable, '--ignored', '--nocapture', name], settings.cell, 'subject', executable, env=environment)
            elif env.get('OXPINYIN_CAPI_SO'):
                fail('FAIL: a supplied library requires OXPINYIN_USER_RT_TEST (prebuilt user_dir_round_trip test)')
            else:
                cargo = ['cargo', 'test', '--locked', '--profile', 'dev', '--manifest-path', str(Path(root) / 'Cargo.toml'),
                         '-p', 'oxpinyin-runtime', *feature_flags, '--test', 'user_dir_round_trip']
                if settings.cell != 'tkrzw':
                    run([*cargo, '--', '--ignored', '--nocapture', name], env=environment, heavy=True)
                else:
                    with (work / 'cargo.json').open('wb') as output:
                        run([*cargo, '--no-run', '--message-format=json'], env=environment, heavy=True, stdout=output)
                    paths = [row['executable'] for line in (work / 'cargo.json').read_text().splitlines()
                             if (row := json.loads(line)).get('executable')]
                    if len(paths) != 1:
                        fail(f'expected one test executable: {paths}')
                    qualified([paths[0], '--ignored', '--nocapture', name], settings.cell, 'subject', paths[0], env=environment)
        print('== Phase A: the pin trains a profile ==')
        pin('train', 'pin')
        original = pin('dump', 'pin', output='pin.dump')
        if not original:
            fail('the pin dump is empty')
        if not any(line.startswith('B') for line in original.splitlines()):
            fail('the pin trained no bigram rows: the driver chose a candidate that installs no constraint')
        print('== Phase B: oxpinyin loads the pin profile and saves it back in place ==')
        subject('a_pin_profile', dict(OX_SYSTEM_DIR=str(data), OX_PIN_DIR=str(work / 'pin')))
        print('== Phase C: the pin renders the original and oxpinyin rewrite ==')
        pin('dump', 'pin', output='pin-reads-ox.dump')
        with (work / 'diff.txt').open('wb') as output:
            status = run(['diff', '-u', work / 'pin.dump', work / 'pin-reads-ox.dump'], stdout=output, check=False)
        if status:
            sys.stderr.buffer.write((work / 'diff.txt').read_bytes())
            fail("DIVERGED: oxpinyin's load->save changed what the pin renders")
        print(f"IDENTICAL: the pin renders oxpinyin's rewrite of its own profile byte-identically ({len(original.splitlines())} rows)")
        print('== Phase D: oxpinyin trains its own profile, the pin reads it ==')
        subject('oxpinyin_trains', dict(OX_SYSTEM_DIR=str(data), OX_OWNED_DIR=str(work / 'ox'),
                                      OX_INPUTS=' '.join(inputs), OX_EXPECTED_PHRASES=str(work / 'expected-phrases')))
        owned = pin('phrases', 'ox', output='pin-reads-ox-owned.dump')
        rows = len(owned.splitlines())
        phrases = sum(line.startswith('P') for line in owned.splitlines())
        if not rows or not phrases:
            fail(f'DIVERGED: the pin found nothing (or no phrases) in oxpinyin own profile (rows={rows} phrases={phrases})')
        (work / 'actual-phrases').write_text(''.join(line.split('\t')[1] + '\n' for line in owned.splitlines() if line.split('\t')[0] == 'P'))
        actual = text(['sort', work / 'actual-phrases'])
        expected = text(['sort', work / 'expected-phrases'])
        if actual != expected:
            print('DIVERGED: the pin rendered phrase set differs from what oxpinyin remembered', file=sys.stderr)
            (work / 'expected').write_text(expected + '\n')
            (work / 'actual').write_text(actual + '\n')
            run(['diff', work / 'expected', work / 'actual'], stdout=sys.stderr, check=False)
            raise Finished(1)
        print(f'READABLE: the pin loads and renders oxpinyin own profile ({rows} rows, {phrases} phrase rows, all expected)')
        capi = settings.artifact('OXPINYIN_CAPI_SO', 'libpinyin_capi.so', 'oxpinyin-capi')
        return run([*PYTHON, 'runner', 'system-import-round-trip', 'pinyin', so, capi, data], env=env)


def drop_in(argv):
    """Compile identical source inputs through both producers and compare the existing parity test."""
    if not argv or not argv[0]:
        fail('backend: kc or tkrzw', 1)
    if len(argv) < 2 or not argv[1]:
        fail('model dir', 1)
    backend, model = argv[0], Path(argv[1]).resolve()
    if not model.is_dir():
        fail(f'{argv[1]}: No such file or directory')
    if backend not in ('kc', 'tkrzw'):
        fail(f'unknown backend {backend} (kc or tkrzw)', 2)
    work = Path(argv[2] if len(argv) > 2 else '/tmp/drop-in-' + backend)
    prefix = Path('/opt/libpinyin-' + backend)
    features = [] if backend == 'kc' else ['--no-default-features', '--features', 'tkrzw']
    theirs, ours = work / 'libpinyin', work / 'oxpinyin'
    shutil.rmtree(work, ignore_errors=True)
    theirs.mkdir(parents=True)
    ours.mkdir()
    shutil.copyfile(prefix / 'lib/libpinyin/data/table.conf', theirs / 'table.conf')
    run([prefix / 'bin/gen_binary_files', '--gen-punct-table', '--table-dir', model], cwd=theirs)
    with (model / 'interpolation2.text').open('rb') as source:
        run([prefix / 'bin/import_interpolation', '--table-dir', model], cwd=theirs, stdin=source)
    run([prefix / 'bin/gen_unigram', '--table-dir', model], cwd=theirs)
    print(f'libpinyin ({backend}) wrote:')
    run(['ls', theirs])
    run(['cargo', 'run', '--locked', '-q', '-p', 'oxpinyin-datagen', *features, '--', 'compile',
         '--backend', 'kyotocabinet' if backend == 'kc' else backend, '--model-dir', model, '--out-dir', ours], heavy=True)
    print(f'oxpinyin ({backend}) wrote:')
    run(['ls', ours])
    run(['cargo', 'test', '--locked', '-p', 'oxpinyin-datagen', *features, '--test', 'libpinyin_parity', '--',
         '--include-ignored', '--nocapture'], heavy=True,
        env=dict(os.environ, OXPINYIN_LIBPINYIN_DATA_DIR=str(theirs), PINYIN_MODEL_DIR=str(model)))
    for chunk in sorted(theirs.glob('*.bin')):
        if chunk.name in ('pinyin_index.bin', 'phrase_index.bin', 'addon_pinyin_index.bin', 'addon_phrase_index.bin', 'punct.bin'):
            continue
        run(['cmp', chunk, ours / chunk.name])
        print(chunk.name + ': byte-exact')
    print(f'drop-in differential ({backend}, {model}): OK')
    return 0
