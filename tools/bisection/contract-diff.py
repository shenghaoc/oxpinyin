#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
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

A class (b) case whose own call would be the pin's undefined behaviour
(`pin_not_run=`) does not run the pin at all: its pin side reads
`NOT-RUN`, and it holds when the subject exits normally and answers the
declared fields, the parts the pin's source defines.

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
conditions stand unchanged in default mode. Under --expect-parent the pin
must satisfy its registered termination contract, and the subject must
complete with a result or terminate as registered for the pin. Other
signals, timeouts, loader failures and missing results are INVALID.
The pin_not_run declaration and held strjoinv_over_read fault keep their
existing classification.

Missing inputs and an empty case list fail. Scratch lives under TMPDIR and is
removed even on failure.

Ordinary cases share a 300-second deadline across pin and subject.
--case-timeout overrides it. A timeout fails in both modes, reporting
INVALID for registered terminations under --expect-parent and TIMEOUT
otherwise. Later cases still run.

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

# The helper modules sit beside this file. A load by path (tests, gcov_measure,
# contract-worker.py) need not have this directory on sys.path.
_HERE = str(Path(__file__).resolve().parent)
if _HERE not in sys.path:
    sys.path.insert(0, _HERE)

# Code that reads none of this module's state lives beside it (#1175). Every name
# that moved is imported back: the cases, contract-diff.test.py, teardown.py and
# gcov_measure resolve them here. A function that did read this module's state takes
# it as an argument, and the adapter of the same name below passes it when called.
from contract_abi import (  # ctypes aliases and facade call helpers
    B, I, P, S, U, UNTOUCHED, Z, Arr, choose_text, consumed, final_step_sentences, full_inst, guessed,
    key_out, owned_text, row_of, row_with_text, sentence_out, tokens_of, unigram_of, zhuyin_chewing,
    zhuyin_sentence_out)
import contract_fixtures
from contract_fixtures import (  # profile, system-directory and DBM-container fixtures
    _craft_bdb, _craft_kc, _craft_tkrzw, _write_user_conf, conf_system, import_rows, private_system, row58_import,
    user_listing, zero_item_padding)
import contract_verdict
from contract_verdict import (  # comparison and verdict rules
    META_OFFSET_TIMESTAMP, NO_ABORT, TKRZW_HASH_METADATA_SIZE, TKRZW_TIMESTAMP_WIDTH, WARNING_DOMAIN,
    _FATAL_GLIB_PREFIX, abort_answered, directory_reads_as_a_chunk, fatal_stderr, host_scanf_accepts, observed,
    pin_loaded_subject_refused_table_conf, policy_observation, registered_termination, strjoinv_fault,
    tkrzw_saved_bigram_bytes, tkrzw_saved_bigram_equal, valid_observation)
