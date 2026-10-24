#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal
"""Case-driven pin-against-subject contract differential (lane C).

Usage: contract-diff.py CELL PREFIX PINYIN_SO [--zhuyin-so PATH]
                        [--cases a,b,...] [--issue N]... [--family F]...
                        [--expect-parent] [--plan-only] [--case-timeout SECONDS] [--jobs N]
       contract-diff.py CELL PREFIX PINYIN_SO --zhuyin-so PATH
                        --per-call wrap|lookup-only [--base-pinyin-so PATH]
                        [--base-zhuyin-so PATH] [--per-call-cpu N]
       contract-diff.py --list-tags
       contract-diff.py bdb PREFIX PREFIX/lib/libpinyin.so
                        --regenerate-sentence-ties FIXTURE

Each case is a short function that drives one facade through the C ABI and
returns the observations: return values, out-params (an untouched out-param
keeps its sentinel), bytes consumed and the GLib log records. Every case runs
in a fresh worker process per side (pin, subject) with a fresh user dir, so a
crash is a result: an exit code or a signal. Workers start from
contract-worker.py, which imports this file with its bytecode cached. The two
JSON documents must be identical, byte for byte. A key that starts with "~" is
recorded but left out of the comparison (an indeterminate value at the pin).

The raw `fprintf(stderr)` text of the pin is compared only by the cases
that ask for it (`stderr=True`, below); every other case counts the lines.
Cases that make the pin abort belong to the class (c) logging differential,
not to this one.

A class (c) case (`abort=`) holds when the pin dies of SIGABRT and the subject
answers the declared value with exactly one warning in its own library's domain
(`libpinyin`, or `libzhuyin` for the zhuyin facade).

A class (b) case (`crash=`) holds when the pin dies of SIGSEGV after its raw
stderr line (an empty chunk read through a NULL base) and the subject, which
does not crash, wrote the same stderr bytes and answers the declared fields.
The pin's bytes are compared up to the crash; the invalid read is never
reproduced.

A policy-accepted language/runtime difference is neither MATCH nor an
unclassified DIFFER. A current discrepancy record
(`docs/findings/language-runtime-discrepancy-template.md`) names the case and
its expected non-MATCH; the disposition is POLICY-ACCEPTED. The case keeps
its pin and subject observations, and it is never deleted or relabelled
MATCH. `policy_record=` links a registered, current record whose
`contract-diff-policy` JSON block specifies both complete observations.

--expect-parent runs the same cases against a parent build: a case marked
`control` must still match, every other case must differ.

A difference counts only between two valid observations. For an ordinary
case (no `abort=`, `crash=` or `both_abort=`), each side must have exited
normally with a result document; a side that died or printed no
result is INVALID, and the case fails in both modes — two failed workers are
not a MATCH, and they are not the expected DIFFER of a parent build either.
For a case whose spec registers how the pin dies, the matcher's own exit
conditions stand unchanged.

Missing inputs and an empty case list fail. Scratch lives under TMPDIR and is
removed even on failure.

Ordinary cases share a 300-second deadline across pin and subject.
--case-timeout overrides it. A timeout reports TIMEOUT and fails in both
modes. Later cases still run.

--jobs N requests at most N concurrent ordinary cases. The default is serial.
Heavy admission reserves memory for every active case and can reduce N.
Each case owns its scratch directory. Its two sides still run in sequence
with one deadline; verdicts and observations stay in selection order.

Selection. --cases names case IDs. --issue N (repeatable; `#N`, `W11` and
`row:54` forms as the registry uses them) selects every case registered with
that tag in its `case(..., issues=(...))` call, and --family F every case
whose ID starts with `F/` (separator, before-cursor, zero-key, ...). An
unknown tag, or a selection that matches nothing, is an error that lists the
registered tags; it never falls back to the default suite. --list-tags
prints the tag index as JSON (`tags`: tag to case IDs, with `family/<F>`
entries; `modes`: case ID to facade). Before running, the plan goes to stderr: the
selected IDs and why each was selected, the facades and the artifacts the
selection needs; --plan-only stops there. Axis C cases carry no tags and are
never selected by tag; they run through --axis-c-replacement.

Timing. The last stdout line is `{"timing": ...}`: the number of cases and
workers and the seconds spent in each phase, summed over the run — worker
startup (spawn to the first statement of the entry; for a script started by
path this includes compiling it), load (the module body, which registers
the cases), fixture (library load, user dir, the facade `init` calls),
pin and subject execution, teardown (worker exit and the user-dir
listing) and comparison. The verdict rows are unchanged.
"""
import argparse
import copy
import contextlib
import ctypes as C
import functools
import hashlib
import json
import os
from pathlib import Path
import re
import resource
import runpy
import shutil
import signal
import statistics
import subprocess
import sys

if sys.flags.optimize != 0:
    print('contract-diff: optimised Python is unsupported; unset PYTHONOPTIMIZE and remove -O/-OO',
          file=sys.stderr)
    raise SystemExit(2)

import tempfile
import threading
import time

_ENTERED = time.monotonic()  # the module body starts here: worker startup ends, load begins

P, U, I, S, B, Z = C.c_void_p, C.c_uint, C.c_int, C.c_char_p, C.c_bool, C.c_size_t
UNTOUCHED = 0xABCDEF  # out-param sentinel
# The DBM cell the run is for (set by `main`, inherited by every worker):
# fixtures that craft a user table write it in that backend's own format.
CELL = os.environ.get('CONTRACT_DIFF_CELL', 'bdb')
check_tkrzw = runpy.run_path(str(Path(__file__).resolve().parents[1] / 'tkrzw/runtime.py'))['check']


def host_oracle_platform():
    """Return the oracle-pin.txt key for Linux x86-64 or AArch64, else None."""
    if sys.platform != 'linux':
        return None
    return {'x86_64': 'linux/amd64', 'amd64': 'linux/amd64',
            'aarch64': 'linux/arm64', 'arm64': 'linux/arm64'}.get(os.uname().machine)


HOST_ORACLE_PLATFORM = host_oracle_platform()

CASES = {}

# Every worker is a fresh process started from this stub, which loads this
# file through the import system so its bytecode is cached between workers
# (a script run as __main__ is recompiled on every start).
WORKER = Path(__file__).resolve().with_name('contract-worker.py')


NO_ABORT = object()

# Each facade logs under its own library's domain.
WARNING_DOMAIN = {'pinyin': 'libpinyin', 'zhuyin': 'libzhuyin'}


def case_spec(mode='pinyin', control=False, abort=NO_ABORT, stderr=False, crash=None, userdir=False,
              cells=('bdb', 'kc', 'tkrzw'), either=False, host_scanf=(), dir_chunk=None, stderr_varies=False,
              abort_logs=None, fatal=False, both_abort=False, strjoinv_over_read=False, issues=(), *, fn=None,
              policy_record=None, case_id=None):
    """Build the registration metadata with shared defaults, without validating it.

    Store host scanner spellings and normalized issue tags as tuples. A
    crash expectation enables stderr comparison even if stderr is false.
    Registration checks option combinations separately."""
    return dict(fn=fn, mode=mode, control=control, abort=abort, stderr=stderr or crash is not None,
                crash=crash, userdir=userdir, cells=cells, either=either, host_scanf=tuple(host_scanf),
                dir_chunk=dir_chunk, stderr_varies=stderr_varies, abort_logs=abort_logs, fatal=fatal,
                both_abort=both_abort, strjoinv_over_read=strjoinv_over_read,
                issues=tuple(normal_tag(tag) for tag in issues),
                policy_record=policy_record, case_id=case_id)


def case(name, *args, **kwargs):
    """Return a decorator registering a unique case and its comparison rules.

    `stderr=True` also compares what the library wrote
    to stderr (raw `fprintf`s of the pin; GLib logs are the `logs` field),
    with the scratch directory names normalised. `abort=<value>` marks a
    class (c) site: the pin
    must die of SIGABRT, and the subject must return `<value>` in its `ret`
    field with exactly one GLib warning in the facade's domain (`libpinyin`,
    or `libzhuyin` for the zhuyin facade; level 16). `crash=<fields>` marks a
    class (b) site: the pin must die of SIGSEGV (after the stderr it wrote
    up to then), and the subject must exit normally, write the same stderr
    bytes and answer `<fields>` (a dict of result fields). `userdir=True`
    also compares the normalized user-directory listings, for an
    abort that comes after the pin has written some of its profile.
    `abort_logs=<list>` overrides the expected subject `logs` list when the
    operation already logged a common entry earlier (e.g. the init critical
    before the candidate warning). `fatal=True` runs both sides with
    `G_DEBUG=fatal-criticals`; `both_abort=True` requires `fatal=True` and
    expects both sides to die of the same GLib fatal-log signal (SIGABRT
    or SIGTRAP). It compares stderr (GLib process id and timestamp
    normalised) and the partial user directory when their respective
    flags are set, instead of a surviving subject result. `cells`
    names the DBM cells whose storage format the case's fixture reproduces;
    a case skipped on the others prints `SKIP` instead of a verdict.
    `either=True` (with both `abort=` and `crash=`) is for a fixture whose
    pin site depends on the file system: the pin's own signal picks the
    matcher, SIGABRT the class (c) one and anything else the class (b) one.
    `stderr_varies=True` (with `crash=`) is for a site where the pin's raw
    stderr is read from freed memory and so changes from run to run: the pin
    must still die of SIGSEGV and the subject answer `<fields>`, but the
    stderr bytes are not compared.
    `host_scanf=<spellings>` names `%f` inputs the case expects the host
    libc's scanner to reject, as the oracle image's glibc does. The pin
    reads them with the host libc, so on a host whose scanner accepts one
    the pin loads what the subject refuses. Only that difference (the pin
    created its context; the subject refused `table.conf`) prints
    `HOST-DEPENDENT` instead of `DIFFER` outside `--expect-parent` and is
    not a failure; any other difference stays a DIFFER. A MATCH is reported as such on any host.
    `dir_chunk='user'|'system'` (with `abort=`) is for a `table.conf` row
    without a file name, which makes the pin read that directory of the
    case as a chunk. `MemoryChunk` asserts on the `read` of a directory
    (the class (c) site) only where `lseek(SEEK_END)` on it answers the
    8-byte header or more (`MemoryChunk::load` and `MemoryChunk::mmap` in
    `src/include/memory_chunk.h` at pin 074a2219), and the
    file system of the scratch directory decides that:
    `directory_reads_as_a_chunk` asks this host. Where it answers less
    (tmpfs refuses `SEEK_END` on a directory), the load is an ordinary
    `false`. For `'user'` the pin then carries on, and so does the
    subject, which asks the same question: the case is compared as an
    ordinary one, with the stderr and the user directory. For `'system'`
    the pin writes `mmap <dir> failed!` and dies of SIGSEGV in
    `SubPhraseIndex::load` on the empty chunk, a class (b) site: the
    subject writes the same line and gives the abort site's answer, so the
    pin must die of SIGSEGV after the stderr the subject wrote, byte for
    byte, and the subject must answer as `abort=` registers.
    `strjoinv_over_read=True` is for the bigram export iterator's join of
    an unterminated `GPtrArray` (register row 1, class (b);
    `strjoinv_fault`): whether the pin faults depends on heap layout. A pin
    that dies of SIGSEGV after the stderr the subject wrote, byte for byte,
    is held, and the subject need only exit normally with a result: its
    rows are not compared with anything then. They are compared on the
    cells where the pin completes, as an ordinary case's are, every field
    exactly. Any other pin signal, or different stderr, is INVALID.
    `issues=<tags>` names the issues, register rows or lane tags the case
    guards (`'640'`, `'row:54'`, `'W11'`), the selectors of `--issue`; a
    case's family for `--family` is the text before the first `/` of its
    ID, when it has one.

    `policy_record` links a discrepancy record whose evidence is checked at
    verdict time. Accepted differences are reported as `POLICY-ACCEPTED`,
    never `MATCH`; registration checks option compatibility, not the record's
    contents."""
    def register(fn):
        """Store the case specification and return the original function.

        Reject duplicate IDs with ValueError and assert the required option
        combinations before modifying CASES. Policy records cannot be combined
        with the signal, host-dependent or profile-comparison exceptions."""
        if name in CASES:
            raise ValueError('duplicate case name: %s' % name)
        spec = case_spec(*args, fn=fn, case_id=name, **kwargs)
        assert not spec['either'] or (spec['abort'] is not NO_ABORT and spec['crash'] is not None), name
        assert not spec['both_abort'] or spec['fatal'], name
        assert spec['dir_chunk'] in (None, 'user', 'system') and (spec['dir_chunk'] is None or spec['abort'] is not NO_ABORT), name
        if spec['policy_record'] is not None:
            assert not (spec['abort'] is not NO_ABORT or spec['crash'] is not None
                        or spec['both_abort'] or spec['strjoinv_over_read']
                        or spec['host_scanf'] or spec['dir_chunk'] or spec['userdir']), name
        CASES[name] = spec
        return fn
    return register


def normal_tag(tag):
    """Normalize an issue selector by stripping whitespace and leading hashes.

    Lowercase the result and canonicalize a leading "Row 54" or "row:54"
    to "row:54"; numeric issue IDs and lane tags remain distinct."""
    tag = str(tag).strip().lstrip('#').lower()
    return re.sub(r'^row[ :]*', 'row:', tag)


def family_of(name):
    """Return the prefix before the first slash, or None if there is no slash."""
    return name.split('/', 1)[0] if '/' in name else None


def tag_index():
    """issue tag -> sorted case IDs, and 'family/<F>' -> IDs; axis C excluded."""
    index = {}
    for name, spec in CASES.items():
        if name.startswith('axis-c-'):
            continue
        for tag in spec['issues']:
            index.setdefault(tag, []).append(name)
        family = family_of(name)
        if family:
            index.setdefault('family/' + family, []).append(name)
    return {tag: sorted(names) for tag, names in sorted(index.items())}


class SelectionError(ValueError):
    """Report an unknown selector or a selection with no case IDs."""
    pass


def case_names(value):
    """Split comma-separated case IDs without trimming or deduplicating them.

    Raise SelectionError if any component is empty or whitespace-only."""
    names = value.split(',')
    if any(not name.strip() for name in names):
        raise SelectionError('--cases must not contain an empty value')
    return names


def select_cases(cases=None, issues=(), families=()):
    """Return case IDs in selection order and the reasons accumulated per ID.

    Process explicit IDs first, then issue selectors, then families; each
    issue or family contributes sorted IDs. Keep the first occurrence's
    position and append all reasons, including repeated selectors. With no
    selectors, choose every non-axis-C case in sorted order. Axis C can be
    selected explicitly but is absent from issue and family indexes.
    Raise SelectionError for unknown selectors, empty comma components or
    an empty selection; never fall back to the default suite on an error."""
    index = tag_index()
    chosen = {}
    for name in case_names(cases) if cases is not None else ():
        if name not in CASES:
            raise SelectionError('unknown case: %s' % name)
        chosen.setdefault(name, []).append('--cases')
    for issue in issues:
        tag = normal_tag(issue)
        if tag not in index or tag.startswith('family/'):
            known = ', '.join(t for t in index if not t.startswith('family/'))
            raise SelectionError('unknown issue tag %r; registered tags: %s' % (issue, known))
        for name in index[tag]:
            chosen.setdefault(name, []).append('issue ' + tag)
    for family in families:
        key = 'family/' + family
        if key not in index:
            known = ', '.join(t[len('family/'):] for t in index if t.startswith('family/'))
            raise SelectionError('unknown family %r; registered families: %s' % (family, known))
        for name in index[key]:
            chosen.setdefault(name, []).append('family ' + family)
    if cases is None and not (issues or families):
        chosen = {n: ['default suite'] for n in sorted(n for n in CASES if not n.startswith('axis-c-'))}
    if not chosen:
        raise SelectionError('no cases selected')
    return list(chosen), chosen


def plan_lines(names, reasons, pinyin_so, zhuyin_so, prefix, cell):
    """The selection plan: IDs and reasons (one line each for an explicit
    selection; a count for the default suite), facades and artifacts."""
    modes = {}
    for name in names:
        modes[CASES[name]['mode']] = modes.get(CASES[name]['mode'], 0) + 1
    skipped = sum(1 for name in names if cell not in CASES[name]['cells'])
    explicit = any(reason != ['default suite'] for reason in reasons.values())
    lines = ['plan: %d case%s%s; cell %s; facades: %s' % (
        len(names), '' if len(names) == 1 else 's',
        '' if explicit else ' (default suite: every registered non-axis-C case)', cell,
        ', '.join('%s %d' % item for item in sorted(modes.items())))]
    if skipped:
        lines.append('  %d case%s skip on this cell (fixture in another backend\'s format)' % (
            skipped, '' if skipped == 1 else 's'))
    artifacts = ['subject %s' % pinyin_so, 'pin %s' % (prefix / 'lib/libpinyin.so'),
                 'system tables %s' % (prefix / 'lib/libpinyin/data')]
    if 'zhuyin' in modes:
        artifacts += ['subject %s' % (zhuyin_so or '--zhuyin-so (missing)'), 'pin %s' % (prefix / 'lib/libzhuyin.so')]
    lines.append('  artifacts: ' + '; '.join(artifacts))
    if explicit:
        width = max(len(name) for name in names)
        for name in names:
            lines.append('  %-*s  %-7s %s' % (width, name, CASES[name]['mode'], ', '.join(reasons[name])))
    return lines


def host_scanf_accepts(spelling):
    """Test whether the host libc converts one float from the spelling's prefix.

    Call sscanf with %f in the current numeric locale, without requiring
    the entire string to be consumed or changing the locale. This checks
    the conversion used by SystemTableInfo2::load in src/storage/table_info.cpp
    at pin 074a2219; that loader sets LC_NUMERIC to C before scanning."""
    scanned = C.c_float()
    return C.CDLL(None).sscanf(spelling.encode(), b'%f', C.byref(scanned)) == 1


def pin_loaded_subject_refused_table_conf(pin, subject):
    """Recognize successful pin initialization paired with subject table refusal.

    Require zero exits, ctx exactly True for the pin and False for the
    subject, and only the expected user.conf-open and table.conf-load
    diagnostics. Other result fields are not checked. The pin diagnostic
    has no trailing newline, as emitted by UserTableInfo::load in
    src/storage/table_info.cpp at pin 074a2219."""
    return (pin['exit'] == 0 and subject['exit'] == 0
            and (pin['result'] or {}).get('ctx') is True
            and (subject['result'] or {}).get('ctx') is False
            and re.fullmatch(r'open \S*/user\.conf failed\.', pin['stderr']) is not None
            and re.fullmatch(r'load \S*table\.conf failed!\n', subject['stderr']) is not None)


@functools.cache
def directory_reads_as_a_chunk():
    """Cache whether a temporary directory passes the pin's 8-byte size gate.

    Create a directory containing user.conf in the default temporary
    directory, seek to its end, and remove it on exit. Return False if the
    seek raises OSError; creation and open failures propagate. The cached
    result is reused even if the temporary-directory setting later changes.
    This reproduces only the size check in MemoryChunk::load and
    MemoryChunk::mmap in src/include/memory_chunk.h at pin 074a2219, not
    their subsequent reads. The single entry models the user.conf saved by
    check_format in src/pinyin.cpp before the directory-chunk cases run."""
    with tempfile.TemporaryDirectory(prefix='contract-dir-probe-') as probe:
        Path(probe, 'user.conf').touch()
        fd = os.open(probe, os.O_RDONLY)
        try:
            return os.lseek(fd, 0, os.SEEK_END) >= 8
        except OSError:
            return False
        finally:
            os.close(fd)


class Kit:
    """Hold a worker's native libraries, fresh profile and lazy facade handles.

    Capture GLib diagnostics through process-wide handlers. Context and
    instance handles are cached on demand; the kit does not automatically
    free them, remove its profile or unregister the handlers."""

    def __init__(self, mode, so, data, scratch):
        """Load the facade and GLib, install log handlers and create a user directory.

        Check the selected tkrzw runtime before loading the facade. Register
        handlers for both facade domains and the default GLib domain, retaining
        the callback on this object. Create the profile under scratch; native
        context initialization is deferred until requested."""
        self.mode, self.data, self.scratch = mode, data, scratch
        check_tkrzw(os.environ.get("CONTRACT_DIFF_CELL", CELL), "contract-diff.py", os.environ.get("OXPINYIN_RUNTIME_SIDE", "subject"), so)
        self.lib = C.CDLL(so, use_errno=True)
        self.glib = C.CDLL('libglib-2.0.so.0')
        self.glib.g_free.argtypes = [P]
        self.glib.g_free.restype = None
        self.logs = []
        self.log_messages = []
        cb_type = C.CFUNCTYPE(None, S, U, S, P)

        def handler(domain, level, message, _unused):
            """Append the domain, level and message to the kit's log lists.

            Decode nonempty strings as UTF-8 strictly; record None for NULL
            or empty domain and message values."""
            self.logs.append([domain.decode() if domain else None, level])
            self.log_messages.append(message.decode() if message else None)
        self._handler = cb_type(handler)
        self.glib.g_log_set_handler.argtypes = [S, U, cb_type, P]
        self.glib.g_log_set_handler.restype = U
        for domain in (b'libpinyin', b'libzhuyin', None):
            self.glib.g_log_set_handler(domain, 0xFFFFFFFC, self._handler, None)
        self._ctx = None
        self._inst = None
        self.user = tempfile.mkdtemp(prefix='user-', dir=scratch)
        self.init_seconds = 0.0  # the facade `init` calls: fixture, not case work

    def fn(self, name, res, *args):
        """Set a facade symbol's ctypes return and argument types, then return it.

        Prefix name with the kit's mode; missing symbols raise AttributeError.
        This changes the loaded function's signature without calling it."""
        f = getattr(self.lib, self.mode + '_' + name)
        f.restype, f.argtypes = res, list(args)
        return f

    def per_call_step(self):
        """Return a no-op context manager for per-call instrumentation hooks."""
        return contextlib.nullcontext()

    def init(self, system=None, user=None, *, literal=False):
        """Call the facade initializer and accumulate elapsed monotonic seconds.

        Replace false system or user values with the kit defaults unless
        literal is true. Pass None and bytes through; encode strings as UTF-8.
        Return the native pointer, possibly NULL, without caching or asserting
        success. Accumulate time even if argument conversion or the call raises."""
        def raw(value):
            """Preserve None and byte strings; encode other values with their encode method."""
            return value if value is None or isinstance(value, bytes) else value.encode()
        if not literal:
            system, user = system or self.data, user or self.user
        started = time.monotonic()
        try:
            return self.fn('init', P, S, S)(raw(system), raw(user))
        finally:
            self.init_seconds += time.monotonic() - started

    @property
    def ctx(self):
        """Initialize and cache the native context on first access, asserting success.

        A NULL result raises AssertionError and leaves initialization eligible
        for another attempt on the next access."""
        if self._ctx is None:
            self._ctx = self.init()
            assert self._ctx, 'init failed'
        return self._ctx

    def alloc(self):
        """Allocate a fresh instance from the cached context and assert it is non-NULL.

        Return the pointer without replacing the kit's cached instance."""
        inst = self.fn('alloc_instance', P, P)(self.ctx)
        assert inst
        return inst

    @property
    def inst(self):
        """Allocate and cache one native instance on first access."""
        if self._inst is None:
            self._inst = self.alloc()
        return self._inst

    def text(self, ptr):
        """Decode and free a caller-owned GLib string, returning None for NULL.

        Replace invalid UTF-8 bytes during decoding and release the non-NULL
        pointer with g_free."""
        if not ptr:
            return None
        value = C.string_at(ptr).decode('utf-8', 'replace')
        self.glib.g_free(ptr)
        return value


def worker(mode, so, data, name):
    """Run one isolated case, print its observation and exit without teardown.

    Consume CONTRACT_DIFF_SPAWNED and disable core dumps. For ordinary cases,
    create a kit under TMPDIR, run the registered function and attach its GLib
    logs. Axis-C workers also disable process dumpability and include log
    messages. If a spawn timestamp was supplied, print a separate timing line
    in seconds, charging facade initialization to fixture time. Flush the JSON
    result before os._exit(0); failures before that point can raise or terminate
    the process, and the caller is responsible for scratch cleanup.

    In profile-rows mode, instead read the container at data using name as its
    kind, print JSON rows and return normally. Validate the tkrzw reader anchor
    when applicable; an uncertified anchor raises ValueError."""
    # The driver's spawn time, popped so a nested worker (axis C's reopen)
    # neither inherits it nor prints a timing line into a parsed stdout.
    spawned = os.environ.pop('CONTRACT_DIFF_SPAWNED', None)
    entered = time.monotonic()
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    if mode == 'profile-rows':
        # Untimed teardown comparison in this existing isolated worker. Do not
        # retain a backend CDLL in the measurement controller between samples.
        runtime = check_tkrzw(CELL, 'contract-diff.py/profile-rows', 'reader', so)
        if CELL == 'tkrzw' and runtime is None:
            raise ValueError('tkrzw profile reader anchor has no certified libtkrzw dependency')
        rows = index_file_rows(data, kind=name, allow_empty=True, library=runtime)
        print(json.dumps(dict(rows=rows)), flush=True)
        return
    if name.startswith('axis-c-'):
        # Fedora's pipe core_pattern can invoke systemd-coredump even when
        # RLIMIT_CORE is zero. This guard prevents routing these deliberate
        # pin faults into a host capture service; descendants inherit it.
        libc = C.CDLL(None, use_errno=True)
        libc.prctl.argtypes, libc.prctl.restype = [I, C.c_ulong, C.c_ulong, C.c_ulong, C.c_ulong], I
        assert libc.prctl(4, 0, 0, 0, 0) == 0, ('PR_SET_DUMPABLE failed', C.get_errno())
    scratch = os.environ['TMPDIR']
    kit = Kit(mode, so, data, scratch)
    built = time.monotonic()
    result = CASES[name]['fn'](kit)
    finished = time.monotonic()
    result['logs'] = kit.logs
    if name.startswith('axis-c-'):
        result['log_messages'] = kit.log_messages
    if spawned is not None:
        # One line the driver reads and nothing compares: `run_worker` keeps
        # only `{`-lines as results and this is not part of the observation.
        print('T ' + json.dumps(dict(startup=_ENTERED - float(spawned), load=entered - _ENTERED,
                                     fixture=built - entered + kit.init_seconds,
                                     execute=finished - built - kit.init_seconds)), flush=True)
    print(json.dumps(result, sort_keys=True, ensure_ascii=False), flush=True)
    os._exit(0)  # no atexit: a case never shares teardown with the next


def observed(result):
    """Return a shallow copy omitting top-level keys that start with a tilde."""
    return {k: v for k, v in result.items() if not k.startswith('~')}


# --------------------------------------------------------------------------
# Cases. A control case holds on the parent as well; every other case is the
# regression guard of the fix named in its comment.
# --------------------------------------------------------------------------

def consumed(kit, parse_name, text, fresh=True):
    """Parse a byte string and return the native parser's consumed byte count.

    Allocate a new instance by default; with fresh=False, mutate the kit's
    cached instance instead. The caller retains responsibility for freeing
    new instances. parse_name selects a size_t-returning facade parser."""
    inst = kit.alloc() if fresh else kit.inst
    return kit.fn(parse_name, Z, P, S)(inst, text)


def key_out(kit, text):
    """Return the full-pinyin parser's success flag and raw 16-bit output key.

    Use the cached instance and seed the key with the low 16 bits of
    UNTOUCHED, retaining that numeric sentinel if the parser leaves it alone."""
    key = C.c_ushort(UNTOUCHED & 0xFFFF)
    ret = kit.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_ushort))(kit.inst, text, C.byref(key))
    return [ret, key.value]


# lane C, #532: the option word `pinyin_init` seeds is USE_TONE.
@case('default-option-parse', issues=('532',))
def _(k):
    """Record consumed bytes for complete, incomplete and invalid default-option inputs.

    Parse each spelling on a fresh instance without changing context options."""
    return {t.decode(): consumed(k, 'parse_more_full_pinyins', t)
            for t in (b'su3cl3', b'ni2hao', b'n', b'zzz', b'nihao', b'ni3')}


@case('default-option-single-key', issues=('532',))
def _(k):
    """Record single-key return flags and raw keys under the initial context options.

    Reuse the cached instance for the tone, incomplete and invalid spellings."""
    return {t.decode(): key_out(k, t) for t in (b'n', b'ni3', b'ni', b'a1', b'zhang', b'zzz')}


@case('explicit-use-tone-parse', control=True, issues=('532',))
def _(k):
    """Set only USE_TONE and record consumed bytes for each spelling on a fresh instance."""
    k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    return {t.decode(): consumed(k, 'parse_more_full_pinyins', t)
            for t in (b'su3cl3', b'ni2hao', b'n', b'zzz', b'nihao')}


@case('explicit-incomplete-parse', control=True, issues=('532',))
def _(k):
    """Set only PINYIN_INCOMPLETE and record consumed bytes on fresh instances."""
    k.fn('set_options', B, P, U)(k.ctx, 1 << 3)
    return {t.decode(): consumed(k, 'parse_more_full_pinyins', t)
            for t in (b'su3cl3', b'ni2hao', b'n', b'zzz', b'nihao')}


# lane C, #587: valid pinyin followed by invalid UTF-8 keeps the prefix.
@case('bytes-parse-more-full', issues=('587',))
def _(k):
    """Record full-pinyin byte consumption with invalid UTF-8 at several positions.

    Use a fresh instance per byte string and repr(text) as the result key."""
    return {repr(t): consumed(k, 'parse_more_full_pinyins', t)
            for t in (b'ni\xffhao', b'nihao\xff', b'\xffnihao', b'ni\xe4\xbd', b'ni\xc0\x80hao')}


@case('bytes-parse-more-double', issues=('587',))
def _(k):
    """Record double-pinyin byte consumption with invalid UTF-8 before, within and after input."""
    return {repr(t): consumed(k, 'parse_more_double_pinyins', t)
            for t in (b'ni\xffhk', b'nihk\xff', b'\xffnihk')}


@case('bytes-parse-more-chewing', issues=('587',))
def _(k):
    """Record chewing byte consumption with invalid UTF-8 before, within and after input."""
    return {repr(t): consumed(k, 'parse_more_chewings', t)
            for t in (b'su\xff3', b'su3\xff', b'\xffsu3')}


@case('bytes-add-phrase-pinyin', issues=('587',))
def _(k):
    """Add phrases with invalid pinyin bytes and enumerate the resulting library rows.

    Write through an add iterator for library 7, then return its two success
    flags and the phrase, reading and count from each exported row. Decode
    and free exported strings; leave the get iterator allocated."""
    rows = []
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    add = k.fn('iterator_add_phrase', B, P, S, S, I)
    ret = [add(it, '你好'.encode(), b"ni'ha\xffo", 1), add(it, '世界'.encode(), b"shi'jie\xff", 1)]
    k.fn('end_add_phrases', None, P)(it)
    out = k.fn('begin_get_phrases', P, P, U)(k.ctx, 7)
    has = k.fn('iterator_has_next_phrase', B, P)
    nxt = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))
    while has(out):
        ph, py, n = P(), P(), I()
        nxt(out, C.byref(ph), C.byref(py), C.byref(n))
        rows.append([k.text(ph.value), k.text(py.value), n.value])
    return dict(added=ret, rows=rows)


@case('bytes-whole-argument', control=True, issues=('587',))
def _(k):
    """Collect whole-argument API results for invalid UTF-8 in readings or phrases.

    Reuse the cached instance for single-key parsing, token lookup, a valid
    multi-key parse and remembering input. Also attempt a library-7 phrase
    addition, closing its add iterator. Return the flags, key and parsed byte
    count; the allocated token array is not freed here."""
    # Entries that hand the whole argument to the pin keep refusing it.
    out = {'parse_full_pinyin': key_out(k, b'ni\xff')}
    arr = k.glib.g_array_new
    arr.restype, arr.argtypes = P, [I, I, U]
    tokens = arr(0, 0, 4)
    out['lookup_tokens'] = k.fn('lookup_tokens', B, P, S, P)(k.inst, '你'.encode() + b'\xff', tokens)
    parse_n = consumed(k, 'parse_more_full_pinyins', b'nihaoshijie', fresh=False)
    out['remember'] = k.fn('remember_user_input', B, P, S, I)(k.inst, '你好世界'.encode() + b'\xff', 1)
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    out['add_bad_phrase'] = k.fn('iterator_add_phrase', B, P, S, S, I)(
        it, '你好'.encode() + b'\xff', b"ni'hao", 1)
    k.fn('end_add_phrases', None, P)(it)
    out['parsed'] = parse_n
    return out


@case('bytes-zhuyin-parse-more', mode='zhuyin', issues=('587',))
def _(k):
    """Record zhuyin-facade full-pinyin and chewing consumption around invalid bytes.

    Use a fresh instance per spelling and label results by parser and byte repr."""
    out = {}
    for name, texts in (('parse_more_full_pinyins', (b'ni3\xffhao3', b'ni3hao3\xff', b'\xffni3')),
                        ('parse_more_chewings', (b'su\xff3', b'su3\xff', b'\xffsu3'))):
        for t in texts:
            out[name + ' ' + repr(t)] = consumed(k, name, t)
    return out


# The zhuyin direct parser splits on spaces and apostrophes, so a reading
# with an invalid byte in its last token is refused whole: unchanged.
@case('bytes-zhuyin-add-phrase', mode='zhuyin', control=True, issues=('587',))
def _(k):
    """Compare phrase-add return flags for valid, trailing-invalid and middle-invalid zhuyin.

    Attempt each addition to library 7 with its own add iterator, closing it
    before the next attempt; all attempts share the same context."""
    out = {}
    for label, reading in (('whole', 'ㄋㄧˇ ㄏㄠˇ'.encode()), ('tail-invalid', 'ㄋㄧˇ ㄏㄠˇ'.encode() + b'\xff'),
                           ('mid-invalid', 'ㄋㄧˇ'.encode() + b'\xff' + 'ㄏㄠˇ'.encode())):
        it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
        out[label] = k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), reading, 1)
        k.fn('end_add_phrases', None, P)(it)
    return out


def sentence_out(k, inst, index):
    """Return a sentence lookup flag and decoded text, None or an untouched marker.

    Call the pinyin facade's indexed sentence accessor with an 8-bit index.
    Free a returned non-NULL string through the kit, but never dereference
    the seeded UNTOUCHED pointer."""
    out = P(UNTOUCHED)
    ret = k.fn('get_sentence', B, P, C.c_ubyte, C.POINTER(P))(inst, index, C.byref(out))
    if out.value == UNTOUCHED:
        return [ret, 'untouched']
    return [ret, k.text(out.value)]


# #902: separators consume bytes without keys (074a2219,
# storage/pinyin_parser2.cpp:238-250,360-378). fill_matrix clears the matrix
# before refusing zero keys (storage/phonetic_key_matrix.cpp:34-38).
# Sentence lookup refuses before clearing results (lookup/phonetic_lookup.h:
# 743-748); candidate lookup clears candidates before refusing (pinyin.cpp:
# 2193-2196; zhuyin.cpp:1468-1471,1548-1551). Observe each export separately,
# both fresh and after a successful guess, including its subsequent state.
def zero_key_guess_case(text, options, operation):
    """Build a case comparing a guess operation on fresh and previously guessed instances.

    Capture the input bytes, optional context options and operation name.
    The returned case supports sentence guesses and the facade-specific
    candidate guesses used by the registrations below."""
    def probe(k):
        """Record parse, guess, sentence and candidate state for both seeding conditions.

        Assert an explicitly requested option update and the seed sentence guess.
        Parse text on a new instance for each condition, then call the selected
        guess export at offset zero or with the prefix 你. Return consumed and
        parsed lengths in bytes, the guess flag, and the sentence/count accessor
        outputs, preserving untouched sentinels and freeing returned sentence text.
        Neither instance is freed here."""
        if options is not None:
            assert k.fn('set_options', B, P, U)(k.ctx, options)
        out = {}
        for seeded in (False, True):
            inst = k.alloc()
            if seeded:
                k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni3hao3')
                assert k.fn('guess_sentence', B, P)(inst)
            parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
            if operation == 'guess_sentence':
                ret = k.fn(operation, B, P)(inst)
            elif operation == 'guess_sentence_with_prefix':
                ret = k.fn(operation, B, P, S)(inst, '你'.encode())
            elif operation == 'guess_candidates':
                ret = k.fn(operation, B, P, Z, U)(inst, 0, 0x1e)
            else:
                ret = k.fn(operation, B, P, Z)(inst, 0)
            # Neither facade exports a sentence-count accessor. Read its
            # public sentence result and candidate count without inventing one.
            sentence = P(UNTOUCHED)
            if k.mode == 'pinyin':
                got_sentence = k.fn('get_sentence', B, P, C.c_ubyte, C.POINTER(P))(
                    inst, 0, C.byref(sentence))
            else:
                got_sentence = k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(sentence))
            count = U(UNTOUCHED)
            got_count = k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            out['seeded' if seeded else 'fresh'] = dict(
                consumed=parsed,
                parsed_length=k.fn('get_parsed_input_length', Z, P)(inst),
                ret=ret, sentence=[got_sentence, owned_text(k, sentence)],
                candidates=[got_count, count.value])
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _operations = ('guess_sentence', 'guess_sentence_with_prefix') + (
        ('guess_candidates',) if _mode == 'pinyin' else
        ('guess_candidates_after_cursor', 'guess_candidates_before_cursor'))
    for _options in (None, 0x20, 0x18a):
        for _text in (b"'", b"''", b"'''"):
            for _operation in _operations:
                _label = 'default' if _options is None else hex(_options)
                case(f'zero-key-guess/{_mode}/{_label}/{_text.decode()}/{_operation}',
                     mode=_mode, control=_mode == 'pinyin', issues=('902',))(
                    zero_key_guess_case(_text, _options, _operation))


# lane C, #542 rows 2, 6, 7, 20: out-params the pin leaves alone, or writes.
@case('empty-parse-sentence', issues=('542',))
def _(k):
    """Record sentence access after empty or invalid parsing and on an unparsed instance.

    Attempt a sentence guess in each state and retain its return flag plus
    sentence output; parsed states also include the consumed byte count."""
    out = {}
    for t in (b'', b'!'):
        inst = k.alloc()
        n = k.fn('parse_more_full_pinyins', Z, P, S)(inst, t)
        guessed = k.fn('guess_sentence', B, P)(inst)
        out[repr(t)] = [n, guessed, sentence_out(k, inst, 0)]
    inst = k.alloc()
    out['fresh'] = [k.fn('guess_sentence', B, P)(inst), sentence_out(k, inst, 0)]
    return out


@case('character-offset-out', issues=('542',))
def _(k):
    """Record character-offset return flags and whether the output length was written.

    Use fresh full- or double-pinyin parses for matching, missing, empty and
    keyless phrase queries. Seed each length with UNTOUCHED and replace only
    an unchanged sentinel with the string "untouched" in the returned mapping."""
    fn = k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))
    out = {}
    full, double = 'parse_more_full_pinyins', 'parse_more_double_pinyins'
    for label, parse, text, phrase, offset in (
            ('recursion-fails-1', full, b'nihao', '啊', 3), ('recursion-fails-2', full, b'nihao', '你', 5),
            ('recursion-fails-3', full, b'nihao', '好', 5), ('hit', full, b'nihao', '你好', 5),
            ('no-token', full, b'nihao', 'x', 2), ('empty-phrase', full, b'nihao', '', 2),
            ('no-parse', full, b'', '你好', 0), ('keyless', full, b"'", '你好', 0),
            ('double-fails', double, b'nihk', '啊', 2), ('double-hit', double, b'nihk', '你好', 4)):
        inst = k.alloc()
        k.fn(parse, Z, P, S)(inst, text)
        length = Z(UNTOUCHED)
        ret = fn(inst, phrase.encode(), offset, C.byref(length))
        out[label] = [ret, 'untouched' if length.value == UNTOUCHED else length.value]
    return out


@case('token-get-phrase-out', issues=('542',))
def _(k):
    """Query selected token IDs and preserve independent length and text sentinels.

    Return each lookup flag, length output and decoded phrase; free any
    returned non-NULL phrase without dereferencing an untouched pointer."""
    out = {}
    for token in (0xFFFFFFFF, 0x0DEADBEE, 0x01000000):
        length, text = U(UNTOUCHED), P(UNTOUCHED)
        ret = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
            k.inst, token, C.byref(length), C.byref(text))
        out[hex(token)] = [ret, 'untouched' if length.value == UNTOUCHED else length.value,
                           'untouched' if text.value == UNTOUCHED else k.text(text.value)]
    return out


class Arr(C.Structure):
    """View GLib's public GArray data pointer and unsigned element count via ctypes."""
    _fields_ = [('data', P), ('len', U)]


def tokens_of(k, phrase):
    """Copy phrase-token IDs from a newly allocated four-byte-element GArray.

    UTF-8 encode phrase and perform lookup on the cached instance. Ignore the
    lookup return flag and copy the array's reported contents into a Python
    list; the native array remains allocated."""
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    arr = new(0, 0, 4)
    k.fn('lookup_tokens', B, P, S, P)(k.inst, phrase.encode(), arr)
    view = C.cast(arr, C.POINTER(Arr)).contents
    return C.cast(view.data, C.POINTER(U))[:view.len]


@case('nth-pronunciation-range', issues=('542',))
def _(k):
    """Record pronunciation-array results at valid, boundary and out-of-range indices.

    Use the first token for 你好 and report its pronunciation count. Include
    raw two-byte key contents in ordinary observations only for in-range
    indices; retain every content dump under a tilde-prefixed diagnostic key,
    which observed excludes from comparison. Finally query an unknown token
    using a seeded array and record the resulting length. Native arrays are
    not freed here."""
    token = tokens_of(k, '你好')[0]
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    count = U(UNTOUCHED)
    k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(k.inst, token, C.byref(count))
    out = {'n': count.value}
    for nth in (0, count.value - 1, count.value, count.value + 1, 0xFFFFFFFF):
        keys = new(0, 0, 2)
        ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, nth, keys)
        view = C.cast(keys, C.POINTER(Arr)).contents
        content = C.string_at(view.data, view.len * 2).hex() if view.len else ''
        in_range = nth < count.value
        out['nth %d' % nth] = [ret, view.len] + ([content] if in_range else [])
        # Past the last reading the pin returns whatever its stack held; the
        # ruled answer is zeroed keys (class (b) for the content).
        out['~content nth %d' % nth] = content
    # An unknown token still empties the caller's array first.
    append = k.glib.g_array_append_vals
    append.restype, append.argtypes = P, [P, P, U]
    keys = new(0, 0, 2)
    seed = C.c_ushort(0x1234)
    append(keys, C.byref(seed), 1)
    ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, 0xFFFFFFFF, 0, keys)
    out['unknown token'] = [ret, C.cast(keys, C.POINTER(Arr)).contents.len]
    return out


# lane C, #542 rows 14 and 19: the pin's function-static slots and u16 length.
@case('static-key-slots', issues=('542',))
def _(k):
    """Observe key-pointer sharing and contents across two instances and one free.

    Parse ni and hao separately, fetch both key and rest pointers, compare
    addresses and snapshot the first pointers after the second lookup. Free
    the first instance and read those pointers again. This exercises the
    function-static slots in pinyin_get_pinyin_key and pinyin_get_pinyin_key_rest
    in src/pinyin.cpp at pin 074a2219; it assumes the returned pointers can
    be read for two and four bytes respectively."""
    first, second = k.alloc(), k.alloc()
    k.fn('parse_more_full_pinyins', Z, P, S)(first, b'ni')
    k.fn('parse_more_full_pinyins', Z, P, S)(second, b'hao')
    key = k.fn('get_pinyin_key', B, P, Z, C.POINTER(P))
    rest = k.fn('get_pinyin_key_rest', B, P, Z, C.POINTER(P))
    a, b, ra, rb = P(), P(), P(), P()
    out = {'ret': [key(first, 0, C.byref(a)), key(second, 0, C.byref(b)),
                   rest(first, 0, C.byref(ra)), rest(second, 0, C.byref(rb))]}
    out['key same pointer'] = a.value == b.value
    out['rest same pointer'] = ra.value == rb.value
    out['first key now'] = C.string_at(a.value, 2).hex()
    out['first rest now'] = C.string_at(ra.value, 4).hex()
    k.fn('free_instance', None, P)(first)
    out['key after free'] = C.string_at(a.value, 2).hex()
    out['rest after free'] = C.string_at(ra.value, 4).hex()
    return out


class Rest(C.Structure):
    """Mirror the two unsigned 16-bit raw-input offsets of ChewingKeyRest.

    The layout is _ChewingKeyRest in src/storage/chewing_key.h at pin
    074a2219; begin and end measure byte positions in the parsed input."""
    _fields_ = [('begin', C.c_ushort), ('end', C.c_ushort)]


@case('key-rest-length', issues=('542',))
def _(k):
    """Record return flags and 16-bit lengths for ordered, reversed and extreme offsets.

    Seed each length with 0xBEEF before calling get_pinyin_key_rest_length.
    The tested native calculation is _ChewingKeyRest::length in
    src/storage/chewing_key.h, exposed by pinyin_get_pinyin_key_rest_length
    in src/pinyin.cpp at pin 074a2219."""
    fn = k.fn('get_pinyin_key_rest_length', B, P, C.POINTER(Rest), C.POINTER(C.c_ushort))
    out = {}
    for begin, end in ((0, 2), (5, 2), (2, 2), (0, 65535), (65535, 0), (1, 65535)):
        length = C.c_ushort(0xBEEF)
        rest = Rest(begin, end)
        out['%d..%d' % (begin, end)] = [fn(k.inst, C.byref(rest), C.byref(length)), length.value]
    return out


# lane C, #542 rows 1, 3, 10, 21: return values of sentence and unload.
@case('sentence-before-guess', issues=('542',))
def _(k):
    """Observe sentence outputs as one instance is parsed, guessed, reset and reparsed.

    Query indices zero and 255 before the first guess, then index zero after
    each later transition, including a new parse following a guess."""
    out = {}
    inst = k.alloc()
    out['fresh'] = [sentence_out(k, inst, 0), sentence_out(k, inst, 255)]
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    out['parsed'] = [sentence_out(k, inst, 0), sentence_out(k, inst, 255)]
    k.fn('guess_sentence', B, P)(inst)
    out['guessed'] = sentence_out(k, inst, 0)
    k.fn('reset', B, P)(inst)
    out['reset'] = sentence_out(k, inst, 0)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    out['reparsed'] = sentence_out(k, inst, 0)
    # A parse alone keeps the rows of the last guess.
    k.fn('guess_sentence', B, P)(inst)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'xian')
    out['guessed then reparsed'] = sentence_out(k, inst, 0)
    return out


@case('sentence-before-guess-schemes', issues=('542',))
def _(k):
    """Read sentence zero after double-pinyin and chewing parses without guessing.

    Use a separate new instance for each parsing scheme."""
    out = {}
    inst = k.alloc()
    k.fn('parse_more_double_pinyins', Z, P, S)(inst, b'nihk')
    out['double'] = sentence_out(k, inst, 0)
    inst = k.alloc()
    k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
    out['chewing'] = sentence_out(k, inst, 0)
    return out


@case('guess-sentence-keyless', issues=('542',))
def _(k):
    """Record sentence guesses and outputs for separators, invalid input and a valid prefix.

    Use fresh instances for initial cases, then separately observe keyless
    reparsing after a guess and keyless guessing with the prefix 你. Return
    consumed byte counts for initial cases and preserve sentence sentinels."""
    out = {}
    for t in (b"'", b"''", b'', b'!', b"nihao'"):
        inst = k.alloc()
        n = k.fn('parse_more_full_pinyins', Z, P, S)(inst, t)
        ret = k.fn('guess_sentence', B, P)(inst)
        out[repr(t)] = [n, ret, sentence_out(k, inst, 0)]
    # A keyless guess answers false before it clears anything: the rows of
    # the last guess stay.
    for t in (b"'", b'!'):
        inst = k.alloc()
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
        first = k.fn('guess_sentence', B, P)(inst)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, t)
        out['stale after ' + repr(t)] = [first, k.fn('guess_sentence', B, P)(inst), sentence_out(k, inst, 0)]
        inst = k.alloc()
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, t)
        out['with-prefix ' + repr(t)] = [k.fn('guess_sentence_with_prefix', B, P, S)(inst, '你'.encode()),
                                         sentence_out(k, inst, 0)]
    return out


@case('guess-candidates-keyless', issues=('542',))
def _(k):
    """Record candidate-guess flags and whether any candidates remain after parsing.

    For each separator, invalid, empty or valid-prefix input, compare a fresh
    instance with one that has already parsed ni and attempted a sentence
    guess. Guess at byte offset zero with option mask 0x1E; reduce the returned
    candidate count to a nonzero boolean."""
    # `0 == matrix.size()`: the candidates are freed and the answer is
    # false, whatever sentence rows an earlier guess left.
    out = {}
    for t in (b"'", b"'''", b'!', b'', b"ni'"):
        for stale in (False, True):
            inst = k.alloc()
            if stale:
                k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
                k.fn('guess_sentence', B, P)(inst)
            k.fn('parse_more_full_pinyins', Z, P, S)(inst, t)
            ret = k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1E)
            count = U(UNTOUCHED)
            k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            out['%r stale=%s' % (t, stale)] = [ret, count.value > 0]
    return out


@case('unload-phrase-library-repeat', issues=('542',))
def _(k):
    """Return unload flags for repeated library 2 requests interleaved with other IDs.

    Issue all requests against the same cached context in the listed order."""
    unload = k.fn('unload_phrase_library', B, P, U)
    return {'unload': [unload(k.ctx, i) for i in (2, 2, 2, 1, 3, 15, 2)]}


@case('sentence-with-prefix-bytes', issues=('542',))
def _(k):
    """Observe prefix-based guessing with valid, empty and invalid UTF-8 prefixes.

    Parse nihao on a fresh instance for each prefix, then return the guess
    flag and sentence outputs at indices zero and one."""
    out = {}
    for label, prefix in (('invalid', b'\xff\xfe'), ('valid', '你'.encode()), ('empty', b''),
                          ('tail-invalid', '你'.encode() + b'\xff')):
        inst = k.alloc()
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
        ret = k.fn('guess_sentence_with_prefix', B, P, S)(inst, prefix)
        out[label] = [ret, sentence_out(k, inst, 0), sentence_out(k, inst, 1)]
    return out


# lane C, #542 rows 8 and 9: auxiliary text renders from the shared parse.
def aux_matrix(k, parse_name, text, cursors):
    """Collect all three auxiliary renderers at each byte cursor after the chosen parse.

    Allocate and parse a separate instance for each renderer/cursor pair.
    Return labels of the form renderer@cursor mapped to a flag and text,
    None or "untouched"; decode and free returned strings. cursors must be
    reiterable to visit the same positions for each renderer."""
    out = {}
    for fname in ('full_pinyin', 'double_pinyin', 'chewing'):
        for cursor in cursors:
            inst = k.alloc()
            k.fn(parse_name, Z, P, S)(inst, text)
            aux = P(UNTOUCHED)
            ret = k.fn('get_%s_auxiliary_text' % fname, B, P, Z, C.POINTER(P))(inst, cursor, C.byref(aux))
            out['%s@%d' % (fname, cursor)] = [ret, 'untouched' if aux.value == UNTOUCHED else k.text(aux.value)]
    return out


@case('aux-text-full-parse', issues=('542',))
def _(k):
    """Render nihao with all auxiliary renderers at boundaries, mid-key and past-end cursors.

    The selected byte cursors keep double-pinyin mid-key positions within
    its supported one- or two-byte split in pinyin_get_double_pinyin_auxiliary_text
    in src/pinyin.cpp at pin 074a2219."""
    # Mid-key cursors stay at one or two bytes into a key: the pin aborts on
    # a longer mid-key cursor of the double-pinyin renderer.
    return aux_matrix(k, 'parse_more_full_pinyins', b'nihao', (0, 1, 2, 3, 4, 5, 9))


@case('aux-text-full-parse-apostrophe', issues=('542',))
def _(k):
    """Collect all auxiliary renderers around the apostrophe in a full-pinyin xi'an parse."""
    return aux_matrix(k, 'parse_more_full_pinyins', b"xi'an", (0, 1, 2, 4, 5))


@case('aux-text-double-parse', issues=('542',))
def _(k):
    """Collect all auxiliary renderers at boundary, mid-key and past-end cursors for nihk."""
    return aux_matrix(k, 'parse_more_double_pinyins', b'nihk', (0, 1, 2, 3, 4, 9))


@case('aux-text-chewing-parse', issues=('542',))
def _(k):
    """Collect all auxiliary renderers across and beyond the chewing input su3cl3."""
    return aux_matrix(k, 'parse_more_chewings', b'su3cl3', (0, 1, 2, 3, 4, 5, 6, 9))


@case('aux-text-tones', issues=('542',))
def _(k):
    """Render tone-bearing full- and double-pinyin parses with all three auxiliary APIs.

    Return each flag/text pair under a label containing the parser, renderer
    and byte cursor, using a fresh instance for each observation."""
    # Tone digits ride the keys: the pinyin spelling appends them, the double
    # renderer appends them after a cut key, the zhuyin spelling marks them.
    out = {}
    for parse_name, text, cursors in (('parse_more_full_pinyins', b'ni3hao4', (0, 1, 2, 3, 4, 5, 7)),
                                      ('parse_more_double_pinyins', b'ni3hk4', (0, 1, 2, 3, 4, 5, 6))):
        for label, value in aux_matrix(k, parse_name, text, cursors).items():
            out[parse_name + ' ' + label] = value
    return out


def aux_cursors(k, text, cursors, functions=('full_pinyin', 'double_pinyin', 'chewing')):
    """Collect selected auxiliary renderers at byte cursors after a full-pinyin parse.

    Allocate a separate instance per renderer/cursor pair. Key the mapping
    by decoded input, renderer and cursor; values contain the return flag
    and decoded/freed text, None or "untouched". text must also decode as
    UTF-8 for labels, and cursors must be reiterable across renderers."""
    out = {}
    for fname in functions:
        for cursor in cursors:
            inst = k.alloc()
            k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
            aux = P(UNTOUCHED)
            ret = k.fn('get_%s_auxiliary_text' % fname, B, P, Z, C.POINTER(P))(inst, cursor, C.byref(aux))
            out['%s %s@%d' % (text.decode(), fname, cursor)] = [
                ret, 'untouched' if aux.value == UNTOUCHED else k.text(aux.value)]
    return out


def aux_options(k, word, texts):
    """Set context options and collect auxiliary text over each supplied byte string.

    For full-pinyin and chewing renderers, visit every byte cursor from zero
    through one past the input length. For double-pinyin, visit only zero,
    one, two, end and one past end. pinyin_get_double_pinyin_auxiliary_text
    in src/pinyin.cpp at pin 074a2219 aborts for mid-key splits beyond two
    bytes; the restricted cursors avoid those splits for the registered
    fixtures. Leave the option word set and merge the labelled observations."""
    k.fn('set_options', B, P, U)(k.ctx, word)
    out = {}
    for text in texts:
        every = range(0, len(text) + 2)
        out.update(aux_cursors(k, text, every, ('full_pinyin', 'chewing')))
        out.update(aux_cursors(k, text, (0, 1, 2, len(text), len(text) + 1), ('double_pinyin',)))
    return out


# Several keys can begin at one byte under the divided, resplit and fuzzy
# options; the pin's renderers read the first (`get_item(column, 0)`).
@case('aux-text-divided-table', issues=('542',))
def _(k):
    """Render the selected full-pinyin inputs with tone and divided-syllable options enabled."""
    return aux_options(k, (1 << 5) | (1 << 7), (b'xian', b'jiangnan', b'xianan', b'tiananmen'))


@case('aux-text-resplit-table', issues=('542',))
def _(k):
    """Render the selected full-pinyin inputs with tone and resplit options enabled."""
    return aux_options(k, (1 << 5) | (1 << 8), (b'zhengfu', b'xianan', b'dianxin', b'changan'))


@case('aux-text-divided-and-resplit', issues=('542',))
def _(k):
    """Render ambiguous full-pinyin inputs with tone, divided and resplit options together."""
    return aux_options(k, (1 << 5) | (1 << 7) | (1 << 8), (b'xian', b'zhengfu', b'xianan'))


@case('aux-text-fuzzy-keys', issues=('542',))
def _(k):
    """Render spellings with tone and Z/ZH, L/N and AN/ANG ambiguities enabled.

    Set the options through aux_options, retaining its restricted
    double-pinyin cursor set."""
    return aux_options(k, (1 << 5) | (1 << 12) | (1 << 15) | (1 << 17), (b'zongguo', b'nan', b'lan', b'zi', b'banggan'))


@case('aux-text-no-parse', control=True, issues=('542',))
def _(k):
    """Record all auxiliary renderers at cursor zero on separate unparsed instances.

    Preserve untouched output pointers and decode/free any returned text."""
    out = {}
    for fname in ('full_pinyin', 'double_pinyin', 'chewing'):
        inst = k.alloc()
        aux = P(UNTOUCHED)
        ret = k.fn('get_%s_auxiliary_text' % fname, B, P, Z, C.POINTER(P))(inst, 0, C.byref(aux))
        out[fname] = [ret, 'untouched' if aux.value == UNTOUCHED else k.text(aux.value)]
    return out


@case('aux-text-mixed-parses', issues=('542',))
def _(k):
    """Render one instance after double-pinyin parsing followed by full-pinyin parsing.

    At byte cursor two, return each auxiliary renderer's flag and text,
    None or untouched marker; decode and free returned strings."""
    # The matrix is the last parse, whatever mode made it.
    out = {}
    inst = k.alloc()
    k.fn('parse_more_double_pinyins', Z, P, S)(inst, b'nihk')
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'xian')
    for fname in ('full_pinyin', 'double_pinyin', 'chewing'):
        aux = P(UNTOUCHED)
        ret = k.fn('get_%s_auxiliary_text' % fname, B, P, Z, C.POINTER(P))(inst, 2, C.byref(aux))
        out[fname] = [ret, 'untouched' if aux.value == UNTOUCHED else k.text(aux.value)]
    return out


# batch2 group 7: the sub-index unigram total guard (PR 7, #540)
def choose_text(k, inst, offset, wanted):
    """Generate candidates at offset and choose the first whose UTF-8 text matches wanted.

    Use sort option zero and return the native selection result, or None if
    no text matches. Candidate-generation and accessor flags are not checked;
    the candidate list and selection state of inst can change."""
    k.fn('guess_candidates', B, P, Z, U)(inst, offset, 0)
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    for i in range(count.value):
        cand, text = P(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        if text.value == wanted.encode():
            return k.fn('choose_candidate', Z, P, Z, P)(inst, offset, cand)
    return None


def unigram_of(k, token):
    """Return the unigram lookup flag and raw unsigned frequency for a token.

    Seed the output with UNTOUCHED and retain that numeric value if unwritten.
    Use the kit's cached instance without changing the frequency here."""
    freq = U(UNTOUCHED)
    ret = k.fn('token_get_unigram_frequency', B, P, U, C.POINTER(U))(k.inst, token, C.byref(freq))
    return [ret, freq.value]


# #576: zhuyin.cpp:1813-1826 at 074a2219 reads the phrase item's
# stored unigram, including generation-time +1 and the in-memory overlay.
# e2853126 already ported the getter; keep the pinyin twin as a control.
def token_unigram_system(k):
    """Record a fixed system token's unigram before and after adding seven.

    Use token 0x01001225 from the oracle fixture, retain the add return flag,
    and also query the fixture's absent-item and absent-library IDs. The
    frequency update affects the same context used for all observations."""
    token = 0x01001225  # 你; the stored system field is 52887, not 52888.
    out = {'token': hex(token), 'before': unigram_of(k, token)}
    out['add'] = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, token, 7)
    out['after'] = unigram_of(k, token)
    out['absent-item'] = unigram_of(k, 0x01FFFFFF)
    out['absent-library'] = unigram_of(k, 0x0FFFFFFF)
    return out


def token_unigram_user(k, *, add_frequency=True):
    """Import a phrase into library 7 and observe its unigram, optionally adding seven.

    Use the facade's reading syntax and import count nine. Assert iterator
    creation, successful addition and exactly one matching library-7 token.
    Close the add iterator, then return the token and before/after lookups;
    include an add flag only when add_frequency is true."""
    phrase = '你好你好'
    reading = "ni3'hao3'ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ ㄋㄧˇ ㄏㄠˇ'
    iterator = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    assert iterator
    added = k.fn('iterator_add_phrase', B, P, S, S, I)(
        iterator, phrase.encode(), reading.encode(), 9)
    k.fn('end_add_phrases', None, P)(iterator)
    assert added
    tokens = [token for token in tokens_of(k, phrase) if token >> 24 == 7]
    assert len(tokens) == 1, tokens
    token = tokens[0]
    out = {'imported': added, 'token': hex(token), 'before': unigram_of(k, token)}
    if add_frequency:
        out['add'] = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, token, 7)
    out['after'] = unigram_of(k, token)
    return out


# #809: the ten zhuyin-only syllables (content_table keys that pinyin_index
# lacks) are stored as key words the pin hands back unfiltered
# (074a2219 zhuyin.cpp:1778-1811, phrase_index.cpp:33-44). Libraries 1-4 read
# the replaced item through the spelling round trip, which dropped them;
# libraries 5-7 read the user store and are the controls.
ZHUYIN_ONLY = {
    'chua': 'ㄔㄨㄚ', 'den': 'ㄉㄣ', 'din': 'ㄉㄧㄣ', 'fe': 'ㄈㄜ', 'kei': 'ㄎㄟ',
    'len': 'ㄌㄣ', 'nia': 'ㄋㄧㄚ', 'rua': 'ㄖㄨㄚ', 'yai': 'ㄧㄞ', 'zhei': 'ㄓㄟ',
}
ZHUYIN_ONLY_WORDS = ['龘鱻', '麤靐', '驫齉', '爨馕', '鬻厵', '龘麤', '鱻靐', '驫爨', '齉鬻', '馕厵']


def zhuyin_only_readings(k, libraries):
    """Import plain and third-tone readings for each zhuyin-only syllable and library.

    Use facade-specific spellings and count five, closing each add iterator.
    Return add flags, matching token IDs and up to four raw pronunciations as
    hex-encoded two-byte keys. Count and pronunciation keys share one row per
    library/syllable and are overwritten as matching tokens are visited.
    Accessor failures remain in the observations; allocated native arrays
    are not freed here."""
    out = {}
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    for library in libraries:
        for (pinyin, zhuyin), word in zip(ZHUYIN_ONLY.items(), ZHUYIN_ONLY_WORDS):
            if k.mode == 'pinyin':
                readings = ["%s'%s" % (pinyin, pinyin), "%s3'%s3" % (pinyin, pinyin)]
            else:
                readings = ['%s %s' % (zhuyin, zhuyin), '%sˇ %sˇ' % (zhuyin, zhuyin)]
            iterator = k.fn('begin_add_phrases', P, P, U)(k.ctx, library)
            added = [k.fn('iterator_add_phrase', B, P, S, S, I)(iterator, word.encode(), r.encode(), 5)
                     for r in readings]
            k.fn('end_add_phrases', None, P)(iterator)
            row = {'added': added}
            tokens = [token for token in tokens_of(k, word) if token >> 24 == library]
            row['tokens'] = [hex(token) for token in tokens]
            for token in tokens:
                count = U(UNTOUCHED)
                row['n_pron'] = [k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(
                    k.inst, token, C.byref(count)), count.value]
                for nth in range(min(count.value, 4)):
                    keys = new(0, 0, 2)
                    ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, nth, keys)
                    view = C.cast(keys, C.POINTER(Arr)).contents
                    row['nth %d' % nth] = [ret, C.string_at(view.data, view.len * 2).hex() if view.len else '']
            out['lib %d %s' % (library, pinyin)] = row
    return out


for _mode in ('pinyin', 'zhuyin'):
    _prefix = 'zhuyin-' if _mode == 'zhuyin' else ''
    case(_prefix + 'system-zhuyin-only-readings', mode=_mode, issues=('809',))(
        lambda k: zhuyin_only_readings(k, (1, 2, 3, 4)))
    case(_prefix + 'user-zhuyin-only-readings', mode=_mode, control=True, issues=('809',))(
        lambda k: zhuyin_only_readings(k, (5, 6, 7)))


# #808: the default facade's USER_FILE libraries 5 (ADDON), 6 (NETWORK) and 7
# (USER) are read from m_phrase_index (074a2219 pinyin.cpp:2821-2834 via
# phrase_index.h:646-652): true with the stored field for a real token, false
# with 0 for an id that owns no item. The pinyin getter used to read the addon
# facade for 5 and 6 and never checked the item for 7.
def token_unigram_user_libraries(k):
    """Plant phrases in libraries 5, 6 and 7 and observe unigram updates and nearby IDs.

    Assert each import succeeds and yields exactly one token in its requested
    library. Record that token's lookup, three presumed-absent neighbors, the
    return flag from adding seven and the subsequent lookup. Imports use
    count nine and close their add iterators; all libraries share the context."""
    out = {}
    plants = {5: '你好你好', 6: '再见再见', 7: '你好你好呀'}
    readings = {
        'pinyin': {5: "ni3'hao3'ni3'hao3", 6: "zai4'jian4'zai4'jian4", 7: "ni3'hao3'ni3'hao3'ya1"},
        'zhuyin': {5: 'ㄋㄧˇ ㄏㄠˇ ㄋㄧˇ ㄏㄠˇ', 6: 'ㄗㄞˋ ㄐㄧㄢˋ ㄗㄞˋ ㄐㄧㄢˋ',
                   7: 'ㄋㄧˇ ㄏㄠˇ ㄋㄧˇ ㄏㄠˇ ㄧㄚ'},
    }[k.mode]
    for library, phrase in plants.items():
        iterator = k.fn('begin_add_phrases', P, P, U)(k.ctx, library)
        assert iterator
        added = k.fn('iterator_add_phrase', B, P, S, S, I)(
            iterator, phrase.encode(), readings[library].encode(), 9)
        k.fn('end_add_phrases', None, P)(iterator)
        assert added
        tokens = [token for token in tokens_of(k, phrase) if token >> 24 == library]
        assert len(tokens) == 1, tokens
        token = tokens[0]
        out['lib %d token' % library] = hex(token)
        out['lib %d item' % library] = unigram_of(k, token)
        for delta in (0, 2, 100):
            absent = (library << 24) | ((token & 0xFFFFFF) + delta) if delta else token - 1
            out['lib %d absent %#x' % (library, absent)] = unigram_of(k, absent)
        out['lib %d add' % library] = k.fn(
            'token_add_unigram_frequency', B, P, U, U)(k.inst, token, 7)
        out['lib %d after' % library] = unigram_of(k, token)
    return out


def train_selected_top(k, wanted, token, guess_before_choose=False):
    """Choose a normal phrase, guess n-best, then train result 0 without reset.

    This test-only sequence preserves the runbook's train_top precondition:
    a candidate list alone cannot be trained. The normal selection also
    installs the OneStep constraint that train_result3 needs to write counts.

    Require a parsed cached instance with the wanted normal candidate and
    a user profile for training. Assert candidate/accessor success, positive
    selection cursor, a successful sentence guess and successful training;
    raise AssertionError if no matching kind-2 row exists. Return selection
    and training details plus the token's unigram immediately after choosing.
    With guess_before_choose, also guess and snapshot sentences before the
    selection, using forked sentence reads for pinyin. Selection and training
    mutate the cached instance and its context without resetting them.

    The constraint path is pinyin_choose_candidate in src/pinyin.cpp and
    zhuyin_choose_candidate in src/zhuyin.cpp at pin 074a2219; the count
    updates are in PhoneticLookup::train_result3 in src/lookup/phonetic_lookup.h.
    """
    inst = k.inst
    if k.mode == 'pinyin':
        # SORT_WITHOUT_SENTENCE (0x1) would train during choose instead
        # of installing a constraint (074a2219 pinyin.cpp:2566-2576).
        assert k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1e)
    else:
        assert k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    count = U()
    assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    for index in range(count.value):
        candidate, text, kind = P(), S(), I()
        assert k.fn('get_candidate', B, P, U, C.POINTER(P))(
            inst, index, C.byref(candidate))
        assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(
            inst, candidate, C.byref(text))
        assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(
            inst, candidate, C.byref(kind))
        # NORMAL_CANDIDATE / NORMAL_CANDIDATE_AFTER_CURSOR are both 2.
        # The zhuyin BEST_MATCH row installs no constraint (074a2219
        # zhuyin.cpp:1643-1654), so selecting it would make this a no-op.
        if kind.value == 2 and text.value == wanted.encode():
            before = {}
            if guess_before_choose:
                # Retain this NORMAL row before guessing promotes matching
                # text to NBEST in a newly generated candidate list. Guess
                # leaves m_candidates intact (074a2219 pinyin.cpp:1372-1386).
                assert k.fn('guess_sentence', B, P)(inst)
                before['before-choose'] = final_step_sentences(k, inst)
            cursor = k.fn('choose_candidate', I, P, Z, P)(inst, 0, candidate)
            assert cursor > 0, 'normal phrase selection failed'
            after_choose = unigram_of(k, token)
            guessed = k.fn('guess_sentence', B, P)(inst)
            assert guessed, 'training needs a guessed n-best result'
            if k.mode == 'pinyin':
                trained = k.fn('train', B, P, C.c_ubyte)(inst, 0)
            else:
                trained = k.fn('train', B, P)(inst)
            assert trained, 'training n-best result 0 failed'
            return {'chosen': wanted, 'kind': kind.value, 'cursor': cursor,
                    'after-choose': after_choose, 'guess': guessed, 'train': trained, **before}
    raise AssertionError('missing normal candidate: ' + wanted)


def token_unigram_trained(k):
    """Record a fixed phrase's unigram around one constrained training operation.

    Require the oracle token for 癫痫 and a complete parse of the facade's
    input bytes. Select its normal candidate and train result zero through
    train_selected_top, asserting that selection alone did not alter the
    unigram. Return the consumed byte count, training details and before/after
    lookups; training modifies the context's learned state."""
    token = 0x01003e57  # 癫痫: one constrained train adds 69 * 7 = 483.
    assert tokens_of(k, '癫痫') == [token]
    parse = 'parse_more_full_pinyins' if k.mode == 'pinyin' else 'parse_more_chewings'
    text = b'dian4xian4' if k.mode == 'pinyin' else b'2u04vu04'
    parsed = k.fn(parse, Z, P, S)(k.inst, text)
    assert parsed == len(text), 'the whole training input must parse'
    out = {'token': hex(token), 'parsed': parsed, 'before': unigram_of(k, token)}
    out['training'] = train_selected_top(k, '癫痫', token)
    assert out['training']['after-choose'] == out['before'], 'selection must not train'
    out['after'] = unigram_of(k, token)
    return out


for _mode in ('pinyin', 'zhuyin'):
    _prefix = 'zhuyin-' if _mode == 'zhuyin' else ''
    case(_prefix + 'token-unigram-system', mode=_mode, control=_mode == 'pinyin', issues=('808',))(
        token_unigram_system)
    case(_prefix + 'token-unigram-user', mode=_mode, control=_mode == 'pinyin', issues=('808',))(
        token_unigram_user)
    case(_prefix + 'token-unigram-trained', mode=_mode, control=_mode == 'pinyin', issues=('808',))(
        token_unigram_trained)
    case(_prefix + 'token-unigram-user-libraries', mode=_mode, control=_mode == 'zhuyin', issues=('808',))(
        token_unigram_user_libraries)


@case('train-unigram-total', issues=('540',))
def _(k):
    """Record unigram growth and training return flags at cumulative training checkpoints.

    Parse nihao, choose 你 and 好 where available, and guess again on the
    cached instance. Train result zero 30,000 times without resetting, recording
    at 1, 10,000, 20,000 and 30,000 calls. Each checkpoint stores sorted distinct
    return flags and both unigram lookups; selection results are recorded
    rather than asserted."""
    # Once the library's guint32 total would overflow the item stops growing:
    # at the pin 你 freezes at 2122897296 after about 20000 trainings.
    inst = k.inst
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    ni, hao = tokens_of(k, '你')[0], tokens_of(k, '好')[0]
    first = choose_text(k, inst, 0, '你')
    second = choose_text(k, inst, first, '好') if first else None
    k.fn('guess_sentence', B, P)(inst)
    out = {'chosen': [first, second], 'before': [unigram_of(k, ni), unigram_of(k, hao)]}
    train = k.fn('train', B, P, C.c_ubyte)
    for step, count in ((1, 1), (2, 9999), (3, 10000), (4, 10000)):
        rets = {train(inst, 0) for _ in range(count)}
        out['after %d' % sum(c for s, c in ((1, 1), (2, 9999), (3, 10000), (4, 10000)) if s <= step)] = [
            sorted(rets), unigram_of(k, ni), unigram_of(k, hao)]
    return out


@case('predicted-unigram-overflow', issues=('540',))
def _(k):
    """Count repeated predicted-candidate acceptances after 20,000 training calls.

    Seed the cached instance with nihao, choose 你 and 好 where available and
    train result zero. Generate predictions for 你 and find the first kind-5
    row whose first text token belongs to library 1. Return chosen offsets
    and whether a row was found; if found, count successful selections up to
    the first false return or 1,000 successes. The reported count can therefore
    be capped without observing a refusal. Training and selection mutate the
    shared context, and the same candidate pointer is reused."""
    # Training 你 and 好 fills library 1 to within one seed of its guint32
    # total; accepted predicted candidates (483 each) then run it out, and
    # the pin answers false from the call that no longer fits.
    inst = k.inst
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    first = choose_text(k, inst, 0, '你')
    second = choose_text(k, inst, first, '好') if first else None
    k.fn('guess_sentence', B, P)(inst)
    train = k.fn('train', B, P, C.c_ubyte)
    for _ in range(20000):
        train(inst, 0)
    k.fn('guess_predicted_candidates', B, P, S)(inst, '你'.encode())
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    pick = None
    for i in range(count.value):
        cand, kind, text = P(), I(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        if kind.value == 5 and (tokens_of(k, text.value.decode()) or [0])[0] >> 24 == 1:
            pick = cand
            break
    out = {'chosen': [first, second], 'found': pick is not None}
    if pick is None:
        return out
    accepted = 0
    choose = k.fn('choose_predicted_candidate', B, P, P)
    while accepted < 1000 and choose(inst, pick):
        accepted += 1
    out['accepted before the first refusal'] = accepted
    return out


# batch2 group 8: choose_predicted_candidate's return table (PR 8, #542, #540)
def predicted_rows(k, inst, prefix, limit=12):
    """Generate predictions including punctuation and return leading rows plus total count.

    UTF-8 encode prefix and replace inst's candidate list. Return a pair of
    [type, decoded text] rows and the full native count, limiting rows to
    limit (none when limit is nonpositive). Native success flags are not
    checked, and text decoding is strict UTF-8."""
    k.fn('guess_predicted_candidates_with_punctuations', B, P, S)(inst, prefix.encode())
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    rows = []
    for i in range(min(count.value, limit)):
        cand, kind, text = P(), I(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        rows.append([kind.value, text.value.decode()])
    return rows, count.value


def choose_predicted_of(k, inst, want_kind, skip=0):
    """Choose a matching kind from the existing candidate list and return its boolean result.

    Skip the first skip matching rows; return None if no eligible row remains.
    A negative skip keeps skipping all matches. This helper does not regenerate
    the list or check accessor flags; choosing can update learned state."""
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    for i in range(count.value):
        cand, kind = P(), I()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        if kind.value == want_kind:
            if skip:
                skip -= 1
                continue
            return k.fn('choose_predicted_candidate', B, P, P)(inst, cand)
    return None


@case('choose-predicted-types', issues=('542', '540'))
def _(k):
    """Record predictions for 我, then select punctuation twice and a prefix row.

    Keep the initial rows/count and each selection result. All selections
    reuse the cached instance's generated candidate list, with kinds 8 and 5."""
    # The return table: a punctuation row has no frequency and answers true,
    # a prefix row answers true. (The prefix row's bigram is not compared:
    # the pin trains its unigram only.)
    out = {}
    inst = k.inst
    out['before'] = predicted_rows(k, inst, '我')
    out['punctuation'] = choose_predicted_of(k, inst, 8)
    out['punctuation again'] = choose_predicted_of(k, inst, 8)
    out['prefix'] = choose_predicted_of(k, inst, 5)
    return out


def remember_negative(count):
    """Build a phrase-remembering case with the supplied signed count, including controls.

    Pass count as a C signed int; negative and positive controls share the
    same sequence."""
    def run(k):
        """Remember 你好世界 after parsing its input and enumerate library 7.

        Return the remember flag and phrase/reading/count rows after the attempt.
        Decode and free exported strings; leave the get iterator allocated and
        the cached instance's parse and user-library changes in place."""
        inst = k.inst
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihaoshijie')
        out = {'remember': k.fn('remember_user_input', B, P, S, I)(inst, '你好世界'.encode(), count)}
        it = k.fn('begin_get_phrases', P, P, U)(k.ctx, 7)
        has = k.fn('iterator_has_next_phrase', B, P)
        nxt = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))
        rows = []
        while has(it):
            ph, py, n = P(), P(), I()
            nxt(it, C.byref(ph), C.byref(py), C.byref(n))
            rows.append([k.text(ph.value), k.text(py.value), n.value])
        out['rows'] = rows
        return out
    return run


for _count in (-2, -5, -2147483648):
    case('remember-count-%d' % -_count if _count > -100 else 'remember-count-min')(remember_negative(_count))
case('remember-count-default', control=True)(remember_negative(-1))
case('remember-count-seven', control=True)(remember_negative(7))


# batch2 group 9: the input length cap (PR 9, #540)
@case('input-length-cap', issues=('540',))
def _(k):
    """Record parser and stored lengths for six ASCII inputs around size boundaries.

    Create a fresh instance for each byte length from 4,096 through 65,537
    listed in the fixture. Repeat ni and truncate to that byte length, then
    return both native byte counts under its decimal label."""
    out = {}
    for length in (4096, 4097, 32766, 32767, 32768, 65537):
        inst = k.alloc()
        text = (b'ni' * length)[:length]
        out[str(length)] = [k.fn('parse_more_full_pinyins', Z, P, S)(inst, text),
                            k.fn('get_parsed_input_length', Z, P)(inst)]
    return out


@case('long-input-sentence', issues=('540',))
def _(k):
    """Parse 4,800 bytes of repeated ni and record guessing and sentence-length outputs.

    Use a fresh instance and sentence index zero. Return parsed bytes, the
    guess flag, and the sentence-accessor flag plus Python string length or
    None. Any string returned by sentence_out, including its untouched marker,
    is reduced to a length; decoded lengths count code points, not UTF-8 bytes."""
    # A decode past the old cap: the sentence is as long as the pin's.
    inst = k.alloc()
    text = b'ni' * 2400
    out = {'parsed': k.fn('parse_more_full_pinyins', Z, P, S)(inst, text),
           'guess': k.fn('guess_sentence', B, P)(inst)}
    row = sentence_out(k, inst, 0)
    out['sentence'] = [row[0], len(row[1]) if isinstance(row[1], str) else row[1]]
    return out


# batch2 group 10: pinyin_alloc_instance on a finalised context (PR 10, #528)
@case('alloc-instance-after-fini', issues=('528',))
def _(k):
    """Observe a native allocation attempt after freeing an instance and finalizing its context.

    Create an independent context and record whether its initial instance was
    non-NULL. Keep the post-finalization pointer's truth value only under a
    tilde-prefixed diagnostic key, excluded by observed. Do not free that
    second pointer. This is an isolated-worker case: the invalid lifetime
    access can terminate the process before an observation is returned.
    The pin path is pinyin_alloc_instance after pinyin_fini in src/pinyin.cpp
    at 074a2219."""
    # The pin reads the freed context (pinyin.cpp:1322) and survives by
    # chance; the ruled answer here is NULL (class (b)), so the answer itself
    # is left out of the comparison and the case holds the exit status: the
    # parent build crashes where the pin does not.
    context = k.init()
    live = k.fn('alloc_instance', P, P)(context)
    k.fn('free_instance', None, P)(live)
    k.fn('fini', None, P)(context)
    after = k.fn('alloc_instance', P, P)(context)
    return {'live instance': bool(live), '~instance after fini': bool(after)}


# #573 (B battery): a second pinyin_fini / zhuyin_fini on one context
def fini_twice(k):
    """Finalize one independently initialized context twice and report only if both calls return.

    Allocate and free a live instance before finalization. This deliberately
    reuses the context address and can terminate the isolated worker. The
    lifetime violation targets pinyin_fini in src/pinyin.cpp and zhuyin_fini
    in src/zhuyin.cpp at pin 074a2219, both of which delete the context."""
    # The pin reads and deletes the freed context again (pinyin.cpp:1194-1222,
    # zhuyin.cpp:741-757) and dies of SIGSEGV, with stderr bytes read from
    # freed memory; the ruled answer is a silent return (class (b), row 105).
    context = k.init()
    live = k.fn('alloc_instance', P, P)(context)
    k.fn('free_instance', None, P)(live)
    k.fn('fini', None, P)(context)
    k.fn('fini', None, P)(context)
    return {'second fini returned': True}


case('fini-twice', crash={'second fini returned': True}, stderr_varies=True, issues=('573',))(fini_twice)
case('zhuyin-fini-twice', mode='zhuyin', crash={'second fini returned': True}, stderr_varies=True, issues=('573',))(fini_twice)


# batch2 group 11: the facade unigram total wraps like the pin's (PR 11, #540)
FACADE_TOTAL = 51051831   # the oracle data's loaded libraries, summed


def wrap_total_to_zero(k, inst):
    """Add the fixture-derived delta intended to wrap the facade's 32-bit total to zero.

    Use token (1 << 24) | 1 and return the native add flag. Assume the loaded
    context total equals FACADE_TOTAL; do not measure it or verify the result.
    FacadePhraseIndex::add_unigram_frequency in src/storage/phrase_index.h
    at pin 074a2219 updates its unsigned total before delegating to
    SubPhraseIndex::add_unigram_frequency in src/storage/phrase_index.cpp,
    so sub-index refusal need not undo the facade-total change."""
    add = k.fn('token_add_unigram_frequency', B, P, U, U)
    return add(inst, (1 << 24) | 1, (2 ** 32 - FACADE_TOTAL) % 2 ** 32)


# The zero total aborts the pin only while a searched row is ranked
# (`_compute_frequency_of_items`, `pinyin.cpp:1859`): a window with no phrase
# row — the reserved slot, a mid-key offset — answers true with no warning.
# One case each, since the wrapped total is the context's.
for _label, _offset, _sort in (('end-sort-3', 5, 3), ('mid-key-sort-3', 3, 3), ('end-sort-2', 5, 2)):
    def _no_rank(offset, sort):
        """Capture the byte offset and sort options for a zero-total candidate case."""
        def run(k):
            """Attempt candidate guessing after parsing nihao and applying the total-wrap delta.

            Allocate a fresh instance, ignore the add flag, and return the guess flag
            and candidate count for the captured offset and sort. The context's
            frequency state is changed and the instance is not freed here."""
            inst = k.alloc()
            k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
            wrap_total_to_zero(k, inst)
            count = U()
            ret = k.fn('guess_candidates', B, P, Z, U)(inst, offset, sort)
            k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            return {'ret': ret, 'rows': count.value}
        return run
    case('facade-total-zero-ranks-nothing-' + _label, control=True, issues=('540',))(_no_rank(_offset, _sort))


@case('facade-total-zero', abort=False, issues=('540',))
def _(k):
    """Apply the total-wrap delta and record candidate guessing at byte offset zero.

    Return the add and guess flags if the native calls return. The selected
    window reaches _compute_frequency_of_items in src/pinyin.cpp at pin
    074a2219, where a zero total asserts; the registered comparison expects
    SIGABRT from the pin and a false guess result from the subject."""
    # `assert(0 < total_freq)` while ranking a candidate (pinyin.cpp:1859).
    inst = k.inst
    added = wrap_total_to_zero(k, inst)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    return {'add': added, 'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1E)}


# batch2 group 12a: library-index, scheme and iterator refusals (PR 12a, #525)
def ctx_call(name, res, *args):
    """Build a context-call case from a return type and (ctypes type, value) pairs.

    Capture the export name and arguments without loading a library or calling it."""
    def run(k):
        """Call the captured export on the cached context and return its result as ret."""
        return {'ret': k.fn(name, res, P, *[t for t, _ in args])(k.ctx, *[v for _, v in args])}
    return run


for _name, _call in (
        ('set-full-pinyin-scheme-0', ctx_call('set_full_pinyin_scheme', B, (I, 0))),
        ('set-full-pinyin-scheme-4', ctx_call('set_full_pinyin_scheme', B, (I, 4))),
        ('set-double-pinyin-scheme-30', ctx_call('set_double_pinyin_scheme', B, (I, 30))),
        ('set-zhuyin-scheme-0', ctx_call('set_zhuyin_scheme', B, (I, 0))),
        ('set-zhuyin-scheme-10', ctx_call('set_zhuyin_scheme', B, (I, 10))),
        ('set-zhuyin-scheme-7', ctx_call('set_zhuyin_scheme', B, (I, 7))),
        ('load-phrase-library-0', ctx_call('load_phrase_library', B, (C.c_ubyte, 0))),
        ('load-phrase-library-8', ctx_call('load_phrase_library', B, (C.c_ubyte, 8))),
        ('unload-phrase-library-16', ctx_call('unload_phrase_library', B, (C.c_ubyte, 16))),
        ('unload-addon-phrase-library-16', ctx_call('unload_addon_phrase_library', B, (C.c_ubyte, 16)))):
    case('abort-' + _name, abort=False, issues=('525',))(_call)


@case('abort-iterator-get-next-phrase', abort=False, issues=('525',))
def _(k):
    """Request a phrase from an empty library-7 iterator and record both iterator flags.

    Call get-next even when has-next is false; output pointers are not observed
    or freed, and the iterator is left allocated for the isolated worker."""
    # An empty user dictionary: the begin probes no next token.
    it = k.fn('begin_get_phrases', P, P, U)(k.ctx, 7)
    phrase, pinyin, count = P(), P(), I()
    return {'has next': k.fn('iterator_has_next_phrase', B, P)(it),
            'ret': k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
                it, C.byref(phrase), C.byref(pinyin), C.byref(count))}


# #681, row 62: the bigram twin. A fresh user dir has an empty user bigram,
# so the iterator's predecessor is still null_token and the pin asserts
# (`pinyin.cpp:902`) before it writes anything. A control: the parent build
# already answers this way; the case registers behaviour, it changes none.
@case('abort-bigram-iterator-get-next-phrase', control=True, abort=False, issues=('681', 'row:62'))
def _(k):
    """Request a row from a fresh bigram iterator and check that all outputs stay untouched.

    Record has-next, the get-next flag and separate sentinel checks for phrase,
    reading and count, even when no next row is reported."""
    it = k.fn('begin_get_bigram_phrases', P, P)(k.ctx)
    phrase, pinyin, count = P(UNTOUCHED), P(UNTOUCHED), I(UNTOUCHED)
    has_next = k.fn('bigram_iterator_has_next_phrase', B, P)(it)
    ret = k.fn('bigram_iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
        it, C.byref(phrase), C.byref(pinyin), C.byref(count))
    return {'has next': has_next, 'ret': ret,
            'untouched phrase': phrase.value == UNTOUCHED,
            'untouched pinyin': pinyin.value == UNTOUCHED,
            'untouched count': count.value == UNTOUCHED}


# The in-range neighbours answer without a warning, as at the pin.
@case('scheme-and-library-neighbours', control=True, issues=('525',))
def _(k):
    """Record scheme changes and library load/unload flags for the listed boundary neighbors.

    Apply all operations in order to the same context, retaining its mutations."""
    out = {}
    for label, name, res, arg in (
            ('full 1', 'set_full_pinyin_scheme', B, (I, 1)), ('full 3', 'set_full_pinyin_scheme', B, (I, 3)),
            ('double 6', 'set_double_pinyin_scheme', B, (I, 6)), ('double 99', 'set_double_pinyin_scheme', B, (I, 99)),
            ('zhuyin 1', 'set_zhuyin_scheme', B, (I, 1)), ('zhuyin 9', 'set_zhuyin_scheme', B, (I, 9)),
            ('load 1', 'load_phrase_library', B, (C.c_ubyte, 1)), ('load 7', 'load_phrase_library', B, (C.c_ubyte, 7)),
            ('load 16', 'load_phrase_library', B, (C.c_ubyte, 16)), ('load 255', 'load_phrase_library', B, (C.c_ubyte, 255)),
            ('unload 1', 'unload_phrase_library', B, (C.c_ubyte, 1)), ('unload 15', 'unload_phrase_library', B, (C.c_ubyte, 15)),
            ('unload addon 15', 'unload_addon_phrase_library', B, (C.c_ubyte, 15)),
            ('load addon 16', 'load_addon_phrase_library', B, (C.c_ubyte, 16))):
        out[label] = k.fn(name, res, P, arg[0])(k.ctx, arg[1])
    return out


# batch2 group 12b: cursor, offset, sentence-index and aux-text refusals (PR 12b, #525)
def full_inst(k, text):
    """Allocate an instance, parse full-pinyin bytes and return the live instance.

    Ignore the consumed-byte result; ownership of the new instance stays with
    the caller."""
    inst = k.alloc()
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
    return inst


@case('abort-get-sentence-past-rows', abort=False, issues=('525',))
def _(k):
    """Guess nihao, then record only the sentence-accessor flag for index five."""
    inst = full_inst(k, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    return {'ret': sentence_out(k, inst, 5)[0]}


@case('abort-get-left-pinyin-offset-after-separator', abort=False, issues=('525',))
def _(k):
    """Query the left offset at byte three after parsing ni'hao, recording only the flag."""
    inst = full_inst(k, b"ni'hao")
    out = Z(UNTOUCHED)
    return {'ret': k.fn('get_left_pinyin_offset', B, P, Z, C.POINTER(Z))(inst, 3, C.byref(out))}


@case('abort-get-right-pinyin-offset-after-separator', abort=False, issues=('525',))
def _(k):
    """Query the right offset at byte three after parsing ni'hao, recording only the flag."""
    inst = full_inst(k, b"ni'hao")
    out = Z(UNTOUCHED)
    return {'ret': k.fn('get_right_pinyin_offset', B, P, Z, C.POINTER(Z))(inst, 3, C.byref(out))}


@case('abort-get-right-pinyin-offset-nihao-5', abort=False, issues=('525',))
def _(k):
    """Query the right offset at byte five of nihao, discarding the seeded output offset."""
    inst = full_inst(k, b'nihao')
    out = Z(UNTOUCHED)
    return {'ret': k.fn('get_right_pinyin_offset', B, P, Z, C.POINTER(Z))(inst, 5, C.byref(out))}


@case('abort-get-character-offset-past-matrix', abort=False, issues=('525',))
def _(k):
    """Query 你好's character offset at byte 99 of nihao and return only the native flag."""
    inst = full_inst(k, b'nihao')
    out = Z(UNTOUCHED)
    return {'ret': k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))(
        inst, '你好'.encode(), 99, C.byref(out))}


@case('abort-get-character-offset-after-separator', abort=False, issues=('525',))
def _(k):
    """Query 你好's character offset at byte three of ni'hao and return only the flag."""
    inst = full_inst(k, b"ni'hao")
    out = Z(UNTOUCHED)
    return {'ret': k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))(
        inst, '你好'.encode(), 3, C.byref(out))}


@case('abort-parse-full-pinyin-apostrophe', abort=False, issues=('525',))
def _(k):
    """Pass n'i to the single-key parser and record its flag without reading the output key."""
    key = C.c_uint16(UNTOUCHED & 0xFFFF)
    return {'ret': k.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_uint16))(
        k.inst, b"n'i", C.byref(key))}


@case('abort-get-pinyin-key-empty-matrix', abort=False, issues=('525',))
def _(k):
    """Request key zero on an unparsed instance and record only the native flag."""
    out = P(UNTOUCHED)
    return {'ret': k.fn('get_pinyin_key', B, P, Z, C.POINTER(P))(k.alloc(), 0, C.byref(out))}


@case('abort-get-pinyin-key-rest-empty-matrix', abort=False, issues=('525',))
def _(k):
    """Request key-rest zero on an unparsed instance and record only the native flag."""
    out = P(UNTOUCHED)
    return {'ret': k.fn('get_pinyin_key_rest', B, P, Z, C.POINTER(P))(k.alloc(), 0, C.byref(out))}


@case('abort-double-auxiliary-text-cut-three-bytes-in', abort=False, issues=('525',))
def _(k):
    """Render double-pinyin auxiliary text three bytes into zhong and retain only the flag.

    Seed the output pointer with UNTOUCHED but do not read or free its value."""
    inst = full_inst(k, b'zhong')
    aux = P(UNTOUCHED)
    return {'ret': k.fn('get_double_pinyin_auxiliary_text', B, P, Z, C.POINTER(P))(inst, 3, C.byref(aux))}


# A leading `'` leaves column 0 empty at the pin and the aux walk asserts
# `get_column_size(offset) >= 1` (`pinyin.cpp:3311`) on every cursor.
def leading_apostrophe_aux(fname, text=b"'nihao"):
    """Capture a renderer suffix and full-pinyin bytes for a cursor-two auxiliary case."""
    def run(k):
        """Parse the captured bytes on a new instance and record the renderer's flag.

        Call get_<suffix>_auxiliary_text at byte cursor two with a seeded pointer;
        the pointer's value is neither compared nor freed."""
        inst = full_inst(k, text)
        aux = P(UNTOUCHED)
        return {'ret': k.fn('get_%s_auxiliary_text' % fname, B, P, Z, C.POINTER(P))(inst, 2, C.byref(aux))}
    return run


for _fname in ('full_pinyin', 'double_pinyin', 'chewing'):
    case('abort-%s-auxiliary-text-leading-apostrophe' % _fname.replace('_', '-'), abort=False, issues=('525',))(
        leading_apostrophe_aux(_fname))
case('abort-full-pinyin-auxiliary-text-doubled-leading-apostrophe', abort=False, issues=('525',))(
    leading_apostrophe_aux('full_pinyin', b"''ni"))


# The graceful neighbours of the sites above answer without a warning.
@case('cursor-and-sentence-neighbours', control=True, issues=('525',))
def _(k):
    """Collect sentence, cursor, key and auxiliary outputs for neighboring valid calls.

    Use guessed nihao and a separate unparsed instance. Preserve numeric-output
    sentinels, report key-pointer nullness and decode/free returned auxiliary text."""
    out = {}
    inst = full_inst(k, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    out['sentence 0'] = sentence_out(k, inst, 0)
    fresh = k.alloc()
    out['sentence of fresh'] = sentence_out(k, fresh, 3)
    for label, name, offset in (('left 2', 'get_left_pinyin_offset', 2), ('right 2', 'get_right_pinyin_offset', 2),
                                ('right 4', 'get_right_pinyin_offset', 4)):
        res = Z(UNTOUCHED)
        out[label] = [k.fn(name, B, P, Z, C.POINTER(Z))(inst, offset, C.byref(res)),
                      'untouched' if res.value == UNTOUCHED else res.value]
    key = C.c_uint16(UNTOUCHED & 0xFFFF)
    out['parse ni'] = [k.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_uint16))(inst, b'ni', C.byref(key)), key.value]
    for offset in (0, 1):
        ptr = P(UNTOUCHED)
        ret = k.fn('get_pinyin_key', B, P, Z, C.POINTER(P))(inst, offset, C.byref(ptr))
        out['key %d' % offset] = [ret, ptr.value is None]
    for cursor in (1, 2, 5):
        aux = P(UNTOUCHED)
        ret = k.fn('get_double_pinyin_auxiliary_text', B, P, Z, C.POINTER(P))(inst, cursor, C.byref(aux))
        out['double aux %d' % cursor] = [ret, k.text(aux.value) if aux.value != UNTOUCHED else 'untouched']
    return out


# batch2 group 12c: candidate-type refusals (PR 12c, #525)
def row_of(k, inst, want_kind):
    """Return the first borrowed candidate pointer of the requested kind, or None.

    Scan the existing list without regenerating it or checking accessor flags;
    the pointer's lifetime belongs to the instance's native candidate list."""
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    for i in range(count.value):
        cand, kind = P(), I()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        if kind.value == want_kind:
            return cand
    return None


def guessed(k, text=b'nihao'):
    """Return a new instance after a full-pinyin parse, sentence guess and candidate guess.

    Guess candidates at byte zero with sort option zero; ignore call flags and
    leave the instance allocated for the caller."""
    inst = k.alloc()
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
    k.fn('guess_sentence', B, P)(inst)
    k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
    return inst


def predicted(k, prefix='我'):
    """Return a new instance with predictions and punctuation for the UTF-8 prefix.

    Ignore the generation flag and leave the instance allocated for the caller."""
    inst = k.alloc()
    k.fn('guess_predicted_candidates_with_punctuations', B, P, S)(inst, prefix.encode())
    return inst


@case('abort-get-candidate-nbest-index-normal-row', abort=False, issues=('525',))
def _(k):
    """Request an n-best index from a normal candidate and record whether 0xAB survives."""
    inst = guessed(k)
    out = C.c_ubyte(0xAB)
    ret = k.fn('get_candidate_nbest_index', B, P, P, C.POINTER(C.c_ubyte))(
        inst, row_of(k, inst, 2), C.byref(out))
    # The pin dies before it writes `*index`; an `untouched*` field must stay true.
    return {'ret': ret, 'untouched index': out.value == 0xAB}


@case('abort-remove-user-candidate-normal-system-token', abort=False, issues=('525',))
def _(k):
    """Attempt user-candidate removal on the first normal row from a system nihao guess."""
    inst = guessed(k)
    return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, row_of(k, inst, 2))}


@case('abort-remove-user-candidate-sentence-row', abort=False, issues=('525',))
def _(k):
    """Attempt user-candidate removal on the sentence row from a nihao guess."""
    inst = guessed(k)
    return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, row_of(k, inst, 1))}


# lane #525 batch C: the pin asserts when pinyin_remove_user_candidate cannot
# remove a user phrase (class (c): pinyin.cpp:3743/3750/3759). The subject
# fails the call with false and one warning in the libpinyin domain.
def _add_user_phrase(k, text, readings, count=5):
    """Add UTF-8 phrase text with byte-string readings to library 7 and return add flags.

    Assert iterator creation and every addition, using the supplied signed
    count. Close the iterator before checking all add flags; mutations affect
    the cached context and are not saved here."""
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    assert it
    add = k.fn('iterator_add_phrase', B, P, S, S, I)
    added = [add(it, text.encode(), reading, count) for reading in readings]
    k.fn('end_add_phrases', None, P)(it)
    assert all(added)
    return added


def _user_candidate_rows(k, inst):
    """Return borrowed pointers to kind-2 rows marked as user candidates in list order.

    Read the current list without regenerating it; pointers remain owned by
    the instance. Native accessor flags are not checked."""
    rows = []
    n = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(n))
    for i in range(n.value):
        cand, kind = P(), I()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        if kind.value == 2 and k.fn('is_user_candidate', B, P, P)(inst, cand):
            rows.append(cand)
    return rows


def _guessed_user_phrase(k, text, readings, parse=b'dada'):
    """Import a user phrase, parse its input on a new instance and return instance and rows.

    Generate candidates at byte zero with sort zero and assert that at least
    one normal user row exists; do not free the returned instance or its rows."""
    _add_user_phrase(k, text, readings)
    inst = k.alloc()
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, parse)
    k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
    rows = _user_candidate_rows(k, inst)
    assert rows
    return inst, rows


@case('abort-remove-user-candidate-twice', abort=False, issues=('525',))
def _(k):
    """Remove one imported candidate twice, asserting the first removal succeeds.

    Reuse the same borrowed pointer and return the first flag and the second
    call's flag if both calls return."""
    inst, rows = _guessed_user_phrase(k, '龘龘', [b"da2'da2"])
    first = k.fn('remove_user_candidate', B, P, P)(inst, rows[0])
    # The first removal must complete, or the second call would assert for
    # the wrong reason (the phrase still present).
    assert first, 'the first removal did not complete'
    # The phrase is gone from the user dictionary: the pin asserts at :3743.
    return {'first': first, 'ret': k.fn('remove_user_candidate', B, P, P)(inst, rows[0])}


@case('abort-remove-user-candidate-missing-phrase-table', abort=False, issues=('525',))
def _(k):
    """Remove a user candidate after saving and renaming its phrase-table file away.

    Finalize the cached context, move user_phrase_index.bin to a .gone path,
    and reopen independently. Assert reopening and a normal user row, then
    record removal; the kit's cached context is not replaced."""
    _add_user_phrase(k, '龘龘', [b"da2'da2"])
    assert k.fn('save', B, P)(k.ctx)
    k.fn('fini', None, P)(k.ctx)
    # user_phrase_index.bin holds the phrase table; user.bin still holds the
    # phrase index, so :3743 passes and :3750 fires.
    index = os.path.join(k.user, 'user_phrase_index.bin')
    os.replace(index, index + '.gone')
    ctx = k.init()
    assert ctx
    inst = k.fn('alloc_instance', P, P)(ctx)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'dada')
    k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
    rows = _user_candidate_rows(k, inst)
    assert rows
    return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, rows[0])}


@case('abort-remove-user-candidate-multi-pron', abort=False, issues=('525',))
def _(k):
    """Import two readings for one phrase and attempt to remove its first normal user row."""
    # Two readings for one phrase: the second is merged without a pinyin
    # index entry, so :3759 fires when the phrase is removed.
    inst, rows = _guessed_user_phrase(k, '龘龘', [b"da2'da2", b"da1'da1"])
    return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, rows[0])}


# batch 525-D: `pinyin_remove_user_candidate` also drives `Bigram::mask_out`
# (`pinyin.cpp:3766`, mask `PHRASE_INDEX_LIBRARY_MASK|PHRASE_MASK`), so all
# three corrupt user-bigram shapes reach it. The pin's phrase-index removals
# are in memory only and die with the abort, so a failed call leaves the store
# unchanged; the subject refuses before it removes anything.
def _removable_user_bigram_context(k, rows_for):
    """Return an instance and removable user row after replacing its persisted bigram file.

    Import and save 龘龘, look up its token and call rows_for(token) for raw
    records. Finalize the cached context, replace user_bigram.db, write the
    profile marker and assert reopening and candidate generation. Return the
    new instance and borrowed candidate without updating the kit's cached
    context. The allocated lookup array is not freed here.

    For residual-item fixtures, rows_for must encode the removed token as a
    successor so the removal mask selects it. This fixture reaches
    Bigram::mask_out through pinyin_remove_user_candidate
    in src/pinyin.cpp at pin 074a2219 after its phrase-removal checks."""
    _add_user_phrase(k, '龘龘', [b"da2'da2"])
    assert k.fn('save', B, P)(k.ctx)
    probe = k.alloc()
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    arr = new(0, 0, 4)
    assert k.fn('lookup_tokens', B, P, S, P)(probe, '龘龘'.encode(), arr)
    view = C.cast(arr, C.POINTER(Arr)).contents
    token = list(C.cast(view.data, C.POINTER(U))[:view.len])[0]
    k.fn('free_instance', None, P)(probe)
    k.fn('fini', None, P)(k.ctx)
    _craft_hash(os.path.join(k.user, 'user_bigram.db'), rows_for(token))
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    inst = k.fn('alloc_instance', P, P)(ctx)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'dada')
    assert k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
    candidates = _user_candidate_rows(k, inst)
    assert candidates, 'no user candidate'
    return inst, candidates[0]


@case('abort-remove-user-candidate-short-bigram-value', abort=False, issues=('525',))
def _(k):
    """Remove a persisted user phrase with a three-byte value planted in its bigram store."""
    inst, cand = _removable_user_bigram_context(
        k, lambda _token: [(b'\x01\x00\x00\x00', b'\x07\x00\x00')])
    return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, cand)}


@case('abort-remove-user-candidate-non-token-bigram-key', abort=False, issues=('525',))
def _(k):
    """Remove a persisted user phrase with a two-byte key planted in its bigram store."""
    inst, cand = _removable_user_bigram_context(
        k, lambda _token: [(b'\x00\x01', b'\x07\x00\x00\x00')])
    return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, cand)}


@case('abort-remove-user-candidate-residual-bigram-gram', abort=False, issues=('525',))
def _(k):
    """Remove a user phrase whose planted bigram total exceeds its sole item's count.

    Replace and reopen the profile through _removable_user_bigram_context,
    then return the removal flag if the native call completes."""
    # `total_freq` 7 with one item of the removed token and frequency 3:
    # `SingleGram::mask_out` drops the item and leaves 4, so the pin's
    # `get_length` assert dies (`ngram.cpp:70`, `ngram_bdb.cpp:243`).
    def rows(token):
        """Encode a little-endian gram with total seven and one item of count three.

        Use the removed token as the successor and token one as the container key."""
        value = b'\x07\x00\x00\x00' + token.to_bytes(4, 'little') + (3).to_bytes(4, 'little')
        return [(b'\x01\x00\x00\x00', value)]

    inst, cand = _removable_user_bigram_context(k, rows)
    return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, cand)}


@case('abort-choose-candidate-predicted-prefix-row', abort=0, issues=('525',))
def _(k):
    """Pass a predicted-prefix row to ordinary candidate selection at byte zero."""
    inst = predicted(k)
    return {'ret': k.fn('choose_candidate', I, P, Z, P)(inst, 0, row_of(k, inst, 5))}


@case('abort-choose-predicted-candidate-normal-row', abort=False, issues=('525',))
def _(k):
    """Pass a normal nihao candidate to the predicted-candidate selection API."""
    inst = guessed(k)
    return {'ret': k.fn('choose_predicted_candidate', B, P, P)(inst, row_of(k, inst, 2))}


@case('abort-choose-predicted-candidate-sentence-row', abort=False, issues=('525',))
def _(k):
    """Pass a nihao sentence row to the predicted-candidate selection API."""
    inst = guessed(k)
    return {'ret': k.fn('choose_predicted_candidate', B, P, P)(inst, row_of(k, inst, 1))}


# `pinyin_choose_candidate` under SORT_WITHOUT_SENTENCE_CANDIDATE asserts
# `0 == offset` on the NORMAL leg (`pinyin.cpp:2566`, register row 45).
def choose_without_sentence(k, offset):
    """Choose a normal nihao row at the supplied offset with sentence candidates disabled.

    Generate the list at zero with sort word 0x1, assert a normal row exists and
    return the signed selection result. The supplied offset is not validated."""
    inst = full_inst(k, b'nihao')
    k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1)
    row = row_of(k, inst, 2)
    assert row, 'no NORMAL row'
    return {'ret': k.fn('choose_candidate', I, P, Z, P)(inst, offset, row)}


@case('abort-choose-candidate-without-sentence-offset-1', abort=0, issues=('525',))
def _(k):
    """Select at byte one from a list generated at zero with sentence rows disabled."""
    return choose_without_sentence(k, 1)


# At offset 0 the leg trains the unigram and answers 1 on both sides.
@case('choose-candidate-without-sentence-offset-0', control=True, issues=('525',))
def _(k):
    """Select at byte zero with sentence rows disabled and record the selection result."""
    return choose_without_sentence(k, 0)


# The rows the guards leave alone answer without a warning.
@case('candidate-type-neighbours', control=True, issues=('525',))
def _(k):
    """Record accepted candidate-type operations on guessed and predicted lists.

    Read a sentence row's n-best index, query user status, choose a normal row,
    and choose predicted-prefix and punctuation rows on a separate instance."""
    out = {}
    inst = guessed(k)
    nbest = C.c_ubyte(0xAB)
    out['nbest of sentence row'] = [k.fn('get_candidate_nbest_index', B, P, P, C.POINTER(C.c_ubyte))(
        inst, row_of(k, inst, 1), C.byref(nbest)), nbest.value]
    out['is user'] = [k.fn('is_user_candidate', B, P, P)(inst, row_of(k, inst, kind)) for kind in (1, 2)]
    out['choose normal'] = k.fn('choose_candidate', I, P, Z, P)(inst, 0, row_of(k, inst, 2))
    pred = predicted(k)
    out['choose predicted prefix'] = k.fn('choose_predicted_candidate', B, P, P)(pred, row_of(k, pred, 5))
    out['choose predicted punctuation'] = k.fn('choose_predicted_candidate', B, P, P)(pred, row_of(k, pred, 8))
    return out


# batch2 group 12d: training a stale forcing (PR 12d, #525, row 6)
def row_with_text(k, inst, kind, text):
    """Return the first borrowed candidate matching both kind and UTF-8 text, or None.

    Scan the current list without checking accessor flags or changing it."""
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    for i in range(count.value):
        cand, ctype, string = P(), I(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(ctype))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(string))
        if ctype.value == kind and string.value == text.encode():
            return cand
    return None


def forced_sentence(k, relook):
    """Return a new nihao instance and the result of choosing 泥 after its sentence guess.

    Generate candidates and choose a normal row at zero; when relook is true,
    guess the sentence again after selection. Native flags are not asserted."""
    inst = k.alloc()
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
    chosen = k.fn('choose_candidate', I, P, Z, P)(inst, 0, row_with_text(k, inst, 2, '泥'))
    if relook:
        k.fn('guess_sentence', B, P)(inst)
    return inst, chosen


@case('abort-zhuyin-train-stale-forcing', mode='zhuyin', abort=False, issues=('525', 'row:6'))
def _(k):
    """Choose 泥 after a zhuyin sentence guess and train without guessing again.

    Parse su3cl3 on a new instance and use the first candidate with matching
    text, without filtering kind or asserting a match; return the train flag."""
    # The same walk through libzhuyin: a forcing added after the last
    # sentence lookup (`phonetic_lookup.h:868`).
    inst = k.alloc()
    k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
    k.fn('guess_sentence', B, P)(inst)
    k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    chosen = None
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    for i in range(count.value):
        cand, text = P(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        if text.value == '泥'.encode():
            chosen = cand
            break
    k.fn('choose_candidate', I, P, Z, P)(inst, 0, chosen)
    return {'ret': k.fn('train', B, P)(inst)}


@case('abort-train-stale-forcing', abort=False, issues=('525', 'row:6'))
def _(k):
    """Train result zero immediately after forcing 泥 onto a previously guessed nihao."""
    inst, _chosen = forced_sentence(k, False)
    return {'ret': k.fn('train', B, P, C.c_ubyte)(inst, 0)}


# An index past a NONEMPTY result set, on a real user directory: the user-dir
# gate passes and `assert(index < results.size())` fails (`pinyin.cpp:2684`).
# `null-user-session-pinyin` trains with a NULL user directory and so stops
# at the earlier gate.
@case('abort-train-index-past-results', abort=False, userdir=True, issues=('525', 'row:6'))
def _(k):
    """Assert a sentence guess exists, then attempt to train result index 255."""
    inst = full_inst(k, b'nihao')
    assert k.fn('guess_sentence', B, P)(inst), 'no sentence to train'
    return {'ret': k.fn('train', B, P, C.c_ubyte)(inst, 255)}


@case('train-forcing-after-lookup', control=True, issues=('525', 'row:6'))
def _(k):
    """Force 泥, guess the sentence again and record two consecutive training flags."""
    inst, chosen = forced_sentence(k, True)
    return {'chosen': chosen, 'train': k.fn('train', B, P, C.c_ubyte)(inst, 0),
            'again': k.fn('train', B, P, C.c_ubyte)(inst, 0)}


# #825: a forced phrase over an abbreviation whose incomplete keys expand to
# more than SCAN_EXPANSION_LIMIT (4,096) complete-key combinations. The pin's
# `validate_constraint` (`phonetic_lookup.cpp:120-165` at 074a2219) searches
# no table: `compute_pronunciation_possibility` walks the matrix columns
# against the forced item's stored pronunciations
# (`phonetic_key_matrix.cpp:534` onward), where an incomplete key matches any
# final (`pinyin_compare_middle_and_final3`, `pinyin_phrase3.h:80-86`), so the
# constraint survives however many completions the abbreviation has.
# Option word 0x18a (incomplete pinyin), sort word 0x1e; two user phrases of
# the same initials, the first (A) counted 50 and the second (B) counted 1.
CONSTRAINT_OPTIONS, CONSTRAINT_SORT = 0x18A, 0x1E


def constraint_phrases(k, phrases):
    """Import (text, reading bytes, count) rows into library 7 and return each add flag.

    UTF-8 encode text, assert iterator creation and close the iterator after
    adding; individual add failures remain in the returned list."""
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    assert it
    add = k.fn('iterator_add_phrase', B, P, S, S, I)
    added = [add(it, text.encode(), reading, count) for text, reading, count in phrases]
    k.fn('end_add_phrases', None, P)(it)
    return added


def candidate_heads(k, inst, limit=6):
    """Return leading candidate [kind, text] rows from the existing list.

    Limit to at most limit rows; map NULL or empty text to None and decode
    other text as strict UTF-8. Ignore native accessor flags."""
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    heads = []
    for i in range(min(count.value, limit)):
        cand, kind, text = P(), I(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        heads.append([kind.value, text.value.decode() if text.value else None])
    return heads


def constraint_instance(k, phrases, parse, options=CONSTRAINT_OPTIONS):
    """Import competing phrases, choose the second phrase and return instance plus observations.

    Require at least two (text, reading bytes, count) rows. Set options, parse,
    guess and collect the candidate head. Prefer a normal row for phrases[1],
    falling back to its n-best row, and assert the chosen row exists. Record
    native flags and selection output without asserting them. Keep the new
    instance allocated. The two constraint paths are pinyin_choose_candidate
    in src/pinyin.cpp at pin 074a2219."""
    out = {'options': k.fn('set_options', B, P, U)(k.ctx, options),
           'import': constraint_phrases(k, phrases)}
    inst = k.alloc()
    out['parse'] = k.fn('parse_more_full_pinyins', Z, P, S)(inst, parse)
    out['control guess'] = k.fn('guess_sentence', B, P)(inst)
    out['control row 0'] = sentence_out(k, inst, 0)
    out['candidates'] = k.fn('guess_candidates', B, P, Z, U)(inst, 0, CONSTRAINT_SORT)
    out['head'] = candidate_heads(k, inst)
    kind = 2 if row_with_text(k, inst, 2, phrases[1][0]) else 1
    out['chosen kind'] = kind
    row = row_with_text(k, inst, kind, phrases[1][0])
    assert row, 'no row for ' + phrases[1][0]
    out['choose'] = k.fn('choose_candidate', I, P, Z, P)(inst, 0, row)
    return inst, out


def constraint_survives(k, phrases, parse):
    """Observe a selected phrase after another guess and after reparsing identical bytes.

    Extend constraint_instance's observations with both sentence-zero results
    and flags, retaining the same instance and imported context."""
    inst, out = constraint_instance(k, phrases, parse)
    out['guess 5'] = k.fn('guess_sentence', B, P)(inst)
    out['row 0 at 5'] = sentence_out(k, inst, 0)
    out['reparse'] = k.fn('parse_more_full_pinyins', Z, P, S)(inst, parse)
    out['guess 6'] = k.fn('guess_sentence', B, P)(inst)
    out['row 0 at 6'] = sentence_out(k, inst, 0)
    return out


CONSTRAINT_PHRASES = [
    ('中华人民', b"zhong1'hua2'ren2'min2", 50), ('种花人民', b"zhong3'hua1'ren2'min2", 1)]
CONSTRAINT_PHRASES_LONG = [
    ('中华人民共和国', b"zhong1'hua2'ren2'min2'gong4'he2'guo2", 50),
    ('中华人民公会国', b"zhong1'hua2'ren2'min2'gong1'hui4'guo2", 1)]

# (input, incomplete-key product on main's `expand_keys`): under the cap the
# subject already agreed with the pin, so those rows are the controls.
for _parse, _product, _phrases in (
        (b'zhonghuarenmin', 'none', CONSTRAINT_PHRASES),
        (b'zhonghuarm', '266', CONSTRAINT_PHRASES),
        (b'zhhrenmin', '361', CONSTRAINT_PHRASES),
        (b'zhonghrm', '5054', CONSTRAINT_PHRASES),
        (b'zhhrm', '96026', CONSTRAINT_PHRASES),
        (b'zhhrmghg', 'over 96026', CONSTRAINT_PHRASES_LONG)):
    case('constraint-survives-' + _parse.decode(), control=_product in ('none', '266', '361'), issues=('825',))(
        lambda k, _p=_parse, _ph=_phrases: constraint_survives(k, _ph, _p))


# The same forcing met by `reconcile_replaced_selection` instead of by the
# next guess: with `PINYIN_CORRECT_V_U` on, `jve` and `jue` are one key, so
# re-parsing `jvezhhrm` over `juezhhrm` changes a byte inside the covered span
# without moving a key boundary. The replacement does not extend the covered
# bytes, so the engine revalidates the retained forcing (96,026 completions of
# `zh h r m`) before the next lookup; nothing is guessed in between.
CONSTRAINT_PHRASES_V_U = [
    ('决中华人民', b"jue2'zhong1'hua2'ren2'min2", 50),
    ('绝种花人民', b"jue2'zhong3'hua1'ren2'min2", 1)]


def constraint_survives_reconcile(parse, replacement):
    """Build a case that reparses replacement bytes over a forced V/U-correction phrase."""
    def run(k):
        """Import and select the V/U fixture, reparse, then record candidates and sentence zero.

        Enable correction bit 27 along with CONSTRAINT_OPTIONS; retain the selected
        instance through reparsing and the subsequent guess."""
        inst, out = constraint_instance(k, CONSTRAINT_PHRASES_V_U, parse, options=CONSTRAINT_OPTIONS | 1 << 27)
        out['reparse'] = k.fn('parse_more_full_pinyins', Z, P, S)(inst, replacement)
        out['candidates after'] = k.fn('guess_candidates', B, P, Z, U)(inst, 0, CONSTRAINT_SORT)
        out['head after'] = candidate_heads(k, inst)
        out['guess'] = k.fn('guess_sentence', B, P)(inst)
        out['row 0'] = sentence_out(k, inst, 0)
        return out
    return run


for _parse, _replacement, _product in (
        (b'juezhonghuarm', b'jvezhonghuarm', 266), (b'juezhhrm', b'jvezhhrm', 96026)):
    case('constraint-survives-reconcile-' + _parse.decode(), control=_product < 4096, issues=('825',))(
        constraint_survives_reconcile(_parse, _replacement))


# #825 review: `validate_constraint` declares `PhraseItem item;` once, before
# its loop (`phonetic_lookup.cpp:141`), and `get_phrase_item` leaves it alone
# when it fails (`phrase_index.h:649-650`, `phrase_index.cpp:189-193`). A
# forced token whose library was unloaded is therefore checked against the
# previous forcing's item. Two phrases forced, the later one only in the GBK
# library (2), which is then unloaded: the stale item keeps the later forcing
# when it spells the later span (`nini`: both read `ni`) and drops it when it
# does not (`nihao`).
def constraint_stale_item(first, second, parse):
    """Capture two phrase selections and input bytes for a library-2 unload case."""
    def run(k):
        """Choose both phrases, then observe sentence zero before and after unloading library 2.

        Use the first selection result as the second byte offset without checking
        it. Return token IDs, flags and sentences; leave the library unloaded."""
        out = {'options': k.fn('set_options', B, P, U)(k.ctx, CONSTRAINT_OPTIONS)}
        inst = k.alloc()
        out['parse'] = k.fn('parse_more_full_pinyins', Z, P, S)(inst, parse)
        out['guess'] = k.fn('guess_sentence', B, P)(inst)
        out['first'] = choose_text(k, inst, 0, first)
        out['second'] = choose_text(k, inst, out['first'], second)
        out['tokens'] = [hex(t) for t in tokens_of(k, second)]
        out['guess loaded'] = k.fn('guess_sentence', B, P)(inst)
        out['row 0 loaded'] = sentence_out(k, inst, 0)
        out['unload'] = k.fn('unload_phrase_library', B, P, U)(k.ctx, 2)
        out['guess unloaded'] = k.fn('guess_sentence', B, P)(inst)
        out['row 0 unloaded'] = sentence_out(k, inst, 0)
        return out
    return run


case('constraint-stale-item-keeps-nini', issues=('825',))(constraint_stale_item('你', '抳', b'nini'))
case('constraint-stale-item-drops-nihao', control=True, issues=('825',))(constraint_stale_item('你', '晧', b'nihao'))


# batch G: a stored forcing outlives a re-parse that shortened the input
# (PR G, #525, row 82). The walk tails the LAST span at
# `constraints->length() - 1` (phonetic_lookup.h:921), not the current
# matrix's last column, and the store survives the parse unvalidated
# (pinyin.cpp:1497-1525). `increase_pronunciation_possibility` then asserts
# `end < matrix->size()` (`storage/phonetic_key_matrix.cpp:661`) on a matrix
# the re-parse shrank; the pin aborts, the subject refuses before observing
# anything and warns once in the `libpinyin` domain.
def reparse_after_choose(k, shrink):
    """Return a new nihao instance after choosing row two, guessing and reparsing shrink.

    Use the same instance throughout; do not validate call flags or reconcile
    its sentence results by guessing after the reparse."""
    inst = k.alloc()
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
    chosen = P()
    k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, 2, C.byref(chosen))
    k.fn('choose_candidate', I, P, Z, P)(inst, 0, chosen)
    k.fn('guess_sentence', B, P)(inst)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, shrink)
    return inst


@case('abort-train-after-reparse-past-matrix-end', abort=False, issues=('525', 'row:82'))
def _(k):
    """Train the retained result after replacing the chosen nihao parse with ni."""
    inst = reparse_after_choose(k, b'ni')
    return {'ret': k.fn('train', B, P, C.c_ubyte)(inst, 0)}


# The `'ni` twin walks the same clamp into a matrix whose column 0 the
# leading apostrophe left empty (`:663`): reproduced, SIGABRT at
# `phonetic_key_matrix.cpp:663` under the `__assert_fail` wrapper.
@case('abort-train-after-reparse-empty-start-column', abort=False, issues=('525', 'row:82'))
def _(k):
    """Train the retained result after reparsing a leading apostrophe followed by ni."""
    inst = reparse_after_choose(k, b"'ni")
    return {'ret': k.fn('train', B, P, C.c_ubyte)(inst, 0)}


# batch G review fix: the separator between keys is a zero-key column, not
# an empty one. `fill_matrix` fills it (`phonetic_key_matrix.cpp:52-78`), so
# the pin's `get_column_size(start) > 0` (`:663`) passes and training
# completes when the stale span's end lands on a key column. After a
# re-parse to `ni'ha`, the stale second span starts on the apostrophe
# column and the pin trains; the engine accepts the zero-key column too.
@case('train-after-reparse-internal-zero-key', control=True, issues=('525', 'row:82'))
def _(k):
    """Train the retained result after reparsing ni'ha with an internal separator."""
    inst = reparse_after_choose(k, b"ni'ha")
    return {'ret': k.fn('train', B, P, C.c_ubyte)(inst, 0)}


# The stale end can still land on an empty column: after a re-parse to
# `ni'hao`, the pin passes `:663` (the apostrophe column holds a zero key)
# and aborts at `:664` (`get_column_size(end) > 0`). The engine refuses with
# the same fault.
@case('abort-train-after-reparse-empty-end-column', abort=False, issues=('525', 'row:82'))
def _(k):
    """Train the retained result after reparsing ni'hao with its shifted key boundaries."""
    inst = reparse_after_choose(k, b"ni'hao")
    return {'ret': k.fn('train', B, P, C.c_ubyte)(inst, 0)}






# The keys the :661/:663 guard leaves alone: the same choose-and-look-up
# with no shortening re-parse, and a re-parse that repeats the input.
@case('train-after-choose-lookup-control', control=True, issues=('525', 'row:82'))
def _(k):
    """Compare training after selection/guessing with no reparse and an identical reparse.

    Use separate new instances for the two paths and return their train flags."""
    def choose_and_relook(inst):
        """Parse nihao, choose candidate index two and regenerate its sentence results.

        Mutate the supplied instance without asserting any native return flag."""
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
        k.fn('guess_sentence', B, P)(inst)
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
        chosen = P()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, 2, C.byref(chosen))
        k.fn('choose_candidate', I, P, Z, P)(inst, 0, chosen)
        k.fn('guess_sentence', B, P)(inst)

    inst = k.alloc()
    choose_and_relook(inst)
    out = {'no-reparse': k.fn('train', B, P, C.c_ubyte)(inst, 0)}

    inst = k.alloc()
    choose_and_relook(inst)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    out['same-length-reparse'] = k.fn('train', B, P, C.c_ubyte)(inst, 0)
    return out


# batch E: over-long user chewing keys (#525, row 84). The pin's user
# `ChewingLargeTable2` instantiates its entries only for lengths 1..=16, so a
# `user_pinyin_index.bin` key past `MAX_PHRASE_LENGTH` syllables drives both
# `mask_out` (`storage/chewing_large_table2_bdb.cpp:529`) and the
# longer-candidate walk (`:282`) into their `switch`'s `default: abort()`. The
# key cannot be written through the API (`_add_phrase` refuses
# `phrase_length >= MAX_PHRASE_LENGTH`, `pinyin.cpp:643`), so the case crafts
# the user index directly, in the running cell's own format (`_craft_db`): a
# Berkeley DB btree on bdb (the pin's own layout,
# `chewing_large_table2_bdb.cpp:58`), a Kyoto Cabinet snapshot on kc and a
# tkrzw `TreeDBM` file on tkrzw. The kc and tkrzw chewing tables carry the
# same switch (`chewing_large_table2_kyotodb.cpp:498`, `:268`;
# `chewing_large_table2_tkrzwdb.cpp:465`, `:251`), so each cell's pin dies at
# its own line.
def _craft_db(path, rows, dbtype, keep=False, *, system_index=False):
    """Write raw byte records using the selected cell's user or system container format.

    For BDB, dbtype selects btree (1) or hash (2). KC user tables are snapshots;
    keep or system_index selects a native TreeDB and requires dbtype 1.
    tkrzw uses HashDBM for type 2 and TreeDBM otherwise. Without keep, replace
    the target; keep updates existing rows. Delegate native failures to the
    writer's assertions and return its result.

    These formats follow ChewingLargeTable2::load_db and ::attach in
    src/storage/chewing_large_table2_kyotodb.cpp and
    src/storage/chewing_large_table2_tkrzwdb.cpp, Bigram::load_db in
    src/storage/ngram_kyotodb.cpp and src/storage/ngram_tkrzwdb.cpp, and
    PhraseLargeTable3::load_db in src/storage/phrase_large_table3_kyotodb.cpp
    and src/storage/phrase_large_table3_tkrzwdb.cpp at pin 074a2219."""
    if CELL == 'kc':
        assert not (keep or system_index) or dbtype == 1, 'KC system indexes are btrees'
        if system_index:
            return _craft_kc(path, rows, keep=keep, system_index=True)
        return _craft_kc(path, rows, keep=keep)
    if CELL == 'tkrzw':
        return _craft_tkrzw(path, rows, 'HashDBM' if dbtype == 2 else 'TreeDBM', keep=keep)
    return _craft_bdb(path, rows, dbtype, keep=keep)


def _craft_kc(path, rows, keep=False, *, system_index=False):
    """Write byte records to a KC snapshot or native system TreeDB through the C API.

    Unlink an existing path unless keep is true. keep opens an existing native
    TreeDB for updates; system_index creates native format even without keep.
    Otherwise populate an in-memory DB and dump a snapshot. Assert native
    operations, then close/delete the handle on success; failures do not have
    a finally-based cleanup path."""
    kc = C.CDLL('libkyotocabinet.so.16')
    kc.kcdbnew.restype = C.c_void_p
    kc.kcdbdel.argtypes = [C.c_void_p]
    kc.kcdbopen.argtypes = [C.c_void_p, C.c_char_p, C.c_uint32]
    kc.kcdbset.argtypes = [C.c_void_p, C.c_char_p, C.c_size_t, C.c_char_p, C.c_size_t]
    kc.kcdbdumpsnap.argtypes = [C.c_void_p, C.c_char_p]
    kc.kcdbclose.argtypes = [C.c_void_p]
    if os.path.exists(path) and not keep:
        os.unlink(path)
    db = kc.kcdbnew()
    assert db
    # Existing system TreeDB: KCOWRITER, explicit type because its suffix
    # is .bin. Fresh system TreeDB and user memory DB: also KCOCREATE.
    native = keep or system_index
    target = (path + '#type=kct').encode() if native else b'-'
    mode = (1 << 1) if keep else (1 << 1) | (1 << 2)
    assert kc.kcdbopen(db, target, mode)
    for key, value in rows:
        assert kc.kcdbset(db, key, len(key), value, len(value))
    if not native:
        assert kc.kcdbdumpsnap(db, path.encode())
    assert kc.kcdbclose(db)
    kc.kcdbdel(db)


def _craft_tkrzw(path, rows, kind, keep=False):
    """Write byte records to the named tkrzw DBM kind through its C API.

    Unlink an existing file unless keep is true; overwrite matching records
    and assert open, set and close success. No failure cleanup is installed.
    TreeDBM index and HashDBM bigram files follow the load_db/save_db methods
    in src/storage/chewing_large_table2_tkrzwdb.cpp,
    src/storage/phrase_large_table3_tkrzwdb.cpp and src/storage/ngram_tkrzwdb.cpp
    at pin 074a2219."""
    tk = C.CDLL('libtkrzw.so.1')
    tk.tkrzw_dbm_open.restype = C.c_void_p
    tk.tkrzw_dbm_open.argtypes = [C.c_char_p, C.c_bool, C.c_char_p]
    tk.tkrzw_dbm_set.restype = C.c_bool
    tk.tkrzw_dbm_set.argtypes = [C.c_void_p, C.c_char_p, C.c_int32, C.c_char_p, C.c_int32, C.c_bool]
    tk.tkrzw_dbm_close.restype = C.c_bool
    tk.tkrzw_dbm_close.argtypes = [C.c_void_p]
    if os.path.exists(path) and not keep:
        os.unlink(path)
    db = tk.tkrzw_dbm_open(path.encode(), True, ('dbm=' + kind).encode())
    assert db
    for key, value in rows:
        assert tk.tkrzw_dbm_set(db, key, len(key), value, len(value), True)
    assert tk.tkrzw_dbm_close(db)


def _craft_bdb(path, rows, dbtype, keep=False, read_keys=(), allow_missing=False):
    """Write or read a Berkeley DB container and return values for read_keys in order.

    rows=None opens read-only; any other rows value enables creation and
    unlinks an existing file unless keep is true. Store byte pairs, then copy
    each requested value to Python bytes. With allow_missing, any nonzero get
    status becomes None, not just a not-found status; otherwise assert success.
    Assert create/open/put/close results. Handles close only on the success path,
    and dynamically loaded libdb must expose the __db_*_pp entry points."""
    import ctypes.util

    class DBT(C.Structure):
        """Mirror the native DBT record passed to Berkeley DB's get and put entry points.

        Include app_data before flags so pointer alignment matches the C layout;
        data points to buffers whose lifetime is managed separately by the caller."""
        # 40 bytes on x86_64: the `app_data` pointer between `doff` and
        # `flags` (omitting it misaligns `flags`, and `__db_put_pp` then
        # rejects the flag argument).
        _fields_ = [
            ('data', C.c_void_p),
            ('size', C.c_uint),
            ('ulen', C.c_uint),
            ('dlen', C.c_uint),
            ('doff', C.c_uint),
            ('app_data', C.c_void_p),
            ('flags', C.c_uint),
        ]

    db = C.CDLL(ctypes.util.find_library('db'))
    db.db_create.argtypes = [C.POINTER(C.c_void_p), C.c_void_p, C.c_uint]
    db.db_create.restype = C.c_int
    db.__db_open_pp.argtypes = [C.c_void_p, C.c_void_p, C.c_char_p, C.c_char_p,
                                C.c_int, C.c_uint, C.c_int]
    db.__db_open_pp.restype = C.c_int
    db.__db_put_pp.argtypes = [C.c_void_p, C.c_void_p, C.POINTER(DBT), C.POINTER(DBT), C.c_uint]
    db.__db_put_pp.restype = C.c_int
    db.__db_close_pp.argtypes = [C.c_void_p, C.c_uint]
    db.__db_close_pp.restype = C.c_int
    db.__db_get_pp.argtypes = [P, P, C.POINTER(DBT), C.POINTER(DBT), U]
    db.__db_get_pp.restype = I
    if rows is not None and os.path.exists(path) and not keep:
        os.unlink(path)
    handle = C.c_void_p()
    assert db.db_create(C.byref(handle), None, 0) == 0
    # DB_CREATE == 0x1; the pin opens its tables the same way
    # (`chewing_large_table2_bdb.cpp:90`, `ngram_bdb.cpp:56`).
    flags = 0x1 if rows is not None else 0x400  # DB_CREATE / DB_RDONLY
    assert db.__db_open_pp(handle, None, path.encode(), None, dbtype, flags, 0o600) == 0
    live = []
    for key, value in rows or ():
        kbuf = C.create_string_buffer(key)
        vbuf = C.create_string_buffer(value)
        live += [kbuf, vbuf]
        kd = DBT(C.cast(kbuf, C.c_void_p), len(key), 0, 0, 0, None, 0)
        vd = DBT(C.cast(vbuf, C.c_void_p), len(value), 0, 0, 0, None, 0)
        assert db.__db_put_pp(handle, None, C.byref(kd), C.byref(vd), 0) == 0
    values = []
    for key in read_keys:
        kbuf = C.create_string_buffer(key)
        kd = DBT(C.cast(kbuf, P), len(key), 0, 0, 0, None, 0)
        vd = DBT()
        status = db.__db_get_pp(handle, None, C.byref(kd), C.byref(vd), 0)
        if status != 0 and allow_missing:
            values.append(None)
            continue
        assert status == 0
        values.append(C.string_at(vd.data, vd.size))
    assert db.__db_close_pp(handle, 0) == 0
    del live
    return values


def _craft_btree(path, keys):
    """Replace an index container with byte keys mapped to empty values in the current cell.

    Delegate format selection and native failures to _craft_db with tree type 1."""
    _craft_db(path, [(key, b'') for key in keys], 1)  # DB_BTREE == 1


def _craft_hash(path, rows):
    """Replace the current cell's user-bigram container with raw byte key/value pairs.

    Use hash type 2 through _craft_db: BDB hash, KC snapshot or tkrzw HashDBM."""
    _craft_db(path, rows, 2)  # DB_HASH == 2


def _write_user_conf(k):
    """Overwrite user.conf with the system table's format fields and an open counter of zero.

    Read binary/model/database versions from table.conf; missing fields raise
    KeyError after the output file has been opened. This prevents crafted
    profiles being discarded by check_format in src/pinyin.cpp and
    src/zhuyin.cpp at pin 074a2219. The pinyin check also rewrites the marker
    on initialization; the zhuyin check does not."""
    keys = ('binary format version:', 'model data version:', 'database format:')
    values = {}
    with open(os.path.join(k.data, 'table.conf')) as table_conf:
        for line in table_conf:
            for key in keys:
                if line.startswith(key):
                    values[key] = line[len(key):].strip()
    with open(os.path.join(k.user, 'user.conf'), 'w') as user_conf:
        for key in keys:
            user_conf.write(key + values[key] + '\n')
        user_conf.write('open counter:0\n')



# #920: user.conf integer-overflow and state-preservation cases. The fixtures
# derive every value from the running cell's table.conf; no database-format
# literal is written. A conforming marker must retain the trained/imported
# state, while a non-conforming one follows check_format's wipe.
def _user_conf_values(k):
    values = {}
    for line in Path(k.data, 'table.conf').read_text().splitlines():
        for key in ('binary format version:', 'model data version:'):
            if line.startswith(key):
                values[key] = int(line[len(key):].strip())
    return values


def _replace_conf_value(data, key, value):
    pattern = rb'(?m)^' + re.escape(key.encode()) + rb'.*$'
    return re.sub(pattern, key.encode() + str(value).encode(), data, count=1)


def _edit_user_conf_versions(k, extra):
    data = Path(k.user, 'user.conf').read_bytes()
    values = _user_conf_values(k)
    for key in ('binary format version:', 'model data version:'):
        data = _replace_conf_value(data, key, values[key] + (1 << 32) + extra)
    return data


def _edit_user_conf_counter(k, value):
    return _replace_conf_value(
        Path(k.user, 'user.conf').read_bytes(), 'open counter:', value)


def _edit_user_conf_nul(k):
    data = Path(k.user, 'user.conf').read_bytes()
    return re.sub(
        rb'(?m)^(database format:)([^\r\n\x00]+)',
        lambda match: match.group(1) + match.group(2) + b'\x00junk',
        data, count=1)


def _edit_user_conf_missing_database(k):
    data = Path(k.user, 'user.conf').read_bytes()
    data = re.sub(rb'(?m)^database format:.*$', b'', data)
    return re.sub(rb'(?m)^open counter:.*$', b'', data)


def _edit_user_conf_missing_counter(k):
    data = Path(k.user, 'user.conf').read_bytes()
    return re.sub(rb'(?m)^open counter:.*$', b'', data)


def _edit_user_conf_negative_overflow(k):
    data = Path(k.user, 'user.conf').read_bytes()
    for key in ('binary format version:', 'model data version:'):
        data = _replace_conf_value(data, key, -9223372036854775808)
    return data


def _user_conf_920_session(edit, keep_state):
    def probe(k):
        training = token_unigram_trained(k)
        imported = token_unigram_user(k, add_frequency=False)
        saved = k.fn('save', B, P)(k.ctx)
        assert saved, 'the trained profile must save'
        k.fn('free_instance', None, P)(k.inst)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = k._inst = None
        Path(k.user, 'user.conf').write_bytes(edit(k))
        ctx = k.init()
        out = {'reopen': bool(ctx), 'training': training, 'import': imported,
               'save': saved}
        if ctx:
            k._ctx = ctx
            out['trained-after'] = unigram_of(k, int(training['token'], 16))
            out['import-after'] = unigram_of(k, int(imported['token'], 16))
            if keep_state:
                assert out['trained-after'] == training['after'], 'trained state was wiped'
                assert out['import-after'] == imported['after'], 'imported state was wiped'
        out['files'] = user_listing(k)
        return out
    return probe


def _user_conf_920_case(name, edit, keep_state, control):
    for mode in ('pinyin', 'zhuyin'):
        suffix = '-zhuyin' if mode == 'zhuyin' else ''
        case('user-conf-920-' + name + suffix, mode=mode, issues=('920',),
             control=control)(_user_conf_920_session(edit, keep_state))


_user_conf_920_case('nul-database-token', _edit_user_conf_nul, True, False)
_user_conf_920_case('version-overflow-conforms',
                     lambda k: _edit_user_conf_versions(k, 0), True, True)
_user_conf_920_case('version-overflow-nonconforms',
                     lambda k: _edit_user_conf_versions(k, 1), False, True)
_user_conf_920_case('counter-overflow-conforms',
                     lambda k: _edit_user_conf_counter(k, 1 << 32), True, True)
_user_conf_920_case('counter-overflow-nonconforms',
                     lambda k: _edit_user_conf_counter(k, (1 << 32) + 7), False, True)
_user_conf_920_case('negative-overflow-nonconforms',
                     _edit_user_conf_negative_overflow, False, True)
_user_conf_920_case('database-missing-at-eof',
                     _edit_user_conf_missing_database, False, True)
_user_conf_920_case('counter-missing',
                     _edit_user_conf_missing_counter, True, True)


def overlong_index_context(k, parse=None, rows=None, options=None):
    """Replace the user pinyin index and return an independently reopened context.

    Initialize/finalize once, extracting the first two-byte key from parse
    when supplied, or using bytes 00 01 otherwise. Default rows are that key
    and its 17-fold repetition; rows(first) can provide a different iterable
    of byte keys. Rewrite the container and user.conf before reopening.
    Apply options to both contexts when non-None, asserting initialization
    and key extraction but not option-setting success. The returned context
    is not cached on the kit and must be finalized separately."""
    ctx = k.init()
    assert ctx, 'the first init failed'
    if options is not None:
        k.fn('set_options', B, P, U)(ctx, options)
    if parse is None:
        first = b'\x00\x01'
    else:
        inst = k.fn('alloc_instance', P, P)(ctx)
        assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, parse)
        key = C.c_void_p()
        assert k.fn('get_pinyin_key', B, P, Z, C.POINTER(P))(inst, 0, C.byref(key))
        first = C.string_at(key.value, 2)
        k.fn('free_instance', None, P)(inst)
    k.fn('fini', None, P)(ctx)
    if rows is None:
        rows = [first, first * 17]
    else:
        rows = rows(first)
    _craft_btree(os.path.join(k.user, 'user_pinyin_index.bin'), rows)
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    if options is not None:
        k.fn('set_options', B, P, U)(ctx, options)
    return ctx


@case('abort-mask-out-overlong-index-key', abort=False, issues=('525', 'row:84'))
def _(k):
    """Mask a reopened profile containing a 17-word user index key and record the flag."""
    ctx = overlong_index_context(k)
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


@case('abort-zhuyin-mask-out-overlong-index-key', mode='zhuyin', abort=False, issues=('525', 'row:84'))
def _(k):
    """Run zhuyin mask_out against a reopened profile with a 17-word user index key."""
    # libzhuyin masks `m_pinyin_table` too (`zhuyin.cpp:763`), the same user
    # chewing table, so the same :529 walk aborts there.
    ctx = overlong_index_context(k)
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


@case('abort-guess-candidates-overlong-index-key', abort=False, issues=('525', 'row:84'))
def _(k):
    """Guess ni candidates against a user index containing its key and a 17-word extension.

    Assert a nonempty parse, then enable the longer-candidate walk with sort zero."""
    ctx = overlong_index_context(k, parse=b'ni')
    inst = k.fn('alloc_instance', P, P)(ctx)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    # sort 0 clears SORT_WITHOUT_LONGER_CANDIDATE, so the walk runs and
    # reaches the crafted extension (`pinyin.cpp:2292-2293`).
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}


@case('abort-guess-candidates-overlong-odd-index-key', abort=False, issues=('525', 'row:84'))
def _(k):
    """Guess ni candidates with a 35-byte user index key extending its parsed syllable.

    Plant the exact prefix as well, reopen and assert parsing before guessing."""
    # A 35-byte key is not one upstream writes, but `phrase_length =
    # db_key.size / sizeof(ChewingKey)` is integer division
    # (`chewing_large_table2_bdb.cpp:470`), so the pin reads it as 17 words
    # and its `default: abort()` dies. The subject classifies the complete-word
    # prefix the same way and drops the trailing byte only after.
    ctx = overlong_index_context(
        k, parse=b'ni', rows=lambda first: [first, first * 17 + b'\x07'])
    inst = k.fn('alloc_instance', P, P)(ctx)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}


@case('abort-mask-out-overlong-odd-index-key', abort=False, issues=('525', 'row:84'))
def _(k):
    """Mask a user index containing a normal key and a 17-word key with one trailing byte."""
    ctx = overlong_index_context(
        k, rows=lambda first: [first, first * 17 + b'\x07'])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


# A key of no whole word (empty, or one byte) reads as `size / 2 == 0` words
# and `mask_out`'s `switch` has no such case: `default: abort()`
# (`chewing_large_table2_bdb.cpp:529`, the KC twin :499, tkrzw :466).
@case('abort-mask-out-empty-index-key', abort=False, userdir=True, issues=('525', 'row:84'))
def _(k):
    """Mask a reopened user index containing an empty key and one normal key."""
    ctx = overlong_index_context(k, rows=lambda first: [b'', first])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


@case('abort-mask-out-one-byte-index-key', abort=False, userdir=True, issues=('525', 'row:84'))
def _(k):
    """Mask a reopened user index containing a normal key and a one-byte key."""
    ctx = overlong_index_context(k, rows=lambda first: [first, b'\x07'])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


@case('abort-zhuyin-mask-out-empty-index-key', mode='zhuyin', abort=False, userdir=True, issues=('525', 'row:84'))
def _(k):
    """Run zhuyin mask_out against an empty-key user index after reopening the profile."""
    ctx = overlong_index_context(k, rows=lambda first: [b'', first])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


# The system `pinyin_index.bin` is the same `ChewingLargeTable2` btree, and
# `FacadeChewingTable2::search_suggestion` walks it too
# (`chewing_large_table2_bdb.cpp:282`): a key past 16 words that extends the
# query dies at the same `default: abort()`. The stock index has none, so the
# fixture is a private copy (never a link into the oracle) with one row added
# in its native attached format (KC aborts at :269, tkrzw at :252).
def system_overlong_index_context(k, trailing=b''):
    """Return a context on a private system index extended with a 17-word ni key.

    Extract ni's two-byte key, finalize its context and replace the private
    index's oracle symlink with a file copy. Add the repeated key plus trailing
    bytes without dropping existing records. Assert reopening against that
    private system directory; leave the returned context uncached on the kit."""
    ctx = k.init()
    assert ctx, 'the first init failed'
    inst = k.fn('alloc_instance', P, P)(ctx)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    key = C.c_void_p()
    assert k.fn('get_pinyin_key', B, P, Z, C.POINTER(P))(inst, 0, C.byref(key))
    first = C.string_at(key.value, 2)
    k.fn('free_instance', None, P)(inst)
    k.fn('fini', None, P)(ctx)
    system = private_system(k)
    path = os.path.join(system, 'pinyin_index.bin')
    os.unlink(path)  # the link into the oracle's data
    shutil.copyfile(os.path.join(k.data, 'pinyin_index.bin'), path)
    _craft_db(path, [(first * 17 + trailing, b'')], 1, keep=True)
    ctx = k.init(system=system)
    assert ctx, 'the open on the private system dir failed'
    return ctx


@case('abort-guess-candidates-overlong-system-index-key', abort=False, userdir=True, issues=('525', 'row:84'))
def _(k):
    """Guess ni candidates after appending a 17-word key to a copied system index."""
    ctx = system_overlong_index_context(k)
    inst = k.fn('alloc_instance', P, P)(ctx)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}


@case('abort-guess-candidates-overlong-odd-system-index-key', abort=False, userdir=True, issues=('525', 'row:84'))
def _(k):
    """Guess ni candidates after appending a 35-byte key to a copied system index."""
    ctx = system_overlong_index_context(k, trailing=b'\x07')
    inst = k.fn('alloc_instance', P, P)(ctx)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}


@case('guess-after-overlong-index-key-without-prefix-answers-true', issues=('525', 'row:84'))
def _(k):
    """Guess ni with an overlong user key present but its exact prefix record absent.

    Reopen the crafted profile and assert parsing, then record the guess flag."""
    # The pin's `search_suggestion` probes the exact query key first
    # (`cursorp->c_get(..., DB_SET)`, `chewing_large_table2_bdb.cpp:576`) and
    # answers SEARCH_NONE when it is absent, so an over-long key whose prefix
    # row was never written walks nothing and the abort is unreachable.
    ctx = overlong_index_context(k, parse=b'ni', rows=lambda first: [first * 17])
    inst = k.fn('alloc_instance', P, P)(ctx)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}


@case('reparse-after-guess-ignores-overlong-index-key', issues=('525', 'row:84'))
def _(k):
    """Guess hao, then reparse ni in a profile carrying ni's overlong index extension.

    Return the first guess flag and consumed bytes from reparsing; do not guess
    candidates again after the replacement parse."""
    # `_prepend_longer_candidates` runs only from `pinyin_guess_candidates`
    # (`pinyin.cpp:2292-2293`); a parse refreshes the cached list without it,
    # so the pin cannot reach the abort from the re-parse.
    ctx = overlong_index_context(k, parse=b'ni')
    inst = k.fn('alloc_instance', P, P)(ctx)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'hao')
    guessed = k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
    reparsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    return {'guess_hao': guessed, 'reparse_ni': reparsed}


@case('abort-guess-candidates-overlong-incomplete-index-key', abort=False, issues=('525', 'row:84'))
def _(k):
    """Guess n'an against crafted incomplete-key prefixes with PINYIN_INCOMPLETE enabled.

    Plant the two-word prefix and its 17-word extension, reopen with option
    word eight and assert parsing before guessing at zero with sort zero."""
    # `compute_incomplete_chewing_index` sets only `m_initial`, so the partial
    # `n` projects to its initial and the vowel-initial `an` to the zero
    # initial. The 17-word key extends that two-word projection, so the pin's
    # `default: abort()` dies at the guess.
    ctx = overlong_index_context(
        k,
        parse=b'n',
        options=8,  # PINYIN_INCOMPLETE
        rows=lambda first: [
            first + b'\x00\x00',
            first + b'\x00\x00' + b'\x01\x00' * 15,
        ],
    )
    inst = k.fn('alloc_instance', P, P)(ctx)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b"n'an")
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}
# batch 525-D: crafted `user_bigram.db` rows the pin's bigram walk aborts on.
# A value shorter than a `guint32` dies at `MemoryChunk::get_content<guint32>`
# (`memory_chunk.h:390`, reached by `get_total_freq`); a key that is not four
# bytes dies at `Bigram::get_all_items` (`ngram_bdb.cpp:199`); an item-less
# gram with a residual total dies at `SingleGram::get_length`
# (`ngram.cpp:70`, reached by `_compute_predicted_bigram_candidates`); and a
# gram whose total its items do not cover dies at `get_length` after the mask
# removes every item (`ngram.cpp:70`, reached by `Bigram::mask_out`,
# `ngram_bdb.cpp:243`). The container is written in the running cell's own
# format (`_craft_db`): a Berkeley DB hash on bdb (the pin's own layout,
# `ngram_bdb.cpp:56`), a Kyoto Cabinet snapshot on kc (`ngram_kyotodb.cpp:54`)
# and a tkrzw `HashDBM` file on tkrzw (`ngram_tkrzwdb.cpp:48`), each read by
# that cell's pin through its own `Bigram` twin.
def crafted_bigram_context(k, rows):
    """Replace user_bigram.db with raw records and return an independently reopened context.

    Initialize and finalize once, write records in the current cell's format
    and overwrite user.conf before reopening. Assert both initializations;
    the returned native context is not cached or finalized here."""
    ctx = k.init()
    assert ctx, 'the first init failed'
    k.fn('fini', None, P)(ctx)
    _craft_hash(os.path.join(k.user, 'user_bigram.db'), rows)
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    return ctx


def predicted_bigram_context(k, phrase, value):
    """Reopen a profile with value stored under every token found for phrase.

    UTF-8 encode the phrase, assert token lookup and use four-byte little-endian
    tokens as bigram keys. Finalize the initial context, replace user_bigram.db
    and user.conf, then return a separately initialized context. Free the lookup
    instance but leave its GArray allocated; no kit context cache is updated."""
    ctx = k.init()
    assert ctx, 'the first init failed'
    inst = k.fn('alloc_instance', P, P)(ctx)
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    arr = new(0, 0, 4)
    assert k.fn('lookup_tokens', B, P, S, P)(inst, phrase.encode(), arr)
    view = C.cast(arr, C.POINTER(Arr)).contents
    tokens = list(C.cast(view.data, C.POINTER(U))[:view.len])
    k.fn('free_instance', None, P)(inst)
    k.fn('fini', None, P)(ctx)
    _craft_hash(os.path.join(k.user, 'user_bigram.db'),
                [(token.to_bytes(4, 'little'), value) for token in tokens])
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    return ctx


def issue1114_collision_saves(k):
    """Save and reopen a three-row tkrzw collision profile twice, checking rows and saved bytes.

    CONTRACT_1114_PROFILE replaces k.user when set. With CONTRACT_1114_RELOAD=1,
    open that profile as-is and compare both saves against its original bytes;
    otherwise create the fixed rows. Import one phrase before each save and
    assert unchanged rows and, for reloads, bytes after the timestamp mask.
    Return each file's byte length, masked SHA-256 and decoded rows. Finalize
    and reopen after each save, then finalize the last context."""
    rows = []
    for token, degree in ((0x01000d46, 1), (0x01004404, 6), (0x01004dd8, 100)):
        value = (69 * degree).to_bytes(4, 'little') + b''.join(
            item.to_bytes(4, 'little') + (69).to_bytes(4, 'little')
            for item in range(0x01000001, 0x01000001 + degree))
        rows.append((token.to_bytes(4, 'little'), value))
    # run_case supplies one donor directory per direction. The receiver
    # must open that exact profile, without crafting a replacement file.
    handoff = os.environ.get('CONTRACT_1114_PROFILE')
    if handoff:
        k.user = handoff
    reloading = os.environ.get('CONTRACT_1114_RELOAD') == '1'
    ctx = k.init() if reloading else crafted_bigram_context(k, rows)
    assert ctx
    path = Path(k.user, 'user_bigram.db')
    original = path.read_bytes() if reloading else None
    expected_rows = sorted([[key.hex(), value.hex()] for key, value in rows])
    saved = []
    for text, pinyin, zhuyin in (('达达', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ'),
                                ('你你', "ni3'ni3", 'ㄋㄧˇ ㄋㄧˇ')):
        iterator = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
        assert iterator
        reading = pinyin if k.mode == 'pinyin' else zhuyin
        assert k.fn('iterator_add_phrase', B, P, S, S, I)(
            iterator, text.encode(), reading.encode(), 100)
        k.fn('end_add_phrases', None, P)(iterator)
        assert k.fn('save', B, P)(ctx)
        raw = path.read_bytes()
        # HashDBM bytes include the bucket pointers and record links. Compare
        # every byte except the authorized timestamp, including byte 56.
        if original is not None:
            assert tkrzw_saved_bigram_bytes(raw) == tkrzw_saved_bigram_bytes(original), 'cross-reload changed saved bytes'
        actual_rows = index_file_rows(path, kind='hash')
        assert actual_rows == expected_rows, 'cross-reload changed a key or value'
        saved.append(dict(bytes=len(raw), sha256=hashlib.sha256(tkrzw_saved_bigram_bytes(raw)).hexdigest(),
                          rows=actual_rows))
        k.fn('fini', None, P)(ctx)
        ctx = k.init()
        assert ctx
    k.fn('fini', None, P)(ctx)
    return {'saved_bigram': saved}


for _issue1114_mode in ('pinyin', 'zhuyin'):
    case('issue1114-collision-load-save-' + _issue1114_mode, mode=_issue1114_mode,
         cells=('tkrzw',), issues=('1114',))(issue1114_collision_saves)


def predicted_bigram_row_context(k, phrase, successor, count=20):
    """Reopen with one counted successor stored under each token found for phrase.

    Use the first token for successor and encode count as unsigned four-byte
    little-endian total and item count. Assert lookups and reopening; replace
    the user bigram and profile marker, freeing the lookup instance but not
    its arrays. Return the uncached context. In src/pinyin.cpp at pin 074a2219,
    _compute_predicted_bigram_candidates requires a count of at least 10 and
    a successor length of one or two characters; this helper does not validate
    eligibility itself."""
    ctx = k.init()
    assert ctx, 'the first init failed'
    inst = k.fn('alloc_instance', P, P)(ctx)
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    found = []
    for text in (phrase, successor):
        arr = new(0, 0, 4)
        assert k.fn('lookup_tokens', B, P, S, P)(inst, text.encode(), arr)
        view = C.cast(arr, C.POINTER(Arr)).contents
        found.append(list(C.cast(view.data, C.POINTER(U))[:view.len]))
    k.fn('free_instance', None, P)(inst)
    k.fn('fini', None, P)(ctx)
    value = count.to_bytes(4, 'little') + found[1][0].to_bytes(4, 'little') + count.to_bytes(4, 'little')
    _craft_hash(os.path.join(k.user, 'user_bigram.db'), [(token.to_bytes(4, 'little'), value) for token in found[0]])
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    return ctx


# `pinyin_choose_candidate` takes no predicted row of either kind
# (`pinyin.cpp:2507`); the prefix arm is `abort-choose-candidate-predicted-prefix-row`.
# BDB-only coverage inherited from e7ad3ab9: the predicted-bigram row and
# pinyin.cpp:2507 abort were verified there. _craft_hash now supports kc and
# tkrzw too, but this case's row selection has not been verified on them;
# their exclusion is a coverage limit, not an unreadable fixture format.
@case('abort-choose-candidate-predicted-bigram-row', abort=0, cells=('bdb',))
def _(k):
    """Plant 我-to-你好 predictions and pass a kind-4 row to ordinary candidate selection.

    Assert the predicted-bigram row exists; record the signed selection result."""
    ctx = predicted_bigram_row_context(k, '我', '你好')
    inst = k.fn('alloc_instance', P, P)(ctx)
    k.fn('guess_predicted_candidates_with_punctuations', B, P, S)(inst, '我'.encode())
    row = row_of(k, inst, 4)
    assert row, 'the planted gram listed no predicted-bigram row'
    return {'ret': k.fn('choose_candidate', I, P, Z, P)(inst, 0, row)}


@case('abort-mask-out-short-bigram-value', abort=False)
def _(k):
    """Mask a profile whose token-one bigram value is three bytes long."""
    # The pin's `mask_out` loads the gram and `get_total_freq` asserts on
    # the three-byte value (`memory_chunk.h:390`, `ngram.cpp:80`).
    ctx = crafted_bigram_context(k, [(b'\x01\x00\x00\x00', b'\x07\x00\x00')])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


@case('abort-zhuyin-mask-out-short-bigram-value', mode='zhuyin', abort=False)
def _(k):
    """Run zhuyin mask_out with a three-byte bigram value under token one."""
    ctx = crafted_bigram_context(k, [(b'\x01\x00\x00\x00', b'\x07\x00\x00')])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


@case('mask-out-short-bigram-value-masked-key', control=True)
def _(k):
    """Mask token one itself in a profile carrying its three-byte bigram value.

    Record the native flag for deleting the key rather than editing its items."""
    # A short value whose key the mask erases wholesale is never loaded, so
    # the pin erases it and completes (`ngram_bdb.cpp:231-238`); the
    # `get_total_freq` assert is reached only for a key the mask leaves
    # alone. A control: the guard must not over-refuse here, so pin and
    # subject both complete and the parent build (no bigram checks) matches.
    ctx = crafted_bigram_context(k, [(b'\x01\x00\x00\x00', b'\x07\x00\x00')])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 1)}


@case('abort-mask-out-non-token-bigram-key', abort=False)
def _(k):
    """Mask a profile with a two-byte bigram key and a four-byte total value."""
    # The pin's `mask_out` walks the container with `get_all_items`, whose
    # `key.size == sizeof(phrase_token_t)` assert dies on the two-byte key
    # (`ngram_bdb.cpp:199`).
    ctx = crafted_bigram_context(k, [(b'\x00\x01', b'\x07\x00\x00\x00')])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


@case('abort-zhuyin-mask-out-non-token-bigram-key', mode='zhuyin', abort=False)
def _(k):
    """Run zhuyin mask_out against a two-byte bigram key with a four-byte value."""
    ctx = crafted_bigram_context(k, [(b'\x00\x01', b'\x07\x00\x00\x00')])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


# A gram whose `total_freq` is not covered by its items (total 7, one item of
# frequency 0): the mask removes the item (token 0 == value 0) but the key is
# left alone, so `SingleGram::mask_out` leaves a residual total and
# `Bigram::mask_out`'s `get_length` assert dies (`ngram.cpp:70`,
# `ngram_bdb.cpp:243`).
@case('abort-mask-out-residual-bigram-gram', abort=False)
def _(k):
    """Mask successor zero from a planted gram with total seven and a zero-count item."""
    ctx = crafted_bigram_context(
        k, [(b'\x01\x00\x00\x00', b'\x07\x00\x00\x00' + b'\x00' * 8)])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


@case('abort-zhuyin-mask-out-residual-bigram-gram', mode='zhuyin', abort=False)
def _(k):
    """Run the residual-total gram mask through zhuyin and record its return flag."""
    ctx = crafted_bigram_context(
        k, [(b'\x01\x00\x00\x00', b'\x07\x00\x00\x00' + b'\x00' * 8)])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


@case('abort-begin-bigram-phrases-non-token-key', abort=False)
def _(k):
    """Begin bigram export after planting a two-byte key, returning iterator-pointer truthiness.

    The iterator is not advanced or closed if creation returns."""
    # `pinyin_begin_get_bigram_phrases` walks the same container through
    # `get_all_items` (`pinyin.cpp:776-787`), so the two-byte key aborts at
    # `ngram_bdb.cpp:199` before any row is exported.
    ctx = crafted_bigram_context(k, [(b'\x00\x01', b'\x07\x00\x00\x00')])
    return {'ret': bool(k.fn('begin_get_bigram_phrases', P, P)(ctx))}


def issue677_sequence(k):
    """Record imports, masks, repeated saves and raw persistence of a seeded bigram key.

    Create a profile with one library-7 predecessor, import three phrases
    across five saves and mask user-library tokens after the first save.
    Return each flag, the file inventory and the key's raw value or absence
    both after masking and at the end; backend get results remain observable.
    The reopened context is retained for the worker."""
    ctx = crafted_bigram_context(
        k, [(b'\x01\x00\x00\x07', d18_gram(100, 0x01000001, 100))])

    def add(text, reading):
        """Import a phrase with count 100 into the captured context's user library.

        Encode text and reading as UTF-8, assert iterator/add success and close it."""
        it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
        assert it, 'the import iterator'
        assert k.fn('iterator_add_phrase', B, P, S, S, I)(
            it, text.encode(), reading.encode(), 100)
        k.fn('end_add_phrases', None, P)(it)

    def save():
        """Save the captured native context and return its boolean result."""
        return bool(k.fn('save', B, P)(ctx))

    reading = (lambda text: text) if k.mode == 'pinyin' else (lambda text: text)
    add('达达', "da2'da2" if k.mode == 'pinyin' else 'ㄉㄚˊ ㄉㄚˊ')
    ret1 = save()
    mask = bool(k.fn('mask_out', B, P, U, U)(ctx, 0x0f000000, 0x07000000))
    ret2 = save()
    path = Path(k.user, 'user_bigram.db')
    key = b'\x01\x00\x00\x07'
    after_mask = d18_bigram_db(k, path, read_keys=[key], allow_missing=True)[0]
    add('你你', "ni3'ni3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄋㄧˇ')
    ret3 = save()
    ret4 = save()
    add('好好', "hao3'hao3" if k.mode == 'pinyin' else 'ㄏㄠˇ ㄏㄠˇ')
    ret5 = save()
    result = {
        'ret1': ret1, 'mask': mask, 'ret2': ret2, 'ret3': ret3,
        'ret4': ret4, 'ret5': ret5,
        'files': sorted(os.listdir(k.user)),
        'removed_key_after_mask': after_mask.hex() if after_mask is not None else None,
    }
    # The raw DBM contract is the backend's own `Get` result. On a
    # library that retains a removed row, its value remains observable;
    # on a library that deletes it, the result is absent. Compare that
    # value, the return sequence and the user-file inventory in every cell.
    final = d18_bigram_db(k, path, read_keys=[key], allow_missing=True)[0]
    result['removed_key_final'] = final.hex() if final is not None else None
    return result


for _issue677_mode in ('pinyin', 'zhuyin'):
    case('issue677-mask-save-sequence-' + _issue677_mode, mode=_issue677_mode, issues=('677',))(issue677_sequence)


# #762: the pin hands a new user phrase `range.m_range_end` of the user
# sub-index (`pinyin.cpp:589-592`), and `SubPhraseIndex::get_range` trims
# trailing zero offsets (`phrase_index.cpp:624-650`), so a mask that empties
# the top of the range frees those tokens for the next import.
TOKEN_REUSE_PHRASES = (
    ('龘龘', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ'), ('龖龖', "ta4'ta4", 'ㄊㄚˋ ㄊㄚˋ'),
    ('靐靐', "bing3'bing3", 'ㄅㄧㄥˇ ㄅㄧㄥˇ'), ('鱻鱻', "xian1'xian1", 'ㄒㄧㄢ ㄒㄧㄢ'),
    ('麤麤', "cu1'cu1", 'ㄘㄨ ㄘㄨ'), ('厵厵', "yuan2'yuan2", 'ㄩㄢˊ ㄩㄢˊ'),
)


def token_reuse_sequence(k, steps, bigram_rows=None, library=7):
    """Execute import, save, mask and reopen steps and snapshot all seen phrase tokens.

    Steps are ('add', n), indexing TOKEN_REUSE_PHRASES, ('save',),
    ('mask', mask, value) or ('reopen',). Use the chosen library and facade
    reading column. A truthy bigram_rows
    seeds a replaced profile first; otherwise use the cached context. Import
    count is 100. Reopen finalizes the current context without implicitly
    saving or replacing the kit's cache. Return action results, per-step token
    mappings and normalized user-file contents. Unrecognized actions still
    produce a token snapshot."""
    ctx = crafted_bigram_context(k, bigram_rows) if bigram_rows else k.ctx
    column = 1 if k.mode == 'pinyin' else 2
    out, seen = [], []

    def tokens():
        """Look up every seen phrase with a temporary instance and return hexadecimal token lists.

        Free that instance after lookup; the per-phrase GArrays remain allocated."""
        inst = k.fn('alloc_instance', P, P)(ctx)
        new = k.glib.g_array_new
        new.restype, new.argtypes = P, [I, I, U]
        found = {}
        for n in seen:
            arr = new(0, 0, 4)
            k.fn('lookup_tokens', B, P, S, P)(inst, TOKEN_REUSE_PHRASES[n][0].encode(), arr)
            view = C.cast(arr, C.POINTER(Arr)).contents
            found[TOKEN_REUSE_PHRASES[n][0]] = [hex(t) for t in C.cast(view.data, C.POINTER(U))[:view.len]]
        k.fn('free_instance', None, P)(inst)
        return found

    for step in steps:
        if step[0] == 'add':
            it = k.fn('begin_add_phrases', P, P, U)(ctx, library)
            assert it, 'the import iterator'
            text, reading = TOKEN_REUSE_PHRASES[step[1]][0], TOKEN_REUSE_PHRASES[step[1]][column]
            out.append(['add', bool(k.fn('iterator_add_phrase', B, P, S, S, I)(it, text.encode(), reading.encode(), 100))])
            k.fn('end_add_phrases', None, P)(it)
            seen.append(step[1])
        elif step[0] == 'save':
            out.append(['save', bool(k.fn('save', B, P)(ctx))])
        elif step[0] == 'mask':
            out.append(['mask', bool(k.fn('mask_out', B, P, U, U)(ctx, step[1], step[2]))])
        elif step[0] == 'reopen':
            k.fn('fini', None, P)(ctx)
            ctx = k.init()
            assert ctx, 'the reopen failed'
        out.append(tokens())
    return {'steps': out, 'files': token_reuse_files(k)}


USER_INDEX_FILES = ('user_phrase_index.bin', 'user_pinyin_index.bin')


# tkrzw 9ee8416, tkrzw_dbm_hash.cc:46,1296: timestamp_ is written as an
# eight-byte microsecond wall clock. The maintainer's #1114 ruling permits
# only this comparison mask. Offset 56 starts db_type_ and stays exact.
META_OFFSET_TIMESTAMP = 48
TKRZW_TIMESTAMP_WIDTH = 8
TKRZW_HASH_METADATA_SIZE = 128


def tkrzw_saved_bigram_bytes(raw):
    """Return saved HashDBM bytes with only timestamp bytes 48 through 55 zeroed.

    Require at least the 128-byte metadata header or raise ValueError. Preserve
    all other bytes, including byte 56. This mask belongs only to the #1114
    user-bigram comparison."""
    if len(raw) < TKRZW_HASH_METADATA_SIZE:
        raise ValueError('saved HashDBM is shorter than its metadata')
    end = META_OFFSET_TIMESTAMP + TKRZW_TIMESTAMP_WIDTH
    return raw[:META_OFFSET_TIMESTAMP] + bytes(TKRZW_TIMESTAMP_WIDTH) + raw[end:]


def tkrzw_saved_bigram_equal(left, right, left_bucket_chains, right_bucket_chains):
    """Require saved-byte identity and exact ordered bucket chains.

    Chains must come from each side's native walk, grouped by bucket and
    kept in chain order; callers must never sort keys to compare them.
    """
    return (tkrzw_saved_bigram_bytes(left) == tkrzw_saved_bigram_bytes(right)
            and left_bucket_chains == right_bucket_chains)


def index_file_rows(path, *, kind='tree', allow_empty=False, library=None):
    """Return sorted [key hex, value hex] rows from the current cell's user container.

    kind must be tree or hash, otherwise raise ValueError. KC reads a snapshot
    through an in-memory DB; tkrzw opens TreeDBM or HashDBM read-only, using
    library as the exact shared-library path when supplied. Checked DB/cursor
    failures raise ValueError; opened readers are closed in finally blocks.
    Only tkrzw uses allow_empty: an initially empty iterator is an error unless
    it is true; KC and BDB already permit empty containers. Empty values remain.

    BDB launches db5.3_dump with type autodetection; process, decoding and dump
    parsing failures propagate. tkrzw's bigram format follows Bigram::load_db
    and ::save_db in src/storage/ngram_tkrzwdb.cpp at pin 074a2219."""
    if kind not in ('tree', 'hash'):
        raise ValueError('unknown profile container kind: ' + kind)
    rows = []
    if CELL == 'kc':
        kc = C.CDLL('libkyotocabinet.so.16')
        kc.kcdbnew.restype = P
        kc.kcdbdel.argtypes = [P]
        kc.kcdbopen.argtypes = [P, S, C.c_uint32]
        kc.kcdbloadsnap.argtypes = [P, S]
        kc.kcdbcursor.restype, kc.kcdbcursor.argtypes = P, [P]
        kc.kccurjump.argtypes = [P]
        kc.kccurget.restype = P
        kc.kccurget.argtypes = [P, C.POINTER(Z), C.POINTER(P), C.POINTER(Z), I]
        kc.kccurecode.argtypes, kc.kccurecode.restype = [P], I
        kc.kccurdel.argtypes = [P]
        kc.kcfree.argtypes = [P]
        kc.kcdbclose.argtypes = [P]
        db = kc.kcdbnew()
        if not db:
            raise ValueError('KC profile allocation failed: ' + str(path))
        opened, cur = False, None
        try:
            if not kc.kcdbopen(db, b'+', (1 << 1) | (1 << 2)):
                raise ValueError('KC profile memory open failed: ' + str(path))
            opened = True
            if not kc.kcdbloadsnap(db, str(path).encode()):
                raise ValueError('KC profile snapshot load failed: ' + str(path))
            cur = kc.kcdbcursor(db)
            if not cur:
                raise ValueError('KC profile cursor allocation failed: ' + str(path))
            if kc.kccurjump(cur):
                while True:
                    ksiz, vbuf, vsiz = Z(), P(), Z()
                    kbuf = kc.kccurget(cur, C.byref(ksiz), C.byref(vbuf), C.byref(vsiz), 1)
                    if not kbuf:
                        if kc.kccurecode(cur) != 7:  # KCENOREC, not a partial walk.
                            raise ValueError('KC profile cursor read failed: ' + str(path))
                        break
                    try:
                        rows.append([C.string_at(kbuf, ksiz.value).hex(), C.string_at(vbuf, vsiz.value).hex()])
                    finally:
                        kc.kcfree(kbuf)
            elif kc.kccurecode(cur) != 7:
                raise ValueError('KC profile cursor first failed: ' + str(path))
        finally:
            if cur:
                kc.kccurdel(cur)
            try:
                if opened and not kc.kcdbclose(db):
                    raise ValueError('KC profile close failed: ' + str(path))
            finally:
                kc.kcdbdel(db)
    elif CELL == 'tkrzw':
        tk = C.CDLL(str(library) if library is not None else 'libtkrzw.so.1')
        libc = C.CDLL(None)
        libc.free.argtypes = [P]
        tk.tkrzw_dbm_open.restype, tk.tkrzw_dbm_open.argtypes = P, [S, B, S]
        tk.tkrzw_dbm_make_iterator.restype, tk.tkrzw_dbm_make_iterator.argtypes = P, [P]
        tk.tkrzw_dbm_iter_first.restype, tk.tkrzw_dbm_iter_first.argtypes = B, [P]
        tk.tkrzw_dbm_iter_get.restype = B
        tk.tkrzw_dbm_iter_get.argtypes = [P, C.POINTER(P), C.POINTER(C.c_int32), C.POINTER(P), C.POINTER(C.c_int32)]
        tk.tkrzw_dbm_iter_next.restype, tk.tkrzw_dbm_iter_next.argtypes = B, [P]
        tk.tkrzw_dbm_iter_free.argtypes = [P]
        tk.tkrzw_dbm_close.restype, tk.tkrzw_dbm_close.argtypes = B, [P]
        tk.tkrzw_get_last_status_code.restype = I
        tk.tkrzw_get_last_status_code.argtypes = []
        params = b'dbm=HashDBM' if kind == 'hash' else b'dbm=TreeDBM'
        db = tk.tkrzw_dbm_open(str(path).encode(), False, params)
        if not db:
            raise ValueError('tkrzw profile read-only open failed: ' + str(path))
        it = None
        try:
            it = tk.tkrzw_dbm_make_iterator(db)
            if not it:
                raise ValueError('tkrzw profile iterator allocation failed: ' + str(path))
            if not tk.tkrzw_dbm_iter_first(it):
                if not allow_empty or tk.tkrzw_get_last_status_code() != 7:
                    raise ValueError('tkrzw profile iterator first failed: ' + str(path))
                return []
            while True:
                kptr, ksiz, vptr, vsiz = P(), C.c_int32(), P(), C.c_int32()
                if not tk.tkrzw_dbm_iter_get(it, C.byref(kptr), C.byref(ksiz), C.byref(vptr), C.byref(vsiz)):
                    if tk.tkrzw_get_last_status_code() != 7:
                        raise ValueError('tkrzw profile iterator read failed: ' + str(path))
                    break
                try:
                    rows.append([C.string_at(kptr, ksiz.value).hex(), C.string_at(vptr, vsiz.value).hex()])
                finally:
                    libc.free(kptr)
                    libc.free(vptr)
                if not tk.tkrzw_dbm_iter_next(it):
                    if tk.tkrzw_get_last_status_code() != 7:
                        raise ValueError('tkrzw profile iterator next failed: ' + str(path))
                    break
        finally:
            if it:
                tk.tkrzw_dbm_iter_free(it)
            if not tk.tkrzw_dbm_close(db):
                raise ValueError('tkrzw profile close failed: ' + str(path))
    else:
        dump = subprocess.run(['db5.3_dump', str(path)], check=True, capture_output=True).stdout.decode()
        body = dump.split('DATA=END')[0].split('HEADER=END\n')[1].split('\n')[:-1]
        rows = [[body[i].strip(), body[i + 1].strip()] for i in range(0, len(body), 2)]
    return sorted(rows)


def zero_item_padding(key_hex, value_hex):
    """Zero two padding bytes per complete odd-length pinyin-index item in a hex value.

    Derive key length L in two-byte words from len(key_hex)//4. For odd L,
    process complete 4+2L+2-byte items and leave an incomplete tail untouched;
    for even L or an empty value, return the original string without decoding.
    Invalid value hex raises ValueError on the odd-length path. The masked
    layout is PinyinIndexItem2 in src/storage/pinyin_phrase3.h at pin
    074a2219; only its alignment padding is normalized."""
    length = len(key_hex) // 4
    if length % 2 == 0 or not value_hex:
        return value_hex
    stride = 4 + 2 * length + 2
    raw = bytearray(bytes.fromhex(value_hex))
    for start in range(0, len(raw) - stride + 1, stride):
        raw[start + stride - 2:start + stride] = b'\0\0'
    return raw.hex()


def token_reuse_files(k):
    """Read normalized contents of regular user-profile files, excluding .tmp files.

    Decode index containers record by record, including empty values, and zero
    pinyin-item padding. For non-BDB bigram files report only whether bytes
    exist. For BDB-magic containers launch db5.3_dump and retain nonempty rows.
    Zero header bytes 4..7 in user.bin, addon.bin and network.bin; encode other
    files in hex. File, reader and subprocess failures propagate."""
    files = {}
    for name in sorted(os.listdir(k.user)):
        path = Path(k.user, name)
        if not path.is_file() or name.endswith('.tmp'):
            continue
        raw = path.read_bytes()
        if name in USER_INDEX_FILES:
            rows = index_file_rows(path)
            if name == 'user_pinyin_index.bin':
                rows = [[key, zero_item_padding(key, value)] for key, value in rows]
            files[name] = rows
        elif CELL != 'bdb' and name == 'user_bigram.db':
            files[name] = len(raw) > 0  # another backend's container: only its presence is compared
        elif raw[12:16] in (b'\x62\x31\x05\x00', b'\x61\x15\x06\x00'):
            dump = subprocess.run(['db5.3_dump', str(path)], check=True, capture_output=True).stdout.decode()
            body = dump.split('DATA=END')[0].split('HEADER=END\n')[1].split('\n')[:-1]
            files[name] = [[body[i].strip(), body[i + 1].strip()] for i in range(0, len(body), 2) if body[i + 1].strip()]
        elif name in ('user.bin', 'addon.bin', 'network.bin'):
            files[name] = (raw[:4] + b'\0\0\0\0' + raw[8:]).hex()
        else:
            files[name] = raw.hex()
    return files


USER_ALL = (0x0f000000, 0x07000000)
TOKEN_REUSE_SEQUENCES = {
    # name: (control, steps). A control holds on the parent too: it has no
    # mask, or reopens first (the load seeds the cursor from the saved items,
    # which already reproduced the pin). A mask below the top of the range
    # hands out the token the parent did, but the save after it writes the
    # masked phrase's index keys back empty (#784).
    'no-mask': (True, [('add', 0), ('add', 1), ('save',), ('add', 2), ('save',)]),
    'mask-all': (False, [('add', 0), ('save',), ('mask',) + USER_ALL, ('save',), ('add', 1), ('save',)]),
    'mask-highest': (False, [('add', 0), ('add', 1), ('add', 2), ('save',), ('mask', 0xffffffff, 0x07000003),
                             ('save',), ('add', 3), ('save',)]),
    'mask-middle': (False, [('add', 0), ('add', 1), ('add', 2), ('save',), ('mask', 0xffffffff, 0x07000002),
                            ('save',), ('add', 3), ('save',)]),
    'mask-twice': (False, [('add', 0), ('add', 1), ('add', 2), ('save',), ('mask', 0xffffffff, 0x07000003),
                           ('mask', 0xffffffff, 0x07000002), ('add', 3), ('add', 4), ('save',)]),
    'mask-import-mask': (False, [('add', 0), ('add', 1), ('save',), ('mask', 0xffffffff, 0x07000002), ('add', 2),
                                 ('mask', 0xffffffff, 0x07000002), ('add', 3), ('save',)]),
    'mask-reopen': (True, [('add', 0), ('add', 1), ('save',), ('mask', 0xffffffff, 0x07000002), ('save',),
                            ('reopen',), ('add', 2), ('save',)]),
    'mask-unsaved-reopen': (True, [('add', 0), ('add', 1), ('save',), ('mask', 0xffffffff, 0x07000002),
                                    ('reopen',), ('add', 2), ('save',)]),
    'mask-reopen-middle': (True, [('add', 0), ('add', 1), ('add', 2), ('save',), ('mask', 0xffffffff, 0x07000002),
                                  ('save',), ('reopen',), ('add', 3), ('save',)]),
}
# A user bigram row under the token and another that lists it as a successor:
# the pin's `mask_out` drops both, so the reused token starts clean.
TOKEN_REUSE_BIGRAM_ROWS = [
    (b'\x01\x00\x00\x07', b''.join(n.to_bytes(4, 'little') for n in (100, 0x01000001, 100))),
    (b'\x01\x00\x00\x01', b''.join(n.to_bytes(4, 'little') for n in (100, 0x07000001, 100))),
]
for _mode in ('pinyin', 'zhuyin'):
    for _name, (_control, _steps) in TOKEN_REUSE_SEQUENCES.items():
        case('issue762-token-reuse-%s-%s' % (_name, _mode), mode=_mode, control=_control, issues=('762',))(
            lambda k, steps=_steps: token_reuse_sequence(k, steps))
    # BDB only: token_reuse_files compares the saved bigram's non-empty rows
    # through db_dump; kc/tkrzw report only its presence, which cannot check
    # that the seeded stale successor was removed.
    case('issue762-token-reuse-stale-bigram-%s' % _mode, mode=_mode, cells=('bdb',), issues=('762',))(
        lambda k: token_reuse_sequence(k, TOKEN_REUSE_SEQUENCES['mask-all'][1], TOKEN_REUSE_BIGRAM_ROWS))
    # The no-mask control needs the same BDB row comparison to establish
    # that the seeded bigram survives; kc/tkrzw presence alone cannot do so.
    case('issue762-token-reuse-stale-bigram-no-mask-%s' % _mode, mode=_mode, control=True, cells=('bdb',), issues=('762',))(
        lambda k: token_reuse_sequence(k, TOKEN_REUSE_SEQUENCES['no-mask'][1], TOKEN_REUSE_BIGRAM_ROWS))


# The pin takes the next token from the trimmed range end for every index
# (`_add_phrase`, `pinyin.cpp:586-592`) and for addon promotion
# (`pinyin.cpp:2541-2544`, which has no `token++` over a zero low word), so the
# ADDON_DICTIONARY sub-index (5) frees its top the same way the user one does.
ADDON_ALL = (0x0f000000, 0x05000000)
ADDON_IMPORT_SEQUENCES = {
    'no-mask': (True, [('add', 0), ('add', 1), ('add', 2), ('save',), ('add', 3), ('save',)]),
    'mask-highest': (False, [('add', 0), ('add', 1), ('add', 2), ('save',), ('mask', 0xffffffff, 0x05000003),
                             ('add', 3), ('save',)]),
    'mask-middle': (False, [('add', 0), ('add', 1), ('add', 2), ('save',), ('mask', 0xffffffff, 0x05000002),
                            ('add', 3), ('save',)]),
}
for _mode in ('pinyin', 'zhuyin'):
    for _name, (_control, _steps) in ADDON_IMPORT_SEQUENCES.items():
        case('issue762-addon-import-%s-%s' % (_name, _mode), mode=_mode, control=_control, issues=('762',))(
            lambda k, steps=_steps: token_reuse_sequence(k, steps, library=5))

ADDON_PROMOTE_INPUTS = (('shuxue', '书学'), ('wuji', '武技'), ('wusheng', '武生'), ('wuxi', '武戏'))


def addon_promote_sequence(k, mask_value):
    """Promote four addon candidates, optionally mask a token, and report tokens and saved files.

    Load addon libraries 4 and 5. Promote the first three inputs, save, apply
    mask_value if non-None, then promote the fourth and save again without
    reopening. Assert library loads and candidate presence; record selection,
    mask and save results. The cached context retains all changes."""
    ctx = k.ctx
    for index in (4, 5):
        assert k.fn('load_addon_phrase_library', B, P, C.c_ubyte)(ctx, index)
    out, texts = [], []

    def promote(pinyin):
        """Choose the first addon row for pinyin, append its result and free the temporary instance.

        Assert a kind-6 candidate exists after parsing and guessing at byte zero."""
        inst = k.fn('alloc_instance', P, P)(ctx)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, pinyin.encode())
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
        row = row_of(k, inst, 6)
        assert row, 'no addon candidate for ' + pinyin
        out.append(['promote', pinyin, k.fn('choose_candidate', I, P, Z, P)(inst, 0, row)])
        k.fn('free_instance', None, P)(inst)

    def tokens():
        """Return token IDs for all promoted texts using a temporary native instance.

        Free the instance after lookup but retain the allocated GArrays."""
        inst = k.fn('alloc_instance', P, P)(ctx)
        new = k.glib.g_array_new
        new.restype, new.argtypes = P, [I, I, U]
        found = {}
        for text in texts:
            arr = new(0, 0, 4)
            k.fn('lookup_tokens', B, P, S, P)(inst, text.encode(), arr)
            view = C.cast(arr, C.POINTER(Arr)).contents
            found[text] = [hex(t) for t in C.cast(view.data, C.POINTER(U))[:view.len]]
        k.fn('free_instance', None, P)(inst)
        return found

    for pinyin, text in ADDON_PROMOTE_INPUTS[:3]:
        promote(pinyin)
        texts.append(text)
        out.append(tokens())
    out.append(['save', bool(k.fn('save', B, P)(ctx))])
    if mask_value is not None:
        out.append(['mask', bool(k.fn('mask_out', B, P, U, U)(ctx, 0xffffffff, mask_value))])
        out.append(tokens())
    promote(ADDON_PROMOTE_INPUTS[3][0])
    texts.append(ADDON_PROMOTE_INPUTS[3][1])
    out.append(tokens())
    out.append(['save', bool(k.fn('save', B, P)(ctx))])
    return {'steps': out, 'files': token_reuse_files(k)}


def candidate_rows(k, inst):
    """Every row of the instance's list as `[type, text]`, in list order."""
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    rows = []
    for i in range(count.value):
        cand, kind, text = P(), I(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        rows.append([kind.value, text.value.decode()])
    return rows


# A promoted addon phrase leaves two rows with one string in the next list:
# the NORMAL row of the new nibble-5 token and the ADDON row. The phrase-string
# dedup keeps the row with the larger `m_freq`, and a tie keeps the earlier row
# (`_remove_duplicated_items_by_phrase_string`, `pinyin.cpp:2128-2138`). The
# ADDON row is scaled by the addon index total (`:1828-1842`) and the NORMAL row
# by the whole index total (`:1855-1866`), so the ADDON row survives.
def addon_promoted_listing(k, sort_option):
    """Compare 书学 candidate rows before and after promoting its addon entry.

    Load addon library 4 and generate fresh lists with sort_option. Assert a
    matching kind-6 row, record its selection result, both counts and matching
    rows, and the entire final list. Free the two instances on normal completion."""
    ctx = k.ctx
    assert k.fn('load_addon_phrase_library', B, P, C.c_ubyte)(ctx, 4)
    out = {}

    def listing(label):
        """Parse shuxue on a new instance and store the list size and 书学 rows under label.

        Return the live instance and complete rows for the caller to inspect and free."""
        inst = k.fn('alloc_instance', P, P)(ctx)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'shuxue')
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, sort_option)
        rows = candidate_rows(k, inst)
        out[label] = [len(rows), [row for row in rows if row[1] == '书学']]
        return inst, rows

    inst, rows = listing('before')
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    chosen = None
    for i in range(count.value):
        cand, kind, text = P(), I(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        if kind.value == 6 and text.value.decode() == '书学':
            chosen = cand
            break
    assert chosen, 'no addon candidate for shuxue'
    out['choose'] = k.fn('choose_candidate', I, P, Z, P)(inst, 0, chosen)
    k.fn('free_instance', None, P)(inst)
    inst, rows = listing('after')
    k.fn('free_instance', None, P)(inst)
    out['rows after'] = rows
    return out


# `sort_option` 0 sorts by nothing, so the two rows stay in collection order
# (NORMAL first) and only the dedup picks the ADDON row. 0x1c is
# `SORT_BY_PHRASE_LENGTH | SORT_BY_PINYIN_LENGTH | SORT_BY_FREQUENCY`: the sort
# puts the larger `m_freq` (the ADDON row) first, so a keep-first dedup also
# answers the pin's row; that case is the control.
case('addon-promoted-listing-unsorted', mode='pinyin', issues=('913',))(
    lambda k: addon_promoted_listing(k, 0))
case('addon-promoted-listing-by-frequency', mode='pinyin', control=True, issues=('913',))(
    lambda k: addon_promoted_listing(k, 0x1c))


# At 074a2219, each before-cursor slice keeps its `m_freq` when appended
# (`zhuyin.cpp:1607-1612`). The final dedup compares all slices (:1630,
# :1406-1416). A parse keeps the last sentence result (:1017-1042).
# Guess `nianhao`, then parse `ni''hao`: result[4] still holds the second
# token. The separator starts have no predecessor; start 4 has the first
# token (:1188-1197). Its bigram lets the later slice win. With the bit
# clear the rows tie.
def before_cursor_frequency_listing(k, dynamic):
    """Observe before-cursor candidates after a guessed phrase is reparsed with separators.

    Import 甲/乙, save and replace the user bigram with million-count rows.
    Set USE_TONE and optional DYNAMIC_ADJUST, assert complete nianhao and
    ni''hao parses, and choose the first 乙 row at the end cursor. Return rows,
    flags, parsed byte length and selection offset; assert that row was found."""
    row58_import(k, [('甲', 'ㄋㄧㄢˊ', 1000000), ('乙', 'ㄏㄠˇ', 1000000)])
    first, second = (tokens_of(k, text)[-1] for text in ('甲', '乙'))
    assert k.fn('save', B, P)(k.ctx)
    _replace_user_bigram(k, [(1, _gram_bytes(1000000, [(first, 1000000)])),
                             (first, _gram_bytes(1000000, [(second, 1000000)]))])
    assert k.fn('set_options', B, P, U)(k.ctx, 0x20 | (0x200 if dynamic else 0))
    inst = k.inst
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nianhao') == 7
    guessed = k.fn('guess_sentence', B, P)(inst)
    assert guessed
    parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b"ni''hao")
    assert parsed == 7
    ret = k.fn('guess_candidates_before_cursor', B, P, Z)(inst, parsed)
    rows = candidate_rows(k, inst)
    chosen = None
    for index, (_, text) in enumerate(rows):
        if text == '乙':
            candidate = P()
            assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(candidate))
            chosen = k.fn('choose_candidate', I, P, Z, P)(inst, parsed, candidate)
            break
    assert chosen is not None, 'the imported before-cursor row is missing'
    return dict(parsed=parsed, guessed=guessed, ret=ret, rows=rows, chosen=chosen)


case('before-cursor-frequency-dedup', mode='zhuyin', issues=('913',))(
    lambda k: before_cursor_frequency_listing(k, True))
case('before-cursor-frequency-tie', mode='zhuyin', control=True, issues=('913',))(
    lambda k: before_cursor_frequency_listing(k, False))


# The divided table adds xi + an to `xian` (phonetic_key_matrix.cpp:169-217,
# special_table.h:27). Two libraries hold one text with xi'an and xian'ni
# readings. New items add both index entries (pinyin.cpp:584-605). The first
# is NORMAL on the divided path; the second is LONGER on the whole key.
# LONGER has only a unigram term (pinyin.cpp:1916-1927), while NORMAL gets
# the planted bigram (:1845-1866). The final dedup sees both (:2292-2300).
def longer_frequency_listing(k, dynamic, longer=True):
    """Compare normal and longer rows for two imported readings of 甲乙.

    Import into libraries 6 and 7, save and replace the user bigram to favor
    the library-6 token. Enable divided/resplit parsing and optional dynamic
    adjustment, assert a complete xian parse and, when requested, the longer
    row at end. Return candidate lists before/after sentence guessing and with
    sentence rows suppressed, retaining the cached context's mutations."""
    for library, reading in ((6, "xi1'an1"), (7, "xian1'ni1")):
        iterator = k.fn('begin_add_phrases', P, P, U)(k.ctx, library)
        assert iterator
        assert k.fn('iterator_add_phrase', B, P, S, S, I)(
            iterator, '甲乙'.encode(), reading.encode(), 10000000)
        k.fn('end_add_phrases', None, P)(iterator)
    token, = (token for token in tokens_of(k, '甲乙') if token >> 24 == 6)
    assert k.fn('save', B, P)(k.ctx)
    _replace_user_bigram(k, [(1, _gram_bytes(1000000, [(token, 1000000)]))])
    assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | (0x200 if dynamic else 0))
    inst = k.inst
    parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'xian')
    assert parsed == 4
    sort = 1 if longer else 3
    out = dict(parsed=parsed, at_end=guess_rows(k, inst, parsed, sort))
    if longer:
        assert [7, '甲乙'] in out['at_end']['rows'], 'the LONGER fixture row is missing'
    out['without_sentence'] = guess_rows(k, inst, 0, sort)
    out['guess_sentence'] = k.fn('guess_sentence', B, P)(inst)
    out['with_sentence'] = guess_rows(k, inst, 0, sort & ~1)
    out['sentence_suppressed'] = guess_rows(k, inst, 0, sort)
    return out


case('longer-normal-frequency-dedup', issues=('913',))(
    lambda k: longer_frequency_listing(k, True))
# With sentence rows suppressed the old path did no final dedup. Even
# the tie case must remove the duplicate NORMAL row (:2298-2300).
case('longer-normal-frequency-tie', issues=('913',))(
    lambda k: longer_frequency_listing(k, False))
case('longer-normal-disabled', control=True, issues=('913',))(
    lambda k: longer_frequency_listing(k, True, longer=False))

case('issue762-addon-promote-no-mask', control=True, issues=('762',))(lambda k: addon_promote_sequence(k, None))
case('issue762-addon-promote-mask-highest', issues=('762',))(lambda k: addon_promote_sequence(k, 0x05000003))
case('issue762-addon-promote-mask-middle', control=True, issues=('762',))(lambda k: addon_promote_sequence(k, 0x05000002))


# #784: the pin never deletes a key from its user index tables. `mask_out`
# rewrites every record in place (`phrase_large_table3_bdb.cpp:391-405` and
# `chewing_large_table2_bdb.cpp:488-537` put with `DB_CURRENT`; the kc visitors
# return the masked value, `phrase_large_table3_kyotodb.cpp:361-370` and
# `chewing_large_table2_kyotodb.cpp:459-506`; the tkrzw processors return it,
# `phrase_large_table3_tkrzwdb.cpp:288-296` and
# `chewing_large_table2_tkrzwdb.cpp:429-473`), and `pinyin_remove_user_candidate`'s
# `remove_index` calls store the emptied entry back
# (`phrase_large_table3_bdb.cpp:353-368`, `chewing_large_table2_bdb.cpp:417-432`,
# `phrase_large_table3_kyotodb.cpp:334-345`, `phrase_large_table3_tkrzwdb.cpp:265-274`).
# A key whose last record left stays with an empty value, and every later save
# copies it. Each step that saves records both index files key by key.
INDEX_KEY_READINGS = {
    # The facade's own parse input for each TOKEN_REUSE_PHRASES reading.
    'pinyin': ('dada', 'tata', 'bingbing', 'xianxian', 'cucu', 'yuanyuan'),
    'zhuyin': ('286286', 'w84w84', '1u/31u/3', 'vu0 vu0 ', 'hj hj ', 'm06m06'),
}


def index_key_sequence(k, steps, phrases=None, readings=None):
    """Execute user-library mutations and collect saved-file or lookup observations.

    Support ('add', n), ('save',), ('mask', mask, value), ('reopen',), ('unload',)
    and ('load',); load/unload target library 7. ('remove', n) parses reading n
    and removes the first normal user candidate, asserting one exists.
    ('lookup', n) reports candidates, phrase and first-character tokens, plus
    pinyin predictions. ('guess', pinyin, zhuyin) selects the facade's input and
    records candidates, without a sentence guess. Unrecognized steps are ignored.

    Default phrases/readings are TOKEN_REUSE_PHRASES and INDEX_KEY_READINGS.
    Imports use count 100, saves snapshot normalized files, and reopening
    finalizes the current context without implicitly saving or updating the
    kit cache. Free temporary instances after each observation; lookup arrays
    remain allocated. Return the ordered steps list."""
    ctx = k.ctx
    phrases = TOKEN_REUSE_PHRASES if phrases is None else phrases
    readings = INDEX_KEY_READINGS if readings is None else readings
    column = 1 if k.mode == 'pinyin' else 2
    parse = 'parse_more_full_pinyins' if k.mode == 'pinyin' else 'parse_more_chewings'
    out = []

    def tokens(inst, text):
        """Return lookup status and hexadecimal tokens for UTF-8 text in a newly allocated GArray.

        Copy the values into Python lists without freeing the native array."""
        arr = k.glib.g_array_new(0, 0, 4)
        ret = k.fn('lookup_tokens', B, P, S, P)(inst, text.encode(), arr)
        view = C.cast(arr, C.POINTER(Arr)).contents
        return [ret, [hex(t) for t in C.cast(view.data, C.POINTER(U))[:view.len]]]

    def candidates(inst, limit=12):
        """Return total candidate count and up to limit [kind, UTF-8 text] rows.

        Use the current list, ignore accessor flags and decode text strictly."""
        count = U()
        k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
        rows = []
        for i in range(min(count.value, limit)):
            cand, kind, text = P(), I(), S()
            k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
            k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
            k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
            rows.append([kind.value, text.value.decode()])
        return [count.value, rows]

    def guess(inst, n):
        """Parse an explicit string or indexed fixture reading and populate candidates at zero.

        Choose the facade's parser and candidate API; do not guess a sentence or
        assert native return flags."""
        reading = (n if isinstance(n, str) else readings[k.mode][n]).encode()
        k.fn(parse, Z, P, S)(inst, reading)
        if k.mode == 'pinyin':
            k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
        else:
            k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)

    k.glib.g_array_new.restype, k.glib.g_array_new.argtypes = P, [I, I, U]
    for step in steps:
        if step[0] == 'add':
            it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
            assert it, 'the import iterator'
            text, reading = phrases[step[1]][0], phrases[step[1]][column]
            out.append(['add', bool(k.fn('iterator_add_phrase', B, P, S, S, I)(it, text.encode(), reading.encode(), 100))])
            k.fn('end_add_phrases', None, P)(it)
        elif step[0] == 'save':
            out.append(['save', bool(k.fn('save', B, P)(ctx)), token_reuse_files(k)])
        elif step[0] == 'mask':
            out.append(['mask', bool(k.fn('mask_out', B, P, U, U)(ctx, step[1], step[2]))])
        elif step[0] == 'reopen':
            k.fn('fini', None, P)(ctx)
            ctx = k.init()
            assert ctx, 'the reopen failed'
        elif step[0] in ('unload', 'load'):
            out.append([step[0], bool(k.fn(step[0] + '_phrase_library', B, P, U)(ctx, 7))])
        elif step[0] == 'remove':
            inst = k.fn('alloc_instance', P, P)(ctx)
            guess(inst, step[1])
            rows = _user_candidate_rows(k, inst)
            assert rows, 'no user candidate to remove'
            out.append(['remove', bool(k.fn('remove_user_candidate', B, P, P)(inst, rows[0]))])
            k.fn('free_instance', None, P)(inst)
        elif step[0] == 'guess':
            inst = k.fn('alloc_instance', P, P)(ctx)
            guess(inst, step[1] if k.mode == 'pinyin' else step[2])
            out.append(['guess', step[1], candidates(inst)])
            k.fn('free_instance', None, P)(inst)
        elif step[0] == 'lookup':
            text = phrases[step[1]][0]
            inst = k.fn('alloc_instance', P, P)(ctx)
            guess(inst, step[1])
            seen = {'candidates': candidates(inst), 'tokens': tokens(inst, text)}
            # #785: on kc and tkrzw a partial mask leaves the previous record's
            # post-mask tail in the first character's phrase-table key, and this
            # lookup returns it; bdb is the control that carries nothing.
            seen['prefix tokens'] = tokens(inst, text[0])
            if k.mode == 'pinyin':
                seen['predicted'] = predicted_rows(k, inst, text[0])
            out.append(['lookup', text, seen])
            k.fn('free_instance', None, P)(inst)
    return {'steps': out}


MIDDLE, HIGHEST = (0xffffffff, 0x07000002), (0xffffffff, 0x07000003)
SAVED = [('add', 0), ('add', 1), ('add', 2), ('save',)]
INDEX_KEY_SEQUENCES = {
    # name: (control, steps). A control holds on the parent too: no key is
    # emptied, or no save writes the files after one is. Neither `mask_out`
    # nor `remove_user_candidate` marks the context modified, so a save right
    # after one answers false and writes nothing, at the pin and here alike;
    # the import after it is what makes the next save write the tables.
    'no-mask': (True, SAVED + [('add', 3), ('save',)]),
    'mask-save': (True, SAVED + [('mask',) + MIDDLE, ('save',)]),
    'mask-middle': (False, SAVED + [('mask',) + MIDDLE, ('add', 3), ('save',)]),
    # The highest token is handed out again (#762), under another text.
    'mask-highest': (False, SAVED + [('mask',) + HIGHEST, ('add', 3), ('save',)]),
    'mask-middle-highest': (False, SAVED + [('mask',) + MIDDLE, ('mask',) + HIGHEST, ('add', 3), ('save',)]),
    'mask-unsaved': (False, [('add', 0), ('add', 1), ('add', 2), ('mask',) + MIDDLE, ('save',)]),
    'mask-all': (False, [('add', 0), ('add', 1), ('save',), ('mask',) + USER_ALL, ('add', 2), ('save',)]),
    # The emptied keys come back from the file: the next session's save keeps
    # them, and the masked phrase imported again fills its own keys.
    'mask-reopen': (False, SAVED + [('mask',) + MIDDLE, ('add', 3), ('save',), ('reopen',), ('add', 4),
                                    ('save',), ('reopen',), ('add', 1), ('save',)]),
    # What a fresh instance shows after the mask: in the masking session, and
    # after a reopen that loaded the emptied keys from the file.
    'mask-lookup': (True, SAVED + [('mask',) + MIDDLE, ('lookup', 1), ('lookup', 0)]),
    'mask-reopen-lookup': (False, SAVED + [('mask',) + MIDDLE, ('add', 3), ('save',), ('reopen',),
                                           ('lookup', 1), ('lookup', 0)]),
}
for _mode in ('pinyin', 'zhuyin'):
    for _name, (_control, _steps) in INDEX_KEY_SEQUENCES.items():
        case('issue784-index-keys-%s-%s' % (_name, _mode), mode=_mode, control=_control, issues=('784',))(
            lambda k, steps=_steps: index_key_sequence(k, steps))
# `pinyin_remove_user_candidate` (the zhuyin facade has none).
REMOVE_SEQUENCES = {
    'remove-save': (True, SAVED + [('remove', 1), ('save',)]),
    'remove-middle': (False, SAVED + [('remove', 1), ('add', 3), ('save',)]),
    'remove-unsaved': (False, [('add', 0), ('add', 1), ('add', 2), ('remove', 2), ('save',)]),
    'remove-reopen': (False, [('add', 0), ('add', 1), ('save',), ('remove', 0), ('add', 2), ('save',),
                              ('reopen',), ('add', 3), ('save',)]),
    'remove-lookup': (True, [('add', 0), ('add', 1), ('save',), ('remove', 1), ('lookup', 1), ('lookup', 0)]),
    'remove-reopen-lookup': (False, [('add', 0), ('add', 1), ('save',), ('remove', 1), ('add', 2), ('save',),
                                     ('reopen',), ('lookup', 1), ('lookup', 0)]),
}
for _name, (_control, _steps) in REMOVE_SEQUENCES.items():
    case('issue784-index-keys-%s' % _name, control=_control, issues=('784',))(
        lambda k, steps=_steps: index_key_sequence(k, steps))


# #785: on kc and tkrzw the user pinyin table's mask walk starts every key
# length from the table entry the last operation of that length left. Two
# phrases share the initials `d d`, so that record holds two items where the
# others hold one; a search hit or an `add_index` on it leaves both in the
# entry, and the first record the walk meets in that length picks up the
# second as its tail. The single-syllable phrase gives length 1 a record
# that the empty prefix markers after it inherit whole.
TAIL_PHRASES = (
    ('龘龘', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ'), ('龖龖', "ta4'ta4", 'ㄊㄚˋ ㄊㄚˋ'),
    ('靐靐', "bing3'bing3", 'ㄅㄧㄥˇ ㄅㄧㄥˇ'), ('鬥鬥', "dou4'dou4", 'ㄉㄡˋ ㄉㄡˋ'),
    ('龘', 'da2', 'ㄉㄚˊ'), ('鱻鱻', "xian1'xian1", 'ㄒㄧㄢ ㄒㄧㄢ'),
    ('龗龗', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ'),
)
TAIL_READINGS = {
    'pinyin': ('dada', 'tata', 'bingbing', 'doudou', 'da', 'xianxian', 'dada'),
    'zhuyin': ('286286', 'w84w84', '1u/31u/3', '2.42.4', '286', 'vu0 vu0 ', '286286'),
}
DD, D = ('guess', 'dada', '286286'), ('guess', 'da', '286')
SHARED = [('add', 0), ('add', 6), ('add', 3), ('add', 1), ('add', 2)]
# A mask that leaves no item behind, and a lookup that reads no tail, hold on
# the parent as well.
TAIL_CONTROLS = ('mask-all', 'lookup')
MASK_TA, MASK_DA2 = ('mask', 0xffffffff, 0x07000004), ('mask', 0xffffffff, 0x07000002)
TAIL_SEQUENCES = {
    # The add of the second `da da` item leaves that record in the length-2 entry.
    'add': SHARED + [MASK_TA, ('save',)],
    # A search hit on the record does the same.
    'hit': SHARED + [('save',), DD, MASK_TA, ('add', 5), ('save',)],
    'hit-shrink': SHARED + [('save',), DD, MASK_DA2, ('add', 5), ('save',)],
    # A suggestion walk empties the entries of the longer lengths.
    'suggestion': SHARED + [DD, D, MASK_TA, ('save',)],
    # The entry the first walk leaves is where the second starts.
    'mask-twice': SHARED + [DD, MASK_TA, MASK_DA2, ('save',)],
    # A hit on a record that carries a tail.
    'hit-tailed': SHARED + [DD, MASK_DA2, DD, MASK_TA, ('save',)],
    # One guess hits two different length-2 records, so the last hit the
    # engine makes is the one the walk starts from.
    'hit-order': SHARED + [('save',), ('guess', 'dadatata', '286286w84w84'), MASK_TA, ('add', 5),
                           ('save',)],
    'hit-order-reverse': SHARED + [('save',), ('guess', 'tatadada', 'w84w84286286'), MASK_TA, ('add', 5),
                                   ('save',)],
    # A removal leaves the edited record in the entry.
    'remove': SHARED + [('remove', 3), MASK_TA, ('save',)],
    # Length 1: the prefix markers after the non-empty record.
    'markers': [('add', 4), ('add', 0), ('add', 1), ('add', 2), MASK_TA, ('save',)],
    'markers-hit': [('add', 4), ('add', 0), ('add', 1), ('add', 2), ('save',),
                    ('guess', 'da', '286'), MASK_TA, ('add', 5), ('save',)],
    # The tails come back from the file.
    'reopen': SHARED + [DD, MASK_TA, ('save',), ('reopen',), ('add', 5), ('save',)],
    'reopen-mask': SHARED + [DD, MASK_TA, ('save',), ('reopen',), MASK_DA2, ('add', 5), ('save',)],
    # What a fresh instance shows over the tailed records.
    'lookup': SHARED + [DD, MASK_DA2, ('lookup', 0), ('lookup', 3)],
    # A mask that removes nothing still rewrites every record.
    'mask-none': SHARED + [DD, ('mask', 0xffffffff, 0x07000009), ('save',)],
    'mask-all': SHARED + [DD, ('mask',) + USER_ALL, ('add', 5), ('save',)],
}
for _mode in ('pinyin', 'zhuyin'):
    for _name, _steps in TAIL_SEQUENCES.items():
        if _mode == 'zhuyin' and _name.startswith('remove'):
            continue  # the zhuyin facade has no `remove_user_candidate`
        case('issue785-pinyin-tails-%s-%s' % (_name, _mode), mode=_mode, control=_name in TAIL_CONTROLS, issues=('785',))(
            lambda k, steps=_steps: index_key_sequence(k, steps, TAIL_PHRASES, TAIL_READINGS))


# #814 meets #785: unsaved imports leave their index entries behind a library
# 7 unload and reload; the item that takes the first token again sits among
# them in the `da da` record (two items, a tail in the mask walk), and the
# mask removes an entry no item reading backs. The zhuyin facade only: the
# pinyin facade never unloads library 7. BDB replaces each mask entry's chunk
# (chewing_large_table2_bdb.cpp:498-500 at 074a2219), so it carries no #785
# tail. It is not a parent control: at 7fa8ce3^, unload/reload retains the
# unsaved items and the new phrase takes token 6, while the pin recreates an
# empty user library and reuses token 1 (zhuyin.cpp:247-260, :379-388 at
# 074a2219). The case also covers that #814 defect on BDB.
case('issue785-pinyin-tails-kept-entry-zhuyin', mode='zhuyin', issues=('785', '814'))(
    lambda k: index_key_sequence(
        k, SHARED + [DD, ('unload',), ('load',), ('add', 5), MASK_DA2, ('save',)],
        TAIL_PHRASES, TAIL_READINGS))


@case('abort-guess-predicted-short-bigram-value', abort=False)
def _(k):
    """Predict after 我 with a three-byte value planted under each of its prefix tokens."""
    # `_compute_predicted_bigram_candidates` merges the three-byte gram and
    # `get_total_freq` asserts (`memory_chunk.h:390`, `pinyin.cpp:2330`).
    ctx = predicted_bigram_context(k, '我', b'\x07\x00\x00')
    inst = k.fn('alloc_instance', P, P)(ctx)
    return {'ret': k.fn('guess_predicted_candidates', B, P, S)(inst, '我'.encode())}


@case('abort-guess-predicted-empty-bigram-gram', abort=False)
def _(k):
    """Predict after 我 with a total-seven gram containing no items under its prefix tokens."""
    # An item-less gram with a residual total merges to `total_freq != 0`
    # and `SingleGram::get_length` asserts (`ngram.cpp:70`,
    # `pinyin.cpp:2332`).
    ctx = predicted_bigram_context(k, '我', b'\x07\x00\x00\x00')
    inst = k.fn('alloc_instance', P, P)(ctx)
    return {'ret': k.fn('guess_predicted_candidates', B, P, S)(inst, '我'.encode())}


# `_compute_predicted_bigram_candidates` loads only the resolved prefixes'
# own grams (`pinyin.cpp:2322-2330`), so a faulty row under a token the
# prediction never resolves to is never read: the call completes. The
# `我` prefixes do not include `你`'s token, so these are controls for the
# two faults above — the prefix-scoped guard must not over-refuse.
@case('guess-predicted-unrelated-short-bigram-value', control=True)
def _(k):
    """Predict after 我 while the three-byte bigram values are planted under 你's tokens."""
    ctx = predicted_bigram_context(k, '你', b'\x07\x00\x00')
    inst = k.fn('alloc_instance', P, P)(ctx)
    return {'ret': k.fn('guess_predicted_candidates', B, P, S)(inst, '我'.encode())}


@case('guess-predicted-unrelated-empty-bigram-gram', control=True)
def _(k):
    """Predict after 我 while itemless total-seven grams are planted under 你's tokens."""
    ctx = predicted_bigram_context(k, '你', b'\x07\x00\x00\x00')
    inst = k.fn('alloc_instance', P, P)(ctx)
    return {'ret': k.fn('guess_predicted_candidates', B, P, S)(inst, '我'.encode())}


# batch2 group 12e: a tone digit on an initial-only key (PR 12e, #525, row 4)
TONE_INCOMPLETE = (1 << 5) | (1 << 3)


def toned_initial(k, text=b'n4'):
    """Enable tone and incomplete-pinyin options and return a new instance plus consumed bytes.

    Parse text as full pinyin, leaving the context's options set and the
    instance allocated for the caller."""
    k.fn('set_options', B, P, U)(k.ctx, TONE_INCOMPLETE)
    inst = k.alloc()
    return inst, k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)


@case('abort-is-incomplete-toned-initial', abort=False, issues=('525', 'row:4'))
def _(k):
    """Parse n4 as one key and query its incomplete status with tone/incomplete options set.

    Ignore the parse flag and record only the incomplete-query result."""
    k.fn('set_options', B, P, U)(k.ctx, TONE_INCOMPLETE)
    inst = k.alloc()
    key = C.c_uint16(0)
    parsed = k.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_uint16))(inst, b'n4', C.byref(key))
    return {'ret': k.fn('get_pinyin_is_incomplete', B, P, C.POINTER(C.c_uint16))(inst, C.byref(key))}


@case('abort-guess-candidates-toned-initial-before-offset', abort=False, issues=('525', 'row:4', '662'))
def _(k):
    """Guess candidates at byte two of n4ni with a toned initial before the lookup offset."""
    # The toned key sits before the lookup offset, outside the window the
    # search walks: the pin dies all the same (review of #662 expected it not
    # to), so the guard looks at the whole matrix.
    inst, _ = toned_initial(k, b'n4ni')
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 2, 0)}


@case('abort-guess-candidates-toned-initial', abort=False, issues=('525', 'row:4'))
def _(k):
    """Guess candidates at byte zero after parsing n4 with tone/incomplete options enabled."""
    inst, _ = toned_initial(k)
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}


@case('abort-guess-sentence-toned-initial', abort=False, issues=('525', 'row:4'))
def _(k):
    """Guess a sentence from n4 with tone/incomplete options enabled and record its flag."""
    inst, _ = toned_initial(k)
    return {'ret': k.fn('guess_sentence', B, P)(inst)}


@case('abort-guess-sentence-toned-initial-after-key', abort=False, issues=('525', 'row:4'))
def _(k):
    """Guess a sentence from nihaon4 with a toned initial following the complete keys."""
    inst, _ = toned_initial(k, b'nihaon4')
    return {'ret': k.fn('guess_sentence', B, P)(inst)}


# The keys the guard leaves alone: a toned complete key, an untoned initial.
@case('toned-initial-neighbours', control=True, issues=('525', 'row:4'))
def _(k):
    """Collect parsing, guessing and incomplete-key outputs for neighboring inputs.

    Enable tone/incomplete options. Use fresh multi-key instances per input,
    then a separate instance for single-key queries, returning consumed bytes,
    flags and raw 16-bit keys."""
    out = {}
    k.fn('set_options', B, P, U)(k.ctx, TONE_INCOMPLETE)
    for text in (b'ni4', b'n', b'nin', b'ni4hao'):
        inst = k.alloc()
        n = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
        out[text.decode()] = [n, k.fn('guess_sentence', B, P)(inst),
                              k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)]
    inst = k.alloc()
    for text in (b'n', b'ni4', b'ni'):
        key = C.c_uint16(0)
        parsed = k.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_uint16))(inst, text, C.byref(key))
        out['incomplete ' + text.decode()] = [parsed, key.value,
                                              k.fn('get_pinyin_is_incomplete', B, P, C.POINTER(C.c_uint16))(inst, C.byref(key))]
    return out


# batch2 group 12f: the zhuyin twins (PR 12f, #525)
for _name, _call in (
        ('chewing-scheme-0', ctx_call('set_chewing_scheme', B, (I, 0))),
        ('chewing-scheme-7', ctx_call('set_chewing_scheme', B, (I, 7))),
        ('chewing-scheme-10', ctx_call('set_chewing_scheme', B, (I, 10))),
        ('chewing-scheme-minus-1', ctx_call('set_chewing_scheme', B, (I, -1))),
        ('full-pinyin-scheme-0', ctx_call('set_full_pinyin_scheme', B, (I, 0))),
        ('full-pinyin-scheme-4', ctx_call('set_full_pinyin_scheme', B, (I, 4))),
        ('load-phrase-library-0', ctx_call('load_phrase_library', B, (C.c_ubyte, 0))),
        ('load-phrase-library-8', ctx_call('load_phrase_library', B, (C.c_ubyte, 8))),
        ('unload-phrase-library-16', ctx_call('unload_phrase_library', B, (C.c_ubyte, 16))),
        ('unload-phrase-library-255', ctx_call('unload_phrase_library', B, (C.c_ubyte, 255)))):
    case('abort-zhuyin-' + _name, mode='zhuyin', abort=False, issues=('525',))(_call)


def zhuyin_chewing(k, text=b'su3cl3'):
    """Return a new instance after parsing chewing bytes, ignoring the consumed-byte result.

    Leave the native instance allocated for the caller."""
    inst = k.alloc()
    k.fn('parse_more_chewings', Z, P, S)(inst, text)
    return inst


def offset_call(name, fresh, offset):
    """Capture an offset export, byte position and whether its case starts unparsed."""
    def run(k):
        """Call the captured offset export on a fresh or su3cl3-parsed instance.

        Seed a size_t output with UNTOUCHED but return only the native flag."""
        inst = k.alloc() if fresh else zhuyin_chewing(k)
        out = Z(UNTOUCHED)
        return {'ret': k.fn(name, B, P, Z, C.POINTER(Z))(inst, offset, C.byref(out))}
    return run


def guess_call(name, offset):
    """Build a candidate-guess case at the captured byte offset after chewing input."""
    def run(k):
        """Parse su3cl3 on a new instance and return the captured candidate export's flag."""
        return {'ret': k.fn(name, B, P, Z)(zhuyin_chewing(k), offset)}
    return run


def key_call(name, fresh, offset):
    """Build a key-pointer case for an unparsed or chewing-parsed instance and byte offset."""
    def run(k):
        """Call the captured key accessor with a seeded pointer and retain only its flag.

        Allocate an unparsed instance when fresh is true, otherwise parse su3cl3;
        the output pointer is neither dereferenced nor freed."""
        inst = k.alloc() if fresh else zhuyin_chewing(k)
        out = P(UNTOUCHED)
        return {'ret': k.fn(name, B, P, Z, C.POINTER(P))(inst, offset, C.byref(out))}
    return run


def char_offset_call(offset):
    """Build a 你好 character-offset query for the captured byte position."""
    def run(k):
        """Parse su3cl3, query 你好's character offset and return only the native flag."""
        out = Z(UNTOUCHED)
        return {'ret': k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))(
            zhuyin_chewing(k), '你好'.encode(), offset, C.byref(out))}
    return run


case('abort-zhuyin-get-left-zhuyin-offset-past-matrix', mode='zhuyin', abort=False, issues=('525',))(
    offset_call('get_left_zhuyin_offset', False, 99))
case('abort-zhuyin-get-right-zhuyin-offset-past-matrix', mode='zhuyin', abort=False, issues=('525',))(
    offset_call('get_right_zhuyin_offset', False, 99))
case('abort-zhuyin-get-right-zhuyin-offset-empty-matrix', mode='zhuyin', abort=False, issues=('525',))(
    offset_call('get_right_zhuyin_offset', True, 0))
case('abort-zhuyin-guess-candidates-after-cursor-past-matrix', mode='zhuyin', abort=False, issues=('525',))(
    guess_call('guess_candidates_after_cursor', 99))
case('abort-zhuyin-guess-candidates-before-cursor-past-matrix', mode='zhuyin', abort=False, issues=('525',))(
    guess_call('guess_candidates_before_cursor', 99))
case('abort-zhuyin-guess-candidates-after-cursor-reserved-slot-plus-one', mode='zhuyin', abort=False, issues=('525',))(
    guess_call('guess_candidates_after_cursor', 7))
case('abort-zhuyin-guess-candidates-before-cursor-reserved-slot-plus-one', mode='zhuyin', abort=False, issues=('525',))(
    guess_call('guess_candidates_before_cursor', 7))
case('abort-zhuyin-get-character-offset-past-matrix', mode='zhuyin', abort=False, issues=('525',))(char_offset_call(99))
case('abort-zhuyin-get-zhuyin-key-empty-matrix', mode='zhuyin', abort=False, issues=('525',))(
    key_call('get_zhuyin_key', True, 0))
case('abort-zhuyin-get-zhuyin-key-rest-empty-matrix', mode='zhuyin', abort=False, issues=('525',))(
    key_call('get_zhuyin_key_rest', True, 0))


# The zhuyin zero total aborts the pin only while a searched row is ranked
# (`zhuyin.cpp:1261`): the reserved slot after the cursor and the start before
# it hold none, and the pin answers true with no warning.
for _name, _offset in (('after_cursor', 6), ('before_cursor', 0)):
    def _no_rank_z(name, offset):
        """Capture the before/after-cursor suffix and byte offset for a zero-total case."""
        def run(k):
            """Apply the fixture-derived total-wrap delta and observe a zhuyin candidate window.

            Parse su3cl3 on a new instance, ignore the add flag and return the guess
            flag and row count. The delta assumes the context total is FACADE_TOTAL;
            its update follows wrap_total_to_zero's pin-derived arithmetic."""
            inst = zhuyin_chewing(k)
            k.fn('token_add_unigram_frequency', B, P, U, U)(inst, (1 << 24) | 1, (2 ** 32 - FACADE_TOTAL) % 2 ** 32)
            count = U()
            ret = k.fn('guess_candidates_' + name, B, P, Z)(inst, offset)
            k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            return {'ret': ret, 'rows': count.value}
        return run
    case('zhuyin-total-zero-ranks-nothing-' + _name.replace('_', '-'), mode='zhuyin', control=True, issues=('525',))(
        _no_rank_z(_name, _offset))


@case('abort-zhuyin-parse-full-pinyin-apostrophe', mode='zhuyin', abort=False, issues=('525',))
def _(k):
    """Pass n'i to zhuyin's full-pinyin parser and test whether its 16-bit key stays untouched."""
    key = C.c_uint16(UNTOUCHED & 0xFFFF)
    ret = k.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_uint16))(k.inst, b"n'i", C.byref(key))
    return {'ret': ret, 'untouched key': key.value == UNTOUCHED & 0xFFFF}


@case('abort-zhuyin-guess-candidates-total-zero', mode='zhuyin', abort=False, issues=('525',))
def _(k):
    """Guess zhuyin candidates at zero after applying the fixture-derived total-wrap delta.

    Parse su3cl3 first; record only the guess flag, without checking the add result."""
    inst = zhuyin_chewing(k)
    k.fn('token_add_unigram_frequency', B, P, U, U)(inst, (1 << 24) | 1, (2 ** 32 - FACADE_TOTAL) % 2 ** 32)
    return {'ret': k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)}


# The zhuyin sites' graceful neighbours answer without a warning.
@case('zhuyin-cursor-neighbours', mode='zhuyin', control=True, issues=('525',))
def _(k):
    """Collect neighboring zhuyin offset, key and candidate calls on parsed and empty instances.

    Preserve numeric-output sentinels, record key/rest accessor flags, and
    compare candidate guesses at start and end of the six-byte chewing input."""
    out = {}
    inst = zhuyin_chewing(k)
    res = Z(UNTOUCHED)
    out['offset 99'] = [k.fn('get_zhuyin_offset', B, P, Z, C.POINTER(Z))(inst, 99, C.byref(res)),
                        'untouched' if res.value == UNTOUCHED else res.value]
    for label, name, offset in (('left 0', 'get_left_zhuyin_offset', 0), ('left 3', 'get_left_zhuyin_offset', 3),
                                ('right 0', 'get_right_zhuyin_offset', 0), ('right 3', 'get_right_zhuyin_offset', 3)):
        res = Z(UNTOUCHED)
        out[label] = [k.fn(name, B, P, Z, C.POINTER(Z))(inst, offset, C.byref(res)),
                      'untouched' if res.value == UNTOUCHED else res.value]
    for label, name, offset in (('key 0', 'get_zhuyin_key', 0), ('key 99', 'get_zhuyin_key', 99),
                                ('rest 3', 'get_zhuyin_key_rest', 3)):
        ptr = P(UNTOUCHED)
        out[label] = k.fn(name, B, P, Z, C.POINTER(P))(inst, offset, C.byref(ptr))
    out['empty left'] = k.fn('get_left_zhuyin_offset', B, P, Z, C.POINTER(Z))(k.alloc(), 0, C.byref(Z()))
    out['empty guess'] = k.fn('guess_candidates_after_cursor', B, P, Z)(k.alloc(), 0)
    out['guess 0'] = k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    out['guess end before'] = k.fn('guess_candidates_before_cursor', B, P, Z)(inst, 6)
    out['guess end after'] = k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 6)
    return out


@case('zhuyin-scheme-and-library-neighbours', mode='zhuyin', control=True, issues=('525',))
def _(k):
    """Record zhuyin scheme changes and library boundary operations on one context.

    Apply the listed setters and load/unload calls in order and return their flags."""
    out = {}
    for label, name, arg in (('chewing 1', 'set_chewing_scheme', 1), ('chewing 6', 'set_chewing_scheme', 6),
                             ('chewing 8', 'set_chewing_scheme', 8), ('chewing 9', 'set_chewing_scheme', 9),
                             ('full 1', 'set_full_pinyin_scheme', 1), ('full 3', 'set_full_pinyin_scheme', 3)):
        out[label] = k.fn(name, B, P, I)(k.ctx, arg)
    for label, name, arg in (('load 1', 'load_phrase_library', 1), ('load 7', 'load_phrase_library', 7),
                             ('load 16', 'load_phrase_library', 16), ('load 255', 'load_phrase_library', 255),
                             ('unload 1', 'unload_phrase_library', 1)):
        out[label] = k.fn(name, B, P, C.c_ubyte)(k.ctx, arg)
    return out


# The pinyin sites the offset probes of this group found still silent.
@case('abort-get-right-pinyin-offset-empty-matrix', abort=False, issues=('525',))
def _(k):
    """Query the right pinyin offset at zero on an unparsed instance, recording only the flag."""
    out = Z(UNTOUCHED)
    return {'ret': k.fn('get_right_pinyin_offset', B, P, Z, C.POINTER(Z))(k.alloc(), 0, C.byref(out))}


@case('abort-guess-candidates-past-matrix', abort=False, issues=('525',))
def _(k):
    """Guess nihao candidates at byte 99 with sort zero and record the native flag."""
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(full_inst(k, b'nihao'), 99, 0)}


# Row 65 / #696. The pin's LONGER row is a property of the whole parse,
# not the lookup offset: `_prepend_longer_candidates` (`pinyin.cpp:1870-
# 1933`) searches the whole matrix with `prefix_len = m_parsed_key_len`
# (`:1876`, `:1883`) and is called for every offset whose sort word leaves
# `SORT_WITHOUT_LONGER_CANDIDATE` clear (`:2292-2293`). Only the main span
# search starts at `offset` (`:2229`), so an offset no span starts on — and
# the reserved slot past the parse — answers the LONGER row alone.
def candidate_rows(k, inst):
    """Return the candidate list in native order as [type, text] rows.

    Use None when fetching a candidate fails and replace invalid UTF-8 while
    decoding its borrowed string. Count, type and string getter flags are not
    checked; a successful fetch is expected to provide a non-NULL string."""
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    rows = []
    for i in range(count.value):
        cand, kind, text = P(), I(), S()
        if k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand)):
            k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
            k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
            rows.append([kind.value, text.value.decode('utf-8', 'replace')])
        else:
            rows.append(None)
    return rows


def guess_rows(k, inst, offset, sort):
    """Guess pinyin candidates with the supplied byte offset and sort word.

    Return the native flag and complete current candidate list, even on failure."""
    ret = k.fn('guess_candidates', B, P, Z, U)(inst, offset, sort)
    return {'ret': ret, 'rows': candidate_rows(k, inst)}


def zhuyin_guess_rows(k, inst, offset):
    """Guess zhuyin candidates after the supplied byte offset and return the flag and rows.

    Read the complete current candidate list even when the guess returns false."""
    ret = k.fn('guess_candidates_after_cursor', B, P, Z)(inst, offset)
    return {'ret': ret, 'rows': candidate_rows(k, inst)}


# The characterisation set: single and multi-syllable, a bare partial tail,
# apostrophes, and inputs whose longer phrases exist (nihao -> 你好吗,
# women -> 我们…) and do not (woshi, xian). Every matrix offset of each input
# — 0 through the reserved slot — three sort words — 0 (LONGER kept,
# unsorted) plus the two sorted words with and without the LONGER bit — and
# with and without a prior sentence lookup, which is what stores the n-best
# rows. Full lists, not returns.
#
# The pin's matrix holds `parsed + 1` columns for the pinyin facade; an
# offset past that reads through `_check_offset`'s `get_column_size` assert
# (`pinyin.cpp:2163-2182`, `:2226`) and aborts — the class (c) sites
# `abort-guess-candidates-past-matrix` and its zhuyin twin hold. The walk
# therefore stops at `parsed` (the reserved slot) and `parsed + 1`.
GUESS_CANDIDATE_INPUTS = (b'ni', b'n', b'nihao', b'nih', b"ni'hao",
                          b'hao', b'women', b'zhongguo', b'woshi', b'xian')
GUESS_CANDIDATE_SORTS = (0, 0x1c, 0x1e)


@case('guess-candidates-lookup-offset', issues=('696', 'row:65'))
def _(k):
    """Collect ordered pinyin candidate lists across inputs, sort words and byte offsets.

    For each fresh parse, optionally guess a sentence first, then query offsets
    zero through parsed length plus one without reparsing between guesses."""
    out = {}
    for text in GUESS_CANDIDATE_INPUTS:
        for previous in (0, 1):
            inst = k.alloc()
            parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
            if previous:
                k.fn('guess_sentence', B, P)(inst)
            for sort in GUESS_CANDIDATE_SORTS:
                for offset in range(parsed + 2):
                    label = '%s prev=%d sort=%#x off=%d' % (
                        text.decode(), previous, sort, offset)
                    out[label] = guess_rows(k, inst, offset, sort)
    return out


# The zhuyin facade has no LONGER prepend (`zhuyin.cpp:1460-1539` prepends
# only the sentence rows at `:1533`), so the same full-list walk must already
# match at every offset; it is the control that bounds the rule to the pinyin
# facade. Its reserved slot is the LAST valid column (`parsed`); one past it
# aborts (row 66,
# `abort-zhuyin-guess-candidates-after-cursor-reserved-slot-plus-one`).
GUESS_CANDIDATE_ZHUYIN_INPUTS = (b'su3', b'su', b'su3cl3', b'su3cl',
                                 b"su3'cl3", b'cl3', b'ji3', b'ji3cl3',
                                 b'zhong1', b'xian1')


@case('guess-candidates-lookup-offset-zhuyin', mode='zhuyin', control=True, issues=('696', 'row:65'))
def _(k):
    """Collect zhuyin candidate lists through each parse's reserved final column.

    Compare fresh parses with and without a preceding sentence guess; offsets
    run from zero through the returned consumed length, inclusive."""
    out = {}
    for text in GUESS_CANDIDATE_ZHUYIN_INPUTS:
        for previous in (0, 1):
            inst = k.alloc()
            parsed = k.fn('parse_more_chewings', Z, P, S)(inst, text)
            if previous:
                k.fn('guess_sentence', B, P)(inst)
            for offset in range(parsed + 1):
                label = '%s prev=%d off=%d' % (text.decode(), previous, offset)
                out[label] = zhuyin_guess_rows(k, inst, offset)
    return out


# One past the reserved slot is an ordinary lookup that finds nothing —
# except the LONGER row, which the pin still lists there (row 65). The full
# list is compared, not the return alone.
@case('guess-candidates-past-the-reserved-slot', issues=('696', 'row:65'))
def _(k):
    """Record nihao candidate lists at byte offsets five and six with sort word zero."""
    out = {}
    inst = full_inst(k, b'nihao')
    for offset in (5, 6):
        out['offset %d' % offset] = guess_rows(k, inst, offset, 0)
    return out


# batch2 group 13: directory names that are not UTF-8 (PR 13, #587)
def non_utf8_dirs(k, bad_system, bad_user):
    """Exercise pinyin initialization and saving through directory names containing 0xFF.

    Optionally create a system symlink and a new user directory under scratch.
    If init succeeds, parse, guess, train and save, then list non-hidden user
    files using filesystem decoding. Leave the native context and instance live."""
    tag = str(os.getpid()).encode()
    scratch = os.fsencode(k.scratch)
    system = os.path.join(scratch, b'system-\xff' + tag) if bad_system else os.fsencode(k.data)
    if bad_system:
        os.symlink(os.fsencode(k.data), system)
    user = os.path.join(scratch, b'user-\xff' + tag) if bad_user else os.fsencode(k.user)
    if bad_user:
        os.mkdir(user)
    ctx = k.init(system=system, user=user)
    out = {'init': bool(ctx)}
    if ctx:
        inst = k.fn('alloc_instance', P, P)(ctx)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
        out['guess'] = k.fn('guess_sentence', B, P)(inst)
        out['train'] = k.fn('train', B, P, C.c_ubyte)(inst, 0)
        out['save'] = k.fn('save', B, P)(ctx)
        out['user files'] = sorted(os.fsdecode(n) for n in os.listdir(user) if not n.startswith(b'.'))
    return out


for _label, _system, _user in (('system', True, False), ('user', False, True), ('both', True, True)):
    case('non-utf8-directory-' + _label, issues=('587',))(
        lambda k, system=_system, user=_user: non_utf8_dirs(k, system, user))


@case('non-utf8-directory-zhuyin-both', mode='zhuyin', issues=('587',))
def _(k):
    """Exercise zhuyin training and saving with 0xFF in both directory names.

    Create a system symlink and user directory under scratch, returning init
    and successful-init operation flags plus non-hidden filenames. Leave the
    context and instance live."""
    tag = str(os.getpid()).encode()
    scratch = os.fsencode(k.scratch)
    system = os.path.join(scratch, b'system-\xff' + tag)
    os.symlink(os.fsencode(k.data), system)
    user = os.path.join(scratch, b'user-\xff' + tag)
    os.mkdir(user)
    ctx = k.init(system=system, user=user)
    out = {'init': bool(ctx)}
    if ctx:
        inst = k.fn('alloc_instance', P, P)(ctx)
        k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
        out['guess'] = k.fn('guess_sentence', B, P)(inst)
        out['train'] = k.fn('train', B, P)(inst)
        out['save'] = k.fn('save', B, P)(ctx)
        out['user files'] = sorted(os.fsdecode(n) for n in os.listdir(user) if not n.startswith(b'.'))
    return out


# batch2 group 14: the zhuyin twins of the batch-1 contracts (PR 14, #542)
# A populated USER_FILE token follows the facade's unload contract. Pinyin
# refuses index 7; zhuyin unloads it and its phrase-item reads fail until reload
# (074a2219 pinyin.cpp:466-474, zhuyin.cpp:378-388, phrase_index.h:646-657).
def user_library_token_unload(k):
    """Snapshot a populated library-7 token before unloading and after reloading it.

    Import 你好 with the facade's reading, assert one user token, save, and
    record phrase and pronunciation getters around unload/load. Lookup arrays
    remain allocated; the kit retains its context and instance."""
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    reading = b"ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode()
    added = k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), reading, 1)
    k.fn('end_add_phrases', None, P)(it)
    tokens = [token for token in tokens_of(k, '你好') if token >> 24 == 7]
    assert added and len(tokens) == 1, 'a populated index-7 token is required'
    token = tokens[0]
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]

    def reads():
        """Read the seeded token's phrase and pronunciation outputs, retaining sentinels.

        Copy 16-bit pronunciation keys as hex and decode/free returned phrase text.
        The pronunciation GArray is not freed."""
        keys = new(0, 0, 2)
        ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, 0, keys)
        view = C.cast(keys, C.POINTER(Arr)).contents
        content = C.string_at(view.data, view.len * 2).hex() if view.len else ''
        length, phrase = U(UNTOUCHED), P(UNTOUCHED)
        phrase_ret = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
            k.inst, token, C.byref(length), C.byref(phrase))
        count = U(UNTOUCHED)
        count_ret = k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(
            k.inst, token, C.byref(count))
        return dict(nth=[ret, view.len, content],
                    phrase=[phrase_ret, length.value,
                            'untouched' if phrase.value == UNTOUCHED else k.text(phrase.value)],
                    n=[count_ret, count.value])

    out = {'before': reads(), 'save': k.fn('save', B, P)(k.ctx)}
    out['unload'] = k.fn('unload_phrase_library', B, P, C.c_ubyte)(k.ctx, 7)
    out['unloaded'] = reads()
    out['load'] = k.fn('load_phrase_library', B, P, C.c_ubyte)(k.ctx, 7)
    out['reloaded'] = reads()
    return out


case('user-library-token-unload-pinyin', control=True, issues=('542',))(user_library_token_unload)
case('user-library-token-unload-zhuyin', mode='zhuyin', issues=('542',))(user_library_token_unload)


def zhuyin_sentence_out(k, inst):
    """Read a zhuyin sentence, distinguishing an untouched output from NULL.

    Return [native flag, text or sentinel]; decode and free any returned string."""
    out = P(UNTOUCHED)
    ret = k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(out))
    if out.value == UNTOUCHED:
        return [ret, 'untouched']
    return [ret, k.text(out.value)]


@case('zhuyin-sentence-before-guess', mode='zhuyin', issues=('542',))
def _(k):
    """Compare zhuyin sentence outputs before parse, after guess, after reset and on a keyless parse."""
    out = {}
    inst = k.alloc()
    out['fresh'] = zhuyin_sentence_out(k, inst)
    k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
    out['parsed'] = zhuyin_sentence_out(k, inst)
    k.fn('guess_sentence', B, P)(inst)
    out['guessed'] = zhuyin_sentence_out(k, inst)
    k.fn('reset', B, P)(inst)
    out['reset'] = zhuyin_sentence_out(k, inst)
    inst = k.alloc()
    k.fn('parse_more_chewings', Z, P, S)(inst, b"'")
    out['keyless'] = [k.fn('guess_sentence', B, P)(inst), zhuyin_sentence_out(k, inst)]
    return out


@case('zhuyin-character-offset-out', mode='zhuyin', issues=('542',))
def _(k):
    """Record zhuyin character-offset flags and output sentinels for the listed parse/phrase pairs."""
    fn = k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))
    out = {}
    for label, text, phrase, offset in (
            ('recursion-fails-1', b'su3cl3', '啊', 3), ('recursion-fails-2', b'su3cl3', '你', 6),
            ('hit', b'su3cl3', '你好', 5), ('reserved-slot', b'su3cl3', '你好', 6), ('reserved-slot-1', b'su3cl3', '你', 6), ('no-token', b'su3cl3', 'x', 2), ('empty-phrase', b'su3cl3', '', 2),
            ('no-parse', b'', '你好', 0), ('keyless', b"'", '你好', 0)):
        inst = k.alloc()
        k.fn('parse_more_chewings', Z, P, S)(inst, text)
        length = Z(UNTOUCHED)
        ret = fn(inst, phrase.encode(), offset, C.byref(length))
        out[label] = [ret, 'untouched' if length.value == UNTOUCHED else length.value]
    return out


@case('zhuyin-token-get-phrase-out', mode='zhuyin', issues=('542',))
def _(k):
    """Read zhuyin phrase outputs for three invalid tokens, preserving untouched sentinels.

    Decode and free any string actually returned."""
    out = {}
    for token in (0xFFFFFFFF, 0x0DEADBEE, 0x01000000):
        length, text = U(UNTOUCHED), P(UNTOUCHED)
        ret = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
            k.inst, token, C.byref(length), C.byref(text))
        out[hex(token)] = [ret, 'untouched' if length.value == UNTOUCHED else length.value,
                           'untouched' if text.value == UNTOUCHED else k.text(text.value)]
    return out


@case('zhuyin-nth-pronunciation-range', mode='zhuyin', issues=('542',))
def _(k):
    """Observe zhuyin pronunciation bounds and an unknown-token call on a seeded array.

    Compare returned key bytes only for in-range indices; retain other bytes
    under diagnostic-only keys. The allocated GArrays remain live."""
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    arr = new(0, 0, 4)
    k.fn('lookup_tokens', B, P, S, P)(k.inst, '你好'.encode(), arr)
    view = C.cast(arr, C.POINTER(Arr)).contents
    token = C.cast(view.data, C.POINTER(U))[:view.len][0]
    count = U(UNTOUCHED)
    k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(k.inst, token, C.byref(count))
    out = {'n': count.value}
    for nth in (0, count.value - 1, count.value, count.value + 1, 0xFFFFFFFF):
        keys = new(0, 0, 2)
        ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, nth, keys)
        kview = C.cast(keys, C.POINTER(Arr)).contents
        content = C.string_at(kview.data, kview.len * 2).hex() if kview.len else ''
        in_range = nth < count.value
        out['nth %d' % nth] = [ret, kview.len] + ([content] if in_range else [])
        out['~content nth %d' % nth] = content
    append = k.glib.g_array_append_vals
    append.restype, append.argtypes = P, [P, P, U]
    keys = new(0, 0, 2)
    seed = C.c_ushort(0x1234)
    append(keys, C.byref(seed), 1)
    ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, 0xFFFFFFFF, 0, keys)
    out['unknown token'] = [ret, C.cast(keys, C.POINTER(Arr)).contents.len]
    return out


@case('zhuyin-static-key-slots', mode='zhuyin', issues=('542',))
def _(k):
    """Compare key/rest pointers across two zhuyin instances and read them after freeing the first.

    Copy two key bytes and four rest bytes without taking ownership of either pointer."""
    first, second = k.alloc(), k.alloc()
    k.fn('parse_more_chewings', Z, P, S)(first, b'su3')
    k.fn('parse_more_chewings', Z, P, S)(second, b'cl3')
    key = k.fn('get_zhuyin_key', B, P, Z, C.POINTER(P))
    rest = k.fn('get_zhuyin_key_rest', B, P, Z, C.POINTER(P))
    a, b, ra, rb = P(), P(), P(), P()
    out = {'ret': [key(first, 0, C.byref(a)), key(second, 0, C.byref(b)),
                   rest(first, 0, C.byref(ra)), rest(second, 0, C.byref(rb))]}
    out['key same pointer'] = a.value == b.value
    out['rest same pointer'] = ra.value == rb.value
    out['first key now'] = C.string_at(a.value, 2).hex()
    out['first rest now'] = C.string_at(ra.value, 4).hex()
    k.fn('free_instance', None, P)(first)
    out['key after free'] = C.string_at(a.value, 2).hex()
    out['rest after free'] = C.string_at(ra.value, 4).hex()
    return out


@case('zhuyin-key-rest-length', mode='zhuyin', issues=('542',))
def _(k):
    """Record 16-bit zhuyin rest lengths for forward, reversed and boundary endpoint pairs."""
    fn = k.fn('get_zhuyin_key_rest_length', B, P, C.POINTER(Rest), C.POINTER(C.c_ushort))
    out = {}
    for begin, end in ((0, 2), (5, 2), (2, 2), (0, 65535), (65535, 0), (1, 65535)):
        length = C.c_ushort(0xBEEF)
        rest = Rest(begin, end)
        out['%d..%d' % (begin, end)] = [fn(k.inst, C.byref(rest), C.byref(length)), length.value]
    return out


@case('zhuyin-unload-phrase-library-repeat', mode='zhuyin', issues=('542',))
def _(k):
    """Record zhuyin unload results for libraries zero through fifteen, then library two twice."""
    unload = k.fn('unload_phrase_library', B, P, U)
    return {'unload': [unload(k.ctx, i) for i in range(0, 16)] + [unload(k.ctx, i) for i in (2, 2)]}


def zhuyin_candidates_at_start(k):
    """Return the reported candidate count after guessing at the start of su3cl3.

    Allocate a fresh parsed zhuyin instance and leave it live; getter flags are
    not checked."""
    inst = zhuyin_chewing(k)
    k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    return count.value


@case('zhuyin-unload-phrase-library-effect', mode='zhuyin', issues=('542',))
def _(k):
    """Compare fresh zhuyin candidate counts around unloading and reloading libraries 3, 2 and 4."""
    unload = k.fn('unload_phrase_library', B, P, C.c_ubyte)
    load = k.fn('load_phrase_library', B, P, C.c_ubyte)
    out = {'before': zhuyin_candidates_at_start(k)}
    for index in (3, 2, 4):
        out['unload %d' % index] = [unload(k.ctx, index), zhuyin_candidates_at_start(k)]
        out['load %d' % index] = [load(k.ctx, index), zhuyin_candidates_at_start(k)]
        out['load %d again' % index] = load(k.ctx, index)
    return out


@case('zhuyin-alloc-instance-after-fini', mode='zhuyin', issues=('542',))
def _(k):
    """Attempt zhuyin instance allocation after finalizing its context.

    Record the live allocation flag and keep the post-finalization result under
    a diagnostic-only key; the attempted allocation's pointer is not freed."""
    # As the pinyin case: the pin reads the freed context and survives by
    # chance, the ruled answer is NULL (class (b)); the case holds the exit
    # status and leaves the answer out.
    context = k.init()
    live = k.fn('alloc_instance', P, P)(context)
    k.fn('free_instance', None, P)(live)
    k.fn('fini', None, P)(context)
    after = k.fn('alloc_instance', P, P)(context)
    return {'live instance': bool(live), '~instance after fini': bool(after)}


# batch2 group 15: the pin's stderr and the save report (PR 15, #545)
def save_after_training(k, remove_dir, parse='parse_more_full_pinyins', text=b'nihao', train_args=(C.c_ubyte,)):
    """Train a guessed sentence and return training and save flags.

    Use the specified parser and training signature, passing rank zero when
    train_args is nonempty. If remove_dir is true, delete the user directory
    before saving. Leave the context and allocated instance live."""
    ctx = k.ctx
    inst = k.alloc()
    k.fn(parse, Z, P, S)(inst, text)
    k.fn('guess_sentence', B, P)(inst)
    out = {'train': k.fn('train', B, P, *train_args)(inst, *((0,) if train_args else ()))}
    if remove_dir:
        shutil.rmtree(k.user)
    out['save'] = k.fn('save', B, P)(ctx)
    return out


def blocked_save(k, blocked, parse='parse_more_full_pinyins', text=b'nihao', train_args=(C.c_ubyte,)):
    """Compare two trained saves with a profile path replaced by a nonempty directory.

    Move an existing blocked path to .moved, then classify each remaining
    post-save entry as new, changed or same using directory names or a short
    content hash. Missing post-save entries are omitted. Return both save flags;
    leave the modified filesystem and native objects for worker cleanup."""
    import hashlib
    ctx = k.ctx
    inst = k.alloc()
    k.fn(parse, Z, P, S)(inst, text)
    k.fn('guess_sentence', B, P)(inst)
    train = k.fn('train', B, P, *train_args)
    train(inst, *((0,) if train_args else ()))
    out = {'first': k.fn('save', B, P)(ctx)}

    def snapshot():
        """Snapshot immediate user entries as sorted directory contents or eight MD5 hex digits."""
        files = {}
        for name in sorted(os.listdir(k.user)):
            path = os.path.join(k.user, name)
            files[name] = ('dir:' + ','.join(sorted(os.listdir(path)))) if os.path.isdir(path) \
                else hashlib.md5(open(path, 'rb').read()).hexdigest()[:8]
        return files
    before = snapshot()
    train(inst, *((0,) if train_args else ()))
    path = os.path.join(k.user, blocked)
    if os.path.isdir(path) or os.path.exists(path):
        os.replace(path, path + '.moved')
    os.mkdir(path)
    open(os.path.join(path, 'keep'), 'w').write('x')
    out['second'] = k.fn('save', B, P)(ctx)
    after = snapshot()
    out['files'] = {n: ('new' if n not in before else 'changed' if before[n] != after[n] else 'same')
                    for n in after if not n.endswith('.moved')}
    return out


# The pin writes and renames file by file (`pinyin.cpp:940-1130`): with one
# path blocked every other file is updated, one line is printed and the
# profile mixes two saves.
@case('stderr-save-one-tmp-blocked', stderr=True, issues=('545',))
def _(k):
    """Save with user_pinyin_index.bin.tmp blocked by a nonempty directory."""
    return blocked_save(k, 'user_pinyin_index.bin.tmp')


@case('stderr-save-one-final-blocked', stderr=True, issues=('545',))
def _(k):
    """Save with the final user_phrase_index.bin path blocked by a nonempty directory."""
    return blocked_save(k, 'user_phrase_index.bin')


@case('stderr-save-one-tmp-blocked-zhuyin', mode='zhuyin', stderr=True, issues=('545',))
def _(k):
    """Repeat the blocked temporary pinyin-index save through the zhuyin facade."""
    return blocked_save(k, 'user_pinyin_index.bin.tmp', 'parse_more_chewings', b'su3cl3', ())


# Batch A (#525): `MemoryChunk::save`'s header writes (`memory_chunk.h:543`,
# `:547`). The pin writes the header as two `guint32`s and `assert`s each, so
# a filesystem that refuses a header word kills it mid-save. The save is the
# one failure the pin does not carry past: oxpinyin fails the call and logs
# one warning in the facade's own domain.
def _dirty_for_save(k, parse='parse_more_full_pinyins', text=b'nihao', train_args=(C.c_ubyte,)):
    """Parse, guess and train the configured input, returning the cached context.

    Use rank zero for a nonempty training signature. Ignore operation flags
    and leave the newly allocated instance live."""
    ctx = k.ctx
    inst = k.alloc()
    k.fn(parse, Z, P, S)(inst, text)
    k.fn('guess_sentence', B, P)(inst)
    train = k.fn('train', B, P, *train_args)
    train(inst, *((0,) if train_args else ()))
    return ctx


def save_header_dev_full(k, parse='parse_more_full_pinyins', text=b'nihao', train_args=(C.c_ubyte,)):
    """Attempt a trained save with user.bin.tmp linked to /dev/full.

    Leave the symlink in place and return the save flag if the call returns.
    This targets the length-word write in MemoryChunk::save in
    src/include/memory_chunk.h at pin 074a2219."""
    ctx = _dirty_for_save(k, parse, text, train_args)
    Path(k.user, 'user.bin.tmp').symlink_to('/dev/full')
    return {'ret': k.fn('save', B, P)(ctx)}


def save_header_rlimit(k, parse='parse_more_full_pinyins', text=b'nihao', train_args=(C.c_ubyte,)):
    """Attempt a trained save after limiting this process's file size to four bytes.

    Ignore SIGXFSZ and set both RLIMIT_FSIZE limits without restoring them.
    This targets the checksum-word write in MemoryChunk::save in
    src/include/memory_chunk.h at pin 074a2219; return the save flag if reached."""
    ctx = _dirty_for_save(k, parse, text, train_args)
    signal.signal(signal.SIGXFSZ, signal.SIG_IGN)
    resource.setrlimit(resource.RLIMIT_FSIZE, (4, 4))
    return {'ret': k.fn('save', B, P)(ctx)}


@case('abort-save-chunk-header-length', abort=False, issues=('525',))
def _(k):
    """Attempt a pinyin save whose first chunk-header write targets /dev/full."""
    return save_header_dev_full(k)


@case('abort-save-chunk-header-checksum', abort=False, issues=('525',))
def _(k):
    """Attempt a pinyin save with a four-byte process file-size limit."""
    return save_header_rlimit(k)


@case('abort-zhuyin-save-chunk-header-length', mode='zhuyin', abort=False, issues=('525',))
def _(k):
    """Attempt a zhuyin save whose first chunk-header write targets /dev/full."""
    return save_header_dev_full(k, 'parse_more_chewings', b'su3cl3', ())


@case('abort-zhuyin-save-chunk-header-checksum', mode='zhuyin', abort=False, issues=('525',))
def _(k):
    """Attempt a zhuyin save with a four-byte process file-size limit."""
    return save_header_rlimit(k, 'parse_more_chewings', b'su3cl3', ())


@case('stderr-fresh-user-dir', stderr=True, issues=('545',))
def _(k):
    """Initialize and finalize a fresh pinyin profile, returning the original context's truth value."""
    ctx = k.ctx
    k.fn('fini', None, P)(ctx)
    return {'init': bool(ctx)}


@case('stderr-fresh-user-dir-zhuyin', mode='zhuyin', stderr=True, issues=('545',))
def _(k):
    """Initialize and finalize a fresh zhuyin profile, returning the original context's truth value."""
    ctx = k.ctx
    k.fn('fini', None, P)(ctx)
    return {'init': bool(ctx)}


@case('stderr-init-missing-system-dir', stderr=True, issues=('545',))
def _(k):
    """Record pinyin initialization with /nonexistent-system-dir as the system path."""
    return {'ctx': bool(k.init(system='/nonexistent-system-dir'))}


@case('stderr-init-missing-system-dir-zhuyin', mode='zhuyin', stderr=True, issues=('545',))
def _(k):
    """Record zhuyin initialization with /nonexistent-system-dir as the system path."""
    return {'ctx': bool(k.init(system='/nonexistent-system-dir'))}


@case('stderr-init-system-dir-without-table-conf', stderr=True, issues=('545',))
def _(k):
    """Initialize against an empty scratch system directory with two trailing slashes."""
    empty = tempfile.mkdtemp(prefix='sys-', dir=k.scratch)
    return {'ctx': bool(k.init(system=empty + '//'))}


@case('stderr-save-dir-removed', stderr=True, issues=('545',))
def _(k):
    """Train pinyin, remove the user directory and record the save result."""
    return save_after_training(k, True)


@case('stderr-save-dir-removed-zhuyin', mode='zhuyin', stderr=True, issues=('545',))
def _(k):
    """Train zhuyin, remove the user directory and record the save result."""
    return save_after_training(k, True, 'parse_more_chewings', b'su3cl3', ())


@case('stderr-fini-dir-removed', stderr=True, issues=('545',))
def _(k):
    """Remove a fresh pinyin profile directory before finalizing its cached context."""
    ctx = k.ctx
    shutil.rmtree(k.user)
    k.fn('fini', None, P)(ctx)
    return {}


@case('stderr-fini-dir-removed-zhuyin', mode='zhuyin', stderr=True, issues=('545',))
def _(k):
    """Remove a fresh zhuyin profile directory before finalizing its cached context."""
    ctx = k.ctx
    shutil.rmtree(k.user)
    k.fn('fini', None, P)(ctx)
    return {}


# The paths that stay quiet at the pin: a save that has nothing to save, a
# save that works, a reopen of the profile it wrote.
# Paths in the pin's lines are the caller's bytes, not text.
@case('stderr-non-utf8-user-dir', stderr=True, issues=('545',))
def _(k):
    """Train in a user directory containing 0xFF, remove it, then save and finalize.

    Return training and save flags while leaving diagnostics to the worker."""
    user = os.path.join(os.fsencode(k.scratch), b'user-\xff' + os.path.basename(k.user.encode())[5:])
    os.mkdir(user)
    ctx = k.init(user=user)
    inst = k.fn('alloc_instance', P, P)(ctx)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    out = {'train': k.fn('train', B, P, C.c_ubyte)(inst, 0)}
    shutil.rmtree(user)
    out['save'] = k.fn('save', B, P)(ctx)
    k.fn('fini', None, P)(ctx)
    return out


@case('stderr-quiet-saves', stderr=True, issues=('545',))
def _(k):
    """Record unmodified and trained saves, then reopen and save the persisted profile.

    Finalize both contexts on the normal path; the kit's cached pointers are
    not cleared."""
    ctx = k.ctx
    out = {'unmodified save': k.fn('save', B, P)(ctx)}
    out.update(save_after_training(k, False))
    k.fn('fini', None, P)(ctx)
    ctx2 = k.init()
    out['reopen'] = bool(ctx2)
    out['second save'] = k.fn('save', B, P)(ctx2)
    k.fn('fini', None, P)(ctx2)
    return out


@case('stderr-unmodified-save-dir-removed', stderr=True, issues=('545',))
def _(k):
    """Remove an unmodified pinyin profile directory and record its save result."""
    ctx = k.ctx
    shutil.rmtree(k.user)
    return {'save': k.fn('save', B, P)(ctx)}


# The raw `mmap %s failed!` lines of the pin's library loaders (#545;
# `pinyin.cpp:256`, `:290`, `:956`, `:1265`, `zhuyin.cpp:200`, `:589`,
# `:800`). Each site hands the chunk it failed to map, empty, on: the
# library load reads `NULL + 16` and the save's `diff` does the same on the
# old chunk, so the pin dies of SIGSEGV right after the line (register
# row 68, class (b)). The system directory is a private copy of the oracle's
# (every file a link back to it) with one library broken; the oracle's own
# files are never touched.
def private_system(k):
    """Create a scratch system directory whose immediate entries symlink to k.data.

    Return its path; callers may replace individual links without modifying the
    source files. The directory is not removed here."""
    system = tempfile.mkdtemp(prefix='sys-', dir=k.scratch)
    for entry in os.listdir(k.data):
        os.symlink(os.path.join(k.data, entry), os.path.join(system, entry))
    return system


SPARSE_CHUNK_SIZE = (1 << 32) + 8


def sparse_chunk(path):
    """Create an exclusive sparse chunk of 4 GiB plus eight bytes with a zero header.

    Close the descriptor even on failure. Assert its logical size and at most
    1 MiB allocated, using st_blocks in 512-byte units. Existing paths fail.
    MemoryChunk::mmap in src/include/memory_chunk.h at pin 074a2219 truncates
    this payload length to zero when checking the header and checksum."""
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
    try:
        os.write(fd, b'\0' * 8)  # length word 0, checksum 0
        os.ftruncate(fd, SPARSE_CHUNK_SIZE)
    finally:
        os.close(fd)
    stat = os.stat(path)
    assert stat.st_size == SPARSE_CHUNK_SIZE, stat
    # st_blocks is in 512-byte units; one 4 KiB block for the header.
    assert stat.st_blocks * 512 <= 1 << 20, ('the fixture materialised', stat)


def chunk_stats(path):
    """Return logical and allocated file sizes in bytes, converting st_blocks from 512-byte units."""
    stat = os.stat(path)
    return {'fixture_size': stat.st_size, 'fixture_allocated': stat.st_blocks * 512}


def nonzero_layout_chunk():
    """Encode a chunk with total 123 and an invalid second sub-index separator.

    Wrap the 32-byte payload in a valid length/checksum header. The offsets
    follow SubPhraseIndex::load in src/storage/phrase_index.cpp at pin 074a2219;
    its total is read before the separator check."""
    # A valid header and checksum with total_freq 123 and an in-bounds
    # second-separator fault. The pin reads 123 into the sub-index's local
    # total and does not add it to the facade aggregate
    # (`phrase_index.cpp:237-248`).
    payload = bytearray(32)
    payload[0:4] = (123).to_bytes(4, 'little')
    payload[4:8] = (17).to_bytes(4, 'little')
    payload[8:12] = (20).to_bytes(4, 'little')
    payload[12:16] = (24).to_bytes(4, 'little')
    payload[16] = ord('#')
    payload[19] = ord('!')
    return _chunk(bytes(payload))


def break_library(k, system, name, how):
    """Replace one private system-library link with the selected broken-file fixture.

    Unlink first. how selects missing, empty, short, bad-length, header-only,
    sparse, oob-layout, short-chunk, nonzero-layout, second-separator,
    third-separator, checksum, dir-long or dir-short. Unknown values assert
    only after unlinking. Checksum corruption copies the source bytes; layout
    fixtures use MemoryChunk::mmap in src/include/memory_chunk.h and
    SubPhraseIndex::load in src/storage/phrase_index.cpp at pin 074a2219.
    Directories contain one file; sparse fixtures retain their holes."""
    path = os.path.join(system, name)
    os.unlink(path)
    if how == 'empty':
        Path(path).write_bytes(b'')
    elif how == 'short':
        Path(path).write_bytes(b'\0\0\0\0')
    elif how == 'bad-length':
        # 16-byte file, payload 8, declared length 7: the pin's truncated
        # length check refuses it before the checksum.
        Path(path).write_bytes((7).to_bytes(4, 'little') + (0).to_bytes(4, 'little') + b'\0' * 8)
    elif how == 'header-only':
        Path(path).write_bytes(b'\0' * 8)
    elif how == 'sparse':
        sparse_chunk(path)
    elif how == 'oob-layout':
        # An in-bounds four-word header whose second separator index lies
        # past the payload. The pin reads the separator outside the mapped
        # chunk (phrase_index.cpp:354-356); oxpinyin refuses the layout.
        payload = bytearray(32)
        payload[0:4] = (123).to_bytes(4, 'little')
        payload[4:8] = (17).to_bytes(4, 'little')
        payload[8:12] = (1 << 28).to_bytes(4, 'little')
        payload[12:16] = (24).to_bytes(4, 'little')
        payload[16] = ord('#')
        Path(path).write_bytes(_chunk(bytes(payload)))
    elif how == 'short-chunk':
        # A payload shorter than the four-word header: the pin's
        # SubPhraseIndex::load reads the index words past the mapped chunk.
        payload = bytearray(8)
        payload[0:4] = (123).to_bytes(4, 'little')
        payload[4:8] = (17).to_bytes(4, 'little')
        Path(path).write_bytes(_chunk(bytes(payload)))
    elif how == 'nonzero-layout':
        Path(path).write_bytes(nonzero_layout_chunk())
    elif how in ('second-separator', 'third-separator'):
        # A chunk with a valid header and checksum whose second or third
        # `SubPhraseIndex::load` separator is in-bounds but is not '#':
        # the pin's checks at `phrase_index.cpp:355-356`.
        payload = bytearray(32)
        payload[4:8] = (17).to_bytes(4, 'little')     # index_one
        payload[8:12] = (20).to_bytes(4, 'little')    # index_two
        payload[12:16] = (21).to_bytes(4, 'little')   # index_three
        payload[16] = ord('#')
        if how == 'second-separator':
            payload[19] = ord('!')                    # index_two - 1
        else:
            payload[19] = ord('#')
            payload[12:16] = (24).to_bytes(4, 'little')
            payload[23] = ord('!')                    # index_three - 1
        Path(path).write_bytes(_chunk(bytes(payload)))
    elif how == 'checksum':
        data = bytearray(Path(k.data, name).read_bytes())
        data[-1] ^= 0xff
        Path(path).write_bytes(bytes(data))
    elif how in ('dir-long', 'dir-short'):
        # A non-empty directory in the library's place. The size the pin
        # reads from `lseek(SEEK_END)` is the file system's: ext4 answers
        # the htree end of file (huge) for any directory, btrfs the summed
        # length of the entry names twice over (2 for `x`, 50 for 25
        # characters), so which `MemoryChunk::mmap` site is reached follows
        # the size (`memory_chunk.h:479-483` against `:493`).
        os.makedirs(path)
        Path(path, 'e' * 25 if how == 'dir-long' else 'x').write_bytes(b'x')
    else:
        assert how == 'missing'


def train_once(k, ctx, parse, text, train_args):
    """Allocate an instance, parse and guess, then return one training call's flag.

    Use rank zero when train_args is nonempty. Ignore parse/guess flags and
    leave the instance live on the caller's context."""
    inst = k.fn('alloc_instance', P, P)(ctx)
    k.fn(parse, Z, P, S)(inst, text)
    k.fn('guess_sentence', B, P)(inst)
    return k.fn('train', B, P, *train_args)(inst, *((0,) if train_args else ()))


LIBRARY_BREAKS = ('missing', 'empty', 'short', 'bad-length', 'checksum')
# mode -> (library file, index, parse call, text, train args)
LIBRARY_FACADES = {
    'pinyin': ('merged.bin', 4, 'parse_more_full_pinyins', b'nihao', (C.c_ubyte,)),
    'zhuyin': ('merged.bin', 4, 'parse_more_chewings', b'su3cl3', ()),
}


def library_probe(site, how):
    """Build a case that breaks a private library before the selected native operation.

    site selects init, init-fresh, init-nonconform, load, addon, save or mask-out;
    how is passed to break_library. The returned callable performs all I/O."""
    def probe(k):
        """Prepare the selected profile state and collect flags around the broken library.

        Init variants optionally train/save a conforming profile or change its
        model version to 999. Other sites initialize first, then break the library
        before loading, saving or masking it. Leave contexts live."""
        name, index, parse, text, train_args = LIBRARY_FACADES[k.mode]
        system = private_system(k)
        if site in ('init', 'init-fresh', 'init-nonconform'):
            # The pin judges the user profile before it loads a library, so
            # the failing init writes the profile's line first when the
            # profile is fresh or non-conforming and nothing of it when the
            # profile conforms: the three user dirs below.
            if site != 'init-fresh':
                settled = k.init()
                assert settled, 'init failed'
                train_once(k, settled, parse, text, train_args)
                assert k.fn('save', B, P)(settled), 'save failed'
                k.fn('fini', None, P)(settled)
            if site == 'init-nonconform':
                conf = Path(k.user, 'user.conf')
                conf.write_text(re.sub(r'(model data version:)\d+', r'\g<1>999', conf.read_text()))
            break_library(k, system, name, how)
            return {'ctx': bool(k.init(system=system))}
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        load = k.fn('load_phrase_library', B, P, C.c_ubyte)
        if site == 'load':
            # GBK is the one library `pinyin_unload_phrase_library` lets go
            # (TSI_DICTIONARY, 1, is the one `zhuyin_unload_phrase_library`
            # keeps); index 2 is unloadable on both facades.
            out = {'unload': k.fn('unload_phrase_library', B, P, C.c_ubyte)(ctx, 2)}
            break_library(k, system, 'gbk_char.bin', how)
            out['load'] = load(ctx, 2)
            return out
        if site == 'addon':
            addon = k.fn('load_addon_phrase_library', B, P, C.c_ubyte)
            out = {'unload': k.fn('unload_addon_phrase_library', B, P, C.c_ubyte)(ctx, 4)}
            break_library(k, system, 'art.bin', how)
            out['load'] = addon(ctx, 4)
            return out
        out = {'train': train_once(k, ctx, parse, text, train_args)}
        break_library(k, system, name, how)
        if site == 'save':
            out['save'] = k.fn('save', B, P)(ctx)
        else:
            assert site == 'mask-out'
            out['mask_out'] = k.fn('mask_out', B, P, U, U)(ctx, 0xff000000, 0x04000000)
        return out
    return probe


def library_expected(site, how, mode):
    """Return the registered subject observation for a broken-library crash case.

    Init with a missing library expects success without logs; other init faults
    expect NULL and a facade warning. Other sites use fixed result dictionaries
    and no logs. Unsupported non-init sites or required warning modes raise KeyError."""
    if site.startswith('init'):
        failed = how != 'missing'
        return {'ctx': not failed, 'logs': [[WARNING_DOMAIN[mode], 16]] if failed else []}
    return dict(logs=[], **{
        'load': {'unload': True, 'load': True},
        'addon': {'unload': True, 'load': False},
        'save': {'train': True, 'save': True},
        'mask-out': {'train': True, 'mask_out': True},
    }[site])


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    for _how in LIBRARY_BREAKS:
        for _site in ('init', 'load', 'save', 'mask-out') + (('addon',) if _mode == 'pinyin' else ()):
            case(f'stderr-library-{_site}-{_how}' + _suffix, mode=_mode,
                 crash=library_expected(_site, _how, _mode), issues=('545',))(library_probe(_site, _how))
        # Both lines in the pin's order: the profile's, then the library's.
        for _site, _label in (('init-fresh', 'fresh-user-dir'), ('init-nonconform', 'nonconforming-user-dir')):
            case(f'stderr-library-init-{_label}-{_how}' + _suffix, mode=_mode,
                 crash=library_expected(_site, _how, _mode), issues=('545',))(library_probe(_site, _how))


# A directory where a system library should be (register row 96). A size of
# 8 or more reads as a header-sized file, `read` then fails with EISDIR and
# `assert(ret_len == sizeof(length))` kills the pin (`memory_chunk.h:493`,
# class (c)); a smaller size is `MemoryChunk::mmap`'s ordinary `false` and
# the empty chunk the pin dies on later (row 76, class (b)). The long entry
# name reaches `:493` on ext4 and on btrfs; the one-character entry reaches
# it on ext4 and row 76 on btrfs, so both shapes match whichever site the
# pin's own signal names.
LIBRARY_FINAL = {'init': 'ctx', 'load': 'load', 'save': 'save', 'mask-out': 'mask_out', 'addon': 'load'}


def library_directory_probe(site, how):
    """Wrap a library-failure case so its final operation is also exposed as ret."""
    inner = library_probe(site, how)

    def probe(k):
        """Run the configured library fault and alias the site's final flag as ret."""
        out = inner(k)
        out['ret'] = out[LIBRARY_FINAL[site]]
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    # The zhuyin facade has no addon entry point.
    for _site in (s for s in LIBRARY_FINAL if s != 'addon' or _mode == 'pinyin'):
        case(f'abort-library-directory-long-entry-{_site}' + _suffix, mode=_mode,
             abort=False, crash=library_expected(_site, 'short', _mode), either=True, issues=('545',))(library_directory_probe(_site, 'dir-long'))
        case(f'library-directory-one-entry-{_site}' + _suffix, mode=_mode, abort=False,
             crash=library_expected(_site, 'short', _mode), either=True, issues=('545',))(library_directory_probe(_site, 'dir-short'))


def library_directory_guard(site, guard):
    """Build a save or mask case with a directory at a reserved or unloaded library.

    The callable rewrites a private table.conf and trains before applying the
    fault. site='save' saves; other site values mask library four."""
    def probe(k):
        """Prepare the guarded library, replace its link with a directory and record the operation.

        For guard='reserved', configure index zero; for 'unloaded', unload index two.
        Assert initialization and training and leave the private context live."""
        system = private_system(k)
        conf = Path(system, 'table.conf')
        text = conf.read_text()
        name = 'gbk_char.bin' if guard == 'unloaded' else 'gb_char.bin'
        if guard == 'reserved':
            text = text.replace('default RESERVED NULL NULL NULL NOT_USED',
                                'default RESERVED gb_char.table reserved.bin reserved.dbin SYSTEM_FILE')
            name = 'reserved.bin'
            os.symlink(os.path.join(k.data, 'gb_char.bin'), os.path.join(system, name))
        conf.unlink()
        conf.write_text(text)
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        _, _, parse, phrase, train_args = LIBRARY_FACADES[k.mode]
        assert train_once(k, ctx, parse, phrase, train_args), 'train failed'
        if guard == 'unloaded':
            assert k.fn('unload_phrase_library', B, P, C.c_ubyte)(ctx, 2), 'unload failed'
        break_library(k, system, name, 'dir-long')
        if site == 'save':
            ret = k.fn('save', B, P)(ctx)
        else:
            ret = k.fn('mask_out', B, P, U, U)(ctx, 0xff000000, 0x04000000)
        return {'ret': ret}
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _guard in ('reserved', 'unloaded'):
        for _site in ('save', 'mask-out'):
            case(f'library-directory-guard-{_guard}-{_site}' + ('-zhuyin' if _mode == 'zhuyin' else ''),
                 mode=_mode, control=True, issues=('545',))(library_directory_guard(_site, _guard))


# The guards that keep the pin silent with the same broken library: the
# library is already loaded (a load asks for nothing), it is unloaded (a
# save and a mask-out skip it), nothing is modified (a save writes nothing),
# or it is whole (a reload maps it).
def library_control(site):
    """Build a control case for a loaded, unloaded, unmodified or intact system library.

    Supported site names select repeated load/addon load, whole reload,
    unmodified save, or trained save/mask after unloading."""
    def probe(k):
        """Apply the control's library state, record operation flags and finalize the context.

        Use checksum corruption where a broken file is needed; initialization must
        succeed. Unsupported fallback sites assert."""
        name, index, parse, text, train_args = LIBRARY_FACADES[k.mode]
        system = private_system(k)
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        unload = k.fn('unload_phrase_library', B, P, C.c_ubyte)
        load = k.fn('load_phrase_library', B, P, C.c_ubyte)
        out = {}
        if site == 'load-already-loaded':
            break_library(k, system, 'gbk_char.bin', 'checksum')
            out['load'] = load(ctx, 2)
        elif site == 'load-whole':
            out['unload'] = unload(ctx, 2)
            out['load'] = load(ctx, 2)
        elif site == 'addon-already-loaded':
            addon = k.fn('load_addon_phrase_library', B, P, C.c_ubyte)
            out['first'] = addon(ctx, 4)
            break_library(k, system, 'art.bin', 'checksum')
            out['again'] = addon(ctx, 4)
        elif site == 'save-unmodified':
            break_library(k, system, name, 'checksum')
            out['save'] = k.fn('save', B, P)(ctx)
        else:
            assert site in ('save-unloaded', 'mask-out-unloaded')
            out['train'] = train_once(k, ctx, parse, text, train_args)
            out['unload'] = unload(ctx, 2)
            break_library(k, system, 'gbk_char.bin', 'checksum')
            if site == 'save-unloaded':
                out['save'] = k.fn('save', B, P)(ctx)
            else:
                out['mask_out'] = k.fn('mask_out', B, P, U, U)(ctx, 0xff000000, 0x04000000)
        k.fn('fini', None, P)(ctx)
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _site in ('load-already-loaded', 'load-whole', 'save-unmodified', 'save-unloaded', 'mask-out-unloaded') + \
            (('addon-already-loaded',) if _mode == 'pinyin' else ()):
        case(f'stderr-library-control-{_site}' + ('-zhuyin' if _mode == 'zhuyin' else ''), mode=_mode,
             control=True, stderr=True, issues=('545',))(library_control(_site))


# The pin's ignored `SubPhraseIndex::load` failure. `MemoryChunk::mmap`
# accepts a header whose length word matches `(file_size - 8)` truncated to
# `guint32` (`memory_chunk.h:499`) and whose checksum verifies; the pinned
# `SubPhraseIndex::load` then rejects the layout with a `g_return_val_if_fail`
# critical (`phrase_index.cpp:354`), which `_load_phrase_library` ignores
# (`pinyin.cpp:264-277`, `zhuyin.cpp:208-221`) and the caller reports as
# success. Main refuses the chunk outright. The sparse 2^32+8 chunk is one
# instance (its truncated payload is 0); the eight-byte header-only chunk is
# the control that separates the truncation from the ignored layout failure.
# The file lives in the private copy of the system directory and replaces
# merged.bin, gbk_char.bin (the one unloadable system library) or art.bin
# (the pinyin facade's addon entry; the zhuyin facade has none).
def chunk_limits():
    """Limit this worker to 12 GiB address space and 60 CPU seconds and disable core dumping.

    Set soft and hard resource limits without restoration and assert Linux
    PR_SET_DUMPABLE succeeds. These settings are not restored."""
    # The pin's init maps 2^32+8 and its save/mask/reinit map it again while
    # the earlier mapping is still held (freemem unmaps only 8 bytes). 8 GiB
    # makes those calls fail on `mmap` — a limit artifact, not pin behaviour.
    # 12 GiB admits the pin's init plus one re-map and the retained mapping
    # (three 4 GiB mappings would not fit, but no case needs a third). The
    # safe subject maps only the 8+data_len chunk (8 bytes for this
    # fixture). The CPU bound and dumpable=0 keep a surprise inside this
    # worker.
    resource.setrlimit(resource.RLIMIT_AS, (12 << 30, 12 << 30))
    resource.setrlimit(resource.RLIMIT_CPU, (60, 60))
    libc = C.CDLL(None, use_errno=True)
    libc.prctl.argtypes, libc.prctl.restype = [I, C.c_ulong, C.c_ulong, C.c_ulong, C.c_ulong], I
    assert libc.prctl(4, 0, 0, 0, 0) == 0, ('PR_SET_DUMPABLE failed', C.get_errno())


def chunk_init_probe(site, how):
    """Build an initialization case with a malformed merged.bin and optional saved profile.

    site='init-settled' trains and saves first; other values use a fresh profile.
    The callable applies chunk_limits and reports fixture sizes in bytes."""
    def probe(k):
        """Install the merged.bin fixture, initialize and report context truth and file sizes.

        For a settled profile, assert its initial training/save and finalize that
        context before the tested init. Leave the final context live."""
        chunk_limits()
        system = private_system(k)
        if site == 'init-settled':
            # A consumer's second run: the user dir carries the profile and
            # a per-library merged.dbin, which init merges after the failed
            # sub-index load.
            settled = k.init()
            assert settled, 'settled init failed'
            _, _, parse, text, train_args = LIBRARY_FACADES[k.mode]
            assert train_once(k, settled, parse, text, train_args), 'train failed'
            assert k.fn('save', B, P)(settled), 'save failed'
            k.fn('fini', None, P)(settled)
        path = os.path.join(system, 'merged.bin')
        break_library(k, system, 'merged.bin', how)
        out = {'ctx': bool(k.init(system=system))}
        return out | chunk_stats(path)
    return probe


def chunk_addon_probe(how, reload):
    """Build an addon-load case with a malformed art.bin and optional unload/reload."""
    def probe(k):
        """Apply worker limits, replace art.bin and record addon load flags and fixture sizes.

        Assert initial context creation; reload controls a second load after unload.
        The context remains live."""
        chunk_limits()
        system = private_system(k)
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        path = os.path.join(system, 'art.bin')
        break_library(k, system, 'art.bin', how)
        addon = k.fn('load_addon_phrase_library', B, P, C.c_ubyte)
        out = {'first': bool(addon(ctx, 4))}
        if reload:
            out['unload'] = bool(k.fn('unload_addon_phrase_library', B, P, C.c_ubyte)(ctx, 4))
            out['again'] = bool(addon(ctx, 4))
        return out | chunk_stats(path)
    return probe


def chunk_load_probe(how):
    """Build a case that unloads library two and reloads it from a malformed gbk_char.bin."""
    def probe(k):
        """Apply worker limits and record unload/load flags after replacing gbk_char.bin.

        Assert context initialization and leave the context live."""
        chunk_limits()
        system = private_system(k)
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        unload = k.fn('unload_phrase_library', B, P, C.c_ubyte)
        load = k.fn('load_phrase_library', B, P, C.c_ubyte)
        out = {'unload': bool(unload(ctx, 2))}
        break_library(k, system, 'gbk_char.bin', how)
        out['load'] = bool(load(ctx, 2))
        return out
    return probe


def chunk_save_probe(how):
    """Build a trained-save case whose merged.bin is malformed before initialization."""
    def probe(k):
        """Install the chunk fixture and, if initialization succeeds, train and save.

        Apply worker limits and always include fixture sizes in bytes. Leave any
        successful context live."""
        chunk_limits()
        system = private_system(k)
        path = os.path.join(system, 'merged.bin')
        break_library(k, system, 'merged.bin', how)
        ctx = k.init(system=system)
        out = {'ctx': bool(ctx)}
        if not ctx:
            return out | chunk_stats(path)
        _, _, parse, text, train_args = LIBRARY_FACADES[k.mode]
        out['train'] = train_once(k, ctx, parse, text, train_args)
        out['save'] = k.fn('save', B, P)(ctx)
        return out | chunk_stats(path)
    return probe


def chunk_mask_probe(how):
    """Build a library-four mask case with merged.bin malformed before initialization."""
    def probe(k):
        """Install the chunk fixture and, if initialization succeeds, train and mask library four.

        Apply worker limits and include fixture sizes in bytes on either return path."""
        chunk_limits()
        system = private_system(k)
        path = os.path.join(system, 'merged.bin')
        break_library(k, system, 'merged.bin', how)
        ctx = k.init(system=system)
        out = {'ctx': bool(ctx)}
        if not ctx:
            return out | chunk_stats(path)
        _, _, parse, text, train_args = LIBRARY_FACADES[k.mode]
        out['train'] = train_once(k, ctx, parse, text, train_args)
        out['mask_out'] = k.fn('mask_out', B, P, U, U)(ctx, 0xff000000, 0x04000000)
        return out | chunk_stats(path)
    return probe


def chunk_reinit_probe():
    """Build a case that initializes twice against a sparse merged.bin."""
    def probe(k):
        """Apply worker limits, initialize twice, and finalize each non-NULL context.

        Return both context truth values and sparse fixture sizes in bytes."""
        chunk_limits()
        system = private_system(k)
        path = os.path.join(system, 'merged.bin')
        break_library(k, system, 'merged.bin', 'sparse')
        first = k.init(system=system)
        if first:
            k.fn('fini', None, P)(first)
        second = k.init(system=system)
        out = {'ctxs': [bool(first), bool(second)]}
        if second:
            k.fn('fini', None, P)(second)
        return out | chunk_stats(path)
    return probe


def chunk_candidates_probe():
    """Build a candidate-observation case with a sparse merged.bin."""
    def probe(k):
        """Guess through the sparse-library context and capture at most eight candidate rows.

        Apply worker limits; return early on NULL init. Otherwise record parse,
        sentence/candidate flags, total count and [type, text] rows, replacing
        invalid UTF-8 and treating a missing or empty string as None."""
        chunk_limits()
        system = private_system(k)
        break_library(k, system, 'merged.bin', 'sparse')
        ctx = k.init(system=system)
        out = {'ctx': bool(ctx)}
        if not ctx:
            return out
        inst = k.fn('alloc_instance', P, P)(ctx)
        _, _, parse, text, _ = LIBRARY_FACADES[k.mode]
        out['parse'] = k.fn(parse, Z, P, S)(inst, text)
        out['guess_sentence'] = bool(k.fn('guess_sentence', B, P)(inst))
        if k.mode == 'pinyin':
            out['guess_candidates'] = bool(k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0))
        else:
            out['guess_candidates'] = bool(k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0))
        n = U()
        k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(n))
        out['n'] = n.value
        rows = []
        for i in range(min(n.value, 8)):
            cand, ctype, string = P(), I(), S()
            k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
            k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(ctype))
            k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(string))
            rows.append([ctype.value, string.value.decode('utf-8', 'replace') if string.value else None])
        out['rows'] = rows
        return out
    return probe


def chunk_token_probe():
    """Build a token-getter case for the first two items of a sparse library four."""
    def probe(k):
        """Read phrase, pronunciation count and unigram outputs after sparse-library init.

        Apply worker limits and return early on NULL init. For pinyin, also open
        and close an export iterator to record whether library four has a next row.
        Decode/free returned phrase strings; leave the instance and context live."""
        chunk_limits()
        system = private_system(k)
        break_library(k, system, 'merged.bin', 'sparse')
        ctx = k.init(system=system)
        out = {'ctx': bool(ctx)}
        if not ctx:
            return out
        inst = k.fn('alloc_instance', P, P)(ctx)
        # The empty sub-index owns no item: every token read refuses, and
        # the library's export range is empty.
        for token in (0x04000001, 0x04000002):
            length, text = U(0), P(0)
            n, freq = U(0), U(0)
            out[hex(token)] = [
                bool(k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
                    inst, token, C.byref(length), C.byref(text))),
                length.value,
                None if not text.value else k.text(text.value),
                bool(k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(inst, token, C.byref(n))),
                n.value,
                bool(k.fn('token_get_unigram_frequency', B, P, U, C.POINTER(U))(inst, token, C.byref(freq))),
                freq.value,
            ]
        if k.mode == 'pinyin':
            it = k.fn('begin_get_phrases', P, P, U)(ctx, 4)
            out['has_next'] = bool(k.fn('iterator_has_next_phrase', B, P)(it))
            k.fn('end_get_phrases', None, P)(it)
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    for _site, _label in (('init', ''), ('init-settled', '-settled')):
        case(f'sparse-library-init{_label}{_suffix}', mode=_mode,
             stderr=True)(chunk_init_probe(_site, 'sparse'))
    case(f'sparse-library-load{_suffix}', mode=_mode, stderr=True)(chunk_load_probe('sparse'))
    case(f'sparse-library-save{_suffix}', mode=_mode, stderr=True,
         userdir=True)(chunk_save_probe('sparse'))
    case(f'sparse-library-mask-out{_suffix}', mode=_mode, stderr=True,
         userdir=True)(chunk_mask_probe('sparse'))
    case(f'sparse-library-reinit{_suffix}', mode=_mode, stderr=True)(chunk_reinit_probe())
    case(f'sparse-library-candidates{_suffix}', mode=_mode, stderr=True)(chunk_candidates_probe())
case('sparse-library-addon', stderr=True)(chunk_addon_probe('sparse', False))
case('sparse-library-addon-reload', stderr=True)(chunk_addon_probe('sparse', True))
case('sparse-library-tokens', stderr=True)(chunk_token_probe())
case('sparse-library-tokens-zhuyin', mode='zhuyin', stderr=True)(chunk_token_probe())
for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    case(f'header-only-library-init{_suffix}', mode=_mode,
         stderr=True)(chunk_init_probe('init', 'header-only'))
    case(f'header-only-library-load{_suffix}', mode=_mode,
         stderr=True)(chunk_load_probe('header-only'))
case('header-only-library-save', stderr=True, userdir=True)(chunk_save_probe('header-only'))


def nonzero_total_candidates_probe():
    """Build a candidate guess with a nonzero-total malformed sub-index and attempted total wrap."""
    def probe(k):
        """Derive the wrap delta from three intact libraries and guess with malformed merged.bin.

        Apply worker limits and assert initialization. Sum stored totals modulo
        2**32, attempt the compensating token update, and return parse/guess flags;
        do not assume the update succeeded."""
        chunk_limits()
        system = private_system(k)
        break_library(k, system, 'merged.bin', 'nonzero-layout')
        # The loaded valid libraries' stored totals; the failed merged is
        # excluded by the pin, and (before the correction) added by main.
        total = 0
        for name in ('gb_char.bin', 'gbk_char.bin', 'opengram.bin'):
            total = (total + int.from_bytes(Path(system, name).read_bytes()[8:12], 'little')) & 0xFFFFFFFF
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        inst = k.fn('alloc_instance', P, P)(ctx)
        delta = (1 << 32) - total
        add = k.fn('token_add_unigram_frequency', B, P, U, U)
        out = {'add': bool(add(inst, 0x04000001, delta & 0xFFFFFFFF))}
        _, _, parse, text, _ = LIBRARY_FACADES[k.mode]
        out['parse'] = k.fn(parse, Z, P, S)(inst, text)
        out['guess_sentence'] = bool(k.fn('guess_sentence', B, P)(inst))
        if k.mode == 'pinyin':
            out['ret'] = bool(k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0))
        else:
            out['ret'] = bool(k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0))
        return out
    return probe


def nonzero_total_save_probe():
    """Build a save case that observes the log header from a malformed sub-index with total 123."""
    def probe(k):
        """Train and save with nonzero-layout merged.bin, then read two fixed log-header totals.

        Apply worker limits and assert init. Return training/save flags plus file
        length in bytes and the two little-endian words, without validating the log."""
        chunk_limits()
        system = private_system(k)
        break_library(k, system, 'merged.bin', 'nonzero-layout')
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        _, _, parse, text, train_args = LIBRARY_FACADES[k.mode]
        out = {'train': train_once(k, ctx, parse, text, train_args)}
        out['save'] = k.fn('save', B, P)(ctx)
        # The local sub-index total survives in the failed library's log:
        # a MODIFY_HEADER record carries the same old/new 123.
        data = Path(k.user, 'merged.dbin').read_bytes()
        payload = data[8:]
        out['dbin'] = [len(data), int.from_bytes(payload[10:14], 'little'),
                       int.from_bytes(payload[14:18], 'little')]
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    case(f'nonzero-total-candidates{_suffix}', mode=_mode, abort=False,
         userdir=True, abort_logs=[[None, 8], [WARNING_DOMAIN[_mode], 16]])(
             nonzero_total_candidates_probe())
    case(f'nonzero-total-save{_suffix}', mode=_mode, stderr=True)(nonzero_total_save_probe())


def crash_layout_init_probe(how, library='gbk_char.bin'):
    """Build an initialization case for the selected broken library and layout."""
    def probe(k):
        """Apply worker limits, replace the private library and return initialization truth."""
        chunk_limits()
        system = private_system(k)
        break_library(k, system, library, how)
        return {'init': bool(k.init(system=system))}
    return probe

def log_walk(data):
    """Return the first MODIFY_HEADER totals and counts of recognized log records.

    Skip the eight-byte chunk header without validating it. Decode little-endian
    fields following PhraseIndexLogger::next_record in
    src/storage/phrase_index_logger.h at pin 074a2219: type/token are four bytes,
    lengths two bytes. ADD/REMOVE carry length/data; MODIFY carries old/new
    lengths then old/new data; MODIFY_HEADER carries one length then both
    totals. Stop on unknown types, fewer than ten remaining bytes,
    or an incomplete header record. ADD/REMOVE/MODIFY counts can include a
    truncated final record; only header payload bounds are checked. Return
    None for totals if no complete header record was seen."""
    payload = data[8:]
    pos = 0
    header = None
    counts = {'add': 0, 'remove': 0, 'modify': 0, 'header': 0}
    while pos + 10 <= len(payload):
        typ = int.from_bytes(payload[pos:pos + 4], 'little')
        if typ == 4:
            ln = int.from_bytes(payload[pos + 8:pos + 10], 'little')
            if pos + 10 + 2 * ln > len(payload):
                break
            if header is None:
                header = [int.from_bytes(payload[pos + 10:pos + 14], 'little'),
                          int.from_bytes(payload[pos + 14:pos + 18], 'little')]
            counts['header'] += 1
            pos += 10 + 2 * ln
        elif typ in (1, 2):
            ln = int.from_bytes(payload[pos + 8:pos + 10], 'little')
            counts['add' if typ == 1 else 'remove'] += 1
            pos += 10 + ln
        elif typ == 3:
            old = int.from_bytes(payload[pos + 8:pos + 10], 'little')
            new = int.from_bytes(payload[pos + 10:pos + 12], 'little')
            counts['modify'] += 1
            pos += 12 + old + new
        else:
            break
    return header, counts


def save_replacement_probe(initial, replacement):
    """Build a save case with merged.bin replaced after its original bytes were loaded.

    initial='invalid' installs a malformed layout before init. replacement is
    same, invalid, valid, or any other value to use gbk_char.bin as the replacement.
    The callable reports the saved log's byte length, SHA-256, totals and counts."""
    def probe(k):
        """Train, atomically replace the private merged.bin when requested, and inspect its saved log.

        Apply worker limits and assert init. Replacement uses a .new sibling and
        os.replace; native training/save flags are recorded rather than asserted."""
        chunk_limits()
        system = private_system(k)
        if initial == 'invalid':
            break_library(k, system, 'merged.bin', 'nonzero-layout')
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        _, _, parse, text, train_args = LIBRARY_FACADES[k.mode]
        out = {'train': train_once(k, ctx, parse, text, train_args)}
        if replacement != 'same':
            path = Path(system, 'merged.bin')
            if replacement == 'invalid':
                data = nonzero_layout_chunk()
            elif replacement == 'valid':
                data = Path(k.data, 'merged.bin').read_bytes()
            else:
                # A different valid chunk: the gbk sub-index's bytes under
                # the merged path. Its items and total differ from merged's.
                data = Path(k.data, 'gbk_char.bin').read_bytes()
            tmp = Path(str(path) + '.new')
            tmp.write_bytes(data)
            os.replace(tmp, path)
        out['save'] = bool(k.fn('save', B, P)(ctx))
        data = Path(k.user, 'merged.dbin').read_bytes()
        header, counts = log_walk(data)
        out['merged'] = [len(data), hashlib.sha256(data).hexdigest(), header]
        out['records'] = counts
        return out
    return probe


def save_changed_with_override_probe():
    """Build a save comparison combining a loaded user override and a replaced system chunk.

    The callable changes token 0x01001225 from 你 to 龘 in a user log, loads it,
    then replaces gb_char.bin with gbk_char.bin before saving. It records the
    new log bytes by length/hash and its decoded header and record counts."""
    def probe(k):
        """Write the single-character override, train, replace the system chunk and summarize the saved log.

        Assert the fixed token names 你 and initialization succeeds. Use the layouts
        from SubPhraseIndex::load and PhraseItem::get_phrase_string in
        src/storage/phrase_index.cpp and PhraseIndexLogger::next_record in
        src/storage/phrase_index_logger.h at pin 074a2219. Apply worker limits and
        replace only private system links."""
        chunk_limits()
        system = private_system(k)
        payload = Path(k.data, 'gb_char.bin').read_bytes()[8:]
        word = lambda offset: int.from_bytes(payload[offset:offset + 4], 'little')
        total, index, content = word(0), word(4), word(8)
        token = 0x01001225
        offset = content + word(index + (token & 0x00FFFFFF) * 4)
        length, prons = payload[offset:offset + 2]
        size = 6 + length * 4 + prons * (length * 2 + 4)
        old = payload[offset:offset + size]
        assert length == 1 and int.from_bytes(old[6:10], 'little') == ord('你')
        new = old[:6] + ord('龘').to_bytes(4, 'little') + old[10:]
        record = ((3).to_bytes(4, 'little') +
                  (token & 0x00FFFFFF).to_bytes(4, 'little') +
                  len(old).to_bytes(2, 'little') + len(new).to_bytes(2, 'little') +
                  old + new)
        _write_user_conf(k)
        _write_log(k, _modify_header(0, total, total) + record, 'gb_char.dbin')
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        _, _, parse, text, train_args = LIBRARY_FACADES[k.mode]
        out = {'train': train_once(k, ctx, parse, text, train_args)}
        data = Path(k.data, 'gbk_char.bin').read_bytes()
        path = Path(system, 'gb_char.bin')
        tmp = Path(str(path) + '.new')
        tmp.write_bytes(data)
        os.replace(tmp, path)
        out['save'] = bool(k.fn('save', B, P)(ctx))
        data = Path(k.user, 'gb_char.dbin').read_bytes()
        header, counts = log_walk(data)
        out['dbin'] = [len(data), hashlib.sha256(data).hexdigest(), header]
        out['records'] = counts
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    # Class (b) row 76 extensions: the same SubPhraseIndex::load separator
    # reads run past the mapped chunk; the subject refuses without the read.
    # The pin SIGSEGVs, so these are crash controls (MATCH on the parent too).
    case('crash-oob-layout' + _suffix, mode=_mode, crash={'init': False},
         control=True)(crash_layout_init_probe('oob-layout'))
    # A short chunk keeps the pin's observable empty sub-index + critical;
    # the out-of-object index reads behind it are not reproduced (class (b)
    # arm of row 76), so the answer is the matched control.
    case('short-chunk-layout' + _suffix, mode=_mode, stderr=True,
         control=True)(crash_layout_init_probe('short-chunk'))
    case('save-replacement-valid-to-different-valid-with-user-override' +
         ('-zhuyin' if _mode == 'zhuyin' else ''), mode=_mode, stderr=True,
         userdir=True)(save_changed_with_override_probe())

for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    for _name, _initial, _replacement in (
            ('save-replacement-valid-to-invalid', 'valid', 'invalid'),
            ('save-replacement-invalid-to-valid', 'invalid', 'valid'),
            ('save-replacement-valid-to-different-valid', 'valid', 'different'),
            ('save-unchanged-valid', 'valid', 'same'),
            ('save-unchanged-layout-failed', 'invalid', 'same')):
        case(_name + _suffix, mode=_mode, stderr=True, userdir=True)(
            save_replacement_probe(_initial, _replacement))
    # `_write_files` emits the critical synchronously; under fatal mode both
    # sides die before the failing library's log and after the earlier
    # libraries' writes (pinyin `.tmp`s; zhuyin already-renamed `.dbin`s).
    case('save-fatal-valid-to-invalid' + _suffix, mode=_mode, fatal=True,
         both_abort=True, stderr=True, userdir=True)(
        save_replacement_probe('valid', 'invalid'))


def save_missing_system_probe():
    """Build a trained-save case that removes gb_char.bin after initialization.

    Record training and save flags if native calls return; crash expectations
    are supplied separately by the case registration."""
    def probe(k):
        """Apply worker limits, train a private context, unlink gb_char.bin and record save.

        Assert initialization; leave the context live for the isolated worker."""
        chunk_limits()
        system = private_system(k)
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        _, _, parse, text, train_args = LIBRARY_FACADES[k.mode]
        out = {'train': train_once(k, ctx, parse, text, train_args)}
        Path(system, 'gb_char.bin').unlink()
        out['save'] = bool(k.fn('save', B, P)(ctx))
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    case('crash-save-missing-system' + _suffix, mode=_mode,
         crash={'save': True}, control=True)(save_missing_system_probe())


def save_unloaded_replacement_probe():
    """Build a save case that replaces an unloaded system library with another valid chunk.

    At pin 074a2219, _write_files in src/pinyin.cpp and zhuyin_save in
    src/zhuyin.cpp skip libraries with no sub-index. The callable records flags
    after unloading library two and replacing its private gbk_char.bin."""
    def probe(k):
        """Train, unload library two, replace gbk_char.bin with merged.bin and save.

        Apply worker limits, assert init, and use a .new sibling plus os.replace.
        Record operation flags without asserting unload, train or save."""
        chunk_limits()
        system = private_system(k)
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        _, _, parse, text, train_args = LIBRARY_FACADES[k.mode]
        out = {'train': train_once(k, ctx, parse, text, train_args)}
        out['unload'] = bool(k.fn('unload_phrase_library', B, P, C.c_ubyte)(ctx, 2))
        path = Path(system, 'gbk_char.bin')
        tmp = Path(str(path) + '.new')
        tmp.write_bytes(Path(k.data, 'merged.bin').read_bytes())
        os.replace(tmp, path)
        out['save'] = bool(k.fn('save', B, P)(ctx))
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    case('save-unloaded-replacement' + _suffix, mode=_mode, stderr=True,
         userdir=True)(save_unloaded_replacement_probe())
for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    for _how in ('second-separator', 'third-separator'):
        case(f'{_how}-library-init{_suffix}', mode=_mode,
             stderr=True)(chunk_init_probe('init', _how))


# --------------------------------------------------------------------------

def remember_export(k, library, phrase, adjacent=False):
    """Return matching phrase export rows and release the iterator and returned strings.

    Rows contain [flag, phrase, reading, count]. Return 'NO EXPORT API' for
    zhuyin. adjacent stops at the first different phrase after a match; this
    can omit later matching tokens if the same text is not contiguous."""
    if k.mode == 'zhuyin':
        return 'NO EXPORT API'
    it=k.fn('begin_get_phrases',P,P,U)(k.ctx,library)
    rows=[]
    while k.fn('iterator_has_next_phrase',B,P)(it):
        ph,py,n=P(),P(),I()
        ret=k.fn('iterator_get_next_phrase',B,P,C.POINTER(P),C.POINTER(P),C.POINTER(I))(it,C.byref(ph),C.byref(py),C.byref(n))
        text,reading=k.text(ph.value),k.text(py.value)
        if text==phrase: rows.append([ret,text,reading,n.value])
        elif adjacent and rows: break
    k.fn('end_get_phrases',None,P)(it)
    return rows


def remember_tones(k):
    """Check remember/export API presence and, for pinyin, remember a toned imported phrase.

    Import 你好你好, parse its toned spelling, guess, remember with count three
    and export its rows. Free the cached instance and finalize the context on
    the pinyin path without clearing the kit's pointers."""
    out={'remember_api':hasattr(k.lib,k.mode+'_remember_user_input'),
         'export_api':hasattr(k.lib,k.mode+'_begin_get_phrases')}
    if k.mode=='zhuyin':return out
    it=k.fn('begin_add_phrases',P,P,U)(k.ctx,7)
    out['add']=k.fn('iterator_add_phrase',B,P,S,S,I)(it,'你好你好'.encode(),b"ni3'hao3'ni3'hao3",100000)
    k.fn('end_add_phrases',None,P)(it)
    out['parse']=k.fn('parse_more_full_pinyins',Z,P,S)(k.inst,b'ni3hao3ni3hao3')
    out['guess']=k.fn('guess_sentence',B,P)(k.inst)
    out['remember']=k.fn('remember_user_input',B,P,S,I)(k.inst,'你好你好'.encode(),3)
    out['export']=remember_export(k,7,'你好你好')
    k.fn('free_instance',None,P)(k.inst)
    k.fn('fini',None,P)(k.ctx)
    return out


case('remember-tones-pinyin')(remember_tones)
case('remember-api-absent-zhuyin', mode='zhuyin', control=True)(remember_tones)

# #643: g_build_filename drops empty elements, but stops at NULL.
def init_system_argument(k, form):
    """Initialize with a literal empty, NULL, dot or absolute system argument in a temporary cwd.

    form='empty-missing' omits system links; other forms link regular data files.
    On successful init, cache the context, parse and guess, then free/finalize
    without clearing caches. Return user filenames and switch cwd to scratch
    on the normal path; exceptions do not restore cwd."""
    with tempfile.TemporaryDirectory(prefix='sys-', dir=k.scratch) as cwd:
        if form != 'empty-missing':
            # Read-only system files; only the separate user directory is writable.
            for path in Path(k.data).iterdir():
                if path.is_file():
                    os.symlink(path, Path(cwd) / path.name)
        os.chdir(cwd)
        system = {'empty': b'', 'empty-missing': b'', 'null': None,
                  'dot': b'.', 'absolute': os.fsencode(cwd)}[form]
        ctx = k.init(system, k.user, literal=True)
        out = {'init': bool(ctx)}
        if ctx:
            k._ctx = ctx
            inst = k.inst
            name = 'parse_more_full_pinyins' if k.mode == 'pinyin' else 'parse_more_chewings'
            out['parse'] = k.fn(name, Z, P, S)(inst, b'ni3hao3' if k.mode == 'pinyin' else b'su3cl3')
            out['guess'] = k.fn('guess_sentence', B, P)(inst)
            k.fn('free_instance', None, P)(inst)
            k.fn('fini', None, P)(ctx)
        out['files'] = sorted(os.listdir(k.user))
        os.chdir(k.scratch)
        return out


for _mode in ('pinyin', 'zhuyin'):
    for _form in ('empty', 'empty-missing', 'null', 'dot', 'absolute'):
        case('init-system-' + _form + '-' + _mode, mode=_mode,
             control=_form in ('empty-missing', 'dot', 'absolute'), stderr=True, issues=('643',))(
                 lambda k, form=_form: init_system_argument(k, form))


def remember_review(k, double=False):
    """Observe remembered readings after clearing tones or using the double-pinyin parser.

    Use count three, export the target phrase, then free/finalize cached native
    objects without clearing their pointers. The full-pinyin path clears options
    after guessing; the double-pinyin path retains the tone option."""
    k.fn('set_options', B, P, U)(k.ctx, 0x20)
    parse = 'parse_more_double_pinyins' if double else 'parse_more_full_pinyins'
    phrase = '你' if double else '你好'
    out = {'parse': k.fn(parse, Z, P, S)(k.inst, b'ni3' if double else b'ni3hao3')}
    out['guess'] = k.fn('guess_sentence', B, P)(k.inst)
    if not double:
        out['clear-tone'] = k.fn('set_options', B, P, U)(k.ctx, 0)
    out['remember'] = k.fn('remember_user_input', B, P, S, I)(k.inst, phrase.encode(), 3)
    out['export'] = remember_export(k, 7, phrase)
    k.fn('free_instance', None, P)(k.inst)
    k.fn('fini', None, P)(k.ctx)
    return out

case('remember-cleared-tone-pinyin')(remember_review)
case('remember-double-tone-pinyin')(lambda k: remember_review(k, True))
# #642: NULL is transient state, not an absent user index. Snapshot both
# filesystem destinations at every step, including a pre-existing canary.
def transient_files(root):
    """Snapshot recursive file entries as relative-path keys with mode bits and raw bytes.

    Follow is_file/stat/read_bytes behavior for symlinks; omit directories and
    let filesystem errors propagate."""
    return {str(p.relative_to(root)): (p.stat().st_mode, p.read_bytes())
            for p in Path(root).rglob('*') if p.is_file()}


def null_user_session(k):
    """Exercise a literal NULL user directory while checking cwd and TMPDIR for file changes.

    Create two canary directories, change cwd and TMPDIR, then record training,
    imports, lookup, save and mutation outputs. Compare a second NULL-user
    context's tokens before masking the first. Finalize successful contexts on
    the normal path; TMPDIR and cached pointers are not reset. A failed first
    init returns with cwd still changed; successful completion returns to scratch."""
    cwd = tempfile.mkdtemp(prefix='cwd-', dir=k.scratch)
    tmp = tempfile.mkdtemp(prefix='tmp-', dir=k.scratch)
    for directory in (cwd, tmp):
        Path(directory, 'canary').write_bytes(b'unchanged')
    os.chdir(cwd)
    os.environ['TMPDIR'] = tmp
    baseline = [transient_files(d) for d in (cwd, tmp)]
    out = {}

    def checkpoint(label):
        """Record sorted file names whose mode or contents differ from each canary baseline.

        Include creations and deletions in cwd and the configured temporary directory."""
        out['files-' + label] = [sorted(n for n in before.keys() | after.keys()
                                       if before.get(n) != after.get(n))
                                for before, after in zip(baseline, (transient_files(d) for d in (cwd, tmp)))]

    ctx = k.init(k.data, None, literal=True)
    out['init'] = bool(ctx)
    checkpoint('init')
    if not ctx:
        return out
    k._ctx = ctx
    inst = k.inst
    train = (lambda: k.fn('train', B, P, C.c_ubyte)(inst, 0)) if k.mode == 'pinyin' else (lambda: k.fn('train', B, P)(inst))
    out['train-fresh'] = train()
    checkpoint('train-fresh')
    out['save-fresh'] = k.fn('save', B, P)(ctx)
    checkpoint('save-fresh')
    phrase = '你好你好'
    reading = ("ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ')
    reading += ("'" if k.mode == 'pinyin' else ' ') + reading
    added = []
    for library in (7,):
        it = k.fn('begin_add_phrases', P, P, U)(ctx, library)
        added.append(k.fn('iterator_add_phrase', B, P, S, S, I)(it, phrase.encode(), reading.encode(), 100000))
        k.fn('end_add_phrases', None, P)(it)
    out['added'] = added
    checkpoint('import')
    name = 'parse_more_full_pinyins' if k.mode == 'pinyin' else 'parse_more_chewings'
    raw = b'ni3hao3' if k.mode == 'pinyin' else b'su3cl3'
    out['parse'] = k.fn(name, Z, P, S)(inst, raw + raw)
    out['guess'] = k.fn('guess_sentence', B, P)(inst)
    out['tokens'] = tokens_of(k, phrase)
    if k.mode == 'pinyin':
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1f)
    else:
        k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    cand, text = P(), S()
    out['candidate0-ok'] = k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, 0, C.byref(cand))
    if cand.value:
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        out['candidate0'] = text.value.decode() if text.value else None
        if k.mode == 'pinyin':
            out['candidate0-user'] = k.fn('is_user_candidate', B, P, P)(inst, cand)
    checkpoint('lookup')
    out['train'] = train()
    if k.mode == 'pinyin':
        out['train-invalid-index'] = k.fn('train', B, P, C.c_ubyte)(inst, 255)
    checkpoint('train')
    out['save'] = k.fn('save', B, P)(ctx)
    checkpoint('save')
    token = tokens_of(k, '你')[0]
    out['frequency-before'] = unigram_of(k, token)
    out['frequency-add'] = k.fn('token_add_unigram_frequency', B, P, U, U)(inst, token, 7)
    out['frequency-after'] = unigram_of(k, token)
    if k.mode == 'pinyin':
        out['remember'] = k.fn('remember_user_input', B, P, S, I)(inst, phrase.encode(), 3)
        export = k.fn('begin_get_phrases', P, P, U)(ctx, 7)
        rows = []
        while k.fn('iterator_has_next_phrase', B, P)(export):
            ph, py, count = P(), P(), I()
            ret = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
                export, C.byref(ph), C.byref(py), C.byref(count))
            rows.append([ret, k.text(ph.value), k.text(py.value), count.value])
        k.fn('end_get_phrases', None, P)(export)
        out['export'] = rows
    other = k.init(k.data, None, literal=True)
    own_ctx, own_inst = k._ctx, k._inst
    k._ctx, k._inst = other, None
    out['other-user-tokens'] = [t for t in tokens_of(k, phrase) if t >> 24 == 7]
    k.fn('free_instance', None, P)(k._inst)
    k.fn('fini', None, P)(other)
    k._ctx, k._inst = own_ctx, own_inst
    out['mask'] = k.fn('mask_out', B, P, U, U)(ctx, 0x0f000000, 0x07000000)
    out['tokens-after-mask'] = [t for t in tokens_of(k, phrase) if t >> 24 == 7]
    checkpoint('mutations')
    k.fn('free_instance', None, P)(inst)
    k.fn('fini', None, P)(ctx)
    checkpoint('fini')
    os.chdir(k.scratch)
    return out


for _mode in ('pinyin', 'zhuyin'):
    case('null-user-session-' + _mode, mode=_mode, stderr=True, issues=('642',))(null_user_session)


def transient_library(k,library,null):
    """Import 你好 into the chosen library with either a literal NULL or ordinary user path.

    Switch cwd to scratch and replace the cached context. Return init/import
    flags, matching library tokens and exported rows, then free/finalize without
    clearing cached pointers. Native import flags are not asserted."""
    os.chdir(k.scratch)
    k._ctx=k.init(k.data, None if null else k.user, literal=True)
    out={'init':bool(k._ctx)}
    it=k.fn('begin_add_phrases',P,P,U)(k.ctx,library)
    out['begin']=bool(it)
    out['add']=k.fn('iterator_add_phrase',B,P,S,S,I)(it,'你好'.encode(),b"ni3'hao3" if k.mode=='pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode(),100000)
    k.fn('end_add_phrases',None,P)(it)
    out['tokens']=[t for t in tokens_of(k,'你好') if t>>24==library]
    out['export']=remember_export(k,library,'你好')
    k.fn('free_instance',None,P)(k.inst)
    k.fn('fini',None,P)(k.ctx)
    return out



for _mode in ('pinyin', 'zhuyin'):
    for _library in range(1, 8):
        for _null in (False, True):
            case('user-library-' + str(_library) + ('-null-' if _null else '-ordinary-') + _mode,
                 mode=_mode, control=not _null, stderr=True, issues=('642',))(
                lambda k, library=_library, null=_null: transient_library(k, library, null))


# #759: the export iterator is the pin's cursor (pinyin.cpp:662-768 at
# 074a2219). Every get_next re-reads the item it is at and re-probes the
# range (:748-767), so an import that lands between two calls is exported:
# a reading merged into the item under the cursor, and a token appended
# past it. The parent buffered each probed item's rows at the probe.
def export_rows(k, it, limit=None):
    """Consume up to limit export rows without closing the caller's iterator.

    Return [flag, phrase, reading, count] rows and decode/free the returned
    strings. None consumes to exhaustion; even a reached limit still calls
    has_next first. Native getter failure does not stop iteration."""
    rows = []
    while k.fn('iterator_has_next_phrase', B, P)(it) and (limit is None or len(rows) < limit):
        ph, py, n = P(), P(), I(UNTOUCHED)
        ret = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
            it, C.byref(ph), C.byref(py), C.byref(n))
        rows.append([ret, k.text(ph.value), k.text(py.value), n.value])
    return rows


def import_rows(k, library, rows):
    """Import (phrase, reading, count) rows into the cached context and return add flags.

    UTF-8 encode both strings and end the iterator on the normal path; begin/add
    results are not asserted and exceptions do not guarantee iterator cleanup."""
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, library)
    added = [k.fn('iterator_add_phrase', B, P, S, S, I)(it, phrase.encode(), reading.encode(), count)
             for phrase, reading, count in rows]
    k.fn('end_add_phrases', None, P)(it)
    return added


def export_open_across_import_system(k):
    """Keep a system export cursor open while importing another reading for its next token.

    Use library one's first two tokens and assert their getter results. Consume
    the first token's rows, add the second token's tone-one reading, then read
    its original count plus one. Close the iterator and free/finalize cached
    objects on success; the pronunciation array remains allocated."""
    # Library 1's first two tokens: their reading counts, and the second's
    # text and first reading through the token getters.
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    counts = []
    for token in (0x01000001, 0x01000002):
        count = U(UNTOUCHED)
        assert k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(k.inst, token, C.byref(count))
        counts.append(count.value)
    length, text = U(UNTOUCHED), P(UNTOUCHED)
    assert k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
        k.inst, 0x01000002, C.byref(length), C.byref(text))
    phrase = k.text(text.value)
    keys = new(0, 0, 2)
    assert k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, 0x01000002, 0, keys)
    view = C.cast(keys, C.POINTER(Arr)).contents
    syllables = []
    for i in range(view.len):
        key = C.c_ushort(C.cast(view.data, C.POINTER(C.c_ushort))[i])
        rendered = P(UNTOUCHED)
        assert k.fn('get_pinyin_string', B, P, C.POINTER(C.c_ushort), C.POINTER(P))(
            k.inst, C.byref(key), C.byref(rendered))
        syllables.append(k.text(rendered.value))
    # The same syllables with tone 1: a reading the item does not hold.
    reading = "'".join(syllable + '1' for syllable in syllables)
    it = k.fn('begin_get_phrases', P, P, U)(k.ctx, 1)
    out = {'counts': counts, 'phrase': phrase, 'reading': reading}
    # Every row of the first token: the cursor has probed the second.
    out['first'] = export_rows(k, it, counts[0])
    out['added'] = import_rows(k, 1, [(phrase, reading, 5)])
    out['second'] = export_rows(k, it, counts[1] + 1)
    k.fn('end_get_phrases', None, P)(it)
    k.fn('free_instance', None, P)(k.inst)
    k.fn('fini', None, P)(k.ctx)
    return out


def export_open_across_import_user(k):
    """Observe a live user export cursor across insertion of a new token and reading.

    Seed 你好, open/probe the iterator, import 好你 and a toned 你好 reading,
    then consume remaining rows. Close the iterator and finalize the context
    without clearing the cached pointer."""
    out = {'seed': import_rows(k, 7, [('你好', "ni'hao", 5)])}
    it = k.fn('begin_get_phrases', P, P, U)(k.ctx, 7)
    out['has_next'] = k.fn('iterator_has_next_phrase', B, P)(it)
    out['added'] = import_rows(k, 7, [('好你', "hao'ni", 7), ('你好', "ni2'hao2", 3)])
    out['rows'] = export_rows(k, it)
    out['has_next_after'] = k.fn('iterator_has_next_phrase', B, P)(it)
    k.fn('end_get_phrases', None, P)(it)
    k.fn('fini', None, P)(k.ctx)
    return out


# #790: train_result3 changes the live PhraseItem (074a2219
# phonetic_lookup.h:925-929; phrase_index.cpp:114-143). Export reads it on
# each call (pinyin.cpp:707-739), before save or a library reload.
def issue790_export(k, action='train'):
    """Compare exports of 你 around the selected training and persistence sequence.

    Actions no-train and unconstrained only guess or train without selection;
    others train the selected token. open-before keeps an export cursor live;
    save-reopen and replayed-train finalize and clear caches, with the latter
    training again after lazy reopening. Return before/after rows and call results."""
    phrase, token = '你', tokens_of(k, '你')[0]
    out = {'before': remember_export(k, 1, phrase)}
    iterator = (k.fn('begin_get_phrases', P, P, U)(k.ctx, 1)
                if action == 'open-before' else None)
    inst = k.inst
    out['parsed'] = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    if action in ('unconstrained', 'no-train'):
        out['guessed'] = k.fn('guess_sentence', B, P)(inst)
        if action == 'unconstrained':
            out['train'] = k.fn('train', B, P, C.c_ubyte)(inst, 0)
    else:
        out['training'] = train_selected_top(k, phrase, token, guess_before_choose=True)
    if action in ('save-reopen', 'replayed-train'):
        out['save'] = k.fn('save', B, P)(k.ctx)
        k.fn('free_instance', None, P)(inst)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = k._inst = None
        if action == 'replayed-train':
            out['replayed'] = remember_export(k, 1, phrase)
            assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'ni')
            out['retraining'] = train_selected_top(k, phrase, token, guess_before_choose=True)
    if iterator:
        out['after'] = [row for row in export_rows(k, iterator) if row[1] == phrase]
        k.fn('end_get_phrases', None, P)(iterator)
    else:
        out['after'] = remember_export(k, 1, phrase)
    return out


case('issue790-export-after-train-system', issues=('790',))(issue790_export)
for _action in ('no-train', 'unconstrained', 'open-before', 'save-reopen', 'replayed-train'):
    case('issue790-export-' + _action + '-system',
         control=_action in ('no-train', 'unconstrained', 'save-reopen'), issues=('790',))(
             lambda k, action=_action: issue790_export(k, action))


def issue790_readings(k, phrase, readings, unigram=None):
    """Replace one library-one character's stored readings through a user MODIFY log.

    readings contains (pinyin spelling, count) pairs encoded as 16-bit keys and
    32-bit counts. Keep the unigram unless supplied; wrap the changed library
    total modulo 2**32. Assert the token's library and single-character length,
    free/finalize cached objects, clear caches, write the marker/log and return
    the token; reopening is deferred. Layouts follow PhraseItem::get_nth_pronunciation
    in src/storage/phrase_index.cpp, PhraseItem::get_unigram_frequency in
    src/storage/phrase_index.h and PhraseIndexLogger::next_record in
    src/storage/phrase_index_logger.h at pin 074a2219."""
    token = tokens_of(k, phrase)[0]
    assert token >> 24 == 1
    packed = [(key_out(k, text.encode())[1], count) for text, count in readings]
    payload = Path(k.data, 'gb_char.bin').read_bytes()[8:]
    word = lambda offset: int.from_bytes(payload[offset:offset + 4], 'little')
    total, index, content = word(0), word(4), word(8)
    offset = content + word(index + (token & 0x00FFFFFF) * 4)
    length, nprons = payload[offset:offset + 2]
    assert length == 1
    old = payload[offset:offset + 10 + nprons * 6]
    previous = int.from_bytes(old[2:6], 'little')
    unigram = previous if unigram is None else unigram
    new = (bytes((1, len(packed))) + unigram.to_bytes(4, 'little') + old[6:10] +
           b''.join(key.to_bytes(2, 'little') + count.to_bytes(4, 'little')
                    for key, count in packed))
    record = ((3).to_bytes(4, 'little') + (token & 0x00FFFFFF).to_bytes(4, 'little') +
              len(old).to_bytes(2, 'little') + len(new).to_bytes(2, 'little') + old + new)
    k.fn('free_instance', None, P)(k.inst)
    k.fn('fini', None, P)(k.ctx)
    k._ctx = k._inst = None
    _write_user_conf(k)
    _write_log(k, _modify_header(0, total, (total + unigram - previous) & 0xFFFFFFFF) + record)
    return token


def issue790_bounded_counts(k, count):
    """Seed 你 with the supplied ni3 count and two zero-count readings, then compare exports after training.

    Assert ni parses and train its selected token after the rewritten log reopens."""
    token = issue790_readings(k, '你', [('ni3', count), ('ni2', 0), ('na4', 0)])
    out = {'before': remember_export(k, 1, '你')}
    assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'ni')
    out['training'] = train_selected_top(k, '你', token, guess_before_choose=True)
    out['after'] = remember_export(k, 1, '你')
    return out


case('issue790-export-zero-and-signed-system', issues=('790',))(
    lambda k: issue790_bounded_counts(k, (1 << 31) - 69))
case('issue790-export-prefix-overflow-system', issues=('790',))(
    lambda k: issue790_bounded_counts(k, (1 << 32) - 76))


def issue790_imported(k, appended=False):
    """Import a system reading or appended four-character phrase and compare exports around training.

    appended chooses 你你你你; otherwise add ni2 to 你. Parse the imported
    reading and train the first matching token, recording import and training outputs."""
    phrase, reading = ('你你你你', "ni'ni'ni'ni") if appended else ('你', 'ni2')
    out = {'imported': import_rows(k, 1, [(phrase, reading, 5)]),
           'before': remember_export(k, 1, phrase)}
    token = tokens_of(k, phrase)[0]
    assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, reading.encode())
    out['training'] = train_selected_top(k, phrase, token, guess_before_choose=True)
    out['after'] = remember_export(k, 1, phrase)
    return out


case('issue790-export-override-only-system', issues=('790',))(issue790_imported)
case('issue790-export-appended-system', issues=('790',))(lambda k: issue790_imported(k, appended=True))


@case('issue790-guess-after-train-system', issues=('790',))
def _(k):
    """Compare ni guesses before and after training a low-mass reading of 尼.

    Rewrite its pronunciation counts and system/user start rows, train the selected
    token, clear the constraint and collect a fresh guess without saving."""
    # The first ni reading has 1/100 of 尼's pronunciation mass. A large
    # unrelated start-row total makes the new bigram too small to explain
    # the ranking change: only reading the trained pronunciation can do it.
    token = issue790_readings(k, '尼', [('ni2', 1), ('na2', 99)], 200000)
    _replace_system_start_row(k, _gram_bytes(1, [(_PAST_RANGES, 1)]))
    _replace_user_bigram(k, [(1, _gram_bytes(1 << 28, [(_PAST_RANGES, 1 << 28)]))])
    out = {'parsed': k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'ni')}
    out['training'] = train_selected_top(k, '尼', token, guess_before_choose=True)
    out['before'] = out['training'].pop('before-choose')
    out['clear'] = k.fn('clear_constraint', B, P, Z)(k.inst, 0)
    out['after'] = _guess_out(k, b'ni')
    return out


@case('issue790-character-offset-after-train-system', issues=('790',))
def _(k):
    """Compare character-offset output before and after training an initially zero-count na reading.

    Seed 哪 with na3/nei3, train through incomplete n, clear its constraint,
    and query again without saving or reopening."""
    # Both readings have shipped index entries. An incomplete n path
    # trains na (initially zero) and nei; the public offset call then tests
    # na without saving/reopening. An unindexed positive reading would
    # instead exercise incomplete-key expansion's membership semantics.
    token = issue790_readings(k, '哪', [('na3', 0), ('nei3', 100)])
    assert k.fn('set_options', B, P, U)(k.ctx, 0x18a)
    get = k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))
    def offset():
        """Parse na and return the offset-call flag and sentinel-initialized output for 哪 at byte one."""
        assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'na')
        value = Z(UNTOUCHED)
        return [get(k.inst, '哪'.encode(), 1, C.byref(value)), value.value]
    out = {'before': offset()}
    assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'n')
    out['training'] = train_selected_top(k, '哪', token, guess_before_choose=True)
    out['clear'] = k.fn('clear_constraint', B, P, Z)(k.inst, 0)
    out['after'] = offset()
    return out


case('issue759-export-open-across-import-system', issues=('759',))(export_open_across_import_system)
case('issue759-export-open-across-import-user', issues=('759',))(export_open_across_import_user)


# #525: reduce_tokens asserts on the sixth add, with five existing tokens.
def combined_import_bound(k, guard=True):
    """Import the same phrase into five libraries, optionally attempting a sixth with state checks.

    Assert the first five adds. With guard false, return their flags immediately.
    Otherwise save a baseline, attempt library six, then compare fixed fixture
    tokens, frequencies, exports, dirty/save status and files. Finalize native
    objects after the guarded attempt; the fixture assumes the listed token IDs."""
    phrase = '你好'
    reading = b"ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode()
    added = []
    for library in range(1, 6):
        it = k.fn('begin_add_phrases', P, P, U)(k.ctx, library)
        added.append(k.fn('iterator_add_phrase', B, P, S, S, I)(it, phrase.encode(), reading, 100000))
        k.fn('end_add_phrases', None, P)(it)
    assert added == [True] * 5
    if not guard:
        return {'added': added}
    assert k.fn('save', B, P)(k.ctx)
    files = transient_files(k.user)
    tokens = [16802309, 33570045, 50359794, 67109866, 83886081]
    frequencies = [unigram_of(k, token) for token in tokens]
    exports = [remember_export(k, library, phrase) for library in range(1, 8)]
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 6)
    ret = k.fn('iterator_add_phrase', B, P, S, S, I)(it, phrase.encode(), reading, 100000)
    # The pin aborts above. These fields are checked by the existing class(c)
    # driver and prove the subject refused before changing prior state.
    out = {'ret': ret,
           'untouched-tokens': tokens_of(k, phrase) == tokens,
           'untouched-frequencies': [unigram_of(k, token) for token in tokens] == frequencies,
           'untouched-readings': [remember_export(k, library, phrase) for library in range(1, 8)] == exports,
           'untouched-dirty': not k.fn('save', B, P)(k.ctx),
           'untouched-files': transient_files(k.user) == files}
    k.fn('end_add_phrases', None, P)(it)
    k.fn('free_instance', None, P)(k.inst)
    k.fn('fini', None, P)(k.ctx)
    return out

for _mode in ('pinyin', 'zhuyin'):
    case('combined-library-sixth-add-' + _mode, mode=_mode, abort=False, issues=('525',))(combined_import_bound)
    case('combined-library-fifth-add-' + _mode, mode=_mode, control=True, issues=('525',))(
        lambda k: combined_import_bound(k, guard=False))


def import_review(k, invalid=False):
    """Attempt an invalid-reading or unused-library import after populating five libraries.

    invalid selects a one-syllable reading for 你好 in library six; otherwise
    use its complete reading in library eight. Record the add flag and
    free/finalize cached native objects without clearing their pointers."""
    out = combined_import_bound(k, guard=False)
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 6 if invalid else 8)
    reading = b'ni3' if invalid else b"ni3'hao3"
    if k.mode == 'zhuyin':
        reading = ('ㄋㄧˇ' if invalid else 'ㄋㄧˇ ㄏㄠˇ').encode()
    out['ret'] = k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), reading, 3)
    k.fn('end_add_phrases', None, P)(it)
    k.fn('free_instance', None, P)(k.inst)
    k.fn('fini', None, P)(k.ctx)
    return out

for _mode in ('pinyin', 'zhuyin'):
    case('combined-library-invalid-reading-' + _mode, mode=_mode, control=True, issues=('525',))(lambda k: import_review(k, True))
    case('combined-library-unused-index-' + _mode, mode=_mode, abort=False, issues=('525',))(import_review)


# #757 PR 0: two user phrases with equal counts whose user-table order
# differs from token order. add_index keeps the user table ordered by
# pinyin_exact_compare2 keys, then token (074a2219 chewing_large_table2.h:
# 208-231, pinyin_phrase3.h:33-63), so 靐靐 (da2'da2, added second) comes
# before 龘龘 (da3'da3, added first). A toneless parse matches both with
# equal unigram and pronunciation possibility: the n-best tie goes to the
# phrase the trellis walk meets first (phonetic_lookup.h:560-572).
def nbest_user_table_order(k):
    """Compare sentence ordering for two equally counted user phrases inserted opposite to reading order.

    Assert unique increasing library-seven tokens for 龘龘 and 靐靐. Parse
    toneless dada; record three pinyin sentence ranks or one zhuyin sentence.
    Zhuyin options retain USE_TONE without FORCE_TONE."""
    phrases = (('龘龘', "da3'da3", 'ㄉㄚˇ ㄉㄚˇ'), ('靐靐', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ'))
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    assert it
    add = k.fn('iterator_add_phrase', B, P, S, S, I)
    added = [add(it, text.encode(), (pinyin if k.mode == 'pinyin' else zhuyin).encode(), 100000)
             for text, pinyin, zhuyin in phrases]
    k.fn('end_add_phrases', None, P)(it)
    assert all(added), added
    tokens = [[token for token in tokens_of(k, text) if token >> 24 == 7] for text, _, _ in phrases]
    assert all(len(found) == 1 for found in tokens), tokens
    assert tokens[0][0] < tokens[1][0], 'the first phrase added holds the lower token'
    out = {'tokens': [hex(found[0]) for found in tokens]}
    if k.mode == 'zhuyin':
        # USE_TONE without FORCE_TONE, so the zhuyin facade takes a toneless key.
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    inst = k.alloc()
    out['parsed'] = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'dada')
    out['guess'] = k.fn('guess_sentence', B, P)(inst)
    if k.mode == 'pinyin':
        out['sentences'] = [sentence_out(k, inst, index) for index in range(3)]
    else:
        out['sentence'] = zhuyin_sentence_out(k, inst)
    return out


# Controls: the parent build already met the user items in this order, so
# PR 0 preserves the output here.
for _mode in ('pinyin', 'zhuyin'):
    case('nbest-user-table-order-' + _mode, mode=_mode, control=True, issues=('757',))(nbest_user_table_order)


def user_dir_listing(scratch, before):
    """Summarize and remove newly created user directories beneath scratch.

    before is the set of scratch entries from before the worker. Scan new
    user-* directories in sorted order, flattening by filename so later entries
    overwrite earlier names. Render user.conf with newlines replaced by '|';
    record byte size and SHA-256 for regular .tmp files and non-user_ .bin/.dbin
    files, size alone for other files, and None for non-files. Remove each
    scanned directory with cleanup errors ignored; read/stat errors propagate."""
    listing = {}
    for entry in sorted(set(os.listdir(scratch)) - before):
        path = os.path.join(scratch, entry)
        if not entry.startswith('user-') or not os.path.isdir(path):
            continue
        for name in sorted(os.listdir(path)):
            file = os.path.join(path, name)
            if name == 'user.conf':
                listing[name] = Path(file).read_text().replace('\n', '|')
            elif os.path.isfile(file) and (name.endswith('.tmp') or
                                           (not name.startswith('user_') and
                                            name.endswith(('.bin', '.dbin')))):
                # Chunk logs and their partial `.tmp` siblings are compared
                # byte-for-byte; a replaced system chunk makes those bytes
                # the save's observable result.
                listing[name] = [os.path.getsize(file),
                                 hashlib.sha256(Path(file).read_bytes()).hexdigest()]
            elif os.path.isfile(file):
                # The DBM containers are value-level in this implementation;
                # the pin and subject agree on their size and records, not
                # their backend byte layout.
                listing[name] = [os.path.getsize(file)]
            else:
                listing[name] = None
        shutil.rmtree(path, ignore_errors=True)
    return listing


# Axis J targeted reachability controls, at libpinyin 074a2219:
# pinyin.cpp:2515-2520 calls ForwardPhoneticConstraints::diff_result when
# choosing a displayed NBEST row. No forged candidate or context is needed.
@case('j-choose-nbest-diff-result', control=True)
def _(k):
    """Choose a displayed NBEST row and record its rank output and signed selection result.

    Assert a row exists and initialize rank to 0xff without asserting getter success."""
    inst = guessed(k)
    candidate = row_of(k, inst, 1)
    assert candidate, 'no displayed NBEST candidate'
    rank = C.c_ubyte(0xff)
    ranked = k.fn('get_candidate_nbest_index', B, P, P, C.POINTER(C.c_ubyte))(
        inst, candidate, C.byref(rank))
    chosen = k.fn('choose_candidate', I, P, Z, P)(inst, 0, candidate)
    return {'ranked': ranked, 'rank': rank.value, 'chosen': chosen}


# Scheme HSU (2) constructs ZhuyinDiscreteParser2; Dachen CP26 (9)
# constructs ZhuyinDaChenCP26Parser2. Batch parse goes through their parse
# virtuals (pinyin.cpp:1595 / zhuyin.cpp:1068), rather than one-key parsing.
def j_multi_chewing(scheme):
    """Build a multi-key chewing parse case for the selected facade scheme number."""
    def probe(k):
        """Select the facade's scheme, parse abc and record consumed and reported input lengths.

        Record selection failure rather than asserting it; lengths are byte counts."""
        setter = 'set_zhuyin_scheme' if k.mode == 'pinyin' else 'set_chewing_scheme'
        selected = k.fn(setter, B, P, I)(k.ctx, scheme)
        parsed = k.fn('parse_more_chewings', Z, P, S)(k.inst, b'abc')
        length = k.fn('get_parsed_input_length', Z, P)(k.inst)
        return {'scheme': scheme, 'selected': selected, 'parsed': parsed, 'length': length}
    return probe


for _j_mode in ('pinyin', 'zhuyin'):
    for _j_scheme in (2, 9):
        case(f'j-chewing-multi-key-{_j_mode}-{_j_scheme}', mode=_j_mode,
             control=True)(j_multi_chewing(_j_scheme))


# Phase timings of the workers in completion order (`T` lines plus the
# driver's own wall and teardown), kept apart from the observations so a
# captured observation is the same bytes from run to run.
TIMINGS = []
WORKER_STATE = threading.local()
# Set once by the controller before starting a pool; never mutate the
# process environment from a case thread. Keep the slot descriptor alive
# in workers, including when the controller itself is killed.
WORKER_ENV = {}
WORKER_FDS = ()


CANCELLED = False


class Cancelled(Exception):
    """The caller cancelled the driver; no case verdict is available."""


def check_cancelled():
    """Raise Cancelled when the controller's shared cancellation flag is set."""
    if CANCELLED:
        raise Cancelled('cancelled; worker group terminated and reaped')


@contextlib.contextmanager
def cancellation_handlers():
    """Temporarily convert SIGTERM, SIGINT and SIGHUP into a shared cancellation flag.

    Reset CANCELLED on entry and restore previous handlers on exit. Handling
    only marks cancellation; worker code must check it and reap processes.
    Signal installation requires Python's main thread."""
    global CANCELLED
    CANCELLED = False
    previous = {}

    def cancel(signum, frame):
        """Mark controller cancellation without performing process cleanup in the signal handler."""
        global CANCELLED
        CANCELLED = True

    # Python cannot handle SIGKILL of the driver.
    for signum in (signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
        previous[signum] = signal.signal(signum, cancel)
    try:
        yield
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)


def terminate_group(proc):
    """Terminate the worker's process group and reap its leader.

    Send SIGTERM, wait up to half a second for the leader, then send SIGKILL
    even if it exited. Ignore missing process groups; other errors propagate.
    proc must have been started as the group leader."""
    with contextlib.suppress(ProcessLookupError):
        os.killpg(proc.pid, signal.SIGTERM)
    try:
        proc.wait(timeout=0.5)
    except subprocess.TimeoutExpired:
        pass
    with contextlib.suppress(ProcessLookupError):
        os.killpg(proc.pid, signal.SIGKILL)
    proc.wait()


def run_worker(mode, so, data, name, scratch, extra_env=None, timeout=None, entry=None):
    """Run an isolated worker and return its normalized observation.

    Use the current interpreter with entry (script plus leading arguments) or
    WORKER, inherited environment plus WORKER_ENV/extra_env, scratch as TMPDIR,
    and WORKER_FDS. Check cancellation and the tkrzw guard before spawning a
    new process session. timeout is seconds from pre-spawn setup; expiry kills
    the group, drains pipes and raises TimeoutExpired with captured bytes.
    None disables the deadline.

    Use the last stdout JSON object, replace invalid stdout UTF-8, preserve
    invalid stderr bytes with surrogateescape, normalize scratch paths, and
    summarize/remove new user directories. Include measurement marker fields
    without interpreting them. On post-spawn exit close pipes and append phase,
    wall and teardown seconds to TIMINGS; errors before completion terminate
    and reap the process group. Malformed JSON and filesystem errors propagate."""
    env = dict(os.environ, **WORKER_ENV)
    env['TMPDIR'] = str(scratch)
    env.update(extra_env or {})
    before = set(os.listdir(scratch))
    spawned = time.monotonic()
    env['CONTRACT_DIFF_SPAWNED'] = repr(spawned)
    # `entry` is the script and its leading arguments; the default is read
    # per call so a patched WORKER takes effect.
    command = [sys.executable, *map(str, entry or (WORKER,)), mode, str(so), str(data), name]
    check_cancelled()
    check_tkrzw(env.get('CONTRACT_DIFF_CELL', CELL), 'contract-diff.py',
                env.get('OXPINYIN_RUNTIME_SIDE', 'subject'), so, env=env)
    proc = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            env=env, start_new_session=True, pass_fds=WORKER_FDS)
    stdout, stderr = b'', b''
    completed = False
    deadline = None if timeout is None else spawned + timeout
    try:
        while True:
            check_cancelled()
            remaining = None if deadline is None else deadline - time.monotonic()
            if remaining is not None and remaining <= 0:
                # Descendants can hold the output pipes after the leader dies.
                with contextlib.suppress(ProcessLookupError):
                    os.killpg(proc.pid, signal.SIGKILL)
                stdout, stderr = proc.communicate()
                completed = True
                check_cancelled()
                raise subprocess.TimeoutExpired(command, timeout, output=stdout, stderr=stderr)
            try:
                stdout, stderr = proc.communicate(timeout=min(0.1, remaining) if remaining is not None else 0.1)
                check_cancelled()
                break
            except subprocess.TimeoutExpired as error:
                stdout, stderr = error.output or b'', error.stderr or b''
        stdout_text = stdout.decode('utf-8', 'replace')
        # Keep invalid UTF-8 apart from a real U+FFFD: the pin writes path bytes.
        stderr_text = stderr.decode('utf-8', 'surrogateescape')
        # Replace the generated root before normalizing user/sys names.
        stderr_text = stderr_text.replace(str(scratch), 'SCRATCH')
        lines = [json.loads(line) for line in stdout_text.splitlines() if line.startswith('{')]
        observation = dict(exit=proc.returncode, result=lines[-1] if lines else None,
                    userfiles=user_dir_listing(scratch, before),
                    measurement_roots=[line.removeprefix('J-ROOT ') for line in stdout_text.splitlines() if line.startswith('J-ROOT ')],
                    measurement_abort_flush=('J-GCOV-ABORT-BEGIN' in stdout_text.splitlines()
                                             and 'J-GCOV-ABORT-END' in stdout_text.splitlines()),
                    stderr_lines=len(stderr_text.splitlines()),
                    stderr=re.sub(r'(user|sys)-(\udcff)?[A-Za-z0-9_]+', r'\1-\2X', stderr_text))
        check_cancelled()
        completed = True
        return observation
    finally:
        if not completed:
            # This also covers KeyboardInterrupt and exceptions while waiting.
            terminate_group(proc)
        proc.stdout.close()
        proc.stderr.close()
        phases = [json.loads(line[2:]) for line in stdout.decode('utf-8', 'replace').splitlines()
                  if line.startswith('T {')]
        timing = dict(case=name, so=str(so), **(phases[-1] if phases else {}))
        timing['wall'] = time.monotonic() - spawned
        timing['teardown'] = timing['wall'] - sum(timing.get(k, 0.0) for k in ('startup', 'load', 'fixture', 'execute'))
        side = getattr(WORKER_STATE, 'side', None)
        if side is not None:
            timing['side'] = side
        TIMINGS.append(timing)


def run_case(spec, pin_so, subject_so, data, name, scratch, timeout):
    """Run pin then subject under one timeout in seconds and return two observations.

    Pass fatal-criticals when requested and track the worker side per thread.
    A timeout becomes TIMEOUT and prevents later ordinary execution; an absent
    second result becomes NOT-RUN. Other errors propagate.

    For the two issue1114 collision cases, retain separate profiles, compare
    saved bigram bytes with only the timestamp normalized, and cross-reload
    both donor profiles in fresh workers under the same deadline. Mismatch or
    reload failure updates the corresponding observation; successful reloads
    add filtered cross_reload results. Profile cleanup belongs to the caller."""
    deadline = time.monotonic() + timeout
    results = []
    fatal_env = {'G_DEBUG': 'fatal-criticals'} if spec['fatal'] else None
    handoff = name in ('issue1114-collision-load-save-pinyin', 'issue1114-collision-load-save-zhuyin')
    profiles = {}
    if handoff:
        for side in ('pin', 'subject'):
            profiles[side] = tempfile.mkdtemp(prefix='handoff-' + side + '-', dir=scratch)
    for side, so in (('pin', pin_so), ('subject', subject_so)):
        previous_side = getattr(WORKER_STATE, 'side', None)
        WORKER_STATE.side = side
        remaining = deadline - time.monotonic()
        try:
            if remaining <= 0:
                raise subprocess.TimeoutExpired(str(so), timeout)
            extra_env = (dict(fatal_env or {}, OXPINYIN_RUNTIME_SIDE=side)
                         if os.environ.get('CONTRACT_DIFF_CELL', CELL) == 'tkrzw' else fatal_env)
            if handoff:
                extra_env = dict(extra_env or {}, CONTRACT_1114_PROFILE=profiles[side])
            result = run_worker(spec['mode'], so.resolve(), data, name, scratch,
                                extra_env=extra_env, timeout=remaining)
        except subprocess.TimeoutExpired as error:
            results.append(dict(exit='TIMEOUT', result=None, userfiles=[],
                                stderr_lines=len((error.stderr or b'').splitlines()),
                                stderr=(error.stderr or b'').decode('utf-8', 'surrogateescape')))
            break
        finally:
            WORKER_STATE.side = previous_side
        results.append(result)
    if len(results) == 1:
        results.append(dict(exit='NOT-RUN', result=None, userfiles=[], stderr_lines=0, stderr=''))
    if handoff and all(result['exit'] == 0 for result in results):
        # Direct byte equality is the gate, not digest equality. Bucket
        # pointers and record links stay exact along with every payload byte.
        raw = [Path(profiles[side], 'user_bigram.db').read_bytes() for side in ('pin', 'subject')]
        if tkrzw_saved_bigram_bytes(raw[0]) != tkrzw_saved_bigram_bytes(raw[1]):
            results[0].update(exit=1, result={'error': 'saved bytes differ outside META_OFFSET_TIMESTAMP'})
            return results
        # Fresh workers isolate the two libraries. Run both donor -> receiver
        # directions under the same case deadline, retaining donor files.
        for index, (donor, receiver, so) in enumerate((('pin', 'subject', subject_so),
                                                     ('subject', 'pin', pin_so))):
            previous_side = getattr(WORKER_STATE, 'side', None)
            WORKER_STATE.side = receiver
            try:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise subprocess.TimeoutExpired(str(so), timeout)
                reload = run_worker(spec['mode'], so.resolve(), data, name, scratch,
                                    extra_env=dict(OXPINYIN_RUNTIME_SIDE=receiver,
                                                   CONTRACT_1114_PROFILE=profiles[donor],
                                                   CONTRACT_1114_RELOAD='1'), timeout=remaining)
            except subprocess.TimeoutExpired:
                results[index].update(exit='TIMEOUT', result=None)
                break
            finally:
                WORKER_STATE.side = previous_side
            if reload['exit'] != 0 or reload['result'] is None:
                results[index].update(exit=reload['exit'] or 1, result=None,
                                      stderr=reload['stderr'], stderr_lines=reload['stderr_lines'])
            else:
                results[index]['result']['cross_reload'] = observed(reload['result'])
    return results


def positive_jobs(value):
    """Convert value with int and reject nonpositive worker counts.

    Nonpositive values raise ArgumentTypeError; conversion failures propagate
    from int rather than being wrapped here."""
    number = int(value)
    if number <= 0:
        raise argparse.ArgumentTypeError('--jobs must be a positive integer')
    return number


def ordinary_jobs(parser, requested):
    """Choose ordinary-run concurrency from the explicit value, environment or serial default.

    Return requested unchanged when supplied. Otherwise validate
    OXPINYIN_CONTRACT_JOBS through tools.dev.host and report invalid settings
    with parser.error; importing the validator may prepend the repository to
    sys.path. Return one when the environment setting is absent."""
    if requested is not None:
        return requested
    value = os.environ.get('OXPINYIN_CONTRACT_JOBS')
    if value is None:
        return 1
    root = str(Path(__file__).resolve().parents[2])
    if root not in sys.path:
        sys.path.insert(0, root)
    from tools.dev import host
    try:
        host.validate_settings({'OXPINYIN_CONTRACT_JOBS': value})
    except ValueError as error:
        parser.error(str(error))
    return int(value)


@contextlib.contextmanager
def case_jobs(requested):
    """Admit a case pool and temporarily publish its worker environment and inherited descriptors.

    Direct serial work yields one without admission. Parallel or inherited-slot
    work acquires HeavyAdmission, checks cancellation while waiting, prints
    requested/admitted counts and yields the effective count. Restore WORKER_ENV
    and WORKER_FDS and close admission on exit. The caller must join case threads
    and reap worker groups before leaving this context."""
    global WORKER_ENV, WORKER_FDS
    if requested == 1 and 'OXPINYIN_HEAVY_SLOT_HELD' not in os.environ:
        yield 1
        return
    root = str(Path(__file__).resolve().parents[2])
    if root not in sys.path:
        sys.path.insert(0, root)
    from tools.dev import slot
    env = dict(os.environ, OXPINYIN_CONTRACT_JOBS=str(requested))
    admission = slot.HeavyAdmission('contract-diff case pool', env, runner=True)
    previous_env, previous_fds = WORKER_ENV, WORKER_FDS
    try:
        # Keep the driver's signal handlers installed. They cancel both an
        # admission wait and active case workers, without nested handlers
        # hiding the driver's cancellation flag.
        while not admission.try_acquire():
            check_cancelled()
            time.sleep(.05)
        check_cancelled()
        WORKER_ENV = admission.env
        WORKER_FDS = admission.slot.pass_fds()
        admission.slot.handoff()
        effective = int(admission.env['OXPINYIN_CONTRACT_JOBS'])
        print(f'contract case workers: requested {requested}, admitted {effective}',
              file=sys.stderr, flush=True)
        yield effective
    finally:
        # The caller joins all case threads and reaps their process groups
        # before leaving this context and releasing its reservation.
        WORKER_ENV, WORKER_FDS = previous_env, previous_fds
        admission.close()


PLATFORM_SKIP = object()  # a case registered away from this host's platform


@contextlib.contextmanager
def case_results(args, names, data, scratch, jobs):
    """Yield case observations in selection order, using isolated scratch directories.

    Serial execution is a lazy map; parallel execution schedules a thread pool.
    Each item is (name, result), with PLATFORM_SKIP for excluded platforms and
    None for excluded cells. On a parallel-body exception mark cancellation;
    always wait for pool shutdown and cancel pending futures before returning."""
    global CANCELLED
    lib = args.prefix / 'lib'
    def observe(name):
        """Check case eligibility and run both libraries within a temporary per-case directory.

        Honor run_platform_excluded, assert zhuyin library availability when needed,
        and remove the isolated directory on exit. Propagate run_case failures."""
        check_cancelled()
        spec = CASES[name]
        if platform_skips(spec) and not getattr(args, 'run_platform_excluded', False):
            return name, PLATFORM_SKIP
        if args.cell not in spec['cells']:
            return name, None
        if spec['mode'] == 'zhuyin':
            assert args.zhuyin_so and args.zhuyin_so.is_file() and (lib / 'libzhuyin.so').is_file(), \
                'zhuyin case %s needs --zhuyin-so and the pin libzhuyin' % name
            pin_so, subject_so = lib / 'libzhuyin.so', args.zhuyin_so
        else:
            pin_so, subject_so = lib / 'libpinyin.so', args.pinyin_so
        # user_dir_listing removes profiles it discovers. Sharing this directory
        # across active cases would let one worker remove another's live files.
        with tempfile.TemporaryDirectory(prefix='case-', dir=scratch) as isolated:
            return name, run_case(spec, pin_so, subject_so, data, name, isolated, args.case_timeout)
    if jobs == 1:
        yield map(observe, names)
        return
    from concurrent.futures import ThreadPoolExecutor
    pool = ThreadPoolExecutor(max_workers=jobs)
    try:
        # map schedules the pool concurrently but yields in input order. Only
        # the controller writes observations, verdicts and diagnostics.
        yield pool.map(observe, names)
    except BaseException:
        CANCELLED = True
        raise
    finally:
        pool.shutdown(wait=True, cancel_futures=True)


# #640 leading separators, #698 retained before-cursor windows, #681 zero keys.
# Calls reuse the existing isolated worker and distinguish NULL from untouched.
def borrowed_text(pointer):
    """Decode a borrowed C string without freeing it, preserving untouched and NULL outputs.

    Return 'untouched' for the sentinel and None for NULL; invalid UTF-8 raises."""
    if pointer.value==UNTOUCHED:
        return 'untouched'
    return C.string_at(pointer.value).decode() if pointer.value else None


def owned_text(k,pointer):
    """Decode/free an owned GLib string unless untouched, returning None for NULL.

    Delegate decoding with UTF-8 replacement and release to Kit.text."""
    return 'untouched' if pointer.value==UNTOUCHED else k.text(pointer.value)


def separator_case(text, op, off, options=0x18a):
    """Build a parse-and-query case for a full-pinyin string and byte offset.

    options is passed to set_options. op selects parse, sentence, candidates,
    before, left, right, offset, character, rest or render; other values query
    a key. The returned callable records output sentinels and borrowed data."""

    def probe(k):
        """Parse with the selected options and collect the requested operation's outputs.

        Guess a sentence unless op is parse. Preserve numeric and pointer sentinels,
        record candidate type/text/rank where available, and copy key/rest bytes.
        Render operations decode/free owned strings; candidate strings are borrowed.
        Character queries choose 阿, 你好 or 你 from the input. No native objects
        are finalized here."""
        inst = k.inst
        k.fn('set_options', B, P, U)(k.ctx, options)
        parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text.encode())
        out = {'parse_return_consumed': parsed, 'get_parsed_input_length': k.fn('get_parsed_input_length', Z, P)(inst)}
        if op == 'parse':
            return out
        guessed = k.fn('guess_sentence', B, P)(inst)
        out['guess_sentence'] = guessed
        if op == 'sentence':
            ptr = P(UNTOUCHED)
            if k.mode == 'pinyin':
                ret = k.fn('get_sentence', B, P, C.c_ubyte, C.POINTER(P))(inst, 0, C.byref(ptr))
            else:
                ret = k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(ptr))
            out.update(ret=ret, sentence='untouched' if ptr.value == UNTOUCHED else k.text(ptr.value))
            return out
        if op in ['candidates', 'before']:
            if k.mode == 'pinyin':
                ret = k.fn('guess_candidates', B, P, Z, U)(inst, off, 0x1e)
            else:
                ret = k.fn('guess_candidates_before_cursor' if op == 'before' else 'guess_candidates_after_cursor', B, P, Z)(inst, off)
            n = U(UNTOUCHED)
            got = k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(n))
            out.update(ret=ret, get_n=got, n=n.value)
            rows = []
            if got:
                for index in range(n.value):
                    candidate, kind, string = (P(UNTOUCHED), I(UNTOUCHED), P(UNTOUCHED))
                    fetched = k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(candidate))
                    typed = k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, candidate, C.byref(kind))
                    rendered = k.fn('get_candidate_string', B, P, P, C.POINTER(P))(inst, candidate, C.byref(string))
                    row = [fetched, typed, kind.value, rendered, borrowed_text(string)]
                    if k.mode == 'pinyin' and kind.value == 1:
                        nbest = C.c_ubyte(0xab)
                        row.extend([k.fn('get_candidate_nbest_index', B, P, P, C.POINTER(C.c_ubyte))(inst, candidate, C.byref(nbest)), nbest.value])
                    rows.append(row)
            out['rows'] = rows
            return out
        outval = Z(UNTOUCHED)
        if op in ['left', 'right', 'offset']:
            name = 'get_' + ('' + k.mode + '_offset' if op == 'offset' else op + '_' + k.mode + '_offset')
            ret = k.fn(name, B, P, Z, C.POINTER(Z))(inst, off, C.byref(outval))
            out.update(ret=ret, out=outval.value, untouched_output=outval.value == UNTOUCHED if not ret else True)
            return out
        if op == 'character':
            phrase = '阿' if text == "'a" else '你好' if 'hao' in text else '你'
            ret = k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))(inst, phrase.encode(), off, C.byref(outval))
            out.update(ret=ret, out=outval.value, phrase=phrase, untouched_output=outval.value == UNTOUCHED if not ret else True)
            return out
        ptr = P(UNTOUCHED)
        suffix = 'key_rest' if op == 'rest' else 'key'
        ret = k.fn('get_' + k.mode + '_' + suffix, B, P, Z, C.POINTER(P))(inst, off, C.byref(ptr))
        out.update(ret=ret, pointer='untouched' if ptr.value == UNTOUCHED else 'null' if not ptr.value else 'key')
        if not ret:
            out['untouched_or_null_pointer'] = ptr.value in (None, UNTOUCHED)
        if ret and ptr.value:
            out['bytes'] = C.string_at(ptr.value, 4 if op == 'rest' else 2).hex()
            if op == 'rest':
                begin, end, length = (C.c_ushort(0xbeef), C.c_ushort(0xbeef), C.c_ushort(0xbeef))
                out['positions_ret'] = k.fn('get_' + k.mode + '_key_rest_positions', B, P, P, C.POINTER(C.c_ushort), C.POINTER(C.c_ushort))(inst, ptr, C.byref(begin), C.byref(end))
                out['positions'] = [begin.value, end.value]
                out['length_ret'] = k.fn('get_' + k.mode + '_key_rest_length', B, P, P, C.POINTER(C.c_ushort))(inst, ptr, C.byref(length))
                out['length'] = length.value
            if op == 'render':
                for name in ['pinyin', 'zhuyin', 'luoma_pinyin', 'secondary_zhuyin'] if k.mode == 'pinyin' else ['zhuyin', 'pinyin']:
                    string = P(UNTOUCHED)
                    rendered = k.fn('get_' + name + '_string', B, P, P, C.POINTER(P))(inst, ptr, C.byref(string))
                    out[name + '_string'] = [rendered, owned_text(k, string)]
                if k.mode == 'pinyin':
                    initial, final = (P(UNTOUCHED), P(UNTOUCHED))
                    rendered = k.fn('get_pinyin_strings', B, P, P, C.POINTER(P), C.POINTER(P))(inst, ptr, C.byref(initial), C.byref(final))
                    out['pinyin_strings'] = [rendered, owned_text(k, initial), owned_text(k, final)]
        return out
    return probe

SEPARATOR_INPUTS=("'ni","''ni","'nihao","'ni'hao","'ni'","'a")
SEPARATOR_ABORTS = {
    'zhuyin/3/before': (4,),
    'zhuyin/4/before': (4,),
    'pinyin/0/right': (3,),
    'pinyin/0/character': (0, 1, 2, 3),
    'pinyin/1/right': (4,),
    'pinyin/1/character': (0, 1, 2, 3, 4),
    'pinyin/2/right': (6,),
    'pinyin/2/character': (0, 1, 2, 3, 4, 5, 6),
    'pinyin/3/left': (4,),
    'pinyin/3/right': (4, 7),
    'pinyin/3/character': (0, 1, 2, 3, 4, 5, 6, 7),
    'pinyin/4/left': (4,),
    'pinyin/4/right': (3, 4),
    'pinyin/4/character': (0, 1, 2, 3, 4),
    'pinyin/5/right': (2,),
    'pinyin/5/character': (0, 1, 2),
    'zhuyin/0/right': (3,),
    'zhuyin/0/character': (0, 1, 2, 3),
    'zhuyin/1/right': (4,),
    'zhuyin/1/character': (0, 1, 2, 3, 4),
    'zhuyin/2/right': (6,),
    'zhuyin/2/character': (0, 1, 2, 3, 4, 5, 6),
    'zhuyin/3/candidates': (4,),
    'zhuyin/3/left': (4, 7),
    'zhuyin/3/right': (4, 7),
    'zhuyin/3/key': (4,),
    'zhuyin/3/rest': (4,),
    'zhuyin/3/character': (0, 1, 2, 3, 4, 5, 6, 7),
    'zhuyin/4/candidates': (4,),
    'zhuyin/4/left': (4,),
    'zhuyin/4/right': (3, 4),
    'zhuyin/4/character': (0, 1, 2, 3, 4),
    'zhuyin/5/right': (2,),
    'zhuyin/5/character': (0, 1, 2),
}
SEPARATOR_CONTROLS = {
    'pinyin/0/parse': (0,),
    'pinyin/0/offset': (0, 1, 2, 3),
    'pinyin/0/left': (0, 1, 2, 3),
    'pinyin/0/right': (0, 1, 2, 3),
    'pinyin/0/key': (1, 2, 3),
    'pinyin/0/rest': (1, 2, 3),
    'pinyin/0/character': (0, 1, 2, 3),
    'pinyin/1/parse': (0,),
    'pinyin/1/offset': (0, 1, 2, 3, 4),
    'pinyin/1/left': (0, 1, 2, 3, 4),
    'pinyin/1/right': (0, 1, 2, 3, 4),
    'pinyin/1/key': (2, 3, 4),
    'pinyin/1/rest': (2, 3, 4),
    'pinyin/1/character': (0, 1, 2, 3, 4),
    'pinyin/2/parse': (0,),
    'pinyin/2/offset': (0, 1, 2, 3, 4, 5, 6),
    'pinyin/2/left': (0, 1, 2, 3, 4, 5, 6),
    'pinyin/2/right': (0, 1, 2, 3, 4, 5, 6),
    'pinyin/2/key': (1, 2, 3, 4, 5, 6),
    'pinyin/2/rest': (1, 2, 3, 4, 5, 6),
    'pinyin/2/character': (0, 1, 2, 3, 4, 5, 6),
    'pinyin/3/parse': (0,),
    'pinyin/3/offset': (0, 1, 2, 3, 4, 5, 6, 7),
    'pinyin/3/left': (0, 1, 2, 3, 4, 5, 6, 7),
    'pinyin/3/right': (0, 1, 2, 3, 4, 5, 6, 7),
    'pinyin/3/key': (1, 2, 3, 4, 5, 6, 7),
    'pinyin/3/rest': (1, 2, 3, 4, 5, 6, 7),
    'pinyin/3/character': (0, 1, 2, 3, 4, 5, 6, 7),
    'pinyin/4/parse': (0,),
    'pinyin/4/offset': (0, 1, 2, 3, 4),
    'pinyin/4/left': (0, 1, 2, 3, 4),
    'pinyin/4/right': (0, 1, 2, 3, 4),
    'pinyin/4/key': (1, 2, 4),
    'pinyin/4/rest': (1, 2, 4),
    'pinyin/4/character': (0, 1, 2, 3, 4),
    'pinyin/5/parse': (0,),
    'pinyin/5/offset': (0, 1, 2),
    'pinyin/5/left': (0, 1, 2),
    'pinyin/5/right': (0, 1, 2),
    'pinyin/5/key': (1, 2),
    'pinyin/5/rest': (1, 2),
    'pinyin/5/character': (0, 1, 2),
    'zhuyin/0/parse': (0,),
    'zhuyin/0/offset': (0, 1, 2, 3),
    'zhuyin/0/left': (0, 1, 2, 3),
    'zhuyin/0/right': (0, 1, 2, 3),
    'zhuyin/0/key': (1, 2, 3),
    'zhuyin/0/rest': (1, 2, 3),
    'zhuyin/0/character': (0, 1, 2),
    'zhuyin/1/parse': (0,),
    'zhuyin/1/offset': (0, 1, 2, 3, 4),
    'zhuyin/1/left': (0, 1, 2, 3, 4),
    'zhuyin/1/right': (0, 1, 2, 3, 4),
    'zhuyin/1/key': (2, 3, 4),
    'zhuyin/1/rest': (2, 3, 4),
    'zhuyin/1/character': (0, 1, 2, 3),
    'zhuyin/2/parse': (0,),
    'zhuyin/2/offset': (0, 1, 2, 3, 4, 5, 6),
    'zhuyin/2/left': (0, 1, 2, 3, 4, 5, 6),
    'zhuyin/2/right': (0, 1, 2, 3, 4, 5, 6),
    'zhuyin/2/key': (1, 2, 3, 4, 5, 6),
    'zhuyin/2/rest': (1, 2, 3, 4, 5, 6),
    'zhuyin/2/character': (0, 1, 2, 3, 4, 5),
    'zhuyin/3/parse': (0,),
    'zhuyin/3/offset': (0, 1, 2, 3, 4, 5, 6, 7),
    'zhuyin/3/left': (0, 1, 2, 3, 4, 5, 6),
    'zhuyin/3/right': (0, 1, 2, 3, 4, 5, 6, 7),
    'zhuyin/3/key': (1, 2, 5, 6, 7),
    'zhuyin/3/rest': (1, 2, 5, 6, 7),
    'zhuyin/3/character': (0, 1, 2, 3, 4, 5, 6),
    'zhuyin/4/parse': (0,),
    'zhuyin/4/offset': (0, 1, 2, 3, 4),
    'zhuyin/4/left': (0, 1, 2, 3, 4),
    'zhuyin/4/right': (0, 1, 2, 3, 4),
    'zhuyin/4/key': (1, 2, 4),
    'zhuyin/4/rest': (1, 2, 4),
    'zhuyin/4/character': (0, 1, 2, 3),
    'zhuyin/5/parse': (0,),
    'zhuyin/5/offset': (0, 1, 2),
    'zhuyin/5/left': (0, 1, 2),
    'zhuyin/5/right': (0, 1, 2),
    'zhuyin/5/key': (1, 2),
    'zhuyin/5/rest': (1, 2),
    'zhuyin/5/character': (0, 1),
}
for mode in ('pinyin','zhuyin'):
    for index,text in enumerate(SEPARATOR_INPUTS):
        operations=['parse','sentence','candidates','offset','left','right','key','rest','character']
        if mode=='zhuyin':
            operations.append('before')
        for op in operations:
            for offset in ([0] if op in ('parse','sentence') else range(len(text)+1)):
                identity=f'{mode}/{index}/{op}/{offset}'
                kwargs={'mode':mode,'control':offset in SEPARATOR_CONTROLS.get(f'{mode}/{index}/{op}', ())}
                if offset in SEPARATOR_ABORTS.get(f"{mode}/{index}/{op}", ()):
                    kwargs['abort']=False
                case('separator/'+identity,**kwargs, issues=('640',))(separator_case(text,op,offset))
BEFORE_CURSOR_INPUTS=('ni','hao','wo','shi','zhong','guo','xian','tian',
    'nihao','zhongguo','beijing','shanghai','renmin','nihaoshijie',
    'zhonghuarenmin','beijingdaxue','shijieheping','nih','zhongg',
    'beijin','nihaoshij','ni3','ni3hao3','zhong1guo2')
BEFORE_CURSOR_CONTROLS=(('ni', 0), ('ni', 2), ('hao', 0), ('hao', 3), ('wo', 0), ('wo', 2), ('shi', 0), ('shi', 3), ('zhong', 0), ('zhong', 5), ('guo', 0), ('guo', 3), ('xian', 0), ('tian', 0), ('nihao', 0), ('nihao', 2), ('nihao', 5), ('zhongguo', 0), ('zhongguo', 5), ('zhongguo', 8), ('beijing', 0), ('beijing', 3), ('beijing', 7), ('shanghai', 0), ('shanghai', 5), ('shanghai', 8), ('renmin', 0), ('renmin', 3), ('renmin', 6), ('nihaoshijie', 0), ('nihaoshijie', 2), ('nihaoshijie', 5), ('nihaoshijie', 8), ('zhonghuarenmin', 0), ('zhonghuarenmin', 5), ('zhonghuarenmin', 8), ('zhonghuarenmin', 11), ('zhonghuarenmin', 14), ('beijingdaxue', 0), ('beijingdaxue', 3), ('beijingdaxue', 7), ('beijingdaxue', 9), ('beijingdaxue', 12), ('shijieheping', 0), ('shijieheping', 3), ('shijieheping', 8), ('shijieheping', 12), ('nih', 0), ('nih', 2), ('nih', 3), ('zhongg', 0), ('zhongg', 5), ('zhongg', 6), ('beijin', 0), ('beijin', 3), ('beijin', 6), ('nihaoshij', 0), ('nihaoshij', 2), ('nihaoshij', 5), ('nihaoshij', 8), ('nihaoshij', 9), ('ni3', 0), ('ni3', 3), ('ni3hao3', 0), ('ni3hao3', 3), ('ni3hao3', 7), ('zhong1guo2', 0), ('zhong1guo2', 6), ('zhong1guo2', 10))
for text in BEFORE_CURSOR_INPUTS:
    options=0x1aa if any(char.isdigit() for char in text) else 0x18a
    for offset in range(len(text)+1):
        case(f'before-cursor/{text}/{offset}',mode='zhuyin',
             control=(text,offset) in BEFORE_CURSOR_CONTROLS, issues=('698',))(separator_case(text,'before',offset,options))
        for op in ('candidates','key','rest'):
            case(f'separator-free-control/{text}/{op}/{offset}',control=True, issues=('640', '698'))(
                separator_case(text,op,offset,options))
    case(f'separator-free-control/{text}/sentence',control=True, issues=('640', '698'))(
        separator_case(text,'sentence',0,options))
for mode in ('pinyin','zhuyin'):
    for text in ("ni'","ni'hao","ni''hao","ni'hao'"):
        for op in ('key','rest'):
            for offset in range(len(text)+1):
                previous_zero=offset>0 and text[offset-1]=="'"
                is_zero=offset<len(text) and text[offset]=="'"
                tail_zero=is_zero and all(char=="'" for char in text[offset:])
                kwargs={'mode':mode,'control':not (tail_zero if mode=='pinyin' else is_zero) and not (mode=='zhuyin' and previous_zero and offset<len(text))}
                if mode=='zhuyin' and previous_zero and offset<len(text):
                    kwargs['abort']=False
                case(f'zero-key/{mode}/{text}/{op}/{offset}',**kwargs, issues=('681',))(separator_case(text,op,offset))
for mode,text,offset in (('pinyin',"xi'",2),('pinyin',"xi''",2),
                         ('pinyin',"xi''",3),('zhuyin',"xi1'an1",3),
                         ('zhuyin',"xi1'",3)):
    for op in ('key','rest','render'):
        case(f'allocator-zero/{mode}/{text}/{op}/{offset}',mode=mode, issues=('681',))(
            separator_case(text,op,offset,0x1aa))
case("allocator-zero/zhuyin/xi1'an1/key/4",mode='zhuyin',abort=False, issues=('681',))(
    separator_case("xi1'an1",'key',4,0x1aa))


# #681: the eight "unexecuted" string-getter lines of the allocator registers
# (E3-E10). Each getter's false branch is `0 == key->get_table_index()`
# (pinyin.cpp:2707-2762, zhuyin.cpp:1732-1760); a conforming consumer reaches
# it with the zero key the key getter answers at a separator column. The
# case records the return and the state of every out-parameter (`untouched`
# is the 0xABCDEF sentinel, `null` a NULL write); the control calls the same
# getter on a real key.
def alloc_register_case(mode, text, offset, which):
    """Build a key-to-string getter case at the selected full-pinyin byte offset.

    mode names the key accessor; which='strings' requests separate pinyin
    initial/final strings, otherwise it names one rendering accessor."""
    def probe(k):
        """Parse and guess, then record raw key bytes and owned string outputs.

        Return early when the key getter fails or writes NULL. Use option word
        0x1aa and preserve untouched output sentinels; free returned strings."""
        inst = k.inst
        k.fn('set_options', B, P, U)(k.ctx, 0x1aa)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, text.encode())
        out = {'guess_sentence': k.fn('guess_sentence', B, P)(inst)}
        key = P(UNTOUCHED)
        out['key_ret'] = k.fn('get_' + mode + '_key', B, P, Z, C.POINTER(P))(inst, offset, C.byref(key))
        if not out['key_ret'] or not key.value:
            return out
        out['key_bytes'] = C.string_at(key.value, 2).hex()
        if which == 'strings':
            initial, final = (P(UNTOUCHED), P(UNTOUCHED))
            out['ret'] = k.fn('get_pinyin_strings', B, P, P, C.POINTER(P), C.POINTER(P))(inst, key, C.byref(initial), C.byref(final))
            out['shengmu'], out['yunmu'] = owned_text(k, initial), owned_text(k, final)
        else:
            string = P(UNTOUCHED)
            out['ret'] = k.fn('get_' + which + '_string', B, P, P, C.POINTER(P))(inst, key, C.byref(string))
            out['out'] = owned_text(k, string)
        return out
    return probe


for _mode, _text, _zero, _real, _names in (
        ('pinyin', "xi'", 2, 0, ('pinyin', 'zhuyin', 'luoma_pinyin', 'secondary_zhuyin', 'strings')),
        ('zhuyin', "xi1'", 3, 0, ('zhuyin', 'pinyin'))):
    for _name in _names:
        case(f'alloc-register-{_mode}_get_{_name}-zero-key', mode=_mode, control=True, issues=('681',))(
            alloc_register_case(_mode, _text, _zero, _name))
        case(f'alloc-register-{_mode}_get_{_name}-real-key', mode=_mode, control=True, issues=('681',))(
            alloc_register_case(_mode, _text, _real, _name))



# #695, row 54: parse replaces the matrix even for empty input; every
# candidate guess searches the currently loaded libraries without a reparse.
def row54_empty(parse_name, text):
    """Build a case that guesses after the original, empty and restored byte input."""
    def probe(k):
        """Set tone options and record three parse/guess pairs on one instance.

        Use the supplied parse entry point and retain native failure flags."""
        k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        inst = k.inst
        parse = k.fn(parse_name, Z, P, S)
        guess = k.fn('guess_sentence', B, P)
        out = {'first_parse': parse(inst, text), 'first_guess': guess(inst)}
        out['empty_parse'] = parse(inst, b'')
        out['empty_guess'] = guess(inst)
        out['reparse'] = parse(inst, text)
        out['recovered_guess'] = guess(inst)
        return out
    return probe


def row54_unload(direction, reparse):
    """Build a candidate-list comparison around unloading library two.

    Optionally reparse nihao before collecting two post-unload lists. direction
    selects the zhuyin cursor side; pinyin always guesses at offset zero."""
    def probe(k):
        """Parse nihao, collect candidates, unload library two and collect two more lists.

        Use tone options and optional reparse; keep the cached context and instance live."""
        k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        inst = k.inst
        parse = k.fn('parse_more_full_pinyins', Z, P, S)
        out = {'parse': parse(inst, b'nihao')}
        def rows():
            """Guess and return flags, count and ordered candidate strings for the selected cursor side.

            Zhuyin before-cursor uses byte five, after-cursor byte zero; pinyin uses
            sort word 0x1e at zero. Assert each candidate/string fetch and decode UTF-8
            strictly without freeing borrowed strings."""
            if k.mode == 'pinyin':
                ret = k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1e)
            else:
                ret = k.fn('guess_candidates_' + direction + '_cursor', B, P, Z)(
                    inst, 0 if direction == 'after' else 5)
            count = U(UNTOUCHED)
            counted = k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            strings = []
            for index in range(count.value):
                candidate, text = P(), S()
                assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(candidate))
                assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, candidate, C.byref(text))
                strings.append(text.value.decode())
            return [ret, counted, count.value, strings]
        out['before'] = rows()
        out['unload'] = k.fn('unload_phrase_library', B, P, C.c_ubyte)(k.ctx, 2)
        if reparse:
            out['reparse'] = parse(inst, b'nihao')
        out['after'] = rows()
        out['repeated'] = rows()
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _parser, _text in (('full_pinyins', b'nihao'), ('chewings', b'su3cl3')):
        case(f'row54-empty-{_mode}-{_parser}', mode=_mode, issues=('695', 'row:54'))(
            row54_empty('parse_more_' + _parser, _text))
    for _reparse in (False, True):
        case(f'row54-unload-{_mode}' + ('-reparse' if _reparse else ''),
             mode=_mode, control=_reparse, issues=('695', 'row:54'))(row54_unload('after', _reparse))
case('row54-empty-pinyin-double', issues=('695', 'row:54'))(row54_empty('parse_more_double_pinyins', b'nihk'))
case('row54-unload-zhuyin-before', mode='zhuyin', control=True, issues=('695', 'row:54'))(row54_unload('before', False))



# PR #709 review: keep a non-first NORMAL choice through an empty parse
# until a sentence guess validates the constraints against that empty matrix.
def row54_forcing(sequence, restored=b'nihao'):
    """Build a constraint-retention case choosing non-first NORMAL 泥 before reparsing.

    sequence='no-empty' omits the empty parse; 'empty-guess' guesses after it;
    other values parse empty without guessing. restored is the final byte input."""
    def probe(k):
        """Choose displayed 泥, perform the requested empty-parse sequence and record the restored sentence.

        Assert candidate getters and that 泥 is a non-first NORMAL row. Use tone
        options and return initial, choice and restored parse/guess observations."""
        k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        inst = k.inst
        parse = k.fn('parse_more_full_pinyins', Z, P, S)
        guess = k.fn('guess_sentence', B, P)
        def sentence():
            """Return the current first sentence and flag, preserving untouched output and freeing returned text."""
            text = P(UNTOUCHED)
            if k.mode == 'pinyin':
                ret = k.fn('get_sentence', B, P, U, C.POINTER(P))(inst, 0, C.byref(text))
            else:
                ret = k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(text))
            return [ret, 'untouched' if text.value == UNTOUCHED else k.text(text.value)]
        out = {'parse': parse(inst, b'nihao'), 'initial_guess': guess(inst),
               'initial_sentence': sentence()}
        if k.mode == 'pinyin':
            out['candidates'] = k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1e)
        else:
            out['candidates'] = k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
        count = U()
        assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
        chosen = None
        for index in range(count.value):
            cand, text, kind = P(), S(), I()
            assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(cand))
            assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
            assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
            if kind.value == 2 and text.value == '泥'.encode():
                assert index > 0, 'the chosen NORMAL candidate must not be first'
                out['chosen_index'], out['chosen_text'] = index, text.value.decode()
                chosen = cand
                break
        assert chosen, 'missing non-first NORMAL candidate 泥'
        out['choose'] = k.fn('choose_candidate', I, P, Z, P)(inst, 0, chosen)
        if sequence != 'no-empty':
            out['empty_parse'] = parse(inst, b'')
            if sequence == 'empty-guess':
                out['empty_guess'] = guess(inst)
        out['restored_parse'] = parse(inst, restored)
        out['restored_guess'] = guess(inst)
        out['restored_sentence'] = sentence()
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _sequence in ('empty', 'empty-guess', 'no-empty'):
        case(f'row54-forcing-{_mode}-{_sequence}', mode=_mode,
             control=_sequence != 'empty', issues=('709', '695', 'row:54'))(row54_forcing(_sequence))


# #710, row 53 item 2: every zero-consumption parse leaves an empty
# matrix. get_nbest_match returns false before clearing sentence rows
# (074a2219, phonetic_lookup.h:743-748), including the prefix-seeded path.
def zero_consumption_sentence(parser, text, with_prefix):
    """Build a fresh-versus-stale sentence comparison after a zero-consumption parse.

    parser is the parse_more_ suffix; text is bytes. with_prefix chooses a
    sentence guess seeded by 你. The callable asserts zero consumption."""
    def probe(k):
        """Compare fresh instances with ones holding a nihao sentence before the zero-consumption parse.

        Set tone options, record parsing/guessing and sentence outputs, and leave
        both allocated instances live."""
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        out = {}
        for stale in (False, True):
            inst = k.alloc()
            def sentence():
                """Read the active instance's first sentence, preserving the sentinel and freeing owned text."""
                value = P(UNTOUCHED)
                if k.mode == 'pinyin':
                    ret = k.fn('get_sentence', B, P, U, C.POINTER(P))(inst, 0, C.byref(value))
                else:
                    ret = k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(value))
                return [ret, 'untouched' if value.value == UNTOUCHED else k.text(value.value)]
            result = {}
            if stale:
                result['initial_parse'] = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
                result['initial_guess'] = k.fn('guess_sentence', B, P)(inst)
                result['initial_sentence'] = sentence()
            result['parse'] = k.fn('parse_more_' + parser, Z, P, S)(inst, text)
            assert result['parse'] == 0, 'the case must leave an empty matrix'
            if with_prefix:
                result['guess'] = k.fn('guess_sentence_with_prefix', B, P, S)(inst, '你'.encode())
            else:
                result['guess'] = k.fn('guess_sentence', B, P)(inst)
            result['sentence'] = sentence()
            out['stale' if stale else 'fresh'] = result
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _parsers = ('full_pinyins', 'chewings', 'double_pinyins') if _mode == 'pinyin' else ('full_pinyins', 'chewings')
    for _parser in _parsers:
        _texts = (b'n', b'!', b'') if _parser == 'full_pinyins' else (b'!', b'')
        for _text in _texts:
            for _prefix in (False, True):
                _label = _text.decode() or 'empty'
                case(f'zero-consumption-{_mode}-{_parser}-{_label}' + ('-prefix' if _prefix else ''),
                     mode=_mode, control=_mode == 'pinyin', issues=('710', 'row:53'))(
                    zero_consumption_sentence(_parser, _text, _prefix))


# #710: parsing never touches m_constraints. At 074a2219,
# lookup/phonetic_lookup.cpp:120-160 first resizes to matrix.size(), then
# clears only ONESTEP spans ending >= size or whose pronunciation
# possibility is < FLT_EPSILON. A guess (or a choose) validates; a parse
# alone, including an empty or temporarily incompatible parse, does not.
# Compare complete sentence text, not just guess success or a prefix.
def changed_parse_constraints(initial, choices, replacement, sequence,
                              intermediate=None):
    """Build a sentence comparison across selected constraints and replacement parses.

    choices contains (byte offset, NORMAL-candidate text) pairs. sequence may
    guess after choosing, parse empty with or without guessing, or guess an
    optional intermediate input. Always guess the replacement, record its
    sentence, then clear each chosen offset."""
    def probe(k):
        """Choose the requested rows, perform the parse sequence and record full sentence outputs.

        Assert tone options and candidate lookup, encode input strings as UTF-8,
        and record native parse/guess/clear flags without asserting success."""
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        inst = k.inst
        parse = k.fn('parse_more_full_pinyins', Z, P, S)
        guess = k.fn('guess_sentence', B, P)

        def sentence():
            """Read and free the current first sentence, distinguishing untouched output from NULL."""
            text = P(UNTOUCHED)
            if k.mode == 'pinyin':
                ret = k.fn('get_sentence', B, P, U, C.POINTER(P))(inst, 0, C.byref(text))
            else:
                ret = k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(text))
            return [ret, 'untouched' if text.value == UNTOUCHED else k.text(text.value)]

        def choose(offset, wanted):
            """Choose the first NORMAL row matching wanted at the given byte offset.

            Assert candidate generation and getters; return [wanted, signed selection
            result] or raise AssertionError when no matching row is displayed."""
            if k.mode == 'pinyin':
                assert k.fn('guess_candidates', B, P, Z, U)(inst, offset, 0x1e)
            else:
                assert k.fn('guess_candidates_after_cursor', B, P, Z)(inst, offset)
            count = U()
            assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            for index in range(count.value):
                cand, text, kind = P(), S(), I()
                assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(cand))
                assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
                assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
                if kind.value == 2 and text.value.decode() == wanted:
                    return [wanted, k.fn('choose_candidate', I, P, Z, P)(inst, offset, cand)]
            raise AssertionError('missing NORMAL candidate: ' + wanted)

        out = {'parse': parse(inst, initial.encode()), 'initial_guess': guess(inst),
               'initial_sentence': sentence()}
        out['choices'] = [choose(offset, text) for offset, text in choices]
        if sequence == 'guess':
            out['chosen_guess'] = guess(inst)
            out['chosen_sentence'] = sentence()
        if sequence in ('empty', 'empty-guess'):
            out['empty_parse'] = parse(inst, b'')
            if sequence == 'empty-guess':
                out['empty_guess'] = guess(inst)
        if intermediate is not None:
            out['intermediate_parse'] = parse(inst, intermediate.encode())
            if sequence == 'intermediate-guess':
                out['intermediate_guess'] = guess(inst)
                out['intermediate_sentence'] = sentence()
        out['replacement_parse'] = parse(inst, replacement.encode())
        out['replacement_guess'] = guess(inst)
        out['replacement_sentence'] = sentence()
        out['cleared_constraints'] = [k.fn('clear_constraint', B, P, Z)(inst, offset)
                                      for offset, _ in choices]
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _name, _initial, _choices, _replacement, _control in (
            ('changed-suffix', 'nihao', ((0, '泥'),), 'nihai', False),
            ('shorter', 'nihao', ((0, '泥'),), 'ni', True),
            ('longer', 'nihao', ((0, '泥'),), 'nihaoma', True),
            ('different-first', 'nihao', ((0, '泥'),), 'nahao', True),
            ('span-past-end', 'nihao', ((2, '浩'),), 'ni', True),
            ('whole-span-past-end', 'nihaoshijie', ((0, '你好'),), 'ni', True),
            ('edit-within-span', 'nihaoshijie', ((0, '你好'),), 'nihaishijie', True),
            ('whole-span-kept', 'nihaoshijie', ((0, '你好'),), 'nihaoshijian', False),
            ('two-choices', 'nihao', ((0, '泥'), (2, '浩')), 'nihai', False)):
        for _sequence in ('direct', 'guess', 'empty', 'empty-guess'):
            case(f'parse-constraints-{_mode}-{_name}-{_sequence}', mode=_mode,
                 control=_control or _sequence == 'empty-guess', issues=('710',))(
                changed_parse_constraints(_initial, _choices, _replacement, _sequence))
    # A temporary mismatch or truncation must not destroy a choice until
    # something validates that intermediate matrix.
    for _name, _intermediate in (('temporary-mismatch', 'nahao'),
                                ('temporary-shorter', 'n')):
        for _sequence in ('direct', 'intermediate-guess'):
            case(f'parse-constraints-{_mode}-{_name}-{_sequence}', mode=_mode,
                 control=_sequence == 'intermediate-guess' and not (
                     _mode == 'zhuyin' and _name == 'temporary-shorter'), issues=('710',))(
                changed_parse_constraints('nihao', ((0, '泥'),), 'nihai',
                                          _sequence, _intermediate))


# #697, row 58: capture the entire list, including row types and order.
def row58_rows(k, prefix):
    """Guess predictions for a Unicode prefix and return the flag and ordered [type, text] rows.

    Assert count and candidate getters, decode borrowed strings as strict UTF-8,
    and read the list even if the prediction call returns false."""
    inst = k.inst
    ret = k.fn('guess_predicted_candidates', B, P, S)(inst, prefix.encode())
    n = U()
    assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(n))
    rows = []
    for i in range(n.value):
        cand, kind, text = P(), I(), S()
        assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        rows.append([kind.value, text.value.decode()])
    return dict(ret=ret, rows=rows)


def row58_seed_bigrams(k):
    """Seed competing Beijing and Shanghai prediction statistics and return their unigram outputs.

    Train constrained 我北京 once and 我上海 three times with dynamic options,
    reset after each phrase, then add 100000 to Beijing's unigram. Assert parses,
    guesses, choices, training, resets and the frequency update; leave live state changed."""
    # Constrain valid n-best results before training, following the existing
    # training cases. Shanghai has more bigram observations; independently
    # boost Beijing's unigram so those two proposed sort keys disagree.
    k.fn('set_options', B, P, U)(k.ctx, 0x18a | (1 << 9))
    for text, phrase, repeats in ((b'wobeijing', '北京', 1), (b'woshanghai', '上海', 3)):
        inst = k.inst
        assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, text) == len(text)
        assert k.fn('guess_sentence', B, P)(inst)
        offset = choose_text(k, inst, 0, '我')
        assert offset == 2
        assert choose_text(k, inst, offset, phrase) == len(text)
        assert k.fn('guess_sentence', B, P)(inst)
        for _ in range(repeats):
            assert k.fn('train', B, P, C.c_ubyte)(inst, 0)
        assert k.fn('reset', B, P)(inst)
    beijing, shanghai = tokens_of(k, '北京')[0], tokens_of(k, '上海')[0]
    assert k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, beijing, 100000)
    return {'北京': unigram_of(k, beijing), '上海': unigram_of(k, shanghai)}


def row58_scoring(dynamic):
    """Build a prediction-order case with seeded competing counts and the chosen dynamic option."""
    def probe(k):
        """Seed bigrams, set the dynamic option and return unigrams plus predictions after 我."""
        counts = row58_seed_bigrams(k)
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        return dict(unigrams=counts, prediction=row58_rows(k, '我'))
    return probe


for _dynamic in (False, True):
    case('row58-bigram-scoring-' + ('on' if _dynamic else 'off'), issues=('697', 'row:58'))(row58_scoring(_dynamic))


def row58_choose(k, kind, text, times):
    """Repeatedly choose the first current predicted row matching kind and text.

    Make times selections. Assert getters, reuse the borrowed row pointer
    without regenerating the list, and return each selection flag.
    Raise AssertionError if the row is absent;
    nonpositive times returns an empty list once a matching row is found."""
    n = U()
    assert k.fn('get_n_candidate', B, P, C.POINTER(U))(k.inst, C.byref(n))
    for i in range(n.value):
        cand, found_kind, found_text = P(), I(), S()
        assert k.fn('get_candidate', B, P, U, C.POINTER(P))(k.inst, i, C.byref(cand))
        assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(k.inst, cand, C.byref(found_kind))
        assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(k.inst, cand, C.byref(found_text))
        if found_kind.value == kind and found_text.value.decode() == text:
            return [k.fn('choose_predicted_candidate', B, P, P)(k.inst, cand) for _ in range(times)]
    raise AssertionError('missing predicted row: %s %s' % (kind, text))


def row58_prefix(dynamic, text, times):
    """Build a repeated PREFIX-row choice case with before/after counts and predictions.

    text is the suffix after 我; times controls consecutive choices and dynamic
    sets the prediction option."""
    def probe(k):
        """Select the requested type-5 suffix repeatedly and snapshot its full-phrase unigram.

        Look up 我 plus text before predicting; return before/after lists and all
        selection flags without saving."""
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        token = tokens_of(k, '我' + text)[0]
        before = row58_rows(k, '我')
        count_before = unigram_of(k, token)
        chosen = row58_choose(k, 5, text, times)
        return dict(before=before, count_before=count_before, chosen=chosen,
                    count_after=unigram_of(k, token), after=row58_rows(k, '我'))
    return probe


def row58_bigram_choices(dynamic):
    """Build a live BIGRAM-row ranking case with successive batches of Shanghai selections."""
    def probe(k):
        """Seed competing counts and select 上海 in batches of one, twenty and two hundred.

        Set the requested dynamic option and snapshot predictions after each batch;
        selections accumulate rather than restarting from the seed."""
        counts = row58_seed_bigrams(k)
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        out = dict(unigrams=counts, before=row58_rows(k, '我'))
        # Fixed observation points, not a ranking threshold: each acceptance
        # contributes 483 unigram and 69 bigram independently of options.
        for times in (1, 20, 200):
            out['chosen-%d' % times] = row58_choose(k, 4, '上海', times)
            out['rows-%d' % times] = row58_rows(k, '我')
        return out
    return probe


def row58_import(k, rows):
    """Import UTF-8 (phrase, reading, count) rows into user library seven.

    Assert iterator creation and every add; close the iterator only on normal
    completion. Leave imported state live and unsaved."""
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    assert it
    for text, reading, count in rows:
        assert k.fn('iterator_add_phrase', B, P, S, S, I)(it, text.encode(), reading.encode(), count)
    k.fn('end_add_phrases', None, P)(it)


def row58_conversion(dynamic):
    """Build prediction fixtures around score ties and integer-to-float count boundaries."""
    def probe(k):
        """Import equal-count 乙乙 before 甲甲, then compare tie and large-count prediction lists.

        Use import counts 33/33/1/2, followed by 5592405/5592406, and report the last
        two full-phrase unigrams. Apply the requested dynamic option without saving."""
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        # Unigram = import count * 3. Counts 3/6 quantize to the same
        # amplified integer; 99 ties a baked 100 after amplification.
        # Insert in reverse UCS-4 byte order to distinguish order from token.
        row58_import(k, [('我乙乙', "wo3'yi3'yi3", 33),
                         ('我甲甲', "wo3'jia3'jia3", 33),
                         ('我丙丙', "wo3'bing3'bing3", 1),
                         ('我丁丁', "wo3'ding1'ding1", 2)])
        out = dict(ties=row58_rows(k, '我'))
        row58_import(k, [('我戊戊', "wo3'wu4'wu4", 5592405),
                         ('我己己', "wo3'ji3'ji3", 5592406)])
        # 2^24-1 and 2^24+2: the integer-to-float representability edge.
        out['float-edge'] = row58_rows(k, '我')
        out['unigrams'] = {text: unigram_of(k, tokens_of(k, text)[0])
                           for text in ('我戊戊', '我己己')}
        return out
    return probe


def row58_lambda(dynamic):
    """Build a prediction-order case with lambda one and a boosted competing phrase."""
    def probe(k):
        """Replace a private table.conf lambda with one, initialize and boost 我是谁 by 100000.

        Cache the new context, assert setup and return predictions after 我 under
        the requested dynamic option. Leave the private directory and context live."""
        system = private_system(k)
        conf = Path(system, 'table.conf')
        text = conf.read_text()
        conf.unlink()
        conf.write_text(re.sub(r'lambda parameter:[^\n]+', 'lambda parameter:1', text))
        k._ctx = k.init(system=system)
        assert k._ctx
        # lambda=1 makes every predicted score zero. A nonuniform unigram
        # overlay must therefore leave only the source collection tie order.
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        token = tokens_of(k, '我是谁')[0]
        assert k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, token, 100000)
        return dict(prediction=row58_rows(k, '我'))
    return probe


for _dynamic in (False, True):
    _state = 'on' if _dynamic else 'off'
    case('row58-prefix-no-bigram-' + _state, issues=('697', 'row:58'))(row58_prefix(_dynamic, '是', 1))
    case('row58-prefix-live-21-' + _state, issues=('697', 'row:58'))(row58_prefix(_dynamic, '是谁', 21))
    case('row58-bigram-choices-' + _state, issues=('697', 'row:58'))(row58_bigram_choices(_dynamic))
    case('row58-conversions-' + _state, issues=('697', 'row:58'))(row58_conversion(_dynamic))
    case('row58-lambda-ties-' + _state, issues=('697', 'row:58'))(row58_lambda(_dynamic))


def row58_prefix_overflow(dynamic):
    """Build a PREFIX-choice case near the user library's 32-bit total limit."""
    def probe(k):
        """Import 我 and a high-count suffix phrase, choose it once and compare counts and rows.

        Use import counts 1 and 1431655631 with the requested dynamic option. Return
        selection flags and live observations without assuming the update succeeded."""
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        # Library 7 has only 399 left before guint32::MAX. The next 483
        # cannot fit, but the facade total still advances and wraps. An
        # explicit prefix keeps this refusal observable in the parent too;
        # the conversion cases separately cover implicit prefix index keys.
        row58_import(k, [('我', 'wo3', 1),
                         ('我甲甲', "wo3'jia3'jia3", 1431655631)])
        token = tokens_of(k, '我甲甲')[0]
        before = row58_rows(k, '我')
        count_before = unigram_of(k, token)
        chosen = row58_choose(k, 5, '甲甲', 1)
        return dict(before=before, count_before=count_before, chosen=chosen,
                    count_after=unigram_of(k, token), after=row58_rows(k, '我'))
    return probe


for _dynamic in (False, True):
    case('row58-prefix-overflow-' + ('on' if _dynamic else 'off'), issues=('697', 'row:58'))(row58_prefix_overflow(_dynamic))


def row58_signed_score(dynamic, duplicate=False, denominator=20000000):
    """Build a large-score prediction case using a fixed-fixture total and requested denominator.

    duplicate selects the existing phrase 我的心 instead of 我甲甲. The callable
    uses a modulo-2**32 absent-token update derived from base total 51051831."""
    def probe(k):
        """Import the high-count phrase, attempt the compensating total update and collect predictions.

        Set the dynamic option and record the absent-token add flag. The delta
        assumes import count 1431655631 contributes three times that count to the
        fixed system total; it is not recalculated from live library files.
        The factor follows _add_phrase in src/pinyin.cpp at pin 074a2219."""
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        phrase = ('我的心', "wo3'de5'xin1") if duplicate else ('我甲甲', "wo3'jia3'jia3")
        row58_import(k, [(*phrase, 1431655631)])
        # The absent-token add advances only the facade total. Make the
        # denominator 20,000,000: score exceeds gint::MAX; at 10,000,000
        # it also exceeds guint32::MAX. Pin x86-64 converts via gint64
        # then retains the low 32 bits, rather than saturating.
        total = (51051831 + 1431655631 * 3) % (1 << 32)
        delta = (denominator - total) % (1 << 32)
        added = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, 0x01ffffff, delta)
        return dict(absent_add=added, prediction=row58_rows(k, '我'))
    return probe


for _dynamic in (False, True):
    _state = 'on' if _dynamic else 'off'
    case('row58-signed-score-' + _state, issues=('697', 'row:58'))(row58_signed_score(_dynamic))
    case('row58-signed-duplicate-' + _state, issues=('697', 'row:58'))(row58_signed_score(_dynamic, True))
    case('row58-unsigned-score-' + _state, issues=('697', 'row:58'))(row58_signed_score(_dynamic, denominator=10000000))


def row58_zero_total(dynamic, punct=False):
    """Build a zero-total bigram prediction case, optionally including punctuation candidates."""
    def probe(k):
        """Seed bigrams, derive the three trained phrases' live delta and attempt to wrap the total.

        Use FACADE_TOTAL plus that delta and an absent-token update modulo 2**32.
        Record the selected prediction flag and whether its list is empty; assert
        that a false prediction has no rows. Do not return candidate contents."""
        tokens = {token for text in ('我', '北京', '上海') for token in tokens_of(k, text)}
        before = sum(unigram_of(k, token)[1] for token in tokens)
        row58_seed_bigrams(k)
        after = sum(unigram_of(k, token)[1] for token in tokens)
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        # Only these three phrases were trained/added: use their live delta,
        # including 我's reselection seed, rather than a guessed threshold.
        total = FACADE_TOTAL + after - before
        added = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, 0x01ffffff,
                                                            (-total) % (1 << 32))
        name = 'guess_predicted_candidates_with_punctuations' if punct else 'guess_predicted_candidates'
        ret = k.fn(name, B, P, S)(k.inst, '我'.encode())
        n = U()
        assert k.fn('get_n_candidate', B, P, C.POINTER(U))(k.inst, C.byref(n))
        if not ret:
            assert n.value == 0
        return dict(add=added, ret=ret, rows=[] if n.value == 0 else ['not empty'])
    return probe


for _dynamic in (False, True):
    for _punct in (False, True):
        name = 'row58-total-zero-' + ('punct-' if _punct else '') + ('on' if _dynamic else 'off')
        case(name, abort=False, issues=('697', 'row:58'))(row58_zero_total(_dynamic, _punct))


def row58_prefix_zero_total(dynamic):
    """Build a PREFIX prediction case after attempting to wrap the facade total to zero."""
    def probe(k):
        """Set the dynamic option, apply wrap_total_to_zero and return the complete prediction list."""
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        added = wrap_total_to_zero(k, k.inst)
        # Prefix rows bypass pinyin.cpp:1859. The pin's invalid conversion
        # has zero low bits; the complete list retains collection tie order.
        return dict(add=added, prediction=row58_rows(k, '我'))
    return probe


for _dynamic in (False, True):
    case('row58-prefix-zero-total-' + ('on' if _dynamic else 'off'), control=True, issues=('697', 'row:58'))(
        row58_prefix_zero_total(_dynamic))


def row58_lambda_rounding(dynamic):
    """Build a prediction case with a decimal lambda just below the binary32 midpoint beneath one."""
    def probe(k):
        """Write lambda 0.9999999701976776123036 into a private config and predict with a large user count.

        Cache and assert the new context, apply the dynamic option and import
        我甲甲 with count 1431655631. Leave the context and private files live."""
        system = private_system(k)
        conf = Path(system, 'table.conf')
        text = conf.read_text()
        conf.unlink()
        # Just below the f32 midpoint between 1 and its predecessor. Direct
        # %f parsing yields the predecessor; a rational -> f64 -> f32 rounds
        # to 1 and erases every score. The large live user count exposes it.
        conf.write_text(re.sub(r'lambda parameter:[^\n]+',
                              'lambda parameter:0.9999999701976776123036', text))
        k._ctx = k.init(system=system)
        assert k._ctx
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        row58_import(k, [('我甲甲', "wo3'jia3'jia3", 1431655631)])
        return dict(prediction=row58_rows(k, '我'))
    return probe


for _dynamic in (False, True):
    case('row58-lambda-rounding-' + ('on' if _dynamic else 'off'), issues=('697', 'row:58'))(row58_lambda_rounding(_dynamic))



# #712 review 4232017129: an imported system-library item has a complete
# override count, whereas an immutable item still needs its store delta.
def row58_system_import(library, dynamic=False, existing=False):
    """Build a prediction/count case for an imported system-library phrase.

    existing selects 我是谁 and its first token's library, ignoring library;
    otherwise import 我甲甲 into library. dynamic affects pinyin only; zhuyin
    records import/count outputs without prediction calls."""
    def probe(k):
        """Import count 33 into a validated system library and observe live count changes.

        Assert the expected token presence and one resulting token. For a new
        pinyin phrase, adjust 我是谁 as a competitor; select the imported PREFIX
        row once then twice, snapshotting counts and complete lists after each
        batch. For zhuyin, stop after the import observations."""
        phrase = '我是谁' if existing else '我甲甲'
        prefix, suffix = '我', ('是谁' if existing else '甲甲')
        if k.mode == 'pinyin':
            reading = "wo3'shi4'shei2" if existing else "wo3'jia3'jia3"
            assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        else:
            reading = 'ㄨㄛˇ ㄕˋ ㄕㄟˊ' if existing else 'ㄨㄛˇ ㄐㄧㄚˇ ㄐㄧㄚˇ'
        initial = tokens_of(k, phrase)
        target_library = (initial[0] >> 24) if existing else library
        assert 1 <= target_library <= 4
        before = [t for t in initial if t >> 24 == target_library]
        assert bool(before) == existing
        it = k.fn('begin_add_phrases', P, P, U)(k.ctx, target_library)
        assert it
        added = k.fn('iterator_add_phrase', B, P, S, S, I)(
            it, phrase.encode(), reading.encode(), 33)
        assert added
        k.fn('end_add_phrases', None, P)(it)
        tokens = [t for t in tokens_of(k, phrase) if t >> 24 == target_library]
        assert len(tokens) == 1
        token = tokens[0]
        out = dict(tokens_before=before, added=added, tokens_after=tokens,
                   imported_count=unigram_of(k, token))
        if k.mode == 'pinyin':
            if not existing:
                # Place an immutable competitor between the correct count
                # after three accepts (33*3 + 3*483) and its erroneous double.
                rival = tokens_of(k, '我是谁')[0]
                rival_count = (33 * 3 + 3 * 483) * 3 // 2
                delta = rival_count - unigram_of(k, rival)[1]
                assert k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, rival, delta)
                out['rival_count'] = unigram_of(k, rival)
            out['rows-0'] = row58_rows(k, prefix)
            # Three acceptances in two snapshots exercise the live override,
            # not only its initial import value. Preserve every candidate.
            for accepted in (1, 2):
                out['chosen-%d' % accepted] = row58_choose(k, 5, suffix, accepted)
                out['count-%d' % accepted] = unigram_of(k, token)
                out['rows-%d' % accepted] = row58_rows(k, prefix)
        return out
    return probe


for _library in range(1, 5):
    for _dynamic in (False, True):
        case('row58-system-import-%d-%s' % (_library, 'on' if _dynamic else 'off'), issues=('712', '697', 'row:58'))(
            row58_system_import(_library, _dynamic))
    # The import/count twin exists; libzhuyin has no predicted API twin.
    case('row58-system-import-zhuyin-%d' % _library, mode='zhuyin', control=True, issues=('712', '697', 'row:58'))(
        row58_system_import(_library))
for _dynamic in (False, True):
    case('row58-system-existing-' + ('on' if _dynamic else 'off'), control=True, issues=('712', '697', 'row:58'))(
        row58_system_import(4, _dynamic, existing=True))
case('row58-system-existing-zhuyin', mode='zhuyin', control=True, issues=('712', '697', 'row:58'))(
    row58_system_import(4, existing=True))


# #682/#683, pin 074a2219: forward resplit mutation, then divided, then
# fuzzy (phonetic_key_matrix.cpp:87-228; pinyin.cpp:1497-1524). Every
# offset reads the whole parse's matrix, including newly created columns.
# libzhuyin's full-pinyin seam omits both split passes (zhuyin.cpp:1017-
# 1040); cover that facade's distinct order too. Compare every ordered
# type/text row.
MATRIX_RESPLIT_INPUTS = (
    'banangang', 'baguanangang', 'chenanengang', 'anangang', 'fanangang',
    'lianai', 'xiane', 'liane', 'xianai', 'qiane', 'lian', 'xian',
)


def matrix_resplit_windows(text, options):
    """Build a full-input candidate-window comparison under the supplied option word.

    text is intended to be ASCII pinyin: the callable compares consumed bytes
    with len(text) and visits offsets through that length, inclusive."""
    def run(k):
        """Assert a complete parse and collect ordered candidates at every input offset.

        Use pinyin sort word 0x1e or zhuyin after-cursor guesses without a preceding
        sentence guess. Return consumed length and the list of window observations."""
        assert k.fn('set_options', B, P, U)(k.ctx, options)
        inst = k.inst
        parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text.encode())
        assert parsed == len(text), 'characterisation requires a complete parse'
        windows = []
        for offset in range(len(text) + 1):
            if k.mode == 'pinyin':
                windows.append(guess_rows(k, inst, offset, 0x1e))
            else:
                windows.append(zhuyin_guess_rows(k, inst, offset))
        return {'parsed': parsed, 'windows': windows}
    return run


for _mode in ('pinyin', 'zhuyin'):
    for _tables in (False, True):
        for _fuzzy in (False, True):
            # PINYIN_AMB_AN_ANG: expand the halves produced by the tables.
            _options = (0x18a if _tables else 0x0a) | ((1 << 17) if _fuzzy else 0)
            for _text in MATRIX_RESPLIT_INPUTS:
                case('matrix-resplit-%s-%s-%s-%s' % (
                    _mode, _text, 'tables' if _tables else 'plain',
                    'fuzzy' if _fuzzy else 'exact'), mode=_mode,
                    control=not _tables or _text in ('baguanangang', 'chenanengang')
                    or (_mode == 'pinyin' and _text in ('lian', 'xian')), issues=('682', '683'))(
                        matrix_resplit_windows(_text, _options))

# --------------------------------------------------------------------------
# #694 and #525 batch B: the system `table.conf` decides the library set.
# `SystemTableInfo2::load` (`table_info.cpp:194-294` at 074a2219) reads the
# header with five `fscanf`s and the rows as words in groups of six; the
# rows' file types and names drive `pinyin_init`/`zhuyin_init`'s library loop
# (`pinyin.cpp:377-392`), the loads, `_write_files` and `_rename_files`.
# Every case runs on a private copy of the oracle's system directory whose
# `table.conf` is rewritten; the data files are links to the oracle's.
# --------------------------------------------------------------------------

def conf_system(k, *edits):
    """A private system directory whose table.conf has `edits` applied:
    (old, new) replaces, ('+', line) appends a row, ('-', substring) drops
    the lines naming it, ('re', pattern, replacement) substitutes."""
    system = private_system(k)
    path = os.path.join(system, 'table.conf')
    # A name may hold a byte that is not UTF-8 (`\udcff`, round-tripped).
    text = Path(k.data, 'table.conf').read_text(errors='surrogateescape')
    for edit in edits:
        if edit[0] == '+':
            text += edit[1] + '\n'
        elif edit[0] == '-':
            text = ''.join(line for line in text.splitlines(True) if edit[1] not in line)
        elif edit[0] == 're':
            assert re.search(edit[1], text), edit
            text = re.sub(edit[1], edit[2], text)
        else:
            assert edit[0] in text, edit
            text = text.replace(edit[0], edit[1])
    os.unlink(path)
    Path(path).write_text(text, errors='surrogateescape')
    return system


CONF_TEXTS = {'pinyin': (b'nihao', b'yishu'), 'zhuyin': (b'ni3hao3', b'yi4shu4')}


def conf_system_bytes(k, mutate):
    """A private system copy whose table.conf is `mutate` of the raw stock
    bytes. Bytes and NUL are preserved, unlike `conf_system`'s text edits."""
    system = private_system(k)
    path = os.path.join(system, 'table.conf')
    data = Path(k.data, 'table.conf').read_bytes()
    os.unlink(path)
    Path(path).write_bytes(mutate(data))
    return system


def conf_guess_rows(k, inst, limit=12):
    """`guess_candidates` at offset 0: the call's answer and the first rows."""
    if k.mode == 'pinyin':
        ok = k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
    else:
        ok = k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    count = U(UNTOUCHED)
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    rows = []
    for index in range(min(count.value, limit)):
        cand, text = P(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(cand))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        rows.append(text.value.decode())
    return ok, count.value, rows


def conf_sentence(k, inst):
    out = P(UNTOUCHED)
    if k.mode == 'pinyin':
        k.fn('get_sentence', B, P, C.c_ubyte, C.POINTER(P))(inst, 0, C.byref(out))
    else:
        k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(out))
    return None if out.value == UNTOUCHED else k.text(out.value)


def conf_view(k, ctx):
    """Sentences and candidate lists for two inputs: the observable the
    loaded library set decides."""
    view = {}
    for text in CONF_TEXTS[k.mode]:
        inst = k.fn('alloc_instance', P, P)(ctx)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
        k.fn('guess_sentence', B, P)(inst)
        view[text.decode()] = [conf_sentence(k, inst), *conf_guess_rows(k, inst)]
    return view


def user_listing(k, directory=None):
    """The files of the user directory (or of `directory`): names as bytes,
    `user.conf` by text, library chunks by size, and the DBM files named
    `user_*` by whether a chunk was written over them (a chunk's length word
    is its size less the 8-byte header)."""
    directory = directory or k.user
    listing = {}
    for name in sorted(os.listdir(directory)):
        file = os.path.join(directory, name)
        key = os.fsencode(name).decode('latin-1')
        if os.path.isdir(file):
            listing[key] = 'dir'
        elif name == 'user.conf':
            listing[key] = Path(file).read_text().replace('\n', '|')
        elif name.endswith(('.bin', '.dbin')):
            data = Path(file).read_bytes()
            framed = len(data) >= 8 and int.from_bytes(data[:4], 'little') == len(data) - 8
            listing[key] = len(data) if not name.startswith('user_') else ('chunk' if framed else 'dbm')
        else:
            listing[key] = None
    return listing


def conf_session(k, system):
    """Open on `system`, look up, learn one sentence and one phrase, save:
    what the rows decide to load, to accept and to write."""
    ctx = k.init(system=system)
    assert ctx, 'init failed'
    out = {'view': conf_view(k, ctx)}
    inst = k.fn('alloc_instance', P, P)(ctx)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, CONF_TEXTS[k.mode][0])
    k.fn('guess_sentence', B, P)(inst)
    out['train'] = k.fn('train', B, P, C.c_ubyte)(inst, 0) if k.mode == 'pinyin' else k.fn('train', B, P)(inst)
    it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
    reading = b"ce'shi'ci" if k.mode == 'pinyin' else 'ㄘㄜˋ ㄕˋ ㄘˊ'.encode()
    out['add'] = k.fn('iterator_add_phrase', B, P, S, S, I)(it, '测试词'.encode(), reading, 1)
    k.fn('end_add_phrases', None, P)(it)
    out['save'] = k.fn('save', B, P)(ctx)
    k.fn('fini', None, P)(ctx)
    out['files'] = user_listing(k)
    return out


# #1119: direct KC classes open literal paths (074a2219
# ngram_kyotodb.cpp:117-119; chewing_large_table2_kyotodb.cpp:87-89).
# table_info.cpp:255-284 preserves '#' in filename fields; pinyin.cpp:
# 250-276, 303-319, 939-988, 1042-1084 loads/saves those literal names.
# The DBM filenames are fixed by the facade; table.conf can rename the
# system chunks, user chunks and journals. The store trait laws also
# exercise '#' in actual TreeDB/HashDB filenames, without a symlink.
def hash_path_fixture(k, placement):
    system = private_system(k)
    system_name, journal_name, user_name = 'gb_char.bin', 'gb_char.dbin', 'user.bin'
    if placement == 'system-directory':
        renamed = system + '#type=kct'
        os.rename(system, renamed)
        system = renamed
    elif placement == 'user-directory':
        renamed = k.user + '#type=kct'
        os.rename(k.user, renamed)
        k.user = renamed
    elif placement in ('system-filename', 'user-filename'):
        conf = Path(system, 'table.conf')
        text = conf.read_text()
        if placement == 'system-filename':
            system_name = 'gb#type=kct.bin'
            Path(system, 'gb_char.bin').rename(Path(system, system_name))
            text = text.replace('gb_char.bin', system_name)
        else:
            journal_name, user_name = 'gb#type=kct.dbin', 'user#type=kct.bin'
            text = text.replace('gb_char.dbin', journal_name).replace('user.bin', user_name)
        conf.unlink()  # private_system linked this file to the immutable pin data
        conf.write_text(text)
    else:
        raise ValueError('unknown hash path placement: ' + placement)
    k.data = system
    return system_name, journal_name, user_name


def hash_path_session(placement):
    def probe(k):
        system_name, journal_name, user_name = hash_path_fixture(k, placement)
        out = {'training': token_unigram_trained(k),
               'import': token_unigram_user(k, add_frequency=False)}
        out['save'] = k.fn('save', B, P)(k.ctx)
        assert out['save'], 'the trained profile must save'
        out['files'] = user_listing(k)
        assert Path(k.data, system_name).is_file(), 'the configured system file must exist'
        assert Path(k.user, journal_name).is_file(), 'the trained journal must be written'
        assert Path(k.user, user_name).is_file(), 'the imported library must be written'
        if placement == 'system-filename':
            assert not Path(k.data, 'gb_char.bin').exists()
        if placement == 'user-filename':
            assert not Path(k.user, 'gb_char.dbin').exists()
            assert not Path(k.user, 'user.bin').exists()
        k.fn('free_instance', None, P)(k.inst)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = k._inst = None
        out['reopen'] = bool(k.ctx)
        out['trained-after-reopen'] = unigram_of(k, int(out['training']['token'], 16))
        assert out['trained-after-reopen'] == out['training']['after']
        out['import-after-reopen'] = unigram_of(k, int(out['import']['token'], 16))
        assert out['import-after-reopen'] == out['import']['after']
        k.fn('free_instance', None, P)(k.inst)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = k._inst = None
        # libzhuyin has no export iterator (zhuyin.h at 074a2219).
        # Reuse the existing export worker on this side's companion
        # libpinyin, as axis_c_import does for either facade's saved profile.
        library = Path(k.lib._name)
        companion = library.with_name(library.name.replace('libzhuyin', 'libpinyin', 1))
        proc = subprocess.run([sys.executable, str(WORKER), 'pinyin', str(companion),
                               k.data, 'axis-c-export-user'], capture_output=True,
                              env=dict(os.environ, AXIS_C_REOPEN_USER=k.user))
        assert proc.returncode == 0, proc.stderr.decode('utf-8', 'surrogateescape')
        out['export'] = json.loads(proc.stdout)
        assert out['export']['rows'], 'reopen must preserve the imported phrase for export'
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _placement in ('system-directory', 'user-directory', 'system-filename', 'user-filename'):
        case('hash-path/' + _placement + '-' + _mode, mode=_mode, stderr=True,
             issues=('1119',))(hash_path_session(_placement))


def addon_session(k, system, indexes=(0, 3, 4, 5, 15)):
    """The addon libraries a row set loads, and the candidates they add."""
    ctx = k.init(system=system)
    assert ctx, 'init failed'
    load = k.fn('load_addon_phrase_library', B, P, C.c_ubyte)
    out = {'load': {str(index): load(ctx, index) for index in indexes}}
    out['view'] = conf_view(k, ctx)
    k.fn('fini', None, P)(ctx)
    return out


# Files the parent already answered like the pin: the header fields it read,
# the words it never needed, an alias, a row that changes nothing observable.
CONF_CONTROLS = {'addon-duplicate-row', 'addon-files-swapped', 'comment-dropped', 'extra-word-dropped',
                 'lambda-exponent', 'lambda-half', 'lambda-hex', 'lambda-hex-fraction', 'lambda-hex-upper',
                 'lambda-one', 'lambda-tiny', 'lambda-zero',
                 'reserved-as-system-file', 'short-row-dropped', 'source-format-zhuyin', 'tsi-is-gb',
                 'versions-other'}


# The libzhuyin candidate law read the pinned λ, not the file's, so these tie-order
# cases differ from the parent on libzhuyin only.
CONF_ZHUYIN_CHANGED = {'lambda-zero', 'lambda-one', 'lambda-tiny'}


def conf_case(name, *edits, modes=('pinyin', 'zhuyin'), session=conf_session,
              issues=('694', '525'), **options):
    for mode in modes:
        def probe(k, edits=edits):
            return session(k, conf_system(k, *edits))
        control = name in CONF_CONTROLS and not (mode == 'zhuyin' and name in CONF_ZHUYIN_CHANGED)
        case('table-conf-' + name + ('-zhuyin' if mode == 'zhuyin' else ''), mode=mode,
             **{'control': control, **options}, issues=issues)(probe)


# Valid files that are not the stock one: the pin follows the rows.
_STOCK_DBIN = 'gb_char.dbin SYSTEM_FILE'
conf_case('row-removed-opengram', ('-', 'OPENGRAM_DICTIONARY'))
conf_case('row-removed-gbk', ('-', 'GBK_DICTIONARY'))
conf_case('row-removed-gb', ('-', 'GB_DICTIONARY'))
conf_case('not-used-opengram', ('opengram.dbin SYSTEM_FILE', 'opengram.dbin NOT_USED'))
conf_case('not-used-gb', (_STOCK_DBIN, 'gb_char.dbin NOT_USED'))
conf_case('not-used-merged', ('merged.dbin SYSTEM_FILE', 'merged.dbin NOT_USED'))
conf_case('not-used-user-dictionary', ('user.bin USER_FILE', 'user.bin NOT_USED'))
conf_case('not-used-network-dictionary', ('network.bin USER_FILE', 'network.bin NOT_USED'))
conf_case('user-files-renamed', ('gb_char.dbin', 'x_gb.dbin'), ('merged.dbin', 'x_merged.dbin'),
          ('user.bin', 'x_user.bin'), ('addon.bin', 'x_addon.bin'))
conf_case('tsi-is-gb', ('default GB_DICTIONARY', 'default TSI_DICTIONARY'))
conf_case('later-row-wins', ('+', 'default OPENGRAM_DICTIONARY opengram.table opengram.bin opengram.dbin NOT_USED'))
conf_case('network-as-system-file', ('default NETWORK_DICTIONARY NULL NULL network.bin USER_FILE',
                                      'default NETWORK_DICTIONARY art.table art.bin network.dbin SYSTEM_FILE'))
conf_case('reserved-as-system-file', ('default RESERVED NULL NULL NULL NOT_USED',
                                       'default RESERVED merged.table merged.bin x_reserved.dbin SYSTEM_FILE'))
# λ spelled inside [0, 1]. The candidate law `(λ·bigram + (1−λ)·unigram)·2²⁴`
# reads the file's λ (it used to be the pinned constant, which listed tied
# libzhuyin rows in another order at λ = 0, 1 and 1e-30).
for _name, _value in (('zero', '0'), ('half', '0.5'), ('one', '1'), ('exponent', '5e-1'), ('tiny', '1e-30')):
    conf_case('lambda-' + _name, ('lambda parameter:0.312699', 'lambda parameter:' + _value))
# λ outside [0, 1]: below 0 the possibilities exceed one and the costs go
# negative; above 1 `unigram_lambda` is negative, `log` of it is NaN and the
# NaN is sticky through the trellis. The tail sort (`g_ptr_array_sort` over a
# comparator that turns the float difference into a `gint`) then orders the
# sentences.
for _name, _value in (('negative-half', '-0.5'), ('negative-two', '-2'), ('one-and-a-half', '1.5'),
                      ('two', '2'), ('million', '1e6'), ('inf', 'inf'), ('nan', 'nan')):
    conf_case('lambda-' + _name, ('lambda parameter:0.312699', 'lambda parameter:' + _value))
# The header fields the user marker conforms against.
conf_case('database-format-other', ('re', r'database format:\w+', lambda found: 'database format:' + (
    'BerkeleyDB' if found.group(0).endswith('Tkrzw') else 'Tkrzw')))
conf_case('versions-other', ('binary format version:7', 'binary format version:8'),
          ('model data version:14', 'model data version:15'))
conf_case('source-format-zhuyin', ('source table format:pinyin', 'source table format:zhuyin'))
# Words are read in groups of six: a short row, a comment and a seventh
# word at the end are dropped, and an addon index goes through a `guint8`.
conf_case('short-row-dropped', ('+', 'default GB_DICTIONARY a b c'))
conf_case('comment-dropped', ('+', '# a comment'))
conf_case('extra-word-dropped', ('+', 'default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE EXTRA'))
conf_case('addon-index-wraps', ('+', 'addon 256 art.table art.bin NULL DICTIONARY'),
          modes=('pinyin',), session=addon_session)
# The addon rows decide what `pinyin_load_addon_phrase_library` loads.
conf_case('addon-moved', ('addon 4 art.table', 'addon 3 art.table'), modes=('pinyin',), session=addon_session)
conf_case('addon-files-swapped', ('addon 4 art.table art.bin', 'addon 4 art.table culture.bin'),
          ('addon 5 culture.table culture.bin', 'addon 5 culture.table art.bin'),
          modes=('pinyin',), session=addon_session)
conf_case('addon-removed', ('-', 'addon 4 '), modes=('pinyin',), session=addon_session)
conf_case('addon-not-used', ('art.bin NULL DICTIONARY', 'art.bin NULL NOT_USED'), modes=('pinyin',), session=addon_session)
conf_case('addon-index-zero', ('addon 4 art.table', 'addon 0 art.table'), modes=('pinyin',), session=addon_session)
conf_case('addon-duplicate-row', ('+', 'addon 4 culture.table culture.bin NULL DICTIONARY'),
          modes=('pinyin',), session=addon_session)


# Class (c): the rows the pin dies on. The triggering call fails with one
# warning; `userdir` holds the profile the pin has written by then.
def conf_abort(name, *edits, call='init', userdir=True, modes=('pinyin', 'zhuyin'), ret=False,
               dir_chunk=None, control=False, issues=('694', '525')):
    for mode in modes:
        def probe(k, edits=edits):
            system = conf_system(k, *edits)
            if call == 'init':
                return {'ret': bool(k.init(system=system))}
            ctx = k.init(system=system)
            assert ctx, 'init failed'
            kind, index = call
            if kind == 'load':
                return {'ret': k.fn('load_phrase_library', B, P, C.c_ubyte)(ctx, index)}
            return {'ret': k.fn('load_addon_phrase_library', B, P, C.c_ubyte)(ctx, index)}
        case('table-conf-abort-' + name + ('-zhuyin' if mode == 'zhuyin' else ''), mode=mode,
             abort=ret, userdir=userdir, dir_chunk=dir_chunk, control=control, issues=issues)(probe)


# `table_info.cpp` aborts inside `SystemTableInfo2::load`, before
# `check_format`: the user directory is untouched.
conf_abort('source-format-unknown', ('source table format:pinyin', 'source table format:foo'))
conf_abort('database-format-unknown', ('re', r'database format:\w+', 'database format:LMDB'))
conf_abort('target-unknown', ('+', 'foo GB_DICTIONARY a b c SYSTEM_FILE'))
conf_abort('default-name-unknown', ('+', 'default FOO_DICTIONARY a b c SYSTEM_FILE'))
conf_abort('file-type-unknown', ('+', 'default GB_DICTIONARY a b c FOO_FILE'))
conf_abort('addon-index-99', ('+', 'addon 99 x.table x.bin NULL DICTIONARY'))
conf_abort('addon-index-16', ('+', 'addon 16 x.table x.bin NULL DICTIONARY'))
conf_abort('addon-index-negative', ('+', 'addon -1 x.table x.bin NULL DICTIONARY'))
# `UserTableInfo::load` converts the `database format:` token of the user's
# `user.conf` and aborts on one it does not know (`table_info.cpp:132`,
# reached from `check_format`); the system `table.conf` reaches the same
# converter above.
@case('user-conf-abort-database-format-unknown', abort=False, userdir=True, issues=('694', '525'))
def _(k):
    _write_user_conf(k)
    path = os.path.join(k.user, 'user.conf')
    with open(path) as user_conf:
        text = user_conf.read()
    text = re.sub(r'database format:\w+', 'database format:LMDB', text)
    assert 'database format:LMDB' in text
    with open(path, 'w') as user_conf:
        user_conf.write(text)
    return {'ret': bool(k.init())}


# The init loop (`pinyin.cpp:377-392`) runs after `check_format`: the profile
# is judged and `user.conf` written when it dies.
conf_abort('default-dictionary', (_STOCK_DBIN, 'gb_char.dbin DICTIONARY'))
conf_abort('default-dictionary-gbk', ('gbk_char.dbin SYSTEM_FILE', 'gbk_char.dbin DICTIONARY'))
# A row without a file name makes the pin read that directory as a chunk
# (`g_build_filename` stops at the NULL). `MemoryChunk` asserts on the
# `read` of a directory (`memory_chunk.h:434` for `load`, `:493` for `mmap`)
# only where `lseek(SEEK_END)` on it answers 8 or more, which the file system
# of the scratch directory decides (#1011): ext4, fuse-overlayfs and a new
# directory in an overlay answer more, tmpfs refuses `SEEK_END` on a
# directory. `dir_chunk` holds each case to the side of that condition the
# host is on (`case`, `directory_reads_as_a_chunk`). Without a system chunk
# the pin reads the system directory: on tmpfs `mmap` refuses it
# (`memory_chunk.h:486-489`), the pin writes `mmap <dir> failed!`
# (`pinyin.cpp:255-256`, `:289-290`, `zhuyin.cpp:199-200`) and dies of
# SIGSEGV on the empty chunk in `SubPhraseIndex::load` (`phrase_index.cpp:354`);
# the subject writes the same line and gives the abort site's answer.
conf_abort('system-file-without-chunk', ('opengram.table opengram.bin opengram.dbin SYSTEM_FILE',
                                         'opengram.table NULL opengram.dbin SYSTEM_FILE'), dir_chunk='system')
# Without a user file the pin reads the user directory, which holds the
# `user.conf` libpinyin saved in `check_format` (`pinyin.cpp:187`); on tmpfs
# the read is an ordinary `false`, and the pin and the subject both carry
# on. The zhuyin facade writes nothing at init, and an empty directory is 0
# bytes on some file systems, so it is not held.
conf_abort('system-file-without-user-file', ('merged.table merged.bin merged.dbin SYSTEM_FILE',
                                             'merged.table merged.bin NULL SYSTEM_FILE'), modes=('pinyin',),
           dir_chunk='user')
conf_abort('user-file-without-user-file', ('user.bin USER_FILE', 'NULL USER_FILE'), modes=('pinyin',),
           dir_chunk='user')
# A library call on a row the pin asserts on (`pinyin.cpp:457`, `:491`).
conf_abort('load-library-not-used', ('opengram.dbin SYSTEM_FILE', 'opengram.dbin NOT_USED'),
           call=('load', 3), userdir=False)
conf_abort('load-library-row-removed', ('-', 'GBK_DICTIONARY'), call=('load', 2), userdir=False)
conf_abort('load-library-gb-not-used', (_STOCK_DBIN, 'gb_char.dbin NOT_USED'), call=('load', 1), userdir=False)
conf_abort('load-library-user-not-used', ('user.bin USER_FILE', 'user.bin NOT_USED'), call=('load', 7),
           userdir=False)
conf_abort('load-addon-system-file', ('art.bin NULL DICTIONARY', 'art.bin NULL SYSTEM_FILE'),
           call=('addon', 4), userdir=False, modes=('pinyin',))
conf_abort('load-addon-user-file', ('art.bin NULL DICTIONARY', 'art.bin NULL USER_FILE'),
           call=('addon', 4), userdir=False, modes=('pinyin',))
conf_abort('load-addon-without-chunk', ('art.table art.bin NULL DICTIONARY', 'art.table NULL NULL DICTIONARY'),
           call=('addon', 4), userdir=False, modes=('pinyin',), dir_chunk='system')
# Class (b): the header's `source table format:` line is missing, and the pin
# compares a buffer it never wrote (undefined behaviour).
conf_abort('source-format-line-missing', ('-', 'source table format'))


# The pin's ordinary `false`: a header directive that does not match answers
# NULL with the raw `load %s failed!` line and no warning.
def header_probe(*edits, issues=('694', '525')):
    def probe(k):
        return {'ctx': bool(k.init(system=conf_system(k, *edits)))}
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    case('table-conf-lambda-line-missing' + _suffix, mode=_mode,
         stderr=True, issues=('694', '525'))(header_probe(('-', 'lambda parameter')))
    # glibc's scanner does not back up: an exponent mark with no digit after
    # it, a `0x` with no hex digit and `infin` fail the `%f` conversion.
    for _name, _spelling in (('exponent-without-digits', '1e'), ('hex-without-digits', '0x'),
                             ('hex-exponent-without-digits', '0x1p'), ('infin', 'infin')):
        # The oracle image's glibc rejects `1e` and `0x1p` (2.43); Ubuntu's
        # 2.39 accepts them, and the pin follows the host libc.
        case('table-conf-lambda-' + _name + _suffix, mode=_mode, stderr=True,
             host_scanf=(_spelling,) if _name.endswith('exponent-without-digits') else (), issues=('694', '525'))(header_probe(('lambda parameter:0.312699', 'lambda parameter:' + _spelling)))


# Every spelling glibc's `%f` accepts, hexadecimal floats included.
for _name, _spelling in (('hex', '0x1p-1'), ('hex-fraction', '0x.8'), ('hex-upper', '0X1.P-1')):
    conf_case('lambda-' + _name, ('lambda parameter:0.312699', 'lambda parameter:' + _spelling),
              modes=('pinyin', 'zhuyin'))



# #919: table.conf scanner partitions. The reference glibc is 2.43-3; the
# cases cover every partition the source walk distinguishes, not only the
# spellings the stock file uses. `control=True` cases already agree with the
# parent; the three incomplete-exponent spellings are the mutation targets.
def conf_919_case(name, mutate, **options):
    for mode in ('pinyin', 'zhuyin'):
        def probe(k, mutate=mutate):
            return conf_session(k, conf_system_bytes(k, mutate))
        case('table-conf-919-' + name + ('-zhuyin' if mode == 'zhuyin' else ''),
             mode=mode, issues=('919',), **options)(probe)


def one_line_conf(data):
    parts = data.split(b'\n', 6)
    return b' '.join(parts[:6]) + b'\n\n' + parts[6]


conf_919_case('header-one-line', one_line_conf, control=True)
conf_919_case('header-crlf', lambda data: data.replace(b'\n', b'\r\n'), control=True)
conf_919_case('header-no-final-newline', lambda data: data.rstrip(b'\n'), control=True)
conf_919_case('header-indented-lambda',
              lambda data: data.replace(b'\nlambda parameter:', b'\n   lambda parameter:'),
              control=True)
conf_919_case('word-nul-truncates',
              lambda data: data.replace(
                  b'gb_char.table gb_char.bin gb_char.dbin',
                  b'gb_char.table gb_char.bin\x00junk gb_char.dbin'),
              control=True)


def header_919(name, *edits, control=True, host_scanf=()):
    for mode in ('pinyin', 'zhuyin'):
        case('table-conf-919-' + name + ('-zhuyin' if mode == 'zhuyin' else ''),
             mode=mode, stderr=True, issues=('919',), control=control,
             host_scanf=host_scanf)(header_probe(*edits))


header_919('lambda-dot-only', ('lambda parameter:0.312699', 'lambda parameter:.'))
header_919('header-leading-space', ('re', r'\Abinary', ' binary'))
header_919('binary-version-line-missing', ('-', 'binary format version'))
header_919('model-version-line-missing', ('-', 'model data version'))

conf_abort('919-database-format-line-missing', ('-', 'database format'), issues=('919',), control=True)
conf_abort('919-source-format-value-missing',
           ('re', r'source table format:\w+', 'source table format:'), issues=('919',), control=True)
conf_abort('919-database-format-value-missing',
           ('re', r'database format:\w+', 'database format:'), issues=('919',), control=True)
conf_abort('919-source-format-line-missing', ('-', 'source table format'), issues=('919',), control=True)
conf_abort('919-word-width-overlong', ('+', 'x' * 300 + ' x.table x.bin x.dbin SYSTEM_FILE'),
           issues=('919',), control=True)

conf_case('919-versions-positive-overflow',
          ('binary format version:7', 'binary format version:9223372036854775808'),
          ('model data version:14', 'model data version:99999999999999999999'),
          issues=('919',), control=False)
conf_case('919-versions-int-wrap',
          ('binary format version:7', 'binary format version:4294967296'),
          ('model data version:14', 'model data version:2147483648'),
          issues=('919',), control=False)
conf_case('919-versions-plus-sign',
          ('binary format version:7', 'binary format version:+7'),
          ('model data version:14', 'model data version:+14'),
          issues=('919',), control=True)
conf_case('919-versions-negative',
          ('binary format version:7', 'binary format version:-1'),
          ('model data version:14', 'model data version:-2'),
          issues=('919',), control=False)
header_919('versions-no-digits', ('binary format version:7', 'binary format version:x'))

for _name, _spelling in (
        ('lambda-hex-no-exponent', '0x1'),
        ('lambda-dot-leading', '.5'),
        ('lambda-exponent-plus', '1e-1'),
        ('lambda-infinity', 'infinity'),
        ('lambda-nan-payload', 'nan(1)'),
        ('lambda-hex-overflow', '0x1p9999'),
        ('lambda-decimal-underflow', '1e-5000'),
        ('lambda-positive-sign', '+0.5')):
    conf_case('919-' + _name, ('lambda parameter:0.312699', 'lambda parameter:' + _spelling),
              issues=('919',), control=True)

for _name, _spelling, _control, _host in (
        ('lambda-exponent-sign-without-digits', '1e+', True, ('1e+',)),
        ('lambda-hex-exponent-sign-without-digits', '0x1p+', True, ('0x1p+',)),
        ('lambda-hex-exponent-minus-without-digits', '0x1p-', True, ('0x1p-',)),
        ('lambda-hex-dot-without-digits', '0x.', True, ()),
        ('lambda-nan-unclosed', 'nan(', True, ())):
    for _mode in ('pinyin', 'zhuyin'):
        _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
        case('table-conf-919-' + _name + _suffix, mode=_mode, stderr=True,
             issues=('919',), control=_control, host_scanf=_host)(
                 header_probe(('lambda parameter:0.312699', 'lambda parameter:' + _spelling)))

# The five-word row plus the next row's first word form a valid six-word
# row with a library/table mismatch; the pin then reads a NULL item and
# SIGSEGVs. The subject answers the same request without the invalid read:
# class (b), recorded as the declared result rather than reproduced.
conf_case('919-short-row-merged',
          ('+', 'default GB_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin'),
          ('+', 'SYSTEM_FILE default RESERVED NULL NULL NULL'),
          issues=('919',), control=True, crash={'train': True, 'save': True},
          stderr_varies=True)
conf_case('919-addon-atoi-nondigit',
          ('+', 'addon abc art.table art.bin NULL DICTIONARY'),
          modes=('pinyin',), session=addon_session, issues=('919',), control=True)
conf_abort('919-addon-positive-overflow',
           ('+', 'addon 99999999999999999999 art.table art.bin NULL DICTIONARY'),
           modes=('pinyin',), issues=('919',), control=True)

# A file name never leaves its directory: `g_build_filename(user_dir,
# "/abs/u.bin")` is `<user_dir>/abs/u.bin`, not `/abs/u.bin`.
def absolute_user_file(k):
    nested = k.user + '/nest'
    os.makedirs(nested)
    os.makedirs(k.user + nested)
    out = conf_session(k, conf_system(k, ('user.bin USER_FILE', nested + '/u.bin USER_FILE')))
    out['beside'] = user_listing(k, nested)
    out['beneath'] = user_listing(k, k.user + nested)
    return out


for _mode in ('pinyin', 'zhuyin'):
    case('table-conf-user-file-absolute' + ('-zhuyin' if _mode == 'zhuyin' else ''), mode=_mode, issues=('694', '525'))(absolute_user_file)

# The `strtol` behind `%d` and `atoi` saturates at LONG_MIN on a negative
# overflow, which the stores into an `int` and a `guint8` cut to 0.
conf_case('versions-negative-overflow', ('binary format version:7', 'binary format version:-9223372036854775808'),
          ('model data version:14', 'model data version:-99999999999999999999'))
conf_case('addon-index-negative-overflow', ('+', 'addon -99999999999999999999 art.table art.bin NULL DICTIONARY'),
          modes=('pinyin',), session=addon_session)

# The names are the bytes the word holds.
conf_case('user-file-not-utf8', ('user.bin USER_FILE', 'u\udcff.bin USER_FILE'))

# `_write_files` writes the library files by sub-index, then the two indices
# and the bigram, and `_rename_files` renames in the same order: where a file
# of the set is given two roles one name, the later writer's contents are what
# the first rename moves into place, and the second rename fails.
def stderr_conf_case(name, *edits):
    for mode in ('pinyin', 'zhuyin'):
        def probe(k, edits=edits):
            return conf_session(k, conf_system(k, *edits))
        case('table-conf-' + name + ('-zhuyin' if mode == 'zhuyin' else ''), mode=mode, stderr=True, issues=('694', '525'))(probe)


stderr_conf_case('colliding-library-files', ('network.bin USER_FILE', 'shared.bin USER_FILE'),
                 ('user.bin USER_FILE', 'shared.bin USER_FILE'))
stderr_conf_case('library-named-like-an-index', ('user.bin USER_FILE', 'user_pinyin_index.bin USER_FILE'))
stderr_conf_case('log-named-like-a-library', ('gb_char.dbin SYSTEM_FILE', 'user.bin SYSTEM_FILE'))


# Class (b): a library whose rows no longer match the index points the
# candidate listing at items that are not there; the pin's duplicate removal
# compares a string it never set (`pinyin.cpp:1635-1637`, `:2053`, SIGSEGV).
def candidates_probe(*edits):
    def probe(k):
        ctx = k.init(system=conf_system(k, *edits))
        assert ctx, 'init failed'
        inst = k.fn('alloc_instance', P, P)(ctx)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, CONF_TEXTS[k.mode][0])
        sentence = k.fn('guess_sentence', B, P)(inst)
        ok = conf_guess_rows(k, inst)[0]
        return {'ret': ok, 'sentence': sentence}
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _domain = WARNING_DOMAIN[_mode]
    for _name, _edits in (
            ('files-swapped', (('gb_char.table gb_char.bin', 'gb_char.table TMP.bin'),
                               ('gbk_char.table gbk_char.bin', 'gbk_char.table gb_char.bin'),
                               ('TMP.bin', 'gbk_char.bin'))),
            ('duplicate-row', (('+', 'default GB_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin SYSTEM_FILE'),)),
            ('library-as-user-file', (('default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE',
                                       'default GB_DICTIONARY NULL NULL gb_char.dbin USER_FILE'),))):
        case('table-conf-candidates-' + _name + ('-zhuyin' if _mode == 'zhuyin' else ''), mode=_mode,
             crash={'ret': False, 'logs': [[_domain, 16]]}, issues=('694', '525'))(candidates_probe(*_edits))



# #594: final-step tails, already ported by 55d861de. The pinyin facade
# exposes up to three indexed sentences; zhuyin exposes its <1, 1> result
# and has no SORT_WITHOUT_SENTENCE_CANDIDATE argument. Keep complete lists.
def final_step_sentences(k, inst):
    if k.mode == 'zhuyin':
        ret, text = zhuyin_sentence_out(k, inst)
        return [text] if ret else []
    sentences = []
    for index in range(3):
        # The pin asserts on a missing index when it has any results
        # (pinyin.cpp:1470-1474). Probe a forked snapshot so count and
        # duplicate texts are measured without killing the case worker.
        read_fd, write_fd = os.pipe()
        pid = os.fork()
        if pid == 0:
            try:
                os.close(read_fd)
                with open(os.devnull, 'w') as quiet:
                    os.dup2(quiet.fileno(), 2)
                with os.fdopen(write_fd, 'w') as output:
                    json.dump(sentence_out(k, inst, index), output)
                os._exit(0)
            except BaseException:
                os._exit(1)
        os.close(write_fd)
        with os.fdopen(read_fd) as source:
            payload = source.read()
        _, status = os.waitpid(pid, 0)
        exit_code = os.waitstatus_to_exitcode(status)
        if exit_code == -signal.SIGABRT:
            break
        assert exit_code == 0 and payload, (
            f'sentence probe failed (exit code {exit_code})')
        ret, text = json.loads(payload)
        if not ret:
            break
        sentences.append(text)
    return sentences


def final_step_surface(text, options, imported=False):
    def probe(k):
        assert k.fn('set_options', B, P, U)(k.ctx, options)
        if imported:
            # USER_DICTIONARY; the zhuyin importer takes zhuyin spelling.
            reading = "ba'kua" if k.mode == 'pinyin' else 'ㄅㄚ ㄎㄨㄚ'
            it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
            assert it
            assert k.fn('iterator_add_phrase', B, P, S, S, I)(
                it, '罢跨'.encode(), reading.encode(), 5)
            k.fn('end_add_phrases', None, P)(it)
        inst = k.inst
        parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text.encode())
        guessed = k.fn('guess_sentence', B, P)(inst)
        sentences = final_step_sentences(k, inst)
        lists = {}
        if k.mode == 'pinyin':
            for sort in (0, 1, 0x1c, 0x1d, 0x1e, 0x1f):
                window = guess_rows(k, inst, 0, sort)
                # Include the original tail index even when duplicate
                # sentence strings collapse into one displayed row.
                indexes = []
                for ordinal, row in enumerate(window['rows']):
                    index = None
                    if row and row[0] == 1:
                        cand, rank = P(), C.c_ubyte(0xff)
                        assert k.fn('get_candidate', B, P, U, C.POINTER(P))(
                            inst, ordinal, C.byref(cand))
                        assert k.fn('get_candidate_nbest_index', B, P, P, C.POINTER(C.c_ubyte))(
                            inst, cand, C.byref(rank))
                        index = rank.value
                    indexes.append(index)
                window['nbest_indexes'] = indexes
                lists[hex(sort)] = window
        else:
            lists['after-cursor'] = zhuyin_guess_rows(k, inst, 0)
        return dict(parsed=parsed, guessed=guessed, nbest_count=len(sentences),
                    sentences=sentences, candidates=lists)
    return probe


FINAL_STEP_INPUTS = ("ba'kua", "li'shi", "xi'an", "ni'hao", "ba'ku", "ni'h",
                     "nihao'", 'n', 'nih', 'ni', 'nihao', 'zhongguo')
for _mode in ('pinyin', 'zhuyin'):
    for _options in (1 << 5, (1 << 5) | (1 << 3)):
        for _text in FINAL_STEP_INPUTS:
            case('final-step-%s-%s-%x' % (_mode, _text, _options),
                 mode=_mode, control=True, issues=('594',))(final_step_surface(_text, _options))
    case('final-step-%s-import-ba-kua' % _mode, mode=_mode, control=True, issues=('594',))(
        final_step_surface("ba'kua", 1 << 5, imported=True))


# #728: mask_out rebuilds loaded system sub-indexes from disk, then merges
# the saved user log with the mask (074a2219 pinyin.cpp:1253-1285,
# zhuyin.cpp:788-820). The separate phrase table keeps the imported token,
# so token_get_phrase, rather than lookup_tokens, observes the reload.
def token_phrase_of(k, token):
    length, text = U(), P()
    ret = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
        k.inst, token, C.byref(length), C.byref(text))
    return [ret, length.value, k.text(text.value)]


def mask_out_system_reload(k, saved, mask=0xFFFFFFFF, selected=False, atomic=False,
                           headerless=False, dropped=False):
    phrase = '龘龘'
    assert not tokens_of(k, phrase), 'the setup phrase already exists'
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 1)
    assert it, 'the import iterator did not open'
    reading = b"da2'da2" if k.mode == 'pinyin' else 'ㄉㄚˊ ㄉㄚˊ'.encode()
    added = k.fn('iterator_add_phrase', B, P, S, S, I)(it, phrase.encode(), reading, 1)
    k.fn('end_add_phrases', None, P)(it)
    assert added, 'the setup phrase did not add'
    tokens = tokens_of(k, phrase)
    assert len(tokens) == 1 and tokens[0] >> 24 == 1, 'the import used the wrong library'

    out = {'added': added, 'before': token_phrase_of(k, tokens[0])}
    if saved:
        out['save'] = k.fn('save', B, P)(k.ctx)
        assert out['save'], 'the setup save failed'
    if headerless:
        # Save emitted the 18-byte MODIFY_HEADER first. Without it,
        # _peek_header returns an empty logger before ADD is re-appended.
        payload = Path(k.user, 'gb_char.dbin').read_bytes()[8:]
        assert int.from_bytes(payload[:4], 'little') == 4
        _write_log(k, payload[18:])
    if atomic:
        # A second library has an unsaved item, so reloading it before the
        # guard would observably change the refused call's state.
        it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 2)
        other_phrase = '龘龘龘'
        other_reading = b"da2'da2'da2" if k.mode == 'pinyin' else 'ㄉㄚˊ ㄉㄚˊ ㄉㄚˊ'.encode()
        assert k.fn('iterator_add_phrase', B, P, S, S, I)(it, other_phrase.encode(), other_reading, 1)
        k.fn('end_add_phrases', None, P)(it)
        other_token = tokens_of(k, other_phrase)[0]
        other_item = token_phrase_of(k, other_token)
        files = transient_files(k.user)
        assert k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, 0x01001225, 7)
        frequency = unigram_of(k, 0x01001225)
    value = tokens[0] & 0x00FFFFFF if selected else 0
    out['ret'] = k.fn('mask_out', B, P, U, U)(k.ctx, mask, value)
    out['after'] = token_phrase_of(k, tokens[0])
    out['phrase-table-before'] = tokens
    out['phrase-table-after'] = tokens_of(k, phrase)
    if dropped:
        # A phrase-table token is not a live sub-index item. The pin refuses
        # the item update and skips it when pricing a pronunciation hit
        # (phrase_index.cpp:150-174; phonetic_lookup.h:646-647 at 074a2219).
        out['add-unigram'] = k.fn('token_add_unigram_frequency', B, P, U, U)(
            k.inst, tokens[0], 7)
        out['unigram'] = unigram_of(k, tokens[0])
        out['parsed'] = k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b"da2'da2")
        out['guessed'] = k.fn('guess_sentence', B, P)(k.inst)
        # Sentence lookup prices pronunciation hits only after checking
        # get_phrase_item. Candidate-list formatting dereferences the pin's
        # dangling table token, so use this defined lookup path instead.
        ret, sentence = (sentence_out(k, k.inst, 0) if k.mode == 'pinyin' else
                         zhuyin_sentence_out(k, k.inst))
        out['lookup'] = [ret, phrase in (sentence or '')]
    if atomic:
        out.update({'untouched-item': out['before'] == out['after'],
                    'untouched-other-library': token_phrase_of(k, other_token) == other_item,
                    'untouched-phrase-table': tokens == out['phrase-table-after'],
                    'untouched-files': transient_files(k.user) == files,
                    'untouched-frequency': unigram_of(k, 0x01001225) == frequency})
    return out


for _mode in ('pinyin', 'zhuyin'):
    case('mask-out-system-unsaved-' + _mode, mode=_mode, issues=('728',))(
        lambda k: mask_out_system_reload(k, False))
    case('mask-out-system-saved-' + _mode, mode=_mode, issues=('728',))(
        lambda k: mask_out_system_reload(k, True))
    case('abort-mask-out-surviving-add-' + _mode, mode=_mode, abort=False, issues=('728',))(
        lambda k: mask_out_system_reload(k, True, 0x00FFFFFF, atomic=True))
    case('mask-out-system-selected-add-' + _mode, mode=_mode, control=True, issues=('728',))(
        lambda k: mask_out_system_reload(k, True, 0x00FFFFFF, selected=True))
    case('mask-out-system-headerless-add-' + _mode, mode=_mode, issues=('728',))(
        lambda k: mask_out_system_reload(k, True, 0x00FFFFFF, headerless=True))
    case('mask-out-dropped-add-full-' + _mode, mode=_mode, issues=('728',))(
        lambda k: mask_out_system_reload(k, True, dropped=True))
    case('mask-out-dropped-add-headerless-' + _mode, mode=_mode, issues=('728',))(
        lambda k: mask_out_system_reload(k, True, 0x00FFFFFF, headerless=True, dropped=True))


def mask_out_system_modify(k, header=True, disk=False):
    # MODIFY passes both non-null pointers supplied by _mask_out_records;
    # ADD does not (phrase_index_logger.h:245, :267-268). Keep the item
    # size and unigram unchanged, replacing only 你's one-character text.
    token = 0x01001225
    payload = Path(k.data, 'gb_char.bin').read_bytes()[8:]
    word = lambda offset: int.from_bytes(payload[offset:offset + 4], 'little')
    total, index, content = word(0), word(4), word(8)
    offset = content + word(index + (token & 0x00FFFFFF) * 4)
    length, prons = payload[offset:offset + 2]
    size = 6 + length * 4 + prons * (length * 2 + 4)
    old = payload[offset:offset + size]
    assert length == 1 and int.from_bytes(old[6:10], 'little') == ord('你')
    new = old[:6] + ord('龘').to_bytes(4, 'little') + old[10:]
    record = ((3).to_bytes(4, 'little') + (token & 0x00FFFFFF).to_bytes(4, 'little') +
              len(old).to_bytes(2, 'little') + len(new).to_bytes(2, 'little') + old + new)
    if disk:
        system = private_system(k)
        k._ctx = k.init(system=system)
        assert k._ctx, 'init failed'
    before = token_phrase_of(k, token)  # initialise before changing disk state
    tokens = tokens_of(k, '你')
    if disk:
        # Replace only a scratch symlink: the reference oracle is read-only.
        path = Path(system, 'gb_char.bin')
        path.unlink()
        path.write_bytes(_chunk(payload[:offset] + new + payload[offset + size:]))
    else:
        _write_log(k, (_modify_header(0, total, total) if header else b'') + record)
    ret = k.fn('mask_out', B, P, U, U)(k.ctx, 0xFFFFFFFF if disk else 0x00FFFFFF, 0)
    return {'ret': ret, 'before': before, 'after': token_phrase_of(k, token),
            'phrase-table-before': tokens, 'phrase-table-after': tokens_of(k, '你')}


for _mode in ('pinyin', 'zhuyin'):
    case('mask-out-system-modify-' + _mode, mode=_mode, issues=('728',))(mask_out_system_modify)
    # Without MODIFY_HEADER the pin returns an empty logger, so even a
    # valid MODIFY record is not merged (phrase_index.cpp:855-856).
    case('mask-out-system-no-header-' + _mode, mode=_mode, control=True, issues=('728',))(
        lambda k: mask_out_system_modify(k, False))
    case('mask-out-system-disk-replacement-' + _mode, mode=_mode, issues=('728',))(
        lambda k: mask_out_system_modify(k, disk=True))


# Batch F (#525): the phrase-index logger's two assert sites. A
# `SYSTEM_FILE`/`DICTIONARY` library's user `.dbin` is a `MemoryChunk` of
# `PhraseIndexLogger` records. `next_record` asserts a `MODIFY_HEADER`
# record's token is `null_token` (`phrase_index_logger.h:202`), reached both
# at init's `merge` and at `mask_out`'s `merge_with_mask`; `_peek_header`
# asserts at most one header (`phrase_index.cpp:745`), reached only by
# `merge_with_mask`, so a multi-header `.dbin` loads at init but refuses a
# mask. `_peek_header` counts the headers it read before `next_record`
# answers false on a malformed record and asserts on that accumulated
# total, so a corrupt tail after two headers aborts too; the mask loop
# only walks libraries `get_range` still holds, so an unloaded library's
# `.dbin` is never merged. A `.dbin` is a backend-independent `MemoryChunk`,
# so those fixtures are the same in every cell; the zhuyin import cases craft
# the user phrase table in the running cell's own format (`_craft_db`).
def _chunk(payload):
    """A `MemoryChunk` image: the length and checksum header words
    (`memory_chunk.h:543-547`) then `payload`."""
    checksum = 0
    aligned = len(payload) & ~0x3
    for i in range(0, aligned, 4):
        checksum ^= int.from_bytes(payload[i:i + 4], 'little')
    for shift, byte in enumerate(payload[aligned:]):
        checksum ^= byte << (8 * shift)
    return (len(payload).to_bytes(4, 'little') +
            (checksum & 0xFFFFFFFF).to_bytes(4, 'little') + payload)


def _modify_header(token, old_total, new_total):
    """One `LOG_MODIFY_HEADER` record: the type (4), the token, a `u16`
    length, then the old and new total runs."""
    return ((4).to_bytes(4, 'little') + token.to_bytes(4, 'little') +
            (4).to_bytes(2, 'little') + old_total.to_bytes(4, 'little') +
            new_total.to_bytes(4, 'little'))


def _write_log(k, payload, name='gb_char.dbin'):
    Path(k.user, name).write_bytes(_chunk(payload))


def logger_init_probe(k, payload):
    """A `.dbin` crafted before the (only) init. `_write_user_conf` keeps
    `check_format` from unlinking it first, as `pinyin_init`'s own rewrite
    already does for the pinyin cases and `zhuyin_init`'s does not."""
    _write_user_conf(k)
    _write_log(k, payload)
    return {'ret': bool(k.init())}


def logger_mask_out_probe(k, payload):
    """A `.dbin` corrupted after a successful init, then a mask: the pin's
    `merge_with_mask` walks the file it re-reads, so this reaches the site
    the load-time snapshot does not."""
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'init failed'
    _write_log(k, payload)
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0, 0)}


def logger_mask_out_unloaded_probe(k, payload):
    """A `.dbin` for a library the call unloads first: the pin's mask loop
    `continue`s on `get_range`'s `ERROR_NO_SUB_PHRASE_INDEX`
    (`pinyin.cpp:1239-1240`, `zhuyin.cpp:796-797`), so the file is never
    merged and the mask completes where the same payload in a loaded
    library refuses it. Index 2 is the one both facades let go
    (`pinyin_unload_phrase_library`'s `GBK_DICTIONARY`), and its user file
    is `gbk_char.dbin`."""
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'init failed'
    _write_log(k, payload, 'gbk_char.dbin')
    assert k.fn('unload_phrase_library', B, P, C.c_ubyte)(ctx, 2), 'the unload failed'
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0, 0)}


def logger_mask_out_untouched(k, payload):
    """A refused mask leaves prior state byte-for-byte: the subject
    validates every `.dbin` before it changes anything, where the pin's
    partial reloads die with its abort."""
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'init failed'
    reading = b"ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode()
    it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
    added = k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), reading, 1)
    k.fn('end_add_phrases', None, P)(it)
    assert added, 'the setup phrase did not add'
    assert k.fn('save', B, P)(ctx)
    k._ctx = ctx
    _write_log(k, payload)
    files = transient_files(k.user)
    tokens = list(tokens_of(k, '你好'))
    out = {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0, 0),
           'untouched-tokens': list(tokens_of(k, '你好')) == tokens,
           'untouched-files': transient_files(k.user) == files}
    return out


_HEADER_TOKEN = _modify_header(0x01000001, 1, 1)
_TWO_HEADERS = _modify_header(0, 1, 1) + _modify_header(0, 1, 2)
# A full record head with an invalid type: `next_record` reads the type and
# the token, hits its `default`, and answers false without advancing the
# header count (`phrase_index_logger.h:214-217`).
_BAD_TAIL = (0x99).to_bytes(4, 'little') + (0).to_bytes(4, 'little')
_TWO_HEADERS_BAD_TAIL = _TWO_HEADERS + _BAD_TAIL
_ONE_HEADER_BAD_TAIL = _modify_header(0, 1, 2) + _BAD_TAIL

for _mode in ('pinyin', 'zhuyin'):
    case('abort-init-log-header-token-' + _mode, mode=_mode, abort=False, issues=('525',))(
        lambda k: logger_init_probe(k, _HEADER_TOKEN))
    case('abort-mask-out-log-header-token-' + _mode, mode=_mode, abort=False, issues=('525',))(
        lambda k: logger_mask_out_probe(k, _HEADER_TOKEN))
    case('abort-mask-out-multiple-log-headers-' + _mode, mode=_mode, abort=False, issues=('525',))(
        lambda k: logger_mask_out_probe(k, _TWO_HEADERS))
    # `_peek_header` counts the headers it read before the malformed tail
    # ends the walk and asserts on the total (`phrase_index.cpp:721-746`),
    # so two headers followed by an invalid record still abort there; the
    # pre-fix Err branch discarded the count and let the mask through.
    case('abort-mask-out-multiple-log-headers-bad-tail-' + _mode, mode=_mode, abort=False, issues=('525',))(
        lambda k: logger_mask_out_probe(k, _TWO_HEADERS_BAD_TAIL))
    case('abort-mask-out-log-header-untouched-' + _mode, mode=_mode, abort=False, issues=('525',))(
        lambda k: logger_mask_out_untouched(k, _TWO_HEADERS))
    # The mask loop skips a library the call unloaded, so its corrupt
    # `.dbin` is never merged and the mask completes. The parent (batch D)
    # had no log validation at all, so it also completed here: a control,
    # held against a future `validate_system_logs` that drops the filter.
    case('mask-out-unloaded-library-log-' + _mode, mode=_mode, control=True, issues=('525',))(
        lambda k: logger_mask_out_unloaded_probe(k, _TWO_HEADERS))
    # The pin applies every header at init's `merge` (it only peeks at
    # mask-out time), so a two-header `.dbin` loads cleanly: a control.
    case('init-multiple-log-headers-' + _mode, mode=_mode, control=True, issues=('525',))(
        lambda k: logger_init_probe(k, _TWO_HEADERS))
    # One header before the same tail passes `1 >= header_count`, so the
    # pin and the pre-fix subject both complete: a control that holds the
    # boundary the count-before-error fix must not cross.
    case('mask-out-log-header-bad-tail-' + _mode, mode=_mode, control=True, issues=('525',))(
        lambda k: logger_mask_out_probe(k, _ONE_HEADER_BAD_TAIL))


# #851: a journal REMOVE of a system item takes it out of the session. A
# system library's `.dbin` is merged at init by `SubPhraseIndex::merge`
# (`phrase_index.cpp:439-517` at 074a2219); REMOVE (`:460-475`) calls
# `remove_phrase_item` (`:219-236`), which zeroes the item's offset and takes its
# unigram off the library total. From then on `get_phrase_item` fails for the
# token (`:180-206`): the item is not readable, a unigram add is refused, an
# export skips it and a save diffs the removal out as a REMOVE record under a
# lowered header, while the phrase table still holds the token.
# `_load_phrase_library` ignores `merge`'s answer (`pinyin.cpp:276`).
#
# The probe crafts the `.dbin` before the only init, then observes the state
# after replay: the touched items through the API, a trailing ADD (whether a
# later record applied), and the `gb_char.dbin` a following `save` diffs out of
# the visible items and the library total (`phrase_index.cpp:392-436`), whose
# header carries the total the replay left. With `reopen` it then closes the
# context and opens the saved profile again, which replays what the save wrote.
_LOG_ADD, _LOG_REMOVE, _LOG_MODIFY = 1, 2, 3


def _log_record(kind, token, old=b'', new=b''):
    """One record as `PhraseIndexLogger::next_record` reads it
    (`phrase_index_logger.h:160-209`): ADD and REMOVE carry one length and
    one chunk, MODIFY both lengths and both chunks."""
    head = kind.to_bytes(4, 'little') + token.to_bytes(4, 'little')
    if kind == _LOG_ADD:
        return head + len(new).to_bytes(2, 'little') + new
    if kind == _LOG_REMOVE:
        return head + len(old).to_bytes(2, 'little') + old
    return head + len(old).to_bytes(2, 'little') + len(new).to_bytes(2, 'little') + old + new


def _item_size(item):
    """`get_phrase_item`'s length from the header bytes alone."""
    length, prons = item[0], item[1]
    return 6 + 4 * length + prons * (2 * length + 4)


def _system_items(k, name='gb_char.bin'):
    """A system chunk's total, its items `{slot: bytes}` at their header
    length, and the item-area offsets `{slot: offset}`."""
    payload = Path(k.data, name).read_bytes()[8:]
    word = lambda offset: int.from_bytes(payload[offset:offset + 4], 'little')
    total, index, content = word(0), word(4), word(8)
    items, offsets = {}, {}
    for slot in range((content - index - 1) // 4):
        offset = word(index + slot * 4)
        if offset:
            start = content + offset
            # Only the two header bytes size an item; copying the remaining
            # chunk here for every slot makes fixture setup quadratic.
            size = _item_size(payload[start:start + 2])
            items[slot] = payload[start:start + size]
            offsets[slot] = offset
    return total, items, offsets


def _item_with(item, unigram=None, freq=None):
    """`item` with its unigram and/or its last pronunciation's frequency
    replaced; the size is unchanged."""
    item = bytearray(item)
    if unigram is not None:
        item[2:6] = unigram.to_bytes(4, 'little')
    if freq is not None:
        item[-4:] = freq.to_bytes(4, 'little')
    return bytes(item)


def _flip(item):
    """`item` with its last byte changed: the same size, so only the
    `memcmp` can refuse it."""
    return item[:-1] + bytes((item[-1] ^ 0xFF,))


def _item_text(item):
    return ''.join(chr(int.from_bytes(item[6 + 4 * i:10 + 4 * i], 'little')) for i in range(item[0]))


def _observe_system_item(k, watched, phrase):
    out = {name: [token_phrase_of(k, token), unigram_of(k, token)] for name, token in watched.items()}
    # The phrase table keeps the token whatever the sub-index holds.
    out['phrase-table'] = list(tokens_of(k, phrase))
    out['export'] = remember_export(k, 1, phrase)
    return out


def system_replay(k, build, reopen=False, unigram_add=True, observe_add=False, observe=None,
                   observe_saved=False):
    """`build(p)` returns the records after the header and the tokens to
    watch; `p` maps `one`, `two` (an item of two or more readings) and `last`
    (the item at the highest item-area offset) to (token, bytes) of library 1,
    and `fresh` and `absent` to a token it does not hold: one for a trailing
    ADD, one no record adds."""
    total, items, offsets = _system_items(k)
    one = next(slot for slot, item in items.items() if item[0] == 1 and item[1] == 1)
    two = next(slot for slot, item in items.items() if item[0] == 1 and item[1] >= 2)
    last = max(offsets, key=offsets.get)
    fresh = max(items) + 2
    records, watched = build({'total': total, 'one': (0x01000000 | one, items[one]),
                              'two': (0x01000000 | two, items[two]),
                              'last': (0x01000000 | last, items[last]),
                              'fresh': (0x01000000 | fresh, _item_with(items[one], unigram=11)),
                              'absent': (0x01000000 | (fresh + 3), b''),
                              'items': items, 'offsets': offsets})
    phrase = _item_text(items[one])
    _write_user_conf(k)
    _write_log(k, _modify_header(0, total, total) + b''.join(records))
    out = {'total': total, 'ctx': bool(k.ctx)}
    out.update(_observe_system_item(k, watched, phrase))
    if observe:
        out['detail'] = observe(k)
    if unigram_add:
        # A unigram add reaches only an item the sub-index holds.
        out['add-unigram'] = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, watched['one'], 7)
    if observe_add:
        out['after-add'] = _observe_system_item(k, watched, phrase)
        if k.mode == 'pinyin':
            out['facade-after-add'] = _facade_total_out(k)
    # An import marks the context modified, so the save writes the diff.
    reading = b"ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode()
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    assert k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), reading, 1)
    k.fn('end_add_phrases', None, P)(it)
    out['save'] = k.fn('save', B, P)(k.ctx)
    out['dbin'] = Path(k.user, 'gb_char.dbin').read_bytes().hex()
    if observe_saved:
        header, counts = log_walk(bytes.fromhex(out['dbin']))
        out['saved-header'] = header
        out['saved-counts'] = counts
    if reopen:
        k.fn('free_instance', None, P)(k.inst)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = k._inst = None
        out['reopen'] = _observe_system_item(k, watched, phrase)
        if observe:
            out['reopen-detail'] = observe(k)
    return out


def _replay_case(name, build, reopen=False, unigram_add=True, observe_add=False, observe=None,
                 observe_saved=False, extra_issues=(), **options):
    for mode in ('pinyin', 'zhuyin'):
        case('replay-' + name + '-' + mode, mode=mode, stderr=True,
             issues=('851', '850', '870') + tuple(extra_issues), **options)(
            lambda k, build=build: system_replay(k, build, reopen, unigram_add, observe_add,
                                                 observe, observe_saved))


def _watch(p):
    return {'one': p['one'][0], 'fresh': p['fresh'][0]}


# #853: accepted live-item increments survive a later dirty save.
_replay_case('unigram-add-live-item', lambda p: ([], _watch(p)))
_replay_case('unigram-add-live-item-reopen', lambda p: ([], _watch(p)), reopen=True)
_replay_case('unigram-add-live-item-control', lambda p: ([], _watch(p)), unigram_add=False, control=True)
_replay_case('unigram-add-overridden-item-reopen', lambda p: (
    [_log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                 new=_item_with(p['one'][1], unigram=19))], _watch(p)), reopen=True)


# #853 lifecycle: the rules are read from phrase_index.cpp:150-178,
# :237-269, :392-436, :648-675; pinyin.cpp:956, :1132-1145,
# :1253-1285, and phonetic_lookup.h:925-929 at 074a2219.
def unigram_lifecycle(k, variant):
    system = private_system(k)
    k.data = system
    name = 'gbk_char.bin' if variant == 'reload' else 'gb_char.bin'
    library = 2 if variant == 'reload' else 1
    total, items, offsets = _system_items(k, name)
    slot = next(slot for slot, item in items.items() if item[0] == 1 and item[1] == 1)
    if variant == 'import-system':
        slot = 0x1225  # 你
    token, item = (library << 24) | slot, items[slot]
    _write_user_conf(k)
    if variant == 'removed':
        _write_log(k, _modify_header(0, total, total) +
                   _log_record(_LOG_REMOVE, token, old=item))
    elif variant == 'wrap':
        # A valid layout with an intentionally inconsistent stored total:
        # item arithmetic wraps independently of the guarded library total.
        payload = bytearray(Path(system, name).read_bytes()[8:])
        content = int.from_bytes(payload[8:12], 'little')
        start = content + offsets[slot] + 2
        payload[start:start + 4] = (0xfffffffc).to_bytes(4, 'little')
        path = Path(system, name)
        path.unlink()  # private symlink; never write the source data
        path.write_bytes(_chunk(payload))
    elif variant == 'overflow':
        _write_log(k, _modify_header(0, total, 0xfffffffc))
    assert k.ctx
    if variant == 'import-system':
        reading = 'ni3' if k.mode == 'pinyin' else 'ㄋㄧˇ'
        assert import_rows(k, 1, [('你', reading, 9)]) == [True]
    add = k.fn('token_add_unigram_frequency', B, P, U, U)
    watched = token if variant != 'missing' else ((library << 24) | (max(items) + 20))
    out = {'before': unigram_of(k, watched)}
    out['add'] = add(k.inst, watched, 7)
    out['after'] = unigram_of(k, watched)
    if variant == 'timing':
        started = time.perf_counter_ns()
        for _ in range(1000):
            assert add(k.inst, token, 1)
        out['~increment-ns'] = time.perf_counter_ns() - started
    if variant == 'repeat':
        out['again'] = add(k.inst, watched, 11)
        out['after-again'] = unigram_of(k, watched)
        other = k.alloc()
        frequency = U()
        out['other-instance'] = [k.fn('token_get_unigram_frequency', B, P, U, C.POINTER(U))(
            other, watched, C.byref(frequency)), frequency.value]
        k.fn('free_instance', None, P)(other)
    if variant in ('overflow', 'missing', 'removed') and k.mode == 'pinyin':
        out['facade'] = _facade_total_out(k)
    out['clean-save'] = k.fn('save', B, P)(k.ctx)
    if variant == 'reload':
        out['unload'] = k.fn('unload_phrase_library', B, P, C.c_ubyte)(k.ctx, 2)
        out['unloaded-add'] = add(k.inst, token, 13)
        out['load'] = k.fn('load_phrase_library', B, P, C.c_ubyte)(k.ctx, 2)
        out['after-load'] = unigram_of(k, token)
    elif variant == 'mask':
        out['mask'] = k.fn('mask_out', B, P, U, U)(k.ctx, 0xffffffff, 0)
        out['after-mask'] = unigram_of(k, token)
    elif variant == 'replacement':
        payload = bytearray(Path(system, name).read_bytes()[8:])
        content = int.from_bytes(payload[8:12], 'little')
        start = content + offsets[slot] + 2
        payload[start:start + 4] = (37).to_bytes(4, 'little')
        payload[:4] = (total - int.from_bytes(item[2:6], 'little') + 37).to_bytes(4, 'little')
        path = Path(system, name)
        temporary = Path(str(path) + '.new')
        temporary.write_bytes(_chunk(payload))
        os.replace(temporary, path)
    elif variant == 'import-system':
        out['system-import'] = import_rows(k, 1, [('你', reading, 17)])
    elif variant == 'training':
        trained_token = 0x01003e57
        out['training-add'] = add(k.inst, trained_token, 7)
        out['training'] = token_unigram_trained(k)
    # An import makes save dirty without replacing the watched system item.
    reading = b"ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode()
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    out['import'] = k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), reading, 1)
    k.fn('end_add_phrases', None, P)(it)
    started = time.perf_counter_ns()
    out['save'] = k.fn('save', B, P)(k.ctx)
    if variant == 'timing':
        out['~save-ns'] = time.perf_counter_ns() - started
    log = Path(k.user, name.replace('.bin', '.dbin')).read_bytes()
    out['journal'] = [len(log), hashlib.sha256(log).hexdigest(), log_walk(log)]
    out['after-save'] = unigram_of(k, watched)
    phrase_table_reopen(k)
    out['reopen'] = unigram_of(k, watched)
    out['add-reopened'] = add(k.inst, watched, 5)
    out['after-reopened'] = unigram_of(k, watched)
    if variant == 'repeat':
        it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
        out['second-import'] = k.fn('iterator_add_phrase', B, P, S, S, I)(
            it, '你好'.encode(), reading, 1)
        k.fn('end_add_phrases', None, P)(it)
        out['second-save'] = k.fn('save', B, P)(k.ctx)
        phrase_table_reopen(k)
        out['second-reopen'] = unigram_of(k, watched)
    return out


for _mode in ('pinyin', 'zhuyin'):
    for _variant in ('repeat', 'replacement', 'wrap', 'overflow', 'missing', 'removed',
                     'reload', 'mask', 'training', 'timing', 'import-system'):
        case('unigram-lifecycle-' + _variant + '-' + _mode, mode=_mode, stderr=True,
             control=_variant in ('overflow', 'missing', 'removed', 'reload', 'mask'), issues=('853', '870'))(
            lambda k, variant=_variant: unigram_lifecycle(k, variant))


def unigram_null_user_save(k):
    # The pin's save guards precede compact (pinyin.cpp:1132-1145,
    # zhuyin.cpp:547-554). Both C facades already enforce those guards;
    # direct reporting-helper coverage is a user-store unit test.
    system = private_system(k)
    k.data = system
    payload = bytearray(Path(system, 'gb_char.bin').read_bytes()[8:])
    payload[:4] = (0xfffffffb).to_bytes(4, 'little')
    path = Path(system, 'gb_char.bin')
    path.unlink()  # replace our private symlink, never the oracle data
    path.write_bytes(_chunk(payload))
    k._ctx = k.init(system, None, literal=True)
    assert k._ctx
    token = 0x01000001
    add = k.fn('token_add_unigram_frequency', B, P, U, U)
    out = {'before': unigram_of(k, token)}
    out['near-bound'] = add(k.inst, token, 2)
    out['before-save'] = unigram_of(k, token)
    out['save'] = k.fn('save', B, P)(k.ctx)
    out['across-bound'] = add(k.inst, token, 4)
    out['after-refusal'] = unigram_of(k, token)
    out['pricing'] = _dynamic_adjust_out(k, b'ni3')
    return out


for _mode in ('pinyin', 'zhuyin'):
    case('unigram-null-user-save-' + _mode, mode=_mode, control=True, stderr=True, issues=('853', '870'))(
        unigram_null_user_save)


# #853/#876 overlap: unigram ownership must not erase a replayed reading's
# count, whether the sub-index accepts or refuses the increment. The facade
# still takes the refused delta before dispatch (phrase_index.h:628-636).
_replay_case('unigram-add-changed-reading-reopen', lambda p: (
    [_log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                 new=_item_with(p['one'][1], unigram=19, freq=250))], _watch(p)),
    reopen=True, observe_add=True)
_replay_case('unigram-refused-changed-reading-reopen', lambda p: (
    [_log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                 new=_item_with(p['one'][1], freq=250)),
     _modify_header(0, p['total'], 0xfffffffc)], _watch(p)),
    reopen=True, observe_add=True, control=True)


def _add_fresh(p):
    return _log_record(_LOG_ADD, p['fresh'][0], new=p['fresh'][1])


# A matching REMOVE, then an ADD: the item is gone, the total is lower and the
# ADD after it applied.
_replay_case('unigram-add-appended-item-reopen', lambda p: (
    [_add_fresh(p)], {'one': p['fresh'][0], 'original': p['one'][0]}), reopen=True)


def _matched_remove(p):
    return [_log_record(_LOG_REMOVE, p['one'][0], old=p['one'][1]), _add_fresh(p)], _watch(p)


_replay_case('matched-remove', _matched_remove)
# The same, saved and opened again: the REMOVE the save wrote replays to the
# same state.
_replay_case('matched-remove-reopen', _matched_remove, reopen=True)


# #850: journal replay compares and sizes items as the pin does. `merge`
# compares a record's old chunk with the visible item by `PhraseItem::operator==`
# (equal chunk sizes, then `memcmp`: `phrase_index.h:235-240`); the visible
# item's length comes from its header, `6 + 4L + n(2L + 4)` (`get_phrase_item`,
# `phrase_index.cpp:180-206`), not from the stored payload; `add_phrase_item`
# stores the whole payload, trailing bytes included (`:208-217`). REMOVE deletes
# the item before it compares (`:460-475`), so a mismatch returns false with
# the removal kept. MODIFY compares first (`:477-483`); a chunk longer than the
# visible item is a remove plus an add of the whole new payload, otherwise it
# is `memmove`d over the stored bytes (`:486-500`). The first failure ends the
# merge and the records before it stay applied (`pinyin.cpp:276` ignores the
# answer). The probe is #851's `system_replay`.

def _compare_case(name, build, **options):
    """The probe without its unigram add: a live item's added delta is a
    different path than the compare and sizing these cases hold."""
    _replay_case(name, build, unigram_add=False, **options)


# #870: merge walks the journal in record order, while a later diff walks
# ascending slots (074a2219 phrase_index.cpp:392-436,439-517). Repeated,
# out-of-order writes leave each slot's final value, including after reopen.
def _touched_order(p):
    rows = sorted((p['one'], p['two']), reverse=True, key=lambda row: row[0])
    records = [_log_record(_LOG_MODIFY, token, old=item, new=_item_with(item, freq=77))
               for token, item in rows]
    token, item = rows[-1]
    records.append(_log_record(_LOG_MODIFY, token, old=_item_with(item, freq=77),
                               new=_item_with(item, freq=99)))
    return records, dict(_watch(p), two=p['two'][0])


_compare_case('touched-order-duplicates-reopen', _touched_order, reopen=True)


# #870: unigram candidates are per slot, not a whole-library zero-change
# proof. Counts stay absolute in the pin's mapped items (phrase_index.h:124-126).
def _seed_mixed(p):
    token, item = p['two']
    return [
        _log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                    new=_item_with(p['one'][1], freq=77)),
        _log_record(_LOG_MODIFY, token, old=item, new=_item_with(item, unigram=71)),
        _log_record(_LOG_MODIFY, token, old=_item_with(item, unigram=71),
                    new=_item_with(item, unigram=93)),
    ], dict(_watch(p), two=token)


_compare_case('seed-mixed-repeated-unigrams-reopen', _seed_mixed, reopen=True)
_compare_case('seed-restored-unigram-reopen', lambda p: ([
    _log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                new=_item_with(p['one'][1], unigram=99)),
    _log_record(_LOG_MODIFY, p['one'][0], old=_item_with(p['one'][1], unigram=99),
                new=_item_with(p['one'][1], freq=77)),
], _watch(p)), reopen=True)
_compare_case('seed-add-remove-unigrams-reopen', lambda p: ([
    _add_fresh(p),
    _log_record(_LOG_REMOVE, p['one'][0], old=p['one'][1]),
    _log_record(_LOG_MODIFY, p['two'][0], old=p['two'][1],
                new=_item_with(p['two'][1], unigram=91)),
], dict(_watch(p), two=p['two'][0])), reopen=True)
_compare_case('seed-header-total-reopen', lambda p: ([
    _log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                new=_item_with(p['one'][1], freq=77)),
    _modify_header(0, p['total'], 123456),
], _watch(p)), reopen=True)
_compare_case('seed-stale-storage-unigram-reopen', lambda p: ([
    _log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                new=_item_with(p['one'][1], freq=77)),
    _log_record(_LOG_MODIFY, p['absent'][0], old=_item_with(p['one'][1], freq=77),
                new=_item_with(p['one'][1], unigram=99, freq=77)),
], _watch(p)), reopen=True)


def _touched_sparse_edits(p):
    token, item = p['one']
    moved = _item_with(item, freq=77)
    high = 0x01040000  # sparse slot 262144; no full-token-range fixture
    records = [
        _log_record(_LOG_MODIFY, token, old=item, new=moved + b'\x01\x02\x03'),
        _log_record(_LOG_MODIFY, token, old=moved, new=_item_with(item, freq=99)),
        _log_record(_LOG_ADD, high, new=p['fresh'][1]),
        _log_record(_LOG_REMOVE, token, old=_item_with(item, freq=99)),
        _log_record(_LOG_ADD, token, new=item),
        _log_record(_LOG_REMOVE, high, old=p['fresh'][1]),
        _log_record(_LOG_ADD, high, new=_item_with(p['fresh'][1], freq=55)),
    ]
    return records, dict(_watch(p), high=high)


_compare_case('touched-sparse-add-remove-relocate-reopen', _touched_sparse_edits, reopen=True)


# REMOVE whose old chunk carries a trailing byte: the sizes differ, the item
# is gone already and the merge stops before the ADD.
_compare_case('remove-trailing-old', lambda p: ([
    _log_record(_LOG_REMOVE, p['one'][0], old=p['one'][1] + b'\xaa'), _add_fresh(p)], _watch(p)))

# MODIFY whose old chunk carries a trailing byte: the compare refuses it and
# nothing is changed.
_compare_case('modify-trailing-old', lambda p: ([
    _log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1] + b'\xaa',
                new=_item_with(p['one'][1], freq=77)), _add_fresh(p)], _watch(p)))


# MODIFY whose new chunk is longer than the visible item by trailing bytes:
# the growing path stores all of it, and the item is still its header's
# length, so the next record compares against the header-length bytes.
def _modify_grow(stored_old=False):
    def build(p):
        token, item = p['one']
        new = _item_with(item, unigram=item[2] + 500, freq=5)
        trailing = b'\x01\x02\x03'
        # The second record's old chunk is the item at its header length, or
        # with the trailing bytes the first record stored.
        second = _item_with(new, freq=9)
        return ([_log_record(_LOG_MODIFY, token, old=item, new=new + trailing),
                 _log_record(_LOG_MODIFY, token, old=new + (trailing if stored_old else b''), new=second),
                 _add_fresh(p)], _watch(p))
    return build


_compare_case('modify-trailing-new-grow', _modify_grow(), control=True)
_compare_case('modify-trailing-new-grow-stored-old', _modify_grow(True))


# The in-place path: a new chunk with a pronunciation fewer and trailing bytes
# that still fits the old size is `memmove`d over the item. Its header now
# gives a shorter item, so the second record compares against that, and not
# against the bytes the memmove left behind it.
def _modify_in_place(stored_old=False):
    def build(p):
        token, item = p['two']
        new = bytes((item[0], item[1] - 1)) + item[2:-6]
        assert _item_size(new) == len(item) - 6
        trailing = b'\x01\x02\x03\x04'
        second = _item_with(new, freq=9)
        return ([_log_record(_LOG_MODIFY, token, old=item, new=new + trailing),
                 _log_record(_LOG_MODIFY, token, old=new + (trailing if stored_old else b''), new=second),
                 _add_fresh(p)], _watch(p))
    return build


_compare_case('modify-trailing-new-in-place', _modify_in_place(), control=True)
_compare_case('modify-trailing-new-in-place-stored-old', _modify_in_place(True))


# An empty phrase is an item of `6 + 4n` bytes. In place, then once more
# against the shorter item.
def _modify_empty(p):
    token, item = p['one']
    empty = bytes((0, 0)) + item[2:6]
    return ([_log_record(_LOG_MODIFY, token, old=item, new=empty),
             _log_record(_LOG_MODIFY, token, old=empty, new=_item_with(empty, unigram=3)),
             _add_fresh(p)], _watch(p))


_compare_case('modify-empty-phrase', _modify_empty)

# A mismatch ends the merge: the record before it applied, the one after it
# did not.
_compare_case('mismatch-then-valid', lambda p: ([
    _log_record(_LOG_MODIFY, p['two'][0], old=p['two'][1], new=_item_with(p['two'][1], freq=21)),
    _log_record(_LOG_MODIFY, p['one'][0], old=_flip(p['one'][1]), new=_item_with(p['one'][1], freq=33)),
    _log_record(_LOG_MODIFY, p['two'][0], old=_item_with(p['two'][1], freq=21),
                new=_item_with(p['two'][1], freq=22)),
    _add_fresh(p)], _watch(p)))

# A REMOVE whose old chunk has the right size and the wrong bytes: the item
# and its unigram are gone from the total already, and the merge stops.
_compare_case('failed-remove', lambda p: ([
    _log_record(_LOG_REMOVE, p['one'][0], old=_flip(p['one'][1])), _add_fresh(p)], _watch(p)))

# A record for a token the library does not hold. `get_phrase_item` fails
# (`ERROR_OUT_OF_RANGE`, `phrase_index.cpp:187-189`) and `merge` goes on with
# what its result leaves behind: REMOVE dereferences the NULL `tmpitem`
# (`:468`, `remove_phrase_item` `:219-224` leaves it unset); MODIFY ignores
# the failure and compares a stale `item` (`:478-483`), the default one with no
# earlier MODIFY, else the last item a MODIFY fetched.
def _watch_absent(p):
    return dict(_watch(p), absent=p['absent'][0])


# The pin dies of SIGSEGV at init; the subject opens, goes on past the
# record and answers the library as it holds it.
_NO_ITEM = [[False, 0, None], [False, 0]]
_compare_case('remove-absent', lambda p: ([
    _log_record(_LOG_REMOVE, p['absent'][0], old=p['one'][1]), _add_fresh(p)], _watch_absent(p)),
    crash={'ctx': True, 'absent': _NO_ITEM, 'one': [[True, 1, '锕'], [True, 3]],
           'fresh': [[True, 1, '锕'], [True, 11]]})

_compare_case('modify-absent', lambda p: ([
    _log_record(_LOG_MODIFY, p['absent'][0], old=p['one'][1],
                new=_item_with(p['one'][1], freq=77)), _add_fresh(p)], _watch_absent(p)))


# After a MODIFY of `two`, the stale `item` is `two`'s bytes. A MODIFY of the
# absent token whose old chunk equals them passes the compare: the same-size
# new chunk is `memmove`d over `two` (`:495-500`), the growing one meets the
# NULL of REMOVE's assert (`:486-491`).
def _modify_absent_stale(grow):
    def build(p):
        token, item = p['two']
        first = _item_with(item, freq=21)
        new = _item_with(first, freq=22)
        return ([_log_record(_LOG_MODIFY, token, old=item, new=first),
                 _log_record(_LOG_MODIFY, p['absent'][0], old=first,
                             new=new + (b'\x01' if grow else b'')),
                 _add_fresh(p)], _watch_absent(p))
    return build


_compare_case('modify-absent-stale', _modify_absent_stale(False))
# The growing form reaches REMOVE's NULL: the pin dies of SIGSEGV, the subject
# stops at the record and keeps what the first MODIFY did.
_compare_case('modify-absent-stale-grow', _modify_absent_stale(True),
             crash={'ctx': True, 'absent': _NO_ITEM, 'fresh': _NO_ITEM,
                    'one': [[True, 1, '锕'], [True, 3]]})


# A MODIFY's new chunk is not checked against its own header: only the old
# chunk is compared (`phrase_index.cpp:481-483`), and a new chunk no longer
# than the visible item is `memmove`d over the stored bytes (`:493-500`). The
# stored item keeps its length, so its header still names the whole item.
_compare_case('modify-short-in-place', lambda p: ([
    _log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                new=p['one'][1][:2] + (200).to_bytes(4, 'little')),
    _add_fresh(p)], _watch(p)))


# The default `PhraseItem` is six zero bytes (`phrase_index.h:81-84`), not an
# empty item: a MODIFY of an absent token whose old chunk is those zeros passes
# the compare with no earlier MODIFY, and a new chunk that fits the six bytes
# is written into `item`'s own buffer. No entry is created and the merge goes
# on; a later compare sees what the write left there.
_ZERO_ITEM = bytes(6)


def _modify_absent_default(twice=False):
    def build(p):
        first = bytes((0, 0)) + (50).to_bytes(4, 'little')
        records = [_log_record(_LOG_MODIFY, p['absent'][0], old=_ZERO_ITEM, new=first)]
        if twice:
            second = bytes((0, 0)) + (60).to_bytes(4, 'little')
            records.append(_log_record(_LOG_MODIFY, p['absent'][0], old=first, new=second))
        records.append(_add_fresh(p))
        return records, _watch_absent(p)
    return build


_compare_case('modify-absent-default', _modify_absent_default())
_compare_case('modify-absent-default-twice', _modify_absent_default(True))
# A new chunk longer than the six-byte default reaches the NULL of REMOVE's
# assert (`:486-491`): the pin dies of SIGSEGV, the subject stops at the record.
_compare_case('modify-absent-default-grow', lambda p: ([
    _log_record(_LOG_MODIFY, p['absent'][0], old=_ZERO_ITEM,
                new=bytes((0, 0)) + (50).to_bytes(4, 'little') + b'\x00'),
    _add_fresh(p)], _watch_absent(p)),
    crash={'ctx': True, 'absent': _NO_ITEM, 'fresh': _NO_ITEM,
           'one': [[True, 1, '锕'], [True, 3]]}, control=True)


# A small over-long header on a non-last item: the compare passes, the
# six-byte chunk is written in place, and the next read takes the twenty-two
# bytes its header names, six of them from the following item. That read ends
# inside the content buffer, so the pin's answer is reproduced.
_compare_case('modify-header-overlong-small', lambda p: ([
    _log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                new=bytes((2, 1)) + (200).to_bytes(4, 'little')),
    _add_fresh(p)], _watch(p)))


# #981: the same borrowed bytes, one record later. A first MODIFY leaves a
# six-byte payload whose header claims two readings (30 bytes), so slot A's
# header-derived item spans the adjacent slot B. A second in-place MODIFY of A
# compares the 30 bytes and rewrites them with B's unigram changed; B has no
# record of its own. The replay view exposes only the slots it touched
# (`ReplayedLibrary::contains`), so a reader that falls back to the original
# item for B answers B's original unigram, while the pin's `get_phrase_item`
# reads the changed bytes. A is a one-character, one-reading slot so the fixed
# 30-byte header always spans B; B's item must hold those 14 bytes. In the
# pinned gb_char.bin the pair is slot 1 (token 0x01000001, 锕, unigram 3) and
# slot 2 (token 0x01000002, 吖, two readings, unigram 11).
def _cross_item_pair(p):
    """The first adjacent pair whose A is a one-character, one-reading item
    and whose B holds the 14 bytes A's (2, 2) header borrows."""
    ordered = sorted(p['offsets'].items(), key=lambda pair: pair[1])
    for (slot_a, offset_a), (slot_b, offset_b) in zip(ordered, ordered[1:]):
        item_a = p['items'][slot_a]
        item_b = p['items'][slot_b]
        if item_a[0] != 1 or item_a[1] != 1 or offset_b != offset_a + len(item_a):
            continue
        # The synthesized second phrase character is A's first pronunciation
        # key plus its frequency's low half; keep it a Unicode scalar so the
        # pin's `g_ucs4_to_utf8` has a value to render.
        second = int.from_bytes(item_a[10:14], 'little')
        if second > 0x10FFFF or 0xD800 <= second <= 0xDFFF or len(item_b) < 14:
            continue
        return (0x01000000 | slot_a, item_a, 0x01000000 | slot_b, item_b)
    raise AssertionError('no adjacent one-reading items the header can span')


def _cross_item_records(p):
    token_a, item_a, token_b, item_b = _cross_item_pair(p)
    expanded = bytes((2, 2)) + (200).to_bytes(4, 'little')
    claimed = _item_size(expanded[:2])
    borrowed = claimed - len(item_a)
    assert claimed == 30 and 0 < borrowed <= len(item_b)
    # The first in-place write copies only the six-byte payload, so the rest
    # of A's slot keeps its bytes and the read borrows B's first 14.
    old = expanded + item_a[6:] + item_b[:borrowed]
    assert len(old) == claimed
    at = len(item_a) + 2  # B's unigram, relative to A
    original = int.from_bytes(old[at:at + 4], 'little')
    changed = 170 if original != 170 else 171
    new = old[:at] + changed.to_bytes(4, 'little') + old[at + 4:]
    return ([_log_record(_LOG_MODIFY, token_a, old=item_a, new=expanded),
             _log_record(_LOG_MODIFY, token_a, old=old, new=new),
             _add_fresh(p)],
            {'a': token_a, 'b': token_b, 'fresh': p['fresh'][0]})


def _cross_item_records_past_area(p):
    token_a, item_a, token_b, item_b = _cross_item_pair(p)
    expanded = bytes((2, 3)) + (200).to_bytes(4, 'little')
    claimed = _item_size(expanded[:2])
    assert claimed == 38
    fresh = p['fresh'][1]
    borrowed = claimed - len(item_a)
    old = expanded + item_a[6:] + item_b + fresh[:borrowed - len(item_b)]
    assert len(old) == claimed
    at = len(item_a) + 2
    original = int.from_bytes(old[at:at + 4], 'little')
    changed = 170 if original != 170 else 171
    new = old[:at] + changed.to_bytes(4, 'little') + old[at + 4:]
    # The ADD comes first so the last original item B is followed by the fresh
    # item; A's 34-byte header reaches past the original area into it.
    return ([_add_fresh(p),
             _log_record(_LOG_MODIFY, token_a, old=item_a, new=expanded),
             _log_record(_LOG_MODIFY, token_a, old=old, new=new)],
            {'a': token_a, 'b': token_b, 'fresh': p['fresh'][0]})


def _observe_replayed_item(k, token):
    phrase = token_phrase_of(k, token)
    text = phrase[2] if phrase[0] else None
    return {'token': '0x%08x' % token,
            'phrase': phrase,
            'n-pron': _reload_pron_keys(k, token),
            'unigram': unigram_of(k, token),
            'export': remember_export(k, 1, text) if text else 'NO PHRASE'}


def _observe_cross_item(k):
    _total, items, offsets = _system_items(k)
    token_a, _item_a, token_b, _item_b = _cross_item_pair({'items': items, 'offsets': offsets})
    return {'a': _observe_replayed_item(k, token_a), 'b': _observe_replayed_item(k, token_b)}


_compare_case('cross-item-unigram-unrecorded', _cross_item_records,
              observe=_observe_cross_item, observe_saved=True, extra_issues=('981',))
_compare_case('cross-item-unigram-unrecorded-reopen', _cross_item_records, reopen=True,
              observe=_observe_cross_item, observe_saved=True, extra_issues=('981',))
_compare_case('cross-item-unigram-unrecorded-past-area', _cross_item_records_past_area,
              observe=_observe_cross_item, observe_saved=True, extra_issues=('981',))


# The pin does not check a new chunk's header against its size: the compare is
# on the old chunk only. A stored item whose header then names more bytes than
# were written is read past by a later `get_phrase_item` (class (b); row 107).
# The last item of the item area sits at the end of the pin's `m_phrase_content`
# copy once the trailing ADD has appended: a 132,096-byte header (255, 255)
# reads past the item, the copy and its allocation, and the pin dies of
# SIGSEGV at the first read. The subject keeps its safe answer: it applies the
# write and the ADD, and the over-read item is unreadable at the finish.
def _watch_last(p):
    return dict(_watch(p), last=p['last'][0])


_compare_case('modify-header-overlong', lambda p: ([
    _log_record(_LOG_MODIFY, p['last'][0], old=p['last'][1],
                new=bytes((255, 255)) + (200).to_bytes(4, 'little')),
    _add_fresh(p)], _watch_last(p)),
    crash={'ctx': True, 'fresh': [[True, 1, '锕'], [True, 11]],
           'one': [[True, 1, '锕'], [True, 3]],
           'last': [[True, 14, '中华人民共和国中央军事委员会'], [True, 62]]})


# Same row-107 refusal after a duplicate touch. The pin still over-reads the
# last slot; the safe finish reports it once and keeps that slot's baseline.
_compare_case('touched-duplicate-invalid', lambda p: ([
    _log_record(_LOG_MODIFY, p['last'][0], old=p['last'][1],
                new=_item_with(p['last'][1], freq=77)),
    _log_record(_LOG_MODIFY, p['last'][0], old=_item_with(p['last'][1], freq=77),
                new=bytes((255, 255)) + (200).to_bytes(4, 'little')),
    _add_fresh(p)], _watch_last(p)),
    crash={'ctx': True, 'fresh': [[True, 1, '锕'], [True, 11]],
           'one': [[True, 1, '锕'], [True, 3]],
           'last': [[True, 14, '中华人民共和国中央军事委员会'], [True, 62]]})


# Many row-107 invalidations, in descending order and each after an earlier
# valid touch. Finalization must emit each diagnostic once in ascending order;
# membership and item reads fall back to the original for every rejected slot.
def touched_many_invalid(k):
    _, items, offsets = _system_items(k)
    slots = sorted(offsets, key=offsets.get)[-256:]

    def build(p):
        records = []
        for slot in reversed(slots):
            token, item = 0x01000000 | slot, items[slot]
            changed = _item_with(item, freq=77)
            records.extend([
                _log_record(_LOG_MODIFY, token, old=item, new=changed),
                _log_record(_LOG_MODIFY, token, old=changed,
                            new=bytes((255, 255)) + (200).to_bytes(4, 'little')),
            ])
        return records + [_add_fresh(p)], _watch_last(p)

    return system_replay(k, build, unigram_add=False)


for _mode in ('pinyin', 'zhuyin'):
    case('replay-touched-many-invalid-' + _mode, mode=_mode, stderr=True,
         issues=('850', '851', '870'),
         crash={'ctx': True, 'fresh': [[True, 1, '锕'], [True, 11]],
                'one': [[True, 1, '锕'], [True, 3]],
                'last': [[True, 14, '中华人民共和国中央军事委员会'], [True, 62]]})(
                    touched_many_invalid)


# An ADD payload shorter than the six bytes `add_phrase_item` reads its
# unigram from: `PhraseItem::get_unigram_frequency` (`phrase_index.h:124-126`)
# reads four bytes at offset 2 through `MemoryChunk::get_content<guint32>`
# (`memory_chunk.h:388-392`), whose `check_result` is an `assert` in the oracle
# build, so `pinyin_init`/`zhuyin_init` dies of SIGABRT (class (c), row 108).
# oxpinyin stops that library's replay at the record, keeps the earlier
# records and logs one warning; init succeeds.
def _short_add(payload):
    def build(p):
        return ([
            _log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                        new=_item_with(p['one'][1], freq=77)),
            _log_record(_LOG_ADD, p['fresh'][0], new=payload),
            _add_fresh(p)], _watch(p))

    def probe(k):
        out = system_replay(k, build, unigram_add=False)
        out['ret'] = out['ctx']
        return out
    return probe


for _name, _payload in (('one-byte', b'\x01'), ('five-bytes', bytes((1, 1, 7, 0, 0)))):
    for _mode in ('pinyin', 'zhuyin'):
        _suffix = '-zhuyin' if _mode == 'zhuyin' else '-pinyin'
        case('replay-short-add-' + _name + _suffix,
             mode=_mode, abort=True, stderr=True, issues=('850', '870'))(_short_add(_payload))


# #871: the pronunciation counts of a replayed system item. A MODIFY record
# changes the item the sub-index holds in place (`phrase_index.cpp:477-501`);
# the item is then a list of (keys, frequency) entries in the file's order,
# and the pin trains (`phrase_index.cpp:114-143`, every entry in turn), merges
# an imported reading (`:122-139`) and diffs the list back out at save
# (`:392-436`) without ever keying the entries by their keys. Two entries with
# the same keys are two entries. The probe replays one journalled item
# (癫痫, one reading `dian4 xian4` at 100), then runs the item through the
# lifecycle a session sees: train it, import a reading, save, open the saved
# profile, train again, save and open once more. It reports the C ABI's view
# after each step and the bytes `gb_char.dbin` holds after each save. The
# export (pinyin only) lists one row per entry, so entries that were merged
# into one show there.
_PRON_TOKEN = 0x01003e57
_PRON_PHRASE = '癫痫'


def _pron_entries(item, entries):
    """`item` with its readings replaced by `entries`, each (keys, freq):
    the header's count and the unigram stay, the rest is rebuilt."""
    length = item[0]
    head = bytes((length, len(entries))) + item[2:6 + 4 * length]
    return head + b''.join(keys + freq.to_bytes(4, 'little') for keys, freq in entries)


def _pron_shape(entries, unigram=None):
    def new_item(item):
        keys = item[6 + 4 * item[0]:6 + 4 * item[0] + 2 * item[0]]
        built = _pron_entries(item, [(keys if name is None else name, freq) for name, freq in entries])
        return _item_with(built, unigram=unigram) if unigram is not None else built
    return new_item


# Another reading's keys (two syllables, as the item's own), taken from a
# two-reading item of the same library: the pin does not check them.
_PRON_OTHER_KEYS = bytes.fromhex('81024702')


def pron_lifecycle(k, new_item_of, steps):
    """`steps` is `train`, `import` or `mask`: the lifecycle that follows the
    replay. `train` trains the item and imports a reading before the first
    save; `import` only imports (twice, the second time onto the reading the
    first one added, after a reopen); `mask` masks the library the journal
    changed before the import. Every variant saves and opens the saved
    profile at least once."""
    total, items, _offsets = _system_items(k)
    token = _PRON_TOKEN
    item = items[token & 0xFFFFFF]
    assert _item_text(item) == _PRON_PHRASE and item[1] == 1, item.hex()
    _write_user_conf(k)
    _write_log(k, _modify_header(0, total, total) +
               _log_record(_LOG_MODIFY, token, old=item, new=new_item_of(item)))
    out = {'ctx': bool(k.ctx)}
    pinyin = k.mode == 'pinyin'
    parse = 'parse_more_full_pinyins' if pinyin else 'parse_more_chewings'
    typed = b'dian4xian4' if pinyin else b'2u04vu04'
    imported = "dian3'xian4" if pinyin else 'ㄉㄧㄢˇ ㄒㄧㄢˋ'

    def look(tag):
        out[tag] = {'export': remember_export(k, 1, _PRON_PHRASE, adjacent=True), 'unigram': unigram_of(k, token),
                    'phrase': token_phrase_of(k, token)}

    def train():
        assert k.fn(parse, Z, P, S)(k.inst, typed) == len(typed)
        return train_selected_top(k, _PRON_PHRASE, token)

    def save(tag):
        out[tag + '-save'] = k.fn('save', B, P)(k.ctx)
        out[tag + '-dbin'] = Path(k.user, 'gb_char.dbin').read_bytes().hex()

    def reopen(tag):
        k.fn('free_instance', None, P)(k.inst)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = k._inst = None
        look(tag)

    look('opened')
    if steps == 'unigram':
        add = k.fn('token_add_unigram_frequency', B, P, U, U)
        out['added-unigram'] = add(k.inst, token, 7)
        look('after-unigram')
        out['trained'] = train()
        look('after-train')
        save('first')
        reopen('reopened')
        out['added-again'] = add(k.inst, token, 5)
        look('after-unigram-again')
        # A unigram increment does not set m_modified. Dirty an unrelated
        # user library before the second save, preserving both live readings.
        out['dirty'] = import_rows(k, 7, [('你好', "ni3'hao3" if pinyin else 'ㄋㄧˇ ㄏㄠˇ', 1)])
        save('second')
        reopen('reopened-again')
        return out

    if steps == 'train':
        out['trained'] = train()
        look('after-train')
        out['imported'] = import_rows(k, 1, [(_PRON_PHRASE, imported, 50)])
        look('after-import')
        save('first')
        reopen('reopened')
        out['trained-again'] = train()
        look('after-train-again')
        save('second')
        reopen('reopened-again')
    elif steps == 'import':
        out['imported'] = import_rows(k, 1, [(_PRON_PHRASE, imported, 50)])
        look('after-import')
        save('first')
        reopen('reopened')
        out['imported-again'] = import_rows(k, 1, [(_PRON_PHRASE, imported, 7), (_PRON_PHRASE, "dian4'xian4" if pinyin else 'ㄉㄧㄢˋ ㄒㄧㄢˋ', 9)])
        look('after-import-again')
        save('second')
        reopen('reopened-again')
    else:
        assert steps == 'mask'
        # Library 1 as a whole: its journal is skipped when the libraries
        # are merged again, so the item is the shipped one.
        out['mask'] = k.fn('mask_out', B, P, U, U)(k.ctx, 0xFF000000, 0x01000000)
        look('after-mask')
        out['imported'] = import_rows(k, 1, [(_PRON_PHRASE, imported, 50)])
        look('after-import')
        save('first')
        reopen('reopened')
    return out


# Each shape is the item the MODIFY record leaves: (keys, frequency) entries,
# `None` standing for the item's own keys. The first two leave one reading;
# the dup- shapes carry the item's keys twice; `other` adds a second reading.
_PRON_SHAPES = {
    'raised': ([(None, 250)], None),
    'unigram-only': ([(None, 100)], 500),
    'lowered': ([(None, 40)], None),
    'dup-raised-last': ([(None, 100), (None, 160)], None),
    'dup-raised-first': ([(None, 160), (None, 100)], None),
    'dup-both': ([(None, 160), (None, 220)], None),
    'dup-equal': ([(None, 100), (None, 100)], 500),
    'other-reading': ([(None, 100), (_PRON_OTHER_KEYS, 130)], None),
    'other-first': ([(_PRON_OTHER_KEYS, 130), (None, 250)], None),
}
for _mode in ('pinyin', 'zhuyin'):
    case('replay-pron-unigram-combined-' + _mode, mode=_mode, stderr=True, issues=('871', '876', '870'))(
        lambda k: pron_lifecycle(k, _pron_shape([(None, 160), (None, 220)]), 'unigram'))

for _shape, (_entries, _unigram) in _PRON_SHAPES.items():
    for _steps in ('train', 'import', 'mask'):
        for _mode in ('pinyin', 'zhuyin'):
            case('replay-pron-' + _steps + '-' + _shape + '-' + _mode, mode=_mode, stderr=True, issues=('871', '876', '870'))(
                lambda k, e=_entries, u=_unigram, st=_steps: pron_lifecycle(k, _pron_shape(e, u), st))


# The same lifecycle over a profile with all of it at once: the journal
# changes 癫痫's reading and unigram and appends a fresh item to the library
# (its reading is seeded, the shipped item's is not), a user phrase is
# imported into library 7, and 癫痫 is trained, which also trains a gram.
# Each save is followed by a reopen; the second round trains again.
def pron_mixed(k):
    total, items, _offsets = _system_items(k)
    token = _PRON_TOKEN
    item = items[token & 0xFFFFFF]
    one = next(slot for slot, it in items.items() if it[0] == 1 and it[1] == 1)
    fresh = 0x01000000 | (max(items) + 2)
    appended = _item_with(items[one], unigram=11)
    changed = _item_with(item, unigram=item[2] + 400, freq=250)
    _write_user_conf(k)
    _write_log(k, _modify_header(0, total, total + 400 + 11) +
               _log_record(_LOG_MODIFY, token, old=item, new=changed) +
               _log_record(_LOG_ADD, fresh, new=appended))
    out = {'ctx': bool(k.ctx)}
    pinyin = k.mode == 'pinyin'
    parse = 'parse_more_full_pinyins' if pinyin else 'parse_more_chewings'
    typed = b'dian4xian4' if pinyin else b'2u04vu04'
    user_row = ('龘龘', "da2'da2" if pinyin else 'ㄉㄚˊ ㄉㄚˊ', 20)

    def look(tag):
        out[tag] = {'export': remember_export(k, 1, _PRON_PHRASE, adjacent=True),
                    'user-export': remember_export(k, 7, user_row[0], adjacent=True),
                    'unigram': unigram_of(k, token), 'fresh': unigram_of(k, fresh),
                    'fresh-phrase': token_phrase_of(k, fresh),
                    # The appended item shares its text with slot `one`: walk every row.
                    'fresh-export': remember_export(k, 1, token_phrase_of(k, fresh)[2])}

    def train():
        assert k.fn(parse, Z, P, S)(k.inst, typed) == len(typed)
        return train_selected_top(k, _PRON_PHRASE, token)

    def save(tag):
        out[tag + '-save'] = k.fn('save', B, P)(k.ctx)
        for name in ('gb_char.dbin', 'user.bin'):
            out[tag + '-' + name] = Path(k.user, name).read_bytes().hex()

    def reopen(tag):
        k.fn('free_instance', None, P)(k.inst)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = k._inst = None
        look(tag)

    look('opened')
    out['imported-user'] = import_rows(k, 7, [user_row])
    out['trained'] = train()
    look('after-train')
    out['imported'] = import_rows(k, 1, [(_PRON_PHRASE, "dian3'xian4" if pinyin else 'ㄉㄧㄢˇ ㄒㄧㄢˋ', 50)])
    look('after-import')
    save('first')
    reopen('reopened')
    out['trained-again'] = train()
    look('after-train-again')
    save('second')
    reopen('reopened-again')
    return out


for _mode in ('pinyin', 'zhuyin'):
    case('replay-pron-mixed-' + _mode, mode=_mode, stderr=True, issues=('871', '876', '870'))(pron_mixed)


# A removed item and the prefix it extends. `SEARCH_CONTINUED` comes from the
# pinyin table's DB get alone (`chewing_large_table2_bdb.cpp:182-187` at
# 074a2219): a REMOVE clears only the sub-index (`phrase_index.cpp:219-236`), so
# the table still reports the removed phrase's key path as continuing, and the
# item drops out later, where `get_phrase_item` fails (`phonetic_lookup.h:646`,
# `:674`). The probe removes the one four-character phrase of the merged
# library that nothing extends, then looks at what the C ABI shows of a search
# over its reading: the parse and the guessed sentence for the reading, for a
# reading that goes on past it and for the prefix the phrase extends; and the
# candidate list of the prefix. The pin lists the removed token itself through
# a stale item (a SIGSEGV), so no candidate list is taken where it is one.
def _removed_extension(k, candidates, labels=('exact', 'longer', 'prefix'), before=False):
    total, items, _offsets = _system_items(k, 'merged.bin')

    def readings(item):
        length, prons = item[0], item[1]
        start = 6 + 4 * length
        stride = 2 * length + 4
        return [item[start + stride * i:start + stride * i + 2 * length] for i in range(prons)]

    # The item's key path must be no other phrase's prefix, in any system
    # library: only then does the removal leave nothing that extends it.
    longer = set()
    for name in ('merged.bin', 'opengram.bin', 'gb_char.bin', 'gbk_char.bin'):
        for item in _system_items(k, name)[1].values():
            if item[0] > 4:
                longer.update(reading[:8] for reading in readings(item))
    slot = next(slot for slot, item in items.items()
                if item[0] == 4 and item[1] == 1 and readings(item)[0] not in longer)
    token = 0x04000000 | slot
    # Read the phrase and its reading from the untouched library first.
    phrase = token_phrase_of(k, token)[2]
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    keys = new(0, 0, 2)
    assert k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, 0, keys)
    view = C.cast(keys, C.POINTER(Arr)).contents
    syllables = []
    for i in range(view.len):
        key = C.c_ushort(C.cast(view.data, C.POINTER(C.c_ushort))[i])
        rendered = P(UNTOUCHED)
        assert k.fn('get_pinyin_string', B, P, C.POINTER(C.c_ushort), C.POINTER(P))(
            k.inst, C.byref(key), C.byref(rendered))
        syllables.append(k.text(rendered.value))
    reading = ''.join(syllables)
    k.fn('free_instance', None, P)(k.inst)
    k.fn('fini', None, P)(k.ctx)
    k._ctx = k._inst = None
    _write_user_conf(k)
    _write_log(k, _modify_header(0, total, total) + _log_record(_LOG_REMOVE, token, old=items[slot]),
               'merged.dbin')
    out = {'phrase': phrase, 'reading': reading, 'removed': token_phrase_of(k, token)}
    if k.mode == 'zhuyin':
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    texts = {'exact': reading, 'longer': reading + 'ni', 'prefix': ''.join(syllables[:-1])}
    for label in labels:
        text = texts[label]
        inst = k.alloc()
        out[label] = {'parsed': k.fn('parse_more_full_pinyins', Z, P, S)(inst, text.encode())}
        out[label]['guessed'] = k.fn('guess_sentence', B, P)(inst)
        out[label]['sentence'] = (sentence_out(k, inst, 0) if k.mode == 'pinyin' else
                                  zhuyin_sentence_out(k, inst))
        if candidates and k.mode == 'pinyin':
            out[label]['candidates'] = guess_rows(k, inst, 0, 0x1e)
        elif candidates and before:
            ret = k.fn('guess_candidates_before_cursor', B, P, Z)(inst, out[label]['parsed'])
            out[label]['candidates'] = {'ret': ret, 'rows': candidate_rows(k, inst)}
        elif candidates:
            out[label]['candidates'] = zhuyin_guess_rows(k, inst, 0)
        k.fn('free_instance', None, P)(inst)
    return out


for _mode in ('pinyin', 'zhuyin'):
    case('removed-extension-sentence-' + _mode, mode=_mode, stderr=True)(
        lambda k: _removed_extension(k, False))
    case('removed-extension-candidates-' + _mode, mode=_mode, stderr=True)(
        lambda k: _removed_extension(k, True, ('prefix',)))


# The candidate list over the removed token itself. `_compute_phrase_strings_of_items`
# leaves `m_phrase_string` unset when `_token_get_phrase` finds no item
# (`pinyin.cpp:1635-1637`, result ignored at `:2013-2016`; `zhuyin.cpp:1103-1105`),
# and `_remove_duplicated_items_by_phrase_string` hands the NULL to `strcmp` in
# `compare_indexed_item_with_phrase_string` (`pinyin.cpp:2053`, called from
# `pinyin_guess_candidates` at `:2300`; `zhuyin.cpp:1360`, from
# `zhuyin_guess_candidates_after_cursor` at `:1537`): SIGSEGV. A removed token
# stays in the phrase table, so a search over its reading lists it. oxpinyin
# lists no candidate for an item that does not exist.
def _removed_candidate(k, before=False):
    out = _removed_extension(k, True, ('exact',), before)
    exact = out.pop('exact')
    out['exact-parsed'] = exact['parsed']
    out['exact-guessed'] = exact['guessed']
    out['exact-candidates-ret'] = exact['candidates']['ret']
    out['~exact-candidates'] = exact['candidates']['rows']
    return out


# Row 102's answer for the site: an item a reload freed adds no candidate and
# the call returns normally. A removed item is the same case, a token whose item
# `get_phrase_item` no longer finds, so the calls answer `true`.
for _mode in ('pinyin', 'zhuyin'):
    case('removed-candidate-' + _mode, mode=_mode, stderr=True, crash={
        'removed': [False, 0, None], 'exact-guessed': True, 'exact-candidates-ret': True,
    }, issues=('row:102',))(_removed_candidate)
case('removed-candidate-before-cursor-zhuyin', mode='zhuyin', stderr=True, crash={
    'removed': [False, 0, None], 'exact-guessed': True, 'exact-candidates-ret': True,
}, issues=('row:102',))(lambda k: _removed_candidate(k, before=True))


# Batch F (#525): `zhuyin.cpp`'s phrase-table walk in `_add_phrase`. A token
# whose sub-index equals the target trips `assert(PHRASE_INDEX_LIBRARY_INDEX
# (token) != index)` (`:440`); a lone match whose phrase-index item text
# differs from the phrase trips the `memcmp` assert (`:457`). The crafted
# `user_phrase_index.bin` is written in the running cell's own format
# (`_craft_db`): a Berkeley DB btree on bdb (the pin's own layout,
# `chewing_large_table2_bdb.cpp:58`), a Kyoto Cabinet snapshot on kc
# (`phrase_large_table3_kyotodb.cpp:109`) and a tkrzw `TreeDBM` file on tkrzw
# (`phrase_large_table3_tkrzwdb.cpp:83`).
def _ucs4_key(text):
    return b''.join(ord(character).to_bytes(4, 'little') for character in text)


@case('abort-zhuyin-add-phrase-duplicate-library-token', mode='zhuyin', abort=False, issues=('525',))
def _(k):
    # Two tokens for 你好, both in library 7 (the target): the pin's second
    # in-index candidate asserts at `:440`.
    ctx = k.init()
    assert ctx, 'the first init failed'
    k.fn('fini', None, P)(ctx)
    value = (0x07000001).to_bytes(4, 'little') + (0x07000002).to_bytes(4, 'little')
    _craft_db(os.path.join(k.user, 'user_phrase_index.bin'), [(_ucs4_key('你好'), value)], 1)
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
    return {'ret': k.fn('iterator_add_phrase', B, P, S, S, I)(
        it, '你好'.encode(), 'ㄋㄧˇ ㄏㄠˇ'.encode(), 1)}


@case('abort-zhuyin-add-phrase-index-text-mismatch', mode='zhuyin', abort=False, issues=('525',))
def _(k):
    # Persist 你們, then point 你好's phrase-table row at 你們's token: the
    # lone in-library match names an item whose text differs, `:457`.
    ctx = k.ctx
    it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
    assert k.fn('iterator_add_phrase', B, P, S, S, I)(
        it, '你們'.encode(), 'ㄋㄧˇ ㄇㄣˊ'.encode(), 1), 'the setup phrase did not add'
    k.fn('end_add_phrases', None, P)(it)
    assert k.fn('save', B, P)(ctx)
    token = int(tokens_of(k, '你們')[0])
    k.fn('free_instance', None, P)(k._inst)
    k._inst = None
    k.fn('fini', None, P)(ctx)
    _craft_db(os.path.join(k.user, 'user_phrase_index.bin'),
              [(_ucs4_key('你好'), token.to_bytes(4, 'little'))], 1)
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
    return {'ret': k.fn('iterator_add_phrase', B, P, S, S, I)(
        it, '你好'.encode(), 'ㄋㄧˇ ㄏㄠˇ'.encode(), 1)}


# The pinyin import iterator's `_add_phrase` walks the same phrase table
# (`pinyin.cpp:533-571`): two tokens in the target library trip the assert at
# `:554`; a lone one whose phrase-index item has another length trips `:568`,
# one with the same length and other text `:571`. `_craft_db` writes
# `user_phrase_index.bin` in the running cell's format: a BDB btree, KC
# snapshot or tkrzw TreeDBM. All three cells are verified (#1120).
def _persist_pinyin_phrase(k, text, reading):
    """Adds `text` to library 7, saves, and returns its token; the context
    is closed."""
    ctx = k.ctx
    it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
    assert k.fn('iterator_add_phrase', B, P, S, S, I)(
        it, text.encode(), reading.encode(), 1), 'the setup phrase did not add'
    k.fn('end_add_phrases', None, P)(it)
    assert k.fn('save', B, P)(ctx)
    token = int(tokens_of(k, text)[0])
    assert token >> 24 == 7, 'the setup phrase is not new to library 7'
    k.fn('free_instance', None, P)(k._inst)
    k._inst = None
    k.fn('fini', None, P)(ctx)
    k._ctx = None
    return token


def _add_pinyin_phrase_after_index(k, value):
    """Points 你好's phrase-table row at `value` and adds 你好 to library 7."""
    _craft_db(os.path.join(k.user, 'user_phrase_index.bin'), [(_ucs4_key('你好'), value)], 1)
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
    return {'ret': k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), b"ni'hao", 1)}


@case('abort-add-phrase-duplicate-library-token', abort=False, userdir=True, issues=('525',))
def _(k):
    ctx = k.init()
    assert ctx, 'the first init failed'
    k.fn('fini', None, P)(ctx)
    value = (0x07000001).to_bytes(4, 'little') + (0x07000002).to_bytes(4, 'little')
    return _add_pinyin_phrase_after_index(k, value)


@case('abort-add-phrase-index-length-mismatch', abort=False, userdir=True, issues=('525',))
def _(k):
    token = _persist_pinyin_phrase(k, '你好嗷', "ni'hao'ao")
    return _add_pinyin_phrase_after_index(k, token.to_bytes(4, 'little'))


@case('abort-add-phrase-index-item-missing', abort=False, userdir=True, issues=('525',))
def _(k):
    # The phrase table names library 7's token, but user.bin has no item:
    # get_phrase_item leaves a zero-length PhraseItem, so :568 asserts.
    ctx = k.init()
    assert ctx, 'the first init failed'
    k.fn('fini', None, P)(ctx)
    return _add_pinyin_phrase_after_index(k, (0x07000001).to_bytes(4, 'little'))


@case('abort-add-phrase-index-text-mismatch', abort=False, userdir=True, issues=('525',))
def _(k):
    token = _persist_pinyin_phrase(k, '你們', "ni'men")
    return _add_pinyin_phrase_after_index(k, token.to_bytes(4, 'little'))


# #759 PR1 / #785: phrase records are token arrays, including duplicates.
# The kc/tkrzw mask walk reuses an entry; MemoryChunk::set_content does not
# shrink its length. Compare complete phrase records, independently of the
# still-unported chewing-table half of #785.
def phrase_table_lookup(k, text):
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    k.glib.g_array_free.restype, k.glib.g_array_free.argtypes = P, [P, I]
    arr = new(0, 0, 4)
    ret = k.fn('lookup_tokens', B, P, S, P)(k.inst, text.encode(), arr)
    view = C.cast(arr, C.POINTER(Arr)).contents
    tokens = list(C.cast(view.data, C.POINTER(U))[:view.len])
    k.glib.g_array_free(arr, True)
    return [ret, tokens]


def phrase_table_reopen(k):
    if k._inst:
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
    k.fn('fini', None, P)(k._ctx)
    k._ctx = None
    assert k.ctx, 'phrase-table reopen failed'


def phrase_table_mask_records(duplicates, remove_middle=False):
    def probe(k):
        phrases = TOKEN_REUSE_PHRASES[:4]
        for text, reading, _ in phrases:
            _add_user_phrase(k, text, [reading.encode()], 100)
        tokens = [phrase_table_lookup(k, text)[1][0] for text, _, _ in phrases]
        assert k.fn('save', B, P)(k.ctx)
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None
        # Explicit byte-key order: the long record is visited before the
        # short one on each backend. The long tail survives a no-match mask.
        keys = sorted(text.encode('utf-32-le') for text, _, _ in phrases)
        long_tokens = [tokens[0], tokens[0], tokens[1]] if duplicates else sorted(tokens[:3])
        rows = [(keys[0], b''.join(t.to_bytes(4, 'little') for t in long_tokens)),
                (keys[1], tokens[3].to_bytes(4, 'little'))]
        path = Path(k.user, 'user_phrase_index.bin')
        _craft_db(str(path), rows, 1)
        _write_user_conf(k)
        assert k.ctx
        out = {'loaded': [phrase_table_lookup(k, key.decode('utf-32-le')) for key, _ in rows],
               'export_before': d18_export_phrases(k)}
        masked_token = tokens[1] if remove_middle else 0x0700fffe
        out['masked'] = k.fn('mask_out', B, P, U, U)(k.ctx, 0xffffffff, masked_token)
        # mask_out does not set m_modified. An unconstrained train marks
        # the context dirty without editing either crafted phrase record.
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'wo') == 2
        assert k.fn('guess_sentence', B, P)(k.inst)
        out['trained'] = k.fn('train', B, P, C.c_ubyte)(k.inst, 0)
        out['saved'] = k.fn('save', B, P)(k.ctx)
        out['stored'] = index_file_rows(path)
        out['export_after'] = d18_export_phrases(k)
        phrase_table_reopen(k)
        out['reopened'] = [phrase_table_lookup(k, key.decode('utf-32-le')) for key, _ in rows]
        out['export_reopened'] = d18_export_phrases(k)
        return out
    return probe


# bdb controls the absence of shared-scratch tails. Both cases also detect
# reconstruction from phrase items when saving: these raw records, including
# their key/token associations, must survive without being derived anew.
case('phrase-table-mask-long-short', issues=('759', '785'))(
    phrase_table_mask_records(False))
case('phrase-table-mask-duplicate-long-short', issues=('759', '785'))(phrase_table_mask_records(True))
# Removing the middle token shrinks the scratch entry before the short row.
# Carry its post-mask length, not the maximum length previously encountered;
# retaining the old maximum would duplicate the surviving final token.
case('phrase-table-mask-post-removal-shrink', issues=('759', '785'))(
    phrase_table_mask_records(False, remove_middle=True))


@case('phrase-table-add-search-remove-roundtrip', control=True, issues=('759', '785'))
def _(k):
    text, reading = '龘龘', b"da2'da2"
    out = {'absent': phrase_table_lookup(k, text)}
    out['added'] = _add_user_phrase(k, text, [reading], 100)
    out['found'] = phrase_table_lookup(k, text)
    out['added_again'] = _add_user_phrase(k, text, [reading], 1)
    out['found_again'] = phrase_table_lookup(k, text)
    out['saved'] = k.fn('save', B, P)(k.ctx)
    phrase_table_reopen(k)
    out['reopened'] = phrase_table_lookup(k, text)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'dada') == 4
    assert k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0)
    candidates = _user_candidate_rows(k, k.inst)
    assert candidates
    out['removed'] = k.fn('remove_user_candidate', B, P, P)(k.inst, candidates[0])
    out['after_remove'] = phrase_table_lookup(k, text)
    out['readded'] = _add_user_phrase(k, text, [reading], 100)
    out['after_readd'] = phrase_table_lookup(k, text)
    out['saved_again'] = k.fn('save', B, P)(k.ctx)
    phrase_table_reopen(k)
    out['final'] = phrase_table_lookup(k, text)
    out['export'] = d18_export_phrases(k)
    return out


# #799 review: prepare_tokens only enables instantiated sub-indices
# (074a2219 storage/phrase_index.h:762-772); PhraseTableEntry::search skips
# NULL arrays (storage/phrase_large_table3.cpp:40-42).
@case('phrase-table-never-loaded-library-filter', issues=('799',))
def _(k):
    token = (15 << 24) | 1  # No library 15 is loaded by this fixture.
    value = token.to_bytes(4, 'little')
    prefix = '你'.encode('utf-32-le')
    _craft_db(str(Path(k.user, 'user_phrase_index.bin')),
              [(prefix, b''), (prefix + '龘'.encode('utf-32-le'), value),
               ('龘龘'.encode('utf-32-le'), value)], 1)
    _write_user_conf(k)
    return {'lookup': phrase_table_lookup(k, '龘龘'),
            'prediction': row58_rows(k, '你')}


def phrase_table_raw_key_prediction(nonunicode):
    def probe(k):
        _add_user_phrase(k, '龘龘', [b"da2'da2"], 100)
        token = next(t for t in tokens_of(k, '龘龘') if t >> 24 == 7)
        assert k.fn('save', B, P)(k.ctx)
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None
        prefix = '你'.encode('utf-32-le')
        suffix = (0x110000).to_bytes(4, 'little') if nonunicode else '好'.encode('utf-32-le')
        _craft_db(str(Path(k.user, 'user_phrase_index.bin')),
                  [(prefix, b''), (prefix + suffix, token.to_bytes(4, 'little'))], 1)
        _write_user_conf(k)
        # search_suggestion compares byte prefixes, never decodes the key
        # (phrase_large_table3_bdb.cpp:225-252). The candidate uses the live
        # item length (pinyin.cpp:2391-2395) and strips m_begin characters
        # from its live text (:2018-2022), with no starts_with check. Both
        # fixtures therefore display 龘, despite the table key starting 你.
        return row58_rows(k, '你')
    return probe


case('phrase-table-predict-nonunicode-raw-key')(
    phrase_table_raw_key_prediction(True))
case('phrase-table-predict-mismatched-live-prefix', control=True)(
    phrase_table_raw_key_prediction(False))


# #540 / D-18. Small boundary fixtures, never a repeated-training workload.
# Read at libpinyin 074a2219: phrase_index.cpp:56-91, 150-179;
# pinyin_parser2.cpp:274-360; ngram.cpp:277-358; pinyin.cpp:520-605,
# 896-911, 2597-2650; phonetic_lookup.h:880-929.
def d18_export_phrases(k):
    it = k.fn('begin_get_phrases', P, P, U)(k.ctx, 7)
    rows = []
    while k.fn('iterator_has_next_phrase', B, P)(it):
        phrase, reading, count = P(), P(), I(UNTOUCHED)
        ret = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
            it, C.byref(phrase), C.byref(reading), C.byref(count))
        rows.append([ret, k.text(phrase.value), k.text(reading.value), count.value])
    k.fn('end_get_phrases', None, P)(it)
    return rows


def d18_remember(deltas):
    def probe(k):
        out = {'parsed': consumed(k, 'parse_more_full_pinyins', b'nihaoshijie', fresh=False)}
        rows = []
        for delta in deltas:
            ret = k.fn('remember_user_input', B, P, S, I)(k.inst, '你好世界'.encode(), delta)
            token = next(t for t in tokens_of(k, '你好世界') if t >> 24 == 7)
            rows.append([delta, ret, d18_export_phrases(k), unigram_of(k, token)])
        out['steps'] = rows
        return out
    return probe


case('d18-remember-reported', control=True, issues=('540',))(d18_remember((-1, 0, 0x7fffffff)))
case('d18-remember-signed-boundary', control=True, issues=('540',))(d18_remember((0x7ffffffe, 1, 1, 0x7fffffff, 1)))
for _count in (715827882, 715827883, 1431655765, 1431655766):
    case(f'd18-remember-multiply-{_count}', control=True, issues=('540',))(d18_remember((_count,)))


# #752: reuse the preregistered full-pinyin NUL-middle case below,
# axis-c-encoding-pinyin-remember_user_input-nul-middle. These are its
# transformed-parser twins and a known-character NUL-termination control.
# 074a2219 pinyin.cpp:3115-3121, 3688-3694 refuses a character with no token;
# bytes after the first NUL cannot change the visible phrase.
def remember_nul(parse_name, input_text, phrase):
    def probe(k):
        k.fn('set_options', B, P, U)(k.ctx, 0x28)
        if parse_name == 'parse_more_double_pinyins':
            assert k.fn('set_double_pinyin_scheme', B, P, I)(k.ctx, 2)
        elif parse_name == 'parse_more_chewings':
            assert k.fn('set_zhuyin_scheme', B, P, I)(k.ctx, 1)
        parsed = k.fn(parse_name, Z, P, S)(k.inst, input_text)
        assert parsed == len(input_text), 'the complete two-syllable input must parse'
        return dict(parsed=parsed,
                    ret=k.fn('remember_user_input', B, P, S, I)(k.inst, phrase, 1),
                    rows=d18_export_phrases(k))
    return probe


case('remember-nul-double-pinyin', stderr=True, issues=('752',))(
    remember_nul('parse_more_double_pinyins', b'nihk', b'ni\0hao'))
case('remember-nul-chewing-pinyin', stderr=True, issues=('752',))(
    remember_nul('parse_more_chewings', b'su3cl3', b'ni\0hao'))
case('remember-known-nul-suffix-pinyin', control=True, stderr=True, issues=('752',))(
    remember_nul('parse_more_full_pinyins', b'nihao', '你好'.encode() + b'\0ignored'))


def d18_unigram(boundary, text='你'):
    def probe(k):
        token = tokens_of(k, text)[0]
        if text == '丠':
            assert token >> 24 == 2, 'the alternate-sub-index control must use library 2'
        out = {'token': token, 'before': unigram_of(k, token)}
        if boundary:
            # Select the token's actual sub-index, not an assumed GB token.
            libraries = {'RESERVED': 0, 'GB_DICTIONARY': 1, 'GBK_DICTIONARY': 2,
                         'OPENGRAM_DICTIONARY': 3, 'MERGED_DICTIONARY': 4,
                         'ADDON_DICTIONARY': 5, 'NETWORK_DICTIONARY': 6, 'USER_DICTIONARY': 7}
            library = token >> 24
            entries = [line.split() for line in Path(k.data, 'table.conf').read_text().splitlines()
                       if line.startswith('default ')]
            entry = next(row for row in entries if libraries[row[1]] == library)
            assert entry[3] != 'NULL', 'boundary fixture needs a system sub-index'
            # MemoryChunk's 8-byte header, then SubPhraseIndex::m_total_freq.
            total = int.from_bytes(Path(k.data, entry[3]).read_bytes()[8:12], 'little')
            deltas = (0xffffffff - total - 1, 1, 1, 0xffffffff)
            out.update(library=library, subindex=entry[3], **{'library-total': total})
        else:
            deltas = (0xffffffff, 0x7fffffff, 0x7fffffff, 0x7fffffff, 0x7fffffff, 0xffffffff)
        add = k.fn('token_add_unigram_frequency', B, P, U, U)
        out['steps'] = [[delta, add(k.inst, token, delta), unigram_of(k, token)] for delta in deltas]
        if boundary:
            assert [row[1] for row in out['steps']] == [True, True, False, False]
            assert out['steps'][1][2] == out['steps'][2][2] == out['steps'][3][2]
        return out
    return probe


def d18_input(k):
    assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    out = {}
    parse = k.fn('parse_more_full_pinyins', Z, P, S)
    for length in (4096, 4097, 32766, 32767, 32768, 65535, 65536, 65537):
        inst = k.alloc()
        text = (b'ni' * ((length + 1) // 2))[:length]
        ret = parse(inst, text)
        stored = k.fn('get_parsed_input_length', Z, P)(inst)
        # A later short parse must replace the retained long matrix/length.
        short = parse(inst, b'hao')
        out[str(length)] = [ret, stored, short, k.fn('get_parsed_input_length', Z, P)(inst)]
        k.fn('free_instance', None, P)(inst)
    for label, text in (('whole-tail', b'ni' * 16382 + b'hao'),
                        ('split-tail', b'ni' * 16383 + b'hao'),
                        ('separator-tail', b'ni' * 16383 + b"'hao"),
                        ('invalid-tail', b'ni' * 16383 + b'!ni')):
        inst = k.alloc()
        out[label] = [parse(inst, text), k.fn('get_parsed_input_length', Z, P)(inst)]
        k.fn('free_instance', None, P)(inst)
    return out


def d18_bigram_db(k, path, rows=None, read_keys=(), allow_missing=False):
    """Independent raw SingleGram fixtures through each installed C DBM API.

    `rows=None` reads an existing fixture; all returned buffers are copied
    before closing the DBM. No installed system/user data is ever written.
    """
    import ctypes.util
    conf = Path(k.data, 'table.conf').read_text()
    if 'database format:BerkeleyDB' in conf or 'database format: BerkeleyDB' in conf:
        return _craft_bdb(str(path), rows, 2, read_keys=read_keys, allow_missing=allow_missing)
    writable = rows is not None
    if writable and Path(path).exists():
        Path(path).unlink()
    if 'KyotoCabinet' in conf:
        db = C.CDLL(ctypes.util.find_library('kyotocabinet'))
        db.kcdbnew.restype = P
        db.kcdbopen.argtypes, db.kcdbopen.restype = [P, S, U], I
        db.kcdbset.argtypes, db.kcdbset.restype = [P, S, Z, S, Z], I
        db.kcdbget.argtypes, db.kcdbget.restype = [P, S, Z, C.POINTER(Z)], P
        db.kcdbloadsnap.argtypes, db.kcdbloadsnap.restype = [P, S], I
        db.kcdbdumpsnap.argtypes, db.kcdbdumpsnap.restype = [P, S], I
        db.kcdbclose.argtypes, db.kcdbclose.restype = [P], I
        db.kcdbdel.argtypes, db.kcfree.argtypes = [P], [P]
        handle = db.kcdbnew()
        # The KC system gram is a HashDB; the user gram is a StashDB
        # snapshot (`074a2219 ngram_kyotodb.cpp:54-90`), not a HashDB file.
        snapshot = Path(path).name == 'user_bigram.db'
        spec = b'-' if snapshot else (str(path) + '#type=kch').encode()
        assert db.kcdbopen(handle, spec, 6 if writable or snapshot else 1)
        try:
            if snapshot and not writable:
                assert db.kcdbloadsnap(handle, str(path).encode())
            for key, value in rows or ():
                assert db.kcdbset(handle, key, len(key), value, len(value))
            if snapshot and writable:
                assert db.kcdbdumpsnap(handle, str(path).encode())
            values = []
            for key in read_keys:
                size = Z()
                value = db.kcdbget(handle, key, len(key), C.byref(size))
                if not value and allow_missing:
                    values.append(None)
                    continue
                assert value
                values.append(C.string_at(value, size.value))
                db.kcfree(value)
            return values
        finally:
            assert db.kcdbclose(handle)
            db.kcdbdel(handle)
    assert 'Tkrzw' in conf, 'unknown fixture backend'
    db = C.CDLL(ctypes.util.find_library('tkrzw'))
    db.tkrzw_dbm_open.argtypes, db.tkrzw_dbm_open.restype = [S, B, S], P
    db.tkrzw_dbm_set.argtypes, db.tkrzw_dbm_set.restype = [P, S, I, S, I, B], B
    db.tkrzw_dbm_get.argtypes, db.tkrzw_dbm_get.restype = [P, S, I, C.POINTER(I)], P
    db.tkrzw_dbm_close.argtypes, db.tkrzw_dbm_close.restype = [P], B
    libc = C.CDLL(None)
    libc.free.argtypes = [P]
    handle = db.tkrzw_dbm_open(str(path).encode(), writable, b'dbm=HashDBM')
    assert handle
    try:
        for key, value in rows or ():
            assert db.tkrzw_dbm_set(handle, key, len(key), value, len(value), True)
        values = []
        for key in read_keys:
            size = I()
            value = db.tkrzw_dbm_get(handle, key, len(key), C.byref(size))
            if not value and allow_missing:
                values.append(None)
                continue
            assert value
            values.append(C.string_at(value, size.value))
            libc.free(value)
        return values
    finally:
        assert db.tkrzw_dbm_close(handle)


def d18_gram(total, token, count):
    return b''.join(n.to_bytes(4, 'little') for n in (total, token, count))


def d18_bigram_fixture(k, system, user, current='号', empty_system=False, extra_successor=False):
    prev, cur, other = (tokens_of(k, text)[0] for text in ('你', current, '我'))
    k.fn('free_instance', None, P)(k._inst)
    k._inst = None
    k.fn('fini', None, P)(k._ctx)
    k._ctx = None
    if system is not None or empty_system:
        copied = tempfile.mkdtemp(prefix='sys-', dir=k.scratch)
        shutil.copytree(k.data, copied, dirs_exist_ok=True)
        k.data = copied
        system_rows = [] if empty_system else [
            (prev.to_bytes(4, 'little'), d18_gram(system[0], cur, system[1]))]
        d18_bigram_db(k, Path(copied, 'bigram.db'), system_rows)
    # More than one predecessor: the export's do/while leaves its last
    # loaded gram for the next has_next call (row 36, already fixed).
    records = [(cur, user[1])] + ([(other, 1)] if extra_successor else [])
    gram = user[0].to_bytes(4, 'little') + b''.join(
        t.to_bytes(4, 'little') + count.to_bytes(4, 'little') for t, count in sorted(records))
    rows = [(p.to_bytes(4, 'little'), gram) for p in (prev, other)]
    rows.append((b'\x01\x00\x00\x00', b'\0' * 4))
    d18_bigram_db(k, Path(k.user, 'user_bigram.db'), rows)
    _write_user_conf(k)
    return prev, cur


def d18_merge_observation(k):
    assert k.fn('set_options', B, P, U)(k.ctx, (1 << 5) | (1 << 9))
    parsed = k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'nihao')
    guessed = k.fn('guess_sentence', B, P)(k.inst)
    sentences = final_step_sentences(k, k.inst)
    first = guess_rows(k, k.inst, 0, 0x1e) if k.mode == 'pinyin' else zhuyin_guess_rows(k, k.inst, 0)
    candidate = row_with_text(k, k.inst, 2, '你')
    assert first['ret'] and candidate, 'missing normal predecessor candidate'
    chosen = k.fn('choose_candidate', I, P, Z, P)(k.inst, 0, candidate)
    constrained = k.fn('guess_sentence', B, P)(k.inst)
    chosen_sentences = final_step_sentences(k, k.inst)
    rows = guess_rows(k, k.inst, 2, 0x1e) if k.mode == 'pinyin' else zhuyin_guess_rows(k, k.inst, 2)
    normal = [row for row in rows['rows'] if row[0] == 2]
    assert normal, 'merged-row fixture must reach normal candidates'
    return dict(parsed=parsed, guessed=guessed, chosen=chosen, constrained=constrained,
                sentences=sentences, chosen_sentences=chosen_sentences,
                ret=rows['ret'], candidates=normal)


def d18_merge(system, user, current='号', empty_system=False, extra_successor=False, lam=None):
    def probe(k):
        d18_bigram_fixture(k, system, user, current=current, empty_system=empty_system,
                          extra_successor=extra_successor)
        if lam is not None:
            conf = Path(k.data, 'table.conf')
            conf.write_text(re.sub(r'(?m)^lambda parameter:.*$', 'lambda parameter:' + lam,
                                   conf.read_text()))
        return d18_merge_observation(k)
    return probe


def d18_bigram_export(count):
    def probe(k):
        d18_bigram_fixture(k, None, (count, count), current='好')
        it = k.fn('begin_get_bigram_phrases', P, P)(k.ctx)
        out = {'has': k.fn('bigram_iterator_has_next_phrase', B, P)(it)}
        phrase, reading, value = P(), P(), I(UNTOUCHED)
        ret = k.fn('bigram_iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
            it, C.byref(phrase), C.byref(reading), C.byref(value))
        out['row'] = [ret, k.text(phrase.value), k.text(reading.value), value.value]
        k.fn('end_get_bigram_phrases', None, P)(it)
        return out
    return probe


def d18_bigram_update(count, predicted=False, user_only=False):
    def probe(k):
        current = '嚎' if user_only else '好'
        prev, cur = d18_bigram_fixture(k, None, (count + int(user_only), count), current=current,
                                     empty_system=user_only, extra_successor=user_only)
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        out = {'before': unigram_of(k, cur)}
        if predicted:
            out['predicted'] = k.fn('guess_predicted_candidates', B, P, S)(k.inst, '你'.encode())
            candidate = row_with_text(k, k.inst, 4, current)
            assert candidate, 'missing predicted bigram candidate'
            out['choose'] = k.fn('choose_predicted_candidate', B, P, P)(k.inst, candidate)
            # A guessed, unconstrained sentence makes train set m_modified
            # without changing counts, so save exposes the predicted update.
            out['parse'] = consumed(k, 'parse_more_full_pinyins', b'wo', fresh=False)
        else:
            out['parse'] = consumed(k, 'parse_more_full_pinyins', b'nihao', fresh=False)
        out['guess'] = k.fn('guess_sentence', B, P)(k.inst)
        if not predicted:
            choices = []
            for offset, text in ((0, '你'), (2, '好')):
                if k.mode == 'pinyin':
                    assert k.fn('guess_candidates', B, P, Z, U)(k.inst, offset, 0x1e)
                else:
                    assert k.fn('guess_candidates_after_cursor', B, P, Z)(k.inst, offset)
                candidate = row_with_text(k, k.inst, 2, text)
                assert candidate, 'missing normal candidate: ' + text
                choices.append(k.fn('choose_candidate', I, P, Z, P)(k.inst, offset, candidate))
            out['choices'] = choices
            out['chosen-guess'] = k.fn('guess_sentence', B, P)(k.inst)
        out['train'] = (k.fn('train', B, P, C.c_ubyte)(k.inst, 0) if k.mode == 'pinyin'
                        else k.fn('train', B, P)(k.inst))
        out['after'] = unigram_of(k, cur)
        out['save'] = k.fn('save', B, P)(k.ctx)
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None
        raw = d18_bigram_db(k, Path(k.user, 'user_bigram.db'), read_keys=(prev.to_bytes(4, 'little'),))[0]
        total = int.from_bytes(raw[:4], 'little')
        records = [[int.from_bytes(raw[i:i+4], 'little'), int.from_bytes(raw[i+4:i+8], 'little')]
                   for i in range(4, len(raw), 8)]
        out['stored'] = [total, records]
        out['reopened-unigram'] = unigram_of(k, cur)
        if user_only:
            out['reopened-guess'] = d18_merge_observation(k)
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    case('d18-unigram-max-' + _mode, mode=_mode, control=True, issues=('540',))(d18_unigram(False))
    case('d18-unigram-total-boundary-' + _mode, mode=_mode, control=True, issues=('540',))(d18_unigram(True))
    case('d18-input-boundaries-' + _mode, mode=_mode, control=True, issues=('540',))(d18_input)
    for _label, _system, _user, _control in (
            ('total-before', (1, 1), (0xfffffffe, 1), True),
            ('total-zero', (1, 1), (0xffffffff, 1), False),
            ('total-after', (2, 1), (0xffffffff, 1), False),
            ('count-before', (0x80000000, 1), (0xffffffff, 0xfffffffe), True),
            ('count-zero', (0x80000000, 1), (0xffffffff, 0xffffffff), False),
            ('count-after', (0x80000000, 2), (0xffffffff, 0xffffffff), False)):
        case(f'd18-merge-{_label}-{_mode}', mode=_mode, control=_control, issues=('540',))(
            d18_merge(_system, _user, current='嚎' if _label.startswith('count-') else '号'))
    for _label, _count, _control in (
            ('stored-zero', 0, False), ('seed-before', 0x7fffffff, True), ('seed-zero', 0x80000000, False),
            ('seed-after', 0x80000001, False), ('total-max', 0xffffffff - 22080, True),
            ('total-overflow', 0xffffffff - 22080 + 1, False)):
        case(f'd18-train-{_label}-{_mode}', mode=_mode, control=_control, issues=('540',))(d18_bigram_update(_count))
for _label, _count, _control in (('max', 0xffffffff - 69, True),
                                ('zero', 0xffffffff - 68, False), ('after', 0xffffffff - 67, False)):
    case('d18-predicted-total-' + _label, control=_control, issues=('540',))(d18_bigram_update(_count, predicted=True))
for _count in (0x3fffffff, 0x40000000, 0x7fffffff, 0x80000000, 0x80000001, 0xffffffff):
    case(f'd18-bigram-export-{_count:x}', control=True, issues=('540',))(d18_bigram_export(_count))

# Review regressions: row/record presence survives a wrapped zero total/count
# (074a2219 ngram.cpp:135-175,277-358; phonetic_lookup.h:600-640).
for _mode in ('pinyin', 'zhuyin'):
    case('d18-review-user-only-zero-total-' + _mode, mode=_mode, issues=('540',))(
        d18_merge(None, (0, 0xffffffff), current='嚎', empty_system=True, extra_successor=True))
    for _label, _lam in (('default', None), ('half', '0.5'), ('zero-lambda', '0')):
        case(f'd18-review-zero-count-zero-total-{_label}-{_mode}', mode=_mode,
             control=_label != 'default' or _mode == 'pinyin', issues=('540',))(
            d18_merge((1, 1), (0xffffffff, 0xffffffff),
                      current='好' if _label == 'default' else '嚎', lam=_lam))
    case('d18-review-unigram-library-two-' + _mode, mode=_mode, control=True, issues=('540',))(
        d18_unigram(True, text='丠'))
case('d18-review-predicted-user-only-zero-total', issues=('540',))(
    d18_bigram_update(0xffffffff - 69, predicted=True, user_only=True))


# #739 review: save_db refuses removing a directory on all three cells
# (074a2219 chewing_large_table2_bdb.cpp:138-140,
# phrase_large_table3_bdb.cpp:153-155; chewing_large_table2_kyotodb.cpp:125-127,
# phrase_large_table3_kyotodb.cpp:140-142; chewing_large_table2_tkrzwdb.cpp:115-118,
# phrase_large_table3_tkrzwdb.cpp:106-109), but zhuyin_save still renames the
# untouched temporary directory (zhuyin.cpp:650-679). A regular live index
# prevents that rename. Both facades and all destination variants are
# verified on bdb, kc and tkrzw (#1120).
def stale_index_tmp(k, index):
    out = {}
    for destination in ('absent', 'empty-directory', 'live-file'):
        k.user = tempfile.mkdtemp(prefix='user-', dir=k.scratch)
        inst = k.inst
        parse, text = ('parse_more_full_pinyins', b'nihao') if k.mode == 'pinyin' else ('parse_more_chewings', b'su3cl3')
        def train():
            assert k.fn(parse, Z, P, S)(inst, text)
            assert k.fn('guess_sentence', B, P)(inst)
            return (k.fn('train', B, P, C.c_ubyte)(inst, 0) if k.mode == 'pinyin'
                    else k.fn('train', B, P)(inst))
        assert train()
        live = Path(k.user, index)
        if destination == 'live-file':
            assert k.fn('save', B, P)(k.ctx)
            before = live.read_bytes()
            assert train()
        elif destination == 'empty-directory':
            live.mkdir()
        tmp = Path(k.user, index + '.tmp')
        tmp.mkdir()
        (tmp / 'stale-marker').write_bytes(b'unchanged temporary directory')
        ret = k.fn('save', B, P)(k.ctx)
        out[destination] = {
            'save': ret,
            'tmp-present': tmp.exists(),
            'stale-moved': (live / 'stale-marker').is_file() and
                (live / 'stale-marker').read_bytes() == b'unchanged temporary directory',
            'live-preserved': destination == 'live-file' and live.read_bytes() == before,
            'messages': list(k.log_messages),
        }
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None
    return out


for _mode in ('pinyin', 'zhuyin'):
    for _index in ('user_pinyin_index.bin', 'user_phrase_index.bin'):
        case(f'issue542-stale-{_index}-{_mode}', mode=_mode, stderr=True,
             control=_mode == 'pinyin', issues=('542',))(
            lambda k, index=_index: stale_index_tmp(k, index))


# #542's remaining named audit inputs, kept in the existing isolated worker.
def phrase_segment_invalid_utf8(k):
    ret = k.fn('phrase_segment', B, P, S)(k.inst, b'\xff\xfe')
    return {'ret': ret, 'messages': k.log_messages}


def train_no_choose_save(k):
    inst = k.inst
    parse, text = ('parse_more_full_pinyins', b'nihao') if k.mode == 'pinyin' else ('parse_more_chewings', b'su3cl3')
    out = {'parse': k.fn(parse, Z, P, S)(inst, text),
           'guess': k.fn('guess_sentence', B, P)(inst)}
    if k.mode == 'pinyin':
        out['train'] = k.fn('train', B, P, C.c_ubyte)(inst, 0)
    else:
        out['train'] = k.fn('train', B, P)(inst)
    out['save'] = k.fn('save', B, P)(k.ctx)
    out['files'] = sorted(os.listdir(k.user))
    return out


for _mode in ('pinyin', 'zhuyin'):
    case('issue542-phrase-segment-invalid-utf8-' + _mode, mode=_mode, issues=('542',))(phrase_segment_invalid_utf8)
    case('issue542-train-no-choose-save-' + _mode, mode=_mode, control=True, issues=('542',))(train_no_choose_save)
    case('issue542-parse-full-one-values-' + _mode, mode=_mode, control=True, issues=('542',))(
        lambda k: {t.decode(): key_out(k, t) for t in (b'n', b'ni3')})


@case('issue542-alias-prefix-borrowed', issues=('542',))
def _(k):
    out = {}
    for predicted_call in (False, True):
        inst = full_inst(k, b'nihao')
        k.fn('guess_sentence', B, P)(inst)
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1E)
        cand, text = P(UNTOUCHED), P(UNTOUCHED)
        assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, 0, C.byref(cand))
        assert k.fn('get_candidate_string', B, P, P, C.POINTER(P))(inst, cand, C.byref(text))
        label = 'predicted' if predicted_call else 'with-prefix'
        out[label + ' borrowed text'] = C.string_at(text.value).decode()
        name = 'guess_predicted_candidates' if predicted_call else 'guess_sentence_with_prefix'
        out[label] = k.fn(name, B, P, P)(inst, text)
        if not predicted_call:
            out['sentences'] = [sentence_out(k, inst, i) for i in range(3)]
        else:
            count = U(UNTOUCHED)
            assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            out['predicted candidate count'] = count.value
            out['predicted sentences'] = [sentence_out(k, inst, i) for i in range(3)]
            # The same text at caller-owned storage must still predict.
            out['owned prefix recovery'] = k.fn('guess_predicted_candidates', B, P, S)(
                inst, '你好'.encode())
            assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            out['owned prefix candidate count'] = count.value
    return out


# #740: removal subtracts from the stored guint32 total, never from a sum
# of survivors (074a2219 ngram.cpp:76-102, ngram_bdb.cpp:215-254;
# pinyin.cpp:1230,3766; zhuyin.cpp:765). Read persisted rows after save
# and again after reopen so the live container and export paths agree.
def removal_bigram(total, survivor_counts, removed_count, operation, multiple=False, unchanged_total=17):
    def probe(k):
        reading = b"da2'da2" if k.mode == 'pinyin' else 'ㄉㄚˊ ㄉㄚˊ'.encode()
        _add_user_phrase(k, '龘龘', [reading])
        token = next(t for t in tokens_of(k, '龘龘') if t >> 24 == 7)
        predecessors = [tokens_of(k, text)[0] for text in ('你', '我')]
        successors = [tokens_of(k, text)[0] for text in ('好', '号')]
        assert k.fn('save', B, P)(k.ctx)
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None

        records = list(zip(successors, survivor_counts)) + [(token, removed_count)]
        if multiple:
            records.append((token + 1, 1))
        gram = total.to_bytes(4, 'little') + b''.join(
            t.to_bytes(4, 'little') + count.to_bytes(4, 'little') for t, count in sorted(records))
        # The second predecessor is untouched, with a deliberately independent
        # stored total. Removing zero items must leave that total alone too.
        unchanged = d18_gram(unchanged_total, successors[0], 2)
        keys = [p.to_bytes(4, 'little') for p in predecessors]
        path = Path(k.user, 'user_bigram.db')
        d18_bigram_db(k, path, list(zip(keys, (gram, unchanged))))
        _write_user_conf(k)

        if operation == 'remove-candidate':
            assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'dada') == 4
            assert k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0)
            candidates = _user_candidate_rows(k, k.inst)
            assert candidates, 'missing removable user candidate'
            removed = k.fn('remove_user_candidate', B, P, P)(k.inst, candidates[0])
        else:
            mask, value = (0xff000000, 0x07000000) if multiple else (0xffffffff, token)
            removed = k.fn('mask_out', B, P, U, U)(k.ctx, mask, value)
        # Neither pin removal call sets m_modified. An unconstrained train
        # marks it without changing any bigram, making save observable.
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'wo') == 2
        assert k.fn('guess_sentence', B, P)(k.inst)
        trained = (k.fn('train', B, P, C.c_ubyte)(k.inst, 0) if k.mode == 'pinyin'
                   else k.fn('train', B, P)(k.inst))
        saved = k.fn('save', B, P)(k.ctx)
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None
        stored = [value.hex() for value in d18_bigram_db(k, path, read_keys=keys)]
        assert k.ctx, 'reopen after removal failed'
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None
        reopened = [value.hex() for value in d18_bigram_db(k, path, read_keys=keys)]
        return dict(removed=removed, trained=trained, saved=saved, stored=stored, reopened=reopened)
    return probe


for _mode, _operation in (('pinyin', 'mask-out'), ('pinyin', 'remove-candidate'),
                         ('zhuyin', 'mask-out')):
    for _label, _total, _survivors, _removed in (
            ('reported', 1, (0xffffffff, 1), 1),
            ('underflow', 0, (2, 3), 1),
            ('independent-total', 17, (2, 3), 4),
            ('remove-zero', 17, (2, 3), 0),
            ('zero-survivors', 1, (0, 0), 1),
            ('wrap-after', 1, (2, 3), 0xffffffff),
            ('ordinary', 9, (2, 3), 4)):
        case(f'removal-bigram-{_label}-{_operation}-{_mode}', mode=_mode,
             control=_label == 'ordinary', issues=('740',))(
            removal_bigram(_total, _survivors, _removed, _operation,
                          unchanged_total=2 if _label == 'ordinary' else 17))
    if _operation == 'mask-out':
        case(f'removal-bigram-multiple-{_mode}', mode=_mode, issues=('740',))(
            removal_bigram(1, (0xffffffff, 0), 1, _operation, multiple=True))


# #757 PR 2: the pin keeps the stored SingleGram array as it is and searches
# it with std::lower_bound (074a2219 stl_lite.h:29; ngram.cpp:161, 185, 215,
# 239, 259), so an unsorted array answers deterministically (Q1) and
# duplicated items are kept: get/set act on the first match, mask_out removes
# every match and subtracts each count, retrieve_all yields stored order (Q2,
# ngram.cpp:86-99, 133-151). Training (phonetic_lookup.h:876-909), predicted
# training (pinyin.cpp:2622-2638), mask and removal (pinyin.cpp:1230, 3764-3766;
# ngram_bdb.cpp:217-254 and twins), export (pinyin.cpp:775-787, 885-890) and
# prediction (pinyin.cpp:2321-2341) all run over that raw array.
def _gram_bytes(total, items):
    return total.to_bytes(4, 'little') + b''.join(
        t.to_bytes(4, 'little') + c.to_bytes(4, 'little') for t, c in items)


def _gram_items(raw):
    if raw is None:
        return None
    return [int.from_bytes(raw[:4], 'little'),
            [[int.from_bytes(raw[i:i + 4], 'little'), int.from_bytes(raw[i + 4:i + 8], 'little')]
             for i in range(4, len(raw) - len(raw[4:]) % 8, 8)]]


def _plant_user_grams(k, phrases, grams):
    """Add `phrases` (text, pinyin, zhuyin, count) as user phrases, save and
    close, then replace user_bigram.db with `grams(tokens)` — a list of
    (predecessor, raw value) — in this cell's own format. Answers the
    phrases' user tokens and the planted keys."""
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    add = k.fn('iterator_add_phrase', B, P, S, S, I)
    for text, pinyin, zhuyin, count in phrases:
        assert add(it, text.encode(), (pinyin if k.mode == 'pinyin' else zhuyin).encode(), count)
    k.fn('end_add_phrases', None, P)(it)
    users = [next(t for t in tokens_of(k, text) if t >> 24 == 7) for text, _, _, _ in phrases]
    system = {text: tokens_of(k, text)[0] for text in ('你', '好', '号', '我')}
    assert k.fn('save', B, P)(k.ctx)
    k.fn('free_instance', None, P)(k._inst)
    k._inst = None
    k.fn('fini', None, P)(k._ctx)
    k._ctx = None
    rows = [(p.to_bytes(4, 'little'), value) for p, value in grams(system, users)]
    d18_bigram_db(k, Path(k.user, 'user_bigram.db'), rows)
    _write_user_conf(k)
    return system, users, [key for key, _ in rows]


def _close_and_read_grams(k, keys):
    k.fn('free_instance', None, P)(k._inst)
    k._inst = None
    k.fn('fini', None, P)(k._ctx)
    k._ctx = None
    values = d18_bigram_db(k, Path(k.user, 'user_bigram.db'), read_keys=keys, allow_missing=True)
    return [_gram_items(value) for value in values]


def _mark_modified_by_unconstrained_train(k):
    # Neither removal call sets m_modified; an unconstrained train marks it
    # without changing a bigram, so the save that follows is observable.
    assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'wo') == 2
    assert k.fn('guess_sentence', B, P)(k.inst)
    return (k.fn('train', B, P, C.c_ubyte)(k.inst, 0) if k.mode == 'pinyin'
            else k.fn('train', B, P)(k.inst))


_DALONG = ('龘龘', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ', 5)
_BINGBING = ('靐靐', "da3'da3", 'ㄉㄚˇ ㄉㄚˇ', 5)


def user_gram_layout(kind, operation):
    """`kind` 'unsorted': the 你 gram holds [龘龘, 号, 好] in descending token
    order. `kind` 'duplicate': it holds 好 twice and 龘龘 twice, sorted.
    Every count passes the export threshold (68) and the predicted filter."""
    def grams(system, users):
        hao, hao2, dalong = system['好'], system['号'], users[0]
        if kind == 'unsorted':
            items = sorted([(dalong, 90), (hao2, 80), (hao, 70)], reverse=True)
        else:
            items = [(hao, 70), (hao, 80), (dalong, 90), (dalong, 100)]
        total = sum(count for _, count in items)
        return [(system['你'], _gram_bytes(total, items))]

    def probe(k):
        system, users, keys = _plant_user_grams(k, [_DALONG], grams)
        out = {}
        if operation == 'train-save':
            assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
            out['parse'] = consumed(k, 'parse_more_full_pinyins', b'nihao', fresh=False)
            out['guess'] = k.fn('guess_sentence', B, P)(k.inst)
            choices = []
            for offset, text in ((0, '你'), (2, '好')):
                if k.mode == 'pinyin':
                    assert k.fn('guess_candidates', B, P, Z, U)(k.inst, offset, 0x1e)
                else:
                    assert k.fn('guess_candidates_after_cursor', B, P, Z)(k.inst, offset)
                candidate = row_with_text(k, k.inst, 2, text)
                assert candidate, 'missing normal candidate: ' + text
                choices.append(k.fn('choose_candidate', I, P, Z, P)(k.inst, offset, candidate))
            out['choices'] = choices
            out['chosen-guess'] = k.fn('guess_sentence', B, P)(k.inst)
            out['train'] = (k.fn('train', B, P, C.c_ubyte)(k.inst, 0) if k.mode == 'pinyin'
                            else k.fn('train', B, P)(k.inst))
            out['save'] = k.fn('save', B, P)(k.ctx)
        elif operation in ('mask-save', 'remove-candidate-save'):
            if operation == 'remove-candidate-save':
                assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'dada') == 4
                assert k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0)
                rows = _user_candidate_rows(k, k.inst)
                assert rows, 'missing removable user candidate'
                out['removed'] = k.fn('remove_user_candidate', B, P, P)(k.inst, rows[0])
            else:
                # Unsorted: mask 龘龘 out of the gram. Duplicate: mask both 好.
                value = users[0] if kind == 'unsorted' else system['好']
                out['masked'] = k.fn('mask_out', B, P, U, U)(k.ctx, 0xffffffff, value)
            out['trained'] = _mark_modified_by_unconstrained_train(k)
            out['save'] = k.fn('save', B, P)(k.ctx)
        elif operation == 'export':
            it = k.fn('begin_get_bigram_phrases', P, P)(k.ctx)
            has = k.fn('bigram_iterator_has_next_phrase', B, P)
            nxt = k.fn('bigram_iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P),
                       C.POINTER(I))
            steps, falses = [], 0
            # The do/while loads the last predecessor's gram on a `has` that
            # answers false, and scans it on the next one (row 36).
            while falses < 2 and len(steps) < 16:
                if not has(it):
                    falses += 1
                    steps.append('no-next')
                    continue
                falses = 0
                phrase, reading, count = P(), P(), I(UNTOUCHED)
                ret = nxt(it, C.byref(phrase), C.byref(reading), C.byref(count))
                steps.append([ret, k.text(phrase.value), k.text(reading.value), count.value])
            k.fn('end_get_bigram_phrases', None, P)(it)
            out['steps'] = steps
            return out
        out['grams'] = _close_and_read_grams(k, keys)
        return out
    return probe


def user_gram_predicted(kind):
    """Two user phrases with equal counts score equally, so the predicted
    list's stable sort keeps their retrieve_all order. 'unsorted' plants them
    in descending token order; 'duplicate' plants 龘龘 twice, the first above
    the filter (10) and the second below it."""
    def grams(system, users):
        dalong, bingbing = users
        if kind == 'unsorted':
            items = sorted([(dalong, 20), (bingbing, 20)], reverse=True)
        else:
            items = [(dalong, 20), (dalong, 3), (bingbing, 20)]
        return [(system['你'], _gram_bytes(sum(c for _, c in items), items))]

    def probe(k):
        _plant_user_grams(k, [_DALONG, _BINGBING], grams)
        out = {'ret': k.fn('guess_predicted_candidates', B, P, S)(k.inst, '你'.encode())}
        rows = []
        n = U()
        k.fn('get_n_candidate', B, P, C.POINTER(U))(k.inst, C.byref(n))
        for i in range(n.value):
            cand, text, kind_ = P(), S(), I()
            k.fn('get_candidate', B, P, U, C.POINTER(P))(k.inst, i, C.byref(cand))
            k.fn('get_candidate_string', B, P, P, C.POINTER(S))(k.inst, cand, C.byref(text))
            k.fn('get_candidate_type', B, P, P, C.POINTER(I))(k.inst, cand, C.byref(kind_))
            rows.append([kind_.value, text.value.decode() if text.value else None])
        out['rows'] = rows
        return out
    return probe


for _kind in ('unsorted', 'duplicate'):
    for _mode in ('pinyin', 'zhuyin'):
        for _operation in ('train-save', 'mask-save'):
            case(f'user-gram-{_kind}-{_operation}-{_mode}', mode=_mode, issues=('757',))(
                user_gram_layout(_kind, _operation))
    case(f'user-gram-{_kind}-remove-candidate-save', issues=('757',))(user_gram_layout(_kind, 'remove-candidate-save'))
    case(f'user-gram-{_kind}-export', strjoinv_over_read=True, issues=('757', 'row:1'))(
        user_gram_layout(_kind, 'export'))
    case(f'user-gram-{_kind}-predicted', issues=('757',))(user_gram_predicted(_kind))


# Q1's abort (register row 85): training 你 after the sentence start finds
# the start's gram ordered [1, 2, 3, 6, 4] around it (你 = 5). get_freq
# misses, insert_freq appends, and set_freq's second lower_bound lands on the
# larger token, so check_result(user->set_freq(token, freq + seed)) dies
# (074a2219 phonetic_lookup.h:888-903; ngram.cpp:178-205, 252-271).
def train_unsorted_set_freq_miss(k):
    def grams(system, users):
        ni = system['你']
        items = [(ni - 4, 1), (ni - 3, 1), (ni - 2, 1), (ni + 1, 1), (ni - 1, 1)]
        return [(1, _gram_bytes(5, items))]

    _plant_user_grams(k, [], grams)
    assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    assert consumed(k, 'parse_more_full_pinyins', b'nihao', fresh=False) == 5
    assert k.fn('guess_sentence', B, P)(k.inst)
    for offset, text in ((0, '你'), (2, '好')):
        if k.mode == 'pinyin':
            assert k.fn('guess_candidates', B, P, Z, U)(k.inst, offset, 0x1e)
        else:
            assert k.fn('guess_candidates_after_cursor', B, P, Z)(k.inst, offset)
        candidate = row_with_text(k, k.inst, 2, text)
        assert candidate, 'missing normal candidate: ' + text
        assert k.fn('choose_candidate', I, P, Z, P)(k.inst, offset, candidate) > 0
    assert k.fn('guess_sentence', B, P)(k.inst)
    ret = (k.fn('train', B, P, C.c_ubyte)(k.inst, 0) if k.mode == 'pinyin'
           else k.fn('train', B, P)(k.inst))
    return {'ret': ret}


for _mode in ('pinyin', 'zhuyin'):
    case(f'abort-train-unsorted-set-freq-miss-{_mode}', mode=_mode, abort=False, issues=('757',))(
        train_unsorted_set_freq_miss)


# Q3: an item-less gram under a key the mask leaves alone is loaded,
# `SingleGram::mask_out` removes nothing, and `Bigram::mask_out` skips it
# untouched — `if (0 == num) { delete gram; continue; }` (074a2219
# ngram_bdb.cpp:238-241, ngram_kyotodb.cpp:219-222, ngram_tkrzwdb.cpp:195-198)
# — whatever its total; register row 88 records that the pin completes the
# mask and leaves the state.
def itemless_gram_untouched(operation, total):
    def grams(system, users):
        return [(system['你'], _gram_bytes(140, [(system['好'], 70), (users[0], 70)])),
                (system['我'], total.to_bytes(4, 'little'))]

    def probe(k):
        system, users, keys = _plant_user_grams(k, [_DALONG], grams)
        out = {}
        if operation == 'remove-candidate':
            assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'dada') == 4
            assert k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0)
            rows = _user_candidate_rows(k, k.inst)
            assert rows, 'missing removable user candidate'
            out['removed'] = k.fn('remove_user_candidate', B, P, P)(k.inst, rows[0])
        else:
            out['masked'] = k.fn('mask_out', B, P, U, U)(k.ctx, 0xffffffff, users[0])
        out['trained'] = _mark_modified_by_unconstrained_train(k)
        out['save'] = k.fn('save', B, P)(k.ctx)
        out['grams'] = _close_and_read_grams(k, keys)
        return out
    return probe


# (name, operation, facade): the name keeps the case ids already cited.
for _name, _operation, _mode in (('pinyin', 'mask-out', 'pinyin'),
                                 ('zhuyin', 'mask-out', 'zhuyin'),
                                 ('remove-candidate', 'remove-candidate', 'pinyin')):
    for _label, _total in (('total-zero', 0), ('total-nonzero', 17)):
        case(f'mask-out-itemless-gram-untouched-{_name}-{_label}', mode=_mode, issues=('757',))(
            itemless_gram_untouched(_operation, _total))


# Q4: an item-less user gram still merges its total with the system gram's
# (074a2219 merge_single_gram, ngram.cpp:298-306). The sentence start's user
# gram carries a total that wraps the merged total to 1, so every bigram from
# the sentence start dominates the first phrase. The phrase segmenter merges
# per step (phrase_lookup.cpp:269-293); the n-best guess merges per row and is
# the control. `texts` replaces the guess inputs; `plant=False` leaves the user
# bigram out and keeps the close-and-reopen around it.
def itemless_user_total(operation, texts=(b'nihaoshijie', b'womenshizhongguoren'), plant=True):
    def probe(k):
        system_total = int.from_bytes(
            d18_bigram_db(k, Path(k.data, 'bigram.db'), read_keys=(b'\x01\x00\x00\x00',))[0][:4],
            'little')
        user_total = (1 - system_total) % (1 << 32)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = None
        k._inst = None
        if plant:
            d18_bigram_db(k, Path(k.user, 'user_bigram.db'),
                          [(b'\x01\x00\x00\x00', user_total.to_bytes(4, 'little'))])
        _write_user_conf(k)
        out = {}
        if operation == 'phrase-segment':
            segment = k.fn('phrase_segment', B, P, S)
            n_phrase = k.fn('get_n_phrase', B, P, C.POINTER(U))
            token = k.fn('get_phrase_token', B, P, U, C.POINTER(U))
            # 今天天气很好 changes its first phrases once the total is
            # merged; the other text keeps its segmentation either way.
            for text in ('今天天气很好', '我们是中国人'):
                ret = segment(k.inst, text.encode())
                count = U(UNTOUCHED)
                n_phrase(k.inst, C.byref(count))
                tokens = []
                for index in range(count.value if count.value != UNTOUCHED else 0):
                    slot = U(UNTOUCHED)
                    token(k.inst, index, C.byref(slot))
                    tokens.append(slot.value)
                out[text] = [ret, tokens]
        else:
            for text in texts:
                inst = k.alloc()
                k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
                out[text.decode()] = [k.fn('guess_sentence', B, P)(inst),
                                      [sentence_out(k, inst, index) for index in range(3)]]
                k.fn('free_instance', None, P)(inst)
        return out
    return probe


case('itemless-user-total-phrase-segment', issues=('757',))(itemless_user_total('phrase-segment'))

case('itemless-user-total-guess', control=True, issues=('757',))(itemless_user_total('guess'))


# #791: with the sentence start's merged total wrapped to 1, bigram_poss is
# count / 1, the first phrase's blend is above one, and the pin takes its log
# uncapped (074a2219 phonetic_lookup.h:692-693; bigram_poss = freq / (gfloat)
# total, ngram.cpp:171): a positive score. For 研究生命起源 that makes 研究 the
# beam head, so the unigram pass (topresults[0] only, phonetic_lookup.h:546-547)
# stores a second copy of the 生命 -> 起源 chain beside the bigram pass's
# (eval_item has no identity check, phonetic_lookup_heap.h:56-66): the pin's
# rows are 研究生命起源 twice, then 研究生名气元. A blend clamped at certainty
# moved the head, so that copy was never stored. The control is the same input
# with no planted record, where both sides already store both copies.
case('itemless-user-total-guess-above-one', issues=('791',))(
    itemless_user_total('guess', texts=(b'yanjiushengmingqiyuan',)))

case('itemless-user-total-guess-above-one-control', control=True, issues=('791',))(
    itemless_user_total('guess', texts=(b'yanjiushengmingqiyuan',), plant=False))


# #798: the n-best unigram denominator is the facade's guint32 total
# (074a2219 phrase_index.h:440, returned at :615-617), read at
# phonetic_lookup.h:651 (unigram arm) and :679 (blended arm) and at
# phrase_lookup.cpp:315 and :334. Importing a new user phrase adds the item
# with frequency 0 and then add_unigram_frequency(count * 3)
# (pinyin.cpp:598-605), which adds to the facade total with no overflow check
# before the sub-index checks its own (phrase_index.h:628-634;
# phrase_index.cpp:169-175). `total` is the facade total the import leaves,
# before the guint32 wraps it; the user library's own total stays under 2^32.
def import_facade_total(total):
    def probe(k):
        count, rest = divmod(total - FACADE_TOTAL, 3)
        assert rest == 0 and 3 * count <= 0xFFFFFFFF and count < 2 ** 31, total
        out = {'count': count, 'added': import_rows(k, 7, [('妈', 'ma1', count)])}
        out.update(_guess_out(k, b"ni3'hao3"))
        out['segment'] = _segment_out(k, ('你好',))
        return out
    return probe


# The count #798 names: the total wraps to 51050831. The pin's third n-best
# row is 你好 again; a total that did not wrap ranks 拟好 there.
case('import-wrapped-facade-total-guess', issues=('798',))(import_facade_total(2 ** 32 + 51050831))

# A total that wraps to 2000000 also moves the phrase segment: the pin splits
# 你好 into 你 and 好, a total that did not wrap keeps the phrase.
case('import-wrapped-facade-total-segment', issues=('798',))(import_facade_total(2 ** 32 + 2000000))

# Just under the boundary: the total is 2^32 - 1 and nothing wraps.
case('import-facade-total-below-wrap', control=True, issues=('798',))(import_facade_total(2 ** 32 - 1))


# A library's total is the one its file stores (074a2219 phrase_index.cpp:345),
# and the facade adds it whole at load (:237-248). user.bin's header is set
# here past the sum of its items so the facade wraps to 2000000 on reopen.
# The save after the next import writes the items' sum again: pinyin_save
# compacts first (pinyin.cpp:1139), which rebuilds each library's total from
# its items (phrase_index.cpp:648-675) and leaves the facade's alone.
def stored_header_total(header):
    def probe(k):
        out = {'added': import_rows(k, 7, [('你好', "ni3'hao3", 5)])}
        out['saved'] = k.fn('save', B, P)(k.ctx)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = None
        k._inst = None
        path = Path(k.user, 'user.bin')
        payload = bytearray(path.read_bytes()[8:])
        out['stored'] = int.from_bytes(payload[:4], 'little')
        payload[:4] = header.to_bytes(4, 'little')
        path.write_bytes(_chunk(bytes(payload)))
        _write_user_conf(k)
        out.update(_guess_out(k, b"ni3'hao3"))
        out['segment'] = _segment_out(k, ('你好',))
        out['import'] = import_rows(k, 7, [('好你', "hao3'ni3", 2)])
        out['resaved'] = k.fn('save', B, P)(k.ctx)
        out['header'] = int.from_bytes(Path(k.user, 'user.bin').read_bytes()[8:12], 'little')
        return out
    return probe


case('stored-header-facade-total')(stored_header_total(2 ** 32 + 2000000 - FACADE_TOTAL))

# A header a little off the items' sum moves nothing visible.
case('stored-header-facade-total-small', control=True)(stored_header_total(7))


# A phrase imported into a system library is saved as an ADD record of
# gb_char.dbin, after a header whose new total already counts it (diff,
# phrase_index.cpp:392-399). On reopen the merge sets the total from that
# header and then adds the item again (add_phrase_item, :453-459), so the
# facade counts its unigram twice. The count makes the doubled total wrap
# to 2000003.
def system_add_reload(count):
    def probe(k):
        out = {'added': import_rows(k, 1, [('龘龘', "da3'da3", count)])}
        out['saved'] = k.fn('save', B, P)(k.ctx)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = None
        k._inst = None
        out.update(_guess_out(k, b"ni3'hao3"))
        out['segment'] = _segment_out(k, ('你好',))
        return out
    return probe


case('system-add-reload-facade-total')(system_add_reload((2 ** 32 + 2000003 - FACADE_TOTAL) // 6))

# The same flow with a count too small to move the ranking.
case('system-add-reload-facade-total-small', control=True)(system_add_reload(5))


# #801 review. add_unigram_frequency raises the item's own unigram with the
# facade total (074a2219 phrase_index.h:628-634, phrase_index.cpp:150-178), and
# the n-best step reads both from the live item (phonetic_lookup.h:651, :679).
def _stored_library_total(k, name):
    """A system library chunk's stored total, after the MemoryChunk header."""
    return int.from_bytes(Path(k.data, name).read_bytes()[8:12], 'little')


def _facade_total_out(k):
    out = _guess_out(k, b"ni3'hao3")
    out['candidates'] = guess_rows(k, k.inst, 0, 0x1e)['rows'][:8]
    out['segment'] = _segment_out(k, ('你好',))
    return out


def bumped_item_guess(k):
    token = [t for t in tokens_of(k, '号') if t >> 24 == 1][0]
    out = {'before': unigram_of(k, token)}
    out['add'] = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, token, 5000000)
    out['after'] = unigram_of(k, token)
    out.update(_facade_total_out(k))
    return out


case('token-add-unigram-numerator-guess', issues=('801',))(bumped_item_guess)


# Choosing a LONGER row trains its token through the same add, 69 * 7 = 483
# each time (pinyin.cpp:2521-2530). Two thousand choices of 你们 for `ni`
# make the pin read 跟你们 as 根你们.
def longer_choices_guess(k):
    inst = k.inst
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    chosen = 0
    for _ in range(2000):
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
        cand = row_of(k, inst, 7)
        if not cand or k.fn('choose_candidate', I, P, Z, P)(inst, 0, cand) != 1:
            break
        chosen += 1
    out = {'chosen': chosen}
    for text in (b'gennimen', b'nimen'):
        fresh = k.alloc()
        k.fn('parse_more_full_pinyins', Z, P, S)(fresh, text)
        k.fn('guess_sentence', B, P)(fresh)
        out[text.decode()] = [final_step_sentences(k, fresh),
                              guess_rows(k, fresh, 0, 0x1e)['rows'][:6]]
    k._inst = None
    out['segment'] = _segment_out(k, ('跟你们',))
    return out


case('longer-choices-numerator-guess', issues=('801',))(longer_choices_guess)


# Unloading a library takes its sub-index's whole total out of the facade,
# accepted bumps included, and loading it maps the stored items again
# (phrase_index.cpp:261-269, :237-248). The bump on a GBK token wraps the
# facade total to 2000000 while the library is loaded.
def unload_accepted_bump(k):
    token = 0x02000001
    out = {'gbk': _stored_library_total(k, 'gbk_char.bin'), 'before': unigram_of(k, token)}
    delta = (2 ** 32 + 2000000 - FACADE_TOTAL) % 2 ** 32
    out['add'] = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, token, delta)
    out['loaded'] = _facade_total_out(k)
    out['unload'] = k.fn('unload_phrase_library', B, P, U)(k.ctx, 2)
    k._inst = None
    out['unloaded'] = _facade_total_out(k)
    out['load'] = k.fn('load_phrase_library', B, P, U)(k.ctx, 2)
    k._inst = None
    out['reloaded'] = _facade_total_out(k)
    out['after'] = unigram_of(k, token)
    return out


case('unload-drops-accepted-bump', issues=('801',))(unload_accepted_bump)


# #805: `pinyin_load_phrase_library` / `zhuyin_load_phrase_library` map a
# SYSTEM_FILE library again and merge only its saved `.dbin`
# (074a2219 pinyin.cpp:264-277, zhuyin.cpp:298-312), after `unload` freed
# the sub-index with everything the session had changed in it
# (phrase_index.cpp:261-268). An unsaved training step, an imported reading
# or an imported item is gone after the reload; the user index tables are
# not unloaded, so `lookup_tokens` still answers a dropped item's token while
# the item reads answer false. The pin's `guess_candidates` dereferences
# that dangling token (class (b)): the subject answers it as a missing item.
RELOAD_WATCH = ('礙', '龘龘')


def _reload_pron_keys(k, token):
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    count = U(UNTOUCHED)
    ok = k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(k.inst, token, C.byref(count))
    out = [ok, count.value]
    for nth in range(count.value if ok else 0):
        keys = new(0, 0, 2)
        k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, nth, keys)
        view = C.cast(keys, C.POINTER(Arr)).contents
        out.append(C.string_at(view.data, view.len * 2).hex() if view.len else '')
    return out


def _reload_candidates(k, text):
    """The candidate count, the first three rows and where the watched
    phrases sit in the list a fresh guess builds."""
    inst = k.alloc()
    parse = 'parse_more_full_pinyins' if k.mode == 'pinyin' else 'parse_more_chewings'
    k.fn(parse, Z, P, S)(inst, text)
    if k.mode == 'pinyin':
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1e)
    else:
        k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    head, where = [], {}
    for i in range(count.value):
        cand, text_out, kind = P(), S(), I()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text_out))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        shown = text_out.value.decode() if text_out.value else None
        if i < 3:
            head.append([shown, kind.value])
        if shown in RELOAD_WATCH:
            where[shown] = [i, kind.value]
    k.fn('free_instance', None, P)(inst)
    return [count.value, head, where]


def _reload_snapshot(k, phrases, inputs):
    k._inst = None
    out = {}
    for phrase in phrases:
        tokens = list(tokens_of(k, phrase))
        out['tokens ' + phrase] = [hex(token) for token in tokens]
        for token in tokens:
            out['unigram %s %s' % (phrase, hex(token))] = unigram_of(k, token)
            out['readings %s %s' % (phrase, hex(token))] = _reload_pron_keys(k, token)
    for text in inputs:
        out['candidates ' + text.decode()] = _reload_candidates(k, text)
    return out


def _reload_probe(change, library, save_first):
    def probe(k):
        phrases, inputs = change(k, False)
        out = {'before': _reload_snapshot(k, phrases, inputs)}
        change(k, True)
        out['changed'] = _reload_snapshot(k, phrases, inputs)
        if save_first:
            out['save'] = bool(k.fn('save', B, P)(k.ctx))
        out['unload'] = k.fn('unload_phrase_library', B, P, U)(k.ctx, library)
        k._inst = None
        out['load'] = k.fn('load_phrase_library', B, P, U)(k.ctx, library)
        k._inst = None
        out['reloaded'] = _reload_snapshot(k, phrases, inputs)
        return out
    return probe


def _reload_train(phrase, token_library, reading):
    def change(k, apply):
        if not apply:
            return [phrase], [reading]
        parse = 'parse_more_full_pinyins' if k.mode == 'pinyin' else 'parse_more_chewings'
        assert k.fn(parse, Z, P, S)(k.inst, reading) == len(reading)
        token = [t for t in tokens_of(k, phrase) if t >> 24 == token_library][0]
        train_selected_top(k, phrase, token)
    return change


def _reload_import(library, rows, phrases, inputs):
    def change(k, apply):
        if not apply:
            return phrases, inputs
        import_rows(k, library, rows)
    return change


# 礙 is GBK-only (0x02000005): library 2 is the one library `pinyin_unload_
# phrase_library` lets go, and `癫痫` (library 1) is one it refuses.
_RELOAD_PINYIN = (
    ('train', 2, _reload_train('礙', 2, b'ai4')),
    ('reading', 2, _reload_import(2, [('礙', 'ai1', 50)], ['礙'], [b'ai1'])),
    ('new-item', 2, _reload_import(2, [('龘龘', "da3'da3", 50)], ['龘龘'], [])),
    ('train-library-1', 1, _reload_train('癫痫', 1, b'dian4xian4')),
    ('import-library-1', 1, _reload_import(1, [('癫痫', "dian3'xian4", 50)], ['癫痫'], [b'dian3xian4'])),
)
# The zhuyin facade runs the same path (zhuyin.cpp:298-312) and lets every
# library but TSI_DICTIONARY (1) go. Inputs are standard-layout bopomofo.
_RELOAD_ZHUYIN = (
    ('train', 2, _reload_train('礙', 2, b'94')),
    ('reading', 2, _reload_import(2, [('礙', 'ㄞ', 50)], ['礙'], [b'9'])),
    ('new-item', 2, _reload_import(2, [('龘龘', 'ㄉㄚˇ ㄉㄚˇ', 50)], ['龘龘'], [])),
    ('train-library-1', 1, _reload_train('癫痫', 1, b'2u04vu04')),
)
for _mode, _table in (('pinyin', _RELOAD_PINYIN), ('zhuyin', _RELOAD_ZHUYIN)):
    for _name, _library, _change in _table:
        _tag = '%s-%s' % (_mode, _name)
        if _library == 1:
            # The facade refuses to unload library 1: both calls answer false
            # and the session's change stays.
            case('reload-refused-%s' % _tag, mode=_mode, control=True, issues=('805',))(_reload_probe(_change, _library, False))
            continue
        case('reload-drops-session-%s' % _tag, mode=_mode, issues=('805',))(_reload_probe(_change, _library, False))
        case('reload-keeps-saved-%s' % _tag, mode=_mode, control=True, issues=('805',))(_reload_probe(_change, _library, True))


# Library 7 is the USER_FILE library: `unload` deletes its sub-index and the
# load builds it again from the saved `user.bin` chunk, or an empty sub-index
# when none was saved (zhuyin.cpp:247-265; pinyin twin pinyin.cpp:303-321).
# The pinyin facade never unloads it (pinyin.cpp:470-471). A dropped item's
# token stays in the phrase table, so no input is walked for it (class (b),
# policy row 102).
def _reload_user(setup, change, phrases, inputs, save_setup, save_change):
    def probe(k):
        if setup:
            import_rows(k, 7, setup)
            if save_setup:
                assert k.fn('save', B, P)(k.ctx)
        k._inst = None
        out = {'before': _reload_snapshot(k, phrases, inputs)}
        import_rows(k, 7, change)
        out['changed'] = _reload_snapshot(k, phrases, inputs)
        if save_change:
            out['save'] = bool(k.fn('save', B, P)(k.ctx))
        out['unload'] = k.fn('unload_phrase_library', B, P, U)(k.ctx, 7)
        k._inst = None
        out['load'] = k.fn('load_phrase_library', B, P, U)(k.ctx, 7)
        k._inst = None
        out['reloaded'] = _reload_snapshot(k, phrases, inputs)
        return out
    return probe


_RELOAD_USER = {
    'pinyin': (("hao3'ni3", "hao4'ni3"), b'hao3ni3'),
    'zhuyin': (('ㄏㄠˇ ㄋㄧˇ', 'ㄏㄠˋ ㄋㄧˇ'), b'cl3su3'),
}
for _mode, (_readings, _input) in _RELOAD_USER.items():
    _first, _second = _readings
    _item = [('好你', _first, 50)]
    _more = [('好你', _second, 50)]
    # An unsaved item (nothing saved: the sub-index comes back empty) and an
    # unsaved reading on a saved item; the controls save first.
    _user = (
        ('item', [], _item, [], False, False, False),
        ('item-saved', [], _item, [_input], False, True, True),
        ('reading', _item, _more, [_input], True, False, False),
        ('reading-saved', _item, _more, [_input], True, True, True),
    )
    for _name, _setup, _change, _inputs, _save_setup, _save_change, _control in _user:
        _probe = _reload_user(_setup, _change, ['好你'], _inputs, _save_setup, _save_change)
        _tag = '%s-%s' % (_mode, _name)
        if _mode == 'pinyin':
            # The pinyin facade refuses to unload library 7: both calls answer
            # false and the session's change stays.
            case('reload-refused-user-%s' % _tag, mode=_mode, control=True, issues=('805',))(_probe)
        else:
            case('reload-user-%s' % _tag, mode=_mode, control=_control, issues=('805',))(_probe)


# After the reload the phrase table and the user pinyin index still hold the
# dropped token, and an import takes `get_range(7).m_range_end` as its token
# (zhuyin.cpp:472-478): a different phrase gets the dropped token again, and
# the kept index entry for the old reading now finds it; the same phrase meets
# its own token in the phrase table (zhuyin.cpp:447-468).
def _reload_reuse_setup(k):
    out = {'first': import_rows(k, 7, [('好你', 'ㄏㄠˇ ㄋㄧˇ', 50)])}
    out['unload'] = k.fn('unload_phrase_library', B, P, U)(k.ctx, 7)
    k._inst = None
    out['load'] = k.fn('load_phrase_library', B, P, U)(k.ctx, 7)
    k._inst = None
    return out


@case('reload-user-zhuyin-reuse-other', mode='zhuyin', issues=('805',))
def _(k):
    out = _reload_reuse_setup(k)
    out['again'] = import_rows(k, 7, [('你好', 'ㄋㄧˇ ㄏㄠˇ', 50)])
    out['after'] = _reload_snapshot(k, ['好你', '你好'], [b'cl3su3', b'su3cl3'])
    return out


# The entries the pin's user pinyin index kept for the dropped token are real
# rows: a save writes them, so a reopen still finds the old reading on the
# phrase that took the token, and a `mask_out` over the library's token range
# removes them, so a phrase that takes the token afterwards is not found under
# the old reading.
_REUSE_INPUTS = [b'cl3su3', b'su3cl3', b'cl3cl3']


@case('reload-user-zhuyin-reuse-other-save-reopen', mode='zhuyin', issues=('805',))
def _(k):
    out = _reload_reuse_setup(k)
    out['again'] = import_rows(k, 7, [('你好', 'ㄋㄧˇ ㄏㄠˇ', 50)])
    out['save'] = bool(k.fn('save', B, P)(k.ctx))
    k._inst = None
    k.fn('fini', None, P)(k.ctx)
    k._ctx = None
    out['after'] = _reload_snapshot(k, ['好你', '你好'], _REUSE_INPUTS[:2])
    return out


@case('reload-user-zhuyin-reuse-other-mask', mode='zhuyin', issues=('805',))
def _(k):
    out = _reload_reuse_setup(k)
    out['again'] = import_rows(k, 7, [('你好', 'ㄋㄧˇ ㄏㄠˇ', 50)])
    out['mask'] = k.fn('mask_out', B, P, U, U)(k.ctx, 0xFF000000, 0x07000000)
    k._inst = None
    out['third'] = import_rows(k, 7, [('好好', 'ㄏㄠˇ ㄏㄠˇ', 50)])
    out['after'] = _reload_snapshot(k, ['好你', '你好', '好好'], _REUSE_INPUTS)
    return out


# The same phrase meets its own token in the phrase table, `get_phrase_item`
# finds no item, and `assert(phrase_length == item.get_phrase_length())`
# (zhuyin.cpp:454) aborts the pin: class (c), the site the import iterator's
# other two asserts (`:440`, `:457`) already answer.
@case('reload-user-zhuyin-reuse-same', mode='zhuyin', abort=False, issues=('805',))
def _(k):
    _reload_reuse_setup(k)
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    return {'ret': k.fn('iterator_add_phrase', B, P, S, S, I)(
        it, '好你'.encode(), 'ㄏㄠˇ ㄋㄧˇ'.encode(), 50)}


# The dangling token: `lookup_tokens` keeps answering it, the item reads
# answer false, and a candidate walk over it is the pin's SIGSEGV
# (class (b)); the subject adds no candidate for it, as for a missing item.
def _reload_dangling(walk, reading):
    def probe(k):
        import_rows(k, 2, [('龘龘', reading, 50)])
        k._inst = None
        token = [t for t in tokens_of(k, '龘龘') if t >> 24 == 2][0]
        k.fn('unload_phrase_library', B, P, U)(k.ctx, 2)
        k._inst = None
        k.fn('load_phrase_library', B, P, U)(k.ctx, 2)
        k._inst = None
        return {'tokens': [hex(t) for t in tokens_of(k, '龘龘')], 'unigram': unigram_of(k, token),
                'readings': _reload_pron_keys(k, token), 'candidates': _reload_candidates(k, walk)}
    return probe


_RELOAD_DANGLING = {'tokens': ['0x2003cfd'], 'unigram': [False, 0], 'readings': [False, 0],
                    'candidates': [88, [['大大', 2], ['鞑靼', 2], ['达达', 2]], {}]}
case('reload-dangling-token-candidates', crash=_RELOAD_DANGLING, issues=('805',))(_reload_dangling(b'dada', "da3'da3"))
case('reload-dangling-token-candidates-zhuyin', mode='zhuyin',
     crash=_RELOAD_DANGLING, issues=('805',))(_reload_dangling(b'283283', 'ㄉㄚˇ ㄉㄚˇ'))


# `unload` takes the library's total out of the facade, an unsaved import's
# mass included (phrase_index.cpp:261-268): while library 2 is gone, the
# sentence and candidate order divide by the smaller total, and the reload
# puts the saved state's share back.
def _reload_total_window(k):
    out = {}

    def look(tag):
        k._inst = None
        for text in (b'nihao', b'women', b'xian', b'zhongguo'):
            inst = k.alloc()
            k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
            k.fn('guess_sentence', B, P)(inst)
            out['%s sentence %s' % (tag, text.decode())] = sentence_out(k, inst, 0)
            out['%s candidates %s' % (tag, text.decode())] = guess_rows(k, inst, 0, 0x1e)['rows'][:12]
            k.fn('free_instance', None, P)(inst)

    look('loaded')
    import_rows(k, 2, [('礙', 'ai1', 2000000000), ('龘龘', "da3'da3", 2000000000)])
    look('imported')
    out['unload'] = k.fn('unload_phrase_library', B, P, U)(k.ctx, 2)
    look('unloaded')
    out['load'] = k.fn('load_phrase_library', B, P, U)(k.ctx, 2)
    look('reloaded')
    return out


case('reload-total-while-unloaded', issues=('805',))(_reload_total_window)


# A system-library import reloaded from its .dbin is counted twice by the
# merge (phrase_index.cpp:453-459 after the header at :503-514). A mask_out
# that removes it reloads the chunk and merges the masked log, taking out the
# sub-index's current total and putting the new one in (:301-332, :237-248).
# Before any compact, the current total still carries the second count; after
# one (here, an import's end, pinyin.cpp:657) it does not, and the facade keeps
# it. The count's reload offset alone wraps the facade total to 1900001. A
# build with no reload offset at all lands on the pin's total in the first
# case and misses it in the second; one that keeps the load-time offset
# through the mask misses the first.
def mask_out_reload(compact):
    def probe(k):
        count = (2 ** 32 + 1900001 - FACADE_TOTAL) // 3
        assert _stored_library_total(k, 'gb_char.bin') + 3 * count <= 0xFFFFFFFF
        out = {'added': import_rows(k, 1, [('龘龘', "da3'da3", count)])}
        out['saved'] = k.fn('save', B, P)(k.ctx)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = None
        k._inst = None
        token = [t for t in tokens_of(k, '龘龘') if t >> 24 == 1][0]
        if compact:
            out['import'] = import_rows(k, 7, [('好你', "hao3'ni3", 2)])
        out['mask'] = k.fn('mask_out', B, P, U, U)(k.ctx, 0xFFFFFFFF, token)
        k._inst = None
        out.update(_facade_total_out(k))
        return out
    return probe


case('mask-out-replaces-reload-offset', control=True, issues=('805',))(mask_out_reload(False))
case('mask-out-after-compact-keeps-reload-offset', issues=('805',))(mask_out_reload(True))


# #802. SubPhraseIndex::add_unigram_frequency refuses when m_total_freq +
# delta wraps (074a2219 phrase_index.cpp:170-171), and until the first compact
# m_total_freq is the total stored in the library file (:345), not its items'
# sum. A MODIFY_HEADER log sets library 1's stored total _STORED_GAP short of
# the guint32 wrap: a delta of GAP - 1 is accepted, GAP is refused. A save
# compacts (pinyin.cpp:1130), the total becomes the items' sum, and GAP is
# accepted. A build that checks the item sum accepts every one of them.
_STORED_GAP = 5000000


def stored_total_overflow(refused, after_save):
    def probe(k):
        total = _stored_library_total(k, 'gb_char.bin')
        k.fn('fini', None, P)(k.ctx)
        k._ctx = None
        k._inst = None
        _write_user_conf(k)
        _write_log(k, _modify_header(0, total, 2 ** 32 - _STORED_GAP))
        # The refused delta overshoots the wrap by the amount that lands the
        # facade total on 4000000000, which moves the ranking off the unbumped one.
        delta = _STORED_GAP - 1
        if refused:
            delta = _STORED_GAP + (4000000000 - FACADE_TOTAL + total) % 2 ** 32
            assert _STORED_GAP <= delta < 2 ** 32
        token = [t for t in tokens_of(k, '号') if t >> 24 == 1][0]
        add = k.fn('token_add_unigram_frequency', B, P, U, U)
        out = {'before': unigram_of(k, token)}
        out['add'] = add(k.inst, token, delta)
        out['after'] = unigram_of(k, token)
        # A refused delta still raised the facade total (phrase_index.h:633-634),
        # which the guess and the candidate frequencies divide by.
        out['facade'] = _facade_total_out(k)
        if after_save:
            # An import marks the context modified; the save compacts.
            out['import'] = import_rows(k, 7, [('好你', "hao3'ni3", 2)])
            out['saved'] = k.fn('save', B, P)(k.ctx)
            out['add-saved'] = add(k.inst, token, delta)
            out['after-saved'] = unigram_of(k, token)
            out['facade-saved'] = _facade_total_out(k)
        return out
    return probe


case('stored-total-overflow-below', control=True, issues=('802',))(stored_total_overflow(False, False))
case('stored-total-overflow-above', issues=('802',))(stored_total_overflow(True, False))
case('stored-total-overflow-above-saved', issues=('802',))(stored_total_overflow(True, True))
case('stored-total-overflow-below-saved', control=True, issues=('802',))(stored_total_overflow(False, True))


# Each MODIFY_HEADER must land on the merge's running total, or the merge
# stops there; one that lands sets the total (phrase_index.cpp:503-514). Two
# headers from the same old total: the second stops the merge. A chained pair:
# both apply. Either way the pin's final total wraps the facade to 2000000.
def log_headers(chained):
    def probe(k):
        total = _stored_library_total(k, 'gb_char.bin')
        shift = (2 ** 32 + 2000000 - FACADE_TOTAL) % 2 ** 32
        if chained:
            first = (total + 1000000000) % 2 ** 32
            log = _modify_header(0, total, first) + _modify_header(0, first, (total + shift) % 2 ** 32)
        else:
            log = _modify_header(0, total, (total + shift) % 2 ** 32) + _modify_header(0, total, total)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = None
        k._inst = None
        _write_user_conf(k)
        _write_log(k, log)
        return {'total': total, **_facade_total_out(k)}
    return probe


case('log-headers-second-stops-merge')(log_headers(False))
case('log-headers-chained-both-apply')(log_headers(True))


# --------------------------------------------------------------------------
# #757 PR 3, the lookup side. The system and user grams merge as the pin's
# merge_single_gram builds them (074a2219 ngram.cpp:277-352: both absent,
# no row; one absent, its bytes; both present, the two-pointer interleave),
# and the lookups read the merged chunk as the pin does: the n-best step
# with SingleGram::search per range, one step per yielded item
# (phonetic_lookup.h:594-632, ngram.cpp:153-176); DYNAMIC_ADJUST and the
# phrase segmenter with get_freq from lower_bound (pinyin.cpp:1845-1852,
# phrase_lookup.cpp:268-290, ngram.cpp:231-250). Nothing is sorted,
# deduplicated or filtered on a count. The grams sit under the sentence
# start, the predecessor of the first phrase in all three lookups.
# --------------------------------------------------------------------------
_PAST_RANGES = 0x0fffffff
_BOOST = 1 << 31


def _start_gram(kind, token):
    """The sentence start's planted gram. 'unsorted': [past-every-range,
    token], so lower_bound from any range start lands on the first item and
    the walk stops there; the boosted `token` is never met. 'duplicate':
    `token` twice, whose counts wrap to 1 when summed, so only a reader that
    keeps both items sees the boost."""
    if kind == 'unsorted':
        return _gram_bytes(_BOOST + 1, [(_PAST_RANGES, 1), (token, _BOOST)])
    return _gram_bytes(_BOOST, [(token, _BOOST), (token, (1 << 32) - _BOOST + 1)])


def _replace_user_bigram(k, rows):
    """Close the context and replace user_bigram.db with `rows`, a list of
    (predecessor, raw value), in this cell's own format."""
    if k._inst is not None:
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
    k.fn('fini', None, P)(k.ctx)
    k._ctx = None
    d18_bigram_db(k, Path(k.user, 'user_bigram.db'),
                  [(p.to_bytes(4, 'little'), value) for p, value in rows])
    _write_user_conf(k)


def _replace_system_start_row(k, value):
    """A copy of the system tables whose bigram.db holds `value` under the
    sentence start."""
    if k._inst is not None:
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
    k.fn('fini', None, P)(k.ctx)
    k._ctx = None
    copied = tempfile.mkdtemp(prefix='sys-', dir=k.scratch)
    shutil.copytree(k.data, copied, dirs_exist_ok=True)
    k.data = copied
    d18_bigram_db(k, Path(copied, 'bigram.db'), [(b'\x01\x00\x00\x00', value)])
    _write_user_conf(k)


def _guess_out(k, text=b'nihao'):
    if k.mode == 'zhuyin':
        # The zhuyin facade parses full pinyin under this option word, as
        # the d18 merge cases do.
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    inst = k.inst
    parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
    guessed = k.fn('guess_sentence', B, P)(inst)
    return dict(parsed=parsed, guessed=guessed, sentences=final_step_sentences(k, inst))


def _dynamic_adjust_out(k, text=b'nihao'):
    assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | (1 << 9))
    inst = k.inst
    parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
    rows = guess_rows(k, inst, 0, 0x1e) if k.mode == 'pinyin' else zhuyin_guess_rows(k, inst, 0)
    return dict(parsed=parsed, **rows)


def _segment_out(k, texts):
    segment = k.fn('phrase_segment', B, P, S)
    n_phrase = k.fn('get_n_phrase', B, P, C.POINTER(U))
    token = k.fn('get_phrase_token', B, P, U, C.POINTER(U))
    out = {}
    for text in texts:
        ret = segment(k.inst, text.encode())
        count = U(UNTOUCHED)
        n_phrase(k.inst, C.byref(count))
        tokens = []
        for index in range(count.value if count.value != UNTOUCHED else 0):
            slot = U(UNTOUCHED)
            token(k.inst, index, C.byref(slot))
            tokens.append(slot.value)
        out[text] = [ret, tokens]
    return out


def _system_start_tokens(k):
    raw = d18_bigram_db(k, Path(k.data, 'bigram.db'), read_keys=(b'\x01\x00\x00\x00',))[0]
    return [token for token, _ in _gram_items(raw)[1]]


def _pin_lower_bound(at, length, token):
    # libstdc++'s __lower_bound, as SingleGram's std_lite::lower_bound runs.
    first = 0
    while length > 0:
        half = length >> 1
        if at(first + half) < token:
            first, length = first + half + 1, length - half - 1
        else:
            length = half
    return first


def _interleave_miss_layout(system, token):
    """A user gram [token, z, a] (a < token < z) that get_freq finds on its
    own but misses once merged with the sorted system gram `system`: the
    merge keeps the user order and places `a` after `z`, out of order, and
    `z` is picked so that the merged lower_bound probes `a`, steps past it
    and lands beyond `token` (ngram.cpp:308-349, :231-250)."""
    assert system == sorted(system), 'the installed start gram is sorted'
    present, below = set(system), sum(1 for item in system if item < token)
    tied = int(token in present)
    a = next(t for t in range(token - 1, 0, -1) if t not in present)
    gaps = [item + 1 for item in system[below:] if item + 1 not in present and item + 1 > token]
    if token + 1 not in present:
        gaps.insert(0, token + 1)
    for z in gaps:
        before_z = sum(1 for item in system if item < z)
        merged = (system[:below] + [token] + system[below + tied:before_z] + [z, a]
                  + system[before_z:])
        start = _pin_lower_bound(lambda i: merged[i], len(merged), token)
        walk = next((t for t in merged[start:] if t >= token), None)
        if walk != token:
            return [(token, _BOOST), (z, 1), (a, 1)]
    raise AssertionError('no interleave misses the token')


# 尼 is a ni reading the n-best and the candidate list rank below 你; the
# boost would put it first for a reader that met it. A boosted 今天 splits
# 今天天气 off the start of 今天天气很好.
def user_gram_lookup(kind, operation):
    def probe(k):
        boosted = tokens_of(k, '今天' if operation == 'phrase-segment' else '尼')[0]
        if operation == 'phrase-segment' and kind == 'unsorted':
            # The phrase segmenter reads each pair, so a gram get_freq
            # misses on its own reads the same merged or not; only the
            # merge's interleave tells the two apart.
            items = _interleave_miss_layout(_system_start_tokens(k), boosted)
            value = _gram_bytes(_BOOST + 2, items)
        else:
            value = _start_gram(kind, boosted)
        _replace_user_bigram(k, [(1, value)])
        if operation == 'guess':
            return _guess_out(k)
        if operation == 'dynamic-adjust':
            return _dynamic_adjust_out(k)
        return _segment_out(k, ('今天天气很好',))
    return probe


for _kind in ('unsorted', 'duplicate'):
    for _operation in ('guess', 'dynamic-adjust', 'phrase-segment'):
        # A duplicated user gram reads the same per pair as merged: the
        # merge sums the first duplicate with the system item and appends
        # the second after it, so get_freq meets the same first match.
        control = (_kind, _operation) == ('duplicate', 'phrase-segment')
        case(f'user-gram-{_kind}-{_operation}', control=control, issues=('757',))(
            user_gram_lookup(_kind, _operation))


# PR 0's ranges and SingleGram::search's one-sided bound (ngram.cpp:161-173):
# the walk stops at the first token at or past the range end, and nothing
# checks the range start. The user gram [你, T, T] for a ni reading T below
# 你 merges to [..., 你, T, T, ...] (ngram.cpp:308-349), so the walk over
# 你's range yields both T items, below that range, and the pin steps T
# with each (phonetic_lookup.h:625-629). The two T counts wrap to 1 when
# summed, so a reader that sorts and sums the gram does not boost T.
@case('user-gram-unsorted-range-boundary', issues=('757',))
def _(k):
    ni = tokens_of(k, '你')[0]
    below = next(token for token in (tokens_of(k, text)[0] for text in '泥拟逆腻妮倪匿溺尼')
                 if token + 1 < ni)
    items = [(ni, 1), (below, _BOOST), (below, (1 << 32) - _BOOST + 1)]
    _replace_user_bigram(k, [(1, _gram_bytes(_BOOST, items))])
    return _guess_out(k)


# The system gram enters the merge as stored, with no user gram beside it.
# 'duplicate' holds 尼 at 1 and then at the boost: search yields both items,
# so the boosted one steps; a reader that keeps the first match does not.
def system_gram_lookup(kind):
    def probe(k):
        ni2 = tokens_of(k, '尼')[0]
        value = (_start_gram(kind, ni2) if kind == 'unsorted'
                 else _gram_bytes(_BOOST + 1, [(ni2, 1), (ni2, _BOOST)]))
        _replace_system_start_row(k, value)
        return _guess_out(k)
    return probe


for _kind in ('unsorted', 'duplicate'):
    case(f'system-gram-{_kind}-guess', issues=('757',))(system_gram_lookup(_kind))


# A stored zero count is an item. The n-best step takes a bigram step for
# each item `search` yields (phonetic_lookup.h:625-629) beside the unigram
# pass's step from the best predecessor, so a one-syllable best sentence
# the start's merged gram holds is answered twice and one it lacks once.
# The start's system gram is replaced by one item past every range, and
# its user gram holds the best sentence's phrase at count 0: the pin's
# merged row holds that item, so the sentence comes twice. A reader that
# drops zero counts answers it once.
@case('user-gram-zero-count-guess', issues=('757',))
def _(k):
    _replace_system_start_row(k, _gram_bytes(1, [(_PAST_RANGES, 1)]))
    best = _guess_out(k, b'a')['sentences'][0]
    _replace_user_bigram(k, [(1, _gram_bytes(0, [(tokens_of(k, best)[0], 0)]))])
    return _guess_out(k, b'a')


# N2, register row 85: a user gram shorter than its guint32 total. Beside a
# system gram the merge reads it (ngram.cpp:304, through phonetic_lookup.h:598,
# pinyin.cpp:2214, zhuyin.cpp:1487, phrase_lookup.cpp:272) and the pin dies
# at memory_chunk.h:390; alone, SingleGram::search reads it
# (ngram.cpp:165, through phonetic_lookup.h:625). The sentence start has a
# system gram; a user phrase used as a sentence prefix has none.
_SHORT = b'\x07\x00\x00'


def short_start_guess(k):
    _replace_user_bigram(k, [(1, _SHORT)])
    return {'ret': _guess_out(k)['guessed']}


for _mode in ('pinyin', 'zhuyin'):
    case(f'abort-guess-sentence-short-user-gram-system-row-{_mode}', mode=_mode, abort=False, issues=('757',))(
        short_start_guess)


@case('abort-guess-sentence-short-user-gram-no-system-row', abort=False, issues=('757',))
def _(k):
    def grams(system, users):
        return [(users[0], _SHORT)]
    _plant_user_grams(k, [_DALONG], grams)
    inst = k.inst
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao') == 5
    return {'ret': k.fn('guess_sentence_with_prefix', B, P, S)(inst, _DALONG[0].encode())}


def short_start_dynamic_adjust(k):
    _replace_user_bigram(k, [(1, _SHORT)])
    return {'ret': _dynamic_adjust_out(k)['ret']}


for _mode in ('pinyin', 'zhuyin'):
    case(f'abort-guess-candidates-dynamic-adjust-short-user-gram-{_mode}', mode=_mode,
         abort=False, issues=('757',))(short_start_dynamic_adjust)


# The same gram under DYNAMIC_ADJUST, parsed and never guessed: the pin's
# parse merges no gram (pinyin.cpp:1497-1520, zhuyin.cpp:1017-1043), so the
# parse answers its full length.
def short_start_dynamic_adjust_parse(k):
    _replace_user_bigram(k, [(1, _SHORT)])
    assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | (1 << 9))
    inst = k.inst
    parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    return dict(parsed=parsed, length=k.fn('get_parsed_input_length', Z, P)(inst))


for _mode in ('pinyin', 'zhuyin'):
    case(f'parse-dynamic-adjust-short-user-gram-{_mode}', mode=_mode, issues=('757',))(
        short_start_dynamic_adjust_parse)


@case('abort-phrase-segment-short-user-gram', abort=False, issues=('757',))
def _(k):
    _replace_user_bigram(k, [(1, _SHORT)])
    return {'ret': k.fn('phrase_segment', B, P, S)(k.inst, '今天天气很好'.encode())}


# #918: 074a2219 phrase_lookup.cpp:137-149 searches the scalar span and
# reads its predecessor before the later surrogate prevents a full path.
# ngram.cpp:303-304 reads the three-byte user's total beside the system row
# and asserts at memory_chunk.h:390. The sound twin reaches final_step:
# false, two zero tokens, no diagnostic. No whole-store preflight.
def phrase_prefix_gram(short, *, before=False, unrelated=False):
    def probe(k):
        predecessor = tokens_of(k, '好')[0] if unrelated else 1
        value = b'\x01\x00\x00' if short else _gram_bytes(1, [])
        _replace_user_bigram(k, [(predecessor, value)])
        inst = k.inst

        def result():
            count = U(0xABCDEF)
            assert k.fn('get_n_phrase', B, P, C.POINTER(U))(inst, C.byref(count))
            tokens = []
            for index in range(count.value):
                token = U(0xABCDEF)
                assert k.fn('get_phrase_token', B, P, U, C.POINTER(U))(
                    inst, index, C.byref(token))
                tokens.append(token.value)
            token = U(0xABCDEF)
            assert not k.fn('get_phrase_token', B, P, U, C.POINTER(U))(
                inst, count.value, C.byref(token))
            assert token.value == 0
            return tokens

        # No scalar span is reachable: seed a distinguishable old result
        # without reading the corrupt start gram on either implementation.
        assert not k.fn('phrase_segment', B, P, S)(inst, b'\xed\xa0\x80' * 3)
        old = result()
        assert old == [0, 0, 0]
        surrogate = b'\xed\xa0\x80'
        text = surrogate + '你'.encode() if before else '你'.encode() + surrogate
        ret = k.fn('phrase_segment', B, P, S)(inst, text)
        tokens = result()
        if short and not before and not unrelated:
            assert k.log_messages == [
                f'{k.mode}_phrase_segment: a user bigram value is shorter than a guint32 '
                'total_freq (upstream aborts, memory_chunk.h:390)'
            ]
            return dict(ret=ret, untouched_phrase_result=tokens == old)
        assert tokens == [0, 0]
        assert not k.log_messages
        return dict(ret=ret, tokens=tokens)
    return probe


for _mode in ('pinyin', 'zhuyin'):
    case(f'abort-phrase-segment-prefix-short-user-gram-{_mode}', mode=_mode,
         abort=False, issues=('918',))(phrase_prefix_gram(True))
    case(f'phrase-segment-prefix-sound-user-gram-{_mode}', mode=_mode,
         control=True, stderr=True, issues=('918',))(phrase_prefix_gram(False))
    case(f'phrase-segment-nonscalar-before-short-user-gram-{_mode}', mode=_mode,
         control=True, stderr=True, issues=('918',))(phrase_prefix_gram(True, before=True))
    case(f'phrase-segment-prefix-unrelated-short-user-gram-{_mode}', mode=_mode,
         control=True, stderr=True, issues=('918',))(phrase_prefix_gram(True, unrelated=True))


# #918 ruling: the phrase table compares the raw UCS-4 words the pin's
# converter produces. A table key `[0xd800]` and a bounded one-character
# item make `phrase_segment("\xed\xa0\x80")` return true with that token;
# no `char` or `str` exists for the word on either side.
def phrase_segment_nonscalar_raw_key(k):
    system = private_system(k)
    word = 0xd800
    token = 0x01000001
    raw_text = word.to_bytes(4, 'little')
    reading = (0x1084).to_bytes(2, 'little')
    content = bytearray(8)
    offsets = [0, 8]
    content += bytes((1, 1)) + (100).to_bytes(4, 'little') + raw_text
    content += reading + (10).to_bytes(4, 'little')
    index_two = 17 + len(offsets) * 4 + 1
    payload = b''.join(value.to_bytes(4, 'little') for value in
                       (200, 17, index_two, index_two + len(content) + 1))
    payload += b'#' + b''.join(value.to_bytes(4, 'little') for value in offsets)
    payload += b'#' + content + b'#'
    path = Path(system, 'gb_char.bin')
    path.unlink()
    path.write_bytes(_chunk(payload))
    path = Path(system, 'phrase_index.bin')
    path.unlink()
    _craft_db(str(path), [(raw_text, token.to_bytes(4, 'little'))], 1,
              system_index=True)
    _write_user_conf(k)
    k.data = system
    inst = k.inst
    assert not k.fn('phrase_segment', B, P, S)(inst, b'\xed\xbf\xbf' * 3)
    ret = k.fn('phrase_segment', B, P, S)(inst, b'\xed\xa0\x80')
    count = U(0xABCDEF)
    assert k.fn('get_n_phrase', B, P, C.POINTER(U))(inst, C.byref(count))
    tokens = []
    for index in range(count.value):
        value = U(0xABCDEF)
        assert k.fn('get_phrase_token', B, P, U, C.POINTER(U))(
            inst, index, C.byref(value))
        tokens.append(value.value)
    return dict(ret=ret, tokens=tokens)


for _mode in ('pinyin', 'zhuyin'):
    case(f'phrase-segment-nonscalar-raw-key-{_mode}', mode=_mode,
         stderr=True, issues=('918',))(phrase_segment_nonscalar_raw_key)


# Controls: a short gram the lookup never loads. 我 never precedes a phrase
# of `nihao`; with DYNAMIC_ADJUST clear the candidate list merges nothing
# (pinyin.cpp:2207-2216).
@case('guess-unrelated-short-user-gram', control=True, issues=('757',))
def _(k):
    _replace_user_bigram(k, [(tokens_of(k, '我')[0], _SHORT)])
    return _guess_out(k)


@case('guess-candidates-short-user-gram-dynamic-adjust-off', control=True, issues=('757',))
def _(k):
    _replace_user_bigram(k, [(1, _SHORT)])
    assert k.fn('set_options', B, P, U)(k.ctx, 0x18a)
    inst = k.inst
    parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    return dict(parsed=parsed, **guess_rows(k, inst, 0, 0x1e))


# --------------------------------------------------------------------------
# #563 axis B (bdb) — the libzhuyin per-entry-point contract. Four exports
# no other case reached (`zhuyin_parse_chewing`,
# `zhuyin_in_chewing_keyboard`, `zhuyin_get_n_phrase`,
# `zhuyin_get_phrase_token`), the optional out-params and the empty-instance
# boundary of `zhuyin_get_zhuyin_key_rest_positions`, and the two pin bodies
# that never read their instance. The NULL-argument cases are the libzhuyin
# half of register row 41 (class (b): the pin dereferences and dies, the
# subject's guard answers). Pin cites are in the PR body; each case names
# the site it holds.
# --------------------------------------------------------------------------


def zy_strv(k, pointer):
    """A `gchar **` a keyboard probe answered; decode and free it."""
    if pointer == UNTOUCHED:
        return 'untouched'
    if not pointer:
        return None
    free = k.glib.g_strfreev
    free.restype, free.argtypes = None, [P]
    array, out, index = C.cast(pointer, C.POINTER(S)), [], 0
    while array[index]:
        out.append(array[index].decode('utf-8', 'replace'))
        index += 1
    free(pointer)
    return out


def parse_chewing_probe(schemes, texts):
    def probe(k):
        out = {}
        for scheme in schemes:
            if scheme is not None:
                k.fn('set_chewing_scheme', B, P, I)(k.ctx, scheme)
            parse = k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))
            row, key = {}, C.c_ushort(UNTOUCHED & 0xFFFF)
            for text in texts:
                key.value = UNTOUCHED & 0xFFFF
                row[repr(text)] = [parse(k.inst, text, C.byref(key)), key.value]
            out[str(scheme)] = row
        return out
    return probe


# `parse_one_key` (Simple `zhuyin_parser2.cpp:163-215`, Discrete `:331-...`,
# DaChen `:789-...`) probes the last byte for a tone under `USE_TONE`, maps
# the rest through the scheme's symbol table and looks the chewing up in
# `zhuyin_index`; `*onekey` is written only on success. `su3cl3` holds two
# keys and is refused as one; the empty string and a tone-only byte are
# refused; every implemented scheme is probed.
case('zhuyin-parse-chewing-key', mode='zhuyin', control=True, issues=('563',))(
    parse_chewing_probe((None, 1, 2, 3, 4, 5, 6, 8, 9),
                        (b'su3', b'cl3', b'su', b'', b'zzz', b'su3cl3', b'su3x', b'1', b'3', b'ni')))


def in_chewing_probe(scheme, options, keys):
    def probe(k):
        if scheme is not None:
            k.fn('set_chewing_scheme', B, P, I)(k.ctx, scheme)
        if options is not None:
            k.fn('set_options', B, P, U)(k.ctx, options)
        keyboard = k.fn('in_chewing_keyboard', B, P, C.c_char, C.POINTER(P))
        row = {}
        for key in keys:
            symbols = P(UNTOUCHED)
            ret = keyboard(k.inst, key, C.byref(symbols))
            row[repr(key)] = [ret, zy_strv(k, symbols.value)]
        return row
    return probe


# `in_chewing_scheme` fixes `*symbols = NULL` first and fills it only when
# the byte maps (Simple `zhuyin_parser2.cpp:301-335`, Discrete `:496-540`,
# DaChen `:798-846`); the tone table is consulted only under `USE_TONE`.
case('zhuyin-in-chewing-keyboard-key', mode='zhuyin', control=True, issues=('563',))(
    in_chewing_probe(None, None, (b's', b'u', b'3', b'1', b'a', b'z', b'q', b'!', b'~', b'\x80', b'\x00', b'i')))
case('zhuyin-in-chewing-keyboard-no-tone', mode='zhuyin', control=True, issues=('563',))(
    in_chewing_probe(None, 0, (b's', b'3', b'1')))
for _zyscheme in (2, 3, 4, 5, 6, 8, 9):
    case(f'zhuyin-in-chewing-keyboard-scheme-{_zyscheme}', mode='zhuyin', control=True, issues=('563',))(
        in_chewing_probe(_zyscheme, None, (b's', b'u', b'i', b'3')))


def key_rest_positions_probe(text, offsets):
    # `zhuyin_get_zhuyin_key_rest` on an empty matrix aborts the pin (a
    # registered class (c) site held by `abort-zhuyin-get-zhuyin-key-rest-
    # empty-matrix`), so the accessor is only reachable on a parsed instance;
    # this case takes a key-rest pointer from the getter the pin's own call
    # returns, then exercises the optional `begin`/`end`.
    def probe(k):
        k.fn('set_options', B, P, U)(k.ctx, 0x1aa)
        inst = k.inst
        k.fn('parse_more_chewings', Z, P, S)(inst, text.encode())
        rest = k.fn('get_zhuyin_key_rest', B, P, Z, C.POINTER(P))
        positions = k.fn('get_zhuyin_key_rest_positions', B, P, P, C.POINTER(C.c_ushort), C.POINTER(C.c_ushort))
        out = {}
        for offset in offsets:
            slot = P(UNTOUCHED)
            got = rest(inst, offset, C.byref(slot))
            row = {'rest_ret': got,
                   'rest': 'untouched' if slot.value == UNTOUCHED else 'null' if not slot.value else 'set'}
            if got and slot.value:
                # `zhuyin.cpp:1930-1941`: both out-params are optional, neither
                # is written when NULL, and the call always answers true.
                begin, end = C.c_ushort(0xABCD), C.c_ushort(0xABCD)
                row['both'] = [positions(inst, slot, C.byref(begin), C.byref(end)), begin.value, end.value]
                begin = C.c_ushort(0xABCD)
                row['begin-only'] = [positions(inst, slot, C.byref(begin), None), begin.value]
                end = C.c_ushort(0xABCD)
                row['end-only'] = [positions(inst, slot, None, C.byref(end)), end.value]
                row['neither'] = positions(inst, slot, None, None)
            out[str(offset)] = row
        return out
    return probe


case('zhuyin-key-rest-positions-out', mode='zhuyin', control=True, issues=('563',))(
    key_rest_positions_probe('su3cl3', (0, 1, 2)))


def phrase_tokens_probe(texts):
    def probe(k):
        inst = k.inst
        segment = k.fn('phrase_segment', B, P, S)
        n_phrase = k.fn('get_n_phrase', B, P, C.POINTER(U))
        token = k.fn('get_phrase_token', B, P, U, C.POINTER(U))

        def read(prefix):
            count = U(UNTOUCHED)
            n_ret = n_phrase(inst, C.byref(count))
            rows, index = [], 0
            while index < (count.value if count.value != UNTOUCHED else 0):
                slot = U(0xDEADBEEF)
                rows.append([token(inst, index, C.byref(slot)), slot.value])
                index += 1
            past = []
            for index in (count.value if count.value != UNTOUCHED else 0, 0xFFFFFFFF):
                slot = U(0xDEADBEEF)
                past.append([token(inst, index, C.byref(slot)), slot.value])
            return {prefix + '-n': [n_ret, count.value], prefix + '-tokens': rows, prefix + '-past': past}

        out = {'empty-instance': read('empty')}
        for text in texts:
            row = {'segment': segment(inst, text)}
            row.update(read('row'))
            out[repr(text)] = row
        segment(inst, '你好'.encode())
        k.fn('reset', B, P)(inst)
        out['after-reset'] = read('reset')
        return out
    return probe


# `zhuyin_get_n_phrase` writes `*num` unconditionally and answers true
# (`zhuyin.cpp:2227-2231`); `zhuyin_get_phrase_token` writes the null token
# before its range check, so a refused index still writes 0 (`:2233-2244`).
# `phrase_segment` fills the array, `reset` empties it (`:1723-1729`).
case('zhuyin-phrase-result-tokens', mode='zhuyin', control=True, issues=('563',))(
    phrase_tokens_probe(('你好'.encode(), b'', b'123', '你好世界'.encode())))


# The two string getters' whole surface: `zhuyin_get_zhuyin_string`
# (`zhuyin.cpp:1732-1741`) renders the key's zhuyin spelling;
# `zhuyin_get_pinyin_string` (`:1743-1766`) reads the context's scheme and
# dispatches (`FULL_PINYIN_HANYU` 1, `LUOMA` 2, `SECONDARY_ZHUYIN` 3). Both
# NULL the out-param before the `get_table_index() == 0` refusal, so a
# refused zero key leaves NULL, not the caller's sentinel.
@case('zhuyin-string-getters', mode='zhuyin', control=True, issues=('563',))
def _(k):
    inst = k.inst
    key = C.c_ushort(0)
    assert k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(inst, b'su3', C.byref(key))
    zero = C.c_ushort(0)
    out = {}
    for scheme in (None, 1, 2, 3):
        if scheme is not None:
            assert k.fn('set_full_pinyin_scheme', B, P, I)(k.ctx, scheme)
        for label, name in (('zhuyin_string', 'get_zhuyin_string'),
                            ('pinyin_string', 'get_pinyin_string')):
            fn = k.fn(name, B, P, C.POINTER(C.c_ushort), C.POINTER(P))
            string = P(UNTOUCHED)
            out['%s scheme %s' % (label, scheme)] = [
                fn(inst, C.byref(key), C.byref(string)), owned_text(k, string)]
            string = P(UNTOUCHED)
            out['%s scheme %s zero-key' % (label, scheme)] = [
                fn(inst, C.byref(zero), C.byref(string)), owned_text(k, string)]
    return out


# `zhuyin_get_zhuyin_offset` in range: the cursor is clamped to the parse end
# and walked back over empty columns to the first key (`zhuyin.cpp:1973-1992`).
# The past-matrix cursor lives in `zhuyin-cursor-neighbours`.
@case('zhuyin-cursor-offset-range', mode='zhuyin', control=True, issues=('563',))
def _(k):
    inst = zhuyin_chewing(k)
    fn = k.fn('get_zhuyin_offset', B, P, Z, C.POINTER(Z))
    out = {}
    for cursor in (0, 1, 2, 3, 6):
        res = Z(UNTOUCHED)
        out[str(cursor)] = [fn(inst, cursor, C.byref(res)),
                            'untouched' if res.value == UNTOUCHED else res.value]
    return out


# The five exports whose pin bodies never read their instance:
# `zhuyin_get_zhuyin_string` (`zhuyin.cpp:1732-1741`),
# `zhuyin_get_candidate_type` (`:1858-1863`),
# `zhuyin_get_candidate_string` (`:1865-1870`),
# `zhuyin_get_zhuyin_key_rest_positions` (`:1930-1941`) and
# `zhuyin_get_zhuyin_key_rest_length` (`:1942-1946`). A NULL instance is a
# defined call there, so the subject answers what the pin answers.
@case('zhuyin-null-instance-defined-getters', mode='zhuyin', issues=('563',))
def _(k):
    inst = k.inst
    k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
    key = C.c_ushort(0)
    assert k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(inst, b'su3', C.byref(key))
    rest = P(UNTOUCHED)
    assert k.fn('get_zhuyin_key_rest', B, P, Z, C.POINTER(P))(inst, 0, C.byref(rest)) and rest.value
    assert k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    cand = P(UNTOUCHED)
    assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, 0, C.byref(cand)) and cand.value
    out = {}
    string = P(UNTOUCHED)
    out['zhuyin_string'] = [k.fn('get_zhuyin_string', B, P, C.POINTER(C.c_ushort), C.POINTER(P))(
        None, C.byref(key), C.byref(string)), owned_text(k, string)]
    begin, end = C.c_ushort(0xABCD), C.c_ushort(0xABCD)
    out['key_rest_positions'] = [k.fn('get_zhuyin_key_rest_positions', B, P, P, C.POINTER(C.c_ushort), C.POINTER(C.c_ushort))(
        None, rest, C.byref(begin), C.byref(end)), begin.value, end.value]
    length = C.c_ushort(0xBEEF)
    out['key_rest_length'] = [k.fn('get_zhuyin_key_rest_length', B, P, P, C.POINTER(C.c_ushort))(
        None, rest, C.byref(length)), length.value]
    kind = I(0x7FFFFFFF)
    out['candidate_type'] = [k.fn('get_candidate_type', B, P, P, C.POINTER(I))(
        None, cand, C.byref(kind)), kind.value]
    candidate_string = P(UNTOUCHED)
    out['candidate_string'] = [k.fn('get_candidate_string', B, P, P, C.POINTER(P))(
        None, cand, C.byref(candidate_string)),
        'untouched' if candidate_string.value == UNTOUCHED
        else C.string_at(candidate_string.value).decode('utf-8', 'replace')]
    return out


# `token_get_phrase`'s optional out-params: `_token_get_phrase`
# (`zhuyin.cpp:1095-1113`, the zhuyin twin of the shared pinyin helper)
# writes `*len` and `*utf8_str` only when their pointer is non-NULL, so all
# four combinations are defined calls — on a token that resolves and on one
# that is refused before either write.
@case('zhuyin-token-get-phrase-optional-out', mode='zhuyin', issues=('563',))
def _(k):
    fn = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))
    out = {}
    for label, token in (('known', tokens_of(k, '你好')[0]), ('unknown', 0xFFFFFFFF)):
        length, text = U(UNTOUCHED), P(UNTOUCHED)
        both = fn(k.inst, token, C.byref(length), C.byref(text))
        out[label] = [both, 'untouched' if length.value == UNTOUCHED else length.value,
                      'untouched' if text.value == UNTOUCHED else k.text(text.value)]
        length = U(UNTOUCHED)
        out[label + '-len-only'] = [fn(k.inst, token, C.byref(length), None), length.value]
        text = P(UNTOUCHED)
        out[label + '-text-only'] = [fn(k.inst, token, None, C.byref(text)),
                                     'untouched' if text.value == UNTOUCHED else k.text(text.value)]
        out[label + '-neither'] = fn(k.inst, token, None, None)
    return out


# #894: source-derived, isolated SYSTEM_FILE words. See the two discrepancy
# records for the complete producer/consumer trace, GLib 2.89.4-2 citations,
# and the binding observations. No pin output is used to generate expectations.
PHRASE_TEXT_WORDS = dict(valid=0x4e01, surrogate=0xd800, **{'above-unicode': 0x110000}, nul=0)


def phrase_text_system(k, variant):
    """Two native items: 甲<word>乙 / da1'da1'da1, then 丙 / ba1.

    Layout: phrase_index.cpp's SubPhraseIndex; PinyinIndexItem2<L> at
    pinyin_phrase3.h:181-197 (four-byte aligned token then packed keys).
    ChewingKey at chewing_key.h:41-47 has tone at bit 12; da = 4|1<<7,
    ba = 1|1<<7 (chewing_enum.h:16,19,65). No API import repairs the words.
    """
    system = private_system(k)
    word = PHRASE_TEXT_WORDS[variant]
    texts = ((0x7532, word, 0x4e59), (0x4e19,))
    keys = ((0x1084,) * 3, (0x1081,))
    content, offsets = bytearray(8), [0]
    phonetic, phrases = {}, {}
    for slot, (text, reading) in enumerate(zip(texts, keys), 1):
        token = 0x01000000 | slot
        raw_text = b''.join(value.to_bytes(4, 'little') for value in text)
        raw_keys = b''.join(value.to_bytes(2, 'little') for value in reading)
        offsets.append(len(content))
        content += bytes((len(text), 1)) + (100).to_bytes(4, 'little') + raw_text
        content += raw_keys + (10).to_bytes(4, 'little')
        index = b''.join((value & 0x0fff).to_bytes(2, 'little') for value in reading)
        for end in range(2, len(index), 2):
            phonetic.setdefault(index[:end], b'')
        item = token.to_bytes(4, 'little') + raw_keys
        phonetic[index] = item + bytes((-len(item)) % 4)
        for end in range(4, len(raw_text), 4):
            phrases.setdefault(raw_text[:end], b'')
        phrases[raw_text] = token.to_bytes(4, 'little')
    index_two = 17 + len(offsets) * 4 + 1
    payload = b''.join(value.to_bytes(4, 'little') for value in
                       (200, 17, index_two, index_two + len(content) + 1))
    payload += b'#' + b''.join(value.to_bytes(4, 'little') for value in offsets)
    payload += b'#' + content + b'#'
    path = Path(system, 'gb_char.bin')
    path.unlink()  # replace the private symlink, never the oracle's file
    path.write_bytes(_chunk(payload))
    for name, rows in (('pinyin_index.bin', phonetic), ('phrase_index.bin', phrases)):
        path = Path(system, name)
        path.unlink()
        _craft_db(str(path), list(rows.items()), 1, system_index=True)
    _write_user_conf(k)
    k.data = system


def phrase_text_bytes(k, ptr, *, owned=True):
    """Lossless text observation: never replace malformed UTF-8 with U+FFFD."""
    if ptr == UNTOUCHED:
        return 'untouched'
    if not ptr:
        return None
    value = C.string_at(ptr).hex()
    if owned:
        k.glib.g_free(ptr)
    return value


def phrase_text_probe(variant, surface):
    def probe(k):
        phrase_text_system(k, variant)
        if surface == 'export' and k.mode == 'zhuyin':
            # zhuyin.h/libzhuyin.ver export no iterator. Never borrow a
            # pinyin context or invent an unsupported zhuyin C function.
            return {'exports': {name: hasattr(k.lib, 'zhuyin_' + name) for name in
                                ('begin_get_phrases', 'iterator_has_next_phrase',
                                 'iterator_get_next_phrase', 'end_get_phrases')}}
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        if surface == 'export':
            it = k.fn('begin_get_phrases', P, P, U)(k.ctx, 1)
            rows = []
            while k.fn('iterator_has_next_phrase', B, P)(it):
                assert len(rows) < 2, 'fixture exported an unexpected extra row'
                text, reading, count = P(UNTOUCHED), P(UNTOUCHED), I(UNTOUCHED)
                ret = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
                    it, C.byref(text), C.byref(reading), C.byref(count))
                rows.append([ret, phrase_text_bytes(k, text.value),
                             phrase_text_bytes(k, reading.value), count.value])
            k.fn('end_get_phrases', None, P)(it)
            return {'rows': rows}
        spelling = b'da1da1da1' + (b'ba1' if surface == 'sentence' else b'')
        parsed = k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, spelling)
        if surface == 'sentence':
            guessed = k.fn('guess_sentence', B, P)(k.inst)
            text = P(UNTOUCHED)
            if k.mode == 'pinyin':
                ret = k.fn('get_sentence', B, P, C.c_ubyte, C.POINTER(P))(k.inst, 0, C.byref(text))
            else:
                ret = k.fn('get_sentence', B, P, C.POINTER(P))(k.inst, C.byref(text))
            return {'parsed': parsed, 'guessed': guessed, 'ret': ret,
                    'text': phrase_text_bytes(k, text.value)}
        if k.mode == 'pinyin':
            ret = k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0x1f)
        else:
            ret = k.fn('guess_candidates_after_cursor', B, P, Z)(k.inst, 0)
        count = U(UNTOUCHED)
        assert k.fn('get_n_candidate', B, P, C.POINTER(U))(k.inst, C.byref(count))
        rows = []
        for index in range(count.value):
            candidate, text, kind = P(), P(UNTOUCHED), I(UNTOUCHED)
            assert k.fn('get_candidate', B, P, U, C.POINTER(P))(k.inst, index, C.byref(candidate))
            assert k.fn('get_candidate_string', B, P, P, C.POINTER(P))(
                k.inst, candidate, C.byref(text))
            assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(
                k.inst, candidate, C.byref(kind))
            rows.append([kind.value, phrase_text_bytes(k, text.value, owned=False)])
        return {'parsed': parsed, 'ret': ret, 'rows': rows}
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _variant in PHRASE_TEXT_WORDS:
        for _surface in ('candidate', 'sentence', 'export'):
            _record = None
            if _variant != 'valid' and not (_mode == 'zhuyin' and _surface == 'export'):
                _record = ('894-nul-phrase-text.md' if _variant == 'nul'
                           else '894-non-scalar-phrase-text.md')
            case(f'phrase-text/{_mode}-{_variant}-{_surface}', mode=_mode,
                 control=True, issues=('894',), policy_record=_record)(
                     phrase_text_probe(_variant, _surface))


# #882: get_phrase_item checks the offset and two header bytes only
# (074a2219 phrase_index.cpp:180-205). Neither getter decodes text or keys
# (pinyin.cpp:2786-2798; zhuyin.cpp:1778-1790).
def stored_count_headers(user):
    def probe(k):
        item = bytes([1, 3]) + (9).to_bytes(4, 'little') + (0xd800).to_bytes(4, 'little') + b'\xff' * 18
        content = b'\0' * 8 + item + b'\x01'
        offsets = [0, 8, len(content) - 1, 0, len(content), 8]
        index_two = 17 + len(offsets) * 4 + 1
        payload = b''.join(value.to_bytes(4, 'little') for value in
                           (9, 17, index_two, index_two + len(content) + 1))
        payload += b'#' + b''.join(value.to_bytes(4, 'little') for value in offsets) + b'#' + content + b'#'
        if user:
            _write_user_conf(k)
            Path(k.user, 'user.bin').write_bytes(_chunk(payload))
            ctx = k.init()
            library = 7
        else:
            system = conf_system(k, ('gb_char.bin', 'count.bin'))
            Path(system, 'count.bin').write_bytes(_chunk(payload))
            ctx = k.init(system=system)
            library = 1
        assert ctx, 'count fixture init'
        inst = k.fn('alloc_instance', P, P)(ctx)
        count = k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))
        out = {}
        for slot in (0, 1, 2, 3, 4, 5, 6, 0xffffff):
            number = U(UNTOUCHED)
            ret = count(inst, (library << 24) | slot, C.byref(number))
            out[str(slot)] = [ret, number.value]
        for tag in (0x10000000, 0xf0000000):
            number = U(UNTOUCHED)
            token = tag | (library << 24) | 1
            out['tag ' + hex(tag)] = [count(inst, token, C.byref(number)), number.value]
        number = U(UNTOUCHED)
        out['no sub-index'] = [count(inst, 0x0f000001, C.byref(number)), number.value]
        k.fn('free_instance', None, P)(inst)
        k.fn('fini', None, P)(ctx)
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _user in (False, True):
        case('stored-count-headers-' + ('user-' if _user else 'system-') + _mode,
             mode=_mode, issues=('882',))(stored_count_headers(_user))


# #804: a 256th reading on one token. `PhraseItem::add_pronunciation` stores
# `npron + 1` through a `guint8` (`phrase_index.cpp:28`, `:88` at 074a2219),
# so the count wraps to 0 and the call still answers true. Every read sizes
# the item from that count (`SubPhraseIndex::get_phrase_item`, `:203-204`),
# and the next add copies only that view (`remove_phrase_item`, `:228`): the
# item has no readings, keeps its unigram, and the 257th add leaves exactly
# one reading, the new one. The cases fill libraries 1-7 to 255 readings with
# a first frequency of 11 and 1 after it, add the 256th, and read the item
# live, after `end_add_phrases`, and after save and reopen; the 257th add
# (frequency 9) follows in the same iterator or in the next one. A library the
# pin does not load answers false on the first add and is recorded as such.
_WRAP_PHRASES = {1: '龘', 2: '龖', 3: '龗', 4: '鬱', 5: '鑫', 6: '靐', 7: '麤'}


def _wrap_pinyin_readings():
    initials = ['b', 'p', 'm', 'f', 'd', 't', 'n', 'l', 'g', 'k', 'h', 'j', 'q', 'x', 'zh', 'ch', 'sh',
                'r', 'z', 'c', 's', 'y', 'w', '']
    finals = ['a', 'o', 'e', 'ai', 'ei', 'ao', 'ou', 'an', 'en', 'ang', 'eng', 'i', 'u', 'ia', 'ie',
              'iao', 'iu', 'ian', 'in', 'iang', 'ing', 'ua', 'uo', 'uai', 'ui', 'uan', 'un', 'uang', 'ong']
    return [(i + f + t).encode() for i in initials for f in finals for t in '1234']


def _wrap_zhuyin_readings():
    initials = 'ㄅㄆㄇㄈㄉㄊㄋㄌㄍㄎㄏㄐㄑㄒㄓㄔㄕㄖㄗㄘㄙ'
    finals = ['ㄚ', 'ㄛ', 'ㄜ', 'ㄞ', 'ㄟ', 'ㄠ', 'ㄡ', 'ㄢ', 'ㄣ', 'ㄤ', 'ㄥ', 'ㄧ', 'ㄨ', 'ㄩ', 'ㄧㄚ', 'ㄧㄝ',
              'ㄧㄠ', 'ㄧㄡ', 'ㄧㄢ', 'ㄧㄣ', 'ㄧㄤ', 'ㄧㄥ', 'ㄨㄚ', 'ㄨㄛ', 'ㄨㄞ', 'ㄨㄟ', 'ㄨㄢ', 'ㄨㄣ',
              'ㄨㄤ', 'ㄨㄥ']
    return [(i + f + t).encode() for i in initials for f in finals for t in ('ˊ', 'ˇ', 'ˋ', '')]


def _wrap_distinct(k, candidates):
    """The candidates the facade accepts as a one-syllable reading, found by
    adding each to a fresh one-character phrase on a scratch context: a new
    phrase has no reading to merge with, so only a refused parse answers
    false, and in the case itself a refused add can only be the 256th."""
    scratch = tempfile.mkdtemp(prefix='wrap-', dir=k.scratch)
    ctx = k.init(user=scratch)
    assert ctx, 'scratch context'
    iterator = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
    kept = [reading for number, reading in enumerate(candidates)
            if k.fn('iterator_add_phrase', B, P, S, S, I)(iterator, chr(0x4E00 + number).encode(), reading, 1)]
    k.fn('end_add_phrases', None, P)(iterator)
    k.fn('fini', None, P)(ctx)
    assert len(kept) >= 258, len(kept)
    return kept


def _wrap_item_reads(k, lib, text, rows, result, label):
    """Records the item `text` in `lib` as the C API shows it under `label`
    (None if it has no token); what the pin leaves indeterminate goes under
    a `~` key, which the comparison leaves out."""
    tokens = [t for t in tokens_of(k, text) if t >> 24 == lib]
    if not tokens:
        result[label] = None
        return
    token = tokens[0]
    count = U(UNTOUCHED)
    ret = k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(k.inst, token, C.byref(count))
    out, loose = {'token': hex(token), 'n': [ret, count.value]}, {}
    # An item the user store holds in the addon or network library has no
    # unigram through `token_get_unigram_frequency`, which reads the shipped
    # addon table there; that is unrelated to the wrap, so it is recorded but
    # not compared for those two libraries.
    (out if lib not in (5, 6) else loose)['unigram'] = unigram_of(k, token)
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    for nth in (0, 1, 254, 255):
        keys = new(0, 0, 2)
        ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, nth, keys)
        view = C.cast(keys, C.POINTER(Arr)).contents
        content = C.string_at(view.data, view.len * 2).hex() if view.len else ''
        # Past the last counted reading the pin hands back an untouched stack
        # buffer; the ruled answer is zeroed keys (class (b) for the content).
        if nth < count.value:
            out['nth %d' % nth] = [ret, view.len, content]
        else:
            out['nth %d' % nth] = [ret, view.len]
            loose['content nth %d' % nth] = content
    if rows:
        listed = rows(text)
        out['export'] = [len(listed), listed[:1], listed[-1:], hashlib.sha256(
            json.dumps(listed, ensure_ascii=False).encode()).hexdigest()]
    result[label] = out
    result['~' + label] = loose


def _wrap_export_rows(k, lib):
    def rows(text):
        out = k.fn('begin_get_phrases', P, P, U)(k.ctx, lib)
        has = k.fn('iterator_has_next_phrase', B, P)
        nxt = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))
        listed = []
        while has(out):
            ph, py, n = P(), P(), I()
            nxt(out, C.byref(ph), C.byref(py), C.byref(n))
            phrase, reading = k.text(ph.value), k.text(py.value)
            if phrase == text:
                listed.append([reading, n.value])
        k.fn('end_get_phrases', None, P)(out)
        return listed
    return rows


def wrap_256th_reading(k, readings, same_iterator):
    readings = _wrap_distinct(k, readings)
    add = k.fn('iterator_add_phrase', B, P, S, S, I)
    begin = k.fn('begin_add_phrases', P, P, U)
    end = k.fn('end_add_phrases', None, P)
    out = {}
    for lib, text in _WRAP_PHRASES.items():
        rows = _wrap_export_rows(k, lib) if k.mode == 'pinyin' else None
        label = 'lib %d' % lib
        pending = iter(readings)
        iterator = begin(k.ctx, lib)
        if not iterator:
            out[label] = 'no iterator'
            continue

        def step(frequency):
            reading = next(pending)
            with k.per_call_step():
                ret = add(iterator, text.encode(), reading, frequency)
                tokens = [t for t in tokens_of(k, text) if t >> 24 == lib]
                count = U(UNTOUCHED)
                if tokens:
                    k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(k.inst, tokens[0], C.byref(count))
            return ret, (count.value if tokens else 0)

        # Fill to 255 readings; a reading that merges into one the item
        # already holds leaves the count where it was.
        ret, count = step(11)
        if not ret:
            out[label] = 'first add refused'
            end(iterator)
            continue
        while count < 255:
            ret, count = step(1)
        _wrap_item_reads(k, lib, text, rows, out, label + ' at 255')
        # The 256th reading: skip any that only merge into an existing one.
        while True:
            before = count
            ret, count = step(1)
            if count != before or not ret:
                break
        out[label + ' 256th'] = [ret, count]
        _wrap_item_reads(k, lib, text, rows, out, label + ' after 256th live')
        if same_iterator:
            out[label + ' 257th add'] = step(9)[0]
            _wrap_item_reads(k, lib, text, rows, out, label + ' after 257th live')
        end(iterator)
        _wrap_item_reads(k, lib, text, rows, out, label + ' after end_add')
        if not same_iterator:
            iterator = begin(k.ctx, lib)
            out[label + ' 257th add'] = step(9)[0]
            _wrap_item_reads(k, lib, text, rows, out, label + ' after 257th live')
            end(iterator)
            _wrap_item_reads(k, lib, text, rows, out, label + ' after 257th end_add')
    out['save'] = k.fn('save', B, P)(k.ctx)
    k.fn('fini', None, P)(k.ctx)
    k._ctx = k._inst = None
    for lib, text in _WRAP_PHRASES.items():
        rows = _wrap_export_rows(k, lib) if k.mode == 'pinyin' else None
        _wrap_item_reads(k, lib, text, rows, out, 'lib %d after reload' % lib)
    return out


@case('wrap-256th-reading', issues=('804',))
def _(k):
    return wrap_256th_reading(k, _wrap_pinyin_readings(), same_iterator=False)


@case('wrap-256th-reading-same-iterator', issues=('804',))
def _(k):
    return wrap_256th_reading(k, _wrap_pinyin_readings(), same_iterator=True)


@case('wrap-256th-reading-zhuyin', mode='zhuyin', issues=('804',))
def _(k):
    return wrap_256th_reading(k, _wrap_zhuyin_readings(), same_iterator=False)


@case('wrap-256th-reading-zhuyin-same-iterator', mode='zhuyin', issues=('804',))
def _(k):
    return wrap_256th_reading(k, _wrap_zhuyin_readings(), same_iterator=True)


# Register row 41 (class (b)), the libzhuyin half: the pin dereferences a
# NULL argument and dies of SIGSEGV, the subject's guard answers. One case
# per site so the pin dies at the site under test.
def null_argument_case(call):
    def probe(k):
        return {'ret': call(k)}
    return probe


_ZY_NULL_FALSE = {'ret': False, 'logs': []}
_ZY_NULL_TRUE = {'ret': True, 'logs': []}


def zy_parse_chewing_null_instance(k):
    key = C.c_ushort(0)
    return k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(None, b'su3', C.byref(key))


def zy_parse_chewing_null_text(k):
    key = C.c_ushort(0)
    return k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(k.inst, None, C.byref(key))


def zy_parse_chewing_null_key(k):
    return k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(k.inst, b'su3', None)


def zy_in_chewing_null_instance(k):
    symbols = P(UNTOUCHED)
    return k.fn('in_chewing_keyboard', B, P, C.c_char, C.POINTER(P))(None, b's', C.byref(symbols))


def zy_in_chewing_null_symbols(k):
    return k.fn('in_chewing_keyboard', B, P, C.c_char, C.POINTER(P))(k.inst, b's', None)


def zy_pinyin_string_null_instance(k):
    key = C.c_ushort(0)
    k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(k.inst, b'su3', C.byref(key))
    string = P(UNTOUCHED)
    return k.fn('get_pinyin_string', B, P, C.POINTER(C.c_ushort), C.POINTER(P))(None, C.byref(key), C.byref(string))


def zy_zhuyin_string_null_key(k):
    string = P(UNTOUCHED)
    return k.fn('get_zhuyin_string', B, P, C.POINTER(C.c_ushort), C.POINTER(P))(k.inst, None, C.byref(string))


def zy_zhuyin_string_null_out(k):
    key = C.c_ushort(0)
    k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(k.inst, b'su3', C.byref(key))
    return k.fn('get_zhuyin_string', B, P, C.POINTER(C.c_ushort), C.POINTER(P))(k.inst, C.byref(key), None)


def zy_key_rest_positions_null_rest(k):
    begin, end = C.c_ushort(0), C.c_ushort(0)
    return k.fn('get_zhuyin_key_rest_positions', B, P, P, C.POINTER(C.c_ushort), C.POINTER(C.c_ushort))(
        k.inst, None, C.byref(begin), C.byref(end))


def zy_n_phrase_null_instance(k):
    count = U(0)
    return k.fn('get_n_phrase', B, P, C.POINTER(U))(None, C.byref(count))


def zy_n_phrase_null_out(k):
    return k.fn('get_n_phrase', B, P, C.POINTER(U))(k.inst, None)


def zy_phrase_token_null_instance(k):
    slot = U(0)
    return k.fn('get_phrase_token', B, P, U, C.POINTER(U))(None, 0, C.byref(slot))


def zy_phrase_token_null_out(k):
    return k.fn('get_phrase_token', B, P, U, C.POINTER(U))(k.inst, 0, None)


def zy_first_rest(k):
    inst = k.inst
    k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
    rest = P(UNTOUCHED)
    assert k.fn('get_zhuyin_key_rest', B, P, Z, C.POINTER(P))(inst, 0, C.byref(rest)) and rest.value
    return inst, rest


def zy_first_candidate(k):
    inst = k.inst
    k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
    assert k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    cand = P(UNTOUCHED)
    assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, 0, C.byref(cand)) and cand.value
    return inst, cand


def zy_key_rest_length_null_rest(k):
    inst, _ = zy_first_rest(k)
    length = C.c_ushort(0)
    return k.fn('get_zhuyin_key_rest_length', B, P, P, C.POINTER(C.c_ushort))(inst, None, C.byref(length))


def zy_key_rest_length_null_length(k):
    inst, rest = zy_first_rest(k)
    return k.fn('get_zhuyin_key_rest_length', B, P, P, C.POINTER(C.c_ushort))(inst, rest, None)


def zy_candidate_type_null_candidate(k):
    inst, _ = zy_first_candidate(k)
    kind = I(0)
    return k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, None, C.byref(kind))


def zy_candidate_type_null_out(k):
    inst, cand = zy_first_candidate(k)
    return k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, None)


def zy_candidate_string_null_candidate(k):
    inst, _ = zy_first_candidate(k)
    text = P(UNTOUCHED)
    return k.fn('get_candidate_string', B, P, P, C.POINTER(P))(inst, None, C.byref(text))


def zy_candidate_string_null_out(k):
    inst, cand = zy_first_candidate(k)
    return k.fn('get_candidate_string', B, P, P, C.POINTER(P))(inst, cand, None)


# Register row 43 (class (b)): `zhuyin_begin_add_phrases` stores any `guint8`
# (`zhuyin.cpp:392-398`), so a following `zhuyin_iterator_add_phrase` on
# library 16 reaches `_add_phrase`'s `phrase_index->get_range(index, range)`
# (`zhuyin.cpp:475`), which reads `m_sub_phrase_indices[16]`
# (`storage/phrase_index.cpp:611`) one past the 16-entry array
# (`storage/phrase_index.h:441`) and dies of SIGSEGV. The subject refuses
# every library that is neither a user file nor a loaded system library.
# The pinyin twin's index is not part of the row (the audit recorded library
# 255 accepted there, so that side has no stable crash to hold).
def zy_import_index_out_of_range(k):
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 16)
    if not it:
        return 'no-iterator'
    out = k.fn('iterator_add_phrase', B, P, S, S, I)(
        it, '你好'.encode(), 'ㄋㄧˇ ㄏㄠˇ'.encode(), 1)
    k.fn('end_add_phrases', None, P)(it)
    return out


case('zhuyin-add-phrase-library-16', mode='zhuyin', control=True, crash={'ret': False}, issues=('563', 'row:41'))(
    null_argument_case(zy_import_index_out_of_range))


for _zyname, _zycall, _zyanswer in (
        ('null-instance-parse-chewing', zy_parse_chewing_null_instance, _ZY_NULL_FALSE),
        ('null-text-parse-chewing', zy_parse_chewing_null_text, _ZY_NULL_FALSE),
        ('null-key-parse-chewing', zy_parse_chewing_null_key, _ZY_NULL_FALSE),
        ('null-instance-in-chewing-keyboard', zy_in_chewing_null_instance, _ZY_NULL_FALSE),
        ('null-symbols-in-chewing-keyboard', zy_in_chewing_null_symbols, _ZY_NULL_TRUE),
        ('null-instance-get-pinyin-string', zy_pinyin_string_null_instance, _ZY_NULL_FALSE),
        ('null-key-get-zhuyin-string', zy_zhuyin_string_null_key, _ZY_NULL_FALSE),
        ('null-out-get-zhuyin-string', zy_zhuyin_string_null_out, _ZY_NULL_FALSE),
        ('null-rest-get-zhuyin-key-rest-positions', zy_key_rest_positions_null_rest, _ZY_NULL_FALSE),
        ('null-rest-get-zhuyin-key-rest-length', zy_key_rest_length_null_rest, _ZY_NULL_FALSE),
        ('null-length-get-zhuyin-key-rest-length', zy_key_rest_length_null_length, _ZY_NULL_TRUE),
        ('null-candidate-get-candidate-type', zy_candidate_type_null_candidate, _ZY_NULL_FALSE),
        ('null-out-get-candidate-type', zy_candidate_type_null_out, _ZY_NULL_TRUE),
        ('null-candidate-get-candidate-string', zy_candidate_string_null_candidate, _ZY_NULL_FALSE),
        ('null-out-get-candidate-string', zy_candidate_string_null_out, _ZY_NULL_TRUE),
        ('null-instance-get-n-phrase', zy_n_phrase_null_instance, _ZY_NULL_FALSE),
        ('null-out-get-n-phrase', zy_n_phrase_null_out, _ZY_NULL_TRUE),
        ('null-instance-get-phrase-token', zy_phrase_token_null_instance, _ZY_NULL_FALSE),
        ('null-out-get-phrase-token', zy_phrase_token_null_out, _ZY_NULL_FALSE)):
    case('zhuyin-' + _zyname, mode='zhuyin', control=True, crash=_zyanswer, issues=('563', 'row:41'))(
        null_argument_case(_zycall))


def declared_sentence_residuals():
    """Read membership from the frozen gate's §12 declaration, not a copy."""
    root = Path(__file__).resolve().parents[2]
    doc = (root / 'docs/findings/sentence-surface.md').read_text()
    section = doc.split('\n## 12. ', 1)[1].split('\n## 13.', 1)[0]
    declaration = section.split(
        'Only the following six shared-comparison rows are class (a), as ruled.', 1)[1]
    table = declaration.split('| Input | Pin deciding f32 bits | Subject native costs |\n', 1)[1]
    names = set()
    for line in table.splitlines()[1:]:
        if not line.startswith('|'):
            break
        name = line.split('|')[1].strip().strip('`')
        assert name not in names, 'duplicate declared residual'
        names.add(name)
    assert len(names) == 6, 'frozen declared residual count moved'
    return names


def encode_sentence_tie(row, guessed, sentences, nbest, residual_sentences, residual_nbest):
    """Pool each text once per input; all displayed rows have NBEST type 1."""
    texts = list(dict.fromkeys(sentences + residual_sentences))
    ids = {text: index for index, text in enumerate(texts)}
    def indexed(items):
        return ','.join(str(ids[text]) for text in items)
    def displayed(items):
        return ','.join(f'{index}:{ids[text]}' for index, text in items)
    return '\t'.join(row[:3] + [str(int(guessed)), '\x01'.join(texts),
                                indexed(sentences), displayed(nbest),
                                indexed(residual_sentences), displayed(residual_nbest)])


def regenerate_sentence_ties(prefix, fixture):
    """Refresh pin expectations; preserve the six approved exact subject rows.

    The first three columns are historical native-tail selection metadata.
    Text pooling keeps indexed duplicates and complete NBEST rows compact.
    Membership comes only from the frozen §12 table; ordinary cases cannot
    carry subject expectations. No observations or other outputs are saved.
    """
    suspend_offset = time.clock_gettime(time.CLOCK_BOOTTIME) - time.clock_gettime(time.CLOCK_MONOTONIC)

    def fields(path):
        return dict(line.split('=', 1) for line in path.read_text().splitlines()
                    if line and not line.startswith('#') and '=' in line)

    manifest = fields(prefix / 'oracle-pin.txt')
    source_pin = fields(Path(__file__).resolve().parents[2] / 'tools/oracle/oracle-pin.txt')
    expected = (f"libpinyin-{source_pin['libpinyin_tag']}-{source_pin['libpinyin_commit']}"
                f"+model20-{source_pin['model_archive_sha256']}+dbm-bdb")
    assert manifest['pin_ref'] == expected and manifest['dbm'] == 'BerkeleyDB', 'off-pin bdb prefix'
    so = prefix / 'lib/libpinyin.so'
    assert hashlib.sha256(so.read_bytes()).hexdigest() == manifest['shared_object_sha256'], \
        'pin library checksum differs'
    verified_files = 0
    for name, field in (('oracle-data.sha256', 'data_manifest_sha256'),
                        ('oracle-data-unstable.sha256', 'data_unstable_manifest_sha256')):
        raw = (prefix / name).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == manifest[field], 'pin data manifest checksum differs'
        for line in raw.decode().splitlines():
            digest, relative = line.split('  ', 1)
            assert hashlib.sha256((prefix / relative).read_bytes()).hexdigest() == digest, \
                'pin data checksum differs: ' + relative
            verified_files += 1
    print(f'pin verified: library and {verified_files} data files', flush=True)

    raw = fixture.read_text()
    headers = [line for line in raw.splitlines() if line.startswith('#')]
    assert '# format=sentence-tie-order-v1' in headers, 'unsupported tie fixture format'
    cases = [line.split('\t') for line in raw.splitlines() if not line.startswith('#')]
    assert cases and all(len(row) == 9 and int(row[1]) > 0 for row in cases), 'bad tie fixture'
    assert len({row[0] for row in cases}) == len(cases), 'duplicate selected input'
    declared = declared_sentence_residuals()
    recorded = {row[0] for row in cases if row[7] or row[8]}
    assert recorded == declared, 'subject expectations must match the frozen declared membership exactly'
    out = []
    row_count = 0
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    with tempfile.TemporaryDirectory(prefix='sentence-ties-') as scratch:
        k = Kit('pinyin', str(so.resolve()), str(prefix / 'lib/libpinyin/data'), scratch)
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a)
        try:
            for ordinal, row in enumerate(cases, 1):
                text = row[0]
                pool = row[4].split('\x01')
                residual_sentences = [pool[int(index)] for index in row[7].split(',')] if row[7] else []
                residual_nbest = [(int(index), pool[int(text_id)])
                                  for index, text_id in (item.split(':') for item in row[8].split(','))] if row[8] else []
                assert k.fn('reset', B, P)(k.inst)
                k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, text.encode())
                guessed = k.fn('guess_sentence', B, P)(k.inst)
                assert k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0x1e)
                nbest = []
                for index, candidate in enumerate(candidate_rows(k, k.inst)):
                    assert candidate is not None, 'unreadable candidate for ' + repr(text)
                    if candidate[0] == 1:
                        pointer, rank = P(), C.c_ubyte(0xff)
                        assert k.fn('get_candidate', B, P, U, C.POINTER(P))(
                            k.inst, index, C.byref(pointer))
                        assert k.fn('get_candidate_nbest_index', B, P, P, C.POINTER(C.c_ubyte))(
                            k.inst, pointer, C.byref(rank))
                        nbest.append((rank.value, candidate[1]))
                sentences = final_step_sentences(k, k.inst)
                out.append(encode_sentence_tie(row, guessed, sentences, nbest,
                                              residual_sentences, residual_nbest))
                row_count += len(nbest)
                if ordinal % 1000 == 0:
                    print(f'pin expectations: {ordinal}/{len(cases)}', flush=True)
        finally:
            if k._inst:
                k.fn('free_instance', None, P)(k._inst)
            if k._ctx:
                k.fn('fini', None, P)(k._ctx)
    headers = [line for line in headers if not line.startswith('# pin_ref=')]
    headers.insert(1, '# pin_ref=' + manifest['pin_ref'])
    elapsed_suspend = (time.clock_gettime(time.CLOCK_BOOTTIME) -
                       time.clock_gettime(time.CLOCK_MONOTONIC) - suspend_offset)
    assert abs(elapsed_suspend) < 0.01, 'discard: suspend overlapped pin regeneration'
    fixture.write_text('\n'.join(headers + out) + '\n')
    print(f'regenerated {fixture}: {len(cases)} distinct inputs, '
          f'{sum(int(row[1]) for row in cases)} corpus entries, {row_count} ordered NBEST rows; '
          f'pin-equal expectations {len(cases) - len(declared)}, exact residual expectations {len(declared)}', flush=True)
    print(f'suspend overlap: none (clock delta {elapsed_suspend:.6f} seconds)', flush=True)


def axis_c_definition(*, verify_sources=False):
    """The committed replacement definition, never inferred from results.

    Loading happens at import for every run (ordinary case selection builds
    the Axis C groups too). Source-fixture hashing is deferred to
    `verify_sources=True`, asked for once in the Axis C branch, so an
    ordinary run is not failed by a changed fixture and an Axis C worker,
    which imports this module after the parent validated, does not repeat it.
    """
    root = Path(__file__).resolve().parents[2]
    definition = json.loads((root / 'tools/bisection/axis-c-replacement.json').read_text())
    assert definition['format'] == 'axis-c-replacement-v1'
    if verify_sources:
        for name, digest in definition['sources'].items():
            assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
    return definition


def axis_c_isolated(k, fn):
    """Fork one read-only observation; both pipes stay in memory.

    The context is opened before forking. Children allocate fresh instances,
    never save/train, and exit without teardown, so one abort cannot truncate
    the remaining corpus. Import cases instead use the existing fresh worker.
    """
    k.ctx
    output_read, output_write = os.pipe()
    error_read, error_write = os.pipe()
    pid = os.fork()
    if pid == 0:
        os.close(output_read)
        os.close(error_read)
        os.dup2(error_write, 2)
        os.close(error_write)
        k.logs.clear()
        k.log_messages.clear()
        try:
            result = fn()
            result['logs'] = k.logs
            result['log_messages'] = k.log_messages
            payload = json.dumps(result, sort_keys=True, ensure_ascii=True).encode()
            with os.fdopen(output_write, 'wb') as destination:
                destination.write(payload)
            os._exit(0)
        except BaseException as error:
            os.write(2, repr(error).encode())
            os._exit(1)
    os.close(output_write)
    os.close(error_write)
    # Each child emits at most short diagnostics. Drain both concurrently,
    # including a large complete candidate vector, without capture files.
    import selectors
    selector = selectors.DefaultSelector()
    selector.register(output_read, selectors.EVENT_READ, 'output')
    selector.register(error_read, selectors.EVENT_READ, 'errors')
    chunks = {'output': [], 'errors': []}
    while selector.get_map():
        for key, _ in selector.select():
            chunk = os.read(key.fd, 65536)
            if chunk:
                chunks[key.data].append(chunk)
            else:
                selector.unregister(key.fd)
                os.close(key.fd)
    selector.close()
    _, status = os.waitpid(pid, 0)
    payload = b''.join(chunks['output'])
    return dict(exit=os.waitstatus_to_exitcode(status),
                result=json.loads(payload) if payload else None,
                stderr=b''.join(chunks['errors']).decode('utf-8', 'surrogateescape'))


def axis_c_surface(k, parser, text, instance=None):
    """Parse -> lengths -> sentence rank zero -> complete candidate list."""
    inst = k.alloc() if instance is None else instance
    parsed = k.fn(parser, Z, P, S)(inst, text)
    out = dict(consumed=parsed, parsed=k.fn('get_parsed_input_length', Z, P)(inst))
    guessed = k.fn('guess_sentence', B, P)(inst)
    out['guess_sentence'] = guessed
    if guessed:
        out['sentence'] = (sentence_out(k, inst, 0) if k.mode == 'pinyin'
                           else zhuyin_sentence_out(k, inst))
    out['guess_candidates'] = (k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1e)
                               if k.mode == 'pinyin' else
                               k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0))
    count = U(UNTOUCHED)
    out['get_n_candidate'] = k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    out['n'] = count.value
    assert count.value != UNTOUCHED, 'candidate count untouched'
    out['candidates'] = candidate_rows(k, inst)
    out['candidate_multiset'] = sorted(out['candidates'], key=lambda row: json.dumps(row))
    return out


def axis_c_options(word, texts):
    def probe(k):
        set_ok = k.fn('set_options', B, P, U)(k.ctx, word)
        # Every fork gets an independent, never-parsed instance snapshot.
        # The parent never parses it; constructor work is independent of input.
        # Retain the original allocation path for a bytewise equivalence check.
        snapshot = None if os.environ.get('AXIS_C_ALLOCATE_EACH') == '1' else k.alloc()
        return dict(set_options=set_ok, observations={
            str(index): axis_c_isolated(k, lambda text=text: axis_c_surface(
                k, 'parse_more_full_pinyins', text.encode(), snapshot))
            for index, text in enumerate(texts)})
    return probe


def axis_c_lookup(k, text):
    new = k.glib.g_array_new
    new.argtypes, new.restype = [I, I, U], P
    arr = new(0, 0, 4)
    try:
        ret = k.fn('lookup_tokens', B, P, S, P)(k.inst, text, arr)
        view = C.cast(arr, C.POINTER(Arr)).contents
        return dict(ret=ret, tokens=list(C.cast(view.data, C.POINTER(U))[:view.len]))
    finally:
        k.glib.g_array_free.argtypes, k.glib.g_array_free.restype = [P, B], P
        k.glib.g_array_free(arr, True)


def axis_c_import(k, operation, text):
    phrase = text if operation == 'import-phrase' else '你好'.encode()
    reading = text if operation == 'import-reading' else (
        b"ni'hao" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode())
    it = k.fn('begin_add_phrases', P, P, C.c_ubyte)(k.ctx, 7)
    assert it, 'missing import iterator'
    added = k.fn('iterator_add_phrase', B, P, S, S, I)(it, phrase, reading, 1)
    k.fn('end_add_phrases', None, P)(it)
    out = dict(added=added, lookup=axis_c_lookup(k, phrase),
               saved=k.fn('save', B, P)(k.ctx))
    k.fn('fini', None, P)(k.ctx)
    k._ctx, k._inst = None, None
    # libzhuyin has no export iterator. Reopen either facade's saved profile
    # through the same side's libpinyin export API in a fresh worker process.
    env = dict(os.environ, AXIS_C_REOPEN_USER=k.user)
    side = os.environ['AXIS_C_COMPANION_PINYIN']
    proc = subprocess.run([sys.executable, str(WORKER), 'pinyin', side,
                           k.data, 'axis-c-export-user'], capture_output=True, env=env)
    out['reopen_exit'] = proc.returncode
    out['reopen_stderr'] = proc.stderr.decode('utf-8', 'surrogateescape').replace(k.user, 'USER')
    out['export'] = json.loads(proc.stdout) if proc.stdout else None
    return out


@case('axis-c-export-user')
def axis_c_export_user(k):
    k.user = os.environ['AXIS_C_REOPEN_USER']
    return dict(rows=d18_export_phrases(k))


def axis_c_encoding(operation, text):
    def probe(k):
        k.fn('set_options', B, P, U)(k.ctx, 0x28)
        if operation.startswith('parse_more_'):
            return axis_c_surface(k, operation, text)
        if operation == 'lookup_tokens':
            return axis_c_lookup(k, text)
        if operation == 'phrase_segment':
            ret = k.fn('phrase_segment', B, P, S)(k.inst, text)
            count = U(UNTOUCHED)
            got = k.fn('get_n_phrase', B, P, C.POINTER(U))(k.inst, C.byref(count))
            out = dict(ret=ret, get_n_phrase=got, count=count.value)
            # #751 reuses the six preregistered surrogate cases. The pin
            # returns false with six null slots and no GLib record (the
            # worker already compares domain, level and exact message).
            # Only read out the result; keep the battery's input, options
            # and state-changing operation exactly as preregistered.
            if b'\xed\xa0\x80' in text:
                token = k.fn('get_phrase_token', B, P, U, C.POINTER(U))
                def read(index):
                    slot = U(UNTOUCHED)
                    return [token(k.inst, index, C.byref(slot)), slot.value]
                out['tokens'] = ([read(index) for index in range(count.value)]
                                 if got and count.value <= len(text) else None)
                out['past'] = [read(count.value), read(0xFFFFFFFF)]
            return out
        if operation == 'remember_user_input':
            parsed = k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'nihao')
            return dict(parsed=parsed, ret=k.fn('remember_user_input', B, P, S, I)(k.inst, text, 1),
                        rows=d18_export_phrases(k))
        return axis_c_import(k, operation, text)
    return probe


def axis_c_keyboard(k, byte):
    out = P(UNTOUCHED)
    ret = k.fn('in_chewing_keyboard', B, P, C.c_char, C.POINTER(P))(
        k.inst, bytes([byte]), C.byref(out))
    if out.value == UNTOUCHED:
        return dict(ret=ret, symbols='untouched')
    if not out.value:
        return dict(ret=ret, symbols=None)
    strings = C.cast(out, C.POINTER(S))
    symbols, index = [], 0
    while strings[index] is not None:
        symbols.append(strings[index].decode())
        index += 1
    k.glib.g_strfreev.argtypes, k.glib.g_strfreev.restype = [P], None
    k.glib.g_strfreev(out)
    return dict(ret=ret, symbols=symbols)


def axis_c_layout(scheme, word, definition):
    def probe(k):
        k.fn('set_options', B, P, U)(k.ctx, word)
        def configured(fn):
            setter = 'set_zhuyin_scheme' if k.mode == 'pinyin' else 'set_chewing_scheme'
            ret = k.fn(setter, B, P, I)(k.ctx, scheme)
            return dict(set_scheme=ret, observation=fn() if ret else None)
        rows = {f'ascii-{byte}': axis_c_isolated(k, lambda byte=byte: configured(
            lambda: axis_c_keyboard(k, byte))) for byte in definition['layout_ascii']}
        for index, text in enumerate(definition['layout_inputs']):
            rows[f'input-{index}'] = axis_c_isolated(k, lambda text=text: configured(
                lambda: axis_c_surface(k, 'parse_more_chewings', text.encode())))
        return dict(observations=rows)
    return probe


AXIS_C_GROUPS = {name: [] for name in ('options', 'encoding', 'layouts')}
AXIS_C_DEFINITION = axis_c_definition()
for _kind, _words in AXIS_C_DEFINITION['words'].items():
    for _word in _words:
        _name = f'axis-c-options-{_kind}-{int(_word, 16):08x}'
        case(_name, stderr=True)(axis_c_options(int(_word, 16), AXIS_C_DEFINITION['option_inputs']))
        AXIS_C_GROUPS['options'].append(_name)
for _mode, _operation in AXIS_C_DEFINITION['encoding_operations']:
    for _input in AXIS_C_DEFINITION['encoding_inputs']:
        _name = f"axis-c-encoding-{_mode}-{_operation}-{_input['name']}"
        case(_name, mode=_mode, stderr=True)(axis_c_encoding(_operation, bytes.fromhex(_input['hex'])))
        AXIS_C_GROUPS['encoding'].append(_name)
# The invalid scheme numbers whose abort the pin takes and the register already
# classifies (rows 5c/5d). These layout cases are matched with the classic
# class (c) `abort=` expectation at the nested-observation level and counted as
# covered, not DIFFER, exactly as `abort-zhuyin-chewing-scheme-*` is.
AXIS_C_LAYOUT_COVERED = {}
_covered_rows = {scheme: entry['row'] for entry in AXIS_C_DEFINITION['layout_covered']
                 for scheme in entry['schemes']}
for _mode in ('pinyin', 'zhuyin'):
    for _scheme in AXIS_C_DEFINITION['layout_schemes']:
        for _word in AXIS_C_DEFINITION['layout_words']:
            _name = f'axis-c-layout-{_mode}-{_scheme}-{int(_word, 16):08x}'
            case(_name, mode=_mode, stderr=True)(axis_c_layout(_scheme, int(_word, 16), AXIS_C_DEFINITION))
            AXIS_C_GROUPS['layouts'].append(_name)
            if _scheme in _covered_rows:
                AXIS_C_LAYOUT_COVERED[_name] = _covered_rows[_scheme]


def axis_c_canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=True, separators=(',', ':')).encode()


def axis_c_normalize_diagnostics(value, scratch, field=''):
    """Only generated paths in diagnostics, never candidate/input payloads."""
    if isinstance(value, dict):
        return {key: axis_c_normalize_diagnostics(item, scratch, key) for key, item in value.items()}
    if isinstance(value, list):
        return [axis_c_normalize_diagnostics(item, scratch, field) for item in value]
    if isinstance(value, str) and field in ('stderr', 'reopen_stderr', 'log_messages'):
        return re.sub(r'(user|sys)-(\udcff)?[A-Za-z0-9_]+', r'\1-\2X', value.replace(scratch, 'SCRATCH'))
    return value


def check_axis_c_snapshots(args):
    names = ['axis-c-options-single-00000008', 'axis-c-options-pair-00000060',
             'axis-c-options-boundary-ffffffff']
    offset = time.clock_gettime(time.CLOCK_BOOTTIME) - time.clock_gettime(time.CLOCK_MONOTONIC)
    # The private switch belongs to the allocation half only; clear any caller
    # value while validating the fork-snapshot half and restore it on the way
    # out so an invoking shell is left as found.
    caller_allocate_each = os.environ.pop('AXIS_C_ALLOCATE_EACH', None)
    try:
        with tempfile.TemporaryDirectory(prefix='contract-diff-') as scratch:
            for side in (args.prefix / 'lib/libpinyin.so', args.pinyin_so):
                for name in names:
                    os.environ['AXIS_C_ALLOCATE_EACH'] = '1'
                    original = run_worker('pinyin', side.resolve(), args.prefix / 'lib/libpinyin/data', name, scratch)
                    os.environ.pop('AXIS_C_ALLOCATE_EACH', None)
                    snapshot = run_worker('pinyin', side.resolve(), args.prefix / 'lib/libpinyin/data', name, scratch)
                    # Ignore only incidental directory inventory, as in the battery.
                    assert all(original[key] == snapshot[key] for key in ('exit', 'result', 'stderr')), name
                    print(f'instance snapshot: {side.name} {name} identical', flush=True)
    finally:
        if caller_allocate_each is None:
            os.environ.pop('AXIS_C_ALLOCATE_EACH', None)
        else:
            os.environ['AXIS_C_ALLOCATE_EACH'] = caller_allocate_each
    delta = time.clock_gettime(time.CLOCK_BOOTTIME) - time.clock_gettime(time.CLOCK_MONOTONIC) - offset
    assert abs(delta) < 0.01, 'discard suspend-overlapping snapshot validation'
    print(f'instance snapshot: 6 word/library comparisons, 96 input observations; identical; suspend delta {delta:.6f}', flush=True)
    return 0


def axis_c_covered_abort(pin_row, subject_row, mode):
    """Register-covered class (c) site at the nested-observation level.

    Mirrors the classic ``abort=`` expectation: the pin child dies of SIGABRT
    before returning, and the subject returns normally with the scheme setter
    refused (``set_scheme`` false, no observation) and exactly one warning in
    the facade's GLib domain at level 16. Rows 5c/5d of the register.
    """
    if not isinstance(pin_row, dict) or not isinstance(subject_row, dict):
        return False
    result = subject_row.get('result')
    return (pin_row.get('exit') == -signal.SIGABRT and pin_row.get('result') is None
            and set(pin_row) == set(AXIS_C_ABORT_ROW_FIELDS)
            and subject_row.get('exit') == 0 and isinstance(result, dict)
            and set(result) == set(AXIS_C_REFUSAL_RESULT_FIELDS)
            and result.get('set_scheme') is False and result.get('observation') is None
            and result.get('logs') == [[WARNING_DOMAIN[mode], 16]])


# The exact registered shape of a scheme-setter observation. The abort row
# carries only the worker envelope; the refusal row's result carries exactly
# these four keys, so an extra unregistered field is never licensed.
AXIS_C_ABORT_ROW_FIELDS = ('exit', 'result', 'stderr')
AXIS_C_REFUSAL_RESULT_FIELDS = ('set_scheme', 'observation', 'logs', 'log_messages')


def axis_c_live(row):
    """A worker row that exited cleanly and returned a result.

    A row that died, was signalled, or printed no result observed nothing, so
    two identical failures must never read as an exact match. This mirrors the
    normal runner's rejection (`run_worker` verdicts); a pin abort is an
    explicit class (c) expectation and is matched by `axis_c_covered_abort`,
    never by equality.
    """
    return isinstance(row, dict) and row.get('exit') == 0 and row.get('result') is not None


def axis_c_licensed_remnant(row):
    """A covered row with only its licensed abort/refusal fields removed.

    Anything the row carries beyond those three fields stays and is compared,
    so an unregistered extra field still fails the case.
    """
    return {key: value for key, value in row.items() if key not in AXIS_C_ABORT_ROW_FIELDS}


def axis_c_covered_residual(pin, subject, covered_keys):
    """Both envelopes with only the licensed fields of the covered rows removed.

    Root exit/result/stderr, root `log_messages` and the parent-side call
    results, plus every non-covered observation in full, are retained and
    compared; a covered verdict therefore never hides an unrelated difference.
    """
    pin_residual = copy.deepcopy(pin)
    subject_residual = copy.deepcopy(subject)
    pin_rows = (pin_residual.get('result') or {}).get('observations')
    subject_rows = (subject_residual.get('result') or {}).get('observations')
    for key in covered_keys:
        pin_rows[key] = axis_c_licensed_remnant(pin_rows[key])
        subject_rows[key] = axis_c_licensed_remnant(subject_rows[key])
    return pin_residual, subject_residual


def axis_c_first_difference(left, right, path=''):
    if type(left) is type(right) and isinstance(left, dict):
        for key in sorted(set(left) | set(right)):
            if left.get(key) != right.get(key):
                return axis_c_first_difference(left.get(key), right.get(key), path + '.' + key)
    if type(left) is type(right) and isinstance(left, list):
        for index, (a, b) in enumerate(zip(left, right)):
            if a != b:
                return axis_c_first_difference(a, b, path + f'[{index}]')
    return dict(field=path, pin=left if len(repr(left)) < 300 else repr(left)[:300],
                subject=right if len(repr(right)) < 300 else repr(right)[:300])


def run_axis_c(args, names):
    """Repeat and compare existing worker observations, with no saved output."""
    from collections import Counter
    from concurrent.futures import ThreadPoolExecutor
    offset = time.clock_gettime(time.CLOCK_BOOTTIME) - time.clock_gettime(time.CLOCK_MONOTONIC)
    totals, fingerprints = Counter(), {side: hashlib.sha256() for side in ('pin', 'subject')}
    differences, examples = Counter(), {}
    case_verdicts = {}
    def record_difference(name, key, left, right):
        first = axis_c_first_difference(left, right)
        pin_exit = left.get('exit') if isinstance(left, dict) else None
        subject_exit = right.get('exit') if isinstance(right, dict) else None
        if pin_exit:
            diagnostic = left.get('stderr', '')
            result = (right.get('result') or {}) if isinstance(right, dict) else {}
            signature = json.dumps(dict(pin_exit=pin_exit, subject_exit=subject_exit,
                                        pin_diagnostic=diagnostic,
                                        subject_logs=result.get('log_messages', result.get('logs'))),
                                   sort_keys=True, ensure_ascii=True)
        else:
            signature = json.dumps(first, sort_keys=True, ensure_ascii=True)
        differences[signature] += 1
        examples.setdefault(signature, dict(case=name, row=key, first=first))
    def observe(name, subject, companion):
        spec = CASES[name]
        # Each simultaneous observation owns its scratch/profile inventory.
        # The existing cleanup helper must never see another worker's files.
        with tempfile.TemporaryDirectory(prefix='worker-', dir=scratch) as isolated:
            value = run_worker(spec['mode'], subject, args.prefix / 'lib/libpinyin/data', name, isolated,
                               {'AXIS_C_COMPANION_PINYIN': str(companion)})
            return axis_c_normalize_diagnostics(
                {key: value[key] for key in ('exit', 'result', 'stderr')}, isolated)
    with tempfile.TemporaryDirectory(prefix='contract-diff-') as scratch, ThreadPoolExecutor(max_workers=2) as pool:
        for ordinal, name in enumerate(names, 1):
            mode = CASES[name]['mode']
            if args.axis_c_mutation:
                pin_so = args.pinyin_so if mode == 'pinyin' else args.zhuyin_so
                subject_so = args.axis_c_mutant_pinyin_so if mode == 'pinyin' else args.axis_c_mutant_zhuyin_so
                pin_companion, subject_companion = args.pinyin_so, args.axis_c_mutant_pinyin_so
            else:
                pin_so = args.prefix / ('lib/libpinyin.so' if mode == 'pinyin' else 'lib/libzhuyin.so')
                subject_so = args.pinyin_so if mode == 'pinyin' else args.zhuyin_so
                pin_companion, subject_companion = args.prefix / 'lib/libpinyin.so', args.pinyin_so
            assert pin_so and subject_so and pin_so.is_file() and subject_so.is_file(), name
            futures = [pool.submit(observe, name, so.resolve(), companion.resolve())
                       for so, companion in ((pin_so, pin_companion), (subject_so, subject_companion),
                                             (pin_so, pin_companion), (subject_so, subject_companion))]
            pin, subject, pin_repeat, subject_repeat = [future.result() for future in futures]
            stable = pin == pin_repeat and subject == subject_repeat
            assert stable, 'NOT-ESTABLISHED: unstable case ' + name
            for side, observation in (('pin', pin), ('subject', subject)):
                fingerprints[side].update(axis_c_canonical([name, observation]))
            totals['cases'] += 1
            totals['diagnostics_match' if pin['stderr'] == subject['stderr'] else 'diagnostics_differ'] += 1
            covered_row = AXIS_C_LAYOUT_COVERED.get(name)
            covered_rows, differ_rows = [], 0
            residual_first = None
            pin_rows = (pin['result'] or {}).get('observations')
            subject_rows = (subject['result'] or {}).get('observations')
            if pin_rows is not None and subject_rows is not None:
                for key in pin_rows:
                    left, right = pin_rows[key], subject_rows.get(key)
                    totals['observations'] += 1
                    if axis_c_live(left) and axis_c_live(right) and left == right:
                        totals['payload_match'] += 1
                    elif covered_row and axis_c_covered_abort(left, right, mode):
                        totals['payload_covered'] += 1
                        covered_rows.append(key)
                    else:
                        totals['payload_differ'] += 1
                        differ_rows += 1
                        record_difference(name, key, left, right)
                # First differing row only; all rows were compared in full.
                if differ_rows == 0 and axis_c_live(pin) and axis_c_live(subject) and pin == subject:
                    totals['case_match'] += 1
                    verdict = 'MATCH'
                elif (covered_row and differ_rows == 0 and covered_rows
                      and axis_c_live(pin) and axis_c_live(subject)):
                    # Don't just trust the covered rows: remove only their
                    # licensed fields and require the rest of the envelope to
                    # agree, so a new parent-side warning, root log message or
                    # outer stderr still makes the case DIFFER.
                    residual_pin, residual_subject = axis_c_covered_residual(pin, subject, covered_rows)
                    if residual_pin == residual_subject:
                        totals['covered'] += 1
                        covered_by_row = totals.setdefault('covered_by_row', {})
                        covered_by_row[covered_row] = covered_by_row.get(covered_row, 0) + 1
                        verdict = 'COVERED'
                    else:
                        totals['case_differ'] += 1
                        record_difference(name, 'covered-residual', residual_pin, residual_subject)
                        residual_first = axis_c_first_difference(residual_pin, residual_subject)
                        verdict = 'DIFFER'
                else:
                    totals['case_differ'] += 1
                    if differ_rows == 0:
                        record_difference(name, 'case', pin, subject)
                    verdict = 'DIFFER'
            else:
                totals['observations'] += 1
                if axis_c_live(pin) and axis_c_live(subject) and pin == subject:
                    totals['payload_match'] += 1
                    totals['case_match'] += 1
                    verdict = 'MATCH'
                else:
                    totals['payload_differ'] += 1
                    totals['case_differ'] += 1
                    record_difference(name, 'case', pin, subject)
                    verdict = 'DIFFER'
            case_verdicts[name] = verdict
            if verdict == 'DIFFER':
                print(json.dumps(dict(case=name, verdict='DIFFER',
                                      first=residual_first or axis_c_first_difference(pin, subject)),
                                 ensure_ascii=True), flush=True)
            elif verdict == 'COVERED':
                print(json.dumps(dict(case=name, verdict='COVERED', row=covered_row,
                                      covered_observations=len(covered_rows)), ensure_ascii=True), flush=True)
            if ordinal % 25 == 0:
                print(f'axis C progress: {ordinal}/{len(names)} cases', flush=True)
    elapsed_suspend = (time.clock_gettime(time.CLOCK_BOOTTIME) -
                       time.clock_gettime(time.CLOCK_MONOTONIC) - offset)
    assert abs(elapsed_suspend) < 0.01, 'NOT-ESTABLISHED: discard suspend-overlapping run'
    for signature in sorted(differences):
        print(json.dumps(dict(difference_signature=json.loads(signature), count=differences[signature],
                              example=examples[signature]), ensure_ascii=True), flush=True)
    print(json.dumps(dict(battery=args.axis_c_replacement, mutation=args.axis_c_mutation,
                          counts=dict(totals), repeats='identical', suspend_delta=elapsed_suspend,
                          hashes={side: h.hexdigest() for side, h in fingerprints.items()}), sort_keys=True), flush=True)
    if args.axis_c_mutation:
        controls = AXIS_C_DEFINITION['mutations'][args.axis_c_mutation]['controls']
        for control in controls:
            print(json.dumps(dict(mutation=args.axis_c_mutation, control=control,
                                  verdict=case_verdicts.get(control)), ensure_ascii=True), flush=True)
        undetected = [control for control in controls if case_verdicts.get(control) != 'DIFFER']
        assert not undetected, ('NOT-ESTABLISHED: %s control(s) not detected: %s'
                                % (args.axis_c_mutation, ', '.join(undetected)))
        return 0
    return 1 if totals['case_differ'] else 0

_FATAL_GLIB_PREFIX = re.compile(
    r'\*\* \(process:\d+\): (CRITICAL|WARNING|MESSAGE|ERROR|DEBUG|INFO) \*\*: '
    r'\d\d:\d\d:\d\d\.\d+: ')


def fatal_stderr(text):
    """Drop GLib's process id and timestamp from a fatal run's stderr: they
    differ between the two processes, while the severity and the message are
    the diagnostics under comparison."""
    return _FATAL_GLIB_PREFIX.sub(r'** \1 **: ', text)


def registered_termination(spec):
    """Whether the spec says how the pin dies (`abort=`, `crash=`,
    `both_abort=`, with or without `either=`): then `matches` owns the exit
    conditions of both sides and no validity check is layered on top."""
    return spec['abort'] is not NO_ABORT or spec['crash'] is not None or spec['both_abort']


def valid_observation(spec, side):
    """An ordinary case observed something only when the worker exited
    normally and printed its result document; a signal, a non-zero exit, a
    timeout or an empty stdout observed nothing."""
    return registered_termination(spec) or (side['exit'] == 0 and side['result'] is not None)


POLICY_ROOT = Path(__file__).resolve().parents[2] / 'docs/findings'


def policy_expectations(spec):
    """Read the current registered record, never a cached exemption.

    Each case lists complete exit/result/stderr observations for both sides.
    No wildcards, ignored result fields or partial-dict matching are allowed.
    Rejected metadata is INVALID, including under --expect-parent.
    """
    name = spec['policy_record']
    if not isinstance(name, str) or Path(name).name != name or not name.endswith('.md'):
        raise ValueError('policy_record must name a findings Markdown record')
    path = POLICY_ROOT / name
    if path.resolve().parent != POLICY_ROOT.resolve():
        raise ValueError('policy record escapes findings directory')
    register = (POLICY_ROOT / 'upstream-divergences.md').read_text()
    if f']({name})' not in register:
        raise ValueError('policy record is not registered')
    blocks = re.findall(r'^```contract-diff-policy\n(.*?)\n```$', path.read_text(), re.M | re.S)
    if len(blocks) != 1:
        raise ValueError('policy record needs exactly one observations block')
    record = json.loads(blocks[0])
    if (not isinstance(record, dict) or type(record.get('schema')) is not int or record.get('schema') != 1
            or record.get('record') != path.stem or record.get('status') != 'current'
            or record.get('disposition') != 'POLICY-ACCEPTED'):
        raise ValueError('policy record is not current or has incompatible metadata')
    pair = record['cases'][spec['case_id']]
    if not isinstance(pair, dict) or set(pair) != {'pin', 'subject'}:
        raise ValueError('policy case must specify both observations')
    for side in pair.values():
        if (not isinstance(side, dict) or set(side) != {'exit', 'result', 'stderr'}
                or type(side['exit']) is not int or side['exit'] != 0
                or not isinstance(side['result'], dict) or 'logs' not in side['result']
                or any(key.startswith('~') for key in side['result'])
                or not isinstance(side['stderr'], str)):
            raise ValueError('policy observation must be complete and normally terminated')
    if pair['pin'] == pair['subject']:
        raise ValueError('policy case must record a difference, not parity')
    return pair


def policy_observation(side):
    """The full stable observation, excluding worker timings only."""
    return {key: side[key] for key in ('exit', 'result', 'stderr')}


def verdict_for(spec, pin, subject, expect_parent):
    """The driver's decision for one case: (verdict, expected_ok, same).

    INVALID when either side of an ordinary case produced no valid
    observation; it is never expected, in either mode. A `strjoinv_over_read`
    case's held pin fault (`strjoinv_fault`) counts as valid. Otherwise MATCH or
    DIFFER by `matches`, and expected when it is what the mode asks for: a
    MATCH, or under --expect-parent a DIFFER from a non-control case."""
    if spec['policy_record'] is not None:
        try:
            expected_pair = policy_expectations(spec)
            actual = dict(pin=policy_observation(pin), subject=policy_observation(subject))
        except (OSError, ValueError, KeyError, TypeError):
            return 'INVALID', False, False
        if not all(valid_observation(spec, side) for side in (pin, subject)):
            return 'INVALID', False, False
        if json.dumps(actual, sort_keys=True) == json.dumps(expected_pair, sort_keys=True):
            # An existing accepted quirk is a control on its parent too.
            return 'POLICY-ACCEPTED', True, False
        # A bad record observation is never an expected parent difference.
        return 'DIFFER', False, False
    same = matches(spec, pin, subject)
    held_fault = spec['strjoinv_over_read'] and strjoinv_fault(pin, subject)
    if not held_fault and not all(valid_observation(spec, side) for side in (pin, subject)):
        return 'INVALID', False, same
    expected = same if (not expect_parent or spec['control']) else not same
    return ('MATCH' if same else 'DIFFER'), expected, same


def abort_answered(spec, pin, subject):
    """The subject's half of a class (c) site: it survives with the `ret`
    the spec names, the one warning and every untouched out-param, and
    leaves the profile the pin left (`userdir`)."""
    shown = subject['result'] or {}
    expected_logs = spec['abort_logs'] if spec['abort_logs'] is not None \
        else [[WARNING_DOMAIN[spec['mode']], 16]]
    return subject['exit'] == 0 and \
        shown.get('ret') == spec['abort'] and shown.get('logs') == expected_logs and \
        all(v for key, v in shown.items() if key.startswith('untouched')) and \
        (not spec['userdir'] or pin['userfiles'] == subject['userfiles'])


def short_dir_chunk(spec):
    """A `dir_chunk` case on a host where the pin does not reach its assert."""
    return spec['dir_chunk'] is not None and not directory_reads_as_a_chunk()


# Register row 1 (class (b)): `pinyin_bigram_iterator_has_next_phrase` joins
# each predecessor's and each successor's syllables from a `GPtrArray` made by
# `g_ptr_array_new` and taken by `g_ptr_array_free(…, FALSE)`, never
# terminated, with `g_strjoinv("'", …)` and frees it with `g_strfreev`
# (`pinyin.cpp:844-850`, `:857-863` at 074a2219). At GLib 2.90.0 (the oracle
# image) and 2.80.0, `g_ptr_array_maybe_expand` grows a non-terminated array,
# one add at a time, to the power of two of bytes that holds its `len`
# pointers, at least `MIN_ARRAY_SIZE` (16), with `g_realloc`, and clears the
# new slots only under `g_mem_gc_friendly` (`garray.c:1893-1921` at 2.90.0);
# `ptr_array_free` hands that buffer back as is (`:1845-1890`). So
# `pdata[len]` is uninitialised heap (inside the 16-byte block for one
# syllable, the word past it for two), and `g_strjoinv` walks to the first
# NULL (`gstrfuncs.c:2661`, `:2672`). An unreadable word faults; a NULL word
# gives the subject's reading. A readable non-NULL word would be joined and
# then freed by `g_strfreev` (`pinyin.cpp:850`, `:863`), so a completed run
# with such a tail is not probable, and a completed pin is compared exactly.
def strjoinv_fault(pin, subject):
    """Row 1's fault: the pin dies of SIGSEGV after the stderr the subject
    wrote, byte for byte, and the subject completes. The subject's rows are
    not compared with anything here."""
    return pin['exit'] == -signal.SIGSEGV and pin['stderr'] == subject['stderr'] and \
        subject['exit'] == 0 and subject['result'] is not None


def matches(spec, pin, subject):
    """The verdict predicate shared by the driver and gcov measurement tier."""
    if spec['policy_record'] is not None:
        return False  # accepted differences must never enter MATCH totals
    short_dir = short_dir_chunk(spec)
    as_abort = spec['abort'] is not NO_ABORT and not short_dir and (not spec['either'] or pin['exit'] == -6)
    if spec['both_abort']:
        # GLib's _g_log_abort can use g_abort or G_BREAKPOINT (gmessages.c
        # at GLib 2.80.0 / 2.89.4). Require the same fatal-log signal;
        # the comparison is the signal, the raw stderr and the partial
        # profile left behind, not a surviving subject result.
        same = (
            pin['exit'] in (-signal.SIGABRT, -signal.SIGTRAP)
            and subject['exit'] == pin['exit']
            and (
                not spec['stderr']
                or fatal_stderr(pin['stderr']) == fatal_stderr(subject['stderr'])
            )
            and (not spec['userdir'] or pin['userfiles'] == subject['userfiles'])
        )
    elif spec['crash'] is not None and not as_abort:
        shown = subject['result'] or {}
        same = pin['exit'] == -signal.SIGSEGV and subject['exit'] == 0 and \
            (spec['stderr_varies'] or pin['stderr'] == subject['stderr']) and \
            all(shown.get(key) == value for key, value in spec['crash'].items())
    elif as_abort:
        same = pin['exit'] == -6 and abort_answered(spec, pin, subject)
    elif spec['strjoinv_over_read'] and pin['exit'] == -signal.SIGSEGV:
        same = strjoinv_fault(pin, subject)
    elif short_dir and spec['dir_chunk'] == 'system':
        # Class (b): the pin dies of SIGSEGV after the stderr the subject
        # writes in full; the subject answers as the abort site registers.
        same = pin['exit'] == -signal.SIGSEGV and pin['stderr'] == subject['stderr'] and \
            abort_answered(spec, pin, subject)
    else:
        # A side that died or printed nothing observed nothing: two
        # identical failures must not read as a match (nor satisfy
        # --expect-parent as a difference).
        same = all(r['exit'] == 0 and r['result'] is not None for r in (pin, subject)) and \
            observed(pin['result']) == observed(subject['result'])
        if spec['stderr'] or short_dir:
            same = same and pin['stderr'] == subject['stderr']
        if short_dir:
            # `dir_chunk='user'`: both sides carry on past an ordinary false.
            same = same and pin['userfiles'] == subject['userfiles']
    return same


# Per-call measurement stays in this driver so it uses the same fixtures and
# verdict predicate. Only the native function-pointer call is collected.
PER_CALL_OPS = ('add', 'count', 'lookup')
PER_CALL_NAMES = dict(zip(PER_CALL_OPS, ('iterator_add_phrase', 'token_get_n_pronunciation', 'lookup_tokens')))
PER_CALL_NATIVE = r'''
#define _POSIX_C_SOURCE 200809L
#include <stdbool.h>
#include <stdint.h>
#include <time.h>
#include <valgrind/callgrind.h>
#if !defined(__x86_64__)
#error "per-call calibration currently requires x86-64"
#endif
static uint64_t calls[3], nanos[3];
static int profile, started;
void pc_stop(void) { if (started) { CALLGRIND_STOP_INSTRUMENTATION; started = 0; } }
void pc_start(void) { if (profile && !started) { CALLGRIND_START_INSTRUMENTATION; started = 1; } }
void pc_reset(int p) { pc_stop(); profile = p; for (int i=0; i<3; i++) calls[i] = nanos[i] = 0; }
uint64_t pc_calls(int i) { return calls[i]; }
uint64_t pc_nanos(int i) { return nanos[i]; }
static uint64_t now(void) {
    struct timespec t;
    if (clock_gettime(CLOCK_MONOTONIC_RAW, &t)) __builtin_trap();
    return (uint64_t)t.tv_sec * 1000000000 + t.tv_nsec;
}
typedef bool (*add_fn)(void *, const char *, const char *, int);
typedef bool (*count_fn)(void *, unsigned, unsigned *);
typedef bool (*lookup_fn)(void *, const char *, void *);
#define MEASURE(INDEX, EXPR) \
    bool ret; \
    if (profile) { \
        CALLGRIND_TOGGLE_COLLECT; \
        ret = (EXPR); \
        CALLGRIND_TOGGLE_COLLECT; \
    } else { \
        uint64_t begin = now(); \
        ret = (EXPR); \
        uint64_t end = now(); \
        nanos[INDEX] += end - begin; \
    } \
    calls[INDEX]++; \
    return ret
bool pc_add(add_fn f, void *p, const char *s, const char *r, int n) { MEASURE(0, f(p,s,r,n)); }
bool pc_count(count_fn f, void *p, unsigned t, unsigned *n) { MEASURE(1, f(p,t,n)); }
bool pc_lookup(lookup_fn f, void *p, const char *s, void *a) { MEASURE(2, f(p,s,a)); }
/* Each callee has exactly three instructions. The driver checks disassembly.
 * The caller uses the same ABI and client requests as the real call. */
__asm__(".text\n.globl pc_noop\n.type pc_noop,@function\npc_noop:\nmov $1,%eax\nnop\nret\n.size pc_noop,.-pc_noop");
extern bool pc_noop_add(void *, const char *, const char *, int) __asm__("pc_noop");
extern bool pc_noop_count(void *, unsigned, unsigned *) __asm__("pc_noop");
extern bool pc_noop_lookup(void *, const char *, void *) __asm__("pc_noop");
void pc_calibrate(int op, unsigned n) {
    pc_start();
    for (unsigned i=0; i<n; i++) {
        if (op == 0) pc_add(pc_noop_add, 0, 0, 0, 0);
        if (op == 1) pc_count(pc_noop_count, 0, 0, 0);
        if (op == 2) pc_lookup(pc_noop_lookup, 0, 0, 0);
    }
    pc_stop();
}
bool pc_lookup_loop(lookup_fn f, void *p, const char *s, void *a, unsigned n) {
    bool ret = true;
    pc_start();
    for (unsigned i=0; i<n; i++) ret &= pc_lookup(f,p,s,a);
    pc_stop();
    return ret;
}
'''


def per_call_helper(scratch):
    source, library = Path(scratch) / 'calls.c', Path(scratch) / 'calls.so'
    source.write_text(PER_CALL_NATIVE)
    compiled = subprocess.run(['cc', '-O2', '-shared', '-fPIC', '-fcf-protection=none',
                               '-Wall', '-Wextra', '-Werror', '-o', str(library), str(source)],
                              capture_output=True, text=True)
    if compiled.returncode:
        raise ValueError('per-call helper compile failed: ' + compiled.stderr)
    assembly = subprocess.check_output(['objdump', '-d', '--disassemble=pc_noop', str(library)], text=True)
    instructions = re.findall(r'^\s*[0-9a-f]+:\s+(?:[0-9a-f]{2}\s+)+\s*(\S+)', assembly, re.M)
    # GCC can put alignment bytes or an unreachable ud2 after the return.
    if instructions[:3] != ['mov', 'nop', 'ret']:
        raise ValueError('calibration callee must be mov, nop, ret: ' + repr(instructions))
    return library


def per_call_native(library, profile):
    native = C.CDLL(str(library))
    for name in ('pc_start', 'pc_stop'):
        getattr(native, name).argtypes, getattr(native, name).restype = [], None
    native.pc_reset.argtypes, native.pc_reset.restype = [I], None
    native.pc_calibrate.argtypes, native.pc_calibrate.restype = [I, U], None
    for name in ('pc_calls', 'pc_nanos'):
        getattr(native, name).argtypes, getattr(native, name).restype = [I], C.c_uint64
    for op, args in (('add', [P, S, S, I]), ('count', [P, U, C.POINTER(U)]), ('lookup', [P, S, P])):
        f = getattr(native, 'pc_' + op)
        f.argtypes, f.restype = [P] + args, B
    native.pc_lookup_loop.argtypes, native.pc_lookup_loop.restype = [P, P, S, P, U], B
    native.pc_reset(profile)
    return native


class PerCallKit(Kit):
    def __init__(self, mode, so, data, scratch, native, operations):
        super().__init__(mode, so, data, scratch)
        self.native, self.operations, self.active = native, operations, False

    @contextlib.contextmanager
    def per_call_step(self):
        self.active = True
        self.native.pc_start()
        try:
            yield
        finally:
            self.active = False

    def fn(self, name, res, *args):
        original = super().fn(name, res, *args)
        operation = next((op for op in self.operations if PER_CALL_NAMES[op] == name), None)
        if operation is None:
            if not self.active:
                self.native.pc_stop()
            return original
        wrapped = getattr(self.native, 'pc_' + operation)
        def call(*values):
            if self.active:
                return wrapped(C.cast(original, P), *values)
            self.native.pc_stop()
            return original(*values)
        return call


def per_call_worker(mode, so, data, name, library, metric, operations):
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    native = per_call_native(library, metric == 'ir')
    if name == 'calibrate':
        native.pc_calibrate(PER_CALL_OPS.index(operations), 1000)
        result = {'calibration': True}
    else:
        kit = PerCallKit(mode, so, data, os.environ['TMPDIR'], native, operations.split(','))
        if name == 'lookup-only':
            # No add occurs. All iterations run in C and reuse one GArray.
            new = kit.glib.g_array_new
            new.argtypes, new.restype = [I, I, U], P
            array = new(0, 0, 4)
            lookup = Kit.fn(kit, 'lookup_tokens', B, P, S, P)
            ret = native.pc_lookup_loop(C.cast(lookup, P), kit.inst, '你好'.encode(), array, 10000)
            view = C.cast(array, C.POINTER(Arr)).contents
            result = dict(ret=ret, tokens=C.cast(view.data, C.POINTER(U))[:view.len])
            kit.glib.g_array_free.argtypes, kit.glib.g_array_free.restype = [P, I], P
            kit.glib.g_array_free(array, True)
        else:
            result = CASES[name]['fn'](kit)
        result['logs'] = kit.logs
    native.pc_stop()
    stats = {op: dict(calls=native.pc_calls(i), ns=native.pc_nanos(i)) for i, op in enumerate(PER_CALL_OPS)}
    print(json.dumps(dict(observation=result, stats=stats), ensure_ascii=True), flush=True)


def per_call_ir(stderr):
    events = re.findall(r'^events:\s*(.*?)\s*$', stderr, re.M)
    totals = re.findall(r'^totals:\s*(.*?)\s*$', stderr, re.M)
    if events != ['Ir'] or len(totals) != 1 or not re.fullmatch(r'\d+', totals[0]):
        raise ValueError('expected one Callgrind Ir events/totals pair')
    value = int(totals[0])
    if value <= 0:
        raise ValueError('Callgrind collected no instructions')
    return value


def per_call_profile(stderr):
    """Read exclusive function costs and inclusive call edges, in memory.
    Callgrind name IDs share object/file/function namespaces with their
    callee forms. Position columns can use relative compression; this reader
    needs their width, not their values. Costs stay raw and additive only
    within the exclusive function table.
    """
    total = per_call_ir(stderr)
    aliases = {'ob': 'ob', 'cob': 'ob', 'fn': 'fn', 'cfn': 'fn',
               'fl': 'fl', 'fi': 'fl', 'fe': 'fl', 'cfi': 'fl', 'cfl': 'fl'}
    names = {key: {} for key in ('ob', 'fn', 'fl')}
    lines = stderr.splitlines()
    for line in lines:
        match = re.match(r'^(\w+)=(.*)$', line)
        if match and match[1] in aliases:
            compressed = re.fullmatch(r'\((\d+)\)(?: (.*))?', match[2])
            if compressed and compressed[2] is not None:
                table = names[aliases[match[1]]]
                if compressed[1] in table and table[compressed[1]] != compressed[2]:
                    raise ValueError('conflicting Callgrind name ID')
                table[compressed[1]] = compressed[2]

    def resolve(kind, value):
        compressed = re.fullmatch(r'\((\d+)\)(?: (.*))?', value)
        if not compressed:
            return value
        try:
            return names[aliases[kind]][compressed[1]]
        except KeyError:
            raise ValueError('unresolved Callgrind name ID: ' + kind + '=' + value) from None

    positions = 1
    obj, function = '???', '???'
    next_callee_obj, callee = None, None
    pending = None
    exclusive, edges = {}, {}
    for line in lines:
        if line.startswith('positions:'):
            positions = len(line.split(':', 1)[1].split())
            if not 1 <= positions <= 2:
                raise ValueError('unsupported Callgrind position columns')
        match = re.match(r'^(\w+)=(.*)$', line)
        if match and match[1] in aliases:
            kind, value = match[1], resolve(match[1], match[2])
            if kind == 'ob':
                obj = value
            elif kind == 'fn':
                function, next_callee_obj, callee = value, None, None
            elif kind == 'cob':
                next_callee_obj = value
            elif kind == 'cfn':
                callee = (next_callee_obj if next_callee_obj is not None else obj, value)
                next_callee_obj = None
            continue
        if line.startswith('calls='):
            fields = line.removeprefix('calls=').split()
            if pending is not None or callee is None or not fields or not fields[0].isdigit():
                raise ValueError('invalid Callgrind call association')
            pending = ((obj, function), callee, int(fields[0]))
            continue
        if line.startswith(('jump=', 'jcnd=')):
            raise ValueError('jump associations are not supported by per-call attribution')
        if not re.match(r'^(?:0x[0-9a-fA-F]+|\d+|[+\-]\d+|\*)(?:\s|$)', line):
            continue
        fields = line.split()
        costs = fields[positions:]
        if len(fields) < positions or len(costs) > 1 or (costs and not costs[0].isdigit()):
            raise ValueError('invalid Callgrind Ir cost line')
        cost = int(costs[0]) if costs else 0
        if pending is None:
            key = (obj, function)
            exclusive[key] = exclusive.get(key, 0) + cost
        else:
            caller, target, count = pending
            old_cost, old_count = edges.get((caller, target), (0, 0))
            edges[caller, target] = (old_cost + cost, old_count + count)
            pending = None
    if pending is not None or sum(exclusive.values()) != total:
        raise ValueError('Callgrind exclusive costs do not reconcile with totals')
    return dict(total=total, exclusive=exclusive, edges=edges)


def per_call_attribution(profile, calls, limit):
    """Named roots are evidence, not an invented partition of an inlined API.
    Inclusive edges can overlap. Never sum them as exclusive phase costs.
    """
    def identity(key):
        return dict(object=key[0], function=key[1])
    functions = [dict(**identity(key), ir_per_call=value / calls)
                 for key, value in sorted(profile['exclusive'].items(), key=lambda item: (-item[1], item[0]))
                 if value]
    edges = [dict(caller=identity(caller), callee=identity(callee), inclusive_ir_per_call=value / calls,
                  callgrind_calls_per_selected_call=count / calls)
             for (caller, callee), (value, count) in sorted(profile['edges'].items(),
                                                         key=lambda item: (-item[1][0], item[0])) if value]
    # These roots follow the current add path in both iterators.rs files.
    # A source boundary may disappear under LTO, so absence is unresolved.
    patterns = dict(lookup=r'phrase_table_tokens|SystemDictionary.*tokens_for_text',
                    validation=r'RuntimeDict.*token_introspection|GenericUserStore.*::phrase$',
                    store_merge=r'GenericUserStore.*::add_(?:system_)?phrase_in$')
    candidates = {}
    for phase, pattern in patterns.items():
        selected = [edge for edge in edges if re.search(pattern, edge['callee']['function'])]
        candidates[phase] = dict(status='named_roots_only' if selected else 'unresolved',
                                 inclusive_edges=selected)
    return dict(basis='raw_collected_Ir', raw_ir_per_call=profile['total'] / calls,
                exclusive_sum_ir_per_call=sum(profile['exclusive'].values()) / calls,
                inclusive_edges_overlap=True, phase_partition='not_established',
                edge_call_counts='Callgrind records; can include calls outside collection',
                named_phase_candidates=candidates, functions=functions[:limit] if limit else functions,
                call_edges=edges[:limit] if limit else edges,
                omitted_functions=max(0, len(functions) - limit) if limit else 0,
                omitted_call_edges=max(0, len(edges) - limit) if limit else 0)


def per_call_run(mode, so, data, name, helper, metric, operations, scratch, cpu, attribution=False):
    with tempfile.TemporaryDirectory(prefix='sample-', dir=scratch) as sample:
        command = ['taskset', '-c', str(cpu)]
        if metric == 'ir':
            command += ['valgrind', '--tool=callgrind', '--instr-atstart=no', '--collect-atstart=no',
                        '--cache-sim=no', '--branch-sim=no', '--callgrind-out-file=/dev/stderr']
            if attribution:
                command += ['--read-inline-info=yes', '--demangle=yes']
        command += [sys.executable, __file__, '--per-call-worker', mode, str(so), str(data), name,
                    str(helper), metric, operations]
        env = dict(os.environ, TMPDIR=sample)
        if name != 'calibrate':
            check_tkrzw(env.get('CONTRACT_DIFF_CELL', CELL), 'contract-diff.py/per_call',
                        env.get('OXPINYIN_RUNTIME_SIDE', 'subject'), so, env=env)
        # Captures and profiler output never reach a file.
        check_cancelled()
        with subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env,
                              start_new_session=True) as proc:
            deadline = time.monotonic() + 900
            stdout, stderr = b'', b''
            try:
                while True:
                    check_cancelled()
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        raise subprocess.TimeoutExpired(command, 900, output=stdout, stderr=stderr)
                    try:
                        stdout, stderr = proc.communicate(timeout=min(0.1, remaining))
                    except subprocess.TimeoutExpired as error:
                        stdout, stderr = error.output or b'', error.stderr or b''
                        continue
                    check_cancelled()
                    break
            except BaseException:
                terminate_group(proc)
                raise
        check_cancelled()
        stderr = stderr.decode('utf-8', 'surrogateescape')
        documents = [json.loads(line) for line in stdout.decode().splitlines() if line.startswith('{')]
        if proc.returncode or len(documents) != 1:
            raise ValueError('per-call worker failed: exit %s, %s' % (proc.returncode, stderr[-3000:]))
        document = documents[0]
        observation = dict(exit=0, result=document['observation'], stderr=stderr,
                           stderr_lines=len(stderr.splitlines()), userfiles=user_dir_listing(sample, set()))
        return dict(observation=observation, stats=document['stats'],
                    ir=per_call_ir(stderr) if metric == 'ir' else None,
                    profile=per_call_profile(stderr) if metric == 'ir' and attribution else None)


def per_call_checked(spec, reference, sample, operations):
    verdict, expected, same = verdict_for(spec, reference['observation'], sample['observation'], False)
    if verdict != 'MATCH' or not expected or not same:
        raise ValueError('per-call observation failed: ' + verdict)
    for op in operations:
        count = sample['stats'][op]['calls']
        if count <= 0 or count != reference['stats'][op]['calls']:
            raise ValueError('per-call attempt count differs for ' + op)


def per_call_calibration(total, calls=1000):
    if calls != 1000 or total % calls or total <= 3 * calls:
        raise ValueError('invalid three-instruction callee calibration')
    return total // calls - 3


def run_per_call(args, parser):
    if args.cases or args.issue or args.family or args.expect_parent or args.observations or args.plan_only or \
            args.regenerate_sentence_ties or args.axis_c_replacement or args.axis_c_mutation or args.axis_c_snapshot_check:
        parser.error('--per-call cannot be combined with contract selectors, captures or other modes')
    operations = tuple(args.per_call_operations.split(',')) if args.per_call_operations else (
        PER_CALL_OPS if args.per_call == 'wrap' else ('lookup',))
    if not operations or len(set(operations)) != len(operations) or any(op not in PER_CALL_OPS for op in operations):
        parser.error('--per-call-operations needs unique add,count,lookup names')
    if args.per_call == 'lookup-only' and operations != ('lookup',):
        parser.error('lookup-only measures lookup only')
    if args.per_call_attribution_limit < 0:
        parser.error('--per-call-attribution-limit must be nonnegative')
    modes = ('pinyin', 'zhuyin') if args.per_call_facades == 'both' else (args.per_call_facades,)
    sides = {'pin': {'pinyin': args.prefix / 'lib/libpinyin.so', 'zhuyin': args.prefix / 'lib/libzhuyin.so'},
             'subject': {'pinyin': args.pinyin_so, 'zhuyin': args.zhuyin_so}}
    if args.base_pinyin_so or args.base_zhuyin_so:
        sides = dict(pin=sides['pin'], base={'pinyin': args.base_pinyin_so, 'zhuyin': args.base_zhuyin_so},
                     subject=sides['subject'])
    for side, artifacts in sides.items():
        for mode in modes:
            if not artifacts[mode] or not artifacts[mode].is_file():
                parser.error('missing %s %s library' % (side, mode))
            artifacts[mode] = artifacts[mode].resolve()
    FUNCTION_ORDER['check_arms'](args, {side: {mode: paths[mode] for mode in modes}
                                      for side, paths in sides.items()}, references=('pin',))
    cpu = args.per_call_cpu if args.per_call_cpu is not None else min(os.sched_getaffinity(0))
    if cpu not in os.sched_getaffinity(0):
        parser.error('--per-call-cpu is outside this process CPU affinity')
    data = args.prefix / 'lib/libpinyin/data'
    provenance = {side: {mode: dict(path=str(paths[mode]), sha256=hashlib.sha256(paths[mode].read_bytes()).hexdigest())
                         for mode in modes} for side, paths in sides.items()}
    print(json.dumps(dict(per_call='start', workload=args.per_call, cpu=cpu, libraries=provenance,
                         wall_unit='ns/call', ir_unit='Ir/call', wall_rounds=3)), flush=True)
    with tempfile.TemporaryDirectory(prefix='contract-per-call-') as scratch:
        helper = per_call_helper(scratch)
        calibration = {}
        for op in operations:
            sample = per_call_run('pinyin', sides['pin']['pinyin'], data, 'calibrate', helper, 'ir', op, scratch, cpu)
            calibration[op] = per_call_calibration(sample['ir'], sample['stats'][op]['calls'])
        print(json.dumps(dict(per_call='calibration', wrapper_ir=calibration, callee_ir=3, calls=1000)), flush=True)
        for mode in modes:
            name = ('wrap-256th-reading' + ('-zhuyin' if mode == 'zhuyin' else '')
                    if args.per_call == 'wrap' else 'lookup-only')
            spec = CASES['wrap-256th-reading' + ('-zhuyin' if mode == 'zhuyin' else '')]
            reference, walls, counts = None, {side: {op: [] for op in operations} for side in sides}, {}
            for round_index in range(3):
                order = list(sides)
                order = order[round_index % len(order):] + order[:round_index % len(order)]
                for side in order:
                    print('per-call: %s native round %d %s' % (mode, round_index + 1, side), file=sys.stderr, flush=True)
                    sample = per_call_run(mode, sides[side][mode], data, name, helper, 'wall',
                                          ','.join(operations), scratch, cpu)
                    if reference is None:
                        reference = sample
                    per_call_checked(spec, reference, sample, operations)
                    for op in operations:
                        counts[op] = sample['stats'][op]['calls']
                        walls[side][op].append(sample['stats'][op]['ns'] / counts[op])
            irs = {side: {} for side in sides}
            profiles = {side: {} for side in sides}
            for side in sides:
                for op in operations:
                    print('per-call: %s Ir %s %s' % (mode, side, op), file=sys.stderr, flush=True)
                    sample = per_call_run(mode, sides[side][mode], data, name, helper, 'ir', op, scratch, cpu,
                                          attribution=args.per_call_attribution)
                    per_call_checked(spec, reference, sample, (op,))
                    value = sample['ir'] / counts[op] - calibration[op]
                    if value <= 0:
                        raise ValueError('nonpositive calibrated Ir for ' + op)
                    irs[side][op] = value
                    if args.per_call_attribution:
                        profiles[side][op] = per_call_attribution(
                            sample['profile'], counts[op], args.per_call_attribution_limit)
            for op in operations:
                pin_ns = statistics.median(walls['pin'][op])
                pin_ir = irs['pin'][op]
                for side in sides:
                    ns = statistics.median(walls[side][op])
                    ir = irs[side][op]
                    if args.per_call_attribution:
                        print(json.dumps(dict(per_call='attribution', facade=mode, operation=op, side=side,
                                              wrapper_ir_per_call=calibration[op],
                                              **profiles[side][op])), flush=True)
                    print(json.dumps(dict(per_call='result', workload=args.per_call, facade=mode, operation=op,
                                          side=side, calls=counts[op], wall_ns_per_call=ns, ir_per_call=ir,
                                          wall_ratio=ns / pin_ns, ir_ratio=ir / pin_ir,
                                          native_rounds_ns_per_call=walls[side][op],
                                          observation_sha256=hashlib.sha256(json.dumps(observed(
                                              reference['observation']['result']), sort_keys=True).encode()).hexdigest())),
                          flush=True)
    return 0


FUNCTION_ORDER = runpy.run_path(str(Path(__file__).resolve().parents[1] / 'packaging/function_order.py'))


# The pin crashes (SIGBUS/SIGTRAP) or orders candidates differently on
# arm64 for these cases; the amd64 pin is the reference and still runs them.
# Diagnosis: #1137. The contract gate excludes them on arm64 only.
ARM64_DIAGNOSIS_ISSUE = 1137
ARM64_EXCLUDED_CASES = (
    'fini-twice',
    'zhuyin-fini-twice',
    'replay-modify-header-overlong-pinyin',
    'replay-modify-header-overlong-zhuyin',
    'replay-touched-duplicate-invalid-pinyin',
    'replay-touched-duplicate-invalid-zhuyin',
    'replay-touched-many-invalid-pinyin',
    'replay-touched-many-invalid-zhuyin',
    'row58-unsigned-score-off',
    'row58-unsigned-score-on',
    'table-conf-lambda-million-zhuyin',
    'table-conf-lambda-one-and-a-half-zhuyin',
    'table-conf-lambda-two-zhuyin',
)
ARM64_EXCLUSION_REASON = (
    '#1137: the pin crashes or orders candidates differently on arm64; '
    'excluded there only, amd64 still runs the case'
)
for _excluded in ARM64_EXCLUDED_CASES:
    if _excluded not in CASES:
        raise RuntimeError('arm64 exclusion names an unregistered case: ' + _excluded)
    CASES[_excluded] = dict(CASES[_excluded], platforms=('linux/amd64',),
                            platform_skip_reason=ARM64_EXCLUSION_REASON)


def platform_skips(spec):
    """True when the case is registered away from this host's platform."""
    platforms = spec.get('platforms')
    return bool(platforms) and HOST_ORACLE_PLATFORM is not None and HOST_ORACLE_PLATFORM not in platforms


def run_main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    FUNCTION_ORDER['add_arguments'](parser)
    parser.add_argument('--teardown', action='store_true', help='native teardown measurement workload')
    parser.add_argument('--teardown-stage', choices=('provision', 'verify', 'pilot', 'confirm', 'repeat', 'multi', 'attribute', 'init-attribute'), default='verify')
    parser.add_argument('--teardown-cache', choices=('warm', 'cold'), default='warm',
                        help='cold: verified per-file eviction before each native time/memory sample (Linux ext4)')
    parser.add_argument('--teardown-work-dir', type=Path, help='owned temporary ext4 directory; caller cleans it')
    parser.add_argument('--teardown-cases', default='empty,modest,phrases,journals,modest-unsaved,modest-saved')
    parser.add_argument('--teardown-facades', choices=('both', 'pinyin', 'zhuyin'), default='both')
    parser.add_argument('--teardown-cpu', type=int)
    parser.add_argument('--teardown-attribution-case', choices=('empty', 'modest', 'phrases', 'journals', 'modest-unsaved', 'modest-saved'), default='journals')
    parser.add_argument('cell', nargs='?', choices=['bdb', 'kc', 'tkrzw'])
    parser.add_argument('prefix', nargs='?', type=Path)
    parser.add_argument('pinyin_so', nargs='?', type=Path)
    parser.add_argument('--zhuyin-so', type=Path)
    parser.add_argument('--per-call', choices=['wrap', 'lookup-only'])
    parser.add_argument('--per-call-facades', choices=['pinyin', 'zhuyin', 'both'], default='both')
    parser.add_argument('--per-call-cpu', type=int)
    parser.add_argument('--per-call-operations', help='comma-separated add,count,lookup (default: all)')
    parser.add_argument('--per-call-attribution', action='store_true')
    parser.add_argument('--per-call-attribution-limit', type=int, default=25, help='top functions and edges; 0 emits all')
    parser.add_argument('--base-pinyin-so', type=Path)
    parser.add_argument('--base-zhuyin-so', type=Path)
    parser.add_argument('--cases')
    parser.add_argument('--issue', action='append', default=[], metavar='TAG',
                        help='select every case registered with this issue, row or lane tag')
    parser.add_argument('--family', action='append', default=[], metavar='F',
                        help='select every case whose ID starts with F/')
    parser.add_argument('--list-tags', action='store_true', help='print the tag index as JSON and exit')
    parser.add_argument('--plan-only', action='store_true', help='print the selection plan and exit')
    parser.add_argument('--case-timeout', type=float, default=300,
                        help='wall-clock seconds per ordinary case, pin and subject combined (default: 300)')
    parser.add_argument('--jobs', type=positive_jobs,
                        help='maximum concurrent ordinary cases, reduced by memory admission '
                             '(default: OXPINYIN_CONTRACT_JOBS, else serial)')
    parser.add_argument('--expect-parent', action='store_true')
    parser.add_argument('--regenerate-sentence-ties', type=Path,
                        help='regenerate pin expectations in the supplementary #747 source fixture')
    parser.add_argument('--observations', type=Path, default=os.environ.get('CONTRACT_DIFF_OBSERVATIONS'),
                        help='retain both complete observations as JSONL (also CONTRACT_DIFF_OBSERVATIONS)')
    parser.add_argument('--run-platform-excluded', action='store_true',
                        help='run cases whose registered platform excludes this host; each '
                             'override reason is printed to stderr (not an ordinary gate)')
    axis_c_mode = parser.add_mutually_exclusive_group()
    axis_c_mode.add_argument('--axis-c-replacement', choices=['options', 'encoding', 'layouts', 'all'])
    axis_c_mode.add_argument('--axis-c-mutation', choices=['C-m1-r', 'C-m3-r'])
    axis_c_mode.add_argument('--axis-c-snapshot-check', action='store_true')
    parser.add_argument('--axis-c-mutant-pinyin-so', type=Path)
    parser.add_argument('--axis-c-mutant-zhuyin-so', type=Path)
    args = parser.parse_args()
    if args.cases is not None:
        try:
            case_names(args.cases)
        except SelectionError as error:
            parser.error(str(error))
    if not 0 < args.case_timeout < float('inf'):
        parser.error('--case-timeout must be a positive finite number of seconds')
    if args.jobs not in (None, 1) and (args.teardown or args.per_call or args.axis_c_replacement or
                           args.axis_c_mutation or args.axis_c_snapshot_check or args.regenerate_sentence_ties):
        parser.error('--jobs applies to ordinary contract cases only')
    if args.list_tags:
        print(json.dumps(dict(tags=tag_index(), modes={name: spec['mode'] for name, spec in CASES.items()
                                                       if not name.startswith('axis-c-')}), indent=1))
        return 0
    if not (args.cell and args.prefix and args.pinyin_so):
        parser.error('CELL, PREFIX and PINYIN_SO are required')
    if (args.axis_c_mutant_pinyin_so or args.axis_c_mutant_zhuyin_so) and not args.axis_c_mutation:
        parser.error('--axis-c-mutant-pinyin-so/--axis-c-mutant-zhuyin-so require --axis-c-mutation')
    if (args.issue or args.family or args.plan_only) and (
            args.axis_c_replacement or args.axis_c_mutation or args.axis_c_snapshot_check or
            args.regenerate_sentence_ties):
        parser.error('--issue/--family/--plan-only select ordinary cases only')
    os.environ['CONTRACT_DIFF_CELL'] = args.cell
    lib = args.prefix / 'lib'
    data = lib / 'libpinyin/data'
    inputs = [args.prefix / 'oracle-pin.txt', lib / 'libpinyin.so', args.pinyin_so, data / 'bigram.db']
    for path in inputs:
        assert path.is_file(), 'missing input: %s' % path
    manifest = (args.prefix / 'oracle-pin.txt').read_text()
    assert 'libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99' in manifest
    assert '+dbm-' + args.cell + '\n' in manifest
    if not args.plan_only:
        for side, library in (("pin", lib / 'libpinyin.so'), ("subject", args.pinyin_so),
                              ("pin", lib / 'libzhuyin.so' if args.zhuyin_so else None),
                              ("subject", args.zhuyin_so)):
            if library is not None:
                check_tkrzw(args.cell, 'contract-diff.py', side, library)
    if args.teardown:
        import teardown
        return teardown.run(args, sys.modules[__name__], parser)
    if args.per_call:
        return run_per_call(args, parser)
    if args.function_order or args.unordered_arm:
        parser.error('function-order expectations require --teardown or --per-call')
    if args.base_pinyin_so or args.base_zhuyin_so or args.per_call_attribution or args.per_call_operations:
        parser.error('base paths, attribution and operation selection require --per-call')
    if args.axis_c_replacement or args.axis_c_mutation or args.axis_c_snapshot_check:
        assert not (args.observations or args.cases or args.expect_parent or args.regenerate_sentence_ties), \
            'axis C replacement cannot be combined with capture, regeneration or parent modes'
        # Hash the committed source fixtures once, here, before any Axis C
        # worker is launched; the loader no longer does it at import.
        axis_c_definition(verify_sources=True)
        if args.axis_c_snapshot_check:
            return check_axis_c_snapshots(args)
        # The fork-snapshot protocol is preregistered; a caller-provided
        # AXIS_C_ALLOCATE_EACH must never silently switch the battery over to
        # per-child allocation (that is the snapshot check's private half).
        os.environ.pop('AXIS_C_ALLOCATE_EACH', None)
        names = (AXIS_C_DEFINITION['mutations'][args.axis_c_mutation]['controls']
                 if args.axis_c_mutation else
                 sum(AXIS_C_GROUPS.values(), []) if args.axis_c_replacement == 'all' else
                 AXIS_C_GROUPS[args.axis_c_replacement])
        return run_axis_c(args, names)
    if args.regenerate_sentence_ties:
        assert args.cell == 'bdb', '#747 fixture regeneration is bdb only'
        assert args.pinyin_so.resolve() == (lib / 'libpinyin.so').resolve(), \
            'fixture expectations must come from the pin, never the subject'
        assert not (args.cases or args.expect_parent or args.zhuyin_so or args.observations), \
            'fixture regeneration cannot be combined with differential or capture modes'
        regenerate_sentence_ties(args.prefix, args.regenerate_sentence_ties)
        return 0
    requested_jobs = ordinary_jobs(parser, args.jobs)
    # The whole set only when no selector is given: an explicit list that
    # names nothing (`--cases ,`), an unknown tag or a tag that matches
    # nothing is an error, never a request for everything.
    try:
        names, reasons = select_cases(args.cases, args.issue, args.family)
    except SelectionError as error:
        parser.error(str(error))
    for line in plan_lines(names, reasons, args.pinyin_so, args.zhuyin_so, args.prefix, args.cell):
        print(line, file=sys.stderr, flush=True)
    if args.plan_only:
        return 0
    with case_jobs(requested_jobs) as jobs:
        started = time.monotonic()
        compare_seconds = 0.0
        failures = 0
        verdict_counts = {}
        if args.observations:
            args.observations.write_text('')
        with tempfile.TemporaryDirectory(prefix='contract-diff-') as scratch, \
                case_results(args, names, data, scratch, jobs) as results:
            for n, pair in results:
                check_cancelled()
                spec = CASES[n]
                if platform_skips(spec) and getattr(args, 'run_platform_excluded', False):
                    print(json.dumps(dict(cell=args.cell, case=n, override='platform-excluded',
                                          reason=spec['platform_skip_reason'],
                                          note='not an ordinary gate'), sort_keys=True),
                          file=sys.stderr, flush=True)
                if pair is PLATFORM_SKIP:
                    verdict_counts['SKIP'] = verdict_counts.get('SKIP', 0) + 1
                    print(json.dumps(dict(cell=args.cell, case=n, verdict='SKIP',
                                          expected_ok=True, exit=[0, 0], stderr_lines=[0, 0],
                                          reason=spec['platform_skip_reason']), sort_keys=True),
                          flush=True)
                    continue
                if pair is None:
                    verdict_counts['SKIP'] = verdict_counts.get('SKIP', 0) + 1
                    print(json.dumps(dict(cell=args.cell, case=n, verdict='SKIP',
                                          expected_ok=True, exit=[0, 0], stderr_lines=[0, 0]),
                                     sort_keys=True), flush=True)
                    continue
                pin, subject = pair
                check_cancelled()
                if args.observations:
                    with args.observations.open('a') as capture:
                        capture.write(json.dumps(dict(cell=args.cell,case=n,pin=pin,subject=subject),ensure_ascii=True)+'\n')
                compared = time.monotonic()
                if any(side['exit'] == 'TIMEOUT' for side in (pin, subject)):
                    verdict, expected, same = 'TIMEOUT', False, False
                else:
                    verdict, expected, same = verdict_for(spec, pin, subject, args.expect_parent)
                compare_seconds += time.monotonic() - compared
                host_dependent = (verdict == 'DIFFER' and not args.expect_parent
                                  and any(map(host_scanf_accepts, spec['host_scanf']))
                                  and pin_loaded_subject_refused_table_conf(pin, subject))
                if host_dependent:
                    verdict = 'HOST-DEPENDENT'
                    expected = True
                verdict_counts[verdict] = verdict_counts.get(verdict, 0) + 1
                row = dict(cell=args.cell, case=n, verdict=verdict, expected_ok=expected,
                           exit=[pin['exit'], subject['exit']],
                           stderr_lines=[pin['stderr_lines'], subject['stderr_lines']])
                if spec['policy_record'] is not None:
                    row.update(record=spec['policy_record'], pin=pin, subject=subject)
                print(json.dumps(row,
                                 sort_keys=True), flush=True)
                if host_dependent:
                    continue
                if not expected:
                    failures += 1
                    if not same:
                        for side, r in (('pin', pin), ('subject', subject)):
                            print('  %s exit=%s %s' % (side, r['exit'], json.dumps(
                                r['result'] and observed(r['result']), sort_keys=True, ensure_ascii=False)))
                            if spec['userdir']:
                                print('  %s userfiles=%s' % (side, json.dumps(r['userfiles'], sort_keys=True)))
                            if spec['stderr']:
                                print('  %s stderr=%r' % (side, r['stderr']))
        print(json.dumps(dict(verdict_counts=verdict_counts), sort_keys=True), flush=True)
        print(json.dumps(dict(timing=dict(timing_summary(names, compare_seconds, time.monotonic() - started), jobs=jobs)),
                         sort_keys=True), flush=True)
        return 1 if failures else 0


def timing_summary(names, compare_seconds, total):
    """Seconds per phase summed over the workers the driver ran; pin and
    subject execution apart (the pin's worker is the first of each pair)."""
    seconds = dict(startup=0.0, load=0.0, fixture=0.0, pin_execute=0.0, subject_execute=0.0,
                   teardown=0.0, compare=compare_seconds, total=total)
    for index, timing in enumerate(TIMINGS):
        for phase in ('startup', 'load', 'fixture', 'teardown'):
            seconds[phase] += timing.get(phase, 0.0)
        seconds[timing.get('side', 'pin' if index % 2 == 0 else 'subject') + '_execute'] += timing.get('execute', 0.0)
    return dict(cases=len(names), workers=len(TIMINGS),
                seconds={phase: round(value, 4) for phase, value in seconds.items()})


def main():
    with cancellation_handlers():
        try:
            status = run_main()
            check_cancelled()
            return status
        except Cancelled as error:
            print(str(error), file=sys.stderr)
            return 1


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--per-call-worker':
        per_call_worker(*sys.argv[2:])
    elif len(sys.argv) > 1 and sys.argv[1] == '--worker':
        # The uncached entry; the driver starts its workers through WORKER.
        worker(*sys.argv[2:])
    else:
        sys.exit(main())
