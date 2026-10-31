"""GLib array lifecycle and null-array cases."""
import ctypes as C
from pathlib import Path
import subprocess

from contract_abi import Arr, B, I, P, S, U, UNTOUCHED, tokens_of


def register_array(case):
    """Register the array and critical cases in their original position."""
    # #924: expectations derived from 074a2219 pinyin.cpp:1450-1459,
    # zhuyin.cpp:969-978, phrase_large_table3.h:77-97 and both nth getters.
    # GLib 2.89.4 garray.c:605-625,773-795 preserves caller metadata and
    # invokes clear callbacks on shrink; gmessages.h:663-674 returns only
    # after the critical, leaving the prior segmentation result untouched.
    def glib_924_array_lifecycle(k):
        """Mutate, resize, reuse and free both real caller-owned GArray types."""
        new = k.glib.g_array_new
        new.restype, new.argtypes = P, [I, I, U]
        append = k.glib.g_array_append_vals
        append.restype, append.argtypes = P, [P, P, U]
        resize = k.glib.g_array_set_size
        resize.restype, resize.argtypes = P, [P, U]
        free = k.glib.g_array_free
        free.restype, free.argtypes = P, [P, I]
        clear_type = C.CFUNCTYPE(None, P)
        set_clear = k.glib.g_array_set_clear_func
        set_clear.restype, set_clear.argtypes = None, [P, clear_type]
        out = {}
        token = None
        for kind, scalar in (('tokens', U), ('keys', C.c_ushort)):
            arr = new(1, 1, C.sizeof(scalar))
            cleared = []

            def clear(element):
                """Record values removed by the library's set_size(0)."""
                cleared.append(C.cast(element, C.POINTER(scalar))[0])

            callback = clear_type(clear)
            set_clear(arr, callback)

            def values():
                """Copy the live payload and check its promised zero terminator."""
                view = C.cast(arr, C.POINTER(Arr)).contents
                data = C.cast(view.data, C.POINTER(scalar))
                assert data[view.len] == 0
                return list(data[:view.len])

            try:
                seed = scalar(0x1234)
                append(arr, C.byref(seed), 1)
                if kind == 'tokens':
                    ret = k.fn('lookup_tokens', B, P, S, P)(k.inst, '你好'.encode(), arr)
                else:
                    ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, 0, arr)
                assert ret and cleared == [0x1234]
                original = values()
                assert original
                if kind == 'tokens':
                    token = original[0]
                # Force growth past spare capacity, then shrink and reuse it.
                extra = (scalar * 257)(*range(1, 258))
                append(arr, extra, 257)
                assert values() == original + list(range(1, 258))
                resize(arr, len(original))
                assert values() == original
                cleared.clear()
                if kind == 'tokens':
                    refused = k.fn('lookup_tokens', B, P, S, P)(k.inst, b'not-a-phrase', arr)
                else:
                    refused = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, 0xFFFFFFFF, 0, arr)
                assert not refused and values() == [] and cleared == original
                resize(arr, 17)
                assert values() == [0] * 17
                # free(FALSE) transfers the data allocation to the caller.
                payload = free(arr, False)
                arr = None
                k.glib.g_free(payload)
                out[kind] = dict(ret=ret, original=original, refused=refused,
                                 cleared=cleared, caller_mutation=True)
            finally:
                if arr:
                    free(arr, True)
        return out


    def glib_924_segment_critical(k):
        """Require unchanged prior token outputs; return the logs the refusal wrote.

        The pin logs `g_return_val_if_fail`'s critical (074a2219
        pinyin.cpp:1454, zhuyin.cpp:973); the subject writes none
        (docs/findings/1297-segment-critical.md)."""
        inst = k.inst
        segment = k.fn('phrase_segment', B, P, S)

        def result():
            """Read the public result count and token outputs with initialized sentinels."""
            count = U(UNTOUCHED)
            assert k.fn('get_n_phrase', B, P, C.POINTER(U))(inst, C.byref(count))
            tokens = []
            for index in range(count.value):
                token = U(UNTOUCHED)
                assert k.fn('get_phrase_token', B, P, U, C.POINTER(U))(inst, index, C.byref(token))
                tokens.append(token.value)
            return tokens

        assert segment(inst, '你好'.encode())
        old = result()
        assert old and any(old)
        k.logs.clear()
        k.log_messages.clear()
        ret = segment(inst, b'\xff\xfe')
        assert ret is False and result() == old
        return dict(ret=ret, retained=old, messages=k.log_messages)


    for _mode in ('pinyin', 'zhuyin'):
        case(f'glib-924-array-lifecycle-{_mode}', mode=_mode, issues=('924',))(glib_924_array_lifecycle)
        case(f'glib-924-segment-critical-{_mode}', mode=_mode, issues=('924', '1297'),
             policy_record='1297-segment-critical.md')(glib_924_segment_critical)
        # GLib 2.89.4 gmessages.c:1274-1313 applies the caller's fatal mask:
        # the pin aborts at its critical; the subject logs none and returns.
        case(f'glib-924-segment-fatal-critical-{_mode}', mode=_mode, fatal=True,
             stderr=True, issues=('924', '1297'),
             policy_record='1297-segment-critical.md')(glib_924_segment_critical)


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


def register_oom(case, *, glib_924_oom_array):
    """Register the shared driver OOM helper at its original position."""
    for _mode in ('pinyin', 'zhuyin'):
        case(f'glib-924-oom-array-growth-{_mode}', mode=_mode,
             issues=('924', '924-oom'))(glib_924_oom_array)


def register_null(case):
    """Register the null-array cases at their original position."""
    # Source-derived defect witnesses, deliberately ordinary DIFFERs rather than
    # policy exceptions. 074a2219 pinyin.cpp:2805-2818 / zhuyin.cpp:1797-1810
    # ignore g_array_set_size/append_vals' NULL returns; reduce_tokens at
    # storage/phrase_large_table3.h:77-97 does the same for lookup_tokens.
    # GLib 2.89.4 garray.c:611,778 emits criticals, not a NULL dereference.
    def glib_924_null_array(k, operation):
        """Observe NULL-array return values and macro-generated GLib criticals."""
        inst = k.inst
        token = tokens_of(k, '你好')[0]
        # Kit normally captures the facade and default domains. These checks
        # originate inside GLib, whose build supplies G_LOG_DOMAIN="GLib".
        install = k.glib.g_log_set_handler
        callback_type = type(k._handler)
        install.argtypes, install.restype = [S, U, callback_type, P], U
        handler_id = install(b'GLib', 0xFFFFFFFC, k._handler, None)
        out = {}
        try:
            for found in (True, False):
                k.logs.clear()
                k.log_messages.clear()
                if operation == 'lookup':
                    phrase = '你好'.encode() if found else b'not-a-phrase'
                    ret = k.fn('lookup_tokens', B, P, S, P)(inst, phrase, None)
                else:
                    ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(
                        inst, token if found else 0xFFFFFFFF, 0, None)
                out['found' if found else 'missing'] = dict(
                    ret=ret, logs=list(k.logs), messages=list(k.log_messages))
        finally:
            remove = k.glib.g_log_remove_handler
            remove.argtypes, remove.restype = [S, U], None
            remove(b'GLib', handler_id)
        return out


    for _mode in ('pinyin', 'zhuyin'):
        for _operation in ('lookup', 'pronunciation'):
            case(f'glib-924-null-array-{_operation}-{_mode}', mode=_mode,
                 issues=('924', '924-null-array'))(
                lambda k, operation=_operation: glib_924_null_array(k, operation))
