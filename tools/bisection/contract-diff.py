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
    """Read the first sentence, returning None for both untouched output and NULL.

    Ignore the native flag; decode/free returned text with Kit.text."""
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


def conf_session(k, system):
    """Open on `system`, look up, learn one sentence and one phrase, save:
    what the rows decide to load, to accept and to write."""
    ctx = k.init(system=system)
    if not ctx:
        return {'ctx': False}
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
    """Put literal #type=kct text into the selected private directory or configured filename.

    placement selects system-directory, user-directory, system-filename or
    user-filename; otherwise raise ValueError. Rename directories or replace
    the private table.conf link, update k.data and optionally k.user, and return
    (system chunk name, journal name, user chunk name). Do not initialize a context."""
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
    """Build a training/import/save/reopen case using the selected literal-hash path placement."""
    def probe(k):
        """Assert trained and imported counts survive saving through the renamed paths.

        Check expected files and absence of replaced names, reopen after clearing
        cached pointers, and compare unigram outputs. Close again, then launch the
        companion pinyin library's export worker with AXIS_C_REOPEN_USER. Require
        zero exit and nonempty JSON export rows; the subprocess has no timeout here."""
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
    """Register edited-config session cases for the selected facades.

    Compute control status from the named control sets, allowing options to
    override it. Each registered callable builds a private system when run;
    registration itself does not initialize native state."""
    for mode in modes:
        def probe(k, edits=edits):
            """Apply the captured config edits and return the selected session's observations."""
            return session(k, conf_system(k, *edits))
        probe.config_edits = edits
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
    """Register expected-abort config cases for initialization or a library load.

    call='init' tests initialization; otherwise unpack (kind, index), using
    'load' for a normal library and any other kind for an addon. Forward expected
    ret, userdir, directory-chunk and control metadata to case registration."""
    for mode in modes:
        def probe(k, edits=edits):
            """Initialize with the captured config edits and perform the selected load if requested.

            Return its truth/flag as ret; loads require successful initialization and
            leave the context live."""
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
    """Write an LMDB database-format marker into user.conf and record initialization truth.

    Assert the replacement matched before overwriting the profile marker."""
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
    """Build an initialization-only probe with text config edits.

    The issues argument is unused here; registration owns issue metadata."""
    def probe(k):
        """Create the edited private system and return the new context's truth value."""
        return {'ctx': bool(k.init(system=conf_system(k, *edits)))}
    probe.config_edits = edits
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    case('table-conf-lambda-line-missing' + _suffix, mode=_mode,
         stderr=True, issues=('694', '525'))(header_probe(('-', 'lambda parameter')))
    # glibc's scanner does not back up: an exponent mark with no digit after
    # it, a `0x` with no hex digit and `infin` fail the `%f` conversion.
    for _name, _spelling in (('exponent-without-digits', '1e'), ('hex-without-digits', '0x'),
                             ('hex-exponent-without-digits', '0x1p'), ('infin', 'infin')):
        # Each host's libc decides acceptance identically for both facades.
        case('table-conf-lambda-' + _name + _suffix, mode=_mode, stderr=True,
             issues=('694', '525'))(header_probe(('lambda parameter:0.312699', 'lambda parameter:' + _spelling)))


# Every spelling glibc's `%f` accepts, hexadecimal floats included.
for _name, _spelling in (('hex', '0x1p-1'), ('hex-fraction', '0x.8'), ('hex-upper', '0X1.P-1')):
    conf_case('lambda-' + _name, ('lambda parameter:0.312699', 'lambda parameter:' + _spelling),
              modes=('pinyin', 'zhuyin'))


# #919: table.conf scanner partitions. The reference image uses glibc 2.41; the
# cases cover every partition the source walk distinguishes, not only the
# spellings the stock file uses. `control=True` cases already agree with the
# parent; the three incomplete-exponent spellings are the mutation targets.
def conf_919_case(name, mutate, **options):
    """Register both facade sessions with a raw-byte config mutation under issue 919.

    Forward options to case registration; each callable applies mutate only when run."""
    for mode in ('pinyin', 'zhuyin'):
        def probe(k, mutate=mutate):
            """Apply the captured byte mutation to private table.conf and run the standard config session."""
            return conf_session(k, conf_system_bytes(k, mutate))
        probe.config_mutate = mutate
        case('table-conf-919-' + name + ('-zhuyin' if mode == 'zhuyin' else ''),
             mode=mode, issues=('919',), **options)(probe)