import contract_selection
from contract_selection import SelectionError, case_names, family_of, normal_tag  # case selection
import contract_registry
from contract_registry import case_spec  # registration metadata and checks
import contract_measurement
import contract_axis_c
import contract_alloc_after_fini
import contract_default_options
import contract_invalid_input_bytes
import contract_negative_remember
import contract_input_length_cap
import contract_double_fini
import contract_non_utf8_directories
import contract_axis_j
import contract_predicted_types
import contract_facade_total
import contract_stored_count_headers
import contract_issue542_remaining
import contract_pinyin_out_parameters
import contract_candidate_lookup_offsets
import contract_remember_tone
import contract_allocator_register
import contract_cursor_sentence_refusals
import contract_duplicate_phrase_add
import contract_zhuyin_facade_twins
import contract_scheme_library_refusals
import contract_row54_empty_forcing
import contract_zero_consumption
import contract_glib_arrays
import contract_reading_256th
import contract_auxiliary_text
import contract_combined_import_nbest
import contract_stored_total_overflow
import contract_zhuyin_per_entry
import contract_case_helpers
# Helpers several case families share (#1175). Each is re-exported by assignment, not
# imported by name: CPython 3.15 compiles a call on an imported name differently, and the
# remaining cases here must compile as they did when the helpers were defined in this file.
ADDON_PROMOTE_INPUTS = contract_case_helpers.ADDON_PROMOTE_INPUTS
FACADE_TOTAL = contract_case_helpers.FACADE_TOTAL
LIBRARY_FACADES = contract_case_helpers.LIBRARY_FACADES
PHRASE_TEXT_WORDS = contract_case_helpers.PHRASE_TEXT_WORDS
PREFIX_WORDS = contract_case_helpers.PREFIX_WORDS
TOKEN_REUSE_PHRASES = contract_case_helpers.TOKEN_REUSE_PHRASES
USER_ALL = contract_case_helpers.USER_ALL
USER_INDEX_FILES = contract_case_helpers.USER_INDEX_FILES
_DALONG = contract_case_helpers._DALONG
_PAST_RANGES = contract_case_helpers._PAST_RANGES
_add_user_phrase = contract_case_helpers._add_user_phrase
_chunk = contract_case_helpers._chunk
_dynamic_adjust_out = contract_case_helpers._dynamic_adjust_out
_facade_total_out = contract_case_helpers._facade_total_out
_gram_bytes = contract_case_helpers._gram_bytes
_gram_items = contract_case_helpers._gram_items
_guess_out = contract_case_helpers._guess_out
_modify_header = contract_case_helpers._modify_header
_plant_user_grams = contract_case_helpers._plant_user_grams
_reload_pron_keys = contract_case_helpers._reload_pron_keys
_replace_system_start_row = contract_case_helpers._replace_system_start_row
_replace_user_bigram = contract_case_helpers._replace_user_bigram
_segment_out = contract_case_helpers._segment_out
_stored_library_total = contract_case_helpers._stored_library_total
_ucs4_key = contract_case_helpers._ucs4_key
_user_candidate_rows = contract_case_helpers._user_candidate_rows
_write_log = contract_case_helpers._write_log
candidate_rows = contract_case_helpers.candidate_rows
d18_bigram_db = contract_case_helpers.d18_bigram_db
d18_export_phrases = contract_case_helpers.d18_export_phrases
d18_gram = contract_case_helpers.d18_gram
glib_utf8 = contract_case_helpers.glib_utf8
guess_rows = contract_case_helpers.guess_rows
log_walk = contract_case_helpers.log_walk
phrase_table_reopen = contract_case_helpers.phrase_table_reopen
phrase_text_bytes = contract_case_helpers.phrase_text_bytes
predicted_rows = contract_case_helpers.predicted_rows
remember_export = contract_case_helpers.remember_export
row58_rows = contract_case_helpers.row58_rows
token_phrase_of = contract_case_helpers.token_phrase_of
token_unigram_trained = contract_case_helpers.token_unigram_trained
token_unigram_user = contract_case_helpers.token_unigram_user
train_once = contract_case_helpers.train_once
train_selected_top = contract_case_helpers.train_selected_top
transient_files = contract_case_helpers.transient_files
wrap_total_to_zero = contract_case_helpers.wrap_total_to_zero
zhuyin_guess_rows = contract_case_helpers.zhuyin_guess_rows
import contract_before_cursor_dedup
import contract_token_unigram_training
import contract_bigram_removal
import contract_candidate_removals
import contract_null_user_library
import contract_bigram_mask_prediction
import contract_sort_pinyin_length
import contract_system_mask_out
import contract_phrase_index_logger
import contract_phrase_table_mask
import contract_punctuation
import contract_training_constraints
import contract_user_stored_words
import contract_toned_keys_zhuyin_refusals
import contract_final_step_surface
import contract_zero_key_guess
import contract_token_reuse_addon_import
import contract_system_phrase_text
import contract_prefix_conversion
import contract_facade_totals_counts
import contract_changed_parse_constraints
import contract_stored_user_gram
import contract_phrase_export_training
import contract_user_index_phrase_mask
import contract_glib_path_construction
import contract_matrix_resplit_windows
import contract_library_reload
import contract_user_conf_overlong_index
import contract_row58_ranking
import contract_zhuyin_null_arguments
import contract_save_failures_stderr
save_929_marker_interrupt = contract_save_failures_stderr.save_929_marker_interrupt
save_929_physical_files = contract_save_failures_stderr.save_929_physical_files
import contract_integer_limits_d18
import contract_c_string_inputs
import contract_gram_lookup
import contract_library_loading
import contract_table_conf_scanner
hash_path_fixture = contract_table_conf_scanner.hash_path_fixture
import contract_journal_replay
import contract_axis_c_cases
import contract_kit
import contract_runner
from contract_runner import Cancelled
import contract_execution
from contract_execution import ordinary_jobs, positive_jobs, terminate_group, user_dir_listing  # worker and run-output helpers

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
    `host_scanf=<spellings>` is legacy metadata for `%f` inputs whose pin
    acceptance varies by host libc. Current scanner cases use discrepancy
    records instead. The pin reads them with the host libc, so on a host whose scanner
    accepts one
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

    `pin_not_run=<fields>` marks a class (b) site where the case's own call
    would be the pin's undefined behaviour (a read of storage the call has
    freed): the pin is not run and is reported `NOT-RUN`, the subject must
    exit normally and answer `<fields>`, and the verdict compares the
    registered declaration, not the two libraries.

    `policy_record` links a discrepancy record whose evidence is checked at
    verdict time. Accepted differences are reported as `POLICY-ACCEPTED`,
    never `MATCH`; registration checks option compatibility, not the record's
    contents."""
    def register(fn):
        """Store the case specification in CASES as it is now (contract_registry)."""
        return contract_registry.register(CASES, name, fn, *args, **kwargs)
    return register


# The selection rules are in contract_selection. These keep their names and
# signatures and select from the CASES this module holds when they are called.
def tag_index():
    """issue tag -> sorted case IDs, and 'family/<F>' -> IDs; axis C excluded (contract_selection)."""
    return contract_selection.tag_index(CASES)


def select_cases(cases=None, issues=(), families=()):
    """Return case IDs in selection order and the reasons accumulated per ID (contract_selection)."""
    return contract_selection.select_cases(CASES, cases, issues, families)


def plan_lines(names, reasons, pinyin_so, zhuyin_so, prefix, cell):
    """The selection plan: IDs and reasons, facades and artifacts (contract_selection)."""
    return contract_selection.plan_lines(CASES, names, reasons, pinyin_so, zhuyin_so, prefix, cell)


class Kit(contract_kit.Kit):
    """Keep the driver class and method patch points for workers and PerCallKit."""

    def __init__(self, mode, so, data, scratch):
        super().__init__(mode, so, data, scratch, cell=CELL, check_tkrzw=check_tkrzw)


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


# Fixture helpers that read the per-run cell (`CELL`), directly or through one another.
# They stay in the driver, where the tests patch `CELL` and the crafters, and are
# defined above the families that receive them at registration.

def _craft_db(path, rows, dbtype, keep=False, *, system_index=False):
    """A user-table container of raw `key -> value` records in the running
    cell's own format: a Berkeley DB `dbtype` container on bdb, the Kyoto
    Cabinet snapshot the pin's `load_db` reads on kc (every user table
    there is loaded with `load_snapshot`, `chewing_large_table2_kyotodb.cpp:106`,
    `ngram_kyotodb.cpp:54`, `phrase_large_table3_kyotodb.cpp:109`), or the
    tkrzw `TreeDBM` file the pin's `load_db` opens on tkrzw
    (`chewing_large_table2_tkrzwdb.cpp:83`, `ngram_tkrzwdb.cpp:48`,
    `phrase_large_table3_tkrzwdb.cpp:83`); the bigram is a `HashDBM` there
    (`ngram_tkrzwdb.cpp:50`). `dbtype` picks the container kind (1 btree/tree,
    2 hash); the rows are the same records in every cell. `keep=True` is
    the fixture that adds rows to the copied system index: a native attached
    TreeDB on kc (`chewing_large_table2_kyotodb.cpp:87-89`), a TreeDBM on
    tkrzw (`chewing_large_table2_tkrzwdb.cpp:87-88`), and a btree on bdb.
    `system_index=True` also selects that native format for a fresh index;
    with `keep=False` it replaces the file instead of writing a KC snapshot.

    Rows contain raw byte keys and values. Assert dbtype == 1 for KC when
    keep or system_index selects native format. Without keep, replace the
    target; keep updates existing rows. Delegate native failures to the
    writer's assertions and return its result. tkrzw selects HashDBM for type two
    and TreeDBM for other values."""
    if CELL == 'kc':
        assert not (keep or system_index) or dbtype == 1, 'KC system indexes are btrees'
        if system_index:
            return _craft_kc(path, rows, keep=keep, system_index=True)
        return _craft_kc(path, rows, keep=keep)
    if CELL == 'tkrzw':
        return _craft_tkrzw(path, rows, 'HashDBM' if dbtype == 2 else 'TreeDBM', keep=keep)
    return _craft_bdb(path, rows, dbtype, keep=keep)


def _craft_hash(path, rows):
    """A Berkeley DB hash of `key -> value` records (the pin's user bigram
    layout, `ngram_bdb.cpp:56`).

    Dispatch through _craft_db with hash type two and raw byte key/value pairs,
    replacing the current cell's container. The other supported formats are a
    KC snapshot and tkrzw HashDBM."""
    _craft_db(path, rows, 2)  # DB_HASH == 2


def crafted_bigram_context(k, rows):
    """A context whose user dir carries crafted `user_bigram.db` rows
    (`(key_bytes, value_bytes)` records). The first init writes the
    conforming `user.conf`; the rows are then written and the profile
    reopened.

    Write the raw records in the current cell's format and explicitly
    overwrite user.conf before reopening. Assert both initializations;
    the returned context is not cached or finalized here."""
    ctx = k.init()
    assert ctx, 'the first init failed'
    k.fn('fini', None, P)(ctx)
    _craft_hash(os.path.join(k.user, 'user_bigram.db'), rows)
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    return ctx


def predicted_bigram_context(k, phrase, value):
    """A context whose user bigram carries `value` under every
    phrase-table token of `phrase`, so `_compute_prefixes` reaches it. The
    token is read from this side's own table, so the fixture matches the
    pin and the subject alike.

    UTF-8 encode phrase, assert token lookup and use four-byte little-endian
    tokens as keys. Finalize the initial context and replace user_bigram.db
    and user.conf before returning a separately initialized context. Free
    the lookup instance but leave its GArray allocated; do not update kit caches."""
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


def token_reuse_sequence(k, steps, bigram_rows=None, library=7):
    """Runs `steps` over the user library and reports what a caller can see:
    each step's return, the user token of every phrase imported so far, and
    the user files left behind. `('add', n)` imports phrase n, `('save',)`,
    `('mask', mask, value)` and `('reopen',)` are the facade calls.
    `bigram_rows` seeds `user_bigram.db` first, with rows that name the token
    the first import will take.

    Use the chosen library and facade reading column with import count 100.
    A truthy bigram_rows replaces the seeded profile; otherwise use the cached
    context. Reopen finalizes without implicitly saving or replacing the kit's
    cache. Return action results, per-step token mappings and normalized files.
    Unrecognized actions still produce a token snapshot."""
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


def index_file_rows(path, *, kind='tree', allow_empty=False, library=None):
    """Every key/value of a user container, sorted, in this run's cell (contract_fixtures.index_file_rows)."""
    return contract_fixtures.index_file_rows(path, cell=CELL, kind=kind, allow_empty=allow_empty, library=library)


