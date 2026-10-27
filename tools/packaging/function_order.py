#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Refuse shipping measurements unless the linked ELF has its policy prefix.

Inspect the unstripped object before copying/stripping or starting workers.
The current compiler driver's linker is diagnostic only; the object's layout,
not a build message or a capability probe, establishes ordering. No files are
written. Policy validation and Rust identities reuse the packaging resolver
and generator, including v0 instantiating crates and compiler clone families.

The Mach-O functions derive Apple's -order_file from the same ELF policy and
check the linked dylib's __TEXT,__text prefix with the same rule
(docs/packaging.md, "Mach-O order file"). The policy's identities are
target-independent; the raw Mach-O names they resolve to are not.
"""
import argparse
from collections import defaultdict
import json
import os
from pathlib import Path
import re
import runpy

HERE = Path(__file__).resolve().parent
GEN = runpy.run_path(str(HERE.parent / 'generate-function-order.py'))


def launcher(run=None):
    """RUN, a captured process adapter, or this tool's own pool launcher."""
    if run is not None:
        return run
    from tools.dev import pool, slot
    return pool.UnitContext('function-order', slot.CANCELLED,
                            environment={**os.environ, 'LC_ALL': 'C'}).launch


def resolve(mode, archive=None, policy=None, soname=None, *, run=None):
    """Run the existing nm/AWK resolver; keep policy semantics in one place.

    Return (status, stdout, stderr), including nm's pipefail status. The
    pool feeds the nm bytes to AWK on stdin, as the shell pipe did.
    RUN is a captured process adapter; packaging supplies its pool launcher.
    """
    from tools.dev import pool
    launch = launcher(run)
    arguments = ['awk', '-v', f'mode={mode}']
    if mode != 'inventory':
        arguments += ['-v', f'policy={policy}', '-v', f'soname={soname}']
    arguments += ['-f', str(HERE / 'function-order.awk')]
    if mode == 'policy':
        return launch([*arguments, str(policy)])
    nm = launch(['nm', '--format=sysv', '--defined-only', '--no-sort', '--demangle=rust', str(archive)])
    done = launch([*arguments, *([] if mode == 'inventory' else [str(policy)]), '-'], input=nm.stdout)
    output = done.stdout
    if mode == 'inventory':
        output = b''.join(line + b'\n' for line in sorted(set(output.splitlines())))
    return pool.Completed(done.status or nm.status, output, nm.stderr + done.stderr)


def supported(compiler, archive, scratch, *, run):
    """Probe the actual compiler driver; gold and LLD never imply BFD semantics."""
    base = [str(compiler), '-shared', '-o', str(Path(scratch) / 'probe'), '-Xlinker']
    version = run([*base, '--version', str(archive)])
    text = version.stdout.decode(errors='replace')
    if version.status or not re.search(r'(^|\n)GNU ld \(', text) or 'gold' in text or 'LLD' in text:
        return False
    help_text = run([*base, '--help', str(archive)])
    return not help_text.status and bool(re.search(r'--section-ordering-file([=\s]|$)',
                                                   help_text.stdout.decode(errors='replace')))


def command(arguments):
    from tools.dev import pool, slot
    context = pool.UnitContext('function-order', slot.CANCELLED,
                               environment={**os.environ, 'LC_ALL': 'C'})
    result = context.launch(list(map(str, arguments)))
    if result.status:
        raise ValueError(f'{arguments[0]}: {result.stderr.decode().strip()}')
    return result.stdout.decode()


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


def library_backend(library):
    """Select the policy from the linked object's backend, not its caller's env."""
    needed = re.findall(r'\(NEEDED\).*\[([^]]+)\]', command(['readelf', '-dW', library]))
    patterns = {'bdb': r'libdb(?:-[0-9.]+)?\.so(?:\.[0-9.]+)?',
                'kc': r'libkyotocabinet\.so(?:\.[0-9.]+)?',
                'tkrzw': r'libtkrzw\.so(?:\.[0-9.]+)?'}
    backends = [backend for name in needed for backend, pattern in patterns.items()
                if re.fullmatch(pattern, name)]
    if len(backends) != 1:
        raise ValueError('expected exactly one backend in DT_NEEDED: ' + ', '.join(needed))
    return backends[0]


def check_policy_backend(policy, backend):
    """Reject a foreign backend's layout entry, including through a symlink."""
    directory = policy.resolve().parent
    if (directory.parent.name == 'function-order' and directory.name in ('bdb', 'kc', 'tkrzw')
            and directory.name != backend):
        raise ValueError(f'policy backend {directory.name} does not match {backend}')


