"""Axis C cases: the replacement battery's options, encoding and layout probes (inventory row 66, #1175).

The battery that runs and compares them is contract_axis_c. The driver builds the
AXIS_C_* registration tables on every load and passes them in; registration fills them.
"""
import ctypes as C
import json
import os
import subprocess
import sys

from contract_abi import Arr, B, I, P, S, U, UNTOUCHED, Z, sentence_out, zhuyin_sentence_out
from contract_case_helpers import candidate_rows, d18_export_phrases


def register(case, *, AXIS_C_DEFINITION, AXIS_C_GROUPS, AXIS_C_LAYOUT_COVERED, WORKER):
    """Register these cases at their original driver position."""
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