def token_reuse_files(k):
    """The user files a save leaves, by content. The two user index
    containers are compared record by record, a key whose value is empty
    included ([`index_file_rows`]); a Berkeley DB user bigram is read back
    through `db_dump` and only its non-empty rows are compared (on the other
    backends only that the bigram container exists), `user.bin` is compared
    whole except for the 4 timestamp bytes of its header, and every other file
    is compared as written.

    Skip non-regular entries and .tmp files; normalize pinyin-item padding
    in index rows. Recognize BDB-magic containers for db5.3_dump. The header
    bytes zeroed are 4..7, also in addon.bin and network.bin; other file content
    is hex encoded. File, reader and subprocess failures propagate."""
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


def addon_promote_sequence(k, mask_value):
    """Promotes the addon candidates in turn (`pinyin_choose_candidate`); with
    `mask_value`, masks that token after the third and before the fourth, with
    no reopen between.

    Load addon libraries four and five, save after the first three promotions
    and again after the fourth. Assert library loads and candidate presence;
    return selection/mask/save results, token observations and normalized files.
    The cached context retains all changes."""
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


def phrase_text_system(k, variant):
    """Two native items: 甲<word>乙 / da1'da1'da1, then 丙 / ba1.

    Layout: phrase_index.cpp's SubPhraseIndex; PinyinIndexItem2<L> at
    pinyin_phrase3.h:181-197 (four-byte aligned token then packed keys).
    ChewingKey at chewing_key.h:41-47 has tone at bit 12; da = 4|1<<7,
    ba = 1|1<<7 (chewing_enum.h:16,19,65). No API import repairs the words.
    """
    word = PHRASE_TEXT_WORDS[variant]
    items_system(k, [((0x7532, word, 0x4e59), (0x1084,) * 3, 100), ((0x4e19,), (0x1081,), 100)])


