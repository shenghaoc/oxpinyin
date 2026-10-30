"""Compact discrepancy-record entries for the contract differential (contract-diff.py).

A record's `contract-diff-policy` block keeps, per case and cell, a SHA-256 of
each side's complete stable observation, a short digest per top-level field,
and the pin-to-subject divergence in plain text. The verdict compares the two
SHA-256 digests, so a POLICY-ACCEPTED verdict still needs the complete
observation (exit, result, stderr and, for a user-directory case, userfiles)
to equal the recorded one on both sides, with no wildcards, ignored fields or
partial matching. The field digests say which side and which field moved when
it does not. The divergence is what a reviewer reads; it is written by
`tools.dev policy-record` from contract-diff.py's `--observations` capture and
never read by the verdict.

Standard library only. contract_verdict.py imports this module by its bare
name beside the driver; tools/dev/policy.py imports it as
tools.bisection.contract_policy.
"""
import difflib
import hashlib
import itertools
import json
import signal

SCHEMA = 2
FIELD_DIGEST_LENGTH = 16
STABLE_KEYS = ('exit', 'result', 'stderr')
SIDES = ('pin', 'subject')
DIVERGENCE_BYTES = 4096
OMITTED = '… further divergence omitted (4096-byte budget)'
PREVIEW_BYTES = 384
LIST_DIFF_ITEMS = 32


def stable_observation(side, userdir):
    """The complete stable observation of one side: exit, result, stderr and,
    for a user-directory case, userfiles; never worker timings or counts."""
    keys = (*STABLE_KEYS, *(['userfiles'] if userdir else []))
    return {key: side[key] for key in keys}


def canonical(value):
    """One JSON text per JSON value: sorted keys, no spaces, ASCII escapes."""
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True)


def digest(value):
    """SHA-256 of the canonical text, as 64 lowercase hex digits."""
    return hashlib.sha256(canonical(value).encode('ascii')).hexdigest()


def field_digests(observation):
    """A short digest per top-level field: `exit`, `stderr`, `userfiles` and
    each key of a result document as `result.<key>` (`result` itself when the
    document is absent). Diagnostic only; the SHA-256 decides."""
    fields = {}
    for key, value in observation.items():
        if key == 'result' and isinstance(value, dict):
            for name, member in value.items():
                fields[f'result.{name}'] = digest(member)[:FIELD_DIGEST_LENGTH]
        else:
            fields[key] = digest(value)[:FIELD_DIGEST_LENGTH]
    return dict(sorted(fields.items()))


def side_entry(observation):
    """What the record keeps of one side."""
    return {'sha256': digest(observation), 'fields': field_digests(observation)}


def observation_problem(side, *, pin_abort=False):
    """Why `side` cannot be recorded or accepted, or None.

    A side terminates normally with a result document that carries `logs`
    and no `~` field, or it is the pin of a registered abort site: SIGABRT
    and no document. userfiles, when present, lists text by text."""
    if not isinstance(side, dict) or not isinstance(side.get('stderr'), str):
        return 'needs an observation with text stderr'
    files = side.get('userfiles')
    if files is not None and (not isinstance(files, dict) or any(
            not isinstance(key, str) or not isinstance(value, str) for key, value in files.items())):
        return 'user-directory listing must be complete'
    exit_code, result = side.get('exit'), side.get('result')
    if type(exit_code) is not int:
        return 'exit must be an integer'
    if pin_abort and exit_code == -signal.SIGABRT:
        return None if result is None else 'aborted pin cannot have a result document'
    if exit_code != 0 or not isinstance(result, dict):
        return 'must normally terminate with a result document, or hold a registered pin abort'
    if 'logs' not in result:
        return 'result must carry logs'
    if any(key.startswith('~') for key in result):
        return 'result must not carry ~ fields'
    return None


def tool_observation_problem(side, *, pin_signal=None):
    """Tool observations include all bytes; only a source-registered pin may signal."""
    if not isinstance(side, dict) or set(side) != {'exit', 'stdout', 'stderr', 'files'}:
        return 'needs complete exit, stdout, stderr and files'
    code = side['exit']
    if type(code) is not int or (code < 0 and code != pin_signal):
        return 'unexpected process termination'
    if not all(isinstance(side[key], str) for key in ('stdout', 'stderr')):
        return 'streams must preserve bytes as surrogateescaped text'
    if not isinstance(side['files'], dict) or any(
            not isinstance(key, str) or not isinstance(value, str)
            for key, value in side['files'].items()):
        return 'file listing must be complete'
    return None


def _utf8_text(text):
    """Escape surrogate code points without escaping ordinary non-ASCII text."""
    return text.encode('utf-8', 'backslashreplace').decode('utf-8')


