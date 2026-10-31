"""Existing oracle differential campaigns and their shared suite/environment wiring."""
import os
from pathlib import Path
import re
import shutil
import sys

from . import images
from .process import ROOT, PYTHON, fail, run, text

# Paths are the upstream build tree's, shared by developer and fixture campaigns.
TOOLS = {
    'PINYIN_SPSEG': 'utils/segment/spseg', 'PINYIN_MERGESEQ': 'utils/segment/mergeseq',
    'PINYIN_NGSEG': 'utils/segment/ngseg',
    **{f'PINYIN_{key}': f'utils/training/{value}' for key, value in (
        ('GEN_KMM', 'gen_k_mixture_model'), ('EXPORT_KMM', 'export_k_mixture_model'),
        ('MERGE_KMM', 'merge_k_mixture_model'), ('PRUNE_KMM', 'prune_k_mixture_model'),
        ('VALIDATE_KMM', 'validate_k_mixture_model'), ('KMM_TO_INTERP', 'k_mixture_model_to_interpolation'),
        ('GEN_UNIGRAM', 'gen_unigram'), ('GEN_NGRAM', 'gen_ngram'), ('GEN_DELETED_NGRAM', 'gen_deleted_ngram'),
        ('ESTIMATE_INTERPOLATION', 'estimate_interpolation'), ('EVAL_CORRECTION_RATE', 'eval_correction_rate'))},
    'PINYIN_GEN_BINARY_FILES': 'utils/storage/gen_binary_files',
    'PINYIN_EXPORT_INTERPOLATION': 'utils/storage/export_interpolation',
}
FEATURES = ['--no-default-features', '--features', 'tkrzw']
REPORT = re.compile(r'live parity|parity:|value-identical|skipping|test result|diverges|stale|panicked|assertion|left:|right:')


def cargo_test(package, target, filters=(), *, locked=False, quiet=False, features=True, nocapture=False):
    """The existing ignored-test grammar, with explicit matching backend features."""
    return ['cargo', 'test', *(['--locked'] if locked else []), *(['-q'] if quiet else []),
            '-p', package, *(FEATURES if features else []), '--test', target, '--', '--include-ignored',
            *(['--nocapture'] if nocapture else []), *filters]


def run_suite(title, command, *, env, report=False):
    """Run every selected suite, accumulating failures without masking an earlier one."""
    print(f'== {title} ==', flush=True)
    if report:
        # The shared heavy launcher takes admission. A temporary file preserves
        # shell command-substitution output without buffering a pipe under slot.
        import tempfile
        with tempfile.TemporaryDirectory(prefix='oracle-suite.') as scratch:
            output = Path(scratch) / 'out'
            # Only matching report lines are printed, just like the old report pipe.
            with output.open('wb') as stream:
                wrapper = [sys.executable, '-c',
                           'import os,sys; os.dup2(1,2); os.execvpe(sys.argv[1],sys.argv[1:],os.environ)', *map(str, command)]
                code = run(wrapper, env=env, heavy=True, stdout=stream, check=False)
            for line in output.read_text(errors='replace').splitlines():
                if REPORT.search(line):
                    print(line)
    else:
        code = run(command, env=env, heavy=True, check=False)
    if code:
        print(f'FAILED: {title}')
    return bool(code)