def items_system(k, items, total=200):
    """A private system whose library 1 holds exactly `items`, tokens 1, 2, ….

    Each item is `(words, keys, unigram)`, written raw: no API import
    repairs a non-scalar word. Items that share a reading share one
    `pinyin_index.bin` value, in token order; every proper prefix of a
    reading or a text is an empty marker row, as import writes them.
    Replace the private links to gb_char.bin, pinyin_index.bin and
    phrase_index.bin, write user.conf and point the kit at the system."""
    system = private_system(k)
    content, offsets = bytearray(8), [0]
    phonetic, phrases = {}, {}
    for slot, (text, reading, unigram) in enumerate(items, 1):
        token = 0x01000000 | slot
        raw_text = b''.join(value.to_bytes(4, 'little') for value in text)
        raw_keys = b''.join(value.to_bytes(2, 'little') for value in reading)
        offsets.append(len(content))
        content += bytes((len(text), 1)) + unigram.to_bytes(4, 'little') + raw_text
        content += raw_keys + (10).to_bytes(4, 'little')
        index = b''.join((value & 0x0fff).to_bytes(2, 'little') for value in reading)
        for end in range(2, len(index), 2):
            phonetic.setdefault(index[:end], b'')
        item = token.to_bytes(4, 'little') + raw_keys
        phonetic[index] = phonetic.get(index, b'') + item + bytes((-len(item)) % 4)
        for end in range(4, len(raw_text), 4):
            phrases.setdefault(raw_text[:end], b'')
        phrases[raw_text] = phrases.get(raw_text, b'') + token.to_bytes(4, 'little')
    index_two = 17 + len(offsets) * 4 + 1
    payload = b''.join(value.to_bytes(4, 'little') for value in
                       (total, 17, index_two, index_two + len(content) + 1))
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


# --------------------------------------------------------------------------
# Cases. A control case holds on the parent as well; every other case is the
# regression guard of the fix named in its comment.
# --------------------------------------------------------------------------


# The #532 default-option cases stay at their original point in the registration sequence.
contract_default_options.register(case)


# The #587 invalid-byte cases stay at their original point in the registration sequence.
contract_invalid_input_bytes.register(case)


contract_zero_key_guess.register(case)


class Rest(C.Structure):
    """Mirror the two unsigned 16-bit raw-input offsets of ChewingKeyRest.

    The layout is _ChewingKeyRest in src/storage/chewing_key.h at pin
    074a2219; begin and end measure byte positions in the parsed input."""
    _fields_ = [('begin', C.c_ushort), ('end', C.c_ushort)]


contract_pinyin_out_parameters.register(case, Rest=Rest)


contract_auxiliary_text.register(case)


# batch2 group 7: the sub-index unigram total guard (PR 7, #540)


contract_token_unigram_training.register(case)


# batch2 group 8: choose_predicted_candidate's return table (PR 8, #542, #540)


contract_predicted_types.register(case, predicted_rows=predicted_rows)


contract_punctuation.register(case, _craft_db=_craft_db)


# The remember-count cases stay at their original point in the registration sequence.
contract_negative_remember.register(case)


# The #540 input-length cases stay at their original point in the registration sequence.
contract_input_length_cap.register(case)


# The #528 case stays at its original point in the registration sequence.
contract_alloc_after_fini.register(case)


# The #573 double-fini cases stay at their original point in the registration sequence.
contract_double_fini.register(case)


# batch2 group 11: the facade unigram total wraps like the pin's (PR 11, #540)


contract_facade_total.register(case, wrap_total_to_zero=wrap_total_to_zero)


# batch2 group 12a: library-index, scheme and iterator refusals (PR 12a, #525)
contract_scheme_library_refusals.register_pinyin(case)


contract_cursor_sentence_refusals.register(case)


# batch2 group 12c: candidate-type refusals (PR 12c, #525)


contract_candidate_removals.register(case, _craft_hash=_craft_hash)


# batch2 group 12d: training a stale forcing (PR 12d, #525, row 6)


contract_training_constraints.register(case)


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


contract_user_conf_overlong_index.register(case, _craft_db=_craft_db)
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


contract_bigram_mask_prediction.register(
    case, _craft_hash=_craft_hash, crafted_bigram_context=crafted_bigram_context, index_file_rows=index_file_rows)


# #762: the pin hands a new user phrase `range.m_range_end` of the user
# sub-index (`pinyin.cpp:589-592`), and `SubPhraseIndex::get_range` trims
# trailing zero offsets (`phrase_index.cpp:624-650`), so a mask that empties
# the top of the range frees those tokens for the next import.