def render(value, **kwargs):
    """Readable JSON that is safe to write as UTF-8, including surrogate bytes."""
    return _utf8_text(json.dumps(value, ensure_ascii=False, **kwargs))


def _text_bytes(text):
    """Bytes of a diagnostic string including its JSON quotes and escapes."""
    return len(render(text).encode('utf-8'))


class _Divergence:
    """Reserve the marker and the deepest policy-record field's indentation."""
    def __init__(self):
        self.lines = []
        self.used = len('          "divergence": [\n          ]')
        self.omitted = False
        self.full = False

    def append(self, line, omitted=False):
        cost = 14 + _text_bytes(line)  # 12-space indent, comma and newline.
        if self.used + cost + 14 + _text_bytes(OMITTED) > DIVERGENCE_BYTES:
            self.omitted = self.full = True
            return False
        self.lines.append(line)
        self.used += cost
        self.omitted |= omitted
        return True

    def finish(self):
        if self.omitted:
            self.lines.append(OMITTED)
        return self.lines


def _preview(value, budget=PREVIEW_BYTES, depth=0):
    """Build a bounded excerpt before JSON rendering; never serialize a large value."""
    cut = False
    if isinstance(value, str):
        shown = value[:max(0, (budget - 48) // 8)]
        cut = len(shown) != len(value)
        text = render(shown) + (f' … (length {len(value)})' if cut else '')
    elif isinstance(value, int) and value.bit_length() > 128:
        return f'<int {value.bit_length()} bits>', True
    elif isinstance(value, (list, dict)) and value:
        if depth >= 3 or budget < 64:
            return f'<{type(value).__name__} length {len(value)}>', True
        count = min(len(value), 3)
        share = (budget - 48) // count
        parts = []
        for item in itertools.islice(value.items() if isinstance(value, dict) else value, count):
            if isinstance(value, dict):
                key, member = item
                key_text, key_cut = _preview(key, share // 2, depth + 1)
                text, member_cut = _preview(member, share // 2, depth + 1)
                parts.append(f'{key_text}: {text}')
                cut |= key_cut or member_cut
            else:
                text, member_cut = _preview(item, share, depth + 1)
                parts.append(text)
                cut |= member_cut
        left, right = ('{', '}') if isinstance(value, dict) else ('[', ']')
        text = left + ', '.join(parts) + right
        if len(value) > count:
            text += f' … (length {len(value)})'
            cut = True
    else:
        text = render(value)
    if _text_bytes(text) > budget:
        return f'<{type(value).__name__}' + (f' length {len(value)}>' if hasattr(value, '__len__') else '>'), True
    return text, cut


def _same(pin, subject):
    """JSON equality for diagnostics, including bool/int and int/float distinctions."""
    if type(pin) is not type(subject):
        return False
    if isinstance(pin, dict):
        return pin.keys() == subject.keys() and all(_same(value, subject[key]) for key, value in pin.items())
    if isinstance(pin, list):
        return len(pin) == len(subject) and all(_same(a, b) for a, b in zip(pin, subject))
    if isinstance(pin, float):
        return canonical(pin) == canonical(subject)
    return pin == subject


def _pair(path, pin, subject, lines, *, omitted=False):
    before, before_cut = _preview(pin)
    after, after_cut = _preview(subject)
    return lines.append(f'{path}: pin {before}; subject {after}', omitted or before_cut or after_cut)


def _divergence(path, pin, subject, lines):
    if lines.full:
        return
    if isinstance(pin, dict) and isinstance(subject, dict):
        for key in sorted(set(pin) | set(subject)):
            name = _utf8_text(key[:48])
            if len(key) > 48:
                name += f'… (key length {len(key)})'
                lines.omitted = True
            member = f'{path}.{name}' if path else name
            if len(member) > 160:
                member = member[:160] + '…'
                lines.omitted = True
            if key not in pin:
                text, cut = _preview(subject[key])
                lines.append(f'{member}: subject only {text}', cut)
            elif key not in subject:
                text, cut = _preview(pin[key])
                lines.append(f'{member}: pin only {text}', cut)
            else:
                _divergence(member, pin[key], subject[key], lines)
            if lines.full:
                return
        return
    if isinstance(pin, list) and isinstance(subject, list):
        if _same(pin, subject):
            return
        # Only small, fully previewable lists need sequence alignment. A large
        # list gets its lengths and first changed item, without slicing or
        # encoding the complete list (or its large members) for the renderer.
        small = max(len(pin), len(subject)) <= LIST_DIFF_ITEMS
        if small:
            small = all(not _preview(item)[1] for item in itertools.chain(pin, subject))
        if not small:
            index = next((i for i, (a, b) in enumerate(zip(pin, subject)) if not _same(a, b)),
                         min(len(pin), len(subject)))
            member = f'{path} (list lengths pin {len(pin)}, subject {len(subject)}; first change [{index}])'
            if index == len(pin):
                text, _ = _preview(subject[index])
                lines.append(f'{member}: subject inserts {text}', True)
            elif index == len(subject):
                text, _ = _preview(pin[index])
                lines.append(f'{member}: subject drops {text}', True)
            else:
                _pair(member, pin[index], subject[index], lines, omitted=True)
            return
        matcher = difflib.SequenceMatcher(None, [canonical(item) for item in pin],
                                          [canonical(item) for item in subject], autojunk=False)
        for tag, i1, i2, j1, j2 in matcher.get_opcodes():
            if tag == 'equal':
                continue
            if tag == 'insert':
                text, cut = _preview(subject[j1:j2])
                lines.append(f'{path}[{i1}]: subject inserts {text}', cut)
            elif tag == 'delete':
                text, cut = _preview(pin[i1:i2])
                lines.append(f'{path}[{i1}:{i2}]: subject drops {text}', cut)
            elif i2 - i1 == j2 - j1 and all(isinstance(pin[i], (dict, list)) for i in range(i1, i2)):
                for offset in range(i2 - i1):
                    _divergence(f'{path}[{i1 + offset}]', pin[i1 + offset], subject[j1 + offset], lines)
                    if lines.full:
                        return
            else:
                _pair(f'{path}[{i1}:{i2}]', pin[i1:i2], subject[j1:j2], lines)
            if lines.full:
                return
        return
    if pin != subject or type(pin) is not type(subject):
        _pair(path, pin, subject, lines)


def divergence(pin, subject):
    """The recorded difference, one line per moved field or list edit, in a
    stable order: dictionary keys sorted, list edits in pin order with the
    pin's indices. Equal-length edits of nested documents are described
    member by member. The serialized divergence field (including JSON escapes,
    indentation and the omission marker) is bounded to 4096 UTF-8 bytes per
    case/cell pair. Previews are bounded before encoding; omitted text never
    changes the complete-observation digests used for acceptance."""
    lines = _Divergence()
    _divergence('', pin, subject, lines)
    return lines.finish()


def entry(pin, subject):
    """A record entry for one recorded pair of complete stable observations."""
    if canonical(pin) == canonical(subject):
        raise ValueError('policy case must record a difference, not parity')
    return {'pin': side_entry(pin), 'subject': side_entry(subject), 'divergence': divergence(pin, subject)}


def _hex(value, length):
    return isinstance(value, str) and len(value) == length and all(c in '0123456789abcdef' for c in value)


def check_entry(value):
    """`value` as a complete entry: both sides with their SHA-256 and field
    digests, a divergence text, and a difference between the sides."""
    if not isinstance(value, dict) or set(value) != {*SIDES, 'divergence'}:
        raise ValueError('policy case must specify both observations and their divergence')
    for side in SIDES:
        kept = value[side]
        if (not isinstance(kept, dict) or set(kept) != {'sha256', 'fields'} or not _hex(kept['sha256'], 64)
                or not isinstance(kept['fields'], dict) or not kept['fields']
                or any(not isinstance(key, str) or not _hex(member, FIELD_DIGEST_LENGTH)
                       for key, member in kept['fields'].items())):
            raise ValueError('policy observation must be complete')
    if not isinstance(value['divergence'], list) or not value['divergence'] \
            or any(not isinstance(line, str) for line in value['divergence']):
        raise ValueError('policy divergence must be lines of text')
    if value['pin']['sha256'] == value['subject']['sha256']:
        raise ValueError('policy case must record a difference, not parity')
    return value


def accepted(kept, actual):
    """Whether both actual stable observations digest to the recorded ones."""
    return all(digest(actual[side]) == kept[side]['sha256'] for side in SIDES)


def diagnose(kept, actual):
    """Which side and which field moved, from the record's field digests."""
    report = {}
    for side in SIDES:
        recorded, current = kept[side]['fields'], field_digests(actual[side])
        report[side] = dict(
            matches=digest(actual[side]) == kept[side]['sha256'],
            moved=[name for name in recorded if name in current and current[name] != recorded[name]],
            added=[name for name in current if name not in recorded],
            missing=[name for name in recorded if name not in current])
    return report


def describe(report):
    """One line: `pin matches; subject moved in result.with_sentence`."""
    parts = []
    for side in SIDES:
        side_report = report[side]
        if side_report['matches']:
            parts.append(f'{side} matches')
            continue
        where = [*side_report['moved'], *(f'+{name}' for name in side_report['added']),
                 *(f'-{name}' for name in side_report['missing'])]
        parts.append(f'{side} moved in {", ".join(where)}' if where else f'{side} moved')
    return _utf8_text('; '.join(parts))
