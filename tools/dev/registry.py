"""Read-only access to tools/bisection/runners.json. Standard library only.

The registry remains the only catalog. This module validates the shape it
reads and never writes, rewrites or derives a second catalog.
"""
import json
from pathlib import Path

REGISTRY = Path(__file__).resolve().parents[1] / 'bisection/runners.json'
COLUMNS = ('name', 'kind', 'cells', 'heavy')


class RegistryError(ValueError):
    pass


def load(path=None):
    """Return the registry rows in file order, after checking their shape."""
    path = Path(path) if path is not None else REGISTRY
    try:
        rows = json.loads(path.read_text())
    except (OSError, ValueError) as exc:
        raise RegistryError(f'cannot read registry {path}: {exc}') from exc
    if not isinstance(rows, list):
        raise RegistryError(f'registry {path} must be a JSON list')
    names = set()
    for row in rows:
        if not isinstance(row, dict) or not isinstance(row.get('name'), str) or not row['name']:
            raise RegistryError(f'registry {path} has a row without a name')
        if row['name'] in names:
            raise RegistryError(f'registry {path} names {row["name"]!r} twice')
        names.add(row['name'])
        if not isinstance(row.get('command'), list) or not row['command']:
            raise RegistryError(f'registry row {row["name"]!r} has no command')
        if not isinstance(row.get('cells'), list) or not row['cells']:
            raise RegistryError(f'registry row {row["name"]!r} has no cells')
    return rows


def by_name(rows):
    return {row['name']: row for row in rows}


def is_heavy(row):
    """A registry row is heavy when it says so (`"heavy": true`); the default
    is light. The cell build is always heavy (cargo decides how much it
    compiles; a no-op build holds the slot for a fraction of a second)."""
    return bool(row.get('heavy'))


def select(rows, names=None, cells=None):
    """Select rows by runner name and cell, preserving registry order."""
    wanted = None if not names else {name for name in names if name}
    chosen = []
    for row in rows:
        if wanted is not None and row['name'] not in wanted:
            continue
        if cells is not None and not (set(cells) & set(row['cells'])):
            continue
        chosen.append(row)
    if wanted is not None:
        missing = sorted(wanted - {row['name'] for row in rows})
        if missing:
            raise RegistryError('unknown runner(s): ' + ', '.join(missing))
    return chosen