def one_line_conf(data):
    """Join the first six newline-delimited config fields with spaces and retain the remaining bytes.

    Insert two newlines before the remainder; fewer than six newlines raises IndexError."""
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

conf_919_case('source-token-nul',
              lambda data: data.replace(b'source table format:pinyin', b'source table format:pinyin\x00suffix'))
conf_919_case('database-token-nul',
              lambda data: data.replace(b'database format:BerkeleyDB', b'database format:BerkeleyDB\x00suffix')
                  .replace(b'database format:KyotoCabinet', b'database format:KyotoCabinet\x00suffix')
                  .replace(b'database format:Tkrzw', b'database format:Tkrzw\x00suffix'))


def header_919(name, *edits, control=True):
    """Register initialization-only header cases for both facades with stderr."""
    for mode in ('pinyin', 'zhuyin'):
        case('table-conf-919-' + name + ('-zhuyin' if mode == 'zhuyin' else ''),
             mode=mode, stderr=True, issues=('919',), control=control)(header_probe(*edits))


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

for _name, _spelling in (
        ('lambda-exponent-sign-without-digits', '1e+'),
        ('lambda-hex-exponent-sign-without-digits', '0x1p+'),
        ('lambda-hex-exponent-minus-without-digits', '0x1p-'),
        ('lambda-hex-dot-without-digits', '0x.'),
        ('lambda-nan-unclosed', 'nan(')):
    for _mode in ('pinyin', 'zhuyin'):
        _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
        case('table-conf-919-' + _name + _suffix, mode=_mode, stderr=True,
             issues=('919',), control=True)(
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

# #1297 Phase 2: these malformed headers now stop before a session exists.
# Keep each fixture and ID; observe initialization directly, without requiring
# the old scanner's successful initialization to reach the case's assertions.
def scanner_initialization(original):
    """Keep the fixture while observing the header parser before user-state work."""
    def probe(k):
        """Observe header acceptance with an already conforming user marker."""
        if hasattr(original, 'config_edits'):
            system = conf_system(k, *original.config_edits)
        else:
            system = conf_system_bytes(k, original.config_mutate)
        _write_user_conf(k)
        return {'ctx': bool(k.init(system=system))}
    return probe

_SCANNER_RECORDS = {
    '1297-table-decimals.md': ('919-versions-positive-overflow', '919-versions-int-wrap'),
    '1297-table-lines.md': ('919-header-one-line', '919-header-leading-space'),
    '1297-table-words.md': ('919-source-token-nul', '919-database-token-nul'),
    '1297-addon-index.md': ('addon-index-wraps', '919-addon-atoi-nondigit'),
}
for _record, _names in _SCANNER_RECORDS.items():
    for _name in _names:
        for _suffix in ('', '-zhuyin'):
            _id = 'table-conf-' + _name + _suffix
            if _id not in CASES:
                continue
            _spec = CASES[_id]
            _original = _spec['fn']
            _spec['fn'] = scanner_initialization(_original)
            _spec.update(policy_record=_record, host_scanf=(), issues=tuple(dict.fromkeys((*_spec['issues'], '919'))))

# Maintainer ruling 2026-10-28: both sides read lambda with the same host
# libc. Preserve current main's initialization observations for these scanner
# fixtures; every spelling must MATCH, including version-sensitive failures.
for _name in ('lambda-hex', 'lambda-hex-fraction', 'lambda-hex-upper', 'lambda-inf', 'lambda-nan',
              '919-lambda-hex-no-exponent', '919-lambda-infinity', '919-lambda-nan-payload',
              '919-lambda-hex-overflow'):
    for _suffix in ('', '-zhuyin'):
        _spec = CASES['table-conf-' + _name + _suffix]
        _spec['fn'] = scanner_initialization(_spec['fn'])
        _spec['issues'] = tuple(dict.fromkeys((*_spec['issues'], '919')))
for _name in ('lambda-exponent-without-digits', 'lambda-hex-exponent-without-digits',
              '919-lambda-exponent-sign-without-digits',
              '919-lambda-hex-exponent-sign-without-digits',
              '919-lambda-hex-exponent-minus-without-digits',
              '919-lambda-hex-dot-without-digits'):
    for _suffix in ('', '-zhuyin'):
        _spec = CASES['table-conf-' + _name + _suffix]
        _spec['issues'] = tuple(dict.fromkeys((*_spec['issues'], '919')))

for _suffix in ('', '-zhuyin'):
    CASES['table-conf-abort-919-word-width-overlong' + _suffix].update(
        policy_record='1297-table-words.md', control=False)
for _suffix, _domain in (('', 'libpinyin'), ('-zhuyin', 'libzhuyin')):
    _spec = CASES['table-conf-919-short-row-merged' + _suffix]
    _spec['crash'] = {'ctx': False, 'logs': [[_domain, 16]]}


# A file name never leaves its directory: `g_build_filename(user_dir,
# "/abs/u.bin")` is `<user_dir>/abs/u.bin`, not `/abs/u.bin`.
def absolute_user_file(k):
    """Observe where a configured absolute-looking user filename is written.

    Create both the named nested directory and its user-root-prefixed counterpart,
    run a trained config session and return listings of both destinations."""
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
    """Register both facade config sessions with stderr comparison enabled."""
    for mode in ('pinyin', 'zhuyin'):
        def probe(k, edits=edits):
            """Build the captured edited system and return the standard session's observations."""
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
    """Build a candidate-guess case against the supplied config edits."""
    def probe(k):
        """Initialize the edited system, parse its first configured input and record sentence/candidate flags.

        Assert init and collect candidate rows through conf_guess_rows even though
        only its flag is returned. Leave the instance and context live."""
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
    """Decode the item's counted four-byte little-endian character fields.

    Use PhraseItem::get_phrase_string's layout in src/storage/phrase_index.cpp:103-105
    at pin 074a2219. Do not validate total extent; chr rejects values above
    Unicode's maximum and an absent count byte raises IndexError."""
    return ''.join(chr(int.from_bytes(item[6 + 4 * i:10 + 4 * i], 'little')) for i in range(item[0]))


def _observe_system_item(k, watched, phrase):
    """Collect watched tokens' phrase/unigram outputs plus phrase-table matches and library-one exports."""
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
    """Register both facade replay cases with the supplied builder and observation options.

    Enable stderr comparison and combine the replay issue IDs with extra_issues;
    the lambdas defer file mutation and native work until case execution."""
    for mode in ('pinyin', 'zhuyin'):
        case('replay-' + name + '-' + mode, mode=mode, stderr=True,
             issues=('851', '850', '870') + tuple(extra_issues), **options)(
            lambda k, build=build: system_replay(k, build, reopen, unigram_add, observe_add,
                                                 observe, observe_saved))


def _watch(p):
    """Return the standard watched-token mapping for the existing one and fresh fixture items."""
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
    """Observe a system unigram through the selected mutation, dirty save and reopen sequence.

    Work on private system links and a single-character item (你 for import-system).
    Variants prepare removed, wrapping, overflowing or missing items, repeat adds,
    reload/mask a library, replace its disk bytes, import a system reading or train.
    Record clean/dirty save flags, journal size/hash/records and post-reopen adds.
    timing additionally performs 1000 increments and emits diagnostic nanoseconds;
    repeat performs a second dirty save/reopen. Leave the final context live."""
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
    """Exercise unigram updates around a save with a literal NULL user directory.

    Replace a private library's stored total with 0xfffffffb, initialize and
    attempt increments of two and four around save. Return count and dynamic
    pricing observations without assuming either update succeeds."""
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
    """Encode an ADD for the builder's fresh token and item bytes."""
    return _log_record(_LOG_ADD, p['fresh'][0], new=p['fresh'][1])


# A matching REMOVE, then an ADD: the item is gone, the total is lower and the
# ADD after it applied.
_replay_case('unigram-add-appended-item-reopen', lambda p: (
    [_add_fresh(p)], {'one': p['fresh'][0], 'original': p['one'][0]}), reopen=True)


def _matched_remove(p):
    """Build a matching REMOVE followed by a fresh ADD and return the standard watched tokens."""
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
    """Build descending-token MODIFY records and a repeated update to the lower token.

    Set last pronunciation frequencies to 77 then 99 and watch both source items
    plus the fresh token; do not execute a replay."""
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
    """Build one frequency override and two successive unigram overrides on the other item.

    Encode 77 for one's last frequency and 71 then 93 for two's unigram;
    return the records and watched tokens."""
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
    """Build relocation, replacement and sparse-slot add/remove records in a fixed sequence.

    Use high token 0x01040000, retain a trailing-byte growth fixture, and return
    records plus watched tokens without touching files."""
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
    """Build a record factory for growing an item with trailing bytes before modifying it again.

    stored_old decides whether the second record compares those trailing bytes
    or only the first replacement's header-sized content."""
    def build(p):
        """Encode two modifications of one followed by a fresh ADD.

        Set the first unigram to the original low unigram byte plus 500, append three
        bytes, then change its last frequency from five to nine. Return watched tokens."""
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
    """Build a record factory that drops one reading but stores four trailing bytes.

    stored_old includes those bytes in the second record's expected old payload."""
    def build(p):
        """Shorten two by six bytes, append a four-byte tail and encode a second frequency update.

        Assert the new header size, append a fresh ADD, and return records and watched tokens."""
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
    """Encode two replacements of one with a six-byte empty-phrase item, then a fresh ADD.

    Keep its initial unigram for the first record and use three for the second."""
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
    """Extend the standard watched-token mapping with the absent fixture token."""
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
    """Build records that modify two, then address an absent token using two's replacement bytes.

    grow appends one byte to the absent-token replacement; both forms finish
    with a fresh ADD and the absent-token watch set."""
    def build(p):
        """Encode frequency 21 on two, then frequency 22 through the absent-token record.

        Optionally grow that replacement by one byte; return records without replaying them."""
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
    """Build an absent-token MODIFY sequence beginning with a six-zero-byte expected item.

    twice adds a second six-byte replacement before the trailing fresh ADD."""
    def build(p):
        """Encode absent-token unigram 50 and optionally 60, then return a fresh ADD and watches."""
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
    """Encode a 30-byte claimed A item whose second MODIFY changes B's overlapping unigram.

    Assert the borrowed extent fits B, choose 170 or 171 to differ from its
    original value, append a fresh ADD and watch all three tokens."""
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
    """Encode an ADD followed by two A modifications claiming a 38-byte cross-item view.

    Assemble the comparison bytes from A, all of B and a fresh-item prefix;
    assert the resulting extent. Change the overlapping B unigram to 170 or
    171 and return A/B/fresh watches without replaying records."""
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
    """Collect a replayed token's phrase, pronunciation keys, unigram and matching export rows.

    Render the token as eight hexadecimal digits; use 'NO PHRASE' when its
    phrase getter fails or yields empty text."""
    phrase = token_phrase_of(k, token)
    text = phrase[2] if phrase[0] else None
    return {'token': '0x%08x' % token,
            'phrase': phrase,
            'n-pron': _reload_pron_keys(k, token),
            'unigram': unigram_of(k, token),
            'export': remember_export(k, 1, text) if text else 'NO PHRASE'}


def _observe_cross_item(k):
    """Select the adjacent fixture pair from the original system bytes and observe both live tokens."""
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
    """Extend standard watched tokens with the item at the highest content offset."""
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
    """Replay repeated invalidating writes to the last 256 system items in reverse offset order.

    For each selected slot, change its last frequency then write a six-byte
    header claiming 255 characters/readings. Append a fresh ADD and observe/save
    through system_replay with unigram increments disabled."""
    _, items, offsets = _system_items(k)
    slots = sorted(offsets, key=offsets.get)[-256:]

    def build(p):
        """Encode paired frequency and oversized-header modifications for the captured slots.

        Return those records plus a fresh ADD and the last-item watch mapping."""
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
    """Build a replay case with a supplied ADD payload between a valid MODIFY and ADD."""
    def build(p):
        """Encode frequency 77 on one, the supplied fresh ADD payload, and a valid fresh ADD."""
        return ([
            _log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                        new=_item_with(p['one'][1], freq=77)),
            _log_record(_LOG_ADD, p['fresh'][0], new=payload),
            _add_fresh(p)], _watch(p))

    def probe(k):
        """Replay the short-payload sequence without a unigram add and alias init truth as ret."""
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
    """Build an item transformer replacing readings and optionally its unigram.

    Each (keys, frequency) pair uses the original first reading's keys when keys
    is None. Preserve entry order and duplicates."""
    def new_item(item):
        """Rebuild captured pronunciation entries, substituting the item's first keys for None."""
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
        """Store adjacent export rows, unigram and phrase outputs under the requested checkpoint tag."""
        out[tag] = {'export': remember_export(k, 1, _PRON_PHRASE, adjacent=True), 'unigram': unigram_of(k, token),
                    'phrase': token_phrase_of(k, token)}

    def train():
        """Assert complete parsing of 癫痫's facade input and train its selected token."""
        assert k.fn(parse, Z, P, S)(k.inst, typed) == len(typed)
        return train_selected_top(k, _PRON_PHRASE, token)

    def save(tag):
        """Record the save flag and complete gb_char.dbin bytes as hex under tag-derived keys."""
        out[tag + '-save'] = k.fn('save', B, P)(k.ctx)
        out[tag + '-dbin'] = Path(k.user, 'gb_char.dbin').read_bytes().hex()

    def reopen(tag):
        """Free/finalize cached objects, clear their pointers and observe the lazily reopened profile."""
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
    """Replay mixed system-item overrides and a fresh ADD, then train/import/save twice.

    Use the fixed 癫痫 token with last frequency 250 and low unigram byte plus
    400, a fresh item at max slot plus two, and a library-seven 龘龘 import.
    Record live exports/counts and complete gb_char.dbin/user.bin hex around
    both reopens; leave final native state cached."""
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
        """Record system/user exports and unigram outputs for the overridden and appended items.

        Scan all rows for the appended item's shared phrase text."""
        out[tag] = {'export': remember_export(k, 1, _PRON_PHRASE, adjacent=True),
                    'user-export': remember_export(k, 7, user_row[0], adjacent=True),
                    'unigram': unigram_of(k, token), 'fresh': unigram_of(k, fresh),
                    'fresh-phrase': token_phrase_of(k, fresh),
                    # The appended item shares its text with slot `one`: walk every row.
                    'fresh-export': remember_export(k, 1, token_phrase_of(k, fresh)[2])}

    def train():
        """Assert complete facade parsing of 癫痫 and train its selected token."""
        assert k.fn(parse, Z, P, S)(k.inst, typed) == len(typed)
        return train_selected_top(k, _PRON_PHRASE, token)

    def save(tag):
        """Save and record complete system-journal and user-library bytes as hex."""
        out[tag + '-save'] = k.fn('save', B, P)(k.ctx)
        for name in ('gb_char.dbin', 'user.bin'):
            out[tag + '-' + name] = Path(k.user, name).read_bytes().hex()

    def reopen(tag):
        """Close cached native objects, clear pointers and record observations after lazy reopening."""
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
    """Remove a four-character system phrase by journal and observe its search paths.

    Choose a single-reading merged item whose key sequence prefixes no longer
    phrase in four system libraries. Obtain its rendered reading, close/reset
    caches, write a REMOVE log and reopen. labels selects exact, longer or
    prefix input; candidates optionally records lists, with before selecting
    zhuyin's before-cursor path. Free per-input instances but retain the context
    and lookup GArray. Missing fixture shapes raise through next/assertions."""
    total, items, _offsets = _system_items(k, 'merged.bin')

    def readings(item):
        """Slice raw pronunciation key sequences using the item's character count and entry stride.

        Follow PhraseItem::get_nth_pronunciation in src/storage/phrase_index.cpp:33-43 at
        pin 074a2219; do not validate slice lengths."""
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
    """Flatten exact-reading observations after removal, retaining candidate rows as diagnostic-only data.

    Delegate fixture setup and optional zhuyin before-cursor lookup to
    _removed_extension; omit its exact sentence field from the returned mapping."""
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


