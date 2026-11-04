"""Frozen pin references for the contract differential (contract-diff.py).

`capture` runs selected cases on the pin in the Debian stable pin environment
and writes one reference row per case and cell; `verify` runs the same cases on
oxpinyin only and compares with those rows. A row stores the pin's complete
observation exactly as `Runner.run_worker` returns it (exit, result, userfiles,
stderr, stderr_lines), never a verdict, plus the identity that makes the row
valid: the case and a digest of its inputs, the pin commit, the backend, the
system data and the capture environment. The oxpinyin commit is not part of it.

References live in `fixtures/contract-refs/<cell>/<module>.jsonl`, one file per
case module, rows sorted by case ID. A policy-accepted departure is not stored
here: its digests stay in the discrepancy record and are applied at
verification. A case the pin must not run keeps its source-derived expectation
with `provenance: pin_not_run`.

Standard library only; contract-diff.py and its tests import it by bare name.
"""
import ast
import base64
import binascii
import ctypes as C
import hashlib
import inspect
import io
import json
import os
from pathlib import Path
import platform
import re
import textwrap
import tokenize
import zlib

SCHEMA = 1
ROOT = Path(__file__).resolve().parents[2]
REFERENCES = ROOT / 'fixtures/contract-refs'
PIN_COMMIT = '074a2219c90feaf962d0d24f034514033ece5f99'
OBSERVATION_KEYS = ('exit', 'result', 'userfiles', 'stderr_lines', 'stderr')
NOT_RUN = dict(exit='NOT-RUN', result=None, userfiles=[], stderr_lines=0, stderr='')
# The system-data kinds of a row: the package data directory, or files the case builds.
PACKAGE, SYNTHETIC = 'package', 'synthetic'

class ReferenceError(Exception):
    """A reference is missing, incomplete or incompatible with this run."""


def canonical(value):
    """One JSON text per value: sorted keys, no spaces, ASCII escapes."""
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=True)


def sha256_text(value):
    return hashlib.sha256(canonical(value).encode('ascii')).hexdigest()


