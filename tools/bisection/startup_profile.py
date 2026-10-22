"""The startup-profile manifest, shared by its writer and its readers. Standard library only.

Schema `oxpinyin-startup-profile-v1`
(docs/findings/tooling-consolidation-python-just.md section 7.4): a header
line, then one line per regular file, `size<TAB>sha256<TAB>relative path`,
in path-component order. tools/bisection/teardown.py's provisioner writes
it as `profile.manifest`. tools/generate-function-order.py and
tools/measure-function-order.py compare it and hash it as the system-data
identity. A symlink or any other non-regular entry is an error.
"""
import hashlib
from pathlib import Path
import re

SCHEMA = 'oxpinyin-startup-profile-v1'
LINE = re.compile(r'([0-9]+)\t([0-9a-f]{64})\t(.+)')


def manifest(directory):
    directory = Path(directory)
    rows = [SCHEMA]

    def walk(parent):
        for path in sorted(parent.iterdir(), key=lambda entry: entry.name):
            if path.is_symlink():
                raise ValueError(f'manifest input contains a symlink: {path}')
            if path.is_dir():
                walk(path)
            elif path.is_file():
                with path.open('rb') as handle:
                    digest = hashlib.file_digest(handle, 'sha256').hexdigest()
                rows.append(f'{path.stat().st_size}\t{digest}\t{path.relative_to(directory).as_posix()}')
            else:
                raise ValueError(f'manifest input is not a regular file: {path}')

    walk(directory)
    return '\n'.join(rows) + '\n'


def identity(directory):
    """SHA-256 of the directory's manifest text: the --system-identity value."""
    return hashlib.sha256(manifest(directory).encode()).hexdigest()


def check(directory, recorded):
    """Raise unless the manifest file `recorded` describes `directory` exactly."""
    text = Path(recorded).read_text()
    lines = text.split('\n')
    if lines[0] != SCHEMA:
        raise ValueError(f'profile manifest schema is not {SCHEMA}: {recorded}')
    if lines[-1] != '':
        raise ValueError(f'profile manifest lacks a final newline: {recorded}')
    entries = {}
    for number, line in enumerate(lines[1:-1], 2):
        match = LINE.fullmatch(line)
        if not match:
            raise ValueError(f'profile manifest line {number} is not size<TAB>sha256<TAB>path: {recorded}')
        entries[match[3]] = match[1], match[2]
    actual = manifest(directory)
    if actual == text:
        return
    found = {path: (size, digest) for size, digest, path in (row.split('\t', 2) for row in actual.split('\n')[1:-1])}
    differences = [*(f'added {path}' for path in found if path not in entries),
                   *(f'removed {path}' for path in entries if path not in found),
                   *(f'changed {path}' for path in found if path in entries and found[path] != entries[path])]
    raise ValueError(f'profile manifest differs from {directory}: '
                     + (', '.join(differences) or 'lines out of order or repeated'))