# #1231: a system journal REMOVE leaves the phrase-table token, and the
# prediction walk (`_compute_predicted_prefix_candidates`) reads its item
# without checking the result (`pinyin.cpp:2389-2405` at 074a2219). The
# loop-wide `PhraseItem` keeps the previous success's length, so a missing
# token can pass or be skipped; an appended one carries a NULL phrase
# string. With a sole row both dedup comparisons are skipped and the NULL
# survives; with two or more rows `strcmp` reads it (row 106's class (b)).
# The fixture removes one merged.bin phrase whose prefix is a stored phrase
# and observes the plain prediction API. The stale case uses the reference
# cell's byte-lexical cursor order: a library-one long phrase is read first,
# raises the stale length past the bound, and the missing merged phrase is
# skipped without ever being appended. Other cells' hash order can put the
# missing row first, so those three cases are bdb only.
def _removed_predict_setup(k, phrase):
    """Remove one merged.bin phrase named by its rendered text and reopen.

    Find the slot whose item text is phrase, close the cached context and
    instance, write a REMOVE log for that token and reopen lazily. Return the
    removed token. The phrase table keeps the token."""
    total, items, _offsets = _system_items(k, 'merged.bin')
    slot = next(slot for slot, item in items.items() if _item_text(item) == phrase)
    token = 0x04000000 | slot
    k.fn('free_instance', None, P)(k.inst)
    k.fn('fini', None, P)(k.ctx)
    k._ctx = k._inst = None
    _write_user_conf(k)
    _write_log(k, _modify_header(0, total, total) +
               _log_record(_LOG_REMOVE, token, old=items[slot]), 'merged.dbin')
    return token