def differentials(argv):
    """Validate the complete developer input set before running any existing suite."""
    values = dict(libpinyin='', data='', export='', model='')
    rest = list(argv)
    while rest:
        option = rest.pop(0)
        if option in ('-h', '--help'):
            from .help import TEXT
            print(TEXT['differentials'], end='')
            return 0
        if option not in ('--libpinyin', '--data', '--export', '--model'):
            fail(f'unknown option: {option}', 2)
        if not rest:
            fail(f'{option}: requires an argument', 1)
        values[option[2:]] = rest.pop(0)
    for key, value in values.items():
        if not value:
            fail(f'missing --{key}', 2)
    missing = False
    def need(path, kind, reason):
        nonlocal missing
        valid = path.is_dir() if kind == 'directory' else os.access(path, os.X_OK) if kind == 'executable' else path.is_file()
        if not valid:
            print(f'missing {kind}: {path} ({reason})', file=sys.stderr)
            missing = True
    for key, reason in (('libpinyin', '--libpinyin: built libpinyin 2.11.92 tree'), ('data', '--data: built system data dir'),
                        ('export', '--export: oxpinyin tkrzw export'), ('model', '--model: extracted model20 dir')):
        need(Path(values[key]), 'directory', reason)
    if missing:
        return 2
    library, data, exported, model = (Path(value).resolve() for value in values.values())
    for path, reason in ((library / 'src/.libs/libpinyin.so.15', 'the built shared object (make)'),
                          (data / 'table.conf', 'system table.conf'), (data / 'phrase_index.bin', 'gen_binary_files output'),
                          (data / 'pinyin_index.bin', 'gen_binary_files output'), (exported / 'datagen-manifest.txt', 'oxpinyin-datagen compile')):
        need(path, 'file', reason)
    manifest = images.fields(exported / 'datagen-manifest.txt') if (exported / 'datagen-manifest.txt').is_file() else {}
    if manifest.get('backend') != 'tkt':
        fail(f'fatal: --export holds a {manifest.get("backend", "")} export; this runner\'s oracle is tkrzw. Rebuild the export with --features tkrzw (see the header recipe).', 2)
    need(model / 'interpolation2.text', 'file', 'model20 export')
    env = dict(os.environ, **{key: str(library / value) for key, value in TOOLS.items()})
    for key in TOOLS:
        need(Path(env[key]), 'executable', f'{key} (built by make in the libpinyin tree)')
    if missing:
        return 2
    from ..measure.bench import loader_path
    env.update(LD_LIBRARY_PATH=loader_path(library / 'src/.libs', env.get('LD_LIBRARY_PATH')),
               PINYIN_GEN_NGRAM_DATA=str(data), PINYIN_NGSEG_DATA=str(data), PINYIN_EXPORT_DIR=str(exported),
               PINYIN_MODEL_DIR=str(model), OXPINYIN_LIBPINYIN_DATA_DIR=str(data))
    evaluator = (data / 'bigram.db').is_file() and (data / 'evals2.text').is_file()
    if evaluator:
        for path, reason in ((data / 'interpolation2.text', 'the interpolation2.text bigram.db was imported from'),
                              (exported / 'pinyin_index.bin', 'oxpinyin-datagen output'), (exported / 'phrase_index.bin', 'oxpinyin-datagen output')):
            need(path, 'file', reason)
        if missing:
            return 2
        env.update(PINYIN_EVAL_DATA=str(data), PINYIN_EVAL_INTERPOLATION2=str(data / 'interpolation2.text'),
                   PINYIN_EVAL_TABLE_CONF=str(data / 'table.conf'), PINYIN_EVAL_PINYIN_INDEX=str(exported / 'pinyin_index.bin'),
                   PINYIN_EVAL_PHRASE_INDEX=str(exported / 'phrase_index.bin'))
    suites = [
        ('datagen: compiled tkrzw backend reads model20 back', 'oxpinyin-datagen', 'cross_backend'),
        ('runtime: unmodified tkrzw libpinyin data directory', 'oxpinyin-runtime', 'libpinyin_data_dir'),
        ('KMM differentials (gen+export, merge, prune, validate)', 'oxpinyin-kmm', 'differential'),
        ('KMM to interpolation: k_mixture_model_to_interpolation', 'oxpinyin-utils', 'k_mixture_model_to_interpolation'),
        ('segment: spseg / mergeseq', 'oxpinyin-training', 'segment-spseg-mergeseq'),
        ('segment: ngseg (needs the system bigram)', 'oxpinyin-training', 'segment-differential'),
        ('lambda: estimate_interpolation', 'oxpinyin-training', 'lambda-differential'),
        ('counter: gen_ngram', 'oxpinyin-training', 'counter-differential')]
    if evaluator:
        suites.append(('eval: eval_correction_rate', 'oxpinyin-training', 'eval-differential'))
    status = 0
    for title, package, target in suites:
        status |= run_suite(title, cargo_test(package, target, features=package != 'oxpinyin-kmm', nocapture=True), env=env, report=True)
    if not evaluator:
        print(f'== eval: eval_correction_rate ==\nskipping: needs {data}/bigram.db (import_interpolation) and {data}/evals2.text')
    print('RESULT: at least one differential suite FAILED' if status else 'RESULT: every differential suite passed')
    return int(status)


