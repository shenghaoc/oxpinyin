#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Capture the pin's proposed observations into a staging directory.

    pin_capture.py capture RUN --prefix PREFIX --data DATA --provision-cache CACHE --model-dir MODEL
                           [--staging DIR]

`capture` runs in the capture image (`just capture-pin`). Before any observation it
refuses the run unless:
- `tools.oracle.image.prefix_ok` accepts PREFIX, the image's tkrzw cell;
- the libpinyin.so and libzhuyin.so it loads have the prefix's recorded `shared_object_sha256`
  and `zhuyin_shared_object_sha256`;
- the data's digest is `content_sha256` in tools/oracle/libpinyin-data.json;
- MODEL is a complete model20 extraction whose parent's `verified` marker records the pinned
  archive digest (tools/dev/w3_fixtures.py model_dir);
- for each cell a utility run uses, `prefix_ok` accepts CACHE/oracle/CELL (a `tools.dev provision
  oracle --cache-dir CACHE` prefix), its build tree CACHE/data-CELL/src/libpinyin-COMMIT holds the
  pin commit, the library that tree built has the prefix's recorded `shared_object_sha256`, and
  each utility it runs is a built executable there. The utilities link the pin's static archives,
  not the prefix's library, so the tree that built the verified library is what ties them to it.

There are three observation kinds:
- C API calls on one facade, pinyin or zhuyin, each in a fresh worker process with a fresh user
  directory. An observation that ends in the pin's abort keeps the results of the calls before it.
- The data sequence (C1): the pin's gen_binary_files, import_interpolation and gen_unigram over the
  W3 subset of a verified model20 extraction (--model-dir), staging each program's end and streams
  and the rows of bigram.db, phrase_index.bin and pinyin_index.bin.
- A utility run: one pin utility in a private copy of fixtures/w3/<cell>/, with a given stdin.
  It stages the end, stdout, stderr and the resulting tree: every path with its kind, mode and
  size, a file's SHA-256 and whether it is unchanged, and the sorted rows of each new or changed
  DBM file with their digest, as the frozen utility cases of crates/oxpinyin-utils record them.

The run writes DIR/RUN/observations.json and DIR/RUN/capture.json (DIR defaults to
target/pin-observations). capture.json is provenance, not frozen values.