def _predicted_surface(k, inst, prefix):
    """Call the plain prediction API and return its flag, count and rows.

    Each row records every getter's flag, the type, and the string with NULL
    kept distinct from an empty string. Return the candidate pointers too, for
    the choose case; they are instance-borrowed and not part of the result."""
    ret = k.fn('guess_predicted_candidates', B, P, S)(inst, prefix.encode())
    count = U()
    got = k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    rows = []
    pointers = []
    for index in range(count.value):
        cand, kind, string = P(), I(), S()
        fetched = k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(cand))
        typed = k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        rendered = k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(string))
        text = None if string.value is None else string.value.decode('utf-8', 'replace')
        rows.append([fetched, typed, kind.value, rendered, text])
        pointers.append(cand)
    return {'ret': ret, 'get-count': got, 'count': count.value, 'rows': rows}, pointers


def _predicted_sole(k):
    """Remove 不仅仅是 and predict from 不仅仅: the sole pre-dedup row.

    Record the sole row's getters, user status, and the token's failed phrase
    read with both outputs left unchanged."""
    token = _removed_predict_setup(k, '不仅仅是')
    inst = k.inst
    surface, pointers = _predicted_surface(k, inst, '不仅仅')
    length, text = U(UNTOUCHED), P(UNTOUCHED)
    phrase_ret = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
        inst, token, C.byref(length), C.byref(text))
    out = {'predicted': surface,
           'token-phrase': [phrase_ret, length.value == UNTOUCHED,
                            text.value == UNTOUCHED],
           'is-user': k.fn('is_user_candidate', B, P, P)(inst, pointers[0]) if pointers else None}
    return out


