"""Comparison and verdict rules of the contract differential (contract-diff.py).

From one case's registered spec and its pin and subject observations, decide
MATCH, DIFFER, INVALID or POLICY-ACCEPTED: the exit and stderr conditions of
the class (b) and (c) sites, the `~` fields left out of the comparison, the
discrepancy-record digests (contract_policy), the host-dependent table.conf
refusal and the #1114 saved-bigram mask. What the rules read from the driver's per-run state, the
findings directory, the DBM cell and the directory-size probe, is a keyword
argument: contract-diff.py passes its own globals at call time, which is where
its tests patch them. The driver re-exports every name.
"""
import ctypes as C
import functools
import json
import os
from pathlib import Path
import re
import signal
import tempfile

import contract_policy


NO_ABORT = object()


# Each facade logs under its own library's domain.
WARNING_DOMAIN = {'pinyin': 'libpinyin', 'zhuyin': 'libzhuyin'}


def observed(result):
    """Return a shallow copy omitting top-level keys that start with a tilde."""
    return {k: v for k, v in result.items() if not k.startswith('~')}


def registered_termination(spec):
    """Whether the spec says how the pin dies (`abort=`, `crash=`,
    `both_abort=`, with or without `either=`), or is deliberately not run.
    In default mode `matches` alone owns these cases' exit conditions."""
    return spec['abort'] is not NO_ABORT or spec['crash'] is not None or spec['both_abort'] \
        or spec.get('pin_not_run') is not None


def valid_observation(spec, side):
    """Default-mode validity: an ordinary case needs a normal worker exit
    and a result document; a signal, a non-zero exit, a timeout or an empty
    stdout observed nothing."""
    return registered_termination(spec) or (side['exit'] == 0 and side['result'] is not None)


def valid_parent_observations(spec, pin, subject, *, directory_probe):
    """A parent difference needs a pin satisfying its termination contract.

    The subject must complete with a result, or terminate as registered for
    the pin. A fatal-log case requires the pin's same signal. Keep the
    deliberate pin_not_run declaration's existing classification.
    """
    if spec.get('pin_not_run') is not None:
        return True
    if not registered_termination(spec):
        return all(valid_observation(spec, side) for side in (pin, subject))
    if spec['both_abort']:
        signals = (-signal.SIGABRT, -signal.SIGTRAP)
    elif short_dir_chunk(spec, directory_probe=directory_probe):
        signals = (-signal.SIGSEGV,) if spec['dir_chunk'] == 'system' else ()
        if not signals:
            return all(side['exit'] == 0 and side['result'] is not None for side in (pin, subject))
    else:
        signals = ((-signal.SIGABRT,) if spec['abort'] is not NO_ABORT else ()) + \
            ((-signal.SIGSEGV,) if spec['crash'] is not None
             and (spec['abort'] is NO_ABORT or spec['either']) else ())
    return pin['exit'] in signals and (
        (subject['exit'] == 0 and subject['result'] is not None)
        or subject['exit'] in ((pin['exit'],) if spec['both_abort'] else signals))


def policy_expectations(spec, *, policy_root, cell):
    """Read the current registered record, never a cached exemption.

    Each case keeps, per cell or once for every cell, the SHA-256 of each
    side's complete stable observation, a short digest per top-level field
    and the pin-to-subject divergence as text (contract_policy). The record
    holds no observation and allows no wildcard, ignored result field or
    partial-dict match: the digests decide. Rejected metadata is INVALID,
    including under --expect-parent. A schema-1 record, which held the
    complete observations, is refused by name.

    The record is read from policy_root. cell is the DBM cell whose entry
    a per-cell record supplies when CONTRACT_DIFF_CELL is unset.
    """
    name = spec['policy_record']
    if not isinstance(name, str) or Path(name).name != name or not name.endswith('.md'):
        raise ValueError('policy_record must name a findings Markdown record')
    path = policy_root / name
    if path.resolve().parent != policy_root.resolve():
        raise ValueError('policy record escapes findings directory')
    register = (policy_root / 'upstream-divergences.md').read_text()
    if f']({name})' not in register:
        raise ValueError('policy record is not registered')
    blocks = re.findall(r'^```contract-diff-policy\n(.*?)\n```$', path.read_text(), re.M | re.S)
    if len(blocks) != 1:
        raise ValueError('policy record needs exactly one observations block')
    record = json.loads(blocks[0])
    if isinstance(record, dict) and record.get('schema') == 1:
        raise ValueError('policy record holds complete observations (schema 1); '
                         'record its cases again with just policy-record')
    if (not isinstance(record, dict) or type(record.get('schema')) is not int
            or record.get('schema') != contract_policy.SCHEMA
            or record.get('record') != path.stem or record.get('status') != 'current'
            or record.get('disposition') != 'POLICY-ACCEPTED'):
        raise ValueError('policy record is not current or has incompatible metadata')
    kept = record['cases'][spec['case_id']]
    if isinstance(kept, dict) and set(kept) == {'cells'}:
        kept = kept['cells'][os.environ.get('CONTRACT_DIFF_CELL', cell)]
    return contract_policy.check_entry(kept)


def policy_observation(spec, side):
    """The complete stable observation, excluding worker timings only."""
    return contract_policy.stable_observation(side, spec['userdir'])


