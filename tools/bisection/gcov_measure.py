#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal
"""Private -O0 gcov measurement; counters are build intermediates, JSON is not saved.

The installed oracle remains the differential reference. Measurement execution
is credited only after the original differential matches and the instrumented
library produces the same reference observation. The private SIGABRT handler
dumps counters and re-raises. Missing flush completion or a timeout remains a
measurement limit, never a non-execution claim.
"""

import argparse
from collections import defaultdict
import ctypes
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import resource
import shlex
import subprocess
import sys
import tempfile
import pickle
import socket
import re
import runpy

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
check_tkrzw = runpy.run_path(str(REPO / "tools/tkrzw/runtime.py"))["check"]
PIN = "074a2219c90feaf962d0d24f034514033ece5f99"
WORKER_TIMEOUT = 60
# The pin's three DBM cells (configure.ac:93-132 at the pin): the Makefile.am
# conditional naming the cell's translation units, the config.h macro its
# configure output defines, the configure/manifest spellings, the library
# AC_SEARCH_LIBS adds to LIBS, the subject's cargo feature and the suffix of
# the cell-specific headers the shared headers include under the macro.
CELLS = {
    'bdb': dict(conditional='BERKELEYDB', macro='HAVE_BERKELEY_DB', dbm='BerkeleyDB',
                configure='--with-dbm=BerkeleyDB', library='db', feature='bdb', suffix='bdb'),
    'kc': dict(conditional='KYOTOCABINET', macro='HAVE_KYOTO_CABINET', dbm='KyotoCabinet',
               configure='--with-dbm=KyotoCabinet', library='kyotocabinet', feature='kyotocabinet', suffix='kyotodb'),
    'tkrzw': dict(conditional='TKRZW', macro='HAVE_TKRZW', dbm='Tkrzw',
                  configure='--with-dbm=Tkrzw', library='tkrzw', feature='tkrzw', suffix='tkrzwdb'),
}
LEDGERS = {cell: f'docs/findings/axis-j-{cell}-ledger.md' for cell in CELLS}
GATE_TESTS = (
    'sentence_surface_matches_the_declared_residual',
    'sentence_surface_detects_per_step_cost_shift',
    'sentence_tie_fixture_uses_only_the_declared_residuals',
    'sentence_surface_matches_supplementary_tie_order',
)

FLUSH_SHIM = r'''#include <signal.h>
#include <unistd.h>
extern "C" void __gcov_dump();
extern "C" void oxpinyin_j_gcov_dump() { __gcov_dump(); }
static void flush_abort(int signal) {
    const char begin[] = "J-GCOV-ABORT-BEGIN\n";
    const char end[] = "J-GCOV-ABORT-END\n";
    (void)write(STDOUT_FILENO, begin, sizeof(begin) - 1);
    __gcov_dump();
    (void)write(STDOUT_FILENO, end, sizeof(end) - 1);
    // SA_RESETHAND restores SIG_DFL before entry; SA_NODEFER leaves this
    // signal unblocked, so raise retains subprocess's -SIGABRT outcome.
    raise(signal);
    _exit(128 + signal); // Only a failed re-raise reaches this fallback.
}
__attribute__((constructor)) static void install_abort_flush() {
    struct sigaction action = {};
    action.sa_handler = flush_abort;
    sigemptyset(&action.sa_mask);
    action.sa_flags = SA_RESETHAND | SA_NODEFER;
    sigaction(SIGABRT, &action, nullptr);
}
'''


def command(argv, **kwargs):
    result = subprocess.run([str(x) for x in argv], capture_output=True, **kwargs)
    if result.returncode:
        raise RuntimeError(f"{argv[0]} failed ({result.returncode}): " + result.stderr.decode(errors="replace")[-4000:])
    return result.stdout.decode()