contract_token_reuse_addon_import.register(case, token_reuse_sequence=token_reuse_sequence)


contract_before_cursor_dedup.register(case, addon_promote_sequence=addon_promote_sequence)


contract_user_index_phrase_mask.register(
    case, predicted_bigram_context=predicted_bigram_context, token_reuse_files=token_reuse_files)


contract_toned_keys_zhuyin_refusals.register(case)


contract_scheme_library_refusals.register_zhuyin(case)


contract_toned_keys_zhuyin_refusals.register_2(case)


# Row 65 / #696. The pin's LONGER row is a property of the whole parse,
# not the lookup offset: `_prepend_longer_candidates` (`pinyin.cpp:1870-
# 1933`) searches the whole matrix with `prefix_len = m_parsed_key_len`
# (`:1876`, `:1883`) and is called for every offset whose sort word leaves
# `SORT_WITHOUT_LONGER_CANDIDATE` clear (`:2292-2293`). Only the main span
# search starts at `offset` (`:2229`), so an offset no span starts on — and
# the reserved slot past the parse — answers the LONGER row alone.


contract_candidate_lookup_offsets.register(
    case, guess_rows=guess_rows, zhuyin_guess_rows=zhuyin_guess_rows)


# The #587 directory-name cases stay at their original point in the registration sequence.
contract_non_utf8_directories.register(case)


contract_zhuyin_facade_twins.register(case, Rest=Rest)


contract_save_failures_stderr.register(
    case, CASES=CASES, crafted_bigram_context=crafted_bigram_context, index_file_rows=index_file_rows, token_reuse_sequence=token_reuse_sequence)


# The raw `mmap %s failed!` lines of the pin's library loaders (#545;
# `pinyin.cpp:256`, `:290`, `:956`, `:1265`, `zhuyin.cpp:200`, `:589`,
# `:800`). Each site hands the chunk it failed to map, empty, on: the
# library load reads `NULL + 16` and the save's `diff` does the same on the
# old chunk, so the pin dies of SIGSEGV right after the line (register
# row 68, class (b)). The system directory is a private copy of the oracle's
# (every file a link back to it) with one library broken; the oracle's own
# files are never touched.


contract_library_loading.register(case)


# --------------------------------------------------------------------------


contract_remember_tone.register_tones(case, remember_export=remember_export)


contract_glib_path_construction.register(case)


contract_remember_tone.register_review(case, remember_export=remember_export)


# #642: NULL is transient state, not an absent user index. Snapshot both
# filesystem destinations at every step, including a pre-existing canary.


contract_null_user_library.register(case)


contract_phrase_export_training.register(case)


contract_combined_import_nbest.register(
    case, remember_export=remember_export, transient_files=transient_files)


# The axis J controls stay at their original point in the registration sequence.
contract_axis_j.register(case)


# Timing consumers and tests patch this list through the driver at call time.
TIMINGS = []
_RUNNER = contract_runner.Runner(globals())
WORKER_STATE = _RUNNER.worker_state
PLATFORM_SKIP = _RUNNER.platform_skip


def __getattr__(name):
    """Expose the runner's legacy state names without duplicate driver globals."""
    if name in contract_runner.STATE_NAMES:
        return _RUNNER.setting(name)
    raise AttributeError(name)


def check_cancelled():
    """Driver patch point for contract_runner.Runner.check_cancelled."""
    return _RUNNER.check_cancelled()


def cancellation_handlers():
    """Driver patch point for contract_runner.Runner.cancellation_handlers."""
    return _RUNNER.cancellation_handlers()


def run_worker(mode, so, data, name, scratch, extra_env=None, timeout=None, entry=None):
    """Driver patch point for contract_runner.Runner.run_worker."""
    return _RUNNER.run_worker(mode, so, data, name, scratch, extra_env, timeout, entry)


def run_case(spec, pin_so, subject_so, data, name, scratch, timeout):
    """Driver patch point for contract_runner.Runner.run_case."""
    return _RUNNER.run_case(spec, pin_so, subject_so, data, name, scratch, timeout)


def case_jobs(requested):
    """Driver patch point for contract_runner.Runner.case_jobs."""
    return _RUNNER.case_jobs(requested)


def case_results(args, names, data, scratch, jobs):
    """Driver patch point for contract_runner.Runner.case_results."""
    return _RUNNER.case_results(args, names, data, scratch, jobs)


# #640 leading separators, #698 retained before-cursor windows, #681 zero keys.
# Calls reuse the existing isolated worker and distinguish NULL from untouched.
def borrowed_text(pointer):
    """Decode a borrowed C string without freeing it, preserving untouched and NULL outputs.

    Return 'untouched' for the sentinel and None for NULL; invalid UTF-8 raises."""
    if pointer.value==UNTOUCHED:
        return 'untouched'
    return C.string_at(pointer.value).decode() if pointer.value else None


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


contract_allocator_register.register(case)


contract_row54_empty_forcing.register(case)


contract_zero_consumption.register(case)


contract_changed_parse_constraints.register(case)


contract_row58_ranking.register(case)


contract_matrix_resplit_windows.register(case)

# --------------------------------------------------------------------------
# #694 and #525 batch B: the system `table.conf` decides the library set.
# `SystemTableInfo2::load` (`table_info.cpp:194-294` at 074a2219) reads the
# header with five `fscanf`s and the rows as words in groups of six; the
# rows' file types and names drive `pinyin_init`/`zhuyin_init`'s library loop
# (`pinyin.cpp:377-392`), the loads, `_write_files` and `_rename_files`.
# Every case runs on a private copy of the oracle's system directory whose
# `table.conf` is rewritten; the data files are links to the oracle's.
# --------------------------------------------------------------------------