def policy_diagnosis(spec, pin, subject, *, policy_root, cell):
    """Which side and which field moved against the record, from its field
    digests: per side `matches`, `moved`, `added` and `missing` fields and a
    one-line `summary`. None when the record cannot be read (the verdict is
    INVALID) or when both sides digest to the record (POLICY-ACCEPTED)."""
    try:
        kept = policy_expectations(spec, policy_root=policy_root, cell=cell)
        actual = dict(pin=policy_observation(spec, pin), subject=policy_observation(spec, subject))
    except (OSError, ValueError, KeyError, TypeError):
        return None
    if contract_policy.accepted(kept, actual):
        return None
    report = contract_policy.diagnose(kept, actual)
    return dict(report, summary=contract_policy.describe(report))


def verdict_for(spec, pin, subject, expect_parent, *, policy_root, cell, directory_probe):
    """The driver's decision for one case: (verdict, expected_ok, same).

    INVALID when either side of an ordinary case produced no valid
    observation; it is never expected, in either mode. Under --expect-parent,
    registered terminations also require valid_parent_observations. A `strjoinv_over_read`
    case's held pin fault (`strjoinv_fault`) counts as valid. Otherwise MATCH or
    DIFFER by `matches`, and expected when it is what the mode asks for: a
    MATCH, or under --expect-parent a DIFFER from a non-control case.

    A policy case is POLICY-ACCEPTED only when both complete stable
    observations digest to the record's SHA-256 values and could have been
    recorded (a normal termination with a complete document, or the pin of a
    registered abort site); any other pair of valid observations is DIFFER,
    and `policy_diagnosis` says which side and field moved.

    policy_root and cell locate the discrepancy record (policy_expectations) and
    directory_probe is the host's directory-size probe (short_dir_chunk)."""
    if spec['policy_record'] is not None:
        try:
            kept = policy_expectations(spec, policy_root=policy_root, cell=cell)
            actual = dict(pin=policy_observation(spec, pin), subject=policy_observation(spec, subject))
        except (OSError, ValueError, KeyError, TypeError):
            return 'INVALID', False, False
        valid = (valid_parent_observations(spec, pin, subject, directory_probe=directory_probe) if expect_parent
                 else all(valid_observation(spec, side) for side in (pin, subject)))
        if not valid:
            return 'INVALID', False, False
        if contract_policy.accepted(kept, actual):
            if any(contract_policy.observation_problem(
                    actual[side], pin_abort=side == 'pin' and spec['abort'] is not NO_ABORT)
                   for side in contract_policy.SIDES):
                return 'INVALID', False, False
            # An existing accepted quirk is a control on its parent too.
            return 'POLICY-ACCEPTED', True, False
        # A bad record observation is never an expected parent difference.
        return 'DIFFER', False, False
    same = matches(spec, pin, subject, directory_probe=directory_probe)
    held_fault = spec['strjoinv_over_read'] and strjoinv_fault(pin, subject)
    valid = (valid_parent_observations(spec, pin, subject, directory_probe=directory_probe) if expect_parent
             else all(valid_observation(spec, side) for side in (pin, subject)))
    if not held_fault and not valid:
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


def short_dir_chunk(spec, *, directory_probe):
    """A `dir_chunk` case on a host where the pin does not reach its assert.

    directory_probe is `directory_reads_as_a_chunk`; it is called only for a
    case that registers a `dir_chunk`."""
    return spec['dir_chunk'] is not None and not directory_probe()


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


def matches(spec, pin, subject, *, directory_probe):
    """The verdict predicate shared by the driver and gcov measurement tier.

    directory_probe is the host's directory-size probe (short_dir_chunk)."""
    if spec['policy_record'] is not None:
        return False  # accepted differences must never enter MATCH totals
    short_dir = short_dir_chunk(spec, directory_probe=directory_probe)
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
    elif spec.get('pin_not_run') is not None:
        shown = subject['result'] or {}
        same = pin['exit'] == 'NOT-RUN' and subject['exit'] == 0 and \
            all(shown.get(key) == value for key, value in spec['pin_not_run'].items())
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


_FATAL_GLIB_PREFIX = re.compile(
    r'\*\* \(process:\d+\): (CRITICAL|WARNING|MESSAGE|ERROR|DEBUG|INFO) \*\*: '
    r'\d\d:\d\d:\d\d\.\d+: ')


def fatal_stderr(text):
    """Drop GLib's process id and timestamp from a fatal run's stderr: they
    differ between the two processes, while the severity and the message are
    the diagnostics under comparison."""
    return _FATAL_GLIB_PREFIX.sub(r'** \1 **: ', text)


# tkrzw 9ee8416, tkrzw_dbm_hash.cc:46,1296: timestamp_ is written as an
# eight-byte microsecond wall clock. The maintainer's #1114 ruling permits
# only this comparison mask. Offset 56 starts db_type_ and stays exact.
META_OFFSET_TIMESTAMP = 48
TKRZW_TIMESTAMP_WIDTH = 8
TKRZW_HASH_METADATA_SIZE = 128


def tkrzw_saved_bigram_bytes(raw):
    """Saved HashDBM bytes with only META_OFFSET_TIMESTAMP (48-55) masked.

    This is specific to #1114's user-bigram comparison; it changes no
    existing contract's file comparison or other backend's rules.

    Require at least the 128-byte metadata header or raise ValueError.
    Zero the eight timestamp bytes; preserve all other bytes, including byte 56."""
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
