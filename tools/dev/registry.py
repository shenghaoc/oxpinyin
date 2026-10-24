"""Read-only access to tools/bisection/runners.json. Standard library only.

The registry remains the only catalog. This module validates the shape it
reads and never writes, rewrites or derives a second catalog.
"""
import json
from pathlib import Path, PurePosixPath
import re

ROOT = Path(__file__).resolve().parents[2]
REGISTRY = ROOT / 'tools/bisection/runners.json'
COLUMNS = ('name', 'kind', 'cells', 'heavy')
IMPLEMENTATION_FORMS = ('legacy', 'python', 'instrument')


class RegistryError(ValueError):
    """Indicate an unreadable catalog, invalid row, or invalid runner selection."""
    pass


def load(path=None):
    """Return registry rows in file order after validating their shape and commands.

    Read the checkout catalog unless path is supplied. Raise RegistryError
    for read/JSON errors, duplicate names, or invalid implementation metadata.
    """
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
        _check_implementation(row)
    return rows


def _check_implementation(row):
    """`implementation` names what runs the row: `legacy:<path>`,
    `python:<module>` or `instrument:<path>`, each resolving inside the checkout."""
    name = row['name']
    value = row.get('implementation')
    if not isinstance(value, str) or not value:
        raise RegistryError(f'registry row {name!r} has no implementation')
    form, sep, target = value.partition(':')
    if not sep or form not in IMPLEMENTATION_FORMS or not target:
        raise RegistryError(f'registry row {name!r} implementation {value!r} is not '
                            'legacy:<path>, python:<module> or instrument:<path>')
    if form == 'python':
        if not all(part.isidentifier() for part in target.split('.')):
            raise RegistryError(f'registry row {name!r} implementation {value!r} is not a module name')
        base = ROOT.joinpath(*target.split('.'))
        if base.with_suffix('.py').is_file():
            file = base.with_suffix('.py')
        elif (base / '__init__.py').is_file():
            file = base / '__main__.py'
            if not file.is_file():
                raise RegistryError(f'registry row {name!r} implementation {value!r} is a package '
                                    'without __main__.py')
        else:
            file = None
        command = row['command']
        runs = any(part == '-m' and command[index + 1:index + 2] == [target]
                   for index, part in enumerate(command))
    else:
        path = PurePosixPath(target)
        if path.is_absolute() or '..' in path.parts or '\\' in target:
            raise RegistryError(f'registry row {name!r} implementation {value!r} must be a '
                                'checkout-relative path')
        file = ROOT.joinpath(*path.parts)
        file = file if file.is_file() else None
        runs = path in {_command_path(part) for part in row['command'] if isinstance(part, str)}
    if file is None:
        raise RegistryError(f'registry row {name!r} implementation {value!r} does not exist')
    if not file.resolve().is_relative_to(ROOT.resolve()):
        raise RegistryError(f'registry row {name!r} implementation {value!r} resolves outside '
                            'the checkout')
    if not runs:
        raise RegistryError(f'registry row {name!r} implementation {value!r} does not match '
                            f'its command {json.dumps(row["command"])}')


def _command_path(part):
    """A command element as a checkout-relative path: drop `{root}/` or the
    checkout's absolute path; PurePosixPath drops `./` components."""
    for prefix in ('{root}/', ROOT.as_posix() + '/'):
        if part.startswith(prefix):
            return PurePosixPath(part[len(prefix):])
    return PurePosixPath(part)


def by_name(rows):
    """Index rows by name, keeping the last row if names repeat."""
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
    """Normalize issue selectors by trimming, lowercasing, and canonicalizing row tags.

    Remove leading # characters and rewrite an initial row plus spaces or
    colons to row:; numeric issue text otherwise stays unchanged.
    """
    return re.sub(r'^row[ :]*', 'row:', value.strip().lstrip('#').lower())


def select(rows, names=None, cells=None, issues=None, kind=None, require_selection=False):
    """Return rows matching all supplied filter categories, preserving input order.

    Values within one category are alternatives; issue tags are normalized.
    Raise RegistryError for empty or unknown selectors, no matches, or no
    selector when require_selection is true.
    """
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
