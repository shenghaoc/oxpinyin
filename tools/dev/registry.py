"""Read-only access to tools/bisection/runners.json. Standard library only.

The registry remains the only catalog. This module validates the shape it
reads and never writes, rewrites or derives a second catalog.
"""
import json
from pathlib import Path
import re

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
        issues = row.get('issues', [])
        if not isinstance(issues, list) or any(not isinstance(issue, str) for issue in issues):
            raise RegistryError(f'registry row {row["name"]!r} issues must be a list of strings')
    return rows


def by_name(rows):
    return {row['name']: row for row in rows}


def is_heavy(row):
    """A registry row is heavy when it says so (`"heavy": true`); the default
    is light. The cell build is always heavy (cargo decides how much it
    compiles; a no-op build holds the slot for a fraction of a second)."""
    return bool(row.get('heavy'))


def names(value):
    """Parse an explicit comma-separated selector without dropping empty items."""
    values = [item.strip() for item in value.split(',')]
    if not all(values):
        raise RegistryError('selection must not contain an empty value')
    return values


def normal_issue(value):
    return re.sub(r'^row[ :]*', 'row:', value.strip().lstrip('#').lower())


def select(rows, names=None, cells=None, issues=None, kind=None, require_selection=False):
    """Filter static rows only. Explicit empty or unknown selectors always fail."""
    selectors = {'runner': names, 'cell': cells, 'issue': issues,
                 'kind': None if kind is None else [kind]}
    if require_selection and all(value is None for value in selectors.values()):
        raise RegistryError('plan needs an explicit selection (for example --runners bisect)')
    known = {'runner': {row['name'] for row in rows},
             'cell': {cell for row in rows for cell in row['cells']},
             'issue': {normal_issue(issue) for row in rows for issue in row.get('issues', [])},
             'kind': {row.get('kind', '') for row in rows}}
    for label, values in selectors.items():
        if values is None:
            continue
        if not values or any(not value.strip() for value in values):
            raise RegistryError(f'empty {label} selection')
        normalized = {normal_issue(value) if label == 'issue' else value for value in values}
        missing = sorted(normalized - known[label])
        if missing:
            raise RegistryError(f'unknown {label}(s): ' + ', '.join(missing))
    wanted = None if names is None else set(names)
    wanted_issues = None if issues is None else {normal_issue(value) for value in issues}
    chosen = []
    for row in rows:
        if wanted is not None and row['name'] not in wanted:
            continue
        if cells is not None and not (set(cells) & set(row['cells'])):
            continue
        if wanted_issues is not None and not wanted_issues.intersection(
                normal_issue(issue) for issue in row.get('issues', [])):
            continue
        if kind is not None and row.get('kind') != kind:
            continue
        chosen.append(row)
    if not chosen:
        raise RegistryError('selection matched no registry rows')
    return chosen