def contract_module(cell='bdb'):
    # The driver snapshots CELL at import and uses it in fixture registration:
    # the requested cell is set explicitly here, never inherited from the
    # environment, so a stray CONTRACT_DIFF_CELL cannot select another cell.
    if cell not in CELLS:
        raise RuntimeError('unknown DBM cell: ' + repr(cell))
    os.environ['CONTRACT_DIFF_CELL'] = cell
    spec = importlib.util.spec_from_file_location("contract", HERE / "contract-diff.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    if module.CELL != cell:
        raise RuntimeError(f'contract driver did not select the {cell} cell')
    return module


def cell_sources(pin, cell='bdb'):
    """Read the autoconf source list, selecting only one cell's DBM conditional.

    The shared list precedes the first conditional; each cell's block is the
    text between its `if <CONDITIONAL>` and the next `endif` (src/Makefile.am
    at the pin). Another cell's translation units are absent by this recipe.
    """
    text = (pin / "src/Makefile.am").read_text()
    shared = text.split("pinyin_SOURCES =", 1)[1].split("if BERKELEYDB", 1)[0]
    conditional = text.split("if " + CELLS[cell]['conditional'], 1)[1].split("endif", 1)[0]
    return sorted(set(re.findall(r"\b[\w/]+\.cpp", shared + conditional)))


def configured_libraries(config_status, cell='bdb'):
    """The DBM library AC_SEARCH_LIBS appended to LIBS, read from config.status.

    The pin links each cell's shared object with the configured LIBS value
    (libpinyin_la_LIBADD carries only GLib); the measurement build repeats
    that exact link input instead of spelling a library name per cell.
    """
    match = re.search(r'^S\["LIBS"\]="([^"]*)"', config_status.read_text(), re.M)
    if not match:
        raise RuntimeError('config.status does not record the configured LIBS')
    libraries = shlex.split(match.group(1))
    if '-l' + CELLS[cell]['library'] not in libraries:
        raise RuntimeError(f'configured LIBS {libraries!r} do not name the {cell} DBM library')
    return libraries


def recorded_prefix_hashes(cell='bdb'):
    """Legacy read-only manifests use the authorized ledger for the Z digest.

    This is a recorded input, not a digest accepted from the current library.
    A newly provisioned prefix may instead carry zhuyin_shared_object_sha256.
    Each cell's ledger records its own oracle digests in the same sentence.
    """
    text = (REPO / LEDGERS[cell]).read_text()
    pattern = r'Oracle shared-object SHA-256: libpinyin=`([0-9a-f]{64})`; libzhuyin=`([0-9a-f]{64})`'
    matches = re.findall(pattern, text)
    if len(matches) != 1 or f'Pin `{PIN}`' not in text:
        raise RuntimeError('record both oracle binary digests in the ledger provenance before measurement')
    return dict(zip(('libpinyin', 'libzhuyin'), matches[0], strict=True))


def oracle_hashes(prefix, provenance=None, cell='bdb'):
    fields = dict(line.split('=', 1) for line in (prefix / 'oracle-pin.txt').read_text().splitlines()
                  if '=' in line and not line.startswith('#'))
    recorded = provenance if provenance is not None else recorded_prefix_hashes(cell)
    expected = {'libpinyin': fields['shared_object_sha256'],
                'libzhuyin': fields.get('zhuyin_shared_object_sha256', recorded['libzhuyin'])}
    if recorded != expected:
        raise RuntimeError('oracle manifest and recorded binary provenance disagree')
    actual = {name: hashlib.sha256((prefix / f'lib/{name}.so').read_bytes()).hexdigest()
              for name in expected}
    for name in expected:
        if actual[name] != expected[name]:
            raise RuntimeError(name + ' oracle SHA-256 differs from recorded prefix provenance')
    return actual


def verify_prefix(prefix, provenance=None, cell='bdb'):
    """Read-only pin, cell, library and full data-manifest hash cross-check."""
    fields = lambda path: dict(line.split('=', 1) for line in path.read_text().splitlines() if '=' in line and not line.startswith('#'))
    actual = fields(prefix / 'oracle-pin.txt')
    expected = fields(REPO / 'tools/oracle/oracle-pin.txt')
    ref = f"libpinyin-{expected['libpinyin_tag']}-{PIN}+model20-{expected['model_archive_sha256']}+dbm-{cell}"
    if actual['pin_ref'] != ref or actual['dbm'] != CELLS[cell]['dbm']:
        raise RuntimeError(f'oracle prefix requires pin_ref {ref!r} and DBM {CELLS[cell]["dbm"]!r}; '
                           f'found {actual["pin_ref"]!r} and {actual["dbm"]!r}')
    oracle_hashes(prefix, provenance, cell)
    count = 0
    for name, field in [('oracle-data.sha256', 'data_manifest_sha256'), ('oracle-data-unstable.sha256', 'data_unstable_manifest_sha256')]:
        raw = (prefix / name).read_bytes()
        if hashlib.sha256(raw).hexdigest() != actual[field]:
            raise RuntimeError(f'oracle data manifest SHA-256 differs from recorded provenance: {name}')
        for line in raw.decode().splitlines():
            digest, path = line.split('  ', 1)
            if hashlib.sha256((prefix / path).read_bytes()).hexdigest() != digest:
                raise RuntimeError(f'oracle data SHA-256 differs from manifest: {path}')
            count += 1
    return count


def build(pin, config, out, oracle, cell='bdb', provenance=None):
    """Reproduce the configured autoconf shared-library recipe without log files.

    config.h is the unchanged configure output of the selected cell. Only
    CXXFLAGS is replaced by -g -O0 --coverage. Makefile.am supplies the
    conditional TU lists; the installed config.status guards the other
    original configure flags and supplies the configured DBM link input.
    """
    revision = command(["git", "-C", pin, "rev-parse", "HEAD"]).strip()
    if revision != PIN:
        raise RuntimeError(f'measurement requires pin checkout {PIN}; found {revision}')
    if command(["git", "-C", pin, "status", "--porcelain", "--untracked-files=no"]).strip():
        raise RuntimeError('measurement requires a pin checkout with no tracked changes')
    verify_prefix(oracle, provenance, cell)
    configuration = command([config.parent / "config.status", "--config"]).strip()
    required = ["--disable-static", CELLS[cell]['configure'], "--enable-libzhuyin"]
    if not all(flag in shlex.split(configuration) for flag in required):
        raise RuntimeError(f'measurement requires configure flags {required!r}; found {configuration!r}')
    if "#define " + CELLS[cell]['macro'] not in config.read_text():
        raise RuntimeError(f'measurement config.h requires {CELLS[cell]["macro"]}')
    for other in CELLS.values():
        if other['macro'] != CELLS[cell]['macro'] and "#define " + other['macro'] in config.read_text():
            raise RuntimeError(f'measurement config.h for {cell} must not define {other["macro"]}')
    libraries = configured_libraries(config.parent / "config.status", cell)
    out.mkdir(parents=True, exist_ok=True)
    if out.is_relative_to(oracle) or out.is_relative_to(pin):
        raise RuntimeError('measurement output must be lane-owned outside the pin/oracle')
    clear(out)  # old stamps must not pollute a newly compiled measurement
    (out / "config.h").write_bytes(config.read_bytes())
    common = cell_sources(pin, cell)
    cflags = shlex.split(command(["pkg-config", "--cflags", "glib-2.0"]))
    flags = ["-g", "-O0", "--coverage", "-fPIC", "-DPIC", "-DHAVE_CONFIG_H", "-I" + str(out)]
    flags += ["-I" + str(pin / "src" / part) for part in ["", "include", "storage", "lookup"]] + cflags
    objects = {}
    for tu in common + ["pinyin.cpp", "zhuyin.cpp"]:
        obj = out / (tu.replace("/", "-") + ".o")
        print("gcov compile " + tu, file=sys.stderr, flush=True)
        command(["g++", *flags, "-c", pin / "src" / tu, "-o", obj])
        objects[tu] = obj
    # Export only this measurement shim in addition to the unchanged C exports.
    # It dumps libgcov counters before the harness's intentional os._exit,
    # without running library or interpreter teardown.
    shim = out / "gcov-flush.cpp"
    shim.write_text(FLUSH_SHIM)
    command(["g++", "-fPIC", "-c", shim, "-o", out / "gcov-flush.o"])
    for library in ["pinyin", "zhuyin"]:
        version = out / f"lib{library}.ver"
        version.write_text((pin / f"src/lib{library}.ver").read_text().replace("local:", "oxpinyin_j_gcov_dump;\nlocal:"))
        command(["g++", "--coverage", "-shared", *[objects[x] for x in common], objects[library + ".cpp"], out / "gcov-flush.o", *libraries, *shlex.split(command(["pkg-config", "--libs", "glib-2.0"])), "-Wl,--version-script=" + str(version), "-Wl,-soname,lib" + library + ".so.15", "-o", out / f"lib{library}.so"])
        original = command(["nm", "-D", "--defined-only", oracle / f"lib/lib{library}.so"])
        measured = command(["nm", "-D", "--defined-only", out / f"lib{library}.so"])
        symbols = lambda text: {line.split()[-1].split("@", 1)[0] for line in text.splitlines() if len(line.split()) >= 3 and line.split()[1] in ["T", "W"]}
        assert symbols(measured) == symbols(original) | {"oxpinyin_j_gcov_dump"}
    return configuration


def clear(out):
    for path in out.glob("*.gcda"):
        path.unlink()


def counters(out, pin):
    """Return exact GCC symbol/source extents; never save a gcov report."""
    counts = {}
    decoder = json.JSONDecoder()
    paths = sorted(out.glob("*.gcda"))
    if not paths:
        return counts
    raw = command(["gcov", "--json-format", "--stdout", *paths])
    position = 0
    while position < len(raw):
        start = raw.find('{"', position)
        if start < 0:
            break
        document, end = decoder.raw_decode(raw[start:])
        position = start + end
        version = document.get("format_version")
        if version != "2":
            gcov_version = command(["gcov", "--version"]).splitlines()[0]
            raise RuntimeError('gcov 14 or later is required (JSON format_version 2); '
                               f'found format_version {version!r}; gcov --version: {gcov_version}')
        tu = Path(document["data_file"]).name.removesuffix(".gcda")
        for source in document["files"]:
            try:
                relative = str(Path(source["file"]).resolve().relative_to(pin))
            except ValueError:
                continue
            for function in source.get("functions", []):
                identity = (function["name"], relative, function["start_line"], function["end_line"], tu)
                counts[identity] = max(counts.get(identity, 0), function["execution_count"])

    return counts


def measured_worker(module, mode, so, data, name, scratch, cell='bdb'):
    """Keep the existing observation parser and isolated-worker semantics."""
    # Start this file's tracing `--worker`, not the driver's ordinary entry.
    # The measurement worker selects its fixture cell from this explicit
    # variable, not from an inherited default.
    return bounded_worker(module, mode, so, data, name, scratch, extra_env={'J_MEASUREMENT_CELL': cell},
                          entry=(Path(__file__).resolve(), '--worker'))


def bounded_worker(module, mode, so, data, name, scratch, **options):
    try:
        return module.run_worker(mode, so, data, name, scratch, timeout=WORKER_TIMEOUT, **options)
    except subprocess.TimeoutExpired:
        return dict(exit='TIMEOUT', result=None, userfiles=[], measurement_roots=[],
                    measurement_abort_flush=False, stderr='', stderr_lines=0)


def worker(mode, so, data, name):
    module = contract_module(os.environ['J_MEASUREMENT_CELL'])
    check_tkrzw("auto", "gcov_measure.py", "subject", so)
    library = ctypes.CDLL(so)
    original_fn = module.Kit.fn
    def traced_fn(kit, function, result, *args):
        native = original_fn(kit, function, result, *args)
        def call(*values):
            print('J-ROOT ' + kit.mode + '_' + function, flush=True)
            return native(*values)
        return call
    module.Kit.fn = traced_fn
    dump = getattr(library, 'oxpinyin_j_gcov_dump', None)
    if dump:
        dump.argtypes, dump.restype = [], None
        original_exit = os._exit
        def exit_with_counters(status):
            dump()
            original_exit(status)
        os._exit = exit_with_counters
    module.worker(mode, so, data, name)


def matches(module, spec, pin, subject):
    """Existing contract verdict, including its exact registered signal rules."""
    return module.matches(spec, pin, subject)


def same_reference(module, spec, pin, measurement):
    if pin['exit'] == 'TIMEOUT' or measurement['exit'] == 'TIMEOUT': return False
    as_abort = spec['abort'] is not module.NO_ABORT and (not spec['either'] or pin['exit'] == -6)
    # The existing abort matcher compares signal and requested profile state,
    # not assert's source pathname. Mixed abort/crash cases set stderr=True
    # for the crash arm only (contract-diff.py's matches predicate).
    compare_stderr = spec['stderr'] and not as_abort
    return pin['exit'] == measurement['exit'] and (pin['result'] is None and measurement['result'] is None or pin['result'] is not None and measurement['result'] is not None and module.observed(pin['result']) == module.observed(measurement['result'])) and (not compare_stderr or pin['stderr'] == measurement['stderr']) and (not spec['userdir'] or pin['userfiles'] == measurement['userfiles'])


def contracts(prefix, pinyin, zhuyin, out, pin, progress=None, names=None, cell='bdb', provenance=None):
    verify_prefix(prefix, provenance, cell)
    module = contract_module(cell)
    assert module.CELL == cell
    attributed, nonmatch, incomplete, results = defaultdict(set), set(), set(), []
    data = prefix / 'lib/libpinyin/data'
    with tempfile.TemporaryDirectory(prefix='j-contract-') as scratch:
        # Match the shared driver's ordinary default. Axis C is an explicit
        # separately owned battery, not an implicit addition to this sweep.
        selected = sorted(name for name in module.CASES if not name.startswith('axis-c-')) if names is None else sorted(names)
        for index, name in enumerate(selected, 1):
            spec = module.CASES[name]
            if cell not in spec['cells']:
                results.append((name, 'SKIP'))
                continue
            mode = spec['mode']
            reference = bounded_worker(module, mode, (prefix / f'lib/lib{mode}.so').resolve(), data, name, scratch)
            subject = bounded_worker(module, mode, pinyin if mode == 'pinyin' else zhuyin, data, name, scratch)
            clear(out)
            measurement = measured_worker(module, mode, out / f'lib{mode}.so', data, name, scratch, cell)
            timed_out = any(record['exit'] == 'TIMEOUT' for record in (reference, subject, measurement))
            differential_match = not timed_out and matches(module, spec, reference, subject)
            measurement_match = same_reference(module, spec, reference, measurement)
            verdict = 'MATCH' if differential_match and measurement_match else 'DIFFER' if not differential_match else 'MEASUREMENT-DIFFER'
            if timed_out:
                verdict = 'TIMEOUT'
                incomplete.add('contract/' + name)
            try:
                counts = counters(out, pin)
            except RuntimeError:
                if timed_out:
                    counts = {}
                    measurement['counter_limit'] = True
                    incomplete.add('contract/' + name)
                    print('TIMEOUT_COUNTER_LIMIT ' + name + ': no execution credit', file=sys.stderr, flush=True)
                else:
                    counts = checksum_limit(out, name, measurement, incomplete)
            if not counts and measurement.get('measurement_abort_flush'):
                measurement['counter_limit'] = True
                incomplete.add('contract/' + name)
            if measurement['exit'] == -6 and not measurement.get('measurement_abort_flush'):
                counts = {}  # A failed/recursive flush is not execution evidence.
            if progress:
                progress(name, reference, subject, measurement, counts, verdict)
            for identity, count in counts.items():
                if count:
                    if verdict == 'MATCH': attributed[identity].add('contract/' + name)
                    else: nonmatch.add(identity)
            if measurement['exit'] != 0 and not (
                    measurement['exit'] == -6 and measurement.get('measurement_abort_flush')):
                incomplete.add('contract/' + name)
            results.append((name, verdict))
            if index % 25 == 0 or verdict != 'MATCH':
                print(f'contract {index}/{len(selected)} {name}: {verdict}; measured exit={measurement["exit"]}; abort flush={measurement.get("measurement_abort_flush", False)}', file=sys.stderr, flush=True)
    return attributed, nonmatch, incomplete, results


def checksum_limit(out, name, measurement, incomplete):
    if name not in {'abort-save-chunk-header-checksum', 'abort-zhuyin-save-chunk-header-checksum'}:
        raise  # unexpected decoder failures must remain visible
    truncated = list(out.glob('*.gcda'))
    if not truncated or any(path.stat().st_size > 4 for path in truncated):
        raise  # Only the actual four-byte resource-limit footprint is accepted.
    # A test may hard-limit file size (the checksum-save cases
    # use four bytes). __gcov_dump can return after truncated
    # writes: its END marker alone is not valid counter evidence.
    measurement['counter_limit'] = True
    incomplete.add('contract/' + name)
    print(f'COUNTER_DECODE_LIMIT {name}: no execution credit; test resource limits retained',
          file=sys.stderr, flush=True)
    return {}


def reconcile_interrupted(results, names, prefix, pinyin, zhuyin, out, pin, cell='bdb', provenance=None):
    """Resume only named interrupted cases; all completed MATCH inputs survive.

    The private orchestrator supplies identities read from its paused process
    namespace. Normal regeneration has no interruption receipt. Aggregated
    nonmatching counters cannot be separated per case, so any other nonmatch
    is replayed too before replacing that aggregate. No saved result is read.
    """
    behavior, nonmatch, incomplete, verdicts = results
    interrupted = set(names)
    known = dict(verdicts)
    if not interrupted or not interrupted <= known.keys():
        raise RuntimeError('interrupted case identities absent from the completed sweep')
    replay = interrupted | {name for name, verdict in verdicts if verdict not in {'MATCH', 'SKIP'}}
    labels = {'contract/' + name for name in replay}
    for identity in list(behavior):
        behavior[identity].difference_update(labels)
        if not behavior[identity]: del behavior[identity]
    incomplete.difference_update(labels)
    fresh, failed, limited, observations = contracts(prefix, pinyin, zhuyin, out, pin, names=replay, cell=cell, provenance=provenance)
    for identity, credited in fresh.items(): behavior[identity].update(credited)
    incomplete.update(limited)
    replacements = dict(observations)
    verdicts = [(name, replacements.get(name, verdict)) for name, verdict in verdicts]
    print('Interrupted-case reconciliation: ' + repr(observations) + '; completed MATCH cases retained in memory', flush=True)
    return behavior, failed, incomplete, verdicts


def limit_roots(prefix, out, incomplete, verdicts, pin, cell='bdb'):
    """Trace only root calls for fatal/discrepant cases, through stdout pipes.

    The trace wraps existing ctypes calls without changing their arguments,
    returns, signal handling or teardown. The original oracle is traced too:
    a discrepant measurement could stop before the reference's later calls.
    The conservative static descendants of these roots remain unknown when
    they lack other evidence, rather than being declared unexecuted.
    """
    module = contract_module(cell)
    data = prefix / 'lib/libpinyin/data'
    names = {name for name, verdict in verdicts if verdict != 'MATCH' and verdict != 'SKIP'}
    names.update(label.removeprefix('contract/') for label in incomplete if label.startswith('contract/'))
    roots, cases, corrections, recovered = set(), {}, {}, defaultdict(set)
    previous = dict(verdicts)
    with tempfile.TemporaryDirectory(prefix='j-limit-roots-') as scratch:
        for name in sorted(names):
            spec = module.CASES[name]
            observed = set()
            measurements = []
            for so in [(prefix / f'lib/lib{spec["mode"]}.so').resolve(), out / f'lib{spec["mode"]}.so']:
                if so.parent == out: clear(out)
                record = measured_worker(module, spec['mode'], so, data, name, scratch, cell)
                observed.update(record['measurement_roots'])
                measurements.append(record)
            if previous[name] == 'MEASUREMENT-DIFFER' and same_reference(module, spec, *measurements):
                # Original oracle/Rust MATCH was already recorded. Recheck the
                # exact reference/measurement fields after fixing the abort-arm
                # observer; no mutant or frozen expectation changes here.
                corrections[name] = 'MATCH'
                for identity, count in counters(out, pin).items():
                    if count: recovered[identity].add('contract/' + name)
            roots.update(observed); cases[name] = observed
    print(f'unknown measurement provenance: {len(cases)} cases, {len(roots)} actually invoked export roots', file=sys.stderr, flush=True)
    return roots, cases, corrections, recovered


def same_data(prefix, pinyin, out, pin, cell='bdb', provenance=None):
    """The exact seven preregistered drivers; fresh writable state per side."""
    verify_prefix(prefix, provenance, cell)
    names = ['key-surface-diff', 'dict-surface-diff', 'phrase-surface-diff',
             'pred-order-diff', 'predict-diff', 'punct-diff', 'import-diff']
    attributed, nonmatch, incomplete, results = defaultdict(set), set(), set(), []
    data = prefix / 'lib/libpinyin/data'
    with tempfile.TemporaryDirectory(prefix='j-same-data-') as scratch:
        work = Path(scratch)
        for name in names:
            exe = work / name
            command(['gcc', '-std=gnu11', '-Wall', '-Wextra', '-O2', '-o', exe, HERE / (name + '.c'), '-ldl', *shlex.split(command(['pkg-config', '--cflags', '--libs', 'glib-2.0']))])
            observations = []
            clear(out)
            for side, so in [('reference', prefix / 'lib/libpinyin.so'), ('subject', pinyin), ('measurement', out / 'libpinyin.so')]:
                cwd = work / (name + '-' + side)
                cwd.mkdir()
                env = dict(os.environ, LD_LIBRARY_PATH=str(so.parent) + (":" + os.environ["LD_LIBRARY_PATH"] if os.environ.get("LD_LIBRARY_PATH") else ""))
                check_tkrzw(cell, "gcov_measure.py/same_data", side, so, env=env, cwd=cwd)
                resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
                run = subprocess.run([str(exe), str(so), str(data), *(['matched-tables'] if name == 'punct-diff' else [])], cwd=cwd, env=env, capture_output=True)
                observations.append((run.returncode, run.stdout))
            reference, subject, measurement = observations
            verdict = 'MATCH' if reference[0] == subject[0] == measurement[0] == 0 and reference == subject == measurement else 'DIFFER'
            for identity, count in counters(out, pin).items():
                if count:
                    if verdict == 'MATCH': attributed[identity].add('same-data/' + name)
                    else: nonmatch.add(identity)
            if measurement[0]: incomplete.add('same-data/' + name)
            results.append((name, verdict))
            print(f'same-data {name}: {verdict}; exits={[o[0] for o in observations]}', file=sys.stderr, flush=True)
    return attributed, nonmatch, incomplete, results


def declared_residuals(root=REPO):
    """The six §12 shared-comparison residual inputs, read from the same
    membership source and with the same anchors as the Rust gate
    (`declared_sentence_residuals` in
    crates/pinyin-oracle/tests/sentence_surface_parity.rs); no second list."""
    doc = (root / 'docs/findings/sentence-surface.md').read_text()
    anchors = ('\n## 12. ', 'Only the following six shared-comparison rows are class (a), as ruled.',
               '| Input | Pin deciding f32 bits | Subject native costs |\n')
    if any(anchor not in doc for anchor in anchors):
        raise RuntimeError('the frozen §12 shared-comparison residual declaration moved')
    section = doc.split(anchors[0], 1)[1].split('\n## 13.', 1)[0]
    table = section.split(anchors[1], 1)[1].split(anchors[2], 1)[1].splitlines()
    if not table or not table[0].startswith('|---'):
        raise RuntimeError('the declared residual table lost its separator')
    names = set()
    for line in table[1:]:
        if not line.startswith('|'):
            break
        columns = line.strip('|').split('|')
        name = columns[0].strip().strip('`')
        if len(columns) != 3 or not name or name in names:
            raise RuntimeError('residual declaration format changed: ' + line)
        names.add(name)
    if len(names) != 6:
        raise RuntimeError('declared residual membership count moved')
    return frozenset(names)


def surface_worker(so, data, group, cell='bdb'):
    """Replay the existing C-ABI sentence call order, output through stdout only."""
    module = contract_module(cell)
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    check_tkrzw("auto", "gcov_measure.py", "subject", so)
    library = ctypes.CDLL(so)
    dump = getattr(library, 'oxpinyin_j_gcov_dump', None)
    if dump:
        dump.argtypes, dump.restype = [], None
        original_exit = os._exit
        def exit_with_counters(status):
            dump()
            original_exit(status)
        os._exit = exit_with_counters
    if group == 'sentence':
        inputs = [line.split('\t')[0] for line in (REPO / 'fixtures/w4/oracle-sentence-surface.txt').read_text().splitlines() if line and not line.startswith('#')]
    elif group == 'ties':
        inputs = [line.split('\t')[0] for line in (REPO / 'fixtures/w4/oracle-sentence-tie-order.txt').read_text().splitlines() if line and not line.startswith('#')]
    elif group == 'corpus':
        inputs = [line for path in sorted((REPO / 'tests/parity/corpus/inputs').glob('*.txt')) for line in path.read_text().splitlines()]
    else:
        raise ValueError(group)
    selected = json.loads(sys.stdin.read()) if os.environ.get('J_MATCH_INPUTS') == 'stdin' else None
    if selected is not None:
        selected = set(selected)
        inputs = [text for text in inputs if text in selected]
    if not inputs:
        os._exit(0)  # no phantom initialization credit for an empty MATCH set
    # A single context is the existing corpus walk; reset instance per input.
    with tempfile.TemporaryDirectory(prefix='j-surface-') as scratch:
        k = module.Kit('pinyin', so, data, scratch)
        assert k.fn('set_options', module.B, module.P, module.U)(k.ctx, 0x18a)
        for index, text in enumerate(inputs, 1):
            assert k.fn('reset', module.B, module.P)(k.inst)
            parsed = k.fn('parse_more_full_pinyins', module.Z, module.P, module.S)(k.inst, text.encode())
            guessed = k.fn('guess_sentence', module.B, module.P)(k.inst)
            candidate_guess = k.fn('guess_candidates', module.B, module.P, module.Z, module.U)(k.inst, 0, 0x1e)
            candidates = module.candidate_rows(k, k.inst)
            rows = []
            for rank, candidate in enumerate(candidates):
                assert candidate is not None
                nbest = None
                if candidate[0] == 1:
                    pointer, tail = module.P(), ctypes.c_ubyte(0xff)
                    assert k.fn('get_candidate', module.B, module.P, module.U, ctypes.POINTER(module.P))(k.inst, rank, ctypes.byref(pointer))
                    assert k.fn('get_candidate_nbest_index', module.B, module.P, module.P, ctypes.POINTER(ctypes.c_ubyte))(k.inst, pointer, ctypes.byref(tail))
                    nbest = tail.value
                rows.append([candidate[0], nbest, candidate[1]])
            sentences = module.final_step_sentences(k, k.inst)
            # Frozen gate observes first six rows; tie/corpus gate full NBEST.
            shown = rows[:6] if group == 'sentence' else [row for row in rows if row[0] == 1]
            print(json.dumps([text, parsed, guessed, candidate_guess, sentences, shown], ensure_ascii=True), flush=True)
            if index % 1000 == 0:
                print(f'{group} {index}/{len(inputs)}', file=sys.stderr, flush=True)
        k.fn('free_instance', None, module.P)(k._inst)
        k.fn('fini', None, module.P)(k._ctx)
        if dump:
            dump()
        os._exit(0)


def surface_records(stdout):
    """Known shim framing is separate from the existing JSON observations."""
    rows, pending, aborts = [], 0, 0
    for line in stdout.splitlines():
        if line == b'J-GCOV-ABORT-BEGIN':
            if pending: raise RuntimeError('nested surface abort-flush marker')
            pending = 1
        elif line == b'J-GCOV-ABORT-END':
            if not pending: raise RuntimeError('surface abort end without begin')
            pending = 0; aborts += 1
        else:
            if pending: raise RuntimeError('surface observation precedes completed abort flush')
            try:
                row = json.loads(line)
            except ValueError as error:
                raise RuntimeError('unexpected surface stdout: ' + repr(line[:200])) from error
            if not isinstance(row, list) or len(row) != 6:
                raise RuntimeError('malformed surface observation')
            rows.append(row)
    if pending: raise RuntimeError('incomplete surface abort flush; no execution credit')
    return rows, aborts


def surfaces(prefix, pinyin, out, pin, cell='bdb', provenance=None, excused=None):
    """Credit only replays of exactly matching inputs; aggregate per gate.

    Nonmatching inputs are exercised once but their combined footprint is
    discarded. A second replay contains only exact pin/Rust MATCH inputs.
    It must again match every reference observation before receiving credit.
    The six approved supplementary residuals are checked by the Rust gate;
    they are deliberately not relabelled as exact C-ABI MATCH here.

    With `excused` (the peer-cell delta ledgers), any other nonmatching
    input, a pin/Rust difference or a measurement difference, fails the run,
    and each result also lists the excused inputs left uncredited.
    """
    verify_prefix(prefix, provenance, cell)
    attributed, nonmatch, incomplete, results = defaultdict(set), set(), set(), []
    data = prefix / 'lib/libpinyin/data'
    for group in ['sentence', 'ties', 'corpus']:
        observations = []
        clear(out)
        for side, so in [('reference', prefix / 'lib/libpinyin.so'), ('subject', pinyin), ('measurement', out / 'libpinyin.so')]:
            check_tkrzw(cell, 'gcov_measure.py/surfaces', side, so)
            result = subprocess.run([sys.executable, '-B', __file__, '--surface-worker', str(so), str(data), group, cell], capture_output=True)
            if result.returncode:
                raise RuntimeError(f'{group}/{side} failed ({result.returncode}): ' + result.stderr.decode(errors='replace')[-1200:])
            records, aborts = surface_records(result.stdout)
            if aborts:
                incomplete.add('surface/' + group + '/forked-abort-probes')
                print(f'{group}/{side}: {aborts} paired forked abort-flush markers; individual child observations remain a limit', file=sys.stderr, flush=True)
            observations.append(records)
            print(f'{group}/{side}: {len(records)} inputs', file=sys.stderr, flush=True)
        reference, subject, measured = observations
        assert len(reference) == len(subject) == len(measured)
        assert [row[0] for row in reference] == [row[0] for row in subject] == [row[0] for row in measured]
        matched = [r[0] for r, s, m in zip(reference, subject, measured, strict=True) if r == s == m]
        uncredited = sorted({r[0] for r, s, m in zip(reference, subject, measured, strict=True) if not r == s == m})
        if excused is not None:
            unexpected = [(r[0], 'pin/Rust' if r != s else 'measurement') for r, s, m in zip(reference, subject, measured, strict=True)
                          if not r == s == m and r[0] not in excused]
            if unexpected:
                raise RuntimeError(f'{group}: undeclared {cell} surface difference (only the six declared residuals are excused): '
                                   + '; '.join(f'{text!r} ({kind})' for text, kind in unexpected[:20])
                                   + (f'; {len(unexpected) - 20} more' if len(unexpected) > 20 else ''))
        extra = (uncredited,) if excused is not None else ()
        all_counts = counters(out, pin)
        nonmatch.update(identity for identity, count in all_counts.items() if count)
        clear(out)
        if not matched:
            results.append((group, len(reference), 0, len(reference)) + extra)
            print(f'{group}: no matching input; no execution credited', file=sys.stderr, flush=True)
            continue
        env = dict(os.environ, J_MATCH_INPUTS='stdin')
        replay = subprocess.run([sys.executable, '-B', __file__, '--surface-worker', str(out / 'libpinyin.so'), str(data), group, cell], env=env, input=json.dumps(matched).encode(), capture_output=True)
        if replay.returncode:
            raise RuntimeError(f'{group} matched replay failed ({replay.returncode})')
        selected = set(matched)
        expected = [row for row in reference if row[0] in selected]
        replayed, aborts = surface_records(replay.stdout)
        if aborts: incomplete.add('surface/' + group + '/forked-abort-probes')
        assert replayed == expected, f'{group}: matched replay changed observation'
        for identity, count in counters(out, pin).items():
            if count: attributed[identity].add('surface/' + group + '/exact-MATCH-subset')
        results.append((group, len(reference), len(matched), len(reference) - len(matched)) + extra)
        print(f'{group}: exact MATCH {len(matched)}/{len(reference)}; other-input execution discarded before MATCH replay', file=sys.stderr, flush=True)
    return attributed, nonmatch, incomplete, results


def gate_summary(stdout, name):
    summaries = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured;', stdout)
    if summaries != [('1', '0', '0', '0')] or not re.search(r'(?m)^test ' + re.escape(name) + r' \.\.\. ok$', stdout):
        raise RuntimeError('Rust gate must execute exactly the selected test successfully: ' + name)
    return 1


def gates(target, export, model, receipt=None):
    tests = target / 'j-tests' if (target / 'j-tests').exists() else target
    binaries = list((tests / 'debug/deps').glob('sentence_surface_parity-*'))
    binaries = [path for path in binaries if path.is_file() and os.access(path, os.X_OK)]
    if len(binaries) != 1:
        raise RuntimeError(f'exactly one sentence_surface_parity test binary is required; found {len(binaries)} in {tests}')
    env = dict(os.environ, PINYIN_EXPORT_DIR=str(export), PINYIN_MODEL_DIR=str(model))
    passed = 0
    observations = []
    for name in GATE_TESTS:
        print('Rust gate start: ' + name, file=sys.stderr, flush=True)
        result = subprocess.run([str(binaries[0]), '--exact', name, '--include-ignored', '--nocapture'], capture_output=True, env=env)
        if result.returncode:
            raise RuntimeError('sentence Rust gate failed: ' + name + ': ' + result.stdout.decode(errors='replace')[-2000:])
        passed += gate_summary(result.stdout.decode(), name)
        observations.append((name, result.stdout.decode()))
        print('Rust gate PASS: ' + name, file=sys.stderr, flush=True)
    if passed != 4: raise RuntimeError('exactly four passing Rust gate executions are required')
    if receipt is not None:
        receipt.update(exit_code=0, passed=passed, observations=observations,
                       crates_tree=command(['git', '-C', REPO, 'rev-parse', 'HEAD:crates']).strip(),
                       binary_sha256=hashlib.sha256(binaries[0].read_bytes()).hexdigest())
    return 'sentence_surface_parity: four tests PASS (frozen surface, supplementary ties, native costs, fixture membership)'


def preserved_gate(receipt, target):
    """Reuse a trusted orchestrator's completed stdout result in memory only.

    Normal CLI regeneration executes gates(). This helper prevents repeating
    an already completed unchanged gate in a preserved long-running process.
    """
    tests = target / 'j-tests' if (target / 'j-tests').exists() else target
    binaries = [path for path in (tests / 'debug/deps').glob('sentence_surface_parity-*') if path.is_file() and os.access(path, os.X_OK)]
    if len(binaries) != 1:
        raise RuntimeError(f'exactly one sentence_surface_parity test binary is required; found {len(binaries)} in {tests}')
    observations = receipt.get('observations', [])
    if [item[0] for item in observations] != list(GATE_TESTS):
        raise RuntimeError('preserved gate lacks the four exact fresh test summaries')
    if sum(gate_summary(stdout, name) for name, stdout in observations) != 4:
        raise RuntimeError('preserved gate execution count differs')
    tree = command(['git', '-C', REPO, 'rev-parse', 'HEAD:crates']).strip()
    if receipt['exit_code'] != 0 or receipt['passed'] != 4 or receipt['crates_tree'] != tree or receipt['binary_sha256'] != hashlib.sha256(binaries[0].read_bytes()).hexdigest():
        raise RuntimeError('preserved sentence gate provenance changed; execute the gate again')
    return 'sentence_surface_parity: four tests PASS (frozen surface, supplementary ties, native costs, fixture membership)'


def user_store(prefix, target, out, pin, cell='bdb', provenance=None, libraries=('-ldb',)):
    """Existing four-phase user-dir differential, with dump/expected data in pipes."""
    verify_prefix(prefix, provenance, cell)
    tests = target / 'j-tests' if (target / 'j-tests').exists() else target
    binaries = [path for path in (tests / 'debug/deps').glob('user_dir_round_trip-*') if path.is_file() and os.access(path, os.X_OK)]
    if len(binaries) != 1:
        raise RuntimeError(f'exactly one user_dir_round_trip test binary is required; found {len(binaries)} in {tests}')
    headers = next((prefix / 'include').glob('libpinyin-*'))
    data = prefix / 'lib/libpinyin/data'
    attributed, nonmatch, incomplete, results = defaultdict(set), set(), set(), []
    with tempfile.TemporaryDirectory(prefix='j-user-store-') as scratch:
        work = Path(scratch)
        outputs = []
        for side, so_dir in [('reference', prefix / 'lib'), ('measurement', out)]:
            clear(out)
            exe = work / ('user-driver-' + side)
            # The installed reference requires DB5_3, while Fedora's same-ABI
            # libdb exports unversioned symbols. As with the existing dlopen
            # drivers, defer shared-library dependency resolution to ld.so;
            # direct driver references are still required at link time, and
            # an unsatisfied runtime dependency fails the actual test.
            command(['gcc', '-O2', '-o', exe, REPO / 'tools/oracle/user_driver.c', '-I' + str(headers), *shlex.split(command(['pkg-config', '--cflags', 'glib-2.0'])), '-L' + str(so_dir), '-lpinyin', *libraries, *shlex.split(command(['pkg-config', '--libs', 'glib-2.0'])), '-Wl,--allow-shlib-undefined', '-Wl,-rpath,' + str(so_dir)])
            # SONAME lookup must select the measurement object too.
            if side == 'measurement':
                alias = out / 'libpinyin.so.15'
                if not alias.exists(): alias.symlink_to('libpinyin.so')
            profile, owned = work / ('profile-' + side), work / ('owned-' + side)
            profile.mkdir(); owned.mkdir()
            env = dict(os.environ, LD_LIBRARY_PATH=str(so_dir) + (":" + os.environ["LD_LIBRARY_PATH"] if os.environ.get("LD_LIBRARY_PATH") else ""), OX_SYSTEM_DIR=str(data), OX_PIN_DIR=str(profile), OX_OWNED_DIR=str(owned), OX_INPUTS='nihao nisha')
            check_tkrzw(cell, 'gcov_measure.py/user_store', side, exe, env=env)
            check_tkrzw(cell, 'gcov_measure.py/user_store', 'subject', binaries[0], env=env)
            command([exe, 'train', data, profile, 'nihao', 'nisha'], env=env)
            original = sorted(command([exe, 'dump', data, profile], env=env).splitlines())
            assert original and any(line.startswith('B') for line in original)
            command([binaries[0], '--ignored', '--nocapture', 'a_pin_profile'], env=env)
            rewritten = sorted(command([exe, 'dump', data, profile], env=env).splitlines())
            assert original == rewritten, 'pin profile rewrite differs'
            read_fd, write_fd = os.pipe()
            try:
                env['OX_EXPECTED_PHRASES'] = '/proc/self/fd/' + str(write_fd)
                command([binaries[0], '--ignored', '--nocapture', 'oxpinyin_trains'], env=env, pass_fds=(write_fd,))
                os.close(write_fd); write_fd = None
                expected = sorted(os.read(read_fd, 65536).decode().splitlines())
            finally:
                os.close(read_fd)
                if write_fd is not None: os.close(write_fd)
            rows = sorted(command([exe, 'phrases', data, owned], env=env).splitlines())
            actual = sorted(row.split('\t')[1] for row in rows if row.startswith('P'))
            assert actual == expected and actual, 'owned profile phrase set differs'
            outputs.append((original, rewritten, rows))
            if side == 'measurement': measured = counters(out, pin)
        # Bigram-export indeterminate bytes remain the existing same-renderer
        # comparison within each side, not an invented cross-renderer rule.
        verdict = 'MATCH' if outputs[0] == outputs[1] else 'MEASUREMENT-DIFFER'
        for identity, count in measured.items():
            if count:
                if verdict == 'MATCH': attributed[identity].add('user-store/four-phase-round-trip')
                else: nonmatch.add(identity)
        results.append(('four-phase-round-trip', verdict))
        print('user-store: ' + verdict, file=sys.stderr, flush=True)
    return attributed, nonmatch, incomplete, results


def identity_guard(prefix, pinyin, zhuyin, config, surface_pinyin=None):
    verify_prefix(prefix)
    return tuple(hashlib.sha256(path.read_bytes()).hexdigest() for path in
                 [prefix / 'oracle-pin.txt', prefix / 'lib/libpinyin.so', prefix / 'lib/libzhuyin.so', pinyin, zhuyin, config, surface_pinyin or pinyin])


def prepare_parallel(prefix, pinyin, zhuyin, target, main_out, pin, config, address, surface_pinyin=None):
    """Separate lane-owned counters, result transfer through a Unix stream only.

    No result file is created. The socket inode carries no observations; all
    attribution remains in process memory until the renderer consumes it.
    Normal regeneration uses the sequential functions directly.
    """
    out = main_out.with_name('gcov-parallel')
    out.mkdir(exist_ok=True)
    for source in main_out.glob('*.gcno'):
        target_path = out / source.name
        if not target_path.exists(): target_path.symlink_to(source)
    for name in ['libpinyin.so', 'libzhuyin.so']:
        target_path = out / name
        if not target_path.exists(): target_path.symlink_to(main_out / name)
    os.environ['GCOV_PREFIX'] = str(out)
    # GCC recorded absolute filenames in main_out; retain just each basename.
    os.environ['GCOV_PREFIX_STRIP'] = str(len(main_out.parts) - 1)
    result = {}
    for name, operation in [('same-data', lambda: same_data(prefix, pinyin, out, pin)),
                            ('user-store', lambda: user_store(prefix, target, out, pin)),
                            ('surface', lambda: surfaces(prefix, surface_pinyin or pinyin, out, pin))]:
        result[name] = operation()
    compiled = counters(out, pin)
    payload = (identity_guard(prefix, pinyin, zhuyin, config, surface_pinyin), result, set(compiled))
    # Private task-created socket, never a remotely supplied pickle stream.
    if address.exists(): raise RuntimeError('parallel result socket already exists')
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as server:
        server.bind(str(address)); os.chmod(address, 0o600); server.listen(1)
        print('PARALLEL_STAGES_READY (results retained in memory)', flush=True)
        with server.accept()[0] as connection:
            stream = connection.makefile('wb')
            pickle.dump(payload, stream); stream.flush()
    address.unlink()


def consume_parallel(address, expected_guard):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.connect(str(address))
        guard, results, compiled = pickle.load(connection.makefile('rb'))
    if guard != expected_guard: raise RuntimeError('preserved measurement input hashes changed')
    return results, compiled


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--worker':
        worker(*sys.argv[2:])
    elif len(sys.argv) > 1 and sys.argv[1] == '--surface-worker':
        surface_worker(*sys.argv[2:])
    else:
        parser = argparse.ArgumentParser(description=__doc__)
        parser.add_argument('--pin', type=Path, required=True)
        parser.add_argument('--config', type=Path, required=True)
        parser.add_argument('--oracle', type=Path, required=True)
        parser.add_argument('--build', type=Path, required=True)
        parser.add_argument('--cell', choices=sorted(CELLS), default='bdb')
        args = parser.parse_args()
        print(build(args.pin.resolve(), args.config.resolve(), args.build.resolve(), args.oracle.resolve(), args.cell))
