#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Case-driven pin-against-subject contract differential (lane C).

Usage: contract-diff.py CELL PREFIX PINYIN_SO [--zhuyin-so PATH]
                        [--cases a,b,...] [--issue N]... [--family F]... [--cases-prefix P,Q,...]
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
`contract-diff-policy` JSON block keeps, per case and cell, the SHA-256 of
each side's complete stable observation, a digest per top-level field and
the pin-to-subject divergence as text (`just policy-record` writes it from
an `--observations` capture). The verdict compares the digests, so the
accepted pair is still the complete observation on both sides, with no
wildcard, ignored field or partial match; a pair that differs from the
record is DIFFER, reported with the side and field that moved.

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
whose ID starts with `F/` (separator, before-cursor, zero-key, ...).
--cases-prefix P,Q,... selects IDs beginning with each prefix from the full
registry, including untagged cases. Every prefix must match. Selectors form
a union: explicit IDs first, then issues, families and prefixes; matches
within each issue, family or prefix are sorted, with duplicates kept once
and all selection reasons retained. An
unknown tag, or a selection that matches nothing, is an error that lists the
registered tags; it never falls back to the default suite. --list-tags
prints the tag index as JSON (`tags`: tag to case IDs, with `family/<F>`
entries; `modes`: every registered case ID to facade). Before running, the plan goes to stderr: the
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
    B, I, P, S, U, UNTOUCHED, Z, Arr, Rest, choose_text, consumed, final_step_sentences, full_inst, guessed,
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
import contract_user_key_spellings
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
import contract_separator_before_cursor
import contract_cell_fixtures
import contract_sentence_ties
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


def select_cases(cases=None, issues=(), families=(), prefixes=None):
    """Return case IDs in selection order and the reasons accumulated per ID (contract_selection)."""
    return contract_selection.select_cases(CASES, cases, issues, families, prefixes)


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


# Adapters for the fixtures that read the per-run cell (`CELL`), directly or through one
# another (contract_cell_fixtures). Each passes the driver's current cell, crafters and
# sibling adapters on every call, so tests that patch them here still reach the fixture;
# they are defined above the families that receive them at registration.

def _craft_db(path, rows, dbtype, keep=False, *, system_index=False):
    """Pass the driver's current CELL, _craft_bdb, _craft_kc, _craft_tkrzw on every call (contract_cell_fixtures)."""
    return contract_cell_fixtures._craft_db(path, rows, dbtype, keep, system_index=system_index, CELL=CELL,
                                            _craft_bdb=_craft_bdb, _craft_kc=_craft_kc, _craft_tkrzw=_craft_tkrzw)


def _craft_hash(path, rows):
    """Pass the driver's current _craft_db on every call (contract_cell_fixtures)."""
    return contract_cell_fixtures._craft_hash(path, rows, _craft_db=_craft_db)


def crafted_bigram_context(k, rows):
    """Pass the driver's current _craft_hash on every call (contract_cell_fixtures)."""
    return contract_cell_fixtures.crafted_bigram_context(k, rows, _craft_hash=_craft_hash)


def predicted_bigram_context(k, phrase, value):
    """Pass the driver's current _craft_hash on every call (contract_cell_fixtures)."""
    return contract_cell_fixtures.predicted_bigram_context(k, phrase, value, _craft_hash=_craft_hash)


def token_reuse_sequence(k, steps, bigram_rows=None, library=7):
    """Pass the driver's current crafted_bigram_context, token_reuse_files on every call (contract_cell_fixtures)."""
    return contract_cell_fixtures.token_reuse_sequence(k, steps, bigram_rows, library,
                                                       crafted_bigram_context=crafted_bigram_context,
                                                       token_reuse_files=token_reuse_files)


def index_file_rows(path, *, kind='tree', allow_empty=False, library=None):
    """Every key/value of a user container, sorted, in this run's cell (contract_fixtures.index_file_rows)."""
    return contract_fixtures.index_file_rows(path, cell=CELL, kind=kind, allow_empty=allow_empty, library=library)


def token_reuse_files(k):
    """Pass the driver's current CELL, index_file_rows on every call (contract_cell_fixtures)."""
    return contract_cell_fixtures.token_reuse_files(k, CELL=CELL, index_file_rows=index_file_rows)


