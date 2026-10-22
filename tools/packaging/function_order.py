#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal
"""Refuse shipping measurements unless the linked ELF has its policy prefix.

Inspect the unstripped object before copying/stripping or starting workers.
The current compiler driver's linker is diagnostic only; the object's layout,
not a build message or a capability probe, establishes ordering. No files are
written. Policy validation and Rust identities reuse the packaging resolver
and generator, including v0 instantiating crates and compiler clone families.
"""
import argparse
from collections import defaultdict
import json
import os
from pathlib import Path
import re
import runpy
import subprocess

HERE = Path(__file__).resolve().parent
GEN = runpy.run_path(str(HERE.parent / 'generate-function-order.py'))


def command(arguments):
    result = subprocess.run(list(map(str, arguments)), capture_output=True, text=True,
                            env={**os.environ, 'LC_ALL': 'C'}, check=False)
    if result.returncode:
        raise ValueError(f'{arguments[0]}: {result.stderr.strip()}')
    return result.stdout


def linker_description(library):
    compiler = os.environ.get('CC', 'cc')
    try:
        version = command([compiler, '-shared', '-o', os.devnull, '-Xlinker', '--version', library])
        lines = version.splitlines()
        version = next((line for line in lines if re.match(r'GNU (ld|gold) |LLD ', line)),
                       lines[0] if lines else 'unknown')
    except (OSError, ValueError) as error:
        version = f'unknown ({error})'
    return f'current driver {compiler}: {version}; artifact linker not recorded'


def verify(library, policy, facade):
    """Require every live policy family, in policy order, at the start of .text.

    Multiple bodies for one path must form one contiguous group. Aliases at
    the same address may satisfy adjacent policy paths. Unselected CRT/code
    before or between policy groups, missing paths and ambiguous order fail.
    A stripped object cannot prove this property and fails closed.
    """
    soname = f'lib{facade}.so.15'
    dynamic = command(['readelf', '-dW', library])
    if re.findall(r'\(SONAME\).*\[([^]]+)\]', dynamic) != [soname]:
        raise ValueError(f'expected SONAME {soname}')
    names = command(['bash', HERE / 'function-order.sh', 'policy', policy, soname]).splitlines()
    positions = {name: index for index, name in enumerate(names)}
    sections = command(['readelf', '-SW', library])
    starts = re.findall(r'\[\s*\d+\]\s+\.text\s+PROGBITS\s+([0-9a-fA-F]+)\s', sections)
    if len(starts) != 1:
        raise ValueError('expected one ELF .text section')
    start = int(starts[0], 16)
    # Unsorted paired reads preserve symbol-table order even for same-address
    # aliases whose raw and demangled names sort differently. Sort by address
    # only after joining, as nm --numeric-sort would for distinct addresses.
    nm = ['nm', '--format=sysv', '--defined-only', '--no-sort']
    raw = GEN['symbols'](command([*nm, '--no-demangle', library]))
    decoded = GEN['symbols'](command([*nm, '--demangle=rust', library]))
    if len(raw) != len(decoded):
        raise ValueError('raw/demangled symbol-table lengths differ')
    groups = defaultdict(set)
    families = defaultdict(set)
    found = set()
    for original, symbol in zip(raw, decoded, strict=True):
        if original[1:] != symbol[1:]:
            raise ValueError('raw/demangled symbol-table records disagree')
        if symbol[6] != '.text':
            continue
        name = GEN['stable_identity'](symbol[0], original[0])
        index = positions.get(name)
        address = int(original[1], 16)
        groups[address]  # retain unselected functions to detect interleaving
        if index is not None:
            groups[address].add(index)
            found.add(index)
            families[index].add(GEN['section_family'](original[0]))
    missing = [name for index, name in enumerate(names) if index not in found]
    if missing:
        raise ValueError(f'{len(missing)} policy paths missing (or stripped): {missing[0]}')
    for index, bodies in families.items():
        if len(bodies) != 1:
            raise ValueError('ambiguous policy path: ' + names[index])
    addresses = sorted(groups)
    if not addresses or addresses[0] != start or 0 not in groups[addresses[0]]:
        raise ValueError('first policy function is not at the start of .text')
    previous = 0
    ended = False
    for address in addresses:
        indices = groups[address]
        if not indices:
            if previous == len(names) - 1:
                ended = True
                continue
            raise ValueError(f'unselected function interrupts policy prefix at {address:#x}')
        if ended:
            raise ValueError(f'policy clone lies outside the prefix at {address:#x}')
        low, high = min(indices), max(indices)
        if low not in (previous, previous + 1) or indices != set(range(low, high + 1)):
            raise ValueError(f'policy functions out of order at {address:#x}: {names[low]}')
        previous = high
    return dict(paths=len(names), text_start=hex(start))


