"""Training and constraints cases (inventory row 40, #1175).
"""
import ctypes as C

from contract_abi import B, I, P, S, U, Z, choose_text, full_inst, row_with_text, sentence_out, tokens_of


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


def forced_sentence(k, relook):
    """nihao decoded, 泥 forced over 'ni' afterwards; optionally looked up again.

    Allocate a new instance and return (instance, selection result). Choose a
    normal row at byte zero; relook requests another sentence guess after
    selection. Native flags are not asserted."""
    inst = k.alloc()
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
    chosen = k.fn('choose_candidate', I, P, Z, P)(inst, 0, row_with_text(k, inst, 2, '泥'))
    if relook:
        k.fn('guess_sentence', B, P)(inst)
    return inst, chosen


@case('abort-zhuyin-train-stale-forcing', mode='zhuyin', abort=False, sigabrt=True, issues=('525', 'row:6'))
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


@case('abort-train-stale-forcing', abort=False, sigabrt=True, issues=('525', 'row:6'))
def _(k):
    """Train result zero immediately after forcing 泥 onto a previously guessed nihao."""
    inst, _chosen = forced_sentence(k, False)
    return {'ret': k.fn('train', B, P, C.c_ubyte)(inst, 0)}


# An index past a NONEMPTY result set, on a real user directory: the user-dir
# gate passes and `assert(index < results.size())` fails (`pinyin.cpp:2684`).
# `null-user-session-pinyin` trains with a NULL user directory and so stops
# at the earlier gate.
@case('abort-train-index-past-results', abort=False, sigabrt=True, userdir=True, issues=('525', 'row:6'))
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
    """Imports (text, reading, count) rows into USER_DICTIONARY; the returns.

    Text is UTF-8 encoded; reading is supplied as bytes. Assert iterator
    creation and close it after adding. Individual add failures remain in
    the returned list; the target library is seven."""
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    assert it
    add = k.fn('iterator_add_phrase', B, P, S, S, I)
    added = [add(it, text.encode(), reading, count) for text, reading, count in phrases]
    k.fn('end_add_phrases', None, P)(it)
    return added


def candidate_heads(k, inst, limit=6):
    """(kind, text) of the first `limit` rows of the candidate list.

    Return [kind, text] rows from the existing list without checking native
    accessor flags. Map NULL
    or empty text to None and decode other text as strict UTF-8."""
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
    """The shared head of the sequence: options, imports, parse, control guess,
    candidate list, and the second phrase's candidate chosen. On the full data
    the n-best usually holds the first phrase alone, so the second reaches the
    list as its NORMAL row, else as an NBEST row; choosing either installs
    the same OneStep forcing over the whole input (`pinyin.cpp:2515-2520` and
    `:2566-2576`).

    Require at least two (text, reading bytes, count) rows. Prefer the second
    phrase's normal row, falling back to its n-best row, and assert it exists.
    Return (new instance, observations), leaving the instance allocated.
    Record setup and selection flags without asserting those flags."""
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
    """nihao decoded, a phrase chosen past row 1, looked up again, re-parsed.

    Choose row index two and return the newly allocated instance. Use it
    throughout without validating call flags or guessing again after the
    reparse of shrink."""
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


@case('abort-train-after-reparse-past-matrix-end', abort=False, sigabrt=True, issues=('525', 'row:82'))
def _(k):
    """Train the retained result after replacing the chosen nihao parse with ni."""
    inst = reparse_after_choose(k, b'ni')
    return {'ret': k.fn('train', B, P, C.c_ubyte)(inst, 0)}


# The `'ni` twin walks the same clamp into a matrix whose column 0 the
# leading apostrophe left empty (`:663`): reproduced, SIGABRT at
# `phonetic_key_matrix.cpp:663` under the `__assert_fail` wrapper.
@case('abort-train-after-reparse-empty-start-column', abort=False, sigabrt=True, issues=('525', 'row:82'))
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
@case('abort-train-after-reparse-empty-end-column', abort=False, sigabrt=True, issues=('525', 'row:82'))
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


# Row 82 through libzhuyin: `zhuyin_train` runs the same `train_result3`
# (`zhuyin.cpp:1712-1713`), `zhuyin_parse_more_full_pinyins` refills the
# matrix and leaves the forcings alone (`:1016-1043`), and
# `zhuyin_choose_candidate` validates them against the matrix of the time
# only (`:1647`, `:1664`). With 你 forced over `ni` and `nihao` looked up
# again, the walk trains 你 over columns 0-2 and then 好 from column 2 to
# `constraints->length() - 1`, which is 5 (`phonetic_lookup.h:921`).
# `FORCE_TONE`, set by `zhuyin_init`, would refuse the toneless syllables,
# so the context takes the separator cases' option word first.
ZHUYIN_REPARSE_OPTIONS = 0x18a


def zhuyin_reparse_after_choose(k, shrink):
    """nihao decoded through libzhuyin, 你 chosen at 0, looked up again, re-parsed.

    Set the full-pinyin option word on the context, choose the first row whose
    text is 你 and return the new instance, without validating call flags or
    guessing again after the reparse of shrink."""
    k.fn('set_options', B, P, U)(k.ctx, ZHUYIN_REPARSE_OPTIONS)
    inst = k.alloc()
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    chosen = None
    for i in range(count.value):
        cand, text = P(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        if text.value == '你'.encode():
            chosen = cand
            break
    assert chosen is not None, 'no 你 row after nihao'
    k.fn('choose_candidate', I, P, Z, P)(inst, 0, chosen)
    k.fn('guess_sentence', B, P)(inst)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, shrink)
    return inst


# `ni` leaves a three-column matrix: 好's span ends at 5 (`:661`).
@case('abort-zhuyin-train-after-reparse-past-matrix-end', mode='zhuyin', abort=False, sigabrt=True,
      issues=('525', 'row:82'))
def _(k):
    """Train the retained zhuyin result after replacing the nihao parse with ni."""
    return {'ret': k.fn('train', B, P)(zhuyin_reparse_after_choose(k, b'ni'))}


# `'ni`: the leading apostrophe leaves column 0 empty (`:663`).
@case('abort-zhuyin-train-after-reparse-empty-start-column', mode='zhuyin', abort=False, sigabrt=True,
      issues=('525', 'row:82'))
def _(k):
    """Train the retained zhuyin result after reparsing a leading apostrophe followed by ni."""
    return {'ret': k.fn('train', B, P)(zhuyin_reparse_after_choose(k, b"'ni"))}


# `ni'hao`: 好's stale span runs from the apostrophe's zero-key column 2 to
# column 5, inside hao and empty (`:664`).
@case('abort-zhuyin-train-after-reparse-empty-end-column', mode='zhuyin', abort=False, sigabrt=True,
      issues=('525', 'row:82'))
def _(k):
    """Train the retained zhuyin result after reparsing ni'hao with its shifted key boundaries."""
    return {'ret': k.fn('train', B, P)(zhuyin_reparse_after_choose(k, b"ni'hao"))}


# `ni'ha`: the stale span ends on the reserved slot's zero key, and the pin
# trains (`train-after-reparse-internal-zero-key`'s twin).
@case('zhuyin-train-after-reparse-internal-zero-key', mode='zhuyin', control=True, issues=('525', 'row:82'))
def _(k):
    """Train the retained zhuyin result after reparsing ni'ha with an internal separator."""
    return {'ret': k.fn('train', B, P)(zhuyin_reparse_after_choose(k, b"ni'ha"))}


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