def verify(library, policy, facade, *, policy_reader=resolve):
    """Require every live policy family, in policy order, at the start of .text.

    Multiple bodies for one path must form one contiguous group. Aliases at
    the same address may satisfy adjacent policy paths. Unselected CRT/code
    before or between policy groups, missing Rust or C paths and ambiguous
    order fail. A C++ path the object lacks is skipped, as the relink skips
    it (generate-function-order.py, cxx_mangled). A stripped object cannot
    prove this property and fails closed.
    """
    soname = f'lib{facade}.so.15'
    dynamic = command(['readelf', '-dW', library])
    if re.findall(r'\(SONAME\).*\[([^]]+)\]', dynamic) != [soname]:
        raise ValueError(f'expected SONAME {soname}')
    done = policy_reader('policy', policy=policy, soname=soname)
    if done.status:
        raise ValueError(f'awk: {done.stderr.decode().strip()}')
    names = done.stdout.decode().splitlines()
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
    entries = []
    for original, symbol in zip(raw, decoded, strict=True):
        if original[1:] != symbol[1:]:
            raise ValueError('raw/demangled symbol-table records disagree')
        if symbol[6] == '.text':
            entries.append((GEN['stable_identity'](symbol[0], original[0]), int(original[1], 16),
                            GEN['section_family'](original[0])))
    present = {name for name, _, _ in entries}
    names = [name for name in names if name in present or not GEN['cxx_mangled'](name)]
    return prefix_layout(names, entries, start)


def prefix_layout(names, entries, start, section='.text'):
    """Require NAMES, in order, as contiguous groups from the start of SECTION.

    ENTRIES are (name, address, family) for every function in the section;
    a name outside NAMES is an unselected function, retained to detect
    interleaving. One policy name must map to one family of bodies.
    """
    positions = {name: index for index, name in enumerate(names)}
    groups = defaultdict(set)
    families = defaultdict(set)
    found = set()
    for name, address, family in entries:
        index = positions.get(name)
        groups[address]  # retain unselected functions to detect interleaving
        if index is not None:
            groups[address].add(index)
            found.add(index)
            families[index].add(family)
    missing = [name for index, name in enumerate(names) if index not in found]
    if missing:
        raise ValueError(f'{len(missing)} policy paths missing (or stripped): {missing[0]}')
    for index, bodies in families.items():
        if len(bodies) != 1:
            raise ValueError('ambiguous policy path: ' + names[index])
    addresses = sorted(groups)
    if not addresses or addresses[0] != start or 0 not in groups[addresses[0]]:
        raise ValueError(f'first policy function is not at the start of {section}')
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


# Mach-O (macOS). Apple's linker lays out atoms, one per function, and
# -order_file names them by symbol. Every C-level Mach-O name carries a leading
# underscore, and a Rust name's crate disambiguators and hashes differ between
# targets, so the policy (target-independent demangled identities) is resolved
# against the archive this build links. llvm-nm supplies both views: the
# darwin format carries the section, the demangled BSD format the identity.
# --no-llvm-bc keeps nm on the Mach-O symbol tables; the archive's
# compiler-builtins members embed bitcode from rustc's own LLVM, which Apple's
# reader rejects.
MACHO_NM = ('nm', '--no-sort', '--defined-only', '--no-llvm-bc')
MACHO_TEXT = '__TEXT,__text'
MACHO_RECORD = re.compile(r'([0-9a-fA-F]+) \(([^()\s]+)\) (?:\S.*\s)?(\S+)')
MACHO_BSD_RECORD = re.compile(r'([0-9a-fA-F]+) ([A-Za-z]) (.+)')


def macho_identity(raw, demangled):
    """The stable identity of one Mach-O symbol, as the ELF resolver forms it, or None.

    The leading underscore is Mach-O's, not the name's. A Rust name resolves
    through llvm-nm's demangling plus the instantiating crate decoded from the
    raw name; a Rust name nm could not demangle has no identity. A C++ (Itanium)
    name is its own raw path; a C name is itself. A compiler clone's suffix
    (".1482", which llvm-nm renders as " (.1482)") marks a body of the same
    family, not another identity, as the ELF section family does.
    """
    bare = GEN['section_family'](raw[1:] if raw.startswith('_') else raw)
    demangled = re.sub(r' \(\.[^()]*\)$', '', demangled)
    if GEN['cxx_mangled'](bare):
        return bare
    if GEN['rust_mangled'](bare):
        if demangled == raw:
            return None
        return GEN['stable_identity'](demangled, bare)
    return GEN['stable_identity'](bare, bare)


def macho_records(text):
    """(raw name, address, section) per defined symbol of llvm-nm's darwin format.

    Archive member headers and blank lines carry no symbol. The name is the
    last field; the visibility words and annotations between the section and
    the name ("private external", "[cold func]", "(was a private external)")
    are not identity.
    """
    records = []
    for line in text.splitlines():
        match = MACHO_RECORD.fullmatch(line)
        if match:
            address, section, name = match.groups()
            records.append((name, int(address, 16), section))
    return records


def macho_symbols(darwin, demangled):
    """Join one file's darwin-format and demangled BSD records, in symbol-table order.

    Return (identity, raw, address, section) per defined symbol. Both nm runs
    list the same symbols; a count or address disagreement is an error, as the
    ELF reader's is.
    """
    decoded = [match.groups() for match in map(MACHO_BSD_RECORD.fullmatch, demangled.splitlines()) if match]
    records = macho_records(darwin)
    if len(records) != len(decoded):
        raise ValueError('raw/demangled symbol-table lengths differ')
    symbols = []
    for (raw, address, section), (value, _, name) in zip(records, decoded, strict=True):
        if int(value, 16) != address:
            raise ValueError('raw/demangled symbol-table records disagree')
        symbols.append((macho_identity(raw, name), raw, address, section))
    return symbols