def add_arguments(parser):
    parser.add_argument('--function-order', action='append', default=[], metavar='ARM/FACADE=POLICY',
                        help='expected policy override; default is the checked-in facade policy')
    parser.add_argument('--unordered-arm', action='append', default=[], metavar='ARM',
                        help='explicit non-shipping original-layout experiment (never a shipping result)')


def check_arms(args, arms, references=()):
    """Print a result for every arm/facade before any measurements start."""
    if len(set(references)) != len(references) or set(references) - set(arms):
        raise ValueError('unknown or repeated reference arm')
    unordered = getattr(args, 'unordered_arm', [])
    if len(set(unordered)) != len(unordered) or set(unordered) - (set(arms) - set(references)):
        raise ValueError('--unordered-arm needs unique non-reference arm names: ' + ','.join(arms))
    policies = {}
    allowed = {f'{arm}/{facade}' for arm, libraries in arms.items() if arm not in references
               for facade in libraries}
    for override in getattr(args, 'function_order', []):
        key, separator, value = override.partition('=')
        if not separator or not value or key not in allowed or key in policies or key.split('/')[0] in unordered:
            raise ValueError('invalid, repeated or unordered --function-order: ' + override)
        policies[key] = Path(value)
    results = {}
    failures = []
    for arm, libraries in arms.items():
        results[arm] = {}
        for facade, library in libraries.items():
            library = Path(library)
            row = dict(arm=arm, facade=facade, object=str(library))
            if arm in references:
                row['function_order'] = 'reference'
            else:
                row['linker'] = linker_description(library)
                if arm in unordered:
                    row['function_order'] = 'intentional-unordered'
                else:
                    policy = policies.get(f'{arm}/{facade}', HERE / 'function-order' / f'lib{facade}.order')
                    row['policy'] = str(policy)
                    try:
                        row.update(verify(library, policy, facade), function_order='ordered')
                    except (OSError, ValueError) as error:
                        row.update(function_order='refused', reason=str(error))
                        failures.append(f'function ordering: arm {arm}/{facade} refused: {error}; '
                                        f'{row["linker"]}. Use GNU BFD ld >=2.43 with an applicable policy, '
                                        'or explicitly name an intentional --unordered-arm.')
            print(json.dumps(row), flush=True)
            results[arm][facade] = row
    if failures:
        raise ValueError('\n'.join(failures))
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--arm', help='arm name for unqualified FACADE=OBJECT inputs')
    parser.add_argument('--reference-arm', action='append', default=[])
    parser.add_argument('--library', action='append', required=True, metavar='ARM/FACADE=OBJECT')
    add_arguments(parser)
    args = parser.parse_args()
    arms = {}
    for value in args.library:
        key, separator, path = value.partition('=')
        arm, slash, facade = key.partition('/')
        if not slash:
            arm, facade = args.arm, key
        if not separator or not arm or facade not in ('pinyin', 'zhuyin') or not path:
            parser.error('expected --library ARM/pinyin|zhuyin=OBJECT')
        libraries = arms.setdefault(arm, {})
        if facade in libraries:
            parser.error('repeated library: ' + key)
        libraries[facade] = Path(path)
    try:
        check_arms(args, arms, args.reference_arm)
    except (OSError, ValueError) as error:
        parser.exit(1, str(error) + '\n')


if __name__ == '__main__':
    main()
