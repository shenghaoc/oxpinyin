"""Record policy-accepted observations into a discrepancy record (`just policy-record`).

One command over contract-diff.py's `--observations` captures: for each
captured case, or a `--cases` selection of them, it writes or replaces the
case's entry for the captured cell in its JSON file, in the compact form
tools/bisection/contract_policy.py defines (the
SHA-256 of each side's complete stable observation, a digest per field and
the divergence as text), and creates the file when it does not exist. A
record that still holds complete observations (schema 1) is converted with
`--migrate`. Nothing is spliced by hand.
"""
from . import cell
import argparse
import json
import sys
from pathlib import Path

from tools.bisection import contract_policy, contract_verdict

CELLS = cell.CELLS
REQUIRED = ('cell', 'case', 'userdir', 'abort', 'pin', 'subject')


class RecordError(ValueError):
    """A capture or record this command refuses to write."""


def observations(paths, cases=None):
    """Yield (where, row) for each capture row, or those of the selected cases."""
    wanted = None if cases is None else set(cases)
    for path in paths:
        with Path(path).open(encoding='utf-8') as lines:
            for number, line in enumerate(lines, 1):
                if not line.strip():
                    continue
                try:
                    row = json.loads(line)
                except ValueError as error:
                    raise RecordError(f'{path}:{number}: not JSON ({error})') from None
                if not isinstance(row, dict) or any(key not in row for key in REQUIRED):
                    raise RecordError(f'{path}:{number}: a capture row needs {", ".join(REQUIRED)}; '
                                      'a capture from before the userdir and abort fields must be taken again')
                if wanted is None or row['case'] in wanted:
                    yield f'{path}:{number}', row


def entry_of(where, row):
    """The record entry of one capture row; RecordError for what cannot be recorded."""
    if row['cell'] not in CELLS:
        raise RecordError(f'{where}: unknown cell {row["cell"]!r}')
    if type(row['userdir']) is not bool or type(row['abort']) is not bool:
        raise RecordError(f'{where}: userdir and abort must be booleans')
    if (row['abort'] or 'fatal' in row) and type(row.get('fatal')) is not bool:
        raise RecordError(f'{where}: a registered termination needs a boolean fatal field; '
                          'an older capture must be taken again to distinguish SIGABRT from SIGTRAP')
    sides = {}
    for side in contract_policy.SIDES:
        try:
            sides[side] = contract_policy.stable_observation(row[side], row['userdir'])
        except (KeyError, TypeError):
            raise RecordError(f'{where}: the {side} observation is incomplete') from None
        problem = contract_policy.observation_problem(sides[side], pin_abort=side == 'pin' and row['abort'],
                                                      pin_fatal=side == 'pin' and row.get('fatal', False))
        if problem:
            raise RecordError(f'{where}: {side} {problem}')
    try:
        return contract_policy.entry(sides['pin'], sides['subject'])
    except ValueError as error:
        raise RecordError(f'{where}: {error}') from None


def load_record(path):
    """Load a JSON record, or initialize one for a new findings record."""
    if not path.exists():
        return dict(schema=contract_policy.SCHEMA, record=path.stem, status='current',
                    disposition='POLICY-ACCEPTED', cases={})
    try:
        record = json.loads(path.read_text(encoding='utf-8'))
    except (OSError, ValueError) as error:
        raise RecordError(f'{path}: cannot read policy JSON ({error})') from None
    if (not isinstance(record, dict) or record.get('record') != path.stem or record.get('status') != 'current'
            or record.get('disposition') != 'POLICY-ACCEPTED' or not isinstance(record.get('cases'), dict)
            or record.get('schema') not in (1, contract_policy.SCHEMA)):
        raise RecordError(f'{path}: not a current POLICY-ACCEPTED record named {path.stem}')
    return record