contract_table_conf_scanner.register(case, CASES=CASES, WORKER=WORKER)


contract_final_step_surface.register(case)


contract_sort_pinyin_length.register(case, CASES=CASES)


# #728: mask_out rebuilds loaded system sub-indexes from disk, then merges
# the saved user log with the mask (074a2219 pinyin.cpp:1253-1285,
# zhuyin.cpp:788-820). The separate phrase table keeps the imported token,
# so token_get_phrase, rather than lookup_tokens, observes the reload.


contract_system_mask_out.register(case)


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


contract_phrase_index_logger.register(case)


contract_journal_replay.register(case)


# Batch F (#525): `zhuyin.cpp`'s phrase-table walk in `_add_phrase`. A token
# whose sub-index equals the target trips `assert(PHRASE_INDEX_LIBRARY_INDEX
# (token) != index)` (`:440`); a lone match whose phrase-index item text
# differs from the phrase trips the `memcmp` assert (`:457`). The crafted
# `user_phrase_index.bin` is written in the running cell's own format
# (`_craft_db`): a Berkeley DB btree on bdb (the pin's own layout,
# `chewing_large_table2_bdb.cpp:58`), a Kyoto Cabinet snapshot on kc
# (`phrase_large_table3_kyotodb.cpp:109`) and a tkrzw `TreeDBM` file on tkrzw
# (`phrase_large_table3_tkrzwdb.cpp:83`).


contract_duplicate_phrase_add.register(case, _craft_db=_craft_db, _ucs4_key=_ucs4_key)


contract_phrase_table_mask.register(case, _craft_db=_craft_db, index_file_rows=index_file_rows)


# #540 / D-18. Small boundary fixtures, never a repeated-training workload.
# Read at libpinyin 074a2219: phrase_index.cpp:56-91, 150-179;
# pinyin_parser2.cpp:274-360; ngram.cpp:277-358; pinyin.cpp:520-605,
# 896-911, 2597-2650; phonetic_lookup.h:880-929.


contract_integer_limits_d18.register(case)


contract_glib_arrays.register_array(case)


# #924: source-derived fatal GArray growth, sharing the ABI failure harness.
# Pin pinyin.cpp:2801-2818 / zhuyin.cpp:1793-1810; GLib 2.89.4
# garray.c:1071-1100 and gmem.c:164-177. The harness checks injection,
# GLib error and fatal signal; successful exit alone is insufficient.
def glib_924_oom_array(k):
    """Require a failed real GArray growth to log an error and terminate."""
    import shlex
    root = Path(__file__).resolve().parents[2]
    executable = Path(k.scratch) / 'alloc-failure'
    flags = shlex.split(subprocess.check_output(
        ['pkg-config', '--cflags', '--libs', 'glib-2.0'], text=True))
    subprocess.run([
        'g++', '-std=c++17', '-O0', '-g', '-Wall', '-Wextra', '-Werror',
        '-fno-builtin', '-rdynamic', str(root / 'tools/abi/alloc-failure.cc'),
        *flags, '-ldl', '-o', str(executable)], check=True, capture_output=True)
    # Array-only has the same fatal expectation for both sides and needs no
    # user directory or subject allocation site. No side-dependent policy
    # exemption. The empty user directory is the pin side's, as in the ABI gate.
    result = subprocess.run([
        str(executable), str(k.lib._name), str(k.data), k.mode, 'pin', '',
        'array-only'], check=True, capture_output=True, text=True)
    expected = f'{k.mode} pin allocation-5: PASS (fatal GLib error)\n'
    assert result.stdout == expected, result.stdout
    return dict(injection=True, level=4, fatal=True)


contract_glib_arrays.register_oom(case, glib_924_oom_array=glib_924_oom_array)


contract_glib_arrays.register_null(case)


contract_issue542_remaining.register(case)


contract_bigram_removal.register(case)


# #757 PR 2: the pin keeps the stored SingleGram array as it is and searches
# it with std::lower_bound (074a2219 stl_lite.h:29; ngram.cpp:161, 185, 215,
# 239, 259), so an unsorted array answers deterministically (Q1) and
# duplicated items are kept: get/set act on the first match, mask_out removes
# every match and subtracts each count, retrieve_all yields stored order (Q2,
# ngram.cpp:86-99, 133-151). Training (phonetic_lookup.h:876-909), predicted
# training (pinyin.cpp:2622-2638), mask and removal (pinyin.cpp:1230, 3764-3766;
# ngram_bdb.cpp:217-254 and twins), export (pinyin.cpp:775-787, 885-890) and
# prediction (pinyin.cpp:2321-2341) all run over that raw array.


contract_stored_user_gram.register(case)


contract_facade_totals_counts.register(case)


contract_library_reload.register(case)


contract_stored_total_overflow.register(
    case, FACADE_TOTAL=FACADE_TOTAL, _stored_library_total=_stored_library_total,
    _facade_total_out=_facade_total_out, _modify_header=_modify_header, _write_log=_write_log)


contract_gram_lookup.register(case, _craft_db=_craft_db)


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


contract_zhuyin_per_entry.register(case)


# #894: source-derived, isolated SYSTEM_FILE words. See the two discrepancy
# records for the complete producer/consumer trace, GLib 2.89.4-2 citations,
# and the binding observations. No pin output is used to generate expectations.