def _code(source):
    """The parsed code of SOURCE as `ast.dump` text: comments, blank lines, formatting and docstrings
    are gone, so editing any of them leaves a case's digest alone.

    `inspect.getsource` returns a fragment for a case defined inside a call (a lambda passed to a
    decorator): that is not a module, so its tokens stand in for its tree."""
    try:
        tree = ast.parse(textwrap.dedent(source))
    except SyntaxError:
        skipped = (tokenize.COMMENT, tokenize.NL, tokenize.NEWLINE, tokenize.INDENT, tokenize.DEDENT,
                   tokenize.ENCODING, tokenize.ENDMARKER)
        kept = []
        try:
            for token in tokenize.generate_tokens(io.StringIO(source).readline):
                if token.type not in skipped:
                    kept.append(token.string)
        except tokenize.TokenError:
            pass  # an unclosed bracket at the end of the fragment: the tokens so far are all there is
        return 'tokens:' + ' '.join(kept)
    for node in ast.walk(tree):
        if isinstance(node, (ast.Module, ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            body = node.body
            if body and isinstance(body[0], ast.Expr) and isinstance(body[0].value, ast.Constant) \
                    and isinstance(body[0].value.value, str):
                node.body = body[1:] or [ast.Pass()]
    return ast.dump(tree, include_attributes=False)


def _source(fn):
    """The case code and effective bound inputs, without process-specific addresses."""
    arguments = []
    while hasattr(fn, 'func'):
        arguments.append([_bound(v) for v in fn.args])
        arguments.append(_bound(fn.keywords or {}))
        fn = fn.func
    cells = [_bound(cell.cell_contents) for cell in (fn.__closure__ or ())]
    return canonical(dict(code=_code(inspect.getsource(fn)), partial=arguments,
                          closure=cells, attributes=_bound(vars(fn))))


def _bound(value):
    """Represent ordinary captured inputs deterministically; reject unknown object types."""
    if value is None or type(value) in (bool, int, float, str):
        return value
    if isinstance(value, bytes):
        return {'bytes': value.hex()}
    if isinstance(value, Path):
        # A worker path inside this checkout names the same input on CI and locally.
        path = value.relative_to(ROOT).as_posix() if value.is_relative_to(ROOT) else str(value)
        return {'path': path}
    if isinstance(value, (tuple, list)):
        return [_bound(item) for item in value]
    if isinstance(value, dict):
        return {'dict': sorted((canonical(_bound(key)), _bound(item)) for key, item in value.items())}
    if isinstance(value, (set, frozenset)):
        return {'set': sorted(canonical(_bound(item)) for item in value)}
    if callable(value) and hasattr(value, '__qualname__'):
        # The driver is imported as contract_diff by tests and executed as __main__ by verify.
        module = ('contract_diff' if value.__module__ == '__main__'
                  and Path(inspect.getsourcefile(value) or '').name == 'contract-diff.py'
                  else value.__module__)
        return {'callable': f'{module}.{value.__qualname__}',
                'code': _code(inspect.getsource(value))}
    raise ReferenceError(f'unsupported captured input type: {type(value).__module__}.{type(value).__qualname__}')


# Registration metadata that says how a case is read, not what it does: it is not an input. A
# policy record classifies the pin's observation at verification, so it is not one either.
NOT_INPUTS = ('fn', 'case_id', 'issues', 'control', 'platform_skip_reason', 'policy_record')
DEFAULT_CELLS = ('bdb', 'kc', 'tkrzw')


def _default_option(value):
    """An option at its registration default: None, False or an empty container. By identity and
    type, never equality: `0 == False`, and an option set to 0 or 0.0 is an input."""
    return value is None or value is False or (isinstance(value, (tuple, list, dict, set)) and not value)


def inputs_digest(spec, NO_ABORT):
    """Digest of one case's inputs: its function's parsed code (`ast`, so comments and formatting
    are not inputs) and the registration metadata that changes what it does.

    Options at their default are left out, so adding or removing an option nobody sets does not
    invalidate every reference. Helper modules are not hashed either: a shared helper change that
    moves an observation is caught by the observation."""
    meta = {}
    for key, value in spec.items():
        if key in NOT_INPUTS or value is NO_ABORT or _default_option(value) or (
                key == 'cells' and tuple(value) == DEFAULT_CELLS):
            continue
        meta[key] = value
    return sha256_text(dict(source=_source(spec['fn']), spec=meta, case=spec['case_id']))


def data_identity(directory):
    """The system-data identity of a directory: digest over its files' names and SHA-256.

    The package's data files are identical on amd64 and arm64, so the digest does not
    name an architecture. Raise ReferenceError when the directory is not a data directory."""
    directory = Path(directory)
    if not (directory / 'table.conf').is_file():
        raise ReferenceError(f'{directory} is not a libpinyin system data directory (no table.conf)')
    rows = sorted((file.name, hashlib.sha256(file.read_bytes()).hexdigest())
                  for file in directory.iterdir() if file.is_file())
    return sha256_text(rows)


def glib_version():
    """The GLib this process loads, as `major.minor.micro`."""
    glib = C.CDLL('libglib-2.0.so.0')
    return '.'.join(str(C.c_uint.in_dll(glib, name).value)
                    for name in ('glib_major_version', 'glib_minor_version', 'glib_micro_version'))


def glibc_version():
    libc = C.CDLL(None)
    libc.gnu_get_libc_version.restype = C.c_char_p
    return libc.gnu_get_libc_version().decode()


def environment():
    """The environment a row was captured in, or the host's when verifying."""
    return dict(glib=glib_version(), glibc=glibc_version(), arch=platform.machine(),
                image=os.environ.get('OXPINYIN_CAPTURE_IMAGE'))


def require_capture_environment():
    """Capture only runs in the Debian stable pin image; refuse any other host."""
    release = Path('/etc/os-release')
    codename = re.search(r'^VERSION_CODENAME=(\S+)$', release.read_text(), re.M) if release.is_file() else None
    if not codename or codename[1] != 'trixie':
        raise ReferenceError('capture runs in the Debian stable (trixie) pin image: see tools/oracle/capture-contract.sh')
    if not os.environ.get('OXPINYIN_CAPTURE_IMAGE'):
        raise ReferenceError('capture needs OXPINYIN_CAPTURE_IMAGE, the image identity (set by capture-contract.sh)')


def stable_pin(observation):
    """The stored form of an observation: the comparison keys, nothing measured."""
    return {key: observation[key] for key in OBSERVATION_KEYS}


def encode_profile(directory):
    """A handoff profile as {file name: base64 of zlib}: the pin-written fixture a later run
    reads. The containers are mostly preallocated zeros, which compress to a few hundred bytes."""
    return {file.name: base64.b64encode(zlib.compress(file.read_bytes(), 9)).decode()
            for file in sorted(Path(directory).iterdir()) if file.is_file()}


def decode_profile(files, directory):
    if not isinstance(files, dict):
        raise ReferenceError('invalid handoff profile')
    for name, payload in files.items():
        if not isinstance(name, str) or not name or '..' in name or '/' in name or '\\' in name or Path(name).is_absolute():
            raise ReferenceError(f'invalid handoff profile file name: {name!r}')
        try:
            content = zlib.decompress(base64.b64decode(payload, validate=True))
        except (ValueError, TypeError, binascii.Error, zlib.error) as error:
            raise ReferenceError(f'invalid handoff profile data for {name!r}: {error}') from None
        Path(directory, name).write_bytes(content)


# The lambda scanner cases read `%f` with the host's libc (maintainer ruling 2026-10-28): a
# reference captured on glibc 2.41 may differ on another glibc: a mismatch there (never an equal
# result) is reported as HOST-DEPENDENT, the documented cross-host difference.
HOST_LIBC_PREFIXES = ('table-conf-lambda-', 'table-conf-919-lambda-')
REVERSE_CROSS_OPEN_CASES = ('issue1114-collision-load-save-pinyin', 'issue1114-collision-load-save-zhuyin',
                            'save-929-logical-cross-open-pinyin', 'save-929-logical-cross-open-zhuyin')


def host_libc_sensitive(case_id):
    return isinstance(case_id, str) and case_id.startswith(HOST_LIBC_PREFIXES)


def scanner_spelling(spec):
    """The lambda text installed by a registered scanner case, or None if it removes the line."""
    if not host_libc_sensitive(spec['case_id']):
        return None
    fn = spec['fn']
    seen = set()
    while id(fn) not in seen:
        seen.add(id(fn))
        edits = getattr(fn, 'config_edits', None)
        if edits is not None:
            values = [edit[1].split('lambda parameter:', 1)[1]
                      for edit in edits if len(edit) == 2 and edit[1].startswith('lambda parameter:')]
            if len(values) > 1:
                raise ReferenceError(f'{spec["case_id"]}: more than one lambda scanner input')
            return values[0] if values else None
        nested = [cell.cell_contents for cell in (getattr(fn, '__closure__', None) or ())
                  if callable(cell.cell_contents) and hasattr(cell.cell_contents, 'config_edits')]
        if not nested:
            break
        fn = nested[0]
    raise ReferenceError(f'{spec["case_id"]}: no registered lambda config edits')


def module_of(spec):
    fn = spec['fn']
    while hasattr(fn, 'func'):
        fn = fn.func
    return fn.__module__


def reference_path(root, cell, module):
    return Path(root) / cell / f'{module}.jsonl'


def load_rows(root, cell, module):
    """All rows of one case module and cell, keyed by case ID; {} when the file is absent."""
    path = reference_path(root, cell, module)
    if not path.is_file():
        return {}
    rows = {}
    for number, line in enumerate(path.read_text().splitlines(), 1):
        try:
            row = json.loads(line)
            rows[row['case']] = row
        except (ValueError, KeyError, TypeError):
            raise ReferenceError(f'{path}:{number}: not a reference row') from None
    return rows


def write_rows(root, cell, module, new_rows):
    """Merge NEW_ROWS (case -> row) into the module's file, sorted by case ID."""
    rows = load_rows(root, cell, module)
    rows.update(new_rows)
    path = reference_path(root, cell, module)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(''.join(canonical(rows[name]) + '\n' for name in sorted(rows)))


def data_free_cases(root, modules):
    """The cases whose tkrzw reference says they read no package data, from the tkrzw files.

    KC and BDB run these only. A case without a tkrzw reference is not named here: it fails
    on the tkrzw cell, where its reference is missing."""
    free = set()
    for module in modules:
        free.update(name for name, row in load_rows(root, 'tkrzw', module).items()
                    if row['identity']['system_data']['kind'] == SYNTHETIC)
    return free


def check_row(row, spec, cell, data_digest, NO_ABORT):
    """Raise ReferenceError, naming the field, when ROW is not valid for this case and run."""
    case = spec['case_id']
    if not isinstance(row, dict) or row.get('schema') != SCHEMA:
        raise ReferenceError(f'{case}: reference has schema {row.get("schema") if isinstance(row, dict) else None}, '
                             f'this driver reads schema {SCHEMA}')
    identity = row.get('identity') or {}
    expected = {'case': case, 'cell': cell}
    for key, value in expected.items():
        if row.get(key) != value:
            raise ReferenceError(f'{case}: reference {key} is {row.get(key)!r}, this run is {value!r}')
    if identity.get('inputs') != inputs_digest(spec, NO_ABORT):
        raise ReferenceError(f'{case}: the case changed since its reference was captured (inputs digest differs); '
                             'capture it again')
    if identity.get('pin', {}).get('commit') != PIN_COMMIT:
        raise ReferenceError(f'{case}: reference is for pin {identity.get("pin", {}).get("commit")}, not {PIN_COMMIT}')
    if identity.get('backend') != cell:
        raise ReferenceError(f'{case}: reference backend is {identity.get("backend")!r}, this run is {cell!r}')
    data = identity.get('system_data') or {}
    if data.get('kind') not in (PACKAGE, SYNTHETIC):
        raise ReferenceError(f'{case}: reference has no system-data identity')
    if data['kind'] == PACKAGE and data.get('digest') != data_digest:
        raise ReferenceError(f'{case}: reference was captured on system data {data.get("digest")}, '
                             f'this run uses {data_digest}')
    if not identity.get('environment', {}).get('image'):
        raise ReferenceError(f'{case}: reference does not name its capture image')
    spelling = scanner_spelling(spec)
    if spelling is not None:
        scanner = row.get('scanner_verdict')
        if (not isinstance(scanner, dict) or set(scanner) != {'input', 'accepted', 'value_bits'}
                or scanner.get('input') != spelling
                or type(scanner.get('accepted')) is not bool
                or (scanner['accepted'] and (not isinstance(scanner['value_bits'], str)
                                             or not re.fullmatch(r'[0-9a-f]{8}', scanner['value_bits'])))
                or (not scanner['accepted'] and scanner.get('value_bits') is not None)):
            raise ReferenceError(f'{case}: reference has no valid capture scanner verdict')
    if spec.get('dir_chunk'):
        directory = identity.get('directory_read_verdict')
        if (not isinstance(directory, dict) or set(directory) != {'size_at_least_header', 'reads'}
                or type(directory['size_at_least_header']) is not bool or not isinstance(directory['reads'], list)
                or (not directory['size_at_least_header'] and directory['reads'])
                or (directory['size_at_least_header'] and not 1 <= len(directory['reads']) <= 2)
                or any(not isinstance(read, dict) or set(read) != {'count', 'errno'}
                       or type(read['count']) is not int or not -1 <= read['count'] <= 4
                       or (read['count'] == -1 and type(read['errno']) is not int)
                       or (read['count'] != -1 and read['errno'] is not None)
                       for read in directory['reads'])):
            raise ReferenceError(f'{case}: reference has no directory_read_verdict')
    pin = row.get('pin')
    if not isinstance(pin, dict) or set(pin) != set(OBSERVATION_KEYS):
        raise ReferenceError(f'{case}: reference holds no complete pin observation')
    if spec.get('pin_not_run') is not None:
        if row.get('provenance') != 'pin_not_run' or pin['exit'] != 'NOT-RUN':
            raise ReferenceError(f'{case}: a pin_not_run case needs a pin_not_run reference')
    elif row.get('provenance') != 'pin' or pin['exit'] == 'NOT-RUN':
        raise ReferenceError(f'{case}: reference does not hold a pin observation')
    if case in REVERSE_CROSS_OPEN_CASES and (not isinstance(row.get('profile'), dict) or not row['profile']):
        raise ReferenceError(f'{case}: reference has no handoff profile')
    if case in ('lazy-user-bigram-pinyin', 'lazy-user-bigram-zhuyin') and (
            not isinstance(row.get('profile'), dict)
            or set(row['profile']) != {'empty.db', 'trained.db'}):
        raise ReferenceError(f'{case}: reference has no saved bigram snapshots')


def lookup(root, cell, spec, data_digest, NO_ABORT, rows=None):
    """The valid reference row of one case, or ReferenceError saying why there is none."""
    module = module_of(spec)
    if rows is None:
        rows = load_rows(root, cell, module)
    path = reference_path(root, cell, module)
    if spec['case_id'] not in rows:
        raise ReferenceError(f'{spec["case_id"]}: no reference in {path.relative_to(ROOT) if path.is_relative_to(ROOT) else path}; '
                             'capture it with tools/oracle/capture-contract.sh')
    row = rows[spec['case_id']]
    check_row(row, spec, cell, data_digest, NO_ABORT)
    return row


def file_sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def comparable(spec, capture, observed, fatal_stderr=lambda text: text):
    """What two captures of one case must agree on: exit, the compared result, stderr, userfiles.

    The user-directory listing counts only for a case that compares it (`userdir`), and a handoff
    profile never: a tkrzw container carries a clock field."""
    observation = capture[0]
    result = observation['result']
    stderr = fatal_stderr(observation['stderr']) if spec['fatal'] else observation['stderr']
    return canonical(dict(exit=observation['exit'], stderr=stderr,
                          result=observed(result) if isinstance(result, dict) else result,
                          userfiles=observation['userfiles'] if spec['userdir'] else None))


class Store:
    """The references of one cell, as `verify` reads them."""

    def __init__(self, root, cell, data_digest, no_abort):
        self.root, self.cell, self.data_digest, self.no_abort = Path(root), cell, data_digest, no_abort
        self.modules = {}

    def row(self, spec):
        module = module_of(spec)
        if module not in self.modules:
            self.modules[module] = load_rows(self.root, self.cell, module)
        return lookup(self.root, self.cell, spec, self.data_digest, self.no_abort, self.modules[module])


def captured_cases(root, cell):
    """Case IDs that have a reference row for CELL.

    Transitional: while the families migrate, the unselected `verify` suite is the cases
    captured so far. The last commit of the migration makes it every registered case."""
    directory = Path(root) / cell
    names = []
    if directory.is_dir():
        for path in sorted(directory.glob('*.jsonl')):
            names.extend(load_rows(root, cell, path.stem))
    return names