def _predicted_choose(k):
    """Choose the sole missing row, then read the token's frequency.

    The pin advances the loaded facade total by 483 and leaves the missing
    item unwritten; the later unigram read still fails with its seeded output
    unchanged. (The facade total has no C getter; the runtime's own
    amplified-total assertion covers that side effect.)"""
    token = _removed_predict_setup(k, '不仅仅是')
    inst = k.inst
    surface, pointers = _predicted_surface(k, inst, '不仅仅')
    before = unigram_of(k, token)
    chosen = k.fn('choose_predicted_candidate', B, P, P)(inst, pointers[0]) if pointers else None
    after = unigram_of(k, token)
    return {'predicted': surface, 'choose': chosen,
            'unigram-before': before, 'unigram-after': after}


def _predicted_class_b(k):
    """Remove 这样一个 and predict from 这样一: the missing row and a live row.

    The pin reaches the NULL phrase string in dedup (SIGSEGV); the subject
    drops the missing row and keeps the live prefix candidate."""
    _removed_predict_setup(k, '这样一个')
    surface, _pointers = _predicted_surface(k, k.inst, '这样一')
    return {'ret': surface['ret'], 'count': surface['count'], 'rows': surface['rows']}


def _predicted_stale_skip(k):
    """Remove 明显的 and predict from 明显 in reference-cell cursor order.

    gb_char's 明显的句法线索 (length 7) is read first and is skipped by the
    bound; the failed 明显的 read keeps that length, so it is skipped too.
    The subject drops both; the parent would keep the fallback text."""
    _removed_predict_setup(k, '明显的')
    surface, _pointers = _predicted_surface(k, k.inst, '明显')
    return {'ret': surface['ret'], 'count': surface['count'], 'rows': surface['rows']}