def nightly(argv, *, root=ROOT):
    """Build the pin over the committed toned model and run the existing three tiers."""
    work = Path(argv[0] if argv else '/tmp/oxpinyin-fixture-differentials')
    prefix = Path('/opt/libpinyin-tkrzw')
    recipe = Path(root) / 'tools/oracle/build-oracle.sh'
    pin = images.fields(recipe)
    version, sha, url = (pin.get(key) for key in ('LIBPINYIN_VERSION', 'LIBPINYIN_SHA', 'LIBPINYIN_GIT_URL'))
    if not all((version, sha, url)):
        fail(f'could not read the libpinyin pin from {recipe}', 2)
    print(f'pin: libpinyin {version} @ {sha}')
    model = Path(root) / 'fixtures/datagen-toned'
    if not (model / 'interpolation2.text').is_file():
        fail(f'missing {model}/interpolation2.text', 2)
    work.mkdir(parents=True, exist_ok=True)
    src = work / 'libpinyin'
    marker = src / '.pin-ok'
    if not marker.is_file() or marker.read_text().rstrip('\n') != sha:
        shutil.rmtree(src, ignore_errors=True)
        run(['git', 'init', '-q', src])
        run(['git', '-C', src, 'fetch', '--quiet', '--depth=1', url, sha])
        got = text(['git', '-C', src, 'rev-parse', 'FETCH_HEAD'])
        if got != sha:
            fail(f'pin mismatch: fetched {got}, wanted {sha}', 2)
        run(['git', '-C', src, 'checkout', '--quiet', '--detach', 'FETCH_HEAD'])
        marker.write_text(sha + '\n')
    for path in [*model.glob('*.table'), model / 'interpolation2.text']:
        shutil.copyfile(path, src / 'data' / path.name)
    if not os.access(src / 'configure', os.X_OK):
        run(['autoreconf', '--force', '--install'], cwd=src)
    if not (src / 'Makefile').is_file():
        run(['./configure', '--prefix=' + str(prefix), '--disable-static', '--with-dbm=Tkrzw'], cwd=src)
    jobs = text(['nproc'])
    if run(['make', '-j' + jobs], cwd=src, heavy=True, check=False) or run(['make', 'install'], cwd=src, heavy=True, check=False):
        print('::warning::build or install failed once; retrying on a fresh bigram.db')
        (src / 'data/bigram.db').unlink(missing_ok=True)
        run(['make', 'install'], cwd=src, heavy=True)
    data = prefix / 'lib/libpinyin/data'
    for name in ('table.conf', 'phrase_index.bin', 'pinyin_index.bin', 'bigram.db'):
        if not (data / name).is_file():
            fail(f'installed data dir is missing {name}', 2)
    from ..measure.bench import loader_path
    env = dict(os.environ, LD_LIBRARY_PATH=loader_path(prefix / 'lib', os.environ.get('LD_LIBRARY_PATH')))
    exported = work / 'export-tkrzw'
    if not (exported / 'datagen-manifest.txt').is_file():
        run(['cargo', 'run', '--locked', '-q', '-p', 'oxpinyin-datagen', *FEATURES, '--', 'compile',
             '--backend', 'tkrzw', '--model-dir', model, '--out-dir', exported], cwd=root, env=env, heavy=True)
    w3 = Path(root) / 'fixtures/w3'
    for directory in (exported, w3 / 'tkt'):
        record = images.fields(directory / 'datagen-manifest.txt')
        if record.get('backend') != 'tkt':
            fail(f'fatal: {directory} is not a tkrzw data set (manifest says {record.get("backend", "")}); this runner\'s oracle and tests are tkrzw', 2)
    env.update(PINYIN_NGSEG=str(src / TOOLS['PINYIN_NGSEG']), PINYIN_NGSEG_DATA=str(data),
               PINYIN_EXPORT_DIR=str(exported), PINYIN_MODEL_DIR=str(model))
    status = int(run_suite('segment ngseg live parity (toned tables)',
                           cargo_test('oxpinyin-training', 'segment-differential', ['rust_matches_live_ngseg'], locked=True, quiet=True), env=env))
    env.update(PINYIN_GEN_NGRAM_DATA=str(w3 / 'tkt'), PINYIN_EXPORT_DIR=str(w3 / 'tkt'),
               PINYIN_SPSEG=str(src / TOOLS['PINYIN_SPSEG']), PINYIN_NGSEG_DATA=str(w3 / 'tkt'))
    for key in ('GEN_KMM', 'EXPORT_KMM', 'MERGE_KMM', 'VALIDATE_KMM', 'KMM_TO_INTERP'):
        env['PINYIN_' + key] = str(src / TOOLS['PINYIN_' + key])
    for gate in ('gen_and_export', 'merge', 'validate'):
        status |= run_suite(f'KMM live: {gate} (w3 tables)',
                            cargo_test('oxpinyin-kmm', 'differential', ['rust_kmm_matches_pin_' + gate], locked=True, quiet=True, features=False), env=env)
    status |= run_suite('KMM live: to_interpolation (k_mixture_model_to_interpolation)',
                        cargo_test('oxpinyin-utils', 'k_mixture_model_to_interpolation', ['rust_kmm_matches_pin_to_interpolation'],
                                   locked=True, quiet=True), env=env)
    status |= run_suite('segment spseg live parity (w3 tables)',
                        cargo_test('oxpinyin-training', 'segment-spseg-mergeseq', ['rust_matches_live_spseg'], locked=True, quiet=True), env=env)
    for key in ('PINYIN_EXPORT_DIR', 'PINYIN_NGSEG_DATA', 'PINYIN_GEN_NGRAM_DATA'):
        env.pop(key, None)
    status |= run_suite('drop-in differential (tkrzw, toned model)',
                        [*PYTHON, 'compat', 'drop-in', 'tkrzw', model, work / 'drop-in'], env=env)
    print('RESULT: at least one differential suite FAILED' if status else 'fixture-tier differentials: OK')
    return int(status)