contract_system_phrase_text.register(case, phrase_text_system=phrase_text_system)


# Prefix conversion (#1277). `_compute_prefixes` converts `prefix` with
# `g_utf8_to_ucs4(prefix, -1, NULL, &len, NULL)` (pinyin.cpp:1396-1398;
# zhuyin.cpp:918 at pin 074a2219). GLib 2.84.4 decodes every one- to
# six-byte form, surrogates and values above U+10FFFF included
# (gutf8.c:661-743), returns NULL for a malformed, overlong or truncated
# string (:938-993), and reads `*prefix` first (:950). The callers reset
# before converting: the plain prediction frees the candidates and empties
# `m_prefixes` (:2420-2423) and answers false with no prefix (:2426-2427);
# the punctuation entry answers true (:2498); `*_guess_sentence_with_prefix`
# sets `m_prefixes` to `[sentence_start]` and runs the n-best on what the
# conversion adds (pinyin.cpp:1431-1441; zhuyin.cpp:949-961). NULL is
# registered class (b) ("NULL prediction prefix"): the pin faults at
# gutf8.c:950, the subject gives the no-prefix outcome (`crash=`).
#
# The private system holds 甲<word>乙, 丙 and 戌 (both ba, 丙 the commoner)
# and 甲<word>乙丙; the system bigram makes 戌 follow 甲<word>乙. So a
# converted prefix seeds 戌 in the sentence and suggests 丙 in prediction.


contract_prefix_conversion.register(case, items_system=items_system)


contract_user_stored_words.register(case, phrase_text_system=phrase_text_system)


contract_stored_count_headers.register(case, _chunk=_chunk)


contract_reading_256th.register(case)


contract_zhuyin_null_arguments.register(case)


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
        """Encode texts as comma-separated IDs from the enclosing row's shared pool."""
        return ','.join(str(ids[text]) for text in items)
    def displayed(items):
        """Encode displayed (index, text) pairs as index:pooled-ID entries in row order."""
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
        """Read key=value lines, ignoring empty, comment and separator-free lines.

        Split only the first equals sign and let later duplicate keys replace earlier
        values; propagate file-read errors.
        """
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


# The replacement definition loader and the battery live in contract_axis_c (#1175).
axis_c_definition = contract_axis_c.axis_c_definition


# The registration tables of the axis C family, built anew by every load of this driver.
AXIS_C_GROUPS = {name: [] for name in ('options', 'encoding', 'layouts')}
AXIS_C_DEFINITION = axis_c_definition()
# The invalid scheme numbers whose abort the pin takes and the register already
# classifies (rows 5c/5d). These layout cases are matched with the classic
# class (c) `abort=` expectation at the nested-observation level and counted as
# covered, not DIFFER, exactly as `abort-zhuyin-chewing-scheme-*` is.
AXIS_C_LAYOUT_COVERED = {}


contract_axis_c_cases.register(
    case, AXIS_C_DEFINITION=AXIS_C_DEFINITION, AXIS_C_GROUPS=AXIS_C_GROUPS, AXIS_C_LAYOUT_COVERED=AXIS_C_LAYOUT_COVERED, WORKER=WORKER)


# The battery's comparison rules read no driver state and are re-exported. The
# snapshot check and the battery keep their signatures and pass the registry,
# the worker and the tables the registration above built, as they are when called.
axis_c_canonical = contract_axis_c.axis_c_canonical
axis_c_normalize_diagnostics = contract_axis_c.axis_c_normalize_diagnostics
axis_c_covered_abort = contract_axis_c.axis_c_covered_abort
AXIS_C_ABORT_ROW_FIELDS = contract_axis_c.AXIS_C_ABORT_ROW_FIELDS
AXIS_C_REFUSAL_RESULT_FIELDS = contract_axis_c.AXIS_C_REFUSAL_RESULT_FIELDS
axis_c_live = contract_axis_c.axis_c_live
axis_c_licensed_remnant = contract_axis_c.axis_c_licensed_remnant
axis_c_covered_residual = contract_axis_c.axis_c_covered_residual
axis_c_first_difference = contract_axis_c.axis_c_first_difference


def check_axis_c_snapshots(args):
    """Compare allocation with fork snapshots through the driver's run_worker (contract_axis_c)."""
    return contract_axis_c.check_axis_c_snapshots(args, run_worker=run_worker)


def run_axis_c(args, names):
    """Run the battery over the driver's CASES, worker and axis C tables (contract_axis_c)."""
    return contract_axis_c.run_axis_c(args, names, CASES=CASES, run_worker=run_worker,
                                      AXIS_C_LAYOUT_COVERED=AXIS_C_LAYOUT_COVERED,
                                      AXIS_C_DEFINITION=AXIS_C_DEFINITION)


POLICY_ROOT = Path(__file__).resolve().parents[2] / 'docs/findings'


# The rules are in contract_verdict. These keep their names and signatures and hand
# them the state this module owns, read when they are called.
def policy_expectations(spec):
    """Read the current registered record, never a cached exemption (contract_verdict)."""
    return contract_verdict.policy_expectations(spec, policy_root=POLICY_ROOT, cell=CELL)


def verdict_for(spec, pin, subject, expect_parent):
    """The driver's decision for one case: (verdict, expected_ok, same) (contract_verdict)."""
    return contract_verdict.verdict_for(spec, pin, subject, expect_parent, policy_root=POLICY_ROOT, cell=CELL,
                                        directory_probe=directory_reads_as_a_chunk)