def macho_order(names, symbols):
    """Resolve policy identities to raw __TEXT,__text symbol names, in policy order.

    Return (ordered raw names, unresolved identities). A policy path this
    build lacks is reported, not refused: the policy was generated from the
    ELF build, and the targets' function sets differ. Every body of one
    clone family is ordered, in symbol-table order; an identity with bodies
    of two families is ambiguous, as on ELF.
    """
    bodies = defaultdict(dict)
    for identity, raw, _, section in symbols:
        if identity is not None and section == MACHO_TEXT:
            bodies[identity].setdefault(GEN['section_family'](raw), []).append(raw)
    ordered, unresolved, seen = [], [], set()
    for name in names:
        families = bodies.get(name, {})
        if not families:
            unresolved.append(name)
            continue
        if len(families) != 1:
            raise ValueError('ambiguous policy path: ' + name)
        for raw in next(iter(families.values())):
            if raw not in seen:
                seen.add(raw)
                ordered.append(raw)
    return ordered, unresolved


def macho_tables(path, run=None, *, demangle=True):
    """llvm-nm's darwin-format text of PATH, and its demangled BSD text when asked."""
    launch = launcher(run)
    texts = []
    for flags in (('--format=darwin',), ('--demangle',))[:1 + demangle]:
        done = launch([*MACHO_NM, *flags, str(path)])
        if done.status:
            raise ValueError(f'nm: {done.stderr.decode(errors="replace").strip()}')
        texts.append(done.stdout.decode(errors='surrogateescape'))
    return texts


def macho_order_file(policy, archive, soname, *, run=None):
    """Apple -order_file text for POLICY (an ELF policy of SONAME) against a Mach-O ARCHIVE.

    Return (order-file bytes, policy paths, ordered raw names, unresolved
    paths). The policy is validated by the AWK resolver's policy mode; every
    resolved path keeps its position, with every body of its clone family.
    """
    done = resolve('policy', policy=policy, soname=soname, run=run)
    if done.status:
        raise ValueError(f'awk: {done.stderr.decode(errors="replace").strip()}')
    names = done.stdout.decode().splitlines()
    ordered, unresolved = macho_order(names, macho_symbols(*macho_tables(archive, run)))
    if not ordered:
        raise ValueError('no policy path resolves against the Mach-O archive')
    lines = [f'# oxpinyin function order: {Path(policy).name} ({soname}) resolved against {Path(archive).name}',
             *ordered]
    return ''.join(line + '\n' for line in lines).encode(), names, ordered, unresolved


def macho_text_start(text):
    """The address of the one __TEXT,__text section in otool -l output."""
    starts = re.findall(r'sectname __text\s*\n\s*segname __TEXT\s*\n\s*addr (0x[0-9a-fA-F]+)', text)
    if len(starts) != 1:
        raise ValueError('expected one Mach-O __TEXT,__text section')
    return int(starts[0], 16)


def verify_macho(library, ordered, *, run=None):
    """Require ORDERED, in order, at the start of the linked dylib's __TEXT,__text.

    ORDERED was resolved against the archive; a name the linked object lacks
    was dead-stripped and is returned, not counted. The rest must form the
    section's prefix in policy order with no unselected function between
    them, as the ELF check requires. Return (layout, stripped names).
    """
    launch = launcher(run)
    (darwin,) = macho_tables(library, launch, demangle=False)
    entries = [(raw, address, raw) for raw, address, section in macho_records(darwin) if section == MACHO_TEXT]
    done = launch(['otool', '-l', str(library)])
    if done.status:
        raise ValueError(f'otool: {done.stderr.decode(errors="replace").strip()}')
    start = macho_text_start(done.stdout.decode(errors='surrogateescape'))
    present = {raw for raw, _, _ in entries}
    stripped = [raw for raw in ordered if raw not in present]
    names = [raw for raw in ordered if raw in present]
    if not names:
        raise ValueError('no ordered function is in the linked object')
    return prefix_layout(names, entries, start, MACHO_TEXT), stripped


def add_arguments(parser):
    parser.add_argument('--function-order', action='append', default=[], metavar='ARM/FACADE=POLICY',
                        help="expected policy override; default follows the object's DT_NEEDED backend")
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
                    try:
                        backend = library_backend(library)
                        row['backend'] = backend
                        policy = policies.get(f'{arm}/{facade}',
                                              HERE / 'function-order' / backend / f'lib{facade}.order')
                        row['policy'] = str(policy)
                        check_policy_backend(policy, backend)
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


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--arm', help='arm name for unqualified FACADE=OBJECT inputs')
    parser.add_argument('--reference-arm', action='append', default=[])
    parser.add_argument('--library', action='append', required=True, metavar='ARM/FACADE=OBJECT')
    add_arguments(parser)
    args = parser.parse_args(argv)
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
