"""Auxiliary text renderer cases."""
import ctypes as C

from contract_abi import B, P, S, U, UNTOUCHED, Z


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


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


def chewing_cursors(k, parse_name, text, cursors):
    """Collect chewing auxiliary text after parse_name at each byte cursor.

    Allocate a separate instance per cursor so the parse is the only state.
    Key the mapping by decoded input and cursor; values hold the return flag
    and decoded/freed text, or the untouched marker when the out-param was
    not written. text must decode as UTF-8 for the label, and cursors must
    be reiterable."""
    out = {}
    for cursor in cursors:
        inst = k.alloc()
        k.fn(parse_name, Z, P, S)(inst, text)
        aux = P(UNTOUCHED)
        ret = k.fn('get_chewing_auxiliary_text', B, P, Z, C.POINTER(P))(
            inst, cursor, C.byref(aux))
        out['%s %s@%d' % (parse_name, text.decode(), cursor)] = [
            ret, 'untouched' if aux.value == UNTOUCHED else k.text(aux.value)]
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


@case('aux-text-chewing-cursors', issues=('925',))
def _(k):
    """Collect chewing text at every byte cursor of four full-pinyin inputs.

    The inputs place a cut on both sides of a key boundary, inside a
    multi-scalar zhuyin spelling (nihao@3), and exactly at its last scalar:
    ni1@2 drops the tone-1 digit from the spelling and ni3hao4@6 cuts the
    toned 'hao4'. Those are the two reachable terminator cuts. A cursor one
    past the input and the maximum size_t clamp to the parsed length."""
    out = {}
    for text in (b'nihao', b'ni1', b'ni3hao4', b'xian'):
        every = tuple(range(0, len(text) + 1)) + (2**64 - 1,)
        out.update(chewing_cursors(k, 'parse_more_full_pinyins', text, every))
    return out


@case('aux-text-chewing-zhuyin-cursors', issues=('925',))
def _(k):
    """Collect chewing text at every keystroke cursor after a zhuyin parse.

    su3cl3 spells each key in three keystrokes, so the four interior cursors
    in a key are all below its zhuyin scalar count and the key boundaries
    carry the terminator. The maximum size_t is the overrun cursor."""
    out = {}
    every = tuple(range(0, len(b'su3cl3') + 1)) + (2**64 - 1,)
    out.update(chewing_cursors(k, 'parse_more_chewings', b'su3cl3', every))
    return out


@case('aux-text-chewing-overrun-cursor', issues=('925',))
def _(k):
    """Collect chewing text at cursors past the parsed input on both parse paths.

    Cursors one past the input, seven past it and the maximum size_t all
    clamp to parsed_len in pinyin_get_chewing_auxiliary_text
    (src/pinyin.cpp:3527 at pin 074a2219)."""
    out = {}
    for parse_name, text in (('parse_more_full_pinyins', b'nihao'),
                             ('parse_more_chewings', b'su3cl3')):
        cursors = (len(text) + 1, len(text) + 7, 2**64 - 1)
        out.update(chewing_cursors(k, parse_name, text, cursors))
    return out


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


def register(driver_case):
    """Register the declarations in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