def short_dir_chunk(spec):
    """A `dir_chunk` case on a host where the pin does not reach its assert (contract_verdict)."""
    return contract_verdict.short_dir_chunk(spec, directory_probe=directory_reads_as_a_chunk)


def matches(spec, pin, subject):
    """The verdict predicate shared by the driver and gcov measurement tier (contract_verdict)."""
    return contract_verdict.matches(spec, pin, subject, directory_probe=directory_reads_as_a_chunk)


# Per-call measurement lives in contract_measurement. The driver retains its
# state, entry points and patch points, passing their current values on each call.
PER_CALL_OPS = contract_measurement.PER_CALL_OPS
PER_CALL_NAMES = contract_measurement.PER_CALL_NAMES
PER_CALL_NATIVE = contract_measurement.PER_CALL_NATIVE
per_call_native = contract_measurement.per_call_native
per_call_ir = contract_measurement.per_call_ir
per_call_profile = contract_measurement.per_call_profile
per_call_attribution = contract_measurement.per_call_attribution
per_call_calibration = contract_measurement.per_call_calibration


class PerCallKit(contract_measurement.PerCallKit, Kit):
    """Keep the driver's Kit in the measurement kit's method resolution order."""


def per_call_helper(scratch):
    """Compile the helper using the driver's current source constant."""
    return contract_measurement.per_call_helper(scratch, PER_CALL_NATIVE=PER_CALL_NATIVE)


def per_call_worker(mode, so, data, name, library, metric, operations):
    """Run the measurement worker with the driver's current kit and registry."""
    return contract_measurement.per_call_worker(mode, so, data, name, library, metric, operations,
                                                PerCallKit=PerCallKit, Kit=Kit, CASES=CASES,
                                                per_call_native=per_call_native)


def per_call_run(mode, so, data, name, helper, metric, operations, scratch, cpu, attribution=False):
    """Run a sample using the driver's current worker path and callbacks."""
    return contract_measurement.per_call_run(mode, so, data, name, helper, metric, operations, scratch, cpu,
                                             attribution, driver_file=__file__, CELL=CELL, check_tkrzw=check_tkrzw,
                                             check_cancelled=check_cancelled, terminate_group=terminate_group,
                                             user_dir_listing=user_dir_listing, per_call_ir=per_call_ir,
                                             per_call_profile=per_call_profile)


def per_call_checked(spec, reference, sample, operations):
    """Check a sample with the driver's current verdict evaluator."""
    return contract_measurement.per_call_checked(spec, reference, sample, operations, verdict_for=verdict_for)


def run_per_call(args, parser):
    """Run the controller using the driver's current registry and patch points."""
    return contract_measurement.run_per_call(args, parser, CASES=CASES, FUNCTION_ORDER=FUNCTION_ORDER,
                                            per_call_helper=per_call_helper, per_call_run=per_call_run,
                                            per_call_checked=per_call_checked, per_call_calibration=per_call_calibration,
                                            per_call_attribution=per_call_attribution, observed=observed)


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
    """Parse driver options and dispatch listing, planning or the requested execution mode.

    Validate selectors and pinned inputs, setting CONTRACT_DIFF_CELL after parsing.
    Ordinary runs print verdicts and timing summaries, optionally replace/append
    the observation capture, and return one for unexpected results or zero
    otherwise. Specialized modes return their own status; invalid arguments and
    input assertions abort before ordinary case execution.
    """
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    FUNCTION_ORDER['add_arguments'](parser)
    parser.add_argument('--teardown', action='store_true', help='native teardown measurement workload')
    parser.add_argument('--teardown-stage', choices=('provision', 'verify', 'pilot', 'confirm', 'repeat', 'multi', 'attribute', 'init-attribute'), default='verify')
    parser.add_argument('--teardown-attribute-window', choices=('init', 'first-candidate'), default='init',
                        help='window for the existing init-attribute probes; first-candidate retains RandomState Ir ranges')
    parser.add_argument('--teardown-init-probes', default='count,sample,sample-coarse,faults,trace,pages',
                        help='comma-separated init-attribute probes, or none; Callgrind always runs three fresh processes per arm')
    parser.add_argument('--teardown-summary-dir', type=Path,
                        help='empty local directory for complete per-process Callgrind summaries; never overwrites')
    parser.add_argument('--teardown-startup-heap', action='store_true',
                        help='also report existing Massif startup snapshots in the confirm stage')
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
                if (any(side['exit'] == 'TIMEOUT' for side in (pin, subject))
                        and (not args.expect_parent or not registered_termination(spec)
                             or spec.get('pin_not_run') is not None)):
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
    """Seconds per phase summed over the workers the driver ran (contract_execution)."""
    return contract_execution.timing_summary(TIMINGS, names, compare_seconds, total)


def main():
    """Run the CLI under cancellation handlers and translate cancellation to status one.

    Check once more after the driver returns, print cancellation diagnostics to
    stderr, and allow other errors to propagate.
    """
    with cancellation_handlers():
        try:
            status = run_main()
            check_cancelled()
            return status
        except Cancelled as error:
            print(str(error), file=sys.stderr)
            return 1


contract_c_string_inputs.register(case, items_system=items_system)


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--per-call-worker':
        per_call_worker(*sys.argv[2:])
    elif len(sys.argv) > 1 and sys.argv[1] == '--worker':
        # The uncached entry; the driver starts its workers through WORKER.
        worker(*sys.argv[2:])
    else:
        sys.exit(main())