These are proposed observations. Nothing here writes outside the staging directory or reads
back into a test: a person reviews two runs and writes the values into the Rust tests.
docs/runbooks/pin-captures.md is the procedure. Standard library only.
"""
import argparse
import ctypes as C
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools/bisection'))
sys.path.insert(0, str(ROOT))
import contract_fixtures  # noqa: E402
from tools.dev import contract_data  # noqa: E402
from tools.dev import w3_fixtures  # noqa: E402
from tools.oracle import image  # noqa: E402

SCHEMA = 'pin-observations-v2'
BACKEND = 'tkrzw'
DEFAULT_STAGING = ROOT / 'target/pin-observations'
PLATFORMS = {'x86_64': 'linux/amd64', 'aarch64': 'linux/arm64'}
# facade -> (library under the prefix, the prefix record's key for its SHA-256)
LIBRARIES = {'pinyin': ('lib/libpinyin.so', 'shared_object_sha256'),
             'zhuyin': ('lib/libzhuyin.so', 'zhuyin_shared_object_sha256')}
W3_DIRECTORIES = {'bdb': 'db', 'kc': 'kct', 'tkrzw': 'tkt'}
UNTOUCHED = 0xABCDEF  # out-parameter sentinel
UNTOUCHED_U16 = 0xCDEF  # UNTOUCHED & 0xffff
UNTOUCHED_U8 = 0xEF
WORKER_TIMEOUT = 120  # seconds for one observation's worker, as the Rust death tests allow a child
LC_NUMERIC = 1  # glibc's <locale.h>; the capture image is Linux
DEFAULT_SORT = 0x1e  # SORT_WITHOUT_LONGER_CANDIDATE|BY_PHRASE_LENGTH|BY_PINYIN_LENGTH|BY_FREQUENCY, pinyin.h:54-65
PARITY_OPTIONS = 0x18a  # IS_PINYIN|PINYIN_INCOMPLETE|USE_DIVIDED_TABLE|USE_RESPLIT_TABLE, pinyin_custom2.h:32-40
NBEST_MATCH, NORMAL = 1, 2  # lookup_candidate_type_t, pinyin.h:44-45
INT_MIN, INT_MAX = -2 ** 31, 2 ** 31 - 1


class CaptureError(Exception):
    """A capture cannot proceed; the message says why."""


@dataclass(frozen=True)
class Api:
    """C API calls: FUNCTION(facade, INPUT) on a fresh context of FACADE."""
    function: object
    input: object
    facade: str = 'pinyin'


@dataclass(frozen=True)
class Utility:
    """PROGRAM (under utils/SUBDIR of the cell's build tree) run with ARGUMENTS and STDIN in a
    private copy of fixtures/w3/<CELL>/. GRAMS names a DBM file and the tokens whose SingleGram
    rows are decoded beside its raw rows; an absent row is recorded as absent."""
    cell: str
    subdir: str
    program: str
    arguments: tuple
    stdin: bytes
    grams: tuple = ()

    def programs(self):
        return ((self.subdir, self.program),)


# The pin's data sequence (data/Makefile.am:60-62): each program and its directory under utils/.
SEQUENCE_PROGRAMS = (('storage', 'gen_binary_files'), ('storage', 'import_interpolation'), ('training', 'gen_unigram'))


@dataclass(frozen=True)
class DataSequence:
    """C1: the pin's gen_binary_files --gen-punct-table, import_interpolation and gen_unigram, in that
    order, over the W3 subset of the model20 text, in a private work directory, on CELL. It stages
    each program's end, stdout and stderr, and the rows of each of OUTPUTS, never their bytes.
    tools/dev/w3_fixtures.py builds the subset (subset), the build table.conf (table_conf) and the
    program arguments (sequence); nothing here copies them."""
    cell: str
    outputs: tuple

    def programs(self):
        return SEQUENCE_PROGRAMS


def family(name):
    return name.split('/', 1)[0]


# ── the C API observations ───────────────────────────────────────────────

def parse(facade, text):
    """Parse TEXT as full pinyin; answer the parsed length."""
    facade.call('pinyin_parse_more_full_pinyins', text)
    return facade.call('pinyin_get_parsed_input_length')


def key_abort(facade, text):
    """F-E-14: a key query at offset 0 of an apostrophe-only parse."""
    parse(facade, text)
    facade.call('pinyin_get_pinyin_key', 0)


def key_walk(facade, text):
    """The key, key rest, positions and pinyin string at every offset up to the parsed length.

    The pin dereferences the key rest and the key without a null check, so positions and string
    are asked only of a non-null pointer."""
    for offset in range(parse(facade, text) + 1):
        key = facade.call('pinyin_get_pinyin_key', offset)
        rest = facade.call('pinyin_get_pinyin_key_rest', offset)
        if rest[1]:
            facade.call('pinyin_get_pinyin_key_rest_positions')
        if key[1] is not None:
            facade.call('pinyin_get_pinyin_string')


def zhuyin_key_abort(facade, text):
    """The zhuyin twin of key_abort: libzhuyin's key query reads the column at the offset itself
    (contract_separator_before_cursor.py:256-259), so the pinyin values do not carry over."""
    facade.call('zhuyin_parse_more_full_pinyins', text)
    facade.call('zhuyin_get_parsed_input_length')
    facade.call('zhuyin_get_zhuyin_key', 0)


def zhuyin_key_walk(facade, text):
    """The zhuyin twin of key_walk, with the zhuyin string of each non-null key."""
    facade.call('zhuyin_parse_more_full_pinyins', text)
    for offset in range(facade.call('zhuyin_get_parsed_input_length') + 1):
        key = facade.call('zhuyin_get_zhuyin_key', offset)
        rest = facade.call('zhuyin_get_zhuyin_key_rest', offset)
        if rest[1]:
            facade.call('zhuyin_get_zhuyin_key_rest_positions')
        if key[1] is not None:
            facade.call('zhuyin_get_zhuyin_string')


def import_rows(facade, library, rows, prefix='pinyin'):
    facade.call(f'{prefix}_begin_add_phrases', library)
    for phrase, reading, count in rows:
        facade.call(f'{prefix}_iterator_add_phrase', phrase, reading, count)
    facade.call(f'{prefix}_end_add_phrases')


def export_rows(facade, index):
    """Every row of library INDEX's export, through the pin's own has_next/get_next walk."""
    facade.call('pinyin_begin_get_phrases', index)
    while facade.call('pinyin_iterator_has_next_phrase'):
        facade.call('pinyin_iterator_get_next_phrase')
    facade.call('pinyin_end_get_phrases')


def read_row(facade, index, prefix='pinyin'):
    """Row INDEX: type and string, and for a pinyin NBEST row its n-best index (the pin asserts
    the type at pinyin.cpp:2883). Answer (type, text, n-best index or None), or None when there is
    no row."""
    if facade.call(f'{prefix}_get_candidate', index)[1] != 'row':
        return None
    kind = facade.call(f'{prefix}_get_candidate_type')[1]
    text = facade.call(f'{prefix}_get_candidate_string')[1]
    nbest = None
    if prefix == 'pinyin' and kind == NBEST_MATCH:
        nbest = facade.call('pinyin_get_candidate_nbest_index')[1]
    return kind, text, nbest


def nbest_indexes(facade):
    """The n-best indexes the NBEST rows of the current list carry, ascending. get_sentence is asked
    only of these: an index past the results is pinyin.cpp:1474's assert."""
    count = facade.call('pinyin_get_n_candidate')[1]
    found = (read_row(facade, index) for index in range(count))
    return sorted({row[2] for row in found if row and row[0] == NBEST_MATCH})


def rows(facade, limit=None, prefix='pinyin'):
    """The list's count, then rows 0..min(count, LIMIT); answer their (type, text)."""
    count = facade.call(f'{prefix}_get_n_candidate')[1]
    return [read_row(facade, index, prefix) for index in range(count if limit is None else min(count, limit))]


def choose_text(facade, offset, text, normal_only=False):
    """Choose the first row whose string is TEXT (of type NORMAL with NORMAL_ONLY) at OFFSET; answer
    the cursor, or None when no such row is offered (nothing is chosen then)."""
    count = facade.call('pinyin_get_n_candidate')[1]
    for index in range(count):
        row = read_row(facade, index)
        if row and row[1] == text and (not normal_only or row[0] == NORMAL):
            return facade.call('pinyin_choose_candidate', offset)
    return None


# S1: exporting libraries 0 and 15, the guint8 truncation (pinyin.cpp:126, :666) and an unloaded slot.

def export_alias(facade, index):
    import_rows(facade, 7, [('你好世界', "ni'hao'shi'jie", 5)])
    export_rows(facade, 7)
    export_rows(facade, index)


def export_unloaded(facade, index):
    facade.call('pinyin_unload_phrase_library', index)
    export_rows(facade, index)


# S2: an other-library add on its own, then an accumulation across the compaction at pinyin.cpp:657.

def rebatch(facade, rows_):
    import_rows(facade, 7, rows_[:1])
    export_rows(facade, 7)
    import_rows(facade, 7, rows_[1:])
    export_rows(facade, 7)
    facade.call('pinyin_save')
    facade.call('reopen')
    export_rows(facade, 7)


# S3 and D1: import counts and an import into a slot with no sub-index.

def import_count(facade, rows_):
    import_rows(facade, 7, rows_)
    export_rows(facade, 7)
    facade.call('pinyin_save')
    facade.call('reopen')
    export_rows(facade, 7)


def import_library(facade, library):
    import_rows(facade, library, [('你好', 'nihao', 1)])
    export_rows(facade, 7)


# S4: deep paging, the tenth row, the last row and one past the end.

def candidate_window(facade, text):
    """Options 0x18a, parse TEXT, decode, list at offset 0; answer the row count."""
    facade.call('pinyin_set_options', PARITY_OPTIONS)
    parse(facade, text)
    facade.call('pinyin_guess_sentence')
    facade.call('pinyin_guess_candidates', 0, DEFAULT_SORT)
    return facade.call('pinyin_get_n_candidate')[1]


def choose_row(facade, index):
    """Choose INDEX at offset 0, then re-decode: the sentence, the window at the cursor (rows
    0..7) and sentence 0."""
    if read_row(facade, index) is None:
        return
    cursor = facade.call('pinyin_choose_candidate', 0)
    guessed = facade.call('pinyin_guess_sentence')
    if cursor >= 0:
        facade.call('pinyin_guess_candidates', cursor, DEFAULT_SORT)
        rows(facade, 8)
    if guessed:
        facade.call('pinyin_get_sentence', 0)


def choose_tenth(facade, text):
    candidate_window(facade, text)
    choose_row(facade, 10)


def choose_last(facade, text):
    choose_row(facade, candidate_window(facade, text) - 1)


def row_past_end(facade, text):
    facade.call('pinyin_get_candidate', candidate_window(facade, text))


# S5: one option bit on or off for its parser-table row.

def option_walk(facade, case):
    """Set WORD (after alloc; `pinyin_set_options` stores it, `pinyin.cpp:1299-1307` at 074a2219), walk
    TEXT's keys and read the full-pinyin auxiliary text at the parsed length (`:3381-3438`)."""
    word, text = case
    facade.call('pinyin_set_options', word)
    key_walk(facade, text)
    facade.call('pinyin_get_full_pinyin_auxiliary_text', facade.call('pinyin_get_parsed_input_length'))


# S6: zhuyin's noguess protocol, at the zhuyin defaults (USE_TONE|FORCE_TONE, STANDARD).

def zhuyin_before(facade, case):
    """Parse TEXT; with GUESS, guess the sentence first; then list before OFFSET."""
    text, offset, guess = case
    facade.call('zhuyin_parse_more_chewings', text)
    facade.call('zhuyin_get_parsed_input_length')
    if guess:
        facade.call('zhuyin_guess_sentence')
    facade.call('zhuyin_guess_candidates_before_cursor', offset)
    rows(facade, prefix='zhuyin')


# S7: luoma and secondary zhuyin keys after a scheme parse, and the word moves.

def scheme_key(scheme):
    def observe(facade, text):
        facade.call('pinyin_set_options', PARITY_OPTIONS)
        facade.call('pinyin_set_full_pinyin_scheme', scheme)
        parse(facade, text)
        facade.call('pinyin_guess_sentence')
        if facade.call('pinyin_get_pinyin_key', 0)[1] is not None:
            facade.call('pinyin_get_luoma_pinyin_string')
            facade.call('pinyin_get_secondary_zhuyin_string')
    observe.__name__ = f'scheme_key_{scheme}'
    return observe


def word_moves(facade, text):
    """`pinyin.cpp:3010-3096` at 074a2219: the pinyin offset of the cursor, then the left and right word
    moves from it, cursors 0, 2, 5, 8."""
    facade.call('pinyin_set_options', PARITY_OPTIONS)
    parse(facade, text)
    for cursor in (0, 2, 5, 8):
        offset = facade.call('pinyin_get_pinyin_offset', cursor)[1]
        facade.call('pinyin_get_left_pinyin_offset', offset)
        facade.call('pinyin_get_right_pinyin_offset', offset)


# D3: the pronunciation possibility of an item with an unindexed second reading.

def dilution(facade, text):
    def sentence():
        parse(facade, text)
        facade.call('pinyin_guess_sentence')
        facade.call('pinyin_get_sentence', 0)
        facade.call('pinyin_reset')
    import_rows(facade, 1, [('测侧', "ce'ce", 1000000), ('侧测', "ce'ce", 600000)])
    sentence()
    import_rows(facade, 1, [('测侧', "xi'xi", 9000000)])
    sentence()


# D4: n-best results that survive a keyed reparse.

def stale_nbest(facade, case):
    first, second = case
    parse(facade, first)
    facade.call('pinyin_guess_sentence')
    facade.call('pinyin_get_sentence', 0)
    parse(facade, second)
    facade.call('pinyin_get_sentence', 0)
    facade.call('pinyin_guess_candidates', 0, DEFAULT_SORT)
    count = facade.call('pinyin_get_n_candidate')[1]
    if count:
        read_row(facade, 0)
    facade.call('pinyin_guess_sentence')
    facade.call('pinyin_get_sentence', 0)


# D5: the previous token at an offset past 0 under DYNAMIC_ADJUST.

def dynamic(facade, case):
    """With WORD: parse TEXT and decode; CHOOSE a NORMAL row of that text at 0 first when given,
    else list at OFFSET."""
    word, text, offset, choose = case
    facade.call('pinyin_set_options', word)
    parse(facade, text)
    facade.call('pinyin_guess_sentence')
    if choose:
        facade.call('pinyin_guess_candidates', 0, DEFAULT_SORT)
        offset = choose_text(facade, 0, choose, normal_only=True)
        if offset is None:
            return
        facade.call('pinyin_guess_sentence')
    facade.call('pinyin_guess_candidates', offset, DEFAULT_SORT)
    rows(facade)


# D8: the n-best training sequence over `pinyin.cpp:1372-1387`, `:2184-2308`, `:2501-2589` and
# `:2670-2691` at 074a2219.

def nbest_surface(facade):
    """Decode nihao (`pinyin.cpp:1372-1387`, `:2184-2308` at 074a2219), read every row's type and
    n-best index (`:2845-2886`), up to three sentences (`:1464-1482`), the first eight rows; then
    reset (`:2693-2705`)."""
    parse(facade, 'nihao')
    facade.call('pinyin_guess_sentence')
    facade.call('pinyin_guess_candidates', 0, DEFAULT_SORT)
    for index in nbest_indexes(facade)[:3]:
        facade.call('pinyin_get_sentence', index)
    rows(facade, 8)
    facade.call('pinyin_reset')


def train_pair(facade, first, second):
    """Choose FIRST at 0 and SECOND at the cursor, by text, any type (`pinyin.cpp:2501-2589`), then train
    (`:2670-2691`) at 074a2219."""
    parse(facade, 'nihao')
    facade.call('pinyin_guess_sentence')
    facade.call('pinyin_guess_candidates', 0, DEFAULT_SORT)
    cursor = choose_text(facade, 0, first)
    if cursor is None:
        return
    facade.call('pinyin_guess_sentence')
    facade.call('pinyin_guess_candidates', cursor, DEFAULT_SORT)
    if choose_text(facade, cursor, second) is None:
        return
    facade.call('pinyin_guess_sentence')
    facade.call('pinyin_train', 0)
    facade.call('pinyin_reset')


def token_readings(facade, text):
    """TEXT's tokens; each one's pronunciations rendered with pinyin_get_pinyin_string, and its unigram."""
    ok, tokens = facade.call('pinyin_lookup_tokens', text)
    for token in tokens if ok else ():
        count = facade.call('pinyin_token_get_n_pronunciation', token)[1]
        for nth in range(count):
            keys = facade.call('pinyin_token_get_nth_pronunciation', token, nth)[1]
            for index in range(len(keys)):
                facade.call('pinyin_get_pinyin_string', index)
        facade.call('pinyin_token_get_unigram_frequency', token)


def nbest_train(facade, rounds):
    user_rounds, augmented = rounds
    nbest_surface(facade)
    for _ in range(user_rounds):
        train_pair(facade, '你', '浩')
    nbest_surface(facade)
    for _ in range(augmented):
        train_pair(facade, '你', '好')
    nbest_surface(facade)
    export_rows(facade, 7)
    for text in ('你', '浩', '好'):
        token_readings(facade, text)
    facade.call('pinyin_save')
    facade.call('reopen')
    nbest_surface(facade)


# D9 and D10: choosing an n-best row past 0, and the train index boundaries.

def nbest_choose(facade, case):
    """Under 0x18a, choose the NORMAL row FIRST at offset 0, then the first NBEST row whose index is
    past 0 in the window at the cursor; decode, train result 0, and read each word token's unigram.
    The NBEST choose diffs the constraints (pinyin.cpp:2516-2520) and training follows them
    (phonetic_lookup.h:864-870). After 你 the first capture lists NBEST 0 and 2 there, not 1: the
    deduplication drops index 1 (dynamic/chosen-ni/0x18a), so the choose takes the index the window
    offers, and the staged nbest_index call records which."""
    text, first = case
    facade.call('pinyin_set_options', PARITY_OPTIONS)
    parse(facade, text)
    facade.call('pinyin_guess_sentence')
    facade.call('pinyin_guess_candidates', 0, DEFAULT_SORT)
    cursor = choose_text(facade, 0, first, normal_only=True)
    if cursor is None:
        return
    facade.call('pinyin_guess_sentence')
    facade.call('pinyin_guess_candidates', cursor, DEFAULT_SORT)
    count = facade.call('pinyin_get_n_candidate')[1]
    for index in range(count):
        row = read_row(facade, index)
        if row and row[0] == NBEST_MATCH and row[2] > 0:
            facade.call('pinyin_choose_candidate', cursor)
            break
    else:
        return
    facade.call('pinyin_guess_sentence')
    ok, sentence = facade.call('pinyin_get_sentence', 0)
    facade.call('pinyin_train', 0)
    if ok and facade.call('pinyin_phrase_segment', sentence):
        for index in range(facade.call('pinyin_get_n_phrase')[1]):
            ok, token = facade.call('pinyin_get_phrase_token', index)
            if ok:
                facade.call('pinyin_token_get_unigram_frequency', token)


def train_index(facade, case):
    """Decode TEXT and train result INDEX. One observation per index: the first that aborts is
    pinyin.cpp:2684's assert, `index < results.size()`, and marks the result count."""
    text, index = case
    parse(facade, text)
    facade.call('pinyin_guess_sentence')
    facade.call('pinyin_train', index)


# D11-D14: two contexts, file modes, the numeric locale and an empty segment.

def both_learn(facade, rows_):
    first, second = rows_
    facade.call('open_context')
    facade.call('use_context', 0)
    import_rows(facade, 7, [first])
    facade.call('use_context', 1)
    import_rows(facade, 7, [second])
    facade.call('use_context', 0)
    facade.call('pinyin_save')
    facade.call('use_context', 1)
    facade.call('pinyin_save')
    facade.call('open_context')
    export_rows(facade, 7)


def modes_after_chmod(facade, mode):
    import_rows(facade, 7, [('你好', 'nihao', 1)])
    facade.call('pinyin_save')
    facade.call('user_files')
    facade.call('chmod_user_files', mode)
    import_rows(facade, 7, [('世界', 'shijie', 1)])
    facade.call('pinyin_save')
    facade.call('user_files')


def locale_sites(facade, name):
    """LC_NUMERIC after each table_info.cpp site the C API reaches, NAME installed before each."""
    for step in ('pinyin_fini', 'pinyin_init', 'pinyin_save'):
        facade.call('set_numeric_locale', name)
        facade.call(step)
        facade.call('numeric_locale')
    import_rows(facade, 7, [('你好', 'nihao', 1)])
    facade.call('set_numeric_locale', name)
    facade.call('pinyin_save')
    facade.call('numeric_locale')


def segment(facade, text):
    facade.call('pinyin_phrase_segment', text)
    facade.call('pinyin_get_n_phrase')


# D15 and D16: user rows for an incomplete key, and the zhuyin import separators.

# One-character phrases whose readings differ only in the stored key; the candidate order they
# probe is `pinyin.cpp:2184-2308` at 074a2219.
TIE_USER_PHRASES = (('𦰡', 'na'), ('𫐐', 'ni'), ('𫠜', 'ni'), ('𨺙', 'ni'), ('𫔶', 'nie'),
                    ('𬪩', 'nong'), ('𦰡', 'nuo'), ('𫟅', 'nen'), ('𫵷', 'nao'), ('𬇕', 'nu'))


def tie_user(facade, text):
    import_rows(facade, 7, [(phrase, reading, 1) for phrase, reading in TIE_USER_PHRASES])
    facade.call('pinyin_set_options', PARITY_OPTIONS)
    parse(facade, text)
    facade.call('pinyin_guess_sentence')
    facade.call('pinyin_guess_candidates', 0, DEFAULT_SORT)
    rows(facade)


def zhuyin_import(facade, case):
    phrase, reading = case
    import_rows(facade, 7, [(phrase, reading, 5)], prefix='zhuyin')
    ok, tokens = facade.call('zhuyin_lookup_tokens', phrase)
    for token in tokens if ok else ():
        count = facade.call('zhuyin_token_get_n_pronunciation', token)[1]
        for nth in range(count):
            keys = facade.call('zhuyin_token_get_nth_pronunciation', token, nth)[1]
            for index in range(len(keys)):
                facade.call('zhuyin_get_zhuyin_string', index)


_GATED = (('agn', 1 << 21), ('amg', 1 << 22), ('diou', 1 << 23), ('duei', 1 << 24), ('duen', 1 << 25),
          ('lue', 1 << 26), ('jv', 1 << 27), ('zon', 1 << 28), ('sua', 1 << 11), ('zua', 1 << 12))
OPTION_BASE = 0x188  # PINYIN_INCOMPLETE|USE_DIVIDED_TABLE|USE_RESPLIT_TABLE, the base word of the option cases (`pinyin.cpp:1299-1307` at 074a2219)
OPTION_GATES = 0x1FE01800  # PINYIN_CORRECT_ALL|PINYIN_AMB_S_SH|PINYIN_AMB_Z_ZH, pinyin_custom2.h:51-52,79

# Observation ID -> Api(function, input[, facade]) or Utility.
OBSERVATIONS = {
    "key-abort/'": Api(key_abort, "'"),
    "key-abort/''": Api(key_abort, "''"),
    "key-abort/'''": Api(key_abort, "'''"),
    "key-walk/ni'": Api(key_walk, "ni'"),
    "key-walk/ni'!": Api(key_walk, "ni'!"),
    "key-walk/ni'i": Api(key_walk, "ni'i"),
    **{f'zhuyin-key-abort/{text}': Api(zhuyin_key_abort, text, 'zhuyin') for text in ("'", "''", "'''")},
    **{f'zhuyin-key-walk/{text}': Api(zhuyin_key_walk, text, 'zhuyin') for text in ("ni'", "ni'!", "ni'i")},
    # S1
    'export/empty/0': Api(export_rows, 0),
    'export/empty/15': Api(export_rows, 15),
    'export/truncated/256': Api(export_rows, 256),
    'export/truncated/263': Api(export_alias, 263),
    'export/unloaded/2': Api(export_unloaded, 2),
    # S2
    'import/rebatch/中国': Api(rebatch, [['中国', "zhong'guo", 4], ['中国', "zhong'guo", 6]]),
    # S3
    'import-count/0': Api(import_count, [['零次', "ling'ci", 0]]),
    'import-count/-2': Api(import_count, [['负二', "fu'er", -2]]),
    'import-count/INT_MIN': Api(import_count, [['最小', "zui'xiao", INT_MIN]]),
    'import-count/INT_MAX': Api(import_count, [['最大', "zui'da", INT_MAX]]),
    'import-count/INT_MAX+1': Api(import_count, [['最大', "zui'da", INT_MAX], ['最大', "zui'da", 1]]),
    # S4
    'deep-choose/nihao@10': Api(choose_tenth, 'nihao'),
    'deep-choose/shi@10': Api(choose_tenth, 'shi'),
    'last-choose/nihao': Api(choose_last, 'nihao'),
    'last-choose/shi': Api(choose_last, 'shi'),
    'past-end/nihao': Api(row_past_end, 'nihao'),
    # S5
    **{f'option-row/on/{text}': Api(option_walk, [OPTION_BASE | bit, text]) for text, bit in _GATED},
    **{f'option-row/off/{text}': Api(option_walk, [(OPTION_BASE | OPTION_GATES) & ~bit, text]) for text, bit in _GATED},
    'option-row/on/egn': Api(option_walk, [OPTION_BASE | 1 << 21, 'egn']),
    'option-distance/chonga': Api(option_walk, [OPTION_BASE | 1 << 28, 'chonga']),
    # S6
    **{f'zhuyin-noguess-before/su3cl3@{offset}': Api(zhuyin_before, ['su3cl3', offset, False], 'zhuyin')
       for offset in range(7)},
    'zhuyin-noguess-before/su@0': Api(zhuyin_before, ['su', 0, False], 'zhuyin'),
    **{f'zhuyin-guess-before/su3cl3@{offset}': Api(zhuyin_before, ['su3cl3', offset, True], 'zhuyin')
       for offset in (0, 3, 6)},
    # S7
    'scheme-key/luoma/nihao': Api(scheme_key(2), 'nihao'),
    'scheme-key/secondary-zhuyin/nihao': Api(scheme_key(3), 'nihao'),
    'word-moves/nihaoshijie': Api(word_moves, 'nihaoshijie'),
    # D1
    'import/library/0': Api(import_library, 0),
    'import/library/15': Api(import_library, 15),
    # D3
    'import/dilution/cece': Api(dilution, 'cece'),
    # D4
    'assembly/stale-nbest/nihao-zhongguo': Api(stale_nbest, ['nihao', 'zhongguo']),
    # D5
    # deyi, choosing 的: the pinned bigram's 的 row reorders the window at the cursor under
    # DYNAMIC_ADJUST (checked on oxpinyin over the pinned data before the capture).
    **{f'dynamic/deyi-chosen-de/{word:#x}': Api(dynamic, [word, 'deyi', None, '的']) for word in (0x38a, 0x18a)},
    # D8
    'nbest-train/ordinary': Api(nbest_train, [3, 3]),
    # D9, D10
    'nbest-choose/nihao/past-0': Api(nbest_choose, ['nihao', '你']),
    **{f'train-index/nihao/{index}': Api(train_index, ['nihao', index]) for index in (1, 2, 3)},
    # D11-D14
    'two-context/both-learn': Api(both_learn, [['甲甲', "jia'jia", 1], ['乙乙', "yi'yi", 1]]),
    'file-modes/dbm-after-chmod': Api(modes_after_chmod, 0o640),
    'locale/sites': Api(locale_sites, 'C.UTF-8'),
    'segment/empty': Api(segment, ''),
    # D15, D16
    'tie/user-n': Api(tie_user, 'n'),
    'zhuyin-import/run': Api(zhuyin_import, ['你好', "ㄋㄧˇ' ㄏㄠˇ "], 'zhuyin'),
    'zhuyin-import/leading': Api(zhuyin_import, ['你好', ' ㄋㄧˇ ㄏㄠˇ'], 'zhuyin'),
    'zhuyin-import/initial': Api(zhuyin_import, ['你', 'ㄋ'], 'zhuyin'),
}

# docs/runbooks/goldens-and-pins.md, "The next pin capture": the utility runs no frozen case holds.
KMM_UNIGRAM_ONLY = (b'\\data model "k mixture model" count 3 N 1 total_freq 3\n\\1-gram\n'
                    b'\\item 1 <start> count 2 freq 0\n\\item 16817937 \xe4\xb8\xad\xe5\x9b\xbd count 1 freq 1\n'
                    b'\\item 16802309 \xe4\xbd\xa0\xe5\xa5\xbd count 0 freq 2\n\\end\n')
SEGMENTED = '16777219 阿\n16777220 啊\n\n16777219 阿\n'.encode()
TRAINING_TOKENS = (1, 16777219, 16777220)  # START, 阿, 啊
for _cell in W3_DIRECTORIES:
    OBSERVATIONS[f'utility/{_cell}/kmm-import-unigram'] = Utility(
        _cell, 'training', 'import_k_mixture_model', (), KMM_UNIGRAM_ONLY)
    OBSERVATIONS[f'utility/{_cell}/gen-ngram'] = Utility(
        _cell, 'training', 'gen_ngram', (), SEGMENTED, ('bigram.db', TRAINING_TOKENS))
    OBSERVATIONS[f'utility/{_cell}/gen-deleted-ngram'] = Utility(
        _cell, 'training', 'gen_deleted_ngram', ('--deleted-bigram-file', 'deleted.db'), SEGMENTED,
        ('deleted.db', TRAINING_TOKENS))
# C1, the ignored-tests audit: the pin's data sequence on the bdb cell, for the pin-written Berkeley DB
# evidence of fixtures/w3/db/'s bigram.db and index trees (oxpinyin-utils wrote the committed files).
OBSERVATIONS['utility/bdb/data-sequence'] = DataSequence('bdb', ('bigram.db', 'phrase_index.bin', 'pinyin_index.bin'))


# ── the calls on the pin ──────────────────────────────────────────────────

P, U8, U16, U32, I, B, S, Z = C.c_void_p, C.c_uint8, C.c_uint16, C.c_uint32, C.c_int, C.c_bool, C.c_char_p, C.c_size_t


NOT_A_STEP = object()


class GArray(C.Structure):
    _fields_ = [('data', P), ('len', C.c_uint)]


class Pin:
    """Contexts and instances on one facade of the pin. Each call prints its name before it runs
    and its result after, so the parent sees which call an abort ended. Besides the C functions
    there are a few named steps: reopen, open_context, use_context, user_conf, user_files,
    chmod_user_files, numeric_locale and set_numeric_locale."""

    def __init__(self, library, data, user, facade='pinyin'):
        self.lib = C.CDLL(str(library))
        self.lib.g_free.argtypes, self.lib.g_free.restype = [P], None
        self.lib.g_array_new.argtypes, self.lib.g_array_new.restype = [C.c_int, C.c_int, C.c_uint], C.POINTER(GArray)
        self.lib.g_array_free.argtypes, self.lib.g_array_free.restype = [C.POINTER(GArray), C.c_int], P
        self.libc = C.CDLL(None)
        self.libc.setlocale.argtypes, self.libc.setlocale.restype = [I, S], S
        self.facade, self.data, self.user = facade, os.fsencode(data), os.fsencode(user)
        self.key, self.rest, self.candidate, self.importer, self.exporter = P(), P(), P(), P(), P()
        self.keys = (U16 * 0)()
        self.contexts = []
        self.open()

    def open(self):
        context = self.fn(f'{self.facade}_init', P, S, S)(self.data, self.user)
        if not context:
            raise CaptureError(f'{self.facade}_init failed')
        instance = self.fn(f'{self.facade}_alloc_instance', P, P)(context)
        if not instance:
            raise CaptureError(f'{self.facade}_alloc_instance failed')
        self.contexts.append([context, instance])
        self.current = len(self.contexts) - 1

    @property
    def context(self):
        return self.contexts[self.current][0]

    @property
    def instance(self):
        return self.contexts[self.current][1]

    def fn(self, name, result, *args):
        function = getattr(self.lib, name)
        function.restype, function.argtypes = result, list(args)
        return function

    def call(self, name, *args):
        print(json.dumps({'call': [name, list(args)]}, ensure_ascii=False), flush=True)
        result = self.run(name, *args)
        print(json.dumps({'result': result}, ensure_ascii=False), flush=True)
        return result

    def text(self, pointer, free=True):
        """A caller-owned (or, with FREE false, borrowed) UTF-8 string at POINTER, or None."""
        if not pointer:
            return None
        value = C.string_at(pointer).decode()
        if free:
            self.lib.g_free(pointer)
        return value

    def tokens(self, name, text):
        array = self.lib.g_array_new(0, 1, 4)
        try:
            ok = bool(self.fn(name, B, P, S, C.POINTER(GArray))(self.instance, text.encode(), array))
            values = C.cast(array.contents.data, C.POINTER(U32))
            return [ok, [values[index] for index in range(array.contents.len)]]
        finally:
            self.lib.g_array_free(array, 1)

    def step(self, name, *args):
        """The named steps that are not one C function; NOT_A_STEP for a C function."""
        if name == 'reopen':
            self.run(f'{self.facade}_fini')
            self.run(f'{self.facade}_init')
            return True
        if name == 'open_context':
            self.open()
            return self.current
        if name == 'use_context':
            self.current = args[0]
            return self.current
        user = Path(os.fsdecode(self.user))
        if name == 'user_conf':
            conf = user / 'user.conf'
            return conf.read_text(errors='surrogateescape') if conf.is_file() else None
        if name == 'user_files':
            return [[path.name, oct(path.stat().st_mode & 0o7777), path.stat().st_size]
                    for path in sorted(user.iterdir()) if path.is_file()]
        if name == 'chmod_user_files':
            for path in user.iterdir():
                if path.is_file():
                    path.chmod(args[0])
            return True
        if name == 'numeric_locale':
            value = self.libc.setlocale(LC_NUMERIC, None)
            return value.decode() if value else None
        if name == 'set_numeric_locale':
            value = self.libc.setlocale(LC_NUMERIC, args[0].encode())
            return value.decode() if value else None
        return NOT_A_STEP

    def run(self, name, *args):
        handled = self.step(name, *args)
        if handled is not NOT_A_STEP:
            return handled
        facade, base = name.split('_', 1)
        if facade != self.facade:
            raise CaptureError(f'{name} on the {self.facade} facade')
        inst = self.instance
        if base == 'init':
            self.contexts[self.current][0] = context = self.fn(name, P, S, S)(self.data, self.user)
            self.contexts[self.current][1] = context and self.fn(f'{facade}_alloc_instance', P, P)(context)
            return bool(self.contexts[self.current][1])
        if base == 'fini':
            self.fn(f'{facade}_free_instance', None, P)(inst)
            self.fn(name, None, P)(self.context)
            self.contexts[self.current] = [None, None]
            return True
        base = {'get_zhuyin_key': 'get_pinyin_key', 'get_zhuyin_key_rest': 'get_pinyin_key_rest',
                'get_zhuyin_key_rest_positions': 'get_pinyin_key_rest_positions'}.get(base, base)
        if base in ('parse_more_full_pinyins', 'parse_more_chewings'):
            return int(self.fn(name, Z, P, S)(inst, args[0].encode()))
        if base == 'get_parsed_input_length':
            return int(self.fn(name, Z, P)(inst))
        if base == 'set_options':
            return bool(self.fn(name, B, P, U32)(self.context, args[0]))
        if base == 'set_full_pinyin_scheme':
            return bool(self.fn(name, B, P, I)(self.context, args[0]))
        if base == 'get_pinyin_key':
            self.key = P()
            ok = bool(self.fn(name, B, P, Z, C.POINTER(P))(inst, args[0], C.byref(self.key)))
            return [ok, C.cast(self.key, C.POINTER(U16)).contents.value if self.key else None]
        if base == 'get_pinyin_key_rest':
            self.rest = P()
            ok = bool(self.fn(name, B, P, Z, C.POINTER(P))(inst, args[0], C.byref(self.rest)))
            return [ok, bool(self.rest)]
        if base == 'get_pinyin_key_rest_positions':
            begin, end = U16(UNTOUCHED_U16), U16(UNTOUCHED_U16)
            ok = bool(self.fn(name, B, P, P, C.POINTER(U16), C.POINTER(U16))(
                inst, self.rest, C.byref(begin), C.byref(end)))
            return [ok, begin.value, end.value]
        if base in ('get_pinyin_string', 'get_zhuyin_string', 'get_luoma_pinyin_string',
                    'get_secondary_zhuyin_string'):
            # The key of the last get_pinyin_key, or with an argument that pronunciation key.
            key = P(C.addressof(self.keys) + 2 * args[0]) if args else self.key
            text = P()
            ok = bool(self.fn(name, B, P, P, C.POINTER(P))(inst, key, C.byref(text)))
            return [ok, self.text(text.value)]
        if base in ('get_full_pinyin_auxiliary_text', 'get_sentence'):
            text = P()
            index = U8 if base == 'get_sentence' else Z
            ok = bool(self.fn(name, B, P, index, C.POINTER(P))(inst, args[0], C.byref(text)))
            return [ok, self.text(text.value)]
        if base in ('get_pinyin_offset', 'get_left_pinyin_offset', 'get_right_pinyin_offset'):
            out = Z(UNTOUCHED)
            ok = bool(self.fn(name, B, P, Z, C.POINTER(Z))(inst, args[0], C.byref(out)))
            return [ok, 'untouched' if out.value == UNTOUCHED else out.value]
        if base in ('guess_sentence', 'reset'):
            return bool(self.fn(name, B, P)(inst))
        if base == 'guess_candidates':
            return bool(self.fn(name, B, P, Z, U32)(inst, args[0], args[1]))
        if base in ('guess_candidates_before_cursor', 'guess_candidates_after_cursor'):
            return bool(self.fn(name, B, P, Z)(inst, args[0]))
        if base in ('get_n_candidate', 'get_n_phrase'):
            count = U32(UNTOUCHED)
            ok = bool(self.fn(name, B, P, C.POINTER(U32))(inst, C.byref(count)))
            return [ok, count.value]
        if base == 'get_candidate':
            self.candidate = P(UNTOUCHED)
            ok = bool(self.fn(name, B, P, U32, C.POINTER(P))(inst, args[0], C.byref(self.candidate)))
            value = self.candidate.value
            return [ok, 'untouched' if value == UNTOUCHED else 'row' if value else 'null']
        if base == 'get_candidate_type':
            kind = I(UNTOUCHED)
            ok = bool(self.fn(name, B, P, P, C.POINTER(I))(inst, self.candidate, C.byref(kind)))
            return [ok, kind.value]
        if base == 'get_candidate_string':
            text = P()
            ok = bool(self.fn(name, B, P, P, C.POINTER(P))(inst, self.candidate, C.byref(text)))
            return [ok, self.text(text.value, free=False)]
        if base == 'get_candidate_nbest_index':
            index = U8(UNTOUCHED_U8)
            ok = bool(self.fn(name, B, P, P, C.POINTER(U8))(inst, self.candidate, C.byref(index)))
            return [ok, index.value]
        if base == 'choose_candidate':
            return int(self.fn(name, I, P, Z, P)(inst, args[0], self.candidate))
        if base == 'train':
            return bool(self.fn(name, B, P, U8)(inst, args[0]))
        if base == 'save':
            return bool(self.fn(name, B, P)(self.context))
        if base == 'unload_phrase_library':
            return bool(self.fn(name, B, P, U8)(self.context, args[0]))
        if base == 'begin_add_phrases':
            self.importer = P(self.fn(name, P, P, U8)(self.context, args[0]))
            return bool(self.importer)
        if base == 'iterator_add_phrase':
            return bool(self.fn(name, B, P, S, S, I)(self.importer, args[0].encode(), args[1].encode(), args[2]))
        if base == 'end_add_phrases':
            self.fn(name, None, P)(self.importer)
            return True
        if base == 'begin_get_phrases':
            self.exporter = P(self.fn(name, P, P, C.c_uint)(self.context, args[0]))
            return bool(self.exporter)
        if base == 'iterator_has_next_phrase':
            return bool(self.fn(name, B, P)(self.exporter))
        if base == 'iterator_get_next_phrase':
            phrase, reading, count = P(), P(), I(UNTOUCHED)
            ok = bool(self.fn(name, B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
                self.exporter, C.byref(phrase), C.byref(reading), C.byref(count)))
            return [ok, self.text(phrase.value), self.text(reading.value), count.value]
        if base == 'end_get_phrases':
            self.fn(name, None, P)(self.exporter)
            return True
        if base == 'lookup_tokens':
            return self.tokens(name, args[0])
        if base in ('token_get_n_pronunciation', 'token_get_unigram_frequency'):
            value = U32(UNTOUCHED)
            ok = bool(self.fn(name, B, P, U32, C.POINTER(U32))(inst, args[0], C.byref(value)))
            return [ok, value.value]
        if base == 'token_get_nth_pronunciation':
            array = self.lib.g_array_new(0, 1, 2)
            try:
                ok = bool(self.fn(name, B, P, U32, U32, C.POINTER(GArray))(inst, args[0], args[1], array))
                length = array.contents.len
                self.keys = (U16 * length).from_buffer_copy(C.string_at(array.contents.data, 2 * length)) \
                    if length else (U16 * 0)()
                return [ok, list(self.keys)]
            finally:
                self.lib.g_array_free(array, 1)
        if base == 'phrase_segment':
            return bool(self.fn(name, B, P, S)(inst, args[0].encode()))
        if base == 'get_phrase_token':
            token = U32(UNTOUCHED)
            ok = bool(self.fn(name, B, P, U32, C.POINTER(U32))(inst, args[0], C.byref(token)))
            return [ok, token.value]
        raise CaptureError(f'no call {name}')


def worker(name, prefix, data):
    spec = OBSERVATIONS[name]
    os.umask(0o022)
    with tempfile.TemporaryDirectory(prefix='pin-observation-') as user:
        pin = Pin(Path(prefix) / LIBRARIES[spec.facade][0], data, user, spec.facade)
        spec.function(pin, spec.input)
    return 0


def observe(name, prefix, data):
    """Run C API observation NAME in a fresh worker process; answer its calls, results and end."""
    with tempfile.TemporaryDirectory(prefix='pin-capture-') as scratch:
        try:
            result = subprocess.run([sys.executable, __file__, 'worker', name, str(prefix), str(data)],
                                    capture_output=True, env=dict(os.environ, TMPDIR=scratch),
                                    timeout=WORKER_TIMEOUT)
        except subprocess.TimeoutExpired as error:
            raise CaptureError(f'{name}: the worker did not end within {WORKER_TIMEOUT} s') from error
    calls, results = [], []
    for line in result.stdout.decode().splitlines():
        record = json.loads(line)
        if 'call' in record:
            calls.append(record['call'])
        else:
            results.append(record['result'])
    if result.returncode > 0:
        raise CaptureError(f'{name}: the worker failed ({result.returncode}): '
                           + result.stderr.decode(errors='replace')[-600:])
    end = {'exit': 0} if result.returncode == 0 else {'signal': -result.returncode}
    spec = OBSERVATIONS[name]
    return {'facade': spec.facade, 'input': spec.input, 'calls': calls, 'results': results, 'end': end}


# ── the utility runs ─────────────────────────────────────────────────────

def stream(data):
    """Bytes as text when they are UTF-8, otherwise as hex."""
    try:
        return {'text': data.decode()}
    except UnicodeDecodeError:
        return {'hex': data.hex()}


def dbm_rows(path, cell):
    """A DBM file's [key hex, value hex] rows, sorted, as the frozen cases' `rows` reads them: a
    tkrzw or KC tree first, then a hash; BDB through db5.3_dump."""
    if cell == 'bdb':
        return contract_fixtures.index_file_rows(path, cell='bdb')
    if cell == 'tkrzw':
        library = os.environ.get('OXPINYIN_TKRZW_LIBRARY', 'libtkrzw.so.1')
        try:
            return contract_fixtures.index_file_rows(path, cell='tkrzw', kind='tree', allow_empty=True,
                                                     library=library)
        except ValueError:
            return contract_fixtures.index_file_rows(path, cell='tkrzw', kind='hash', allow_empty=True,
                                                     library=library)
    kc = C.CDLL('libkyotocabinet.so.16')
    kc.kcdbnew.restype = P
    kc.kcdbopen.argtypes, kc.kcdbopen.restype = [P, S, U32], I
    kc.kcdbcursor.argtypes, kc.kcdbcursor.restype = [P], P
    kc.kccurjump.argtypes, kc.kccurjump.restype = [P], I
    kc.kccurget.argtypes, kc.kccurget.restype = [P, C.POINTER(Z), C.POINTER(P), C.POINTER(Z), I], P
    kc.kccurecode.argtypes, kc.kccurecode.restype = [P], I
    kc.kccurdel.argtypes = kc.kcfree.argtypes = kc.kcdbdel.argtypes = [P]
    kc.kcdbclose.argtypes, kc.kcdbclose.restype = [P], I
    for kind in ('kct', 'kch'):
        db = kc.kcdbnew()
        if not kc.kcdbopen(db, f'{path}#type={kind}'.encode(), 1 | 1 << 6):  # KCOREADER|KCONOLOCK, kclangc.h
            kc.kcdbdel(db)
            continue
        rows_, cursor = [], kc.kcdbcursor(db)
        try:
            if kc.kccurjump(cursor):
                while True:
                    key_size, value, value_size = Z(), P(), Z()
                    key = kc.kccurget(cursor, C.byref(key_size), C.byref(value), C.byref(value_size), 1)
                    if not key:
                        break
                    rows_.append([C.string_at(key, key_size.value).hex(), C.string_at(value, value_size.value).hex()])
                    kc.kcfree(key)
            if kc.kccurecode(cursor) not in (0, 7):  # KCESUCCESS, KCENOREC
                raise CaptureError(f'{path}: KC cursor failed')
        finally:
            kc.kccurdel(cursor)
            kc.kcdbclose(db)
            kc.kcdbdel(db)
        return sorted(rows_)
    raise CaptureError(f'{path}: KC opens it as neither a tree nor a hash database')


def rows_digest(rows_):
    """The frozen cases' rows_digest: SHA-256 of `<key hex> <value hex>\\n` per sorted row, unmasked."""
    return hashlib.sha256(''.join(f'{key} {value}\n' for key, value in rows_).encode()).hexdigest()


def is_dbm(name):
    return name.endswith('.db') or name.endswith('_index.bin')


def grams(rows_, tokens):
    """Each token's SingleGram row (ngram.cpp:48-50,133-151: the total, then token and count
    pairs, little-endian guint32), or None when the file has no row for it."""
    found = {bytes.fromhex(key): bytes.fromhex(value) for key, value in rows_}
    decoded = {}
    for token in tokens:
        value = found.get(token.to_bytes(4, 'little'))
        if value is None:
            decoded[str(token)] = None
            continue
        words = [int.from_bytes(value[index:index + 4], 'little') for index in range(0, len(value) - 3, 4)]
        decoded[str(token)] = {'total': words[0],
                               'items': [words[index:index + 2] for index in range(1, len(words) - 1, 2)]}
    return decoded


def tree(work, before, cell):
    """Every path after the run: directories, and files with mode, size and whether the bytes are the
    pre-run ones; the SHA-256 of an unchanged or non-DBM file, the rows of a new or changed DBM
    file instead, and a tkrzw file's clock-masked SHA-256; a removed input as removed."""
    entries, seen = [], set()
    for path in sorted(work.rglob('*')):
        name = path.relative_to(work).as_posix()
        seen.add(name)
        status = path.lstat()
        mode = oct(status.st_mode & 0o7777)
        if path.is_symlink():
            entries.append({'path': name, 'kind': 'symlink', 'target': os.readlink(path)})
        elif path.is_dir():
            entries.append({'path': name, 'kind': 'directory', 'mode': mode})
        else:
            data = path.read_bytes()
            entry = {'path': name, 'kind': 'file', 'mode': mode, 'size': len(data),
                     'unchanged': before.get(name) == data}
            if entry['unchanged'] or not is_dbm(name):
                entry['sha256'] = hashlib.sha256(data).hexdigest()
            else:
                # A new or changed DBM file's container bytes are not the same on two runs: tkrzw keeps
                # a clock in bytes 48-56 and Berkeley DB containers are not byte-stable (#358). Its rows
                # are the observation, as the frozen cases' inventory() records a DBM file
                # (crates/oxpinyin-utils/tests/common/mod.rs:224-242). No frozen case this capture
                # feeds records a DBM file by its bytes, so no file keeps a bytes hash.
                entry['rows'] = dbm_rows(path, cell)
                entry['rows_sha256'] = rows_digest(entry['rows'])
            if data.startswith(b'TkrzwHDB\n') and len(data) >= 128:
                masked = bytearray(data)
                masked[48:56] = bytes(8)  # the frozen cases' mask_tkrzw_timestamp
                entry['sha256_clock_masked'] = hashlib.sha256(masked).hexdigest()
            entries.append(entry)
    entries.extend({'path': name, 'kind': 'removed'} for name in sorted(set(before) - seen))
    return entries


def run_utility(name, program, root=ROOT):
    """Run utility observation NAME's PROGRAM in a private copy of the cell's W3 directory."""
    spec = OBSERVATIONS[name]
    with tempfile.TemporaryDirectory(prefix='pin-utility-') as scratch:
        work, temporary = Path(scratch) / 'w3', Path(scratch) / 'tmp'
        shutil.copytree(Path(root) / 'fixtures/w3' / W3_DIRECTORIES[spec.cell], work)
        temporary.mkdir()
        before = {path.relative_to(work).as_posix(): path.read_bytes()
                  for path in work.rglob('*') if path.is_file()}
        try:
            result = subprocess.run([str(program), *spec.arguments], cwd=work, input=spec.stdin,
                                    capture_output=True, env=dict(os.environ, TMPDIR=str(temporary)),
                                    timeout=WORKER_TIMEOUT)
        except subprocess.TimeoutExpired as error:
            raise CaptureError(f'{name}: the utility did not end within {WORKER_TIMEOUT} s') from error
        files = tree(work, before, spec.cell)
        decoded = None
        if spec.grams:
            dbm, tokens = spec.grams
            entry = next((entry for entry in files if entry['path'] == dbm and entry['kind'] == 'file'), None)
            if entry is not None:
                decoded = grams(entry['rows'] if 'rows' in entry else dbm_rows(work / dbm, spec.cell), tokens)
    observation = {'cell': spec.cell, 'program': spec.program, 'arguments': list(spec.arguments),
                   'stdin': stream(spec.stdin), 'stdout': stream(result.stdout), 'stderr': stream(result.stderr),
                   'end': {'exit': result.returncode} if result.returncode >= 0 else {'signal': -result.returncode},
                   'files': files}
    if spec.grams:
        observation['grams'] = decoded  # None: the run left no such file
    return observation


def run_sequence(name, programs, model, root=ROOT):
    """Run C1's three pin programs over the W3 subset of MODEL in a private work directory.

    The subset's temporary directory is staged as `<table-dir>` in the arguments and streams, so two
    runs compare equal. A program that fails ends the sequence, as the Makefile would."""
    spec = OBSERVATIONS[name]
    with tempfile.TemporaryDirectory(prefix='pin-sequence-') as scratch:
        work, out, temporary = Path(scratch) / 'text', Path(scratch) / 'out', Path(scratch) / 'tmp'
        w3_fixtures.subset(model, work)
        out.mkdir()
        temporary.mkdir()
        (out / 'table.conf').write_text(
            w3_fixtures.table_conf(image.cell.BACKEND_FORMATS[image.cell.CELL_TO_BACKEND[spec.cell]], build=True),
            encoding='utf-8')
        directory = str(work.resolve())

        def hidden(data):
            return data.replace(directory.encode(), b'<table-dir>')

        steps = []
        for program, arguments, stdin in w3_fixtures.sequence(work):
            try:
                result = subprocess.run([str(programs[program]), *arguments], cwd=out, input=stdin, capture_output=True,
                                        env=dict(os.environ, TMPDIR=str(temporary)), timeout=WORKER_TIMEOUT)
            except subprocess.TimeoutExpired as error:
                raise CaptureError(f'{name}: {program} did not end within {WORKER_TIMEOUT} s') from error
            steps.append({'program': program, 'arguments': [argument.replace(directory, '<table-dir>')
                                                            for argument in arguments],
                          'end': {'exit': result.returncode} if result.returncode >= 0 else {'signal': -result.returncode},
                          'stdout': stream(hidden(result.stdout)), 'stderr': stream(hidden(result.stderr))})
            if result.returncode:
                break
        files = {}
        for output in spec.outputs:
            path = out / output
            if path.is_file():
                rows_ = dbm_rows(path, spec.cell)
                files[output] = {'rows': rows_, 'rows_sha256': rows_digest(rows_)}
            else:
                files[output] = None
    return {'cell': spec.cell, 'steps': steps, 'files': files}


# ── capture.json and its checks ──────────────────────────────────────────

def build_tree(cache, cell, commit):
    return Path(cache) / f'data-{cell}' / 'src' / f'libpinyin-{commit}'


def built_library(tree_):
    """The libpinyin.so.X.Y.Z the build tree's src/.libs holds."""
    found = sorted(path for path in (tree_ / 'src/.libs').glob('libpinyin.so.*.*.*') if not path.is_symlink())
    if not found:
        raise CaptureError(f'{tree_}: no built src/.libs/libpinyin.so.*')
    return found[-1]


def checked_utilities(cache, root=ROOT):
    """Each cell's verified provisioned prefix and the utilities its build tree built."""
    needed = {}
    for spec in OBSERVATIONS.values():
        if isinstance(spec, (Utility, DataSequence)):
            needed.setdefault(spec.cell, set()).update(spec.programs())
    if not needed:
        return {}
    if cache is None:
        raise CaptureError('the utility runs need --provision-cache, a `tools.dev provision oracle` cache')
    commit = image.fields(Path(root) / 'tools/oracle/oracle-pin.txt')['libpinyin_commit']
    cells = {}
    for cell, programs in sorted(needed.items()):
        prefix = Path(cache) / 'oracle' / cell
        if not image.prefix_ok(prefix, cell, root):
            raise CaptureError(f'{prefix}: prefix_ok refused it: another pin or recipe, or damaged recorded bytes')
        record = image.fields(prefix / 'oracle-pin.txt')
        tree_ = build_tree(cache, cell, commit)
        head = tree_ / '.git/HEAD'
        if not head.is_file() or head.read_text().strip() != commit:
            raise CaptureError(f'{tree_}: the build tree is not the pin commit {commit}')
        library = built_library(tree_)
        if image.digest_file(library) != record['shared_object_sha256']:
            raise CaptureError(f'{library}: the build tree built {image.digest_file(library)}, '
                               f"the prefix records {record['shared_object_sha256']}")
        built = {}
        for subdir, program in sorted(programs):
            path = tree_ / 'utils' / subdir / program
            if not path.is_file() or not os.access(path, os.X_OK) or path.read_bytes()[:4] != b'\x7fELF':
                raise CaptureError(f'{path}: not a built utility executable')
            built[program] = {'path': str(path), 'sha256': image.digest_file(path)}
        cells[cell] = {'prefix': str(prefix), 'recipe_sha256': record['recipe_sha256'],
                       'library_sha256': record['shared_object_sha256'], 'tree': str(tree_), 'utilities': built}
    return cells


def checked_model(model):
    """The verified model20 extraction C1's subset is built from (w3_fixtures.model_dir)."""
    if not any(isinstance(spec, DataSequence) for spec in OBSERVATIONS.values()):
        return None
    if model is None:
        raise CaptureError('the data sequence needs --model-dir, the model20 extraction `provision oracle` fetched')
    try:
        return w3_fixtures.model_dir(model, {})
    except w3_fixtures.Refused as error:
        raise CaptureError(str(error)) from error


def checked_identity(prefix, data, root=ROOT, libraries=None, cache=None, model=None):
    """capture.json for the facades at PREFIX on the system data DATA, the utility cells under CACHE
    and the model20 extraction MODEL; refuse any other pin, library, data, build or model.
    LIBRARIES overrides the loaded paths."""
    if not image.prefix_ok(prefix, BACKEND, root):
        raise CaptureError(f'{prefix}: prefix_ok refused it: another pin or recipe, or damaged recorded bytes')
    record = image.fields(Path(prefix) / 'oracle-pin.txt')
    facades = {}
    for facade, (relative, key) in LIBRARIES.items():
        library = (libraries or {}).get(facade, Path(prefix) / relative)
        loaded = image.digest_file(library)
        if loaded != record[key]:
            raise CaptureError(f'{library}: the loaded lib{facade} is {loaded}, the prefix records {record[key]}')
        facades[facade] = {'library_sha256': loaded}
    digest = contract_data.data_identity(data)
    pinned = json.loads((Path(root) / 'tools/oracle/libpinyin-data.json').read_text())['content_sha256']
    if digest != pinned:
        raise CaptureError(f'{data}: the data digest is {digest}, the repository pins {pinned}')
    return {'schema': SCHEMA, 'families': sorted({family(name) for name in OBSERVATIONS}), 'backend': BACKEND,
            'platform': PLATFORMS.get(platform.machine(), platform.machine()),
            'image': os.environ.get('OXPINYIN_CAPTURE_IMAGE'),
            'pin_commit': record['libpinyin_commit'],
            'recipe_sha256': record['recipe_sha256'],
            'library_sha256': facades['pinyin']['library_sha256'],
            'facades': facades,
            'versions_sha256': record['versions_sha256'],
            'data': {'kind': 'package', 'digest': digest},
            'utility_cells': checked_utilities(cache, root),
            'model_dir': None if (path := checked_model(model)) is None else str(path)}


def require_capture_environment():
    """Capture only runs in the Debian testing pin image; refuse any other host."""
    release = Path('/etc/os-release')
    content = release.read_text() if release.is_file() else ''
    # Rolling Debian identifies itself as <testing codename>/sid; do not pin that codename.
    if not re.search(r'^ID=debian$', content, re.M) or not re.search(
            r'^PRETTY_NAME="Debian GNU/Linux [^"]+/sid"$', content, re.M):
        raise ValueError('capture runs in the Debian testing pin image: see docs/runbooks/pin-captures.md')
    if not os.environ.get('OXPINYIN_CAPTURE_IMAGE'):
        raise ValueError('capture needs OXPINYIN_CAPTURE_IMAGE, the image identity (set by just capture-pin)')


def capture(run, prefix, data, staging, take, root=ROOT, libraries=None, cache=None, model=None):
    """Check the pin, data and builds, take each observation with TAKE, and stage both under STAGING/RUN."""
    separators = {'/', os.sep, os.altsep} - {None}
    if run in ('', '.', '..') or any(separator in run for separator in separators):
        raise CaptureError(f'the run name {run!r} is not a single path component')
    identity = checked_identity(prefix, data, root, libraries, cache, model)
    directory = Path(staging) / run
    if directory.exists():
        raise CaptureError(f'{directory} exists: stage each run once')
    observations = {name: take(name, identity) for name in OBSERVATIONS}
    directory.mkdir(parents=True)
    for name, value in (('capture.json', identity), ('observations.json', observations)):
        (directory / name).write_text(json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + '\n')
    print(f'{directory}: {len(observations)} proposed observations', flush=True)
    return 0


def take_observation(prefix, data, root=ROOT):
    def take(name, identity):
        spec = OBSERVATIONS[name]
        built = identity['utility_cells'].get(getattr(spec, 'cell', None), {}).get('utilities', {})
        if isinstance(spec, Utility):
            return run_utility(name, built[spec.program]['path'], root)
        if isinstance(spec, DataSequence):
            return run_sequence(name, {program: built[program]['path'] for _, program in SEQUENCE_PROGRAMS},
                                identity['model_dir'], root)
        return observe(name, prefix, data)
    return take


def main(argv):
    if argv[:1] == ['worker']:
        return worker(*argv[1:])
    parser = argparse.ArgumentParser(prog='pin_capture.py', description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest='command', required=True)
    run = commands.add_parser('capture', help='capture one run on the pin, in the capture image')
    run.add_argument('run', help='the run name, such as 1 or 2')
    run.add_argument('--prefix', required=True, help="the pin's tkrzw install prefix")
    run.add_argument('--data', required=True, help='the pinned libpinyin-data directory')
    run.add_argument('--provision-cache', help='the `tools.dev provision oracle --cache-dir` of the utility runs')
    run.add_argument('--model-dir', help="the verified model20 extraction of the data sequence (its parent holds 'verified')")
    run.add_argument('--staging', default=str(DEFAULT_STAGING), help='the staging directory')
    args = parser.parse_args(argv)
    try:
        require_capture_environment()
        return capture(args.run, args.prefix, args.data, args.staging,
                       take_observation(args.prefix, args.data), cache=args.provision_cache,
                       model=args.model_dir)
    except (CaptureError, ValueError, OSError, subprocess.CalledProcessError) as error:
        print(f'FAIL: {error}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