case('removed-predict-sole-pinyin', stderr=True, issues=('1231',))(_predicted_sole)
case('removed-predict-choose-pinyin', stderr=True, issues=('1231',))(_predicted_choose)
case('removed-predict-class-b-pinyin', stderr=True, crash={'ret': True, 'count': 1},
     issues=('1231',))(_predicted_class_b)
case('removed-predict-stale-skip-pinyin', stderr=True, cells=('bdb',),
     issues=('1231',))(_predicted_stale_skip)


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
    """Build an option-word probe with each text observed in a separate fork.

    Reuse a never-parsed instance snapshot unless AXIS_C_ALLOCATE_EACH is '1';
    return the setter status and observations keyed by input position.
    """
    def probe(k):
        """Set the option word and isolate each full-pinyin input observation.

        Allocate the shared snapshot before forking unless per-input allocation is
        explicitly selected; the parent does not parse it.
        """
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
    """Look up UTF-8 phrase bytes and return status plus raw token values.

    Allocate a four-byte-element GArray and free it even if observation fails.
    """
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
    """Import supplied phrase or reading bytes, save, and export through a new process.

    Use library 7 with frequency 1 and a fixed counterpart for the chosen input.
    Require an iterator, close the context, then reopen the saved user directory
    through AXIS_C_COMPANION_PINYIN. Return worker status, decoded export and
    diagnostics with that directory replaced by USER.
    """
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
    """Export the user profile named by AXIS_C_REOPEN_USER through this worker's kit.

    Replace k.user before accessing the context and return the export rows.
    """
    k.user = os.environ['AXIS_C_REOPEN_USER']
    return dict(rows=d18_export_phrases(k))


def axis_c_encoding(operation, text):
    """Build an encoding probe for parser, lookup, segmentation or import entry points.

    Each callback sets options to 0x28 and returns operation-specific outputs;
    remember_user_input first parses nihao and also exports the resulting rows.
    """
    def probe(k):
        """Dispatch the supplied bytes through the chosen encoding-sensitive operation.

        For surrogate segmentation, also observe token outputs and boundary reads,
        bounding the token walk by the input byte length.
        """
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
                    """Return token-call status and the numeric output, including an untouched sentinel."""
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
    """Map one integer byte through the chewing keyboard and release its symbol vector.

    Return status with symbols as a decoded list, None or 'untouched'; invalid
    byte values and invalid UTF-8 propagate conversion errors.
    """
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
    """Build isolated keyboard and parse observations for one layout/option pair.

    Read byte and text inputs from definition; set the facade-specific scheme in
    each child and skip its observation if the setter refuses.
    """
    def probe(k):
        """Set options and fork each registered byte/text layout observation independently."""
        k.fn('set_options', B, P, U)(k.ctx, word)
        def configured(fn):
            """Set this facade's scheme and call fn only when the setter succeeds.

            Return the setter status and either the observation or None.
            """
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