def docker_round_trip(argv, *, root=ROOT):
    """Launch the historical provisioned-image interchange workflow under admission."""
    if not argv or not argv[0]:
        fail('Supply a provisioned round-trip image; see docs/runbooks/setup.md. This is not ordinary setup.', 1)
    root = Path(root)
    if not (root / 'target/model20/extracted').is_dir():
        fail('model20 cache missing; run: just fetch-model')
    mirror = root / 'target/mirrors/libpinyin.git'
    if not mirror.is_dir():
        print('libpinyin mirror missing; creating from a clone of the pin', file=sys.stderr)
        mirror.parent.mkdir(parents=True, exist_ok=True)
        run(['git', 'clone', 'https://github.com/libpinyin/libpinyin', mirror])
    return run(['docker', 'run', '--rm', '--network', 'host', '-v', str(root) + ':/work',
                '-v', str(Path.home() / '.cargo/registry') + ':/root/.cargo/registry', '-w', '/work',
                '-e', 'CARGO_TARGET_DIR=/tmp/target', '-e', 'HOME=/root', argv[0],
                'bash', 'tools/oracle/round-trip-bootstrap.sh'], heavy=True)


def docker_inside(argv):
    """Build from the local pin mirror and aggregate the same guest log and exit lines."""
    log = Path('/work/target/rt-oracle.log')
    # Bootstrap already wrote the apt preamble here; the rest uses managed Python.
    def body():
        print('== oracle build (file:// mirror; ibus skipped: github) ==', flush=True)
        source = Path('tools/oracle/build-oracle.sh').read_text()
        source = source.replace('LIBPINYIN_GIT_URL=https://github.com/libpinyin/libpinyin.git',
                                'LIBPINYIN_GIT_URL=file:///work/target/mirrors/libpinyin.git')
        source = source.replace('ibus_src=$(fetch_commit ibus-libpinyin "$IBUS_LIBPINYIN_GIT_URL" "$IBUS_LIBPINYIN_SHA")',
                                'ibus_src=ibus-skipped-github-unreachable')
        source = re.sub(r'\(\n\tcd "\\\$ibus_src"\n.*?\n\)\n',
                        'echo "ibus build skipped: github unreachable this run"\n', source, count=1, flags=re.S)
        Path('/tmp/bo.sh').write_text(source)
        code = run(['bash', '/tmp/bo.sh', '--dbm', 'tkrzw', '--work-dir', '/tmp/oracle-work', '--prefix', '/tmp/oracle-prefix',
                    '--model-dir', '/work/target/model20/extracted', '--jobs', text(['nproc'])], heavy=True, check=False)
        print(f'ORACLE-BUILD-EXIT:{code}')
        if code:
            return 1
        print('== round trip ==')
        code = run([*PYTHON, 'compat', 'user-round-trip', '/tmp/oracle-prefix', 'nihao', 'nisha', 'nihaoa'], check=False)
        print(f'ROUNDTRIP-EXIT:{code}')
        return int(bool(code))
    sys.stdout.flush()
    sys.stderr.flush()
    saved = os.dup(1), os.dup(2)
    try:
        with log.open('ab') as output:
            os.dup2(output.fileno(), 1)
            os.dup2(output.fileno(), 2)
            status = body()
            sys.stdout.flush()
            sys.stderr.flush()
    finally:
        os.dup2(saved[0], 1)
        os.dup2(saved[1], 2)
        os.close(saved[0])
        os.close(saved[1])
    print(f'CONTAINER-EXIT:{status}')
    run(['tail', '-5', log])
    return status