def addon_promote_sequence(k, mask_value):
    """Pass the driver's current token_reuse_files on every call (contract_cell_fixtures)."""
    return contract_cell_fixtures.addon_promote_sequence(k, mask_value, token_reuse_files=token_reuse_files)


def phrase_text_system(k, variant):
    """Pass the driver's current items_system on every call (contract_cell_fixtures)."""
    return contract_cell_fixtures.phrase_text_system(k, variant, items_system=items_system)


def items_system(k, items, total=200, *, isolated=False):
    """Pass the driver's current _craft_db on every call (contract_cell_fixtures)."""
    return contract_cell_fixtures.items_system(k, items, total, isolated=isolated, _craft_db=_craft_db)


# --------------------------------------------------------------------------
# Cases. A control case holds on the parent as well; every other case is the
# regression guard of the fix named in its comment.
# --------------------------------------------------------------------------


# The #532 default-option cases stay at their original point in the registration sequence.
contract_default_options.register(case)


# The #587 invalid-byte cases stay at their original point in the registration sequence.
contract_invalid_input_bytes.register(case)


contract_zero_key_guess.register(case)


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


contract_before_cursor_dedup.register(case, addon_promote_sequence=addon_promote_sequence, items_system=items_system)


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


contract_separator_before_cursor.register(case)


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
glib_924_oom_array = contract_glib_arrays.glib_924_oom_array


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


contract_facade_totals_counts.register(case, items_system=items_system, _craft_db=_craft_db)


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


# The sentence-tie fixture regeneration lives in contract_sentence_ties (#1175).
declared_sentence_residuals = contract_sentence_ties.declared_sentence_residuals
encode_sentence_tie = contract_sentence_ties.encode_sentence_tie


def regenerate_sentence_ties(prefix, fixture):
    """Regenerate the sentence-tie fixture with the driver's Kit (contract_sentence_ties)."""
    return contract_sentence_ties.regenerate_sentence_ties(prefix, fixture, Kit=Kit)


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


def policy_diagnosis(spec, pin, subject):
    """Which side and field moved against the case's record, or None (contract_verdict)."""
    return contract_verdict.policy_diagnosis(spec, pin, subject, policy_root=POLICY_ROOT, cell=CELL)


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
    parser.add_argument('--cases-prefix', metavar='P[,P...]',
                        help='union of ID prefixes from the full registry, including untagged cases; '
                             'every prefix must match')
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
    try:
        contract_selection.prefix_matches(CASES, args.cases_prefix)
    except SelectionError as error:
        parser.error(str(error))
    if args.cases_prefix is not None and (args.teardown or args.per_call or args.axis_c_replacement or
            args.axis_c_mutation or args.axis_c_snapshot_check or args.regenerate_sentence_ties):
        parser.error('--cases-prefix selects ordinary cases only')
    if not 0 < args.case_timeout < float('inf'):
        parser.error('--case-timeout must be a positive finite number of seconds')
    if args.jobs not in (None, 1) and (args.teardown or args.per_call or args.axis_c_replacement or
                           args.axis_c_mutation or args.axis_c_snapshot_check or args.regenerate_sentence_ties):
        parser.error('--jobs applies to ordinary contract cases only')
    if args.list_tags:
        print(json.dumps(dict(tags=tag_index(), modes={name: spec['mode'] for name, spec in CASES.items()}), indent=1))
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
        names, reasons = select_cases(args.cases, args.issue, args.family, args.cases_prefix)
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
                        capture.write(json.dumps(dict(cell=args.cell, case=n, userdir=spec['userdir'],
                                                      abort=spec['abort'] is not NO_ABORT,
                                                      policy_record=spec['policy_record'], pin=pin, subject=subject),
                                                 ensure_ascii=True) + '\n')
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
                    if verdict == 'DIFFER':
                        diagnosis = policy_diagnosis(spec, pin, subject)
                        if diagnosis is not None:
                            row['policy'] = diagnosis
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
                        if 'policy' in row:
                            print('  policy %s: %s' % (spec['policy_record'], row['policy']['summary']))
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


contract_user_key_spellings.register(case)


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--per-call-worker':
        per_call_worker(*sys.argv[2:])
    elif len(sys.argv) > 1 and sys.argv[1] == '--worker':
        # The uncached entry; the driver starts its workers through WORKER.
        worker(*sys.argv[2:])
    else:
        sys.exit(main())
