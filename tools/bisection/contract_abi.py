"""C ABI vocabulary and call helpers shared by the contract-diff cases.

The ctypes aliases the cases write their signatures with, the out-parameter
sentinel, and the helpers that drive one facade call and return its
observation: parse results, candidate and sentence readers, token and unigram
readers. Every helper takes the case's Kit (contract-diff.py) and uses only
its `mode`, `fn`, `text`, `alloc`, `inst` and `glib`. Nothing here reads the
driver's per-run state (cell, registry, timings, cancellation), so one copy
serves every load of the driver. contract-diff.py re-exports each name; that is
how the cases, its tests and gcov_measure reach them.
"""
import ctypes as C
import json
import os
import signal

P, U, I, S, B, Z = C.c_void_p, C.c_uint, C.c_int, C.c_char_p, C.c_bool, C.c_size_t
UNTOUCHED = 0xABCDEF  # out-param sentinel


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


def full_inst(k, text):
    """Allocate an instance, parse full-pinyin bytes and return the live instance.

    Ignore the consumed-byte result; ownership of the new instance stays with
    the caller."""
    inst = k.alloc()
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
    return inst


def zhuyin_chewing(k, text=b'su3cl3'):
    """Return a new instance after parsing chewing bytes, ignoring the consumed-byte result.

    Leave the native instance allocated for the caller."""
    inst = k.alloc()
    k.fn('parse_more_chewings', Z, P, S)(inst, text)
    return inst


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


def zhuyin_sentence_out(k, inst):
    """Read a zhuyin sentence, distinguishing an untouched output from NULL.

    Return [native flag, text or sentinel]; decode and free any returned string."""
    out = P(UNTOUCHED)
    ret = k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(out))
    if out.value == UNTOUCHED:
        return [ret, 'untouched']
    return [ret, k.text(out.value)]


# #594: final-step tails, already ported by 55d861de. The pinyin facade
# exposes up to three indexed sentences; zhuyin exposes its <1, 1> result
# and has no SORT_WITHOUT_SENTENCE_CANDIDATE argument. Keep complete lists.
def final_step_sentences(k, inst):
    """Read available sentence texts without letting a missing pinyin rank abort this worker.

    Zhuyin reads once directly. Pinyin forks for ranks zero through two, silences
    child stderr, receives JSON through a pipe and reaps each child. Stop on
    SIGABRT or a false getter; assert other exits are successful with a payload.
    There is no per-child timeout here; returned strings are freed in the reader."""
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


def row_of(k, inst, want_kind):
    """The first candidate of a type in the instance's list, or None.

    Scan without regenerating the list or checking accessor flags. The returned
    pointer is borrowed and belongs to the instance's native candidate list."""
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    for i in range(count.value):
        cand, kind = P(), I()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        if kind.value == want_kind:
            return cand
    return None


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


def guessed(k, text=b'nihao'):
    """Return a new instance after a full-pinyin parse, sentence guess and candidate guess.

    Guess candidates at byte zero with sort option zero; ignore call flags and
    leave the instance allocated for the caller."""
    inst = k.alloc()
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
    k.fn('guess_sentence', B, P)(inst)
    k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
    return inst


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


def unigram_of(k, token):
    """Return the unigram lookup flag and raw unsigned frequency for a token.

    Seed the output with UNTOUCHED and retain that numeric value if unwritten.
    Use the kit's cached instance without changing the frequency here."""
    freq = U(UNTOUCHED)
    ret = k.fn('token_get_unigram_frequency', B, P, U, C.POINTER(U))(k.inst, token, C.byref(freq))
    return [ret, freq.value]


def owned_text(k,pointer):
    """Decode/free an owned GLib string unless untouched, returning None for NULL.

    Delegate decoding with UTF-8 replacement and release to Kit.text."""
    return 'untouched' if pointer.value==UNTOUCHED else k.text(pointer.value)