def migrate(record):
    """Convert a schema-1 record's complete observations to entries; return what was converted."""
    if record.get('schema') != 1:
        return []
    converted = []
    for case, kept in record['cases'].items():
        try:
            if isinstance(kept, dict) and set(kept) == {'cells'}:
                for cell, pair in kept['cells'].items():
                    kept['cells'][cell] = contract_policy.entry(pair['pin'], pair['subject'])
                    converted.append(f'{case} [{cell}]')
            else:
                record['cases'][case] = contract_policy.entry(kept['pin'], kept['subject'])
                converted.append(case)
        except (KeyError, TypeError, ValueError) as error:
            raise RecordError(f'{case}: cannot convert the recorded observations ({error})') from None
    record['schema'] = contract_policy.SCHEMA
    return converted


def render(record):
    return contract_policy.render(record, indent=2, sort_keys=True)


def write(path, record):
    """Write only the machine evidence."""
    block = render(record)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(block + '\n', encoding='utf-8')
    return block


def registered(path):
    register = path.parent / 'upstream-divergences.md'
    return register.is_file() and f']({path.name})' in register.read_text(encoding='utf-8')


def record(path, captures, cases=None, *, migrate_schema_1=False, out=print):
    """Write the selected captures into the record; return the names recorded."""
    if not path.is_file() or path.suffix != '.md':
        raise RecordError(f'{path}: findings Markdown record does not exist')
    evidence = contract_verdict.policy_json_path(path.parent, path.name)
    kept = load_record(evidence)
    converted = migrate(kept) if migrate_schema_1 else []
    if kept.get('schema') == 1:
        raise RecordError(f'{path}: schema 1 holds complete observations; pass --migrate to convert it')
    for name in converted:
        out(f'converted {name}')
    wanted = None if cases is None else list(dict.fromkeys(cases))
    recorded = []
    for where, row in observations(captures, wanted):
        entry = entry_of(where, row)
        case, cell = row['case'], row['cell']
        current = kept['cases'].get(case)
        if not (isinstance(current, dict) and set(current) == {'cells'}):
            if current is not None:
                out(f'notice: {case} had one entry for every cell; only the cells recorded now remain')
            kept['cases'][case] = {'cells': {}}
        kept['cases'][case]['cells'][cell] = entry
        label = cell
        recorded.append(f'{case} [{cell}]')
        out(f'recorded {case} [{label}]: pin {entry["pin"]["sha256"][:8]} subject {entry["subject"]["sha256"][:8]}, '
            f'{len(entry["divergence"])} divergence line(s)')
    if wanted is not None:
        missing = [name for name in wanted if not any(label.startswith(name + ' [') for label in recorded)]
        if missing:
            raise RecordError(f'no capture row for {", ".join(missing)}')
    if not recorded and not converted:
        raise RecordError('nothing to record: the captures hold no selected case')
    block = write(evidence, kept)
    out(f'{evidence}: {evidence.stat().st_size} bytes, record {block.count(chr(10)) + 1} lines')
    if not registered(path):
        out(f'notice: {path.name} is not linked from upstream-divergences.md; the verdict reads it as INVALID until it is')
    return recorded


def main(argv=None):
    parser = argparse.ArgumentParser(prog='python -m tools.dev policy-record', description=__doc__.split('\n\n')[1])
    parser.add_argument('record', type=Path, help='the findings record, docs/findings/<name>.md')
    parser.add_argument('captures', type=Path, nargs='*', help='contract-diff.py --observations files')
    parser.add_argument('--cases', help='comma-separated case IDs to record; default: every captured case')
    parser.add_argument('--migrate', action='store_true',
                        help='convert a schema-1 record, which holds complete observations, before recording')
    args = parser.parse_args(argv)
    if not args.captures and not args.migrate:
        parser.error('at least one capture, or --migrate')
    cases = [name for name in args.cases.split(',') if name] if args.cases else None
    try:
        record(args.record, args.captures, cases, migrate_schema_1=args.migrate)
    except RecordError as error:
        print(f'FAIL: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
